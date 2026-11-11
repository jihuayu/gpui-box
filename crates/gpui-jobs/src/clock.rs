//! Injectable monotonic clock.
//!
//! Jobs never sleep and never read the wall clock directly. Tests advance a
//! [`ManualClock`]; hosts may inject an epoch-stable monotonic source.

use std::{
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

/// Host-supplied monotonic milliseconds.
pub trait JobsClock: Send + Sync + 'static {
    fn now_ms(&self) -> u64;
}

/// Process-local monotonic clock for tests and deterministic hosts.
#[derive(Debug, Default)]
pub struct ManualClock {
    now_ms: AtomicU64,
}

impl ManualClock {
    pub fn new(start_ms: u64) -> Self {
        Self {
            now_ms: AtomicU64::new(start_ms),
        }
    }

    pub fn now_ms(&self) -> u64 {
        self.now_ms.load(Ordering::SeqCst)
    }

    pub fn set(&self, now_ms: u64) {
        self.now_ms.store(now_ms, Ordering::SeqCst);
    }

    pub fn advance(&self, delta_ms: u64) -> u64 {
        self.now_ms
            .fetch_add(delta_ms, Ordering::SeqCst)
            .saturating_add(delta_ms)
    }

    pub fn advance_to(&self, now_ms: u64) -> u64 {
        self.now_ms.fetch_max(now_ms, Ordering::SeqCst)
    }
}

impl JobsClock for ManualClock {
    fn now_ms(&self) -> u64 {
        Self::now_ms(self)
    }
}

/// Default host clock. It uses Unix-epoch milliseconds so persisted deadlines
/// remain comparable across a process restart. Hosts that need strict
/// monotonicity across wall-clock adjustments should inject their own
/// [`JobsClock`].
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemJobsClock;

impl SystemJobsClock {
    pub const fn new() -> Self {
        Self
    }
}

impl JobsClock for SystemJobsClock {
    fn now_ms(&self) -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_millis().min(u128::from(u64::MAX)) as u64)
            .unwrap_or(0)
    }
}
