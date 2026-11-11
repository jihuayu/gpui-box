use crate::app::{AppPhase, MemoryPressure, SessionLockState, SurfaceSnapshot};
use gpui_system::{EventEnvelope, EventSender, EventSubscription, SurfaceId};
use std::{
    pin::Pin,
    task::{Context, Poll, Waker},
};

/// Default bounded capacity for revalidation streams.
pub const REVALIDATION_CAPACITY: usize = 16;

/// Default bounded capacity for lifecycle event streams.
pub const LIFECYCLE_CAPACITY: usize = 16;

/// Default bounded capacity for preference event streams.
pub const PREFERENCES_CAPACITY: usize = 8;

/// Coarse network reachability reported by the platform.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum NetworkStatus {
    /// A network path is available.
    Online,
    /// No network path is available.
    Offline,
    /// The platform cannot report reachability.
    #[default]
    Unknown,
}

/// Coherent lifecycle changes delivered to every subscriber.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LifecycleEvent {
    /// The application phase changed.
    PhaseChanged {
        /// The new phase.
        phase: AppPhase,
    },
    /// The device lock state changed.
    SessionLockChanged {
        /// The new lock state.
        lock: SessionLockState,
    },
    /// The platform memory-pressure signal changed.
    MemoryPressureChanged {
        /// The new pressure level.
        pressure: MemoryPressure,
    },
    /// A surface was attached or reported a new snapshot.
    SurfaceChanged {
        /// The surface identity.
        surface: SurfaceId,
        /// The accepted snapshot.
        snapshot: SurfaceSnapshot,
    },
    /// A surface attachment was detached.
    SurfaceDetached {
        /// The detached surface identity.
        surface: SurfaceId,
    },
}

/// Events that ask a query client or screen model to revalidate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RevalidationEvent {
    /// The application resumed; observers must revalidate stale queries.
    AppResumed {
        /// Lifecycle revision at resume time.
        generation: u64,
    },
    /// The network path changed.
    NetworkChanged(NetworkStatus),
}

/// The consumer that owns a scoped revalidation subscription.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum RevalidationScope {
    /// A query cache and its in-flight requests.
    #[default]
    QueryClient,
    /// A screen model derived from cached queries.
    ScreenModel,
}

/// Producer side of a bounded revalidation stream.
#[derive(Clone)]
pub struct RevalidationSender(EventSender<RevalidationEvent>);

impl RevalidationSender {
    /// Publishes one revalidation event.
    pub fn publish(&self, event: RevalidationEvent) -> bool {
        self.0.send(event)
    }

    /// The latest session-local sequence.
    pub fn revision(&self) -> u64 {
        self.0.revision()
    }
}

/// Consumer side with scoped cleanup and overflow detection.
pub struct RevalidationSubscription {
    inner: EventSubscription<RevalidationEvent>,
    scope: RevalidationScope,
}

impl RevalidationSubscription {
    pub(crate) fn from_parts(
        inner: EventSubscription<RevalidationEvent>,
        scope: RevalidationScope,
    ) -> Self {
        Self { inner, scope }
    }

    /// Assigns the owner scope retained for diagnostics.
    pub fn with_scope(mut self, scope: RevalidationScope) -> Self {
        self.scope = scope;
        self
    }

    /// The owning consumer scope.
    pub fn scope(&self) -> RevalidationScope {
        self.scope
    }

    /// Whether overflow discarded events before the caller read them.
    pub fn snapshot_required(&self) -> bool {
        self.inner.snapshot_required()
    }

    /// Acknowledges that a snapshot covered the supplied sequence.
    pub fn acknowledge_snapshot(&self, sequence: u64) {
        self.inner.acknowledge_snapshot(sequence);
    }

    /// Drains every immediately available event without blocking.
    pub fn drain(&self) -> Vec<EventEnvelope<RevalidationEvent>> {
        drain_ready(&self.inner)
    }
}

/// Drains every immediately available event from a bounded stream.
pub fn drain_ready<T>(subscription: &EventSubscription<T>) -> Vec<EventEnvelope<T>> {
    let mut events = Vec::new();
    let waker = Waker::noop();
    let mut context = Context::from_waker(waker);
    while let Ok(mut next) = subscription.try_next() {
        let poll = std::future::Future::poll(Pin::new(&mut next), &mut context);
        match poll {
            Poll::Ready(Some(event)) => events.push(event),
            Poll::Ready(None) | Poll::Pending => break,
        }
    }
    events
}

/// Creates a bounded revalidation stream with drop scoped cleanup.
pub fn revalidation_channel(
    capacity: usize,
    cleanup: impl FnOnce() + 'static,
) -> (RevalidationSender, RevalidationSubscription) {
    let (sender, inner) = gpui_system::event_channel(capacity.max(1), cleanup);
    let subscription = RevalidationSubscription::from_parts(inner, RevalidationScope::QueryClient);
    (RevalidationSender(sender), subscription)
}
