use gpui_network::*;
use std::time::{Duration, Instant};

fn policy() -> RetryPolicy {
    RetryPolicy::new(100, 1_000, 3, Duration::from_secs(5), Duration::ZERO)
}

fn dns() -> EndpointDiagnostic {
    EndpointDiagnostic::DnsFailure {
        kind: DnsFailureKind::Timeout,
    }
}

#[test]
fn exponential_backoff_is_integer_bounded_and_deterministic() {
    let start = Instant::now();
    let mut tracker = RetryTracker::new(policy());
    let first = tracker.evaluate(&dns(), start);
    match first {
        RetryDecision::RetryAfter {
            attempt,
            delay,
            execute_at,
        } => {
            assert_eq!(attempt, 1);
            assert_eq!(delay, Duration::from_millis(100));
            assert_eq!(execute_at, start + Duration::from_millis(100));
        }
        _ => panic!("expected retry, got {first:?}"),
    }

    let second = tracker.evaluate(&dns(), start + Duration::from_secs(1));
    assert!(matches!(
        second,
        RetryDecision::RetryAfter {
            attempt: 2,
            delay,
            ..
        } if delay == Duration::from_millis(200)
    ));
    let third = tracker.evaluate(&dns(), start + Duration::from_secs(2));
    assert!(matches!(
        third,
        RetryDecision::RetryAfter {
            attempt: 3,
            delay,
            ..
        } if delay == Duration::from_millis(400)
    ));

    let exhausted = tracker.evaluate(&dns(), start + Duration::from_secs(3));
    let cooldown_until = match exhausted {
        RetryDecision::Exhausted { cooldown_until } => cooldown_until,
        _ => panic!("expected exhaustion, got {exhausted:?}"),
    };
    assert_eq!(cooldown_until, start + Duration::from_secs(8));
    assert_eq!(
        tracker.evaluate(&dns(), start + Duration::from_secs(4)),
        RetryDecision::Exhausted { cooldown_until }
    );
    assert!(matches!(
        tracker.evaluate(&dns(), cooldown_until),
        RetryDecision::RetryAfter { attempt: 1, .. }
    ));
}

#[test]
fn delay_is_capped_and_overflow_safe() {
    let start = Instant::now();
    let capped = RetryPolicy::new(10_000, 25_000, 10, Duration::from_secs(1), Duration::ZERO);
    let mut tracker = RetryTracker::new(capped);
    for _ in 0..6 {
        let _ = tracker.evaluate(&dns(), start);
    }
    let decision = tracker.evaluate(&dns(), start + Duration::from_secs(10));
    assert!(matches!(
        decision,
        RetryDecision::RetryAfter { delay, .. } if delay == Duration::from_millis(25_000)
    ));

    let huge = RetryPolicy::new(
        u64::MAX,
        u64::MAX,
        u32::MAX,
        Duration::from_secs(1),
        Duration::ZERO,
    );
    let mut tracker = RetryTracker::new(huge);
    let decision = tracker.evaluate(&dns(), Instant::now());
    assert!(matches!(decision, RetryDecision::RetryAfter { .. }));
}

#[test]
fn debounce_reuses_the_scheduled_attempt() {
    let start = Instant::now();
    let policy = RetryPolicy::new(
        100,
        1_000,
        3,
        Duration::from_secs(1),
        Duration::from_millis(500),
    );
    let mut tracker = RetryTracker::new(policy);
    let first = tracker.evaluate(&dns(), start);
    let repeated = tracker.evaluate(&dns(), start + Duration::from_millis(100));
    assert_eq!(first, repeated);
    assert_eq!(tracker.attempts(), 1);
    let later = tracker.evaluate(&dns(), start + Duration::from_millis(600));
    assert!(matches!(
        later,
        RetryDecision::RetryAfter { attempt: 2, .. }
    ));
}

#[test]
fn non_retriable_failures_short_circuit_immediately() {
    let start = Instant::now();
    let cases = [
        (
            EndpointDiagnostic::TlsVerificationFailed {
                reason: TlsFailureReason::UntrustedRoot,
            },
            RetryBlockReason::TlsVerificationFailed,
        ),
        (
            EndpointDiagnostic::ProxyAuthExhausted,
            RetryBlockReason::ProxyAuthExhausted,
        ),
        (
            EndpointDiagnostic::ProxyAuthRequired {
                realm: Some("corp".to_owned()),
            },
            RetryBlockReason::ProxyAuthenticationRequired,
        ),
        (
            EndpointDiagnostic::CaptivePortalBlocked,
            RetryBlockReason::CaptivePortalBlocked,
        ),
        (EndpointDiagnostic::Offline, RetryBlockReason::Offline),
        (
            EndpointDiagnostic::EndpointHttpUnavailable { status_code: 404 },
            RetryBlockReason::PermanentHttpStatus(404),
        ),
        (
            EndpointDiagnostic::EndpointHttpUnavailable { status_code: 302 },
            RetryBlockReason::PermanentHttpStatus(302),
        ),
        (
            EndpointDiagnostic::EndpointReachable { latency_ms: 4 },
            RetryBlockReason::Succeeded,
        ),
    ];
    for (diagnostic, reason) in cases {
        let mut tracker = RetryTracker::new(policy());
        assert_eq!(
            tracker.evaluate(&diagnostic, start),
            RetryDecision::DoNotRetry { reason }
        );
        assert_eq!(tracker.attempts(), 0);
    }
}

#[test]
fn transient_failures_and_rate_limits_retry() {
    let start = Instant::now();
    let cases = [
        EndpointDiagnostic::DnsFailure {
            kind: DnsFailureKind::NonExistentDomain,
        },
        EndpointDiagnostic::DnsFailure {
            kind: DnsFailureKind::ServerFailure,
        },
        EndpointDiagnostic::DnsFailure {
            kind: DnsFailureKind::Refused,
        },
        EndpointDiagnostic::DnsFailure {
            kind: DnsFailureKind::NoRecords,
        },
        EndpointDiagnostic::TcpConnectTimeout,
        EndpointDiagnostic::ProxyResolutionFailed,
        EndpointDiagnostic::ProxyUnreachable,
        EndpointDiagnostic::EndpointHttpUnavailable { status_code: 408 },
        EndpointDiagnostic::EndpointHttpUnavailable { status_code: 429 },
        EndpointDiagnostic::EndpointHttpUnavailable { status_code: 500 },
        EndpointDiagnostic::EndpointHttpUnavailable { status_code: 599 },
    ];
    for diagnostic in cases {
        let mut tracker = RetryTracker::new(policy());
        assert!(
            tracker.evaluate(&diagnostic, start).is_retry(),
            "{diagnostic:?}"
        );
    }
}

#[test]
fn network_change_debounce_and_epoch_reset_are_explicit() {
    let start = Instant::now();
    let policy = RetryPolicy::new(
        100,
        1_000,
        3,
        Duration::from_secs(1),
        Duration::from_millis(500),
    );
    let mut tracker = RetryTracker::new(policy);
    assert!(tracker.on_network_change(start));
    assert!(!tracker.on_network_change(start + Duration::from_millis(100)));
    assert!(tracker.on_network_change(start + Duration::from_millis(600)));
    assert_eq!(tracker.attempts(), 0);

    let _ = tracker.evaluate(&dns(), start + Duration::from_secs(1));
    assert_eq!(tracker.attempts(), 1);
    assert!(!tracker.reset_on_epoch_change(PolicyEpoch::INITIAL));
    assert!(tracker.reset_on_epoch_change(PolicyEpoch::INITIAL.next()));
    assert_eq!(tracker.attempts(), 0);
    assert_eq!(tracker.epoch(), PolicyEpoch::INITIAL.next());

    let _ = tracker.evaluate(&dns(), start + Duration::from_secs(2));
    assert_eq!(tracker.attempts(), 1);
    assert!(!tracker.reset_on_epoch_change(PolicyEpoch::INITIAL));
    assert_eq!(
        tracker.attempts(),
        1,
        "a stale epoch must not reset attempts"
    );
    assert_eq!(
        tracker.epoch(),
        PolicyEpoch::INITIAL.next(),
        "a stale epoch must not move the tracker backward"
    );

    tracker.set_policy(RetryPolicy::new(
        1,
        2,
        1,
        Duration::from_secs(1),
        Duration::ZERO,
    ));
    assert_eq!(tracker.policy().base_delay_ms, 1);
    tracker.reset();
    assert_eq!(tracker.attempts(), 0);
}

#[test]
fn zero_attempt_budget_exhausts_without_retrying() {
    let start = Instant::now();
    let mut tracker = RetryTracker::new(RetryPolicy::new(
        10,
        20,
        0,
        Duration::from_secs(3),
        Duration::ZERO,
    ));
    assert_eq!(
        tracker.evaluate(&dns(), start),
        RetryDecision::Exhausted {
            cooldown_until: start + Duration::from_secs(3)
        }
    );
}
