use crate::provider::{PermissionPrompt, PermissionPromptCompletion, PermissionProvider};
use crate::{
    PermissionChange, PermissionKind, PermissionOperation, PermissionOutcome,
    PermissionReconciliation, PermissionRequest, PermissionSnapshot, PermissionStatus,
    SettingsHandoff, SettingsTarget,
};
use gpui_system::{
    CancelReason, Capability, Completion, ErrorCode, EventSender, EventSubscription,
    OperationContext, Request, RetryAdvice, SystemError, SystemServices, event_channel,
};
use std::{
    cell::{Cell, RefCell},
    collections::{BTreeMap, BTreeSet},
    rc::Rc,
    thread::{self, ThreadId},
    time::Instant,
};

const EVENT_CAPACITY: usize = 16;

type Subscriber = (Rc<Cell<bool>>, EventSender<PermissionChange>);

#[derive(Default)]
struct ServiceState {
    known: BTreeMap<PermissionKind, PermissionStatus>,
    active: Option<ActivePrompt>,
    subscribers: Vec<Subscriber>,
    next_prompt: u64,
}

/// Snapshot each live subscriber's next local sequence while the service state
/// is borrowed. Sends must happen after the caller drops the `RefCell` guard so
/// a waker can safely re-enter `PermissionService`.
fn collect_deliveries(
    state: &mut ServiceState,
    kind: PermissionKind,
    previous: PermissionStatus,
    current: PermissionStatus,
) -> Vec<(EventSender<PermissionChange>, PermissionChange)> {
    state.subscribers.retain(|(live, _)| live.get());
    state
        .subscribers
        .iter()
        .map(|(_, sender)| {
            let change = PermissionChange {
                kind,
                previous,
                current,
                revision: sender.revision().saturating_add(1),
            };
            (sender.clone(), change)
        })
        .collect()
}

struct ActivePrompt {
    token: u64,
    kind: PermissionKind,
    context: OperationContext,
    thread: ThreadId,
}

pub(crate) struct ServiceInner {
    provider: Rc<dyn PermissionProvider>,
    state: RefCell<ServiceState>,
}

impl ServiceInner {
    /// Accept exactly one authoritative completion for the active native prompt.
    /// Cancelled contexts, advanced generations and disposed owners release the
    /// prompt lane without delivering a stale success.
    pub(crate) fn finish_prompt(
        &self,
        token: u64,
        snapshot: PermissionSnapshot,
    ) -> Option<PermissionOutcome> {
        let mut state = self.state.borrow_mut();
        let active = state.active.take()?;
        if active.token != token {
            state.active = Some(active);
            return None;
        }
        if active.kind != snapshot.kind {
            return None;
        }
        if active.context.cancellation.reason().is_some()
            || active.context.check_at(Instant::now()).is_err()
        {
            return None;
        }
        let previous = state
            .known
            .insert(snapshot.kind, snapshot.status)
            .unwrap_or(snapshot.status);
        let deliveries = if previous != snapshot.status {
            collect_deliveries(&mut state, snapshot.kind, previous, snapshot.status)
        } else {
            Vec::new()
        };
        drop(state);
        for (sender, change) in deliveries {
            sender.send(change);
        }
        Some(PermissionOutcome {
            kind: snapshot.kind,
            previous,
            status: snapshot.status,
            prompted: true,
            changed: previous != snapshot.status,
            revision: snapshot.revision,
        })
    }

    /// Release a prompt lane owned by this token. Returns false for a stale
    /// token, so late callbacks cannot complete a newer request.
    pub(crate) fn abort_prompt(&self, token: u64) -> bool {
        let mut state = self.state.borrow_mut();
        if state
            .active
            .as_ref()
            .is_some_and(|active| active.token == token)
        {
            state.active = None;
            return true;
        }
        false
    }
}

impl Drop for ServiceInner {
    fn drop(&mut self) {
        if let Some(active) = self.state.get_mut().active.take() {
            active.context.cancellation.cancel(CancelReason::Caller);
        }
    }
}

/// UI-local permission service. It owns one provider and serializes interactive
/// prompts; cloning shares the same state and native prompt lane.
#[derive(Clone)]
pub struct PermissionService(Rc<ServiceInner>);

impl PermissionService {
    pub fn new(provider: Rc<dyn PermissionProvider>) -> Self {
        Self(Rc::new(ServiceInner {
            provider,
            state: RefCell::new(ServiceState::default()),
        }))
    }

    pub fn from_provider(provider: impl PermissionProvider + 'static) -> Self {
        Self::new(Rc::new(provider))
    }

    pub fn provider(&self) -> &Rc<dyn PermissionProvider> {
        &self.0.provider
    }

    /// Register this concrete wrapper in `SystemServices`. Install overrides
    /// before starting operations.
    pub fn install(self, services: &mut SystemServices) -> Result<(), SystemError> {
        services.register(self)
    }

    pub fn install_cloned(&self, services: &mut SystemServices) -> Result<(), SystemError> {
        services.register(self.clone())
    }

    /// A capability snapshot for one operation. Provider implementations must
    /// not prompt or perform a settings handoff while producing this snapshot.
    pub fn capability(&self, operation: PermissionOperation) -> Capability<PermissionOperation> {
        self.0.provider.capability(operation)
    }

    /// Capability snapshots for the built-in kinds and standard settings
    /// destinations. This method performs no native prompt.
    pub fn capabilities(&self) -> Vec<Capability<PermissionOperation>> {
        let mut operations: Vec<PermissionOperation> = PermissionKind::ALL
            .iter()
            .flat_map(|kind| {
                let kind = *kind;
                [
                    PermissionOperation::Query(kind),
                    PermissionOperation::Request(kind),
                ]
            })
            .collect();
        operations.push(PermissionOperation::OpenSettings(
            SettingsTarget::AppDetails,
        ));
        operations.push(PermissionOperation::OpenSettings(
            SettingsTarget::NotificationSettings,
        ));
        operations
            .into_iter()
            .map(|operation| self.capability(operation))
            .collect()
    }

    /// Create an independent bounded observation stream.
    ///
    /// Each UI entity or view may subscribe to the shared service. A stream's
    /// cleanup marks its slot dead; dead slots are pruned on the next
    /// subscription, revision lookup or emitted change.
    pub fn subscribe(&self) -> EventSubscription<PermissionChange> {
        let live = Rc::new(Cell::new(true));
        let cleanup = Rc::clone(&live);
        let (sender, subscription) = event_channel(EVENT_CAPACITY, move || cleanup.set(false));
        let mut state = self.0.state.borrow_mut();
        state.subscribers.retain(|(live, _)| live.get());
        state.subscribers.push((live, sender));
        subscription
    }

    /// Highest observation sequence across live subscribers. A reconciliation
    /// snapshot taken at this revision is safe to acknowledge on any subscriber,
    /// including one whose local sequence started later and is therefore lower.
    pub fn revision(&self) -> u64 {
        let mut state = self.0.state.borrow_mut();
        state.subscribers.retain(|(live, _)| live.get());
        state
            .subscribers
            .iter()
            .map(|(_, sender)| sender.revision())
            .max()
            .unwrap_or(0)
    }

    /// True while an interactive prompt owns the native prompt lane.
    pub fn active_prompt(&self) -> Option<PermissionKind> {
        self.clear_stale_prompt();
        self.0
            .state
            .borrow()
            .active
            .as_ref()
            .map(|active| active.kind)
    }

    /// Probe authoritative status at execution time. No native prompt is
    /// presented, even when the status is `NotDetermined`.
    pub fn query(&self, kind: PermissionKind, cx: OperationContext) -> Request<PermissionSnapshot> {
        let (request, callback) = gpui_system::request(cx);
        let result = self.0.provider.query(kind).map(|snapshot| {
            self.observe(snapshot.clone());
            Completion::Completed(snapshot)
        });
        callback.complete(result);
        request
    }

    /// Contextually request one permission.
    ///
    /// `NotRequired`, `Granted` and permanently denied states resolve without
    /// an interactive prompt or a UI context. Every promptable state validates
    /// `cx.require_ui()` on the owning UI thread before taking the prompt lane.
    pub fn request_permission(
        &self,
        prompt: PermissionRequest,
        cx: OperationContext,
    ) -> Request<PermissionOutcome> {
        let (request, callback) = gpui_system::request(cx.clone());
        let kind = prompt.kind();
        let snapshot = match self.0.provider.query(kind) {
            Ok(snapshot) => snapshot,
            Err(error) => {
                callback.complete(Err(error));
                return request;
            }
        };
        let status = snapshot.status;
        let revision = snapshot.revision;
        if !status.requires_prompt() {
            let change = self.observe(snapshot);
            let outcome = PermissionOutcome {
                kind,
                previous: change.as_ref().map_or(status, |change| change.previous),
                status,
                prompted: false,
                changed: change.is_some(),
                revision,
            };
            callback.complete(Ok(Completion::Completed(outcome)));
            return request;
        }
        if let Err(error) = cx.require_ui().and_then(|_| cx.check_at(Instant::now())) {
            callback.complete(Err(error));
            return request;
        }
        if let Some(reason) = cx.cancellation.reason() {
            callback.complete(Ok(Completion::Cancelled(reason)));
            return request;
        }
        self.seed_prompt_status(kind, status);
        let token = match self.begin_prompt(kind, &cx) {
            Ok(token) => token,
            Err(error) => {
                callback.complete(Err(error));
                return request;
            }
        };
        let completion =
            PermissionPromptCompletion::new(callback, Rc::downgrade(&self.0), token, kind);
        if let Err(error) = self.0.provider.request_permission(
            PermissionPrompt {
                kind,
                request: request.id(),
            },
            completion.clone(),
        ) {
            completion.fail(error);
        }
        request
    }

    /// Present the platform settings destination. A UI context owned by the
    /// current generation is mandatory. Success is a handoff receipt only; it
    /// never asserts that the user toggled the permission.
    pub fn open_settings(
        &self,
        target: SettingsTarget,
        cx: OperationContext,
    ) -> Request<SettingsHandoff> {
        let (request, callback) = gpui_system::request(cx.clone());
        if let Err(error) = cx.require_ui().and_then(|_| cx.check_at(Instant::now())) {
            callback.complete(Err(error));
            return request;
        }
        if let Some(reason) = cx.cancellation.reason() {
            callback.complete(Ok(Completion::Cancelled(reason)));
            return request;
        }
        let result = self.0.provider.open_settings(target);
        callback.complete(result.map(Completion::Completed));
        request
    }

    /// Re-query every known permission after returning from OS settings or app
    /// resume. Changed statuses are emitted on the scoped stream and returned.
    pub fn reconcile_on_resume(&self) -> Result<PermissionReconciliation, SystemError> {
        let provider = Rc::clone(&self.0.provider);
        let kinds = {
            let state = self.0.state.borrow();
            let mut kinds: BTreeSet<PermissionKind> = state.known.keys().copied().collect();
            kinds.extend(provider.tracked_kinds());
            kinds
        };
        let mut changes = Vec::new();
        for kind in kinds {
            let snapshot = provider.query(kind)?;
            if let Some(change) = self.observe(snapshot) {
                changes.push(change);
            }
        }
        Ok(PermissionReconciliation {
            changes,
            revision: self.revision(),
        })
    }

    fn observe(&self, snapshot: PermissionSnapshot) -> Option<PermissionChange> {
        let mut state = self.0.state.borrow_mut();
        let previous = state
            .known
            .insert(snapshot.kind, snapshot.status)
            .unwrap_or(snapshot.status);
        if previous == snapshot.status {
            return None;
        }
        let deliveries = collect_deliveries(&mut state, snapshot.kind, previous, snapshot.status);
        drop(state);
        let revision = deliveries
            .iter()
            .map(|(_, change)| change.revision)
            .max()
            .unwrap_or(0);
        for (sender, change) in deliveries {
            sender.send(change);
        }
        Some(PermissionChange {
            kind: snapshot.kind,
            previous,
            current: snapshot.status,
            revision,
        })
    }

    fn seed_prompt_status(&self, kind: PermissionKind, status: PermissionStatus) {
        let mut state = self.0.state.borrow_mut();
        state.known.insert(kind, status);
    }

    fn begin_prompt(
        &self,
        kind: PermissionKind,
        cx: &OperationContext,
    ) -> Result<u64, SystemError> {
        cx.check_at(Instant::now())?;
        self.clear_stale_prompt();
        let mut state = self.0.state.borrow_mut();
        if state.active.is_some() {
            let mut error =
                SystemError::new(ErrorCode::Busy, "A permission prompt is already active");
            error.retry = RetryAdvice::AfterConditionsChange;
            return Err(error);
        }
        let token = state.next_prompt;
        state.next_prompt = state.next_prompt.saturating_add(1);
        state.active = Some(ActivePrompt {
            token,
            kind,
            context: cx.clone(),
            thread: thread::current().id(),
        });
        Ok(token)
    }

    /// Release a prompt whose owner generation, disposal or cancellation has
    /// advanced. The thread check prevents a worker-side observation from
    /// mistaking a live UI owner for an expired one.
    fn clear_stale_prompt(&self) {
        let mut state = self.0.state.borrow_mut();
        let current = thread::current().id();
        let stale = state.active.as_ref().is_some_and(|active| {
            active.thread == current
                && (active.context.cancellation.reason().is_some()
                    || active.context.check_at(Instant::now()).is_err())
        });
        if stale {
            state.active = None;
        }
    }
}
