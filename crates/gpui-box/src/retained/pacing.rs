//! Absolute-clock visual sampling. State transactions and lifecycle commands are
//! never discarded by this clock; only obsolete animation samples are skipped.
use std::time::{Duration, Instant};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InvalidFrameRate;

/// A rational target rate, independent of display refresh and render backend.
/// A 120 Hz work deadline does not prove a 120 Hz visible presentation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FrameRate {
    numerator: u32,
    denominator: u32,
}
impl FrameRate {
    pub fn new(numerator: u32, denominator: u32) -> Result<Self, InvalidFrameRate> {
        if numerator == 0
            || denominator == 0
            || u64::from(numerator) > 1000 * u64::from(denominator)
        {
            return Err(InvalidFrameRate);
        }
        Ok(Self {
            numerator,
            denominator,
        })
    }
    pub fn hz(hz: u32) -> Result<Self, InvalidFrameRate> {
        Self::new(hz, 1)
    }
    pub fn budget(self) -> Duration {
        self.offset(1)
    }
    fn offset(self, tick: u64) -> Duration {
        let nanos = u128::from(tick) * 1_000_000_000 * u128::from(self.denominator)
            / u128::from(self.numerator);
        let Ok(seconds) = u64::try_from(nanos / 1_000_000_000) else {
            return Duration::MAX;
        };
        Duration::new(seconds, (nanos % 1_000_000_000) as u32)
    }
}

#[derive(Clone, Copy, Debug)]
pub struct FrameTick {
    pub index: u64,
    pub scheduled_at: Instant,
    pub deadline: Instant,
    pub superseded_samples: u64,
}

/// Latest-sample animation clock. Rational offsets avoid cumulative drift.
/// Hosts arrange a wake for `next_sample_at`; the clock creates no timer/thread.
pub struct FrameClock {
    origin: Instant,
    rate: FrameRate,
    next: u64,
}
impl FrameClock {
    pub fn new(origin: Instant, rate: FrameRate) -> Self {
        Self {
            origin,
            rate,
            next: 0,
        }
    }
    pub fn next_sample_at(&self) -> Option<Instant> {
        self.origin.checked_add(self.rate.offset(self.next))
    }
    pub fn sample(&mut self, now: Instant) -> Option<FrameTick> {
        if now < self.next_sample_at()? {
            return None;
        }
        let elapsed = now.checked_duration_since(self.origin)?.as_nanos();
        // Invert the floored nanosecond offset. At 120 Hz tick 1 is
        // 8_333_333 ns; ordinary integer division would still report tick 0.
        let index = (((elapsed + 1) * u128::from(self.rate.numerator) - 1)
            / (1_000_000_000 * u128::from(self.rate.denominator)))
        .min(u128::from(u64::MAX)) as u64;
        let index = index.max(self.next);
        let next = index.checked_add(1)?;
        let tick = FrameTick {
            index,
            scheduled_at: self.origin.checked_add(self.rate.offset(index))?,
            deadline: self.origin.checked_add(self.rate.offset(next))?,
            superseded_samples: index - self.next,
        };
        self.next = next;
        Some(tick)
    }
}

/// Rendering policy is separate from scene semantics. Software must prioritize
/// responsiveness explicitly; callers decide which decorative effects may be
/// simplified. Text, group opacity, clip, layout and hit geometry stay semantic.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RenderPreference {
    DesktopGpu {
        target: FrameRate,
    },
    SoftwareResponsive {
        target: FrameRate,
        effects: SoftwareEffects,
    },
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SoftwareEffects {
    /// Request exact supported effects, allowing the host to lower cadence.
    Exact,
    /// The application permits omission of decorative box shadows. This never
    /// authorizes changing clip, isolated opacity, text, or interaction geometry.
    OmitDecorativeShadows,
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rational_clock_skips_obsolete_visual_samples_without_accumulating_drift() {
        let origin = Instant::now();
        let mut clock = FrameClock::new(origin, FrameRate::hz(120).unwrap());
        assert_eq!(clock.sample(origin).unwrap().index, 0);
        assert!(clock.sample(origin).is_none());
        let late = clock.sample(origin + Duration::from_millis(50)).unwrap();
        assert_eq!((late.index, late.superseded_samples), (6, 5));
        assert_eq!(late.scheduled_at, origin + Duration::from_millis(50));
        let rate = FrameRate::new(60000, 1001).unwrap();
        let mut clock = FrameClock::new(origin, rate);
        let tick = clock.sample(origin + Duration::from_secs(1001)).unwrap();
        assert_eq!(tick.index, 60000);
        assert_eq!(tick.scheduled_at, origin + Duration::from_secs(1001));
        assert!(FrameRate::hz(0).is_err());
        assert!(FrameRate::hz(1001).is_err());
    }
    #[test]
    fn rounded_boundaries_select_latest_due_tick_and_large_offsets_do_not_wrap() {
        let origin = Instant::now();
        let rate = FrameRate::hz(120).unwrap();
        let mut clock = FrameClock::new(origin, rate);
        let tick = clock.sample(origin + rate.offset(7)).unwrap();
        assert_eq!((tick.index, tick.superseded_samples), (7, 7));
        assert!(
            clock
                .sample(origin + rate.offset(8) - Duration::from_nanos(1))
                .is_none()
        );
        assert_eq!(clock.sample(origin + rate.offset(8)).unwrap().index, 8);
        assert_eq!(
            FrameRate::new(1, u32::MAX).unwrap().offset(u64::MAX),
            Duration::MAX
        );
    }
}
