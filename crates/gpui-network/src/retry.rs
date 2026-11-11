//! Bounded integer exponential backoff with deterministic debounce and
//! cooldown behavior. No timers or threads are used.

use crate::{EndpointDiagnostic, PolicyEpoch};
use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};

/// Retry policy. Every delay is integer milliseconds; no floating point or
/// wall-clock time participates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RetryPolicy {
    pub base_delay_ms: u64,
    pub max_delay_ms: u64,
    pub max_attempts: u32,
    pub cooldown: Duration,
    pub debounce_window: Duration,
}

impl RetryPolicy {
    pub const fn new(
        base_delay_ms: u64,
        max_delay_ms: u64,
        max_attempts: u32,
        cooldown: Duration,
        debounce_window: Duration,
    ) -> Self {
        Self {
            base_delay_ms,
            max_delay_ms,
            max_attempts,
            cooldown,
            debounce_window,
        }
    }

    pub fn normalized(self) -> Self {
        Self {
            base_delay_ms: self.base_delay_ms.max(1),
            max_delay_ms: self.max_delay_ms.max(self.base_delay_ms.max(1)),
            ..self
        }
    }
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            base_delay_ms: 250,
            max_delay_ms: 30_000,
            max_attempts: 5,
            cooldown: Duration::from_secs(60),
            debounce_window: Duration::from_millis(250),
        }
    }
}

/// Why a failure is not worth retrying.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RetryBlockReason {
    TlsVerificationFailed,
    ProxyAuthExhausted,
    ProxyAuthenticationRequired,
    CaptivePortalBlocked,
    Offline,
    PermanentHttpStatus(u16),
    Succeeded,
    Debounced,
    InvalidDiagnostic,
}

/// Deterministic retry decision.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RetryDecision {
    RetryAfter {
        attempt: u32,
        delay: Duration,
        execute_at: Instant,
    },
    Exhausted {
        cooldown_until: Instant,
    },
    DoNotRetry {
        reason: RetryBlockReason,
    },
}

impl RetryDecision {
    pub const fn is_retry(&self) -> bool {
        matches!(self, Self::RetryAfter { .. })
    }
}

/// Stateful retry budget for one logical operation.
#[derive(Debug, Clone)]
pub struct RetryTracker {
    policy: RetryPolicy,
    attempts: u32,
    last_trigger: Option<Instant>,
    last_decision: Option<RetryDecision>,
    cooldown_until: Option<Instant>,
    epoch: PolicyEpoch,
}

impl RetryTracker {
    pub fn new(policy: RetryPolicy) -> Self {
        Self {
            policy: policy.normalized(),
            attempts: 0,
            last_trigger: None,
            last_decision: None,
            cooldown_until: None,
            epoch: PolicyEpoch::INITIAL,
        }
    }

    pub fn with_policy(policy: RetryPolicy) -> Self {
        Self::new(policy)
    }

    pub const fn policy(&self) -> RetryPolicy {
        self.policy
    }

    pub fn set_policy(&mut self, policy: RetryPolicy) {
        self.policy = policy.normalized();
        self.reset();
    }

    pub const fn attempts(&self) -> u32 {
        self.attempts
    }

    pub const fn epoch(&self) -> PolicyEpoch {
        self.epoch
    }

    pub fn reset(&mut self) {
        self.attempts = 0;
        self.last_trigger = None;
        self.last_decision = None;
        self.cooldown_until = None;
    }

    /// Reset the retry budget when the policy epoch actually advances.
    pub fn reset_on_epoch_change(&mut self, new_epoch: PolicyEpoch) -> bool {
        if new_epoch <= self.epoch {
            return false;
        }
        self.epoch = new_epoch;
        self.reset();
        true
    }

    /// Coalesce rapid network-change triggers. Returns true when the trigger
    /// is accepted and starts a fresh retry budget; false when it falls inside
    /// the debounce window.
    pub fn on_network_change(&mut self, now: Instant) -> bool {
        if let Some(previous) = self.last_trigger
            && now.saturating_duration_since(previous) < self.policy.debounce_window
        {
            return false;
        }
        self.reset();
        self.last_trigger = Some(now);
        true
    }

    /// Evaluate one diagnostic at a caller-supplied instant.
    pub fn evaluate(&mut self, diagnostic: &EndpointDiagnostic, now: Instant) -> RetryDecision {
        if let Some(cooldown_until) = self.cooldown_until {
            if now < cooldown_until {
                return RetryDecision::Exhausted { cooldown_until };
            }
            self.reset();
        }

        if let Some(reason) = non_retriable_reason(diagnostic) {
            return RetryDecision::DoNotRetry { reason };
        }
        if !diagnostic.is_retriable() {
            return RetryDecision::DoNotRetry {
                reason: RetryBlockReason::InvalidDiagnostic,
            };
        }

        // A repeated evaluation inside the debounce window reuses the last
        // scheduled execution instead of spending another attempt.
        if let (Some(previous), Some(decision)) = (self.last_trigger, self.last_decision)
            && now.saturating_duration_since(previous) < self.policy.debounce_window
            && matches!(decision, RetryDecision::RetryAfter { .. })
        {
            return decision;
        }

        self.attempts = self.attempts.saturating_add(1);
        if self.attempts > self.policy.max_attempts {
            let cooldown_until = now.checked_add(self.policy.cooldown).unwrap_or(now);
            let decision = RetryDecision::Exhausted { cooldown_until };
            self.cooldown_until = Some(cooldown_until);
            self.last_trigger = Some(now);
            self.last_decision = Some(decision);
            return decision;
        }

        let delay_ms = self.delay_for_attempt(self.attempts);
        let delay = Duration::from_millis(delay_ms);
        let execute_at = now.checked_add(delay).unwrap_or(now);
        let decision = RetryDecision::RetryAfter {
            attempt: self.attempts,
            delay,
            execute_at,
        };
        self.last_trigger = Some(now);
        self.last_decision = Some(decision);
        decision
    }

    fn delay_for_attempt(&self, attempt: u32) -> u64 {
        let exponent = attempt.saturating_sub(1);
        let factor = 1u64.checked_shl(exponent).unwrap_or(u64::MAX);
        self.policy
            .base_delay_ms
            .saturating_mul(factor)
            .min(self.policy.max_delay_ms)
    }
}

fn non_retriable_reason(diagnostic: &EndpointDiagnostic) -> Option<RetryBlockReason> {
    match diagnostic {
        EndpointDiagnostic::Offline => Some(RetryBlockReason::Offline),
        EndpointDiagnostic::CaptivePortalBlocked => Some(RetryBlockReason::CaptivePortalBlocked),
        EndpointDiagnostic::ProxyAuthRequired { .. } => {
            Some(RetryBlockReason::ProxyAuthenticationRequired)
        }
        EndpointDiagnostic::ProxyAuthExhausted => Some(RetryBlockReason::ProxyAuthExhausted),
        EndpointDiagnostic::TlsVerificationFailed { .. } => {
            Some(RetryBlockReason::TlsVerificationFailed)
        }
        EndpointDiagnostic::EndpointReachable { .. } => Some(RetryBlockReason::Succeeded),
        EndpointDiagnostic::EndpointHttpUnavailable { status_code } => {
            let status = *status_code;
            let permanent = ((400..=499).contains(&status) && status != 408 && status != 429)
                || (300..=399).contains(&status);
            permanent.then_some(RetryBlockReason::PermanentHttpStatus(status))
        }
        EndpointDiagnostic::DnsFailure { .. }
        | EndpointDiagnostic::TcpConnectTimeout
        | EndpointDiagnostic::ProxyResolutionFailed
        | EndpointDiagnostic::ProxyUnreachable => None,
    }
}
