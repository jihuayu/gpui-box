//! Invocation task state, terminal lifecycle events and bounded broadcasting.

use crate::{
    CancelReason, CommandId, CommandPayload, CommandTaskSnapshot, CommandTaskState, Completion,
    ContributionScopeId, DuplicateCommandDiagnostic, ErrorCode, ExecutionDomain, InvocationId,
    ServiceResult, SurfaceId, SystemError, TriggerSource,
};
use gpui_system::{EventSender, EventSubscription, event_channel};
use std::{
    sync::{
        Arc, Mutex, PoisonError,
        atomic::{AtomicUsize, Ordering},
    },
    time::Instant,
};

/// Bounded event capacity per subscriber.
pub const MENU_EVENT_CAPACITY: usize = 64;

/// Errors a deferred completion can carry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum DeferredError {
    Cancelled(CancelReason),
    Failed(SystemError),
}

/// Terminal lifecycle event stream. `CommandFinished` is emitted only after the
/// worker/future/completion port actually resolves.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MenuCommandEvent {
    CommandStarted(CommandTaskSnapshot),
    CommandFinished(CommandTaskSnapshot),
    DuplicateCommandRejected(DuplicateCommandDiagnostic),
}

/// Thread-safe subscriber fan-out. Worker threads may emit terminal events
/// without retaining a reference to the UI-local service.
pub(crate) struct Broadcaster {
    senders: Mutex<Vec<EventSender<MenuCommandEvent>>>,
}

impl Default for Broadcaster {
    fn default() -> Self {
        Self {
            senders: Mutex::new(Vec::new()),
        }
    }
}

impl Broadcaster {
    pub(crate) fn add(&self, sender: EventSender<MenuCommandEvent>) {
        lock(&self.senders).push(sender);
    }

    pub(crate) fn send(&self, event: MenuCommandEvent) {
        let senders = lock(&self.senders).clone();
        for sender in senders {
            sender.send(event.clone());
        }
    }
}

fn lock<T>(value: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    value.lock().unwrap_or_else(PoisonError::into_inner)
}

pub(crate) type TerminalCallback = Box<dyn FnOnce(ServiceResult<CommandPayload>) + Send + 'static>;

struct TaskInner {
    state: CommandTaskState,
    callback: Option<TerminalCallback>,
}

pub(crate) struct TaskSpec {
    pub id: InvocationId,
    pub command_id: CommandId,
    pub domain: ExecutionDomain,
    pub trigger: TriggerSource,
    pub scope: Option<ContributionScopeId>,
    pub registration: Option<u64>,
    pub surface: Option<SurfaceId>,
    pub deadline: Option<Instant>,
    pub token: gpui_system::CancellationToken,
    pub events: Arc<Broadcaster>,
    pub in_flight: Arc<AtomicUsize>,
    pub background_slot: Option<Arc<AtomicUsize>>,
    pub callback: TerminalCallback,
}

/// Shared task state readable from the UI thread and from worker completions.
pub(crate) struct TaskShared {
    id: InvocationId,
    command_id: CommandId,
    domain: ExecutionDomain,
    trigger: TriggerSource,
    scope: Option<ContributionScopeId>,
    registration: Option<u64>,
    surface: Option<SurfaceId>,
    deadline: Option<Instant>,
    token: gpui_system::CancellationToken,
    events: Arc<Broadcaster>,
    in_flight: Arc<AtomicUsize>,
    background_slot: Option<Arc<AtomicUsize>>,
    inner: Mutex<TaskInner>,
}

impl TaskShared {
    pub(crate) fn new(spec: TaskSpec) -> Self {
        Self {
            id: spec.id,
            command_id: spec.command_id,
            domain: spec.domain,
            trigger: spec.trigger,
            scope: spec.scope,
            registration: spec.registration,
            surface: spec.surface,
            deadline: spec.deadline,
            token: spec.token,
            events: spec.events,
            in_flight: spec.in_flight,
            background_slot: spec.background_slot,
            inner: Mutex::new(TaskInner {
                state: CommandTaskState::Running,
                callback: Some(spec.callback),
            }),
        }
    }

    pub(crate) fn id(&self) -> InvocationId {
        self.id
    }

    pub(crate) fn command_id(&self) -> &CommandId {
        &self.command_id
    }

    pub(crate) fn trigger(&self) -> TriggerSource {
        self.trigger
    }

    pub(crate) fn surface(&self) -> Option<SurfaceId> {
        self.surface
    }

    pub(crate) fn scope(&self) -> Option<ContributionScopeId> {
        self.scope
    }

    pub(crate) fn registration(&self) -> Option<u64> {
        self.registration
    }

    pub(crate) fn token(&self) -> &gpui_system::CancellationToken {
        &self.token
    }

    pub(crate) const fn deadline(&self) -> Option<Instant> {
        self.deadline
    }

    pub(crate) fn snapshot(&self) -> CommandTaskSnapshot {
        CommandTaskSnapshot {
            id: self.id,
            command_id: self.command_id.clone(),
            domain: self.domain,
            trigger: self.trigger,
            scope: self.scope,
            surface: self.surface,
            state: lock(&self.inner).state,
        }
    }

    pub(crate) fn state(&self) -> CommandTaskState {
        lock(&self.inner).state
    }

    pub(crate) fn is_running(&self) -> bool {
        self.state() == CommandTaskState::Running
    }

    pub(crate) fn announce_started(&self) {
        self.events
            .send(MenuCommandEvent::CommandStarted(self.snapshot()));
    }

    /// Terminal transition wins exactly once. Counters and the completion port
    /// are released only by the winner.
    pub(crate) fn finish_terminal(&self, result: ServiceResult<CommandPayload>) -> bool {
        let callback = {
            let mut inner = lock(&self.inner);
            if inner.state != CommandTaskState::Running {
                return false;
            }
            inner.state = state_for(&result);
            inner.callback.take()
        };
        release_counter(&self.in_flight);
        if let Some(slot) = &self.background_slot {
            release_counter(slot);
        }
        self.events
            .send(MenuCommandEvent::CommandFinished(self.snapshot()));
        if let Some(callback) = callback {
            callback(result);
        }
        true
    }

    /// Apply cancellation/deadline precedence before committing a result.
    pub(crate) fn finish_with_checks(
        &self,
        result: ServiceResult<CommandPayload>,
        now: Instant,
    ) -> bool {
        if matches!(result, Ok(Completion::Cancelled(_))) {
            return self.finish_terminal(result);
        }
        if let Some(reason) = self.token.reason() {
            return self.finish_terminal(Ok(Completion::Cancelled(reason)));
        }
        if self.deadline.is_some_and(|deadline| now >= deadline) {
            return self.finish_terminal(Err(timed_out()));
        }
        self.finish_terminal(result)
    }

    pub(crate) fn finish_error(&self, error: SystemError) -> bool {
        self.finish_with_checks(Err(error), Instant::now())
    }

    pub(crate) fn cancel(&self, reason: CancelReason) -> bool {
        let finished = self.finish_terminal(Ok(Completion::Cancelled(reason)));
        self.token.cancel(reason);
        finished
    }

    pub(crate) fn expire(&self, now: Instant) -> bool {
        if self.deadline.is_none_or(|deadline| now < deadline) {
            return false;
        }
        let finished = self.finish_terminal(Err(timed_out()));
        if finished {
            self.token.cancel(CancelReason::ProcessPolicy);
        }
        finished
    }
}

fn release_counter(counter: &AtomicUsize) {
    let _ = counter.try_update(Ordering::AcqRel, Ordering::Acquire, |value| {
        Some(value.saturating_sub(1))
    });
}

fn state_for(result: &ServiceResult<CommandPayload>) -> CommandTaskState {
    match result {
        Ok(Completion::Completed(_)) => CommandTaskState::Completed,
        Ok(Completion::Cancelled(reason)) => CommandTaskState::Cancelled(*reason),
        Err(error) => CommandTaskState::Failed(error.code),
    }
}

pub(crate) fn timed_out() -> SystemError {
    SystemError::new(ErrorCode::TimedOut, "Command deadline elapsed")
}

/// Create a bounded event stream wired to the shared broadcaster.
pub(crate) fn subscribe_events(
    broadcaster: &Arc<Broadcaster>,
) -> EventSubscription<MenuCommandEvent> {
    let (sender, subscription) = event_channel(MENU_EVENT_CAPACITY, || {});
    broadcaster.add(sender);
    subscription
}
