//! UI-local task-presentation service: leases, deterministic aggregation,
//! throttled updates, surface recreation, badges, attention, command links,
//! notification mirrors and bounded events.

use crate::action::{ActionRejectionReason, TaskActionDispatchOutcome, TaskActionRejection};
use crate::aggregator::{
    AggregateInput, AggregatedAppPresentation, AggregatedSurfacePresentation, aggregate_actions,
    aggregate_progress, select_lead,
};
use crate::notification::{NotificationTaskMirror, notification_action_id_for_task};
use crate::{
    APPLICATION_SURFACE, AcknowledgmentPrecision, AggregateMode, AttentionClearPolicy,
    AttentionRequest, BadgeClearPolicy, BadgeRequest, BadgeScope, BadgeValue,
    BusinessTaskCompletion, PresentationSurfaceKind, SurfacePresentationReceipt, TaskActionButton,
    TaskActionId, TaskCompletionRecord, TaskPresentationId, TaskPresentationOperation,
    TaskPresentationOutcome, TaskPriority, TaskProgressState, TaskRegistration, TaskScope,
    TaskStateKind, TerminalNotificationPolicy, ThrottleConfig,
};
use gpui_lifecycle::LifecycleState;
use gpui_menus::{InvocationContext, MenuCommandService, TriggerSource};
use gpui_notifications::{
    ActionId as NotificationActionId, Event as NotificationChannelEvent, EventEnvelope,
    NotificationEvent, NotificationId, NotificationService,
};
use gpui_system::{
    Availability, Configuration, ErrorCode, EventSender, EventSubscription, PermissionState,
    Support, SurfaceId, SystemError, SystemServices, event_channel,
};
use std::{
    cell::{Cell, RefCell},
    collections::{BTreeMap, HashMap, VecDeque},
    future::Future,
    pin::Pin,
    rc::{Rc, Weak},
    task::{Context as TaskContext, Poll, Waker},
    time::{Duration, Instant},
};

/// Default bounded capacity of one task-presentation event stream.
pub const DEFAULT_TASK_PRESENTATION_EVENT_CAPACITY: usize = 64;

/// Maximum number of terminal task tombstones retained for stale-click checks.
pub const MAX_TASK_TOMBSTONES: usize = 256;

/// Why a badge or attention indicator was cleared.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ClearReason {
    Focus,
    ExplicitCaller,
    LeaseReleased,
    AllTasksTerminal,
    Terminal,
    SurfaceDisposed,
    GenerationExpired,
}

impl ClearReason {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Focus => "focus",
            Self::ExplicitCaller => "explicit_caller",
            Self::LeaseReleased => "lease_released",
            Self::AllTasksTerminal => "all_tasks_terminal",
            Self::Terminal => "terminal",
            Self::SurfaceDisposed => "surface_disposed",
            Self::GenerationExpired => "generation_expired",
        }
    }
}

/// Public-safe service observations. Producers never block on consumers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TaskPresentationEvent {
    TaskRegistered {
        task: TaskPresentationId,
        sequence: u64,
        priority: TaskPriority,
        scope: TaskScope,
    },
    TaskProgressChanged {
        task: TaskPresentationId,
        state: TaskProgressState,
    },
    TaskTerminated {
        task: TaskPresentationId,
        state: TaskStateKind,
    },
    TaskLeaseReleased {
        task: TaskPresentationId,
    },
    AggregatedSurfaceChanged {
        surface: SurfaceId,
        generation: u64,
        progress: Option<TaskProgressState>,
        active_task_count: usize,
    },
    AggregatedAppChanged {
        progress: Option<TaskProgressState>,
        active_task_count: usize,
    },
    BadgeApplied {
        surface: Option<SurfaceId>,
        generation: Option<u64>,
        badge: BadgeValue,
        policy: BadgeClearPolicy,
    },
    BadgeCleared {
        surface: Option<SurfaceId>,
        generation: Option<u64>,
        reason: ClearReason,
    },
    AttentionRequested {
        request: AttentionRequest,
        policy: AttentionClearPolicy,
    },
    AttentionCleared {
        surface: SurfaceId,
        generation: u64,
        reason: ClearReason,
    },
    SurfaceAttached {
        surface: SurfaceId,
        generation: u64,
        focused: bool,
    },
    SurfaceRecreated {
        surface: SurfaceId,
        old_generation: u64,
        new_generation: u64,
    },
    SurfaceDisposed {
        surface: SurfaceId,
        generation: u64,
    },
    ActionDispatched {
        task: TaskPresentationId,
        action_id: TaskActionId,
        command_id: gpui_menus::CommandId,
    },
    ActionRejected {
        task: Option<TaskPresentationId>,
        action_id: String,
        reason: ActionRejectionReason,
    },
    MirrorOutcome {
        task: TaskPresentationId,
        precision: AcknowledgmentPrecision,
    },
    NotificationActionRejected {
        task: Option<TaskPresentationId>,
        action_id: Option<String>,
        reason: ActionRejectionReason,
    },
}

type Subscriber = (u64, Rc<Cell<bool>>, EventSender<TaskPresentationEvent>);

#[derive(Debug, Clone)]
struct TaskRecord {
    id: TaskPresentationId,
    sequence: u64,
    priority: TaskPriority,
    scope: TaskScope,
    progress: TaskProgressState,
    title: Option<String>,
    body: Option<String>,
    channel_id: Option<String>,
    actions: Vec<TaskActionButton>,
    notification_posted: bool,
    last_notification_at: Option<Instant>,
    pending_notification: bool,
}

struct SurfaceRecord {
    generation: u64,
    focused: bool,
    computed: AggregatedSurfacePresentation,
    applied: AggregatedSurfacePresentation,
    last_progress_at: Option<Instant>,
    pending_progress: bool,
}

impl SurfaceRecord {
    fn new(surface: SurfaceId, generation: u64, focused: bool) -> Self {
        let empty = AggregatedSurfacePresentation::empty(surface, generation);
        Self {
            generation,
            focused,
            computed: empty.clone(),
            applied: empty,
            last_progress_at: None,
            pending_progress: false,
        }
    }
}

#[derive(Clone)]
struct BadgeRecord {
    sequence: u64,
    request: BadgeRequest,
}

#[derive(Clone)]
struct AttentionRecord {
    sequence: u64,
    request: AttentionRequest,
    policy: AttentionClearPolicy,
}

struct ServiceState {
    tasks: BTreeMap<TaskPresentationId, TaskRecord>,
    tombstones: BTreeMap<TaskPresentationId, TaskStateKind>,
    tombstone_order: VecDeque<TaskPresentationId>,
    surfaces: HashMap<SurfaceId, SurfaceRecord>,
    badges: BTreeMap<u64, BadgeRecord>,
    attention: BTreeMap<u64, AttentionRecord>,
    next_sequence: u64,
    next_badge_id: u64,
    next_attention_id: u64,
    next_subscriber: u64,
    throttle: ThrottleConfig,
    aggregate_mode: AggregateMode,
    subscribers: Vec<Subscriber>,
}

impl ServiceState {
    fn new() -> Self {
        Self {
            tasks: BTreeMap::new(),
            tombstones: BTreeMap::new(),
            tombstone_order: VecDeque::new(),
            surfaces: HashMap::new(),
            badges: BTreeMap::new(),
            attention: BTreeMap::new(),
            next_sequence: 1,
            next_badge_id: 1,
            next_attention_id: 1,
            next_subscriber: 1,
            throttle: ThrottleConfig::default(),
            aggregate_mode: AggregateMode::default(),
            subscribers: Vec::new(),
        }
    }
}

fn forget_tombstone(state: &mut ServiceState, id: &TaskPresentationId) {
    state.tombstones.remove(id);
    state.tombstone_order.retain(|existing| existing != id);
}

fn remember_tombstone(state: &mut ServiceState, id: TaskPresentationId, terminal: TaskStateKind) {
    state.tombstones.remove(&id);
    state.tombstone_order.retain(|existing| existing != &id);
    state.tombstones.insert(id.clone(), terminal);
    state.tombstone_order.push_back(id);
    while state.tombstones.len() > MAX_TASK_TOMBSTONES {
        let Some(oldest) = state.tombstone_order.pop_front() else {
            break;
        };
        state.tombstones.remove(&oldest);
    }
}

fn ensure_task_sequence(record: &TaskRecord, expected: Option<u64>) -> Result<(), SystemError> {
    if expected.is_some_and(|sequence| sequence != record.sequence) {
        return Err(SystemError::new(
            ErrorCode::ContextExpired,
            "Task lease belongs to an older task registration",
        ));
    }
    Ok(())
}

pub(crate) struct ServiceInner {
    provider: Rc<dyn crate::TaskPresentationProvider>,
    state: RefCell<ServiceState>,
    mirror: RefCell<Option<NotificationTaskMirror>>,
    terminal_policy: RefCell<Option<TerminalNotificationPolicy>>,
    menu: RefCell<Option<MenuCommandService>>,
    notification_subscription: RefCell<Option<EventSubscription<NotificationEvent>>>,
}

fn lock_state<T>(value: &RefCell<T>) -> std::cell::RefMut<'_, T> {
    value
        .try_borrow_mut()
        .unwrap_or_else(|_| panic!("task-presentation service state is already borrowed"))
}

fn borrow_state<T>(value: &RefCell<T>) -> std::cell::Ref<'_, T> {
    value
        .try_borrow()
        .unwrap_or_else(|_| panic!("task-presentation service state is already borrowed"))
}

/// UI-local task-presentation service.
///
/// The handle is `Rc`-backed and cheap to clone. It is intentionally not `Send`:
/// taskbar, Dock and notification providers are UI-thread objects. Every mutation
/// has an explicit [`Instant`] form for deterministic tests and an ergonomic
/// wrapper using [`Instant::now`].
#[derive(Clone)]
pub struct TaskPresentationService {
    inner: Rc<ServiceInner>,
}

impl std::fmt::Debug for TaskPresentationService {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let state = borrow_state(&self.inner.state);
        formatter
            .debug_struct("TaskPresentationService")
            .field("tasks", &state.tasks.len())
            .field("surfaces", &state.surfaces.len())
            .field("badges", &state.badges.len())
            .field("attention", &state.attention.len())
            .field("subscribers", &state.subscribers.len())
            .finish_non_exhaustive()
    }
}

impl TaskPresentationService {
    pub fn new(provider: Rc<dyn crate::TaskPresentationProvider>) -> Self {
        Self {
            inner: Rc::new(ServiceInner {
                provider,
                state: RefCell::new(ServiceState::new()),
                mirror: RefCell::new(None),
                terminal_policy: RefCell::new(None),
                menu: RefCell::new(None),
                notification_subscription: RefCell::new(None),
            }),
        }
    }

    pub fn from_provider(provider: impl crate::TaskPresentationProvider + 'static) -> Self {
        Self::new(Rc::new(provider))
    }

    pub fn unsupported() -> Self {
        Self::from_provider(crate::UnsupportedTaskPresentationProvider::new())
    }

    pub fn from_fake(fake: crate::FakeTaskPresentationProvider) -> Self {
        Self::new(Rc::new(fake))
    }

    pub fn shares_state_with(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.inner, &other.inner)
    }

    pub fn provider(&self) -> &Rc<dyn crate::TaskPresentationProvider> {
        &self.inner.provider
    }

    pub fn install(self, services: &mut SystemServices) -> Result<(), SystemError> {
        services.register(self)
    }

    pub fn install_cloned(&self, services: &mut SystemServices) -> Result<(), SystemError> {
        services.register(self.clone())
    }

    // ---- configuration ---------------------------------------------------

    pub fn with_throttle(self, config: ThrottleConfig) -> Self {
        self.set_throttle_config(config);
        self
    }

    pub fn set_throttle_config(&self, config: ThrottleConfig) {
        lock_state(&self.inner.state).throttle = config;
    }

    pub fn throttle_config(&self) -> ThrottleConfig {
        borrow_state(&self.inner.state).throttle
    }

    pub fn with_aggregate_mode(self, mode: AggregateMode) -> Self {
        self.set_aggregate_mode(mode);
        self
    }

    pub fn set_aggregate_mode(&self, mode: AggregateMode) {
        lock_state(&self.inner.state).aggregate_mode = mode;
    }

    pub fn aggregate_mode(&self) -> AggregateMode {
        borrow_state(&self.inner.state).aggregate_mode
    }

    pub fn with_notifications(self, service: NotificationService) -> Self {
        let mut mirror = NotificationTaskMirror::new(service);
        if let Some(policy) = *self.inner.terminal_policy.borrow() {
            mirror.set_terminal_policy(policy);
        }
        self.with_notification_mirror(mirror)
    }

    pub fn with_notification_mirror(self, mut mirror: NotificationTaskMirror) -> Self {
        if let Some(policy) = *self.inner.terminal_policy.borrow() {
            mirror.set_terminal_policy(policy);
        }
        let subscription = mirror.service().subscribe();
        *self.inner.notification_subscription.borrow_mut() = Some(subscription);
        *self.inner.mirror.borrow_mut() = Some(mirror);
        self
    }

    pub fn with_terminal_notification_policy(self, policy: TerminalNotificationPolicy) -> Self {
        *self.inner.terminal_policy.borrow_mut() = Some(policy);
        if let Some(mirror) = self.inner.mirror.borrow_mut().as_mut() {
            mirror.set_terminal_policy(policy);
        }
        self
    }

    pub fn with_menu_service(self, service: MenuCommandService) -> Self {
        *self.inner.menu.borrow_mut() = Some(service);
        self
    }

    pub fn notification_mirror(&self) -> Option<NotificationTaskMirror> {
        self.inner.mirror.borrow().clone()
    }

    pub fn menu_service(&self) -> Option<MenuCommandService> {
        self.inner.menu.borrow().clone()
    }

    // ---- events ----------------------------------------------------------

    /// Create an independent bounded event stream. Subscriber list borrows are
    /// released before every send, so a synchronous waker may re-enter the
    /// service without a `RefCell` panic.
    pub fn subscribe(&self) -> EventSubscription<TaskPresentationEvent> {
        let (token, active, cleanup_active) = {
            let mut state = lock_state(&self.inner.state);
            let token = state.next_subscriber;
            state.next_subscriber = state.next_subscriber.wrapping_add(1);
            let active = Rc::new(Cell::new(true));
            (token, active.clone(), active)
        };
        let weak = Rc::downgrade(&self.inner);
        let (sender, subscription) =
            event_channel(DEFAULT_TASK_PRESENTATION_EVENT_CAPACITY, move || {
                cleanup_active.set(false);
                if let Some(inner) = weak.upgrade() {
                    let mut state = lock_state(&inner.state);
                    state
                        .subscribers
                        .retain(|(id, active, _)| *id != token && active.get());
                }
            });
        lock_state(&self.inner.state)
            .subscribers
            .push((token, active, sender));
        subscription
    }

    pub fn subscriber_count(&self) -> usize {
        let mut state = lock_state(&self.inner.state);
        state.subscribers.retain(|(_, active, _)| active.get());
        state.subscribers.len()
    }

    fn emit(&self, event: TaskPresentationEvent) {
        let senders = {
            let mut state = lock_state(&self.inner.state);
            state.subscribers.retain(|(_, active, _)| active.get());
            state
                .subscribers
                .iter()
                .map(|(_, _, sender)| sender.clone())
                .collect::<Vec<_>>()
        };
        for sender in senders {
            sender.send(event.clone());
        }
    }

    // ---- task registration and updates ----------------------------------

    pub fn register_task(
        &self,
        id: TaskPresentationId,
        priority: TaskPriority,
        scope: TaskScope,
        progress: TaskProgressState,
    ) -> Result<TaskLease, SystemError> {
        self.register_task_at(id, priority, scope, progress, Instant::now())
    }

    pub fn register_task_at(
        &self,
        id: TaskPresentationId,
        priority: TaskPriority,
        scope: TaskScope,
        progress: TaskProgressState,
        now: Instant,
    ) -> Result<TaskLease, SystemError> {
        self.register_task_with_at(
            TaskRegistration::new(id, progress)
                .with_priority(priority)
                .with_scope(scope),
            now,
        )
    }

    pub fn register_task_with(
        &self,
        registration: TaskRegistration,
    ) -> Result<TaskLease, SystemError> {
        self.register_task_with_at(registration, Instant::now())
    }

    pub fn register_task_with_at(
        &self,
        registration: TaskRegistration,
        now: Instant,
    ) -> Result<TaskLease, SystemError> {
        registration.validate()?;
        if registration.progress.is_terminal() {
            return Err(SystemError::new(
                ErrorCode::InvalidInput,
                "A task cannot be registered in a terminal state",
            ));
        }
        let sequence = {
            let mut state = lock_state(&self.inner.state);
            if state.tasks.contains_key(&registration.id) {
                return Err(SystemError::new(
                    ErrorCode::Busy,
                    "A task with this identity is already active",
                ));
            }
            if let Some(surface) = registration.scope.surface()
                && let Some(record) = state.surfaces.get(&surface)
                && record.generation != registration.scope.generation().unwrap_or(0)
            {
                return Err(SystemError::new(
                    ErrorCode::ContextExpired,
                    "Task surface generation is not the live generation",
                ));
            }
            let sequence = state.next_sequence;
            state.next_sequence = state.next_sequence.saturating_add(1);
            forget_tombstone(&mut state, &registration.id);
            state.tasks.insert(
                registration.id.clone(),
                TaskRecord {
                    id: registration.id.clone(),
                    sequence,
                    priority: registration.priority,
                    scope: registration.scope,
                    progress: registration.progress.clone(),
                    title: registration.title,
                    body: registration.body,
                    channel_id: registration.channel_id,
                    actions: registration.actions,
                    notification_posted: false,
                    last_notification_at: None,
                    pending_notification: false,
                },
            );
            sequence
        };
        let mut outcome = self.apply_all_surfaces(now, true);
        outcome.merge(self.deliver_notification(&registration.id, now, true));
        self.emit(TaskPresentationEvent::TaskRegistered {
            task: registration.id.clone(),
            sequence,
            priority: registration.priority,
            scope: registration.scope,
        });
        Ok(TaskLease {
            inner: Rc::downgrade(&self.inner),
            id: registration.id,
            sequence: Some(sequence),
            initial_outcome: outcome,
        })
    }

    pub fn update_task(
        &self,
        id: &TaskPresentationId,
        progress: TaskProgressState,
    ) -> Result<TaskPresentationOutcome, SystemError> {
        self.update_task_at(id, progress, Instant::now())
    }

    pub fn update_task_at(
        &self,
        id: &TaskPresentationId,
        progress: TaskProgressState,
        now: Instant,
    ) -> Result<TaskPresentationOutcome, SystemError> {
        self.update_task_checked_at(id, progress, None, now)
    }

    fn update_task_checked_at(
        &self,
        id: &TaskPresentationId,
        progress: TaskProgressState,
        expected_sequence: Option<u64>,
        now: Instant,
    ) -> Result<TaskPresentationOutcome, SystemError> {
        if progress.is_terminal() {
            return match progress {
                TaskProgressState::Cancelled => {
                    self.cancel_task_checked_at(id, now, expected_sequence)
                }
                _ => self.complete_task_checked_at(id, now, expected_sequence),
            };
        }
        {
            let mut state = lock_state(&self.inner.state);
            let record = state
                .tasks
                .get_mut(id)
                .ok_or_else(|| SystemError::new(ErrorCode::ContextExpired, "Task is not active"))?;
            ensure_task_sequence(record, expected_sequence)?;
            record.progress = progress.clone();
            record.pending_notification = false;
        }
        let mut outcome = self.apply_all_surfaces(now, false);
        outcome.merge(self.deliver_notification(id, now, true));
        self.emit(TaskPresentationEvent::TaskProgressChanged {
            task: id.clone(),
            state: progress,
        });
        Ok(outcome)
    }

    pub fn rebind_task(
        &self,
        id: &TaskPresentationId,
        scope: TaskScope,
    ) -> Result<TaskPresentationOutcome, SystemError> {
        self.rebind_task_at(id, scope, Instant::now())
    }

    /// Explicitly move a task to another scope, replaying the aggregate at the
    /// new live generation.
    pub fn rebind_task_at(
        &self,
        id: &TaskPresentationId,
        scope: TaskScope,
        now: Instant,
    ) -> Result<TaskPresentationOutcome, SystemError> {
        self.rebind_task_checked_at(id, scope, None, now)
    }

    fn rebind_task_checked_at(
        &self,
        id: &TaskPresentationId,
        scope: TaskScope,
        expected_sequence: Option<u64>,
        now: Instant,
    ) -> Result<TaskPresentationOutcome, SystemError> {
        if let Some(surface) = scope.surface() {
            let state = borrow_state(&self.inner.state);
            let record = state.surfaces.get(&surface).ok_or_else(|| {
                SystemError::new(ErrorCode::ContextExpired, "Task surface is not attached")
            })?;
            if record.generation != scope.generation().unwrap_or(0) {
                return Err(SystemError::new(
                    ErrorCode::ContextExpired,
                    "Task surface generation is not live",
                ));
            }
        }
        {
            let mut state = lock_state(&self.inner.state);
            let record = state
                .tasks
                .get_mut(id)
                .ok_or_else(|| SystemError::new(ErrorCode::ContextExpired, "Task is not active"))?;
            ensure_task_sequence(record, expected_sequence)?;
            record.scope = scope;
        }
        let outcome = self.apply_all_surfaces(now, true);
        Ok(outcome)
    }

    pub fn complete_task(
        &self,
        id: &TaskPresentationId,
    ) -> Result<TaskPresentationOutcome, SystemError> {
        self.complete_task_at(id, Instant::now())
    }

    pub fn complete_task_at(
        &self,
        id: &TaskPresentationId,
        now: Instant,
    ) -> Result<TaskPresentationOutcome, SystemError> {
        self.complete_task_checked_at(id, now, None)
    }

    fn complete_task_checked_at(
        &self,
        id: &TaskPresentationId,
        now: Instant,
        expected_sequence: Option<u64>,
    ) -> Result<TaskPresentationOutcome, SystemError> {
        self.complete_task_with_business_result_checked_at(
            id,
            BusinessTaskCompletion::Succeeded(()),
            now,
            expected_sequence,
        )
        .map(|record| record.presentation)
    }

    pub fn cancel_task(
        &self,
        id: &TaskPresentationId,
    ) -> Result<TaskPresentationOutcome, SystemError> {
        self.cancel_task_at(id, Instant::now())
    }

    pub fn cancel_task_at(
        &self,
        id: &TaskPresentationId,
        now: Instant,
    ) -> Result<TaskPresentationOutcome, SystemError> {
        self.cancel_task_checked_at(id, now, None)
    }

    fn cancel_task_checked_at(
        &self,
        id: &TaskPresentationId,
        now: Instant,
        expected_sequence: Option<u64>,
    ) -> Result<TaskPresentationOutcome, SystemError> {
        self.complete_task_with_business_result_checked_at(
            id,
            BusinessTaskCompletion::<()>::Cancelled(gpui_system::CancelReason::User),
            now,
            expected_sequence,
        )
        .map(|record| record.presentation)
    }

    pub fn complete_task_with_business_result<T>(
        &self,
        id: &TaskPresentationId,
        business: BusinessTaskCompletion<T>,
    ) -> Result<TaskCompletionRecord<T>, SystemError> {
        self.complete_task_with_business_result_at(id, business, Instant::now())
    }

    /// Complete a task with a business result that is preserved independently
    /// from every presentation receipt.
    pub fn complete_task_with_business_result_at<T>(
        &self,
        id: &TaskPresentationId,
        business: BusinessTaskCompletion<T>,
        now: Instant,
    ) -> Result<TaskCompletionRecord<T>, SystemError> {
        self.complete_task_with_business_result_checked_at(id, business, now, None)
    }

    fn complete_task_with_business_result_checked_at<T>(
        &self,
        id: &TaskPresentationId,
        business: BusinessTaskCompletion<T>,
        now: Instant,
        expected_sequence: Option<u64>,
    ) -> Result<TaskCompletionRecord<T>, SystemError> {
        let terminal_state = match &business {
            BusinessTaskCompletion::Succeeded(_) => TaskProgressState::Completed,
            BusinessTaskCompletion::Failed(message) => TaskProgressState::Error {
                per_mille: None,
                message: Some(message.clone()),
            },
            BusinessTaskCompletion::Cancelled(_) => TaskProgressState::Cancelled,
        };
        let record = {
            let mut state = lock_state(&self.inner.state);
            {
                let record = state.tasks.get(id).ok_or_else(|| {
                    SystemError::new(ErrorCode::ContextExpired, "Task is not active")
                })?;
                ensure_task_sequence(record, expected_sequence)?;
            }
            let record = state
                .tasks
                .remove(id)
                .ok_or_else(|| SystemError::new(ErrorCode::ContextExpired, "Task is not active"))?;
            remember_tombstone(&mut state, id.clone(), terminal_state.kind());
            record
        };
        let mut outcome = self.apply_all_surfaces(now, true);
        outcome.merge(self.terminate_notification(&record, &terminal_state));
        self.clear_all_tasks_terminal_badges(now);
        self.emit(TaskPresentationEvent::TaskTerminated {
            task: id.clone(),
            state: terminal_state.kind(),
        });
        Ok(TaskCompletionRecord {
            business,
            presentation: outcome,
        })
    }

    /// Release a task's presentation contribution. This is idempotent and is
    /// invoked by [`TaskLease::drop`].
    pub fn release_task_at(
        &self,
        id: &TaskPresentationId,
        sequence: u64,
        reason: ClearReason,
        now: Instant,
    ) -> Option<TaskPresentationOutcome> {
        let record = {
            let mut state = lock_state(&self.inner.state);
            if state
                .tasks
                .get(id)
                .is_none_or(|record| record.sequence != sequence)
            {
                return None;
            }
            let record = state.tasks.remove(id)?;
            remember_tombstone(&mut state, id.clone(), TaskProgressState::Cancelled.kind());
            record
        };
        let mut outcome = self.apply_all_surfaces(now, true);
        outcome.merge(self.terminate_notification(&record, &TaskProgressState::Cancelled));
        self.clear_all_tasks_terminal_badges(now);
        self.emit(TaskPresentationEvent::TaskLeaseReleased { task: id.clone() });
        let _ = reason;
        Some(outcome)
    }

    pub fn is_task_active(&self, id: &TaskPresentationId) -> bool {
        borrow_state(&self.inner.state).tasks.contains_key(id)
    }

    pub fn task_state(&self, id: &TaskPresentationId) -> Option<TaskProgressState> {
        borrow_state(&self.inner.state)
            .tasks
            .get(id)
            .map(|record| record.progress.clone())
    }

    pub fn active_task_count(&self) -> usize {
        borrow_state(&self.inner.state).tasks.len()
    }

    pub fn tombstone_count(&self) -> usize {
        borrow_state(&self.inner.state).tombstones.len()
    }

    pub fn tombstone_state(&self, id: &TaskPresentationId) -> Option<TaskStateKind> {
        borrow_state(&self.inner.state).tombstones.get(id).copied()
    }

    // ---- aggregated views ------------------------------------------------

    pub fn is_surface_attached(&self, surface: SurfaceId) -> bool {
        borrow_state(&self.inner.state)
            .surfaces
            .contains_key(&surface)
    }

    pub fn surface_generation(&self, surface: SurfaceId) -> Option<u64> {
        borrow_state(&self.inner.state)
            .surfaces
            .get(&surface)
            .map(|record| record.generation)
    }

    pub fn aggregated_app_presentation(&self) -> AggregatedAppPresentation {
        let state = borrow_state(&self.inner.state);
        compute_app_presentation(&state)
    }

    pub fn aggregated_surface_presentation(
        &self,
        surface: SurfaceId,
    ) -> Option<AggregatedSurfacePresentation> {
        let state = borrow_state(&self.inner.state);
        let record = state.surfaces.get(&surface)?;
        Some(compute_surface_presentation(
            &state,
            surface,
            record.generation,
        ))
    }

    // ---- surfaces --------------------------------------------------------

    pub fn attach_surface(&self, surface: SurfaceId, generation: u64, focused: bool) {
        self.attach_surface_at(surface, generation, focused, Instant::now());
    }

    pub fn attach_surface_at(
        &self,
        surface: SurfaceId,
        generation: u64,
        focused: bool,
        now: Instant,
    ) {
        let recreate = {
            let mut state = lock_state(&self.inner.state);
            if let Some(record) = state.surfaces.get(&surface) {
                if record.generation == generation {
                    state
                        .surfaces
                        .get_mut(&surface)
                        .expect("surface exists")
                        .focused = focused;
                    false
                } else if generation > record.generation {
                    true
                } else {
                    drop(state);
                    self.emit(TaskPresentationEvent::SurfaceAttached {
                        surface,
                        generation,
                        focused,
                    });
                    return;
                }
            } else {
                state
                    .surfaces
                    .insert(surface, SurfaceRecord::new(surface, generation, focused));
                false
            }
        };
        if recreate {
            let _ = self.recreate_surface_at(surface, generation, now);
        }
        let mut outcome = self.apply_all_surfaces(now, true);
        if focused {
            outcome.merge(self.notify_surface_focused_at(surface, now));
        }
        self.emit(TaskPresentationEvent::SurfaceAttached {
            surface,
            generation,
            focused,
        });
    }

    pub fn recreate_surface(
        &self,
        surface: SurfaceId,
        new_generation: u64,
    ) -> Result<TaskPresentationOutcome, SystemError> {
        self.recreate_surface_at(surface, new_generation, Instant::now())
    }

    pub fn recreate_surface_at(
        &self,
        surface: SurfaceId,
        new_generation: u64,
        now: Instant,
    ) -> Result<TaskPresentationOutcome, SystemError> {
        let old_generation = {
            let state = borrow_state(&self.inner.state);
            state
                .surfaces
                .get(&surface)
                .map(|record| record.generation)
                .ok_or_else(|| {
                    SystemError::new(ErrorCode::ContextExpired, "Surface is not attached")
                })?
        };
        if new_generation <= old_generation {
            return Err(SystemError::new(
                ErrorCode::InvalidInput,
                "Recreated surface generation must increase monotonically",
            ));
        }
        let (removed_tasks, removed_badges, removed_attention) = {
            let mut state = lock_state(&self.inner.state);
            let removed_tasks = state
                .tasks
                .iter()
                .filter(|(_, task)| task.scope.matches(surface, old_generation))
                .map(|(id, task)| (id.clone(), task.clone()))
                .collect::<Vec<_>>();
            for (id, task) in &removed_tasks {
                state.tasks.remove(id);
                remember_tombstone(&mut state, task.id.clone(), TaskStateKind::Cancelled);
            }
            let removed_badges = state
                .badges
                .iter()
                .filter(|(_, badge)| {
                    matches!(badge.request.scope, BadgeScope::Surface { surface: scoped, generation } if scoped.0 == surface.0 && generation == old_generation)
                })
                .map(|(id, _)| *id)
                .collect::<Vec<_>>();
            for id in &removed_badges {
                state.badges.remove(id);
            }
            let removed_attention = state
                .attention
                .iter()
                .filter(|(_, attention)| {
                    attention.request.surface.0 == surface.0
                        && attention.request.generation == old_generation
                })
                .map(|(id, _)| *id)
                .collect::<Vec<_>>();
            for id in &removed_attention {
                state.attention.remove(id);
            }
            if let Some(record) = state.surfaces.get_mut(&surface) {
                record.generation = new_generation;
                record.focused = false;
                record.computed = AggregatedSurfacePresentation::empty(surface, new_generation);
                record.applied = AggregatedSurfacePresentation::empty(surface, new_generation);
                record.last_progress_at = None;
                record.pending_progress = false;
            }
            (removed_tasks, removed_badges, removed_attention)
        };
        let mut outcome = TaskPresentationOutcome::empty();
        for (_, task) in &removed_tasks {
            outcome.merge(self.terminate_notification(task, &TaskProgressState::Cancelled));
            self.emit(TaskPresentationEvent::TaskTerminated {
                task: task.id.clone(),
                state: TaskStateKind::Cancelled,
            });
        }
        let _ = self
            .inner
            .provider
            .clear_surface_progress(surface, old_generation);
        let _ = self.inner.provider.clear_actions(surface, old_generation);
        for _ in &removed_badges {
            let _ = self
                .inner
                .provider
                .clear_surface_badge(surface, old_generation);
            self.emit(TaskPresentationEvent::BadgeCleared {
                surface: Some(surface),
                generation: Some(old_generation),
                reason: ClearReason::GenerationExpired,
            });
        }
        for _ in &removed_attention {
            let _ = self.inner.provider.clear_attention(surface, old_generation);
            self.emit(TaskPresentationEvent::AttentionCleared {
                surface,
                generation: old_generation,
                reason: ClearReason::GenerationExpired,
            });
        }
        self.clear_all_tasks_terminal_badges(now);
        outcome.merge(self.apply_all_surfaces(now, true));
        self.emit(TaskPresentationEvent::SurfaceRecreated {
            surface,
            old_generation,
            new_generation,
        });
        Ok(outcome)
    }

    pub fn dispose_surface(&self, surface: SurfaceId, generation: u64) {
        self.dispose_surface_at(surface, generation, Instant::now());
    }

    pub fn dispose_surface_at(&self, surface: SurfaceId, generation: u64, now: Instant) {
        let (removed_tasks, removed_badges, removed_attention, existing) = {
            let mut state = lock_state(&self.inner.state);
            if state
                .surfaces
                .get(&surface)
                .is_none_or(|record| record.generation != generation)
            {
                return;
            }
            let existing = state.surfaces.remove(&surface);
            let removed_tasks = state
                .tasks
                .iter()
                .filter(|(_, task)| task.scope.matches(surface, generation))
                .map(|(id, task)| (id.clone(), task.clone()))
                .collect::<Vec<_>>();
            for (id, task) in &removed_tasks {
                state.tasks.remove(id);
                remember_tombstone(&mut state, task.id.clone(), TaskStateKind::Cancelled);
            }
            let removed_badges = state
                .badges
                .iter()
                .filter(|(_, badge)| {
                    matches!(badge.request.scope, BadgeScope::Surface { surface: scoped, generation: scoped_generation } if scoped.0 == surface.0 && scoped_generation == generation)
                })
                .map(|(id, _)| *id)
                .collect::<Vec<_>>();
            for id in &removed_badges {
                state.badges.remove(id);
            }
            let removed_attention = state
                .attention
                .iter()
                .filter(|(_, attention)| {
                    attention.request.surface.0 == surface.0
                        && attention.request.generation == generation
                })
                .map(|(id, _)| *id)
                .collect::<Vec<_>>();
            for id in &removed_attention {
                state.attention.remove(id);
            }
            (removed_tasks, removed_badges, removed_attention, existing)
        };
        if existing.is_none() {
            return;
        }
        for (_, task) in &removed_tasks {
            let _ = self.terminate_notification(task, &TaskProgressState::Cancelled);
            self.emit(TaskPresentationEvent::TaskTerminated {
                task: task.id.clone(),
                state: TaskStateKind::Cancelled,
            });
        }
        let _ = self
            .inner
            .provider
            .clear_surface_progress(surface, generation);
        let _ = self.inner.provider.clear_actions(surface, generation);
        for _ in &removed_badges {
            let _ = self.inner.provider.clear_surface_badge(surface, generation);
            self.emit(TaskPresentationEvent::BadgeCleared {
                surface: Some(surface),
                generation: Some(generation),
                reason: ClearReason::SurfaceDisposed,
            });
        }
        for _ in &removed_attention {
            let _ = self.inner.provider.clear_attention(surface, generation);
            self.emit(TaskPresentationEvent::AttentionCleared {
                surface,
                generation,
                reason: ClearReason::SurfaceDisposed,
            });
        }
        self.clear_all_tasks_terminal_badges(now);
        let _ = self.apply_all_surfaces(now, true);
        self.emit(TaskPresentationEvent::SurfaceDisposed {
            surface,
            generation,
        });
    }

    pub fn notify_surface_focused(&self, surface: SurfaceId) {
        self.notify_surface_focused_at(surface, Instant::now());
    }

    pub fn notify_surface_focused_at(
        &self,
        surface: SurfaceId,
        now: Instant,
    ) -> TaskPresentationOutcome {
        {
            let mut state = lock_state(&self.inner.state);
            if let Some(record) = state.surfaces.get_mut(&surface) {
                record.focused = true;
            }
        }
        let mut outcome = TaskPresentationOutcome::empty();
        let badge_ids = {
            let state = borrow_state(&self.inner.state);
            state
                .badges
                .iter()
                .filter(|(_, badge)| {
                    badge.request.policy == BadgeClearPolicy::ClearOnSurfaceFocus
                        && badge.request.scope.matches_surface(surface)
                })
                .map(|(id, _)| *id)
                .collect::<Vec<_>>()
        };
        for id in badge_ids {
            outcome.merge(self.clear_badge_at(id, ClearReason::Focus, now));
        }
        let attention_ids = {
            let state = borrow_state(&self.inner.state);
            state
                .attention
                .iter()
                .filter(|(_, attention)| {
                    attention.request.surface.0 == surface.0
                        && attention.policy == AttentionClearPolicy::ClearOnSurfaceFocus
                })
                .map(|(id, _)| *id)
                .collect::<Vec<_>>()
        };
        for id in attention_ids {
            outcome.merge(self.clear_attention_at(id, ClearReason::Focus, now));
        }
        outcome
    }

    pub fn sync_lifecycle(&self, lifecycle: &LifecycleState) {
        self.sync_lifecycle_at(lifecycle, Instant::now());
    }

    pub fn sync_lifecycle_at(&self, lifecycle: &LifecycleState, now: Instant) {
        let mut surfaces = lifecycle.surfaces().collect::<Vec<_>>();
        surfaces.sort_by_key(|(surface, _)| surface.0);
        for (surface, snapshot) in surfaces {
            let current = borrow_state(&self.inner.state)
                .surfaces
                .get(&surface)
                .map(|record| record.generation);
            match current {
                None => self.attach_surface_at(surface, snapshot.generation, false, now),
                Some(generation) if snapshot.generation > generation => {
                    let _ = self.recreate_surface_at(surface, snapshot.generation, now);
                }
                Some(_) => {}
            }
            if snapshot.visibility.is_focused() {
                let _ = self.notify_surface_focused_at(surface, now);
            }
        }
    }

    // ---- badges and attention -------------------------------------------

    pub fn apply_badge(&self, request: BadgeRequest) -> Result<BadgeLease, SystemError> {
        self.apply_badge_at(request, Instant::now())
    }

    pub fn apply_badge_at(
        &self,
        request: BadgeRequest,
        now: Instant,
    ) -> Result<BadgeLease, SystemError> {
        if let BadgeScope::Surface {
            surface,
            generation,
        } = request.scope
        {
            let state = borrow_state(&self.inner.state);
            let record = state.surfaces.get(&surface).ok_or_else(|| {
                SystemError::new(ErrorCode::ContextExpired, "Badge surface is not attached")
            })?;
            if record.generation != generation {
                return Err(SystemError::new(
                    ErrorCode::ContextExpired,
                    "Badge surface generation is not live",
                ));
            }
        }
        let id = {
            let mut state = lock_state(&self.inner.state);
            let id = state.next_badge_id;
            state.next_badge_id = state.next_badge_id.saturating_add(1);
            let sequence = state.next_sequence;
            state.next_sequence = state.next_sequence.saturating_add(1);
            state.badges.insert(
                id,
                BadgeRecord {
                    sequence,
                    request: request.clone(),
                },
            );
            id
        };
        let outcome = self.apply_badge_record(&request, now);
        self.emit(TaskPresentationEvent::BadgeApplied {
            surface: request.scope.surface(),
            generation: request.scope.generation(),
            badge: request.badge.clone(),
            policy: request.policy,
        });
        Ok(BadgeLease {
            inner: Rc::downgrade(&self.inner),
            id,
            policy: request.policy,
            initial_outcome: outcome,
        })
    }

    pub fn set_app_badge(
        &self,
        badge: BadgeValue,
        policy: BadgeClearPolicy,
    ) -> Result<BadgeLease, SystemError> {
        self.apply_badge(BadgeRequest::application(badge, policy))
    }

    pub fn set_surface_badge(
        &self,
        surface: SurfaceId,
        generation: u64,
        badge: BadgeValue,
        policy: BadgeClearPolicy,
    ) -> Result<BadgeLease, SystemError> {
        self.apply_badge(BadgeRequest::surface(badge, surface, generation, policy))
    }

    pub fn set_badge(&self, request: BadgeRequest) -> Result<BadgeLease, SystemError> {
        self.apply_badge(request)
    }

    pub fn clear_badge(&self, id: u64) -> TaskPresentationOutcome {
        self.clear_badge_at(id, ClearReason::ExplicitCaller, Instant::now())
    }

    pub fn clear_badge_at(
        &self,
        id: u64,
        reason: ClearReason,
        now: Instant,
    ) -> TaskPresentationOutcome {
        let request = {
            let mut state = lock_state(&self.inner.state);
            state.badges.remove(&id).map(|record| record.request)
        };
        let Some(request) = request else {
            return TaskPresentationOutcome::empty();
        };
        let replacement = {
            let state = borrow_state(&self.inner.state);
            state
                .badges
                .values()
                .filter(|badge| badge.request.scope == request.scope)
                .max_by_key(|badge| badge.sequence)
                .map(|badge| badge.request.clone())
        };
        let outcome = match replacement {
            Some(winner) if !winner.badge.is_clear() => self.apply_badge_record(&winner, now),
            Some(_) | None => self.clear_badge_surface(&request, reason),
        };
        self.emit(TaskPresentationEvent::BadgeCleared {
            surface: request.scope.surface(),
            generation: request.scope.generation(),
            reason,
        });
        outcome
    }

    fn apply_badge_record(&self, request: &BadgeRequest, _now: Instant) -> TaskPresentationOutcome {
        let surface = request.scope.surface().unwrap_or(APPLICATION_SURFACE);
        let generation = request.scope.generation().unwrap_or(0);
        let (precision, error) = match self
            .gate(TaskPresentationOperation::PresentAppBadge, surface)
        {
            Some(precision) => (precision, None),
            None => {
                let result = match request.scope {
                    BadgeScope::Application => self.inner.provider.apply_app_badge(&request.badge),
                    BadgeScope::Surface {
                        surface,
                        generation,
                    } => {
                        self.inner
                            .provider
                            .apply_surface_badge(surface, generation, &request.badge)
                    }
                };
                precision_and_error(result)
            }
        };
        if precision.is_applied() {
            self.sync_applied_badge(request.scope);
        }
        let mut receipt = SurfacePresentationReceipt::new(
            surface,
            generation,
            PresentationSurfaceKind::Badge,
            TaskPresentationOperation::PresentAppBadge,
            precision,
        );
        if let Some(error) = error {
            receipt = receipt.with_error(error);
        }
        TaskPresentationOutcome::from_receipt(receipt)
    }

    fn clear_badge_surface(
        &self,
        request: &BadgeRequest,
        _reason: ClearReason,
    ) -> TaskPresentationOutcome {
        let surface = request.scope.surface().unwrap_or(APPLICATION_SURFACE);
        let generation = request.scope.generation().unwrap_or(0);
        let (precision, error) =
            match self.gate(TaskPresentationOperation::PresentAppBadge, surface) {
                Some(precision) => (precision, None),
                None => {
                    let result = match request.scope {
                        BadgeScope::Application => self.inner.provider.clear_app_badge(),
                        BadgeScope::Surface {
                            surface,
                            generation,
                        } => self.inner.provider.clear_surface_badge(surface, generation),
                    };
                    clear_precision(result)
                }
            };
        if precision.is_applied() {
            self.sync_applied_badge(request.scope);
        }
        let mut receipt = SurfacePresentationReceipt::new(
            surface,
            generation,
            PresentationSurfaceKind::Badge,
            TaskPresentationOperation::PresentAppBadge,
            precision,
        );
        if let Some(error) = error {
            receipt = receipt.with_error(error);
        }
        TaskPresentationOutcome::from_receipt(receipt)
    }

    fn sync_applied_badge(&self, scope: BadgeScope) {
        let targets = {
            let state = borrow_state(&self.inner.state);
            match scope {
                BadgeScope::Application => state
                    .surfaces
                    .iter()
                    .map(|(surface, record)| {
                        (
                            *surface,
                            record.generation,
                            latest_badge_for_surface(&state, *surface, record.generation),
                        )
                    })
                    .collect::<Vec<_>>(),
                BadgeScope::Surface {
                    surface,
                    generation,
                } => vec![(
                    surface,
                    generation,
                    latest_badge_for_surface(&state, surface, generation),
                )],
            }
        };
        let mut state = lock_state(&self.inner.state);
        for (surface, generation, badge) in targets {
            if let Some(record) = state.surfaces.get_mut(&surface)
                && record.generation == generation
            {
                record.applied.badge = badge;
            }
        }
    }

    pub fn request_attention(
        &self,
        request: AttentionRequest,
        policy: AttentionClearPolicy,
    ) -> Result<AttentionLease, SystemError> {
        self.request_attention_at(request, policy, Instant::now())
    }

    pub fn request_attention_at(
        &self,
        request: AttentionRequest,
        policy: AttentionClearPolicy,
        now: Instant,
    ) -> Result<AttentionLease, SystemError> {
        {
            let state = borrow_state(&self.inner.state);
            let record = state.surfaces.get(&request.surface).ok_or_else(|| {
                SystemError::new(
                    ErrorCode::ContextExpired,
                    "Attention surface is not attached",
                )
            })?;
            if record.generation != request.generation {
                return Err(SystemError::new(
                    ErrorCode::ContextExpired,
                    "Attention surface generation is not live",
                ));
            }
        }
        let id = {
            let mut state = lock_state(&self.inner.state);
            let id = state.next_attention_id;
            state.next_attention_id = state.next_attention_id.saturating_add(1);
            state.attention.insert(
                id,
                AttentionRecord {
                    sequence: id,
                    request: request.clone(),
                    policy,
                },
            );
            id
        };
        let outcome = self.apply_attention_record(&request);
        if outcome.has_applied() {
            self.sync_applied_attention(request.surface, request.generation);
        }
        let _ = now;
        self.emit(TaskPresentationEvent::AttentionRequested { request, policy });
        Ok(AttentionLease {
            inner: Rc::downgrade(&self.inner),
            id,
            policy,
            initial_outcome: outcome,
        })
    }

    pub fn request_user_attention(
        &self,
        surface: SurfaceId,
        generation: u64,
        urgency: crate::AttentionUrgency,
        policy: AttentionClearPolicy,
    ) -> Result<AttentionLease, SystemError> {
        self.request_attention(AttentionRequest::new(urgency, surface, generation), policy)
    }

    pub fn clear_attention(&self, id: u64) -> TaskPresentationOutcome {
        self.clear_attention_at(id, ClearReason::ExplicitCaller, Instant::now())
    }

    pub fn clear_attention_at(
        &self,
        id: u64,
        reason: ClearReason,
        now: Instant,
    ) -> TaskPresentationOutcome {
        let request = {
            let mut state = lock_state(&self.inner.state);
            state.attention.remove(&id).map(|record| record.request)
        };
        let Some(request) = request else {
            return TaskPresentationOutcome::empty();
        };
        let replacement = {
            let state = borrow_state(&self.inner.state);
            state
                .attention
                .values()
                .filter(|attention| {
                    attention.request.surface.0 == request.surface.0
                        && attention.request.generation == request.generation
                })
                .max_by_key(|attention| (attention.request.urgency, attention.sequence))
                .map(|attention| attention.request.clone())
        };
        let outcome = match replacement {
            Some(winner) => {
                let outcome = self.apply_attention_record(&winner);
                if outcome.has_applied() {
                    self.sync_applied_attention(request.surface, request.generation);
                }
                outcome
            }
            None => {
                let outcome = self.clear_attention_surface(request.surface, request.generation);
                if outcome.has_applied() {
                    self.sync_applied_attention(request.surface, request.generation);
                }
                outcome
            }
        };
        let _ = now;
        self.emit(TaskPresentationEvent::AttentionCleared {
            surface: request.surface,
            generation: request.generation,
            reason,
        });
        outcome
    }

    fn apply_attention_record(&self, request: &AttentionRequest) -> TaskPresentationOutcome {
        let (precision, error) = match self.gate(
            TaskPresentationOperation::RequestUserAttention,
            request.surface,
        ) {
            Some(precision) => (precision, None),
            None => precision_and_error(self.inner.provider.request_attention(request)),
        };
        let mut receipt = SurfacePresentationReceipt::new(
            request.surface,
            request.generation,
            PresentationSurfaceKind::Attention,
            TaskPresentationOperation::RequestUserAttention,
            precision,
        );
        if let Some(error) = error {
            receipt = receipt.with_error(error);
        }
        TaskPresentationOutcome::from_receipt(receipt)
    }

    fn clear_attention_surface(
        &self,
        surface: SurfaceId,
        generation: u64,
    ) -> TaskPresentationOutcome {
        let (precision, error) =
            match self.gate(TaskPresentationOperation::RequestUserAttention, surface) {
                Some(precision) => (precision, None),
                None => clear_precision(self.inner.provider.clear_attention(surface, generation)),
            };
        let mut receipt = SurfacePresentationReceipt::new(
            surface,
            generation,
            PresentationSurfaceKind::Attention,
            TaskPresentationOperation::RequestUserAttention,
            precision,
        );
        if let Some(error) = error {
            receipt = receipt.with_error(error);
        }
        TaskPresentationOutcome::from_receipt(receipt)
    }

    fn sync_applied_attention(&self, surface: SurfaceId, generation: u64) {
        let attention = {
            let state = borrow_state(&self.inner.state);
            compute_surface_presentation(&state, surface, generation).attention
        };
        let mut state = lock_state(&self.inner.state);
        if let Some(record) = state.surfaces.get_mut(&surface)
            && record.generation == generation
        {
            record.applied.attention = attention;
        }
    }

    fn clear_all_tasks_terminal_badges(&self, now: Instant) {
        let ids = {
            let state = borrow_state(&self.inner.state);
            state
                .badges
                .iter()
                .filter(|(_, badge)| {
                    badge.request.policy == BadgeClearPolicy::ClearOnAllTasksTerminal
                        && !badge_scope_has_active_task(&state, badge.request.scope)
                })
                .map(|(id, _)| *id)
                .collect::<Vec<_>>()
        };
        for id in ids {
            let _ = self.clear_badge_at(id, ClearReason::AllTasksTerminal, now);
        }
    }

    // ---- throttled delivery ---------------------------------------------

    pub fn flush_coalesced(&self) -> TaskPresentationOutcome {
        self.flush_coalesced_at(Instant::now())
    }

    pub fn flush_coalesced_at(&self, now: Instant) -> TaskPresentationOutcome {
        let mut outcome = self.apply_all_surfaces(now, false);
        let ids = borrow_state(&self.inner.state)
            .tasks
            .values()
            .filter(|task| task.pending_notification)
            .map(|task| task.id.clone())
            .collect::<Vec<_>>();
        for id in ids {
            outcome.merge(self.deliver_notification(&id, now, false));
        }
        outcome
    }

    // ---- action dispatch -------------------------------------------------

    pub fn invoke_task_action(
        &self,
        task: &TaskPresentationId,
        action_id: &str,
        context: InvocationContext,
    ) -> TaskActionDispatchOutcome {
        self.dispatch_task_action(task, action_id, context)
    }

    pub fn dispatch_task_action(
        &self,
        task: &TaskPresentationId,
        action_id: &str,
        context: InvocationContext,
    ) -> TaskActionDispatchOutcome {
        let (progress, actions, scope) = {
            let state = borrow_state(&self.inner.state);
            match state.tasks.get(task) {
                Some(record) => (
                    record.progress.clone(),
                    record.actions.clone(),
                    record.scope,
                ),
                None => {
                    let reason = if state.tombstones.contains_key(task) {
                        ActionRejectionReason::TaskTerminal
                    } else {
                        ActionRejectionReason::UnknownTask
                    };
                    drop(state);
                    self.emit(TaskPresentationEvent::ActionRejected {
                        task: Some(task.clone()),
                        action_id: action_id.to_owned(),
                        reason,
                    });
                    return TaskActionDispatchOutcome::Rejected {
                        task: Some(task.clone()),
                        action_id: action_id.to_owned(),
                        rejection: TaskActionRejection::new(reason, action_error(reason)),
                    };
                }
            }
        };
        let Some(action) = actions
            .iter()
            .find(|action| action.action_id.as_str() == action_id)
            .cloned()
        else {
            return self.reject_action(
                Some(task.clone()),
                action_id,
                ActionRejectionReason::UnknownAction,
            );
        };
        if !action.is_enabled_in(&progress) {
            return self.reject_action(
                Some(task.clone()),
                action_id,
                ActionRejectionReason::ActionDisabled,
            );
        }
        if let Some(surface) = scope.surface() {
            if context
                .effective_surface()
                .is_some_and(|target| target.0 != surface.0)
            {
                return self.reject_action(
                    Some(task.clone()),
                    action_id,
                    ActionRejectionReason::SurfaceMismatch,
                );
            }
            if context
                .effective_generation()
                .is_some_and(|generation| generation != scope.generation().unwrap_or(0))
            {
                return self.reject_action(
                    Some(task.clone()),
                    action_id,
                    ActionRejectionReason::GenerationMismatch,
                );
            }
            let state = borrow_state(&self.inner.state);
            let live = state
                .surfaces
                .get(&surface)
                .is_some_and(|record| record.generation == scope.generation().unwrap_or(0));
            if !live {
                drop(state);
                return self.reject_action(
                    Some(task.clone()),
                    action_id,
                    ActionRejectionReason::GenerationMismatch,
                );
            }
        }
        let Some(menu) = self.inner.menu.borrow().clone() else {
            return self.reject_action(
                Some(task.clone()),
                action_id,
                ActionRejectionReason::NoCommandService,
            );
        };
        if !menu.is_registered(&action.command_id) {
            return self.reject_action(
                Some(task.clone()),
                action_id,
                ActionRejectionReason::CommandNotRegistered,
            );
        }
        let mut invocation = context.with_trigger(TriggerSource::Button);
        if let Some(surface) = scope.surface() {
            invocation = invocation.with_surface(surface);
        }
        let request = menu.invoke_command(&action.command_id, action.payload.clone(), invocation);
        self.emit(TaskPresentationEvent::ActionDispatched {
            task: task.clone(),
            action_id: action.action_id.clone(),
            command_id: action.command_id.clone(),
        });
        TaskActionDispatchOutcome::Invoked {
            task: task.clone(),
            action_id: action.action_id,
            command_id: action.command_id,
            request,
        }
    }

    fn reject_action(
        &self,
        task: Option<TaskPresentationId>,
        action_id: &str,
        reason: ActionRejectionReason,
    ) -> TaskActionDispatchOutcome {
        self.emit(TaskPresentationEvent::ActionRejected {
            task: task.clone(),
            action_id: action_id.to_owned(),
            reason,
        });
        TaskActionDispatchOutcome::Rejected {
            task,
            action_id: action_id.to_owned(),
            rejection: TaskActionRejection::new(reason, action_error(reason)),
        }
    }

    /// Route a notification activation back into the task action dispatcher.
    pub fn handle_notification_activation(
        &self,
        notification_id: &NotificationId,
        action_id: Option<&NotificationActionId>,
        context: InvocationContext,
    ) -> Option<TaskActionDispatchOutcome> {
        let Some(action_id) = action_id else {
            self.emit(TaskPresentationEvent::NotificationActionRejected {
                task: None,
                action_id: None,
                reason: ActionRejectionReason::UnknownAction,
            });
            return Some(TaskActionDispatchOutcome::Rejected {
                task: None,
                action_id: String::new(),
                rejection: TaskActionRejection::new(
                    ActionRejectionReason::UnknownAction,
                    action_error(ActionRejectionReason::UnknownAction),
                ),
            });
        };
        let found = {
            let state = borrow_state(&self.inner.state);
            let mut found = None;
            for task in state.tasks.values() {
                let Ok(expected) = crate::notification::notification_id_for_task(&task.id) else {
                    continue;
                };
                if expected != *notification_id {
                    continue;
                }
                for action in &task.actions {
                    if let Ok(candidate) =
                        notification_action_id_for_task(&task.id, &action.action_id)
                        && candidate == *action_id
                    {
                        found = Some((task.id.clone(), action.action_id.as_str().to_owned()));
                        break;
                    }
                }
                if found.is_some() {
                    break;
                }
            }
            found
        };
        let Some((task, action)) = found else {
            self.emit(TaskPresentationEvent::NotificationActionRejected {
                task: None,
                action_id: Some(action_id.as_str().to_owned()),
                reason: ActionRejectionReason::TaskTerminal,
            });
            return Some(TaskActionDispatchOutcome::Rejected {
                task: None,
                action_id: action_id.as_str().to_owned(),
                rejection: TaskActionRejection::new(
                    ActionRejectionReason::TaskTerminal,
                    action_error(ActionRejectionReason::TaskTerminal),
                ),
            });
        };
        Some(self.dispatch_task_action(&task, &action, context))
    }

    /// Drain notification-service observations and route activated actions.
    pub fn pump_notification_activations(
        &self,
        context: InvocationContext,
    ) -> Vec<TaskActionDispatchOutcome> {
        let mut outcomes = Vec::new();
        loop {
            let event = {
                let mut next = {
                    let subscription = self.inner.notification_subscription.borrow();
                    let Some(subscription) = subscription.as_ref() else {
                        return outcomes;
                    };
                    let Ok(next) = subscription.try_next() else {
                        return outcomes;
                    };
                    next
                };
                let waker = Waker::noop();
                let mut task_context = TaskContext::from_waker(waker);
                match Pin::new(&mut next).poll(&mut task_context) {
                    Poll::Ready(Some(EventEnvelope { payload, .. })) => Some(payload),
                    Poll::Ready(None) | Poll::Pending => None,
                }
            };
            let Some(NotificationChannelEvent::Value(event)) = event else {
                if event.is_none() {
                    return outcomes;
                }
                continue;
            };
            if let NotificationEvent::Activated {
                notification_id,
                action_id,
            } = event
                && let Some(outcome) = self.handle_notification_activation(
                    &notification_id,
                    action_id.as_ref(),
                    context.clone(),
                )
            {
                outcomes.push(outcome);
            }
        }
    }

    // ---- internal delivery ----------------------------------------------

    fn apply_all_surfaces(&self, now: Instant, force: bool) -> TaskPresentationOutcome {
        let mut surfaces = borrow_state(&self.inner.state)
            .surfaces
            .iter()
            .map(|(surface, record)| (*surface, record.generation))
            .collect::<Vec<_>>();
        surfaces.sort_by_key(|(surface, _)| surface.0);
        let had_surfaces = !surfaces.is_empty();
        let mut outcome = TaskPresentationOutcome::empty();
        for (surface, generation) in surfaces {
            let plan = {
                let state = borrow_state(&self.inner.state);
                plan_surface(&state, surface, generation, now, force)
            };
            if let Some(plan) = plan {
                outcome.merge(self.execute_surface_plan(plan, now));
            }
        }
        if !had_surfaces {
            let (surface, generation) = borrow_state(&self.inner.state)
                .tasks
                .values()
                .find_map(|task| {
                    task.scope
                        .surface()
                        .map(|surface| (surface, task.scope.generation().unwrap_or(0)))
                })
                .unwrap_or((APPLICATION_SURFACE, 0));
            outcome.receipts.push(SurfacePresentationReceipt::new(
                surface,
                generation,
                PresentationSurfaceKind::Application,
                TaskPresentationOperation::PresentTaskProgress,
                AcknowledgmentPrecision::SkippedUnavailable,
            ));
        }
        let app = {
            let state = borrow_state(&self.inner.state);
            compute_app_presentation(&state)
        };
        self.emit(TaskPresentationEvent::AggregatedAppChanged {
            progress: app.progress,
            active_task_count: app.active_task_count,
        });
        outcome
    }

    fn execute_surface_plan(&self, plan: SurfacePlan, now: Instant) -> TaskPresentationOutcome {
        let mut outcome = TaskPresentationOutcome::empty();
        let mut progress_applied = false;
        let mut badge_applied = false;
        let mut attention_applied = false;
        let mut actions_applied = false;
        match plan.progress {
            ProgressPlan::None => {}
            ProgressPlan::Coalesce => {
                outcome.coalesced = true;
                outcome.receipts.push(SurfacePresentationReceipt::new(
                    plan.surface,
                    plan.generation,
                    PresentationSurfaceKind::Taskbar,
                    TaskPresentationOperation::PresentTaskProgress,
                    AcknowledgmentPrecision::CoalescedInMemory,
                ));
            }
            ProgressPlan::Apply(progress) => {
                let (receipt, applied) =
                    self.apply_progress(plan.surface, plan.generation, progress.as_ref());
                progress_applied = applied;
                outcome.receipts.push(receipt);
            }
        }
        if let Some(badge) = &plan.badge {
            let (receipt, applied) =
                self.apply_badge_for_surface(plan.surface, plan.generation, badge);
            badge_applied = applied;
            outcome.receipts.push(receipt);
        }
        for request in &plan.attention_requests {
            let attention = self.apply_attention_record(request);
            attention_applied |= attention.has_applied();
            outcome.merge(attention);
        }
        if plan.attention_clear {
            let attention = self.clear_attention_surface(plan.surface, plan.generation);
            attention_applied |= attention.has_applied();
            outcome.merge(attention);
        }
        if let Some(actions) = &plan.actions {
            let (receipt, applied) =
                self.apply_actions_for_surface(plan.surface, plan.generation, actions);
            actions_applied = applied;
            outcome.receipts.push(receipt);
        }
        {
            let mut state = lock_state(&self.inner.state);
            if let Some(record) = state.surfaces.get_mut(&plan.surface)
                && record.generation == plan.generation
            {
                record.computed = plan.computed.clone();
                if progress_applied {
                    record.applied.progress = plan.computed.progress.clone();
                    record.applied.lead_task = plan.computed.lead_task.clone();
                    record.applied.lead_priority = plan.computed.lead_priority;
                    record.applied.active_task_count = plan.computed.active_task_count;
                    record.applied.determinate_task_count = plan.computed.determinate_task_count;
                    record.last_progress_at = Some(now);
                    record.pending_progress = false;
                } else if plan.coalesced {
                    record.pending_progress = true;
                }
                if badge_applied {
                    record.applied.badge = plan.computed.badge.clone();
                }
                if attention_applied {
                    record.applied.attention = plan.computed.attention.clone();
                }
                if actions_applied {
                    record.applied.action_buttons = plan.computed.action_buttons.clone();
                }
            }
        }
        self.emit(TaskPresentationEvent::AggregatedSurfaceChanged {
            surface: plan.surface,
            generation: plan.generation,
            progress: plan.computed.progress.clone(),
            active_task_count: plan.computed.active_task_count,
        });
        outcome
    }

    fn apply_progress(
        &self,
        surface: SurfaceId,
        generation: u64,
        progress: Option<&TaskProgressState>,
    ) -> (SurfacePresentationReceipt, bool) {
        let (precision, error) =
            match self.gate(TaskPresentationOperation::PresentTaskProgress, surface) {
                Some(precision) => (precision, None),
                None => precision_and_error(
                    self.inner
                        .provider
                        .apply_surface_progress(surface, generation, progress),
                ),
            };
        let applied = precision.is_applied();
        let mut receipt = SurfacePresentationReceipt::new(
            surface,
            generation,
            PresentationSurfaceKind::Taskbar,
            TaskPresentationOperation::PresentTaskProgress,
            precision,
        );
        if let Some(error) = error {
            receipt = receipt.with_error(error);
        }
        (receipt, applied)
    }

    fn apply_badge_for_surface(
        &self,
        surface: SurfaceId,
        generation: u64,
        badge: &Option<BadgeValue>,
    ) -> (SurfacePresentationReceipt, bool) {
        let (precision, error) = match badge {
            Some(badge) => match self.gate(TaskPresentationOperation::PresentAppBadge, surface) {
                Some(precision) => (precision, None),
                None => precision_and_error(
                    self.inner
                        .provider
                        .apply_surface_badge(surface, generation, badge),
                ),
            },
            None => match self.gate(TaskPresentationOperation::PresentAppBadge, surface) {
                Some(precision) => (precision, None),
                None => {
                    clear_precision(self.inner.provider.clear_surface_badge(surface, generation))
                }
            },
        };
        let applied = precision.is_applied();
        let mut receipt = SurfacePresentationReceipt::new(
            surface,
            generation,
            PresentationSurfaceKind::Badge,
            TaskPresentationOperation::PresentAppBadge,
            precision,
        );
        if let Some(error) = error {
            receipt = receipt.with_error(error);
        }
        (receipt, applied)
    }

    fn apply_actions_for_surface(
        &self,
        surface: SurfaceId,
        generation: u64,
        actions: &[TaskActionButton],
    ) -> (SurfacePresentationReceipt, bool) {
        let (precision, error) =
            match self.gate(TaskPresentationOperation::InvokeTaskAction, surface) {
                Some(precision) => (precision, None),
                None => {
                    if actions.is_empty() {
                        clear_precision(self.inner.provider.clear_actions(surface, generation))
                    } else {
                        precision_and_error(
                            self.inner
                                .provider
                                .apply_actions(surface, generation, actions),
                        )
                    }
                }
            };
        let applied = precision.is_applied();
        let mut receipt = SurfacePresentationReceipt::new(
            surface,
            generation,
            PresentationSurfaceKind::Thumbbar,
            TaskPresentationOperation::InvokeTaskAction,
            precision,
        );
        if let Some(error) = error {
            receipt = receipt.with_error(error);
        }
        (receipt, applied)
    }

    fn gate(
        &self,
        operation: TaskPresentationOperation,
        surface: SurfaceId,
    ) -> Option<AcknowledgmentPrecision> {
        let capability = self.inner.provider.capability(operation, surface);
        if matches!(
            capability.support,
            Support::Unsupported | Support::Unknown | Support::FallbackAvailable
        ) || capability.configuration != Configuration::Ready
            || capability.availability != Availability::Available
        {
            return Some(AcknowledgmentPrecision::SkippedUnavailable);
        }
        if !matches!(
            capability.permission,
            PermissionState::Granted | PermissionState::NotRequired
        ) {
            return Some(AcknowledgmentPrecision::SkippedPermissionDenied);
        }
        None
    }

    fn deliver_notification(
        &self,
        id: &TaskPresentationId,
        now: Instant,
        changed: bool,
    ) -> TaskPresentationOutcome {
        let Some(mirror) = self.inner.mirror.borrow().clone() else {
            return TaskPresentationOutcome::empty();
        };
        let (snapshot, coalesce, defer_empty) = {
            let state = borrow_state(&self.inner.state);
            let Some(record) = state.tasks.get(id) else {
                return TaskPresentationOutcome::empty();
            };
            if !changed && !record.pending_notification {
                return TaskPresentationOutcome::empty();
            }
            let interval = state.throttle.notification_min_interval;
            let elapsed = record
                .last_notification_at
                .map(|last| now.saturating_duration_since(last))
                .unwrap_or(Duration::ZERO);
            let continuous =
                record.progress.kind() == TaskStateKind::Determinate && record.notification_posted;
            let coalesce = changed && continuous && elapsed < interval;
            let defer_empty = !changed && elapsed < interval;
            (record.clone(), coalesce, defer_empty)
        };
        if defer_empty {
            return TaskPresentationOutcome::empty();
        }
        let surface = snapshot.scope.surface().unwrap_or(APPLICATION_SURFACE);
        let generation = snapshot.scope.generation().unwrap_or(0);
        let record = snapshot;
        if coalesce {
            {
                let mut state = lock_state(&self.inner.state);
                if let Some(record) = state.tasks.get_mut(id) {
                    record.pending_notification = true;
                }
            }
            return TaskPresentationOutcome {
                receipts: vec![SurfacePresentationReceipt::new(
                    surface,
                    generation,
                    PresentationSurfaceKind::NotificationMirror,
                    TaskPresentationOperation::MirrorProgressNotification,
                    AcknowledgmentPrecision::CoalescedInMemory,
                )],
                coalesced: true,
            };
        }
        let (receipt, posted) = mirror.submit(
            &record.id,
            surface,
            generation,
            record.title.as_deref(),
            record.body.as_deref(),
            record.channel_id.as_deref(),
            &record.progress,
            &record.actions,
            record.notification_posted,
        );
        let precision = receipt.precision.clone();
        {
            let mut state = lock_state(&self.inner.state);
            if let Some(record) = state.tasks.get_mut(id) {
                if receipt.precision.is_applied() || precision.is_coalesced() {
                    if posted {
                        record.notification_posted = true;
                    }
                    record.last_notification_at = Some(now);
                }
                record.pending_notification = false;
            }
        }
        self.emit(TaskPresentationEvent::MirrorOutcome {
            task: id.clone(),
            precision,
        });
        TaskPresentationOutcome::from_receipt(receipt)
    }

    fn terminate_notification(
        &self,
        record: &TaskRecord,
        state: &TaskProgressState,
    ) -> TaskPresentationOutcome {
        let Some(mirror) = self.inner.mirror.borrow().clone() else {
            return TaskPresentationOutcome::empty();
        };
        if !record.notification_posted {
            return TaskPresentationOutcome::empty();
        }
        let surface = record.scope.surface().unwrap_or(APPLICATION_SURFACE);
        let generation = record.scope.generation().unwrap_or(0);
        let receipt = mirror.terminate(&record.id, surface, generation, state);
        self.emit(TaskPresentationEvent::MirrorOutcome {
            task: record.id.clone(),
            precision: receipt.precision.clone(),
        });
        TaskPresentationOutcome::from_receipt(receipt)
    }

    // ---- test / host helpers --------------------------------------------

    /// Inspect the current raw provider state for deterministic assertions.
    pub fn fake_provider(&self) -> Option<Rc<crate::FakeTaskPresentationProvider>> {
        self.inner
            .provider
            .as_any()
            .downcast_ref::<crate::FakeTaskPresentationProvider>()
            .map(|fake| Rc::new(fake.clone()))
    }
}

fn action_error(reason: ActionRejectionReason) -> SystemError {
    let (code, message) = match reason {
        ActionRejectionReason::UnknownTask => (ErrorCode::ContextExpired, "Task is not active"),
        ActionRejectionReason::TaskTerminal => {
            (ErrorCode::ContextExpired, "Task is already terminal")
        }
        ActionRejectionReason::UnknownAction => (
            ErrorCode::InvalidInput,
            "Action is not declared on this task",
        ),
        ActionRejectionReason::ActionDisabled => (
            ErrorCode::InvalidInput,
            "Action is disabled in the current task state",
        ),
        ActionRejectionReason::SurfaceMismatch => (
            ErrorCode::InvalidInput,
            "Action surface does not match the task",
        ),
        ActionRejectionReason::GenerationMismatch => (
            ErrorCode::ContextExpired,
            "Task surface generation is stale",
        ),
        ActionRejectionReason::NoCommandService => (
            ErrorCode::Unavailable,
            "MenuCommandService is not installed",
        ),
        ActionRejectionReason::CommandNotRegistered => {
            (ErrorCode::Unavailable, "Linked command is not registered")
        }
    };
    SystemError::new(code, message)
}

fn precision_for_error(error: &SystemError) -> AcknowledgmentPrecision {
    match error.code {
        ErrorCode::Unsupported | ErrorCode::Unavailable => {
            AcknowledgmentPrecision::SkippedUnavailable
        }
        ErrorCode::PermissionDenied => AcknowledgmentPrecision::SkippedPermissionDenied,
        code => AcknowledgmentPrecision::Failed { code },
    }
}

fn precision_and_error(
    result: Result<AcknowledgmentPrecision, SystemError>,
) -> (AcknowledgmentPrecision, Option<SystemError>) {
    match result {
        Ok(precision) => (precision, None),
        Err(error) => (precision_for_error(&error), Some(error)),
    }
}

fn clear_precision(
    result: Result<(), SystemError>,
) -> (AcknowledgmentPrecision, Option<SystemError>) {
    match result {
        Ok(()) => (AcknowledgmentPrecision::AppliedConfirmed, None),
        Err(error) => (precision_for_error(&error), Some(error)),
    }
}

struct SurfacePlan {
    surface: SurfaceId,
    generation: u64,
    computed: AggregatedSurfacePresentation,
    progress: ProgressPlan,
    badge: Option<Option<BadgeValue>>,
    attention_requests: Vec<AttentionRequest>,
    attention_clear: bool,
    actions: Option<Vec<TaskActionButton>>,
    coalesced: bool,
}

enum ProgressPlan {
    None,
    Apply(Option<TaskProgressState>),
    Coalesce,
}

fn plan_surface(
    state: &ServiceState,
    surface: SurfaceId,
    generation: u64,
    now: Instant,
    force: bool,
) -> Option<SurfacePlan> {
    let record = state.surfaces.get(&surface)?;
    if record.generation != generation {
        return None;
    }
    let computed = compute_surface_presentation(state, surface, generation);
    let previous = &record.applied;
    let lead_changed = computed.lead_task != previous.lead_task
        || computed.lead_priority != previous.lead_priority;
    let continuous = matches!(
        (&previous.progress, &computed.progress),
        (
            Some(TaskProgressState::Determinate { .. }),
            Some(TaskProgressState::Determinate { .. })
        )
    ) && !lead_changed;
    let elapsed = record
        .last_progress_at
        .map(|last| now.saturating_duration_since(last))
        .unwrap_or(Duration::ZERO);
    let progress = if computed.progress != previous.progress || lead_changed {
        if continuous && !force && elapsed < state.throttle.progress_min_interval {
            ProgressPlan::Coalesce
        } else {
            ProgressPlan::Apply(computed.progress.clone())
        }
    } else {
        ProgressPlan::None
    };
    let coalesced = matches!(progress, ProgressPlan::Coalesce);
    let badge = (computed.badge != previous.badge).then(|| computed.badge.clone());
    let attention_requests = computed
        .attention
        .iter()
        .filter(|request| !previous.attention.contains(request))
        .cloned()
        .collect::<Vec<_>>();
    let attention_clear = previous
        .attention
        .iter()
        .any(|request| !computed.attention.contains(request));
    let actions = (computed.action_buttons != previous.action_buttons)
        .then(|| computed.action_buttons.clone());
    Some(SurfacePlan {
        surface,
        generation,
        computed,
        progress,
        badge,
        attention_requests,
        attention_clear,
        actions,
        coalesced,
    })
}

fn compute_surface_presentation(
    state: &ServiceState,
    surface: SurfaceId,
    generation: u64,
) -> AggregatedSurfacePresentation {
    let mut entries: Vec<(&TaskRecord, AggregateInput<'_>)> = Vec::new();
    for task in state.tasks.values() {
        if task.progress.is_terminal() {
            continue;
        }
        let include = match task.scope {
            TaskScope::Application => true,
            TaskScope::Surface {
                surface: scoped_surface,
                generation: scoped_generation,
            } => scoped_surface.0 == surface.0 && scoped_generation == generation,
        };
        if include {
            entries.push((
                task,
                AggregateInput {
                    id: &task.id,
                    sequence: task.sequence,
                    priority: task.priority,
                    state: &task.progress,
                },
            ));
        }
    }
    let inputs = entries.iter().map(|(_, input)| *input).collect::<Vec<_>>();
    let lead = select_lead(inputs.iter().copied());
    let progress = aggregate_progress(inputs.iter().copied(), state.aggregate_mode);
    let action_pairs = entries
        .iter()
        .map(|(task, input)| (input, task.actions.as_slice()))
        .collect::<Vec<_>>();
    let action_buttons = aggregate_actions(action_pairs);
    let determinate_task_count = inputs
        .iter()
        .filter(|input| input.state.kind() == TaskStateKind::Determinate)
        .count();
    let badge = latest_badge_for_surface(state, surface, generation);
    let attention = state
        .attention
        .values()
        .filter(|attention| {
            attention.request.surface.0 == surface.0 && attention.request.generation == generation
        })
        .map(|attention| attention.request.clone())
        .collect::<Vec<_>>();
    AggregatedSurfacePresentation {
        surface,
        generation,
        lead_task: lead.map(|lead| lead.id.clone()),
        lead_priority: lead.map(|lead| lead.priority),
        progress,
        active_task_count: inputs.len(),
        determinate_task_count,
        action_buttons,
        badge,
        attention,
    }
}

fn latest_badge_for_surface(
    state: &ServiceState,
    surface: SurfaceId,
    generation: u64,
) -> Option<BadgeValue> {
    state
        .badges
        .values()
        .filter(|badge| match badge.request.scope {
            BadgeScope::Application => true,
            BadgeScope::Surface {
                surface: scoped,
                generation: scoped_generation,
            } => scoped.0 == surface.0 && scoped_generation == generation,
        })
        .max_by_key(|badge| badge.sequence)
        .and_then(|badge| match &badge.request.badge {
            BadgeValue::Clear => None,
            badge => Some(badge.clone()),
        })
}

fn compute_app_presentation(state: &ServiceState) -> AggregatedAppPresentation {
    let mut entries: Vec<(&TaskRecord, AggregateInput<'_>)> = Vec::new();
    for task in state.tasks.values() {
        if task.progress.is_terminal() {
            continue;
        }
        entries.push((
            task,
            AggregateInput {
                id: &task.id,
                sequence: task.sequence,
                priority: task.priority,
                state: &task.progress,
            },
        ));
    }
    let inputs = entries.iter().map(|(_, input)| *input).collect::<Vec<_>>();
    let lead = select_lead(inputs.iter().copied());
    let progress = aggregate_progress(inputs.iter().copied(), state.aggregate_mode);
    let action_pairs = entries
        .iter()
        .map(|(task, input)| (input, task.actions.as_slice()))
        .collect::<Vec<_>>();
    let action_buttons = aggregate_actions(action_pairs);
    let determinate_task_count = inputs
        .iter()
        .filter(|input| input.state.kind() == TaskStateKind::Determinate)
        .count();
    let badge = state
        .badges
        .values()
        .filter(|badge| badge.request.scope == BadgeScope::Application)
        .max_by_key(|badge| badge.sequence)
        .and_then(|badge| match &badge.request.badge {
            BadgeValue::Clear => None,
            badge => Some(badge.clone()),
        });
    let mut surfaces = state
        .surfaces
        .keys()
        .map(|surface| {
            compute_surface_presentation(state, *surface, state.surfaces[surface].generation)
        })
        .collect::<Vec<_>>();
    surfaces.sort_by_key(|surface| surface.surface.0);
    AggregatedAppPresentation {
        lead_task: lead.map(|lead| lead.id.clone()),
        lead_priority: lead.map(|lead| lead.priority),
        progress,
        active_task_count: inputs.len(),
        determinate_task_count,
        action_buttons,
        badge,
        surfaces,
    }
}

fn badge_scope_has_active_task(state: &ServiceState, scope: BadgeScope) -> bool {
    state.tasks.values().any(|task| match scope {
        BadgeScope::Application => true,
        BadgeScope::Surface {
            surface,
            generation,
        } => match task.scope {
            TaskScope::Application => true,
            TaskScope::Surface {
                surface: scoped_surface,
                generation: scoped_generation,
            } => scoped_surface.0 == surface.0 && scoped_generation == generation,
        },
    })
}

/// RAII task presentation contribution.
pub struct TaskLease {
    inner: Weak<ServiceInner>,
    id: TaskPresentationId,
    sequence: Option<u64>,
    initial_outcome: TaskPresentationOutcome,
}

impl std::fmt::Debug for TaskLease {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("TaskLease")
            .field("id", &self.id)
            .field("active", &self.sequence.is_some())
            .finish()
    }
}

impl TaskLease {
    pub fn id(&self) -> &TaskPresentationId {
        &self.id
    }

    pub fn is_active(&self) -> bool {
        self.sequence.is_some()
    }

    pub fn initial_outcome(&self) -> &TaskPresentationOutcome {
        &self.initial_outcome
    }

    fn service(&self) -> Result<TaskPresentationService, SystemError> {
        self.inner
            .upgrade()
            .map(|inner| TaskPresentationService { inner })
            .ok_or_else(|| SystemError::new(ErrorCode::Unavailable, "Task service is disposed"))
    }

    fn active_sequence(&self) -> Result<u64, SystemError> {
        self.sequence.ok_or_else(|| {
            SystemError::new(ErrorCode::ContextExpired, "Task lease is no longer active")
        })
    }

    pub fn update(
        &self,
        progress: TaskProgressState,
    ) -> Result<TaskPresentationOutcome, SystemError> {
        self.update_at(progress, Instant::now())
    }

    pub fn update_at(
        &self,
        progress: TaskProgressState,
        now: Instant,
    ) -> Result<TaskPresentationOutcome, SystemError> {
        let sequence = self.active_sequence()?;
        self.service()?
            .update_task_checked_at(&self.id, progress, Some(sequence), now)
    }

    pub fn rebind(&self, scope: TaskScope) -> Result<TaskPresentationOutcome, SystemError> {
        self.rebind_at(scope, Instant::now())
    }

    pub fn rebind_at(
        &self,
        scope: TaskScope,
        now: Instant,
    ) -> Result<TaskPresentationOutcome, SystemError> {
        let sequence = self.active_sequence()?;
        self.service()?
            .rebind_task_checked_at(&self.id, scope, Some(sequence), now)
    }

    pub fn complete(&mut self) -> Result<TaskPresentationOutcome, SystemError> {
        self.complete_at(Instant::now())
    }

    pub fn complete_at(&mut self, now: Instant) -> Result<TaskPresentationOutcome, SystemError> {
        let sequence = self.active_sequence()?;
        let service = self.service()?;
        let outcome = service.complete_task_checked_at(&self.id, now, Some(sequence))?;
        self.sequence = None;
        Ok(outcome)
    }

    pub fn cancel(&mut self) -> Result<TaskPresentationOutcome, SystemError> {
        self.cancel_at(Instant::now())
    }

    pub fn cancel_at(&mut self, now: Instant) -> Result<TaskPresentationOutcome, SystemError> {
        let sequence = self.active_sequence()?;
        let service = self.service()?;
        let outcome = service.cancel_task_checked_at(&self.id, now, Some(sequence))?;
        self.sequence = None;
        Ok(outcome)
    }

    pub fn complete_with_business_result<T>(
        &mut self,
        business: BusinessTaskCompletion<T>,
    ) -> Result<TaskCompletionRecord<T>, SystemError> {
        self.complete_with_business_result_at(business, Instant::now())
    }

    pub fn complete_with_business_result_at<T>(
        &mut self,
        business: BusinessTaskCompletion<T>,
        now: Instant,
    ) -> Result<TaskCompletionRecord<T>, SystemError> {
        let sequence = self.active_sequence()?;
        let service = self.service()?;
        let record = service.complete_task_with_business_result_checked_at(
            &self.id,
            business,
            now,
            Some(sequence),
        )?;
        self.sequence = None;
        Ok(record)
    }

    pub fn release(&mut self) -> bool {
        self.release_at(Instant::now())
    }

    pub fn release_at(&mut self, now: Instant) -> bool {
        let Some(sequence) = self.sequence.take() else {
            return false;
        };
        let Some(service) = self.inner.upgrade() else {
            return false;
        };
        TaskPresentationService { inner: service }
            .release_task_at(&self.id, sequence, ClearReason::LeaseReleased, now)
            .is_some()
    }
}

impl Drop for TaskLease {
    fn drop(&mut self) {
        if let Some(sequence) = self.sequence.take()
            && let Some(service) = self.inner.upgrade()
        {
            let _ = TaskPresentationService { inner: service }.release_task_at(
                &self.id,
                sequence,
                ClearReason::LeaseReleased,
                Instant::now(),
            );
        }
    }
}

/// RAII badge contribution.
pub struct BadgeLease {
    inner: Weak<ServiceInner>,
    id: u64,
    policy: BadgeClearPolicy,
    initial_outcome: TaskPresentationOutcome,
}

impl std::fmt::Debug for BadgeLease {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("BadgeLease")
            .field("id", &self.id)
            .field("policy", &self.policy)
            .finish_non_exhaustive()
    }
}

impl BadgeLease {
    pub fn id(&self) -> u64 {
        self.id
    }

    pub fn policy(&self) -> BadgeClearPolicy {
        self.policy
    }

    pub fn initial_outcome(&self) -> &TaskPresentationOutcome {
        &self.initial_outcome
    }

    pub fn release(&mut self) -> bool {
        self.release_at(Instant::now())
    }

    /// Explicit caller release clears the badge regardless of its policy.
    pub fn release_at(&mut self, now: Instant) -> bool {
        let Some(service) = self.inner.upgrade() else {
            return false;
        };
        let existed = borrow_state(&service.state).badges.contains_key(&self.id);
        if existed {
            let _ = TaskPresentationService { inner: service }.clear_badge_at(
                self.id,
                ClearReason::LeaseReleased,
                now,
            );
        }
        existed
    }
}

impl Drop for BadgeLease {
    fn drop(&mut self) {
        if self.policy != BadgeClearPolicy::LeaseScoped {
            return;
        }
        if let Some(service) = self.inner.upgrade() {
            let _ = TaskPresentationService { inner: service }.clear_badge_at(
                self.id,
                ClearReason::LeaseReleased,
                Instant::now(),
            );
        }
    }
}

/// RAII attention contribution.
pub struct AttentionLease {
    inner: Weak<ServiceInner>,
    id: u64,
    policy: AttentionClearPolicy,
    initial_outcome: TaskPresentationOutcome,
}

impl std::fmt::Debug for AttentionLease {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("AttentionLease")
            .field("id", &self.id)
            .field("policy", &self.policy)
            .finish_non_exhaustive()
    }
}

impl AttentionLease {
    pub fn id(&self) -> u64 {
        self.id
    }

    pub fn policy(&self) -> AttentionClearPolicy {
        self.policy
    }

    pub fn initial_outcome(&self) -> &TaskPresentationOutcome {
        &self.initial_outcome
    }

    pub fn release(&mut self) -> bool {
        self.release_at(Instant::now())
    }

    /// Explicit caller release clears the request regardless of its policy.
    pub fn release_at(&mut self, now: Instant) -> bool {
        let Some(service) = self.inner.upgrade() else {
            return false;
        };
        let existed = borrow_state(&service.state)
            .attention
            .contains_key(&self.id);
        if existed {
            let _ = TaskPresentationService { inner: service }.clear_attention_at(
                self.id,
                ClearReason::LeaseReleased,
                now,
            );
        }
        existed
    }
}

impl Drop for AttentionLease {
    fn drop(&mut self) {
        if self.policy != AttentionClearPolicy::LeaseScoped {
            return;
        }
        if let Some(service) = self.inner.upgrade() {
            let _ = TaskPresentationService { inner: service }.clear_attention_at(
                self.id,
                ClearReason::LeaseReleased,
                Instant::now(),
            );
        }
    }
}
