//! Deterministic injectable clocks for schedule admission and reconciliation.
//!
//! Wall-clock milliseconds are the durable trigger domain. Monotonic
//! milliseconds are diagnostic only and are never persisted as wall time. The
//! default system clock reports UTC unless a host installs its own clock.

use crate::timezone::TimeZoneSpec;
use std::{
    sync::{Mutex, OnceLock},
    time::{Instant, SystemTime, UNIX_EPOCH},
};

/// The three observations a scheduler needs. Implementations must be
/// thread-safe; they may be called from a blocking background executor.
pub trait ScheduleClock: Send + Sync + 'static {
    /// Monotonic milliseconds since an arbitrary process-local origin.
    fn monotonic_now_ms(&self) -> u64;
    /// Wall-clock milliseconds since the Unix epoch.
    fn wall_clock_epoch_ms(&self) -> u64;
    /// The active local zone used for calendar schedules that follow the host.
    fn active_time_zone(&self) -> TimeZoneSpec;
}

/// The production clock. Its wall clock is the Unix epoch and its active zone
/// is UTC; hosts that need a local civil zone inject their own clock.
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemScheduleClock;

impl SystemScheduleClock {
    pub const fn new() -> Self {
        Self
    }

    fn origin() -> &'static Instant {
        static ORIGIN: OnceLock<Instant> = OnceLock::new();
        ORIGIN.get_or_init(Instant::now)
    }
}

impl ScheduleClock for SystemScheduleClock {
    fn monotonic_now_ms(&self) -> u64 {
        let elapsed = Instant::now().saturating_duration_since(*Self::origin());
        u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX)
    }

    fn wall_clock_epoch_ms(&self) -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| u64::try_from(duration.as_millis()).unwrap_or(u64::MAX))
            .unwrap_or(0)
    }

    fn active_time_zone(&self) -> TimeZoneSpec {
        TimeZoneSpec::Utc
    }
}

#[derive(Debug, Clone)]
struct FakeClockState {
    monotonic_ms: u64,
    wall_epoch_ms: u64,
    zone: TimeZoneSpec,
}

/// A deterministic clock for tests and headless hosts.
#[derive(Debug, Clone)]
pub struct FakeScheduleClock {
    state: std::sync::Arc<Mutex<FakeClockState>>,
}

impl FakeScheduleClock {
    pub fn new(wall_epoch_ms: u64) -> Self {
        Self {
            state: std::sync::Arc::new(Mutex::new(FakeClockState {
                monotonic_ms: 1,
                wall_epoch_ms,
                zone: TimeZoneSpec::Utc,
            })),
        }
    }

    pub fn with_zone(self, zone: TimeZoneSpec) -> Self {
        lock(&self.state).zone = zone;
        self
    }

    pub fn set_wall_clock_epoch_ms(&self, wall_epoch_ms: u64) {
        lock(&self.state).wall_epoch_ms = wall_epoch_ms;
    }

    pub fn advance_wall_clock_ms(&self, delta_ms: u64) {
        let mut state = lock(&self.state);
        state.wall_epoch_ms = state.wall_epoch_ms.saturating_add(delta_ms);
        state.monotonic_ms = state.monotonic_ms.saturating_add(delta_ms);
    }

    pub fn advance_monotonic_ms(&self, delta_ms: u64) {
        let mut state = lock(&self.state);
        state.monotonic_ms = state.monotonic_ms.saturating_add(delta_ms);
    }

    pub fn set_active_time_zone(&self, zone: TimeZoneSpec) {
        lock(&self.state).zone = zone;
    }

    pub fn wall_clock_epoch_ms_now(&self) -> u64 {
        lock(&self.state).wall_epoch_ms
    }

    pub fn monotonic_ms_now(&self) -> u64 {
        lock(&self.state).monotonic_ms
    }
}

impl ScheduleClock for FakeScheduleClock {
    fn monotonic_now_ms(&self) -> u64 {
        lock(&self.state).monotonic_ms
    }

    fn wall_clock_epoch_ms(&self) -> u64 {
        lock(&self.state).wall_epoch_ms
    }

    fn active_time_zone(&self) -> TimeZoneSpec {
        lock(&self.state).zone.clone()
    }
}

pub(crate) fn lock<T>(value: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    value
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fake_clock_is_deterministic_and_poison_safe() {
        let clock =
            FakeScheduleClock::new(1_000_000).with_zone(TimeZoneSpec::FixedOffsetMinutes(330));
        assert_eq!(clock.wall_clock_epoch_ms(), 1_000_000);
        clock.advance_wall_clock_ms(5_000);
        assert_eq!(clock.wall_clock_epoch_ms(), 1_005_000);
        assert_eq!(clock.monotonic_now_ms(), 5_001);
        assert_eq!(
            clock.active_time_zone(),
            TimeZoneSpec::FixedOffsetMinutes(330)
        );
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _guard = lock(&clock.state);
            panic!("poison fake clock");
        }));
        assert_eq!(clock.wall_clock_epoch_ms(), 1_005_000);
    }

    #[test]
    fn system_clock_is_monotonic_across_calls() {
        let clock = SystemScheduleClock::new();
        let first = clock.monotonic_now_ms();
        let second = clock.monotonic_now_ms();
        assert!(second >= first);
        assert!(clock.wall_clock_epoch_ms() > 1_600_000_000_000);
    }
}
