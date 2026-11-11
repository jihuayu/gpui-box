use crate::{
    app::{
        AppPhase, LifecycleState, MemoryPressure, RejectedCallback, SessionLockState,
        SurfaceSnapshot, SurfaceVisibility,
    },
    events::{
        LIFECYCLE_CAPACITY, LifecycleEvent, NetworkStatus, PREFERENCES_CAPACITY,
        REVALIDATION_CAPACITY, RevalidationEvent, RevalidationScope, RevalidationSubscription,
    },
    foreground::{ForegroundUseTracker, IdleState, ProcessId, SessionId},
    preferences::SystemPreferences,
};
use gpui_system::{
    ErrorCode, EventSender, EventSubscription, Evidence, Platform, ProviderManifest, SurfaceId,
    SystemError, SystemServices,
};
use std::{
    cell::{Cell, RefCell},
    fmt,
    rc::Rc,
    thread::{self, ThreadId},
    time::{Duration, Instant},
};

/// Conservative capability manifest for this contract-only crate.
///
/// No native lifecycle provider is installed by this crate. Platform hosts and
/// their provider issues must publish their own evidence before flipping these
/// flags.
pub const PROVIDER_MANIFEST: [ProviderManifest; 5] = [
    ProviderManifest {
        platform: Platform::Windows,
        native_provider_installed: false,
        evidence: Evidence::ContractOnly,
    },
    ProviderManifest {
        platform: Platform::MacOs,
        native_provider_installed: false,
        evidence: Evidence::ContractOnly,
    },
    ProviderManifest {
        platform: Platform::Linux,
        native_provider_installed: false,
        evidence: Evidence::ContractOnly,
    },
    ProviderManifest {
        platform: Platform::Android,
        native_provider_installed: false,
        evidence: Evidence::ContractOnly,
    },
    ProviderManifest {
        platform: Platform::Ios,
        native_provider_installed: false,
        evidence: Evidence::ContractOnly,
    },
];

/// Upper bound on retained redacted callback rejections.
pub const MAX_REJECTION_RECORDS: usize = 64;

type SenderList<T> = RefCell<Vec<(u64, Rc<Cell<bool>>, Rc<EventSender<T>>)>>;

#[derive(Default)]
struct Publishers {
    lifecycle: SenderList<LifecycleEvent>,
    preferences: SenderList<SystemPreferences>,
    revalidation: SenderList<RevalidationEvent>,
    next_token: Cell<u64>,
}

impl Publishers {
    fn next_token(&self) -> u64 {
        let token = self.next_token.get();
        self.next_token.set(token.wrapping_add(1));
        token
    }

    fn remove_lifecycle(&self, token: u64) {
        remove_sender(&self.lifecycle, token);
    }

    fn remove_preferences(&self, token: u64) {
        remove_sender(&self.preferences, token);
    }

    fn remove_revalidation(&self, token: u64) {
        remove_sender(&self.revalidation, token);
    }
}

#[derive(Debug)]
struct ServiceState {
    lifecycle: LifecycleState,
    tracker: ForegroundUseTracker,
    preferences: SystemPreferences,
    network: NetworkStatus,
    rejections: Vec<RejectedCallback>,
    resume_generation: u64,
}

impl ServiceState {
    fn new(process_id: ProcessId, session_id: SessionId) -> Self {
        Self {
            lifecycle: LifecycleState::default(),
            tracker: ForegroundUseTracker::new(process_id, session_id),
            preferences: SystemPreferences::default(),
            network: NetworkStatus::default(),
            rejections: Vec::new(),
            resume_generation: 0,
        }
    }
}

struct ServiceInner {
    ui_thread: ThreadId,
    state: RefCell<ServiceState>,
    publishers: Publishers,
}

impl fmt::Debug for ServiceInner {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ServiceInner")
            .field("ui_thread", &self.ui_thread)
            .field("state", &self.state)
            .finish_non_exhaustive()
    }
}

/// UI-thread-owned lifecycle observation and foreground accounting.
///
/// The handle is cheap to clone and shares one `Rc<RefCell<..>>` state.
/// [`SystemServices`] stores it as `Rc<LifecycleService>`, so callers holding
/// `&LifecycleService` can query it, subscribe to bounded event streams, and
/// apply host transitions. Every mutation checks the owning thread first.
///
/// All state borrows are released before any event is published, so a
/// synchronous [`std::task::Wake::wake`] callback may re-enter the service
/// without panicking.
#[derive(Clone)]
pub struct LifecycleService {
    inner: Rc<ServiceInner>,
}

impl fmt::Debug for LifecycleService {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("LifecycleService")
            .field("inner", &self.inner)
            .finish()
    }
}

impl LifecycleService {
    /// Builds a service owned by the current thread.
    pub fn new(process_id: ProcessId, session_id: SessionId) -> Self {
        Self {
            inner: Rc::new(ServiceInner {
                ui_thread: thread::current().id(),
                state: RefCell::new(ServiceState::new(process_id, session_id)),
                publishers: Publishers::default(),
            }),
        }
    }

    /// Registers this handle in the typed UI-thread service registry.
    pub fn install(self, services: &mut SystemServices) -> Result<(), SystemError> {
        services.register(self)
    }

    /// Whether the caller runs on the owning UI thread.
    pub fn is_ui_thread(&self) -> bool {
        self.inner.ui_thread == thread::current().id()
    }

    /// Fails when called from a worker thread.
    pub fn require_ui_thread(&self) -> Result<(), SystemError> {
        if self.is_ui_thread() {
            Ok(())
        } else {
            Err(SystemError::new(
                ErrorCode::ContextExpired,
                "Lifecycle service requires its owning UI thread",
            ))
        }
    }

    /// The current application phase.
    pub fn phase(&self) -> AppPhase {
        self.inner.state.borrow().lifecycle.phase()
    }

    /// The current device lock state.
    pub fn session_lock(&self) -> SessionLockState {
        self.inner.state.borrow().lifecycle.session_lock()
    }

    /// The last memory-pressure signal.
    pub fn memory_pressure(&self) -> MemoryPressure {
        self.inner.state.borrow().lifecycle.memory_pressure()
    }

    /// The last host preferences.
    pub fn preferences(&self) -> SystemPreferences {
        self.inner.state.borrow().preferences.clone()
    }

    /// The last network reachability signal.
    pub fn network_status(&self) -> NetworkStatus {
        self.inner.state.borrow().network
    }

    /// The registered snapshot for one surface, if attached.
    pub fn surface(&self, surface: SurfaceId) -> Option<SurfaceSnapshot> {
        self.inner.state.borrow().lifecycle.surface(surface)
    }

    /// Whether a surface is attached and logically visible.
    pub fn is_surface_visible(&self, surface: SurfaceId) -> bool {
        let state = self.inner.state.borrow();
        state.tracker.is_surface_visible(surface)
    }

    /// Redacted records of rejected callbacks.
    pub fn rejections(&self) -> Vec<RejectedCallback> {
        self.inner.state.borrow().rejections.clone()
    }

    /// Monotonic revision of accepted lifecycle state.
    pub fn revision(&self) -> u64 {
        self.inner.state.borrow().lifecycle.revision()
    }

    /// The live union duration at `now`.
    pub fn active_duration(&self, now: Instant) -> Duration {
        self.inner.state.borrow().tracker.total_active_duration(now)
    }

    /// The platform idle state.
    pub fn idle_state(&self) -> IdleState {
        self.inner.state.borrow().tracker.idle_state()
    }

    /// The last wall-clock observation.
    pub fn last_wall_clock(&self) -> Option<std::time::SystemTime> {
        self.inner.state.borrow().tracker.last_wall_clock()
    }

    /// The owning process identity.
    pub fn process_id(&self) -> ProcessId {
        self.inner.state.borrow().tracker.process_id()
    }

    /// The owning session identity.
    pub fn session_id(&self) -> SessionId {
        self.inner.state.borrow().tracker.session_id()
    }

    /// Subscribes to bounded lifecycle changes.
    pub fn subscribe_lifecycle(&self) -> EventSubscription<LifecycleEvent> {
        let token = self.inner.publishers.next_token();
        let active = Rc::new(Cell::new(true));
        let cleanup_active = active.clone();
        let weak = Rc::downgrade(&self.inner);
        let (sender, subscription) = gpui_system::event_channel(LIFECYCLE_CAPACITY, move || {
            cleanup_active.set(false);
            if let Some(inner) = weak.upgrade() {
                inner.publishers.remove_lifecycle(token);
            }
        });
        self.inner
            .publishers
            .lifecycle
            .borrow_mut()
            .push((token, active, Rc::new(sender)));
        subscription
    }

    /// Subscribes to bounded preference changes.
    pub fn subscribe_preferences(&self) -> EventSubscription<SystemPreferences> {
        let token = self.inner.publishers.next_token();
        let active = Rc::new(Cell::new(true));
        let cleanup_active = active.clone();
        let weak = Rc::downgrade(&self.inner);
        let (sender, subscription) = gpui_system::event_channel(PREFERENCES_CAPACITY, move || {
            cleanup_active.set(false);
            if let Some(inner) = weak.upgrade() {
                inner.publishers.remove_preferences(token);
            }
        });
        self.inner
            .publishers
            .preferences
            .borrow_mut()
            .push((token, active, Rc::new(sender)));
        subscription
    }

    /// Subscribes to bounded revalidation events with an owning scope.
    pub fn subscribe_revalidation(&self, scope: RevalidationScope) -> RevalidationSubscription {
        let token = self.inner.publishers.next_token();
        let active = Rc::new(Cell::new(true));
        let cleanup_active = active.clone();
        let weak = Rc::downgrade(&self.inner);
        let (sender, inner) = gpui_system::event_channel(REVALIDATION_CAPACITY, move || {
            cleanup_active.set(false);
            if let Some(inner) = weak.upgrade() {
                inner.publishers.remove_revalidation(token);
            }
        });
        let subscription = RevalidationSubscription::from_parts(inner, scope);
        self.inner
            .publishers
            .revalidation
            .borrow_mut()
            .push((token, active, Rc::new(sender)));
        subscription
    }

    /// Number of attached lifecycle, preference and revalidation subscribers.
    pub fn subscriber_counts(&self) -> (usize, usize, usize) {
        let lifecycle = self.inner.publishers.lifecycle.borrow().len();
        let preferences = self.inner.publishers.preferences.borrow().len();
        let revalidation = self.inner.publishers.revalidation.borrow().len();
        (lifecycle, preferences, revalidation)
    }

    /// Applies an application phase at an explicit monotonic instant.
    ///
    /// Moving from `Background` or `Suspended` into a foreground phase
    /// publishes [`RevalidationEvent::AppResumed`] with a fresh resume
    /// generation.
    pub fn set_phase_at(&self, phase: AppPhase, now: Instant) -> Result<bool, SystemError> {
        self.require_ui_thread()?;
        let (changed, resume) = {
            let mut state = self.inner.state.borrow_mut();
            if state.lifecycle.phase() == phase {
                (false, None)
            } else {
                let previous = state.lifecycle.phase();
                state.lifecycle.set_phase(phase);
                state.tracker.set_phase(phase, now);
                let resume = if phase.is_foreground()
                    && matches!(previous, AppPhase::Background | AppPhase::Suspended)
                {
                    state.resume_generation = state.resume_generation.saturating_add(1);
                    Some(state.resume_generation)
                } else {
                    None
                };
                (true, resume)
            }
        };
        if changed {
            self.publish_lifecycle(LifecycleEvent::PhaseChanged { phase });
            if let Some(generation) = resume {
                self.publish_revalidation(RevalidationEvent::AppResumed { generation });
            }
        }
        Ok(changed)
    }

    /// Replaces host preferences.
    pub fn set_preferences(&self, preferences: SystemPreferences) -> Result<bool, SystemError> {
        self.require_ui_thread()?;
        let (changed, snapshot) = {
            let mut state = self.inner.state.borrow_mut();
            let changed = state.preferences != preferences;
            if changed {
                state.preferences = preferences;
                (true, Some(state.preferences.clone()))
            } else {
                (false, None)
            }
        };
        if let Some(snapshot) = snapshot {
            self.publish_preferences(snapshot);
        }
        Ok(changed)
    }

    /// Replaces the network reachability signal.
    pub fn set_network_status(&self, status: NetworkStatus) -> Result<bool, SystemError> {
        self.require_ui_thread()?;
        let changed = {
            let mut state = self.inner.state.borrow_mut();
            let changed = state.network != status;
            state.network = status;
            changed
        };
        if changed {
            self.publish_revalidation(RevalidationEvent::NetworkChanged(status));
        }
        Ok(changed)
    }

    /// Applies device lock state.
    pub fn set_session_lock(&self, lock: SessionLockState) -> Result<bool, SystemError> {
        self.require_ui_thread()?;
        let changed = {
            let mut state = self.inner.state.borrow_mut();
            if state.lifecycle.session_lock() == lock {
                false
            } else {
                state.lifecycle.set_session_lock(lock);
                true
            }
        };
        if changed {
            self.publish_lifecycle(LifecycleEvent::SessionLockChanged { lock });
        }
        Ok(changed)
    }

    /// Applies a memory-pressure signal.
    pub fn set_memory_pressure(&self, pressure: MemoryPressure) -> Result<bool, SystemError> {
        self.require_ui_thread()?;
        let changed = {
            let mut state = self.inner.state.borrow_mut();
            if state.lifecycle.memory_pressure() == pressure {
                false
            } else {
                state.lifecycle.set_memory_pressure(pressure);
                true
            }
        };
        if changed {
            self.publish_lifecycle(LifecycleEvent::MemoryPressureChanged { pressure });
        }
        Ok(changed)
    }

    /// Registers a surface attachment generation.
    ///
    /// Returns false for generation zero and for generations that do not
    /// advance beyond the currently registered generation.
    pub fn register_surface(
        &self,
        surface: SurfaceId,
        generation: u64,
    ) -> Result<bool, SystemError> {
        self.require_ui_thread()?;
        let snapshot = {
            let mut state = self.inner.state.borrow_mut();
            let current = state.lifecycle.surface(surface);
            let current_generation = current.map(|snapshot| snapshot.generation);
            if generation == 0 || current_generation.is_some_and(|value| value >= generation) {
                record_rejection(
                    &mut state.rejections,
                    surface,
                    generation,
                    current_generation,
                );
                return Ok(false);
            }
            let snapshot = SurfaceSnapshot {
                visibility: SurfaceVisibility::Hidden,
                generation,
            };
            state.lifecycle.insert_surface(surface, snapshot);
            snapshot
        };
        self.publish_lifecycle(LifecycleEvent::SurfaceChanged { surface, snapshot });
        Ok(true)
    }

    /// Recreates a surface attachment at a new generation.
    pub fn rotate_surface(&self, surface: SurfaceId) -> Result<u64, SystemError> {
        self.rotate_surface_at(surface, Instant::now())
    }

    /// Recreates a surface attachment at a new generation at an explicit instant.
    pub fn rotate_surface_at(&self, surface: SurfaceId, now: Instant) -> Result<u64, SystemError> {
        self.require_ui_thread()?;
        let (generation, snapshot) = {
            let mut state = self.inner.state.borrow_mut();
            let current = state
                .lifecycle
                .surface(surface)
                .ok_or_else(missing_surface)?;
            let generation = current
                .generation
                .checked_add(1)
                .ok_or_else(exhausted_generation)?;
            state.tracker.set_surface_visible(surface, false, now);
            let hidden = SurfaceSnapshot {
                visibility: SurfaceVisibility::Hidden,
                generation,
            };
            state.lifecycle.insert_surface(surface, hidden);
            state.tracker.set_surface_visible(surface, true, now);
            let snapshot = SurfaceSnapshot {
                visibility: SurfaceVisibility::Visible { focused: true },
                generation,
            };
            state.lifecycle.insert_surface(surface, snapshot);
            (generation, snapshot)
        };
        self.publish_lifecycle(LifecycleEvent::SurfaceChanged { surface, snapshot });
        Ok(generation)
    }

    /// Applies a host surface snapshot at an explicit instant.
    ///
    /// Only the exact registered generation is accepted. Older callbacks and
    /// callbacks for detached surfaces are rejected without changing state.
    pub fn apply_surface_at(
        &self,
        surface: SurfaceId,
        snapshot: SurfaceSnapshot,
        now: Instant,
    ) -> Result<bool, SystemError> {
        self.require_ui_thread()?;
        let snapshot = {
            let mut state = self.inner.state.borrow_mut();
            let current = state.lifecycle.surface(surface);
            let current_generation = current.map(|current| current.generation);
            if current_generation != Some(snapshot.generation) {
                record_rejection(
                    &mut state.rejections,
                    surface,
                    snapshot.generation,
                    current_generation,
                );
                return Ok(false);
            }
            let visible = snapshot.visibility.is_visible();
            state.lifecycle.insert_surface(surface, snapshot);
            state.tracker.set_surface_visible(surface, visible, now);
            snapshot
        };
        self.publish_lifecycle(LifecycleEvent::SurfaceChanged { surface, snapshot });
        Ok(true)
    }

    /// Detaches a surface at an explicit instant.
    ///
    /// A stale generation cannot detach or revive the surface.
    pub fn detach_surface_at(
        &self,
        surface: SurfaceId,
        expected_generation: u64,
        now: Instant,
    ) -> Result<bool, SystemError> {
        self.require_ui_thread()?;
        {
            let mut state = self.inner.state.borrow_mut();
            let current = state.lifecycle.surface(surface);
            let current_generation = current.map(|current| current.generation);
            if current_generation != Some(expected_generation) {
                record_rejection(
                    &mut state.rejections,
                    surface,
                    expected_generation,
                    current_generation,
                );
                return Ok(false);
            }
            state.lifecycle.remove_surface(surface);
            state.tracker.set_surface_visible(surface, false, now);
        }
        self.publish_lifecycle(LifecycleEvent::SurfaceDetached { surface });
        Ok(true)
    }

    /// Records a user input observation.
    pub fn record_input_at(&self, at: Instant) -> Result<bool, SystemError> {
        self.require_ui_thread()?;
        let recorded = self.inner.state.borrow_mut().tracker.record_input_at(at);
        Ok(recorded)
    }

    /// Replaces the platform idle state.
    pub fn set_idle_state(&self, idle: IdleState) -> Result<(), SystemError> {
        self.require_ui_thread()?;
        self.inner.state.borrow_mut().tracker.set_idle_state(idle);
        Ok(())
    }

    /// Records a wall-clock observation for diagnostics only.
    pub fn record_wall_clock_observation(
        &self,
        at: std::time::SystemTime,
    ) -> Result<(), SystemError> {
        self.require_ui_thread()?;
        self.inner
            .state
            .borrow_mut()
            .tracker
            .record_wall_clock_observation(at);
        Ok(())
    }

    fn publish_lifecycle(&self, event: LifecycleEvent) {
        publish(&self.inner.publishers.lifecycle, event);
    }

    fn publish_preferences(&self, preferences: SystemPreferences) {
        publish(&self.inner.publishers.preferences, preferences);
    }

    fn publish_revalidation(&self, event: RevalidationEvent) {
        publish(&self.inner.publishers.revalidation, event);
    }
}

fn publish<T: Clone>(senders: &SenderList<T>, event: T) {
    let snapshot = senders.borrow().clone();
    for (_, active, sender) in &snapshot {
        if active.get() && !sender.send(event.clone()) {
            active.set(false);
        }
    }
    senders.borrow_mut().retain(|(_, active, _)| active.get());
}

fn remove_sender<T>(senders: &SenderList<T>, token: u64) {
    let mut senders = senders.borrow_mut();
    for (id, active, _) in senders.iter() {
        if *id == token {
            active.set(false);
        }
    }
    senders.retain(|(id, active, _)| *id != token && active.get());
}

fn missing_surface() -> SystemError {
    SystemError::new(ErrorCode::Unavailable, "Surface is not attached")
}

fn exhausted_generation() -> SystemError {
    SystemError::new(ErrorCode::ResourceExpired, "Surface generation exhausted")
}

fn record_rejection(
    rejections: &mut Vec<RejectedCallback>,
    surface: SurfaceId,
    received_generation: u64,
    current_generation: Option<u64>,
) {
    if rejections.len() == MAX_REJECTION_RECORDS {
        rejections.remove(0);
    }
    rejections.push(RejectedCallback::late(
        surface,
        received_generation,
        current_generation,
    ));
}

/// Deterministic host driver used by tests and examples.
///
/// The fake host owns its monotonic clock, so interval assertions never sleep.
/// Every mutation runs on the owning thread, so the driver unwraps the service's
/// UI-thread check.
#[derive(Debug, Clone)]
pub struct FakeHost {
    service: LifecycleService,
    now: Instant,
}

impl FakeHost {
    /// Builds a fake host for a process and its first session.
    pub fn new(process_id: ProcessId) -> Self {
        Self::with_session(process_id, SessionId(1))
    }

    /// Builds a fake host with an explicit session identity.
    pub fn with_session(process_id: ProcessId, session_id: SessionId) -> Self {
        let now = Instant::now();
        let service = LifecycleService::new(process_id, session_id);
        Self { service, now }
    }

    /// Builds a fake host around an existing shared service handle.
    pub fn with_service(service: LifecycleService) -> Self {
        Self {
            service,
            now: Instant::now(),
        }
    }

    /// The fake monotonic clock.
    pub fn now(&self) -> Instant {
        self.now
    }

    /// Reads the shared service.
    pub fn service(&self) -> &LifecycleService {
        &self.service
    }

    /// A second handle to the shared service.
    pub fn shared_service(&self) -> LifecycleService {
        self.service.clone()
    }

    /// Advances the fake monotonic clock.
    pub fn advance(&mut self, delta: Duration) -> Instant {
        self.now += delta;
        self.now
    }

    /// Registers a surface attachment.
    pub fn register(&mut self, surface: SurfaceId, generation: u64) -> bool {
        self.service
            .register_surface(surface, generation)
            .expect("fake host runs on its owning UI thread")
    }

    /// Applies visibility at the fake clock.
    pub fn set_visibility(
        &mut self,
        surface: SurfaceId,
        generation: u64,
        visibility: SurfaceVisibility,
    ) -> bool {
        let snapshot = SurfaceSnapshot {
            visibility,
            generation,
        };
        let now = self.now;
        self.service
            .apply_surface_at(surface, snapshot, now)
            .expect("fake host runs on its owning UI thread")
    }

    /// Shows a surface with an explicit focus state.
    pub fn show(&mut self, surface: SurfaceId, generation: u64, focused: bool) -> bool {
        let visibility = SurfaceVisibility::Visible { focused };
        self.set_visibility(surface, generation, visibility)
    }

    /// Hides a surface.
    pub fn hide(&mut self, surface: SurfaceId, generation: u64) -> bool {
        let hidden = SurfaceVisibility::Hidden;
        self.set_visibility(surface, generation, hidden)
    }

    /// Recreates a surface and shows it again in one clock tick.
    pub fn rotate(&mut self, surface: SurfaceId) -> u64 {
        let now = self.now;
        self.service
            .rotate_surface_at(surface, now)
            .expect("fake host runs on its owning UI thread")
    }

    /// Moves the application to the background.
    pub fn background(&mut self) {
        self.set_phase(AppPhase::Background);
    }

    /// Moves the application to the foreground and restores input handling.
    pub fn resume(&mut self) {
        self.set_phase(AppPhase::ForegroundActive);
    }

    /// Locks the device.
    pub fn lock(&mut self) {
        self.service
            .set_session_lock(SessionLockState::Locked)
            .expect("fake host runs on its owning UI thread");
    }

    /// Unlocks the device.
    pub fn unlock(&mut self) {
        self.service
            .set_session_lock(SessionLockState::Unlocked)
            .expect("fake host runs on its owning UI thread");
    }

    /// The live union duration at the fake clock.
    pub fn active_duration(&self) -> Duration {
        self.service.active_duration(self.now)
    }

    fn set_phase(&mut self, phase: AppPhase) {
        let now = self.now;
        self.service
            .set_phase_at(phase, now)
            .expect("fake host runs on its owning UI thread");
    }
}
