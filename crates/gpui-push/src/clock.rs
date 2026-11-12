//! Injectable monotonic or epoch-stable clock.
//!
//! Push delivery never sleeps and never reads the wall clock directly. Tests
//! advance a [`ManualPushClock`]; hosts may inject an epoch-stable monotonic
//! source so persisted expiry timestamps remain comparable across a restart.

use std::{
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

/// Host-supplied millisecond timestamp.
pub trait PushClock: Send + Sync + 'static {
    fn now_ms(&self) -> u64;
}

/// Process-local deterministic clock for tests and portable hosts.
#[derive(Debug, Default)]
pub struct ManualPushClock {
    now_ms: AtomicU64,
}

impl ManualPushClock {
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

impl PushClock for ManualPushClock {
    fn now_ms(&self) -> u64 {
        self.now_ms.load(Ordering::SeqCst)
    }
}

/// Default host clock. It uses Unix-epoch milliseconds so persisted dedup
/// expiry remains comparable across a process restart. Hosts that need strict
/// monotonicity across wall-clock adjustments should inject their own
/// [`PushClock`].
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemPushClock;

impl SystemPushClock {
    pub const fn new() -> Self {
        Self
    }
}

impl PushClock for SystemPushClock {
    fn now_ms(&self) -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_millis().min(u128::from(u64::MAX)) as u64)
            .unwrap_or(0)
    }
}
