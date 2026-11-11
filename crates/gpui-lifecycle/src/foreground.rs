use crate::app::AppPhase;
use gpui_system::SurfaceId;
use std::{
    collections::HashMap,
    time::{Duration, Instant, SystemTime},
};

/// Stable identity of the process that produced an interval.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ProcessId(pub u32);

/// Identity of one application session within a process.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SessionId(pub u64);

/// One merged foreground interval.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VisibilityInterval {
    /// Monotonic start.
    pub start: Instant,
    /// Monotonic end.
    pub end: Instant,
}

impl VisibilityInterval {
    /// Elapsed monotonic duration.
    pub fn duration(self) -> Duration {
        self.end.saturating_duration_since(self.start)
    }
}

/// The union of all foreground-visible surface intervals.
///
/// Overlapping windows contribute once. The union opens when the application is
/// foreground and at least one surface is visible, and closes when either
/// condition stops holding, so elapsed time is never double counted across
/// windows and background surfaces never accrue foreground time.
#[derive(Debug, Clone, Default)]
pub struct VisibilityUnion {
    intervals: Vec<VisibilityInterval>,
    open_since: Option<Instant>,
}

impl VisibilityUnion {
    /// Opens a foreground interval. Returns false when one is already open.
    pub fn open(&mut self, at: Instant) -> bool {
        if self.open_since.is_some() {
            return false;
        }
        self.open_since = Some(at);
        true
    }

    /// Closes the open foreground interval. Returns false when none was open.
    pub fn close(&mut self, at: Instant) -> bool {
        let Some(start) = self.open_since.take() else {
            return false;
        };
        let end = at.max(start);
        let interval = VisibilityInterval { start, end };
        self.push_interval(interval);
        true
    }

    /// Whether a foreground interval is currently open.
    pub fn is_open(&self) -> bool {
        self.open_since.is_some()
    }

    /// Merged closed intervals.
    pub fn intervals(&self) -> &[VisibilityInterval] {
        &self.intervals
    }

    /// The monotonic instant the current interval opened.
    pub fn open_since(&self) -> Option<Instant> {
        self.open_since
    }

    /// Total active duration, including the interval still open at `now`.
    pub fn total_duration(&self, now: Instant) -> Duration {
        let closed = self.intervals.iter().map(|interval| interval.duration());
        let mut total: Duration = closed.sum();
        if let Some(start) = self.open_since {
            total += now.saturating_duration_since(start);
        }
        total
    }

    fn push_interval(&mut self, interval: VisibilityInterval) {
        let merged = self.intervals.last_mut().is_some_and(|last| {
            let overlaps = interval.start <= last.end;
            if overlaps {
                last.end = last.end.max(interval.end);
            }
            overlaps
        });
        if !merged {
            self.intervals.push(interval);
        }
    }
}

/// The most recent user input observation.
///
/// This is deliberately separate from platform idle measurement and from
/// foreground intervals: input does not extend or shorten visibility time.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct LastInputActivity {
    last: Option<Instant>,
    events: u64,
}

impl LastInputActivity {
    /// Records input. Older timestamps are ignored, preserving monotonicity.
    pub fn record_at(&mut self, at: Instant) -> bool {
        if self.last.is_some_and(|last| at < last) {
            return false;
        }
        self.last = Some(at);
        self.events = self.events.saturating_add(1);
        true
    }

    /// The last accepted input instant.
    pub fn last_at(&self) -> Option<Instant> {
        self.last
    }

    /// Monotonic time since the last accepted input.
    pub fn elapsed(&self, now: Instant) -> Option<Duration> {
        self.last.map(|last| now.saturating_duration_since(last))
    }

    /// Number of accepted input observations.
    pub fn event_count(&self) -> u64 {
        self.events
    }
}

/// Platform-measured idle state, which the core never fabricates.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum IdleState {
    /// The user interacted recently, according to the platform.
    Active,
    /// The platform measured idleness starting at a monotonic instant.
    Idle {
        /// When idleness began.
        since: Instant,
    },
    /// The platform provides no idle signal.
    #[default]
    Unavailable,
}

impl IdleState {
    /// The platform-reported idle start, if any.
    pub fn idle_since(self) -> Option<Instant> {
        match self {
            Self::Idle { since } => Some(since),
            _ => None,
        }
    }

    /// Whether the platform reports recent activity.
    pub fn is_active(self) -> bool {
        matches!(self, Self::Active)
    }
}

/// Monotonic foreground-use accounting for this application.
///
/// Wall-clock observations are retained for diagnostics only. Every elapsed
/// duration is computed from [`Instant`], so `SystemTime` jumps cannot corrupt
/// active elapsed time.
#[derive(Debug, Clone)]
pub struct ForegroundUseTracker {
    process: ProcessId,
    session: SessionId,
    phase: AppPhase,
    visible: HashMap<SurfaceId, Instant>,
    union: VisibilityUnion,
    last_input: LastInputActivity,
    idle: IdleState,
    last_wall_clock: Option<SystemTime>,
}

impl ForegroundUseTracker {
    /// Builds an idle tracker for one process session.
    pub fn new(process: ProcessId, session: SessionId) -> Self {
        Self {
            process,
            session,
            phase: AppPhase::Launching,
            visible: HashMap::new(),
            union: VisibilityUnion::default(),
            last_input: LastInputActivity::default(),
            idle: IdleState::default(),
            last_wall_clock: None,
        }
    }

    /// The owning process identity.
    pub fn process_id(&self) -> ProcessId {
        self.process
    }

    /// The owning session identity.
    pub fn session_id(&self) -> SessionId {
        self.session
    }

    /// The last application phase.
    pub fn phase(&self) -> AppPhase {
        self.phase
    }

    /// The visibility union.
    pub fn visibility(&self) -> &VisibilityUnion {
        &self.union
    }

    /// Applies an application phase.
    ///
    /// Entering a foreground phase opens a fresh interval when at least one
    /// surface is already visible. Leaving foreground closes the interval but
    /// retains every logically visible surface for the next resume.
    pub fn set_phase(&mut self, phase: AppPhase, now: Instant) -> bool {
        let changed = self.phase != phase;
        self.phase = phase;
        if !phase.is_foreground() {
            self.union.close(now);
        } else if !self.visible.is_empty() {
            self.union.open(now);
        }
        changed
    }

    /// Marks one surface visible or hidden.
    ///
    /// A visible surface is retained even while the application is not
    /// foreground, but it does not open a foreground interval until the next
    /// foreground phase.
    pub fn set_surface_visible(
        &mut self,
        surface: SurfaceId,
        is_visible: bool,
        now: Instant,
    ) -> bool {
        if is_visible {
            if self.visible.contains_key(&surface) {
                return false;
            }
            self.visible.insert(surface, now);
            if self.phase.is_foreground() {
                self.union.open(now);
            }
            true
        } else {
            if self.visible.remove(&surface).is_none() {
                return false;
            }
            if self.visible.is_empty() {
                self.union.close(now);
            }
            true
        }
    }

    /// Whether one surface is currently logically visible.
    pub fn is_surface_visible(&self, surface: SurfaceId) -> bool {
        self.visible.contains_key(&surface)
    }

    /// Number of logically visible surfaces.
    pub fn visible_count(&self) -> usize {
        self.visible.len()
    }

    /// Logically visible surfaces in unspecified order.
    pub fn visible_surfaces(&self) -> impl Iterator<Item = SurfaceId> + '_ {
        self.visible.keys().copied()
    }

    /// The live union duration through `now`.
    pub fn total_active_duration(&self, now: Instant) -> Duration {
        self.union.total_duration(now)
    }

    /// Records a user input observation.
    pub fn record_input_at(&mut self, at: Instant) -> bool {
        self.last_input.record_at(at)
    }

    /// The last accepted input instant.
    pub fn last_input_at(&self) -> Option<Instant> {
        self.last_input.last_at()
    }

    /// Accepted input observation count.
    pub fn input_event_count(&self) -> u64 {
        self.last_input.event_count()
    }

    /// Monotonic time since the last accepted input.
    pub fn elapsed_since_input(&self, now: Instant) -> Option<Duration> {
        self.last_input.elapsed(now)
    }

    /// The platform idle state.
    pub fn idle_state(&self) -> IdleState {
        self.idle
    }

    /// Replaces the platform idle state.
    pub fn set_idle_state(&mut self, idle: IdleState) {
        self.idle = idle;
    }

    /// Records a wall-clock observation for diagnostics only.
    ///
    /// This never participates in `total_active_duration`.
    pub fn record_wall_clock_observation(&mut self, at: SystemTime) {
        self.last_wall_clock = Some(at);
    }

    /// The last wall-clock observation.
    pub fn last_wall_clock(&self) -> Option<SystemTime> {
        self.last_wall_clock
    }
}
