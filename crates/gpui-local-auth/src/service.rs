//! UI-local local-authentication service: prompt serialization, stale callback
//! rejection, lifecycle reconciliation and bounded observations.

use crate::model::{busy, context_expired};
use crate::provider::{AuthCompletion, LocalAuthProvider};
use crate::{
    LocalAuthEvent, LocalAuthHostState, LocalAuthOperation, LocalAuthPrompt,
    LocalAuthPromptCancelReason, LocalAuthSnapshot, ProtectedSecretOutcome, ProtectedSecretPrompt,
    ProtectedSecretRequest, ProtectedSignatureOutcome, RejectedAuthCallback, SignaturePrompt,
    SignatureRequest, SystemError, VerificationOutcome, VerificationPolicy, VerificationRequest,
};
use gpui_lifecycle::LifecycleState;
use gpui_system::{
    CancelReason, Capability, Completion, CompletionPort, Configuration, ErrorCode, EventSender,
    EventSubscription, OperationContext, RecoveryAction, Request, RetryAdvice, Support,
    SystemServices, UiContext, event_channel,
};
use std::{
    cell::{Cell, RefCell},
    collections::{BTreeMap, VecDeque},
    rc::Rc,
    thread::{self, ThreadId},
    time::Instant,
};

const EVENT_CAPACITY: usize = 16;
const MAX_REJECTIONS: usize = 64;

type Subscriber = (Rc<Cell<bool>>, EventSender<LocalAuthEvent>);

#[derive(Default)]
struct ServiceState {
    last_snapshot: Option<LocalAuthSnapshot>,
    snapshots_by_policy: BTreeMap<VerificationPolicy, LocalAuthSnapshot>,
    active: Option<ActivePrompt>,
    subscribers: Vec<Subscriber>,
    next_prompt: u64,
    lifecycle: Option<LocalAuthHostState>,
    rejections: VecDeque<RejectedAuthCallback>,
}

struct ActivePrompt {
    token: u64,
    operation: LocalAuthOperation,
    surface: gpui_system::SurfaceId,
    generation: u64,
    context: OperationContext,
    thread: ThreadId,
}

fn collect_deliveries(
    state: &mut ServiceState,
    event: LocalAuthEvent,
) -> Vec<(EventSender<LocalAuthEvent>, LocalAuthEvent)> {
    state.subscribers.retain(|(live, _)| live.get());
    state
        .subscribers
        .iter()
        .map(|(_, sender)| (sender.clone(), event.clone()))
        .collect()
}

fn record_rejection(
    state: &mut ServiceState,
    rejection: RejectedAuthCallback,
) -> Vec<(EventSender<LocalAuthEvent>, LocalAuthEvent)> {
    state.rejections.push_back(rejection);
    if state.rejections.len() > MAX_REJECTIONS {
        let _ = state.rejections.pop_front();
    }
    collect_deliveries(state, LocalAuthEvent::CallbackRejected(rejection))
}

fn lifecycle_allows(
    lifecycle: &Option<LocalAuthHostState>,
    surface: gpui_system::SurfaceId,
    generation: u64,
) -> bool {
    match lifecycle {
        None => true,
        Some(host) => {
            host.is_foreground()
                && !host.is_session_locked()
                && host.surface(surface).is_some_and(|snapshot| {
                    snapshot.visibility.is_visible() && snapshot.generation == generation
                })
        }
    }
}

fn gate_capability(capability: &Capability<LocalAuthOperation>) -> Result<(), SystemError> {
    if capability.support == Support::Unsupported {
        let mut error = SystemError::new(
            ErrorCode::Unsupported,
            "Local authentication operation is unsupported",
        );
        error.retry = RetryAdvice::Never;
        return Err(error);
    }
    if capability.configuration != Configuration::Ready {
        let mut error = SystemError::new(
            ErrorCode::InvalidConfiguration,
            "Local authentication provider is not configured",
        );
        error.retry = RetryAdvice::Never;
        return Err(error);
    }
    Ok(())
}

fn prompt_invalidation(
    host: &LocalAuthHostState,
    active: &ActivePrompt,
    now: Instant,
) -> Option<LocalAuthPromptCancelReason> {
    if active.context.cancellation.reason().is_some() {
        return Some(LocalAuthPromptCancelReason::Cancelled);
    }
    if active
        .context
        .deadline
        .is_some_and(|deadline| now >= deadline)
    {
        return Some(LocalAuthPromptCancelReason::DeadlineElapsed);
    }
    if !host.is_foreground() {
        return Some(LocalAuthPromptCancelReason::Backgrounded);
    }
    if host.is_session_locked() {
        return Some(LocalAuthPromptCancelReason::SessionLocked);
    }
    match host.surface(active.surface) {
        None => Some(LocalAuthPromptCancelReason::SurfaceDetached),
        Some(snapshot) if !snapshot.visibility.is_visible() => {
            Some(LocalAuthPromptCancelReason::SurfaceDetached)
        }
        Some(snapshot) if snapshot.generation != active.generation => {
            Some(LocalAuthPromptCancelReason::GenerationAdvanced)
        }
        Some(_) => {
            if active
                .context
                .ui
                .as_ref()
                .is_some_and(|ui| !ui.is_current())
            {
                Some(LocalAuthPromptCancelReason::OwnerChanged)
            } else {
                None
            }
        }
    }
}

pub(crate) struct ServiceInner {
    provider: Rc<dyn LocalAuthProvider>,
    state: RefCell<ServiceState>,
}

impl ServiceInner {
    /// Accept exactly one authoritative completion for the active prompt.
    ///
    /// A stale token, operation, surface or generation is rejected without
    /// invoking the provider callback's payload. A callback that arrives after
    /// cancellation, disposal, recreation, backgrounding, lock or deadline
    /// expiry is recorded as a redacted late callback.
    pub(crate) fn finish_prompt(
        &self,
        token: u64,
        operation: LocalAuthOperation,
        surface: gpui_system::SurfaceId,
        generation: u64,
    ) -> bool {
        let (accepted, deliveries) = {
            let mut state = self.state.borrow_mut();
            let matches = state.active.as_ref().is_some_and(|active| {
                active.token == token
                    && active.operation == operation
                    && active.surface == surface
                    && active.generation == generation
            });
            if !matches {
                let rejection = RejectedAuthCallback::late(surface, generation);
                (false, record_rejection(&mut state, rejection))
            } else {
                let Some(active) = state.active.take() else {
                    return false;
                };
                let now = Instant::now();
                let accepted = active.context.cancellation.reason().is_none()
                    && active.context.check_at(now).is_ok()
                    && lifecycle_allows(&state.lifecycle, surface, generation);
                if accepted {
                    (true, Vec::new())
                } else {
                    let rejection = RejectedAuthCallback::late(surface, generation);
                    (false, record_rejection(&mut state, rejection))
                }
            }
        };
        for (sender, event) in deliveries {
            sender.send(event);
        }
        accepted
    }
}

impl Drop for ServiceInner {
    fn drop(&mut self) {
        if let Some(active) = self.state.get_mut().active.take() {
            active.context.cancellation.cancel(CancelReason::Caller);
        }
    }
}

/// UI-local local-authentication service.
///
/// Cloning shares the provider, the single prompt lane and all subscribers. The
/// service is deliberately `Rc`-backed and belongs to the owning UI thread.
#[derive(Clone)]
pub struct LocalAuthService(Rc<ServiceInner>);

impl LocalAuthService {
    pub fn new(provider: Rc<dyn LocalAuthProvider>) -> Self {
        Self(Rc::new(ServiceInner {
            provider,
            state: RefCell::new(ServiceState::default()),
        }))
    }

    pub fn from_provider(provider: impl LocalAuthProvider + 'static) -> Self {
        Self::new(Rc::new(provider))
    }

    pub fn provider(&self) -> &Rc<dyn LocalAuthProvider> {
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

    /// Describe one operation. This performs no prompt and confers no grant.
    pub fn capability(&self, operation: LocalAuthOperation) -> Capability<LocalAuthOperation> {
        self.0.provider.capability(operation)
    }

    pub fn capabilities(&self) -> Vec<Capability<LocalAuthOperation>> {
        LocalAuthOperation::ALL
            .into_iter()
            .map(|operation| self.capability(operation))
            .collect()
    }

    /// Create an independent bounded observation stream.
    ///
    /// Each stream prunes dead slots on the next subscription, revision lookup
    /// or emission. Sends always happen after the service borrow is dropped, so
    /// a synchronous `Waker::wake` callback may re-enter this service.
    pub fn subscribe(&self) -> EventSubscription<LocalAuthEvent> {
        let live = Rc::new(Cell::new(true));
        let cleanup = Rc::clone(&live);
        let (sender, subscription) = event_channel(EVENT_CAPACITY, move || cleanup.set(false));
        let mut state = self.0.state.borrow_mut();
        state.subscribers.retain(|(live, _)| live.get());
        state.subscribers.push((live, sender));
        subscription
    }

    /// Highest observation sequence across live subscribers.
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

    /// The last non-interactive snapshot observed through this service.
    pub fn snapshot(&self) -> Option<LocalAuthSnapshot> {
        self.0.state.borrow().last_snapshot
    }

    /// The last non-interactive snapshot observed for one verification policy.
    pub fn snapshot_for_policy(&self, policy: VerificationPolicy) -> Option<LocalAuthSnapshot> {
        self.0
            .state
            .borrow()
            .snapshots_by_policy
            .get(&policy)
            .copied()
    }

    /// True while an interactive prompt owns the single prompt lane.
    pub fn active_prompt(&self) -> Option<LocalAuthOperation> {
        self.clear_stale_prompts(Instant::now());
        self.0
            .state
            .borrow()
            .active
            .as_ref()
            .map(|active| active.operation)
    }

    /// Redacted records of rejected provider callbacks.
    pub fn rejected_callbacks(&self) -> Vec<RejectedAuthCallback> {
        self.0.state.borrow().rejections.iter().copied().collect()
    }

    /// Probe readiness without displaying a native prompt. A background
    /// `OperationContext` is valid; a changed readiness is emitted to every live
    /// subscriber.
    pub fn query_availability(
        &self,
        policy: VerificationPolicy,
        cx: OperationContext,
    ) -> Request<LocalAuthSnapshot> {
        let operation = LocalAuthOperation::QueryAvailability(policy);
        let (request, port) = gpui_system::request::<LocalAuthSnapshot>(cx.clone());
        if let Err(error) = gate_capability(&self.capability(operation)) {
            port.complete(Err(error));
            return request;
        }
        if let Err(error) = cx.check_at(Instant::now()) {
            port.complete(Err(error));
            return request;
        }
        if let Some(reason) = cx.cancellation.reason() {
            port.complete(Ok(Completion::Cancelled(reason)));
            return request;
        }
        match self.0.provider.query_availability(policy) {
            Ok(snapshot) => {
                self.observe_snapshot(snapshot);
                port.complete(Ok(Completion::Completed(snapshot)));
            }
            Err(error) => {
                port.complete(Err(error));
            }
        }
        request
    }

    /// Interactive local user verification. A live UI context on the owning UI
    /// thread is mandatory. A concurrent interactive request returns `Busy`.
    pub fn verify_user(
        &self,
        request: VerificationRequest,
        cx: OperationContext,
    ) -> Request<VerificationOutcome> {
        let (future, port) = gpui_system::request::<VerificationOutcome>(cx.clone());
        let operation = LocalAuthOperation::VerifyUser(request.policy);
        let Some((ui, token)) = self.start_interactive(operation, &cx, &port) else {
            return future;
        };
        let prompt = LocalAuthPrompt {
            request: future.id(),
            policy: request.policy,
            purpose: request.purpose,
            surface: ui.surface,
            generation: ui.generation,
        };
        let completion = AuthCompletion::new(
            port,
            Rc::downgrade(&self.0),
            token,
            operation,
            ui.surface,
            ui.generation,
        );
        if let Err(error) = self.0.provider.verify_user(prompt, completion.clone()) {
            completion.fail(error);
        }
        future
    }

    /// Authentication-bound protected-secret release.
    pub fn unlock_protected_secret(
        &self,
        request: ProtectedSecretRequest,
        cx: OperationContext,
    ) -> Request<ProtectedSecretOutcome> {
        let (future, port) = gpui_system::request::<ProtectedSecretOutcome>(cx.clone());
        let operation = LocalAuthOperation::UnlockProtectedSecret;
        let Some((ui, token)) = self.start_interactive(operation, &cx, &port) else {
            return future;
        };
        let prompt = ProtectedSecretPrompt {
            request: future.id(),
            account: request.account,
            key: request.key,
            policy: request.policy,
            purpose: request.purpose,
            surface: ui.surface,
            generation: ui.generation,
        };
        let completion = AuthCompletion::new(
            port,
            Rc::downgrade(&self.0),
            token,
            operation,
            ui.surface,
            ui.generation,
        );
        if let Err(error) = self
            .0
            .provider
            .unlock_protected_secret(prompt, completion.clone())
        {
            completion.fail(error);
        }
        future
    }

    /// Authentication-bound signature over bounded challenge bytes.
    pub fn sign_challenge(
        &self,
        request: SignatureRequest,
        cx: OperationContext,
    ) -> Request<ProtectedSignatureOutcome> {
        let (future, port) = gpui_system::request::<ProtectedSignatureOutcome>(cx.clone());
        let operation = LocalAuthOperation::SignChallenge;
        let Some((ui, token)) = self.start_interactive(operation, &cx, &port) else {
            return future;
        };
        let prompt = SignaturePrompt {
            request: future.id(),
            account: request.account,
            key: request.key,
            algorithm: request.algorithm,
            challenge: request.challenge,
            policy: request.policy,
            purpose: request.purpose,
            surface: ui.surface,
            generation: ui.generation,
        };
        let completion = AuthCompletion::new(
            port,
            Rc::downgrade(&self.0),
            token,
            operation,
            ui.surface,
            ui.generation,
        );
        if let Err(error) = self.0.provider.sign_challenge(prompt, completion.clone()) {
            completion.fail(error);
        }
        future
    }

    /// Reconcile a merged `gpui-lifecycle` state. Any in-flight prompt for a
    /// backgrounded app, locked session, hidden/detached surface or recreated
    /// generation is cancelled and published as a prompt cancellation.
    pub fn reconcile_lifecycle(&self, lifecycle: &LifecycleState, now: Instant) -> bool {
        let host = LocalAuthHostState::from_lifecycle(lifecycle);
        self.reconcile_host_state(&host, now)
    }

    /// Reconcile an explicit host observation. This is the testable form used by
    /// hosts that expose lifecycle attributes without an enumerable snapshot.
    pub fn reconcile_host_state(&self, host: &LocalAuthHostState, now: Instant) -> bool {
        let (active, deliveries) = {
            let mut state = self.0.state.borrow_mut();
            state.lifecycle = Some(host.clone());
            let reason = state
                .active
                .as_ref()
                .and_then(|active| prompt_invalidation(host, active, now));
            if let Some(reason) = reason {
                if let Some(active) = state.active.take() {
                    let event = LocalAuthEvent::PromptCancelled {
                        surface: active.surface,
                        generation: active.generation,
                        reason,
                    };
                    (Some(active), collect_deliveries(&mut state, event))
                } else {
                    (None, Vec::new())
                }
            } else {
                (None, Vec::new())
            }
        };
        let cancelled = active.is_some();
        if let Some(active) = active {
            active
                .context
                .cancellation
                .cancel(CancelReason::ProcessPolicy);
        }
        for (sender, event) in deliveries {
            sender.send(event);
        }
        cancelled
    }

    fn observe_snapshot(&self, snapshot: LocalAuthSnapshot) {
        let deliveries = {
            let mut state = self.0.state.borrow_mut();
            let changed = state
                .snapshots_by_policy
                .get(&snapshot.policy)
                .is_none_or(|previous| previous.readiness != snapshot.readiness);
            state.snapshots_by_policy.insert(snapshot.policy, snapshot);
            state.last_snapshot = Some(snapshot);
            if changed {
                collect_deliveries(&mut state, LocalAuthEvent::AvailabilityChanged { snapshot })
            } else {
                Vec::new()
            }
        };
        for (sender, event) in deliveries {
            sender.send(event);
        }
    }

    fn check_lifecycle(&self, ui: &UiContext) -> Result<(), SystemError> {
        let lifecycle = self.0.state.borrow().lifecycle.clone();
        let Some(host) = lifecycle else {
            return Ok(());
        };
        if !host.is_foreground() {
            return Err(context_expired(
                "Local authentication requires a foreground application",
            ));
        }
        if host.is_session_locked() {
            let mut error = SystemError::new(
                ErrorCode::PermissionDenied,
                "Local authentication is blocked while the session is locked",
            );
            error.recovery = Some(RecoveryAction::Authenticate);
            error.retry = RetryAdvice::AfterConditionsChange;
            return Err(error);
        }
        match host.surface(ui.surface) {
            None => Err(context_expired("The requesting surface is not attached")),
            Some(snapshot) if !snapshot.visibility.is_visible() => {
                Err(context_expired("The requesting surface is not visible"))
            }
            Some(snapshot) if snapshot.generation != ui.generation => Err(context_expired(
                "The requesting surface generation is stale",
            )),
            Some(_) => Ok(()),
        }
    }

    fn start_interactive<T>(
        &self,
        operation: LocalAuthOperation,
        cx: &OperationContext,
        port: &CompletionPort<T>,
    ) -> Option<(UiContext, u64)> {
        if let Err(error) = gate_capability(&self.capability(operation)) {
            port.complete(Err(error));
            return None;
        }
        let ui = match cx.require_ui() {
            Ok(ui) => ui.clone(),
            Err(error) => {
                port.complete(Err(error));
                return None;
            }
        };
        if let Err(error) = cx.check_at(Instant::now()) {
            port.complete(Err(error));
            return None;
        }
        if let Some(reason) = cx.cancellation.reason() {
            port.complete(Ok(Completion::Cancelled(reason)));
            return None;
        }
        if let Err(error) = self.check_lifecycle(&ui) {
            port.complete(Err(error));
            return None;
        }
        self.clear_stale_prompts(Instant::now());
        let active = self.0.state.borrow().active.is_some();
        if active {
            port.complete(Err(busy()));
            return None;
        }
        let mut state = self.0.state.borrow_mut();
        let token = state.next_prompt;
        state.next_prompt = state.next_prompt.saturating_add(1);
        state.active = Some(ActivePrompt {
            token,
            operation,
            surface: ui.surface,
            generation: ui.generation,
            context: cx.clone(),
            thread: thread::current().id(),
        });
        Some((ui, token))
    }

    fn clear_stale_prompts(&self, now: Instant) {
        let (active, deliveries) = {
            let mut state = self.0.state.borrow_mut();
            if state
                .active
                .as_ref()
                .is_some_and(|active| active.thread != thread::current().id())
            {
                return;
            }
            let reason = state
                .active
                .as_ref()
                .and_then(|active| match &state.lifecycle {
                    Some(host) => prompt_invalidation(host, active, now),
                    None => {
                        if active.context.cancellation.reason().is_some() {
                            Some(LocalAuthPromptCancelReason::Cancelled)
                        } else if active
                            .context
                            .deadline
                            .is_some_and(|deadline| now >= deadline)
                        {
                            Some(LocalAuthPromptCancelReason::DeadlineElapsed)
                        } else if active
                            .context
                            .ui
                            .as_ref()
                            .is_some_and(|ui| !ui.is_current())
                        {
                            Some(LocalAuthPromptCancelReason::OwnerChanged)
                        } else {
                            None
                        }
                    }
                });
            if let Some(reason) = reason {
                if let Some(active) = state.active.take() {
                    let event = LocalAuthEvent::PromptCancelled {
                        surface: active.surface,
                        generation: active.generation,
                        reason,
                    };
                    (Some(active), collect_deliveries(&mut state, event))
                } else {
                    (None, Vec::new())
                }
            } else {
                (None, Vec::new())
            }
        };
        if let Some(active) = active {
            active
                .context
                .cancellation
                .cancel(CancelReason::ProcessPolicy);
        }
        for (sender, event) in deliveries {
            sender.send(event);
        }
    }
}
