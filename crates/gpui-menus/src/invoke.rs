//! Invocation validation, explicit execution domains and task dispatch.

use crate::handler::{BackgroundDispatch, Handler, UiDispatch, try_acquire};
use crate::service::{CommandEntry, MenuCommandService};
use crate::task::{TaskShared, TaskSpec};
use crate::{
    BackgroundCommandContext, CancelReason, CommandHandle, CommandPayload, CommandTaskState,
    Completion, ConcurrencyPolicy, ErrorCode, ExecutionDomain, InvocationContext, ServiceResult,
    SystemError, UiCommandContext,
};
use serde::{Serialize, de::DeserializeOwned};
use std::{
    future::Future,
    marker::PhantomData,
    panic::AssertUnwindSafe,
    pin::Pin,
    task::{Context, Poll},
    time::Instant,
};

enum StartFailure {
    Error(SystemError),
    Cancelled(CancelReason),
}

impl From<SystemError> for StartFailure {
    fn from(error: SystemError) -> Self {
        Self::Error(error)
    }
}

/// A request that converts the dynamic command result into the registered
/// output type when the invocation actually resolves.
pub struct TypedCommandRequest<O> {
    inner: gpui_system::Request<CommandPayload>,
    marker: PhantomData<fn() -> O>,
}

impl<O> TypedCommandRequest<O> {
    fn new(inner: gpui_system::Request<CommandPayload>) -> Self {
        Self {
            inner,
            marker: PhantomData,
        }
    }

    pub fn id(&self) -> gpui_system::RequestId {
        self.inner.id()
    }
}

impl<O> Unpin for TypedCommandRequest<O> {}

impl<O> Future for TypedCommandRequest<O>
where
    O: DeserializeOwned,
{
    type Output = ServiceResult<O>;

    fn poll(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.get_mut();
        match Pin::new(&mut this.inner).poll(context) {
            Poll::Pending => Poll::Pending,
            Poll::Ready(Ok(Completion::Completed(payload))) => {
                Poll::Ready(payload.into_typed::<O>().map(Completion::Completed))
            }
            Poll::Ready(Ok(Completion::Cancelled(reason))) => {
                Poll::Ready(Ok(Completion::Cancelled(reason)))
            }
            Poll::Ready(Err(error)) => Poll::Ready(Err(error)),
        }
    }
}

/// Build a request that is already resolved with a typed service error.
pub(crate) fn failed_request(
    context: &InvocationContext,
    error: SystemError,
) -> gpui_system::Request<CommandPayload> {
    let (request, port) = gpui_system::request(context.operation.clone());
    port.complete(Err(error));
    request
}

impl MenuCommandService {
    /// Alias for [`MenuCommandService::invoke_command`].
    pub fn invoke(
        &self,
        id: &crate::CommandId,
        payload: CommandPayload,
        context: InvocationContext,
    ) -> gpui_system::Request<CommandPayload> {
        self.invoke_command(id, payload, context)
    }

    /// Compile-time typed invocation. The payload crosses the same dynamic
    /// bridge used by menus, keybindings, palettes, buttons and activations.
    pub fn invoke_typed<I, O>(
        &self,
        handle: &CommandHandle<I, O>,
        input: I,
        context: InvocationContext,
    ) -> TypedCommandRequest<O>
    where
        I: Serialize,
        O: DeserializeOwned,
    {
        if let Some(entry) = self.0.ensure_command(handle.id())
            && entry.descriptor.domain != handle.domain()
        {
            return TypedCommandRequest::new(failed_request(
                &context,
                SystemError::new(
                    ErrorCode::InvalidInput,
                    "Command handle execution domain does not match the registered command",
                ),
            ));
        }
        match CommandPayload::from_typed(&input) {
            Ok(payload) => {
                TypedCommandRequest::new(self.invoke_command(handle.id(), payload, context))
            }
            Err(error) => TypedCommandRequest::new(failed_request(&context, error)),
        }
    }

    /// Dynamic invocation used by every non-typed trigger surface.
    pub fn invoke_command(
        &self,
        id: &crate::CommandId,
        payload: CommandPayload,
        context: InvocationContext,
    ) -> gpui_system::Request<CommandPayload> {
        let (request, port) = gpui_system::request(context.operation.clone());
        match self.start_invocation(id, payload, context, port.clone()) {
            Ok(()) => {}
            Err(StartFailure::Cancelled(reason)) => {
                port.complete(Ok(Completion::Cancelled(reason)));
            }
            Err(StartFailure::Error(error)) => {
                port.complete(Err(error));
            }
        }
        request
    }

    fn start_invocation(
        &self,
        id: &crate::CommandId,
        payload: CommandPayload,
        context: InvocationContext,
        port: gpui_system::CompletionPort<CommandPayload>,
    ) -> Result<(), StartFailure> {
        let now = Instant::now();
        let entry = self
            .0
            .ensure_command(id)
            .ok_or_else(|| SystemError::new(ErrorCode::Unavailable, "Command is not registered"))?;
        entry.descriptor.validate()?;
        self.0.scope_binding(entry.owner.scope)?;
        validate_context(&self.0, &entry, &context, now)?;
        if let Some(reason) = context.operation.cancellation.reason() {
            return Err(StartFailure::Cancelled(reason));
        }
        crate::resolve::check_capabilities(&self.0, &entry.descriptor)?;
        crate::resolve::check_permissions(&self.0, &entry.descriptor)?;
        let fallback_scope = Some(entry.owner.scope);
        let enabled = {
            let scope = context.scope.or(fallback_scope).map(|scope| scope.0);
            self.0.state.borrow().contexts.evaluate(
                &entry.descriptor.enabled_when,
                context.effective_surface(),
                scope,
                context.overlay.as_ref(),
            )
        };
        if !enabled {
            return Err(SystemError::new(
                ErrorCode::InvalidInput,
                "Command is disabled in the current context",
            )
            .into());
        }

        if matches!(
            entry.descriptor.concurrency,
            ConcurrencyPolicy::ReplaceInFlight
        ) {
            let running = self.0.running_tasks_for(id, None);
            for task in running {
                task.cancel(CancelReason::ProcessPolicy);
            }
        }
        let limit = match entry.descriptor.concurrency {
            ConcurrencyPolicy::AllowParallel { max_in_flight } => max_in_flight,
            ConcurrencyPolicy::SingleFlightReject | ConcurrencyPolicy::ReplaceInFlight => 1,
        };
        if !try_acquire(&entry.in_flight, limit) {
            return Err(SystemError::new(
                ErrorCode::Busy,
                "Command concurrency limit is saturated",
            )
            .into());
        }

        let background_slot = if entry.descriptor.domain == ExecutionDomain::Background {
            if !try_acquire(&self.0.background_slots, self.0.max_background_slots) {
                crate::handler::release_counter(&entry.in_flight);
                return Err(SystemError::new(
                    ErrorCode::Busy,
                    "Background worker pool is saturated",
                )
                .into());
            }
            Some(self.0.background_slots.clone())
        } else {
            None
        };

        let invocation = self.0.allocate_invocation();
        let token = gpui_system::CancellationToken::default();
        let surface = context.effective_surface();
        let deadline = context.operation.deadline;
        let callback: crate::task::TerminalCallback = Box::new(move |result| {
            port.complete(result);
        });
        let task = std::sync::Arc::new(TaskShared::new(TaskSpec {
            id: invocation,
            command_id: entry.descriptor.id.clone(),
            domain: entry.descriptor.domain,
            trigger: context.trigger,
            scope: Some(entry.owner.scope),
            registration: Some(entry.owner.registration),
            surface,
            deadline,
            token: token.clone(),
            events: self.0.events.clone(),
            in_flight: entry.in_flight.clone(),
            background_slot,
            callback,
        }));
        self.0.insert_task(task.clone());
        task.announce_started();

        match entry.handler {
            Handler::Ui(handler) => {
                let mut ui_context = UiCommandContext {
                    invocation,
                    command_id: entry.descriptor.id.clone(),
                    trigger: context.trigger,
                    surface,
                    scope: Some(entry.owner.scope),
                    cancellation: token,
                    deadline,
                };
                match handler(payload, &mut ui_context) {
                    Ok(UiDispatch::Completed(payload)) => {
                        task.finish_with_checks(Ok(Completion::Completed(payload)), Instant::now());
                    }
                    Ok(UiDispatch::Deferred(attach)) => {
                        let deferred_task = task.clone();
                        let callback = Box::new(
                            move |result: Result<CommandPayload, crate::task::DeferredError>| {
                                deferred_task
                                    .finish_with_checks(deferred_result(result), Instant::now());
                            },
                        );
                        if !attach(callback) && task.is_running() {
                            task.finish_error(SystemError::new(
                                ErrorCode::InvalidInput,
                                "Deferred UI completion was already used",
                            ));
                        }
                    }
                    Err(error) => {
                        task.finish_with_checks(Err(error), Instant::now());
                    }
                }
            }
            Handler::Background(handler) => {
                let spawn = self.0.pool.borrow_mut().spawn(Box::new(move || {
                    run_background(handler, payload, task);
                }));
                if let Err(error) = spawn {
                    // `task` moved into the closure only on success. On spawn
                    // failure the caller still owns no task handle, so the slot
                    // is released by failing the invocation through the task
                    // record kept in the service.
                    if let Some(task) = self.0.task(invocation) {
                        task.finish_error(error);
                    }
                }
            }
        }
        Ok(())
    }
}

fn deferred_result(
    result: Result<CommandPayload, crate::task::DeferredError>,
) -> ServiceResult<CommandPayload> {
    match result {
        Ok(payload) => Ok(Completion::Completed(payload)),
        Err(crate::task::DeferredError::Cancelled(reason)) => Ok(Completion::Cancelled(reason)),
        Err(crate::task::DeferredError::Failed(error)) => Err(error),
    }
}

fn validate_context(
    service: &crate::service::ServiceInner,
    entry: &CommandEntry,
    context: &InvocationContext,
    now: Instant,
) -> Result<(), SystemError> {
    if entry.descriptor.domain == ExecutionDomain::Ui {
        let ui = context.operation.ui.as_ref().ok_or_else(|| {
            SystemError::new(
                ErrorCode::InvalidInput,
                "UI command requires an active UI context",
            )
        })?;
        context.operation.check_at(now)?;
        context.operation.require_ui()?;
        let active = service.surface_generation(ui.surface).ok_or_else(|| {
            SystemError::new(ErrorCode::ContextExpired, "Surface has been disposed")
        })?;
        if active != ui.generation {
            return Err(SystemError::new(
                ErrorCode::ContextExpired,
                "UI context generation is no longer live",
            ));
        }
        if let Some(surface) = context.surface
            && surface != ui.surface
        {
            return Err(SystemError::new(
                ErrorCode::InvalidInput,
                "Invocation target surface does not match the UI context",
            ));
        }
    } else {
        context.operation.check_at(now)?;
        if let Some(surface) = context.effective_surface() {
            service.surface_generation(surface).ok_or_else(|| {
                SystemError::new(ErrorCode::ContextExpired, "Surface has been disposed")
            })?;
        }
        if let Some(ui) = context.operation.ui.as_ref() {
            let active = service.surface_generation(ui.surface).ok_or_else(|| {
                SystemError::new(ErrorCode::ContextExpired, "Surface has been disposed")
            })?;
            if active != ui.generation {
                return Err(SystemError::new(
                    ErrorCode::ContextExpired,
                    "UI context generation is no longer live",
                ));
            }
        }
    }
    Ok(())
}

fn run_background(
    handler: std::sync::Arc<
        dyn Fn(CommandPayload, BackgroundCommandContext) -> Result<BackgroundDispatch, SystemError>
            + Send
            + Sync,
    >,
    payload: CommandPayload,
    task: std::sync::Arc<TaskShared>,
) {
    if task.state() != CommandTaskState::Running {
        return;
    }
    if task.expire(Instant::now()) {
        return;
    }
    if let Some(reason) = task.token().reason() {
        task.cancel(reason);
        return;
    }
    let context = BackgroundCommandContext {
        invocation: task.id(),
        command_id: task.command_id().clone(),
        trigger: task.trigger(),
        surface: task.surface(),
        scope: task.scope(),
        cancellation: task.token().clone(),
        deadline: task.deadline(),
    };
    let result = std::panic::catch_unwind(AssertUnwindSafe(|| handler(payload, context)));
    match result {
        Ok(Ok(BackgroundDispatch::Completed(payload))) => {
            task.finish_with_checks(Ok(Completion::Completed(payload)), Instant::now());
        }
        Ok(Ok(BackgroundDispatch::Deferred(attach))) => {
            let deferred_task = task.clone();
            let callback = Box::new(
                move |result: Result<CommandPayload, crate::task::DeferredError>| {
                    deferred_task.finish_with_checks(deferred_result(result), Instant::now());
                },
            );
            if !attach(callback) && task.is_running() {
                task.finish_error(SystemError::new(
                    ErrorCode::InvalidInput,
                    "Deferred background completion was already used",
                ));
            }
        }
        Ok(Err(error)) => {
            task.finish_with_checks(Err(error), Instant::now());
        }
        Err(_) => {
            task.finish_with_checks(
                Err(SystemError::new(
                    ErrorCode::PlatformFailure,
                    "Command handler panicked",
                )),
                Instant::now(),
            );
        }
    }
}

/// Helpers for deterministic tests that need a completed task snapshot.
pub fn is_terminal(state: CommandTaskState) -> bool {
    state.is_terminal()
}
