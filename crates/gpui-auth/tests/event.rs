//! Bounded cursor event ring and public-safe event payloads.

mod support;

use gpui_auth::*;
use std::time::Instant;
use support::*;

#[test]
fn default_capacity_and_cursor_drain_are_stable() {
    assert_eq!(DEFAULT_AUTH_EVENT_CAPACITY, 64);
    let harness = Harness::new(AuthPlatform::Linux);
    let now = Instant::now();
    let started = start_login(&harness, 1, 1, 50, None, now);
    let _ = harness
        .service
        .cancel_ceremony(ExecutionOwner::Ui, started.handle.ceremony_id(), now)
        .expect("cancel");
    let cursor = harness.service.subscribe();
    let batch = harness.service.events_since(cursor);
    assert!(batch.resync.is_none());
    assert!(batch.events.iter().any(|event| matches!(
        event,
        AuthEvent::CeremonyLaunched {
            method: AuthMethodKind::BrowserOAuth,
            ..
        }
    )));
    assert!(batch.events.iter().any(|event| matches!(
        event,
        AuthEvent::CeremonyCancelled {
            reason: gpui_system::CancelReason::Caller,
            ..
        }
    )));
    let next = harness.service.events_since(batch.next_cursor);
    assert!(next.events.is_empty());
    assert!(next.resync.is_none());
}

#[test]
fn overflow_reports_dropped_events_and_recovers_with_a_cursor() {
    let harness = Harness::new(AuthPlatform::Linux);
    let now = Instant::now();
    for seed in 0..40u64 {
        let started = start_login(&harness, 1, 1, seed + 100, None, now);
        let _ = harness
            .service
            .cancel_ceremony(ExecutionOwner::Ui, started.handle.ceremony_id(), now)
            .expect("cancel");
    }
    let batch = harness.service.events_since(AuthEventCursor::start());
    let resync = batch.resync.expect("overflow must be reported");
    assert!(resync.dropped_events > 0);
    assert_eq!(batch.events.len(), DEFAULT_AUTH_EVENT_CAPACITY);
    assert_eq!(
        resync.dropped_events,
        80 - DEFAULT_AUTH_EVENT_CAPACITY as u64
    );
    assert_eq!(batch.next_cursor.position(), 80);
    let next = harness.service.events_since(batch.next_cursor);
    assert!(next.events.is_empty());
    assert!(next.resync.is_none());
}

#[test]
fn capacity_one_keeps_the_newest_event() {
    let harness = Harness::with_config(
        AuthPlatform::Linux,
        AuthConfig {
            event_capacity: 1,
            ..AuthConfig::default()
        },
    );
    let now = Instant::now();
    let started = start_login(&harness, 1, 1, 51, None, now);
    let _ = harness
        .service
        .cancel_ceremony(ExecutionOwner::Ui, started.handle.ceremony_id(), now)
        .expect("cancel");
    let batch = harness.service.events_since(AuthEventCursor::start());
    assert_eq!(batch.events.len(), 1);
    assert!(matches!(
        batch.events[0],
        AuthEvent::CeremonyCancelled { .. }
    ));
    assert_eq!(batch.resync.expect("dropped").dropped_events, 1);
}

#[test]
fn events_never_contain_tokens_codes_or_pkce_material() {
    let harness = Harness::new(AuthPlatform::Linux);
    let now = Instant::now();
    let summary = login(&harness, 52, now);
    assert_eq!(summary.account_id().as_str(), "account-1");
    let batch = harness.service.events_since(AuthEventCursor::start());
    let debug = format!("{:?}", batch.events);
    for forbidden in [
        "access-token",
        "refresh-token",
        "id-token",
        "code-52",
        "state-",
        "dBjftJeZ4CVP",
        "pkce",
    ] {
        assert!(!debug.contains(forbidden), "event leaked {forbidden}");
    }
    assert!(
        batch
            .events
            .iter()
            .any(|event| matches!(event, AuthEvent::AccountAuthenticated { .. }))
    );
}
