//! Typed UI/background handler adapters and deferred completion handles.

use crate::{
    ButtonCommandBinding, CancelReason, CommandDescriptor, CommandId, CommandPayload,
    ContributionScopeId, InvocationId, KeychordBinding, MenuNode, SystemActionBinding, SystemError,
    TriggerSource,
};
use serde::{Serialize, de::DeserializeOwned};
use std::{
    cell::RefCell,
    collections::BTreeMap,
    fmt,
    rc::Rc,
    sync::{
        Arc, Mutex, PoisonError,
        atomic::{AtomicUsize, Ordering},
    },
    time::Instant,
};

/// Metadata available to a UI command handler.
#[derive(Clone)]
pub struct UiCommandContext {
    pub invocation: InvocationId,
    pub command_id: CommandId,
    pub trigger: TriggerSource,
    pub surface: Option<gpui_system::SurfaceId>,
    pub scope: Option<ContributionScopeId>,
    pub cancellation: gpui_system::CancellationToken,
    pub deadline: Option<Instant>,
}

impl UiCommandContext {
    pub fn is_cancelled(&self) -> bool {
        self.cancellation.reason().is_some()
    }

    pub fn cancel_reason(&self) -> Option<CancelReason> {
        self.cancellation.reason()
    }

    pub fn is_expired(&self, now: Instant) -> bool {
        self.deadline.is_some_and(|deadline| now >= deadline)
    }

    pub const fn cancellation_token(&self) -> &gpui_system::CancellationToken {
        &self.cancellation
    }
}

impl fmt::Debug for UiCommandContext {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("UiCommandContext")
            .field("invocation", &self.invocation)
            .field("command_id", &self.command_id)
            .field("trigger", &self.trigger)
            .field("surface", &self.surface)
            .field("scope", &self.scope)
            .field("deadline", &self.deadline)
            .finish()
    }
}

/// Sendable metadata available to a background command handler. It never
/// retains a UI context or a provider handle.
#[derive(Clone)]
pub struct BackgroundCommandContext {
    pub invocation: InvocationId,
    pub command_id: CommandId,
    pub trigger: TriggerSource,
    pub surface: Option<gpui_system::SurfaceId>,
    pub scope: Option<ContributionScopeId>,
    pub cancellation: gpui_system::CancellationToken,
    pub deadline: Option<Instant>,
}

impl BackgroundCommandContext {
    pub fn is_cancelled(&self) -> bool {
        self.cancellation.reason().is_some()
    }

    pub fn cancel_reason(&self) -> Option<CancelReason> {
        self.cancellation.reason()
    }

    pub fn is_expired(&self, now: Instant) -> bool {
        self.deadline.is_some_and(|deadline| now >= deadline)
    }

    pub const fn cancellation_token(&self) -> &gpui_system::CancellationToken {
        &self.cancellation
    }
}

impl fmt::Debug for BackgroundCommandContext {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("BackgroundCommandContext")
            .field("invocation", &self.invocation)
            .field("command_id", &self.command_id)
            .field("trigger", &self.trigger)
            .field("surface", &self.surface)
            .field("scope", &self.scope)
            .field("deadline", &self.deadline)
            .finish()
    }
}

pub(crate) type LocalCompletionCallback<T> = Box<dyn FnOnce(Result<T, crate::task::DeferredError>)>;

pub(crate) type SharedCompletionCallback<T> =
    Box<dyn FnOnce(Result<T, crate::task::DeferredError>) + Send + 'static>;

struct LocalCompletionState<T> {
    callback: Option<LocalCompletionCallback<T>>,
    result: Option<Result<T, crate::task::DeferredError>>,
    used: bool,
}

/// Deferred completion handle for a UI command. It is local to the owning UI
/// thread because the handler may return a non-`Send` value.
pub struct UiCommandCompletion<T> {
    state: Rc<RefCell<LocalCompletionState<T>>>,
}

impl<T> Clone for UiCommandCompletion<T> {
    fn clone(&self) -> Self {
        Self {
            state: self.state.clone(),
        }
    }
}

impl<T> UiCommandCompletion<T> {
    fn new() -> Self {
        Self {
            state: Rc::new(RefCell::new(LocalCompletionState {
                callback: None,
                result: None,
                used: false,
            })),
        }
    }

    pub fn complete(&self, value: T) -> bool {
        self.finish(Ok(value))
    }

    pub fn fail(&self, error: SystemError) -> bool {
        self.finish(Err(crate::task::DeferredError::Failed(error)))
    }

    pub fn cancel(&self, reason: CancelReason) -> bool {
        self.finish(Err(crate::task::DeferredError::Cancelled(reason)))
    }

    fn finish(&self, result: Result<T, crate::task::DeferredError>) -> bool {
        let mut result = Some(result);
        let callback = {
            let mut state = self.state.borrow_mut();
            if state.used {
                return false;
            }
            state.used = true;
            match state.callback.take() {
                Some(callback) => Some(callback),
                None => {
                    state.result = result.take();
                    None
                }
            }
        };
        if let Some(callback) = callback
            && let Some(result) = result
        {
            callback(result);
        }
        true
    }

    pub(crate) fn attach(&self, callback: LocalCompletionCallback<T>) -> bool {
        let mut callback = Some(callback);
        let result = {
            let mut state = self.state.borrow_mut();
            if state.callback.is_some() {
                return false;
            }
            match state.result.take() {
                Some(result) => Some(result),
                None => {
                    state.callback = callback.take();
                    None
                }
            }
        };
        if let Some(result) = result
            && let Some(callback) = callback
        {
            callback(result);
        }
        true
    }
}

impl<T> fmt::Debug for UiCommandCompletion<T> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("UiCommandCompletion")
            .field("attached", &self.state.borrow().callback.is_some())
            .field("finished", &self.state.borrow().used)
            .finish()
    }
}

/// The result of one UI handler invocation.
pub enum UiCommandOutcome<T> {
    Completed(T),
    Deferred(UiCommandCompletion<T>),
}

impl<T> UiCommandOutcome<T> {
    pub fn completed(value: T) -> Self {
        Self::Completed(value)
    }

    pub fn deferred() -> (Self, UiCommandCompletion<T>) {
        let completion = UiCommandCompletion::new();
        (Self::Deferred(completion.clone()), completion)
    }
}

struct SharedCompletionState<T> {
    callback: Option<SharedCompletionCallback<T>>,
    result: Option<Result<T, crate::task::DeferredError>>,
    used: bool,
}

/// Deferred completion handle for a background command. It may be completed by
/// the worker, a native callback thread or a completion port.
pub struct BackgroundCommandCompletion<T> {
    state: Arc<Mutex<SharedCompletionState<T>>>,
}

impl<T> Clone for BackgroundCommandCompletion<T> {
    fn clone(&self) -> Self {
        Self {
            state: self.state.clone(),
        }
    }
}

fn lock<T>(value: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    value.lock().unwrap_or_else(PoisonError::into_inner)
}

impl<T> BackgroundCommandCompletion<T> {
    fn new() -> Self {
        Self {
            state: Arc::new(Mutex::new(SharedCompletionState {
                callback: None,
                result: None,
                used: false,
            })),
        }
    }

    pub fn complete(&self, value: T) -> bool {
        self.finish(Ok(value))
    }

    pub fn fail(&self, error: SystemError) -> bool {
        self.finish(Err(crate::task::DeferredError::Failed(error)))
    }

    pub fn cancel(&self, reason: CancelReason) -> bool {
        self.finish(Err(crate::task::DeferredError::Cancelled(reason)))
    }

    fn finish(&self, result: Result<T, crate::task::DeferredError>) -> bool {
        let mut result = Some(result);
        let callback = {
            let mut state = lock(&self.state);
            if state.used {
                return false;
            }
            state.used = true;
            match state.callback.take() {
                Some(callback) => Some(callback),
                None => {
                    state.result = result.take();
                    None
                }
            }
        };
        if let Some(callback) = callback
            && let Some(result) = result
        {
            callback(result);
        }
        true
    }

    pub(crate) fn attach(&self, callback: SharedCompletionCallback<T>) -> bool
    where
        T: Send + 'static,
    {
        let mut callback = Some(callback);
        let result = {
            let mut state = lock(&self.state);
            if state.callback.is_some() {
                return false;
            }
            match state.result.take() {
                Some(result) => Some(result),
                None => {
                    state.callback = callback.take();
                    None
                }
            }
        };
        if let Some(result) = result
            && let Some(callback) = callback
        {
            callback(result);
        }
        true
    }
}

impl<T> fmt::Debug for BackgroundCommandCompletion<T> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let state = lock(&self.state);
        formatter
            .debug_struct("BackgroundCommandCompletion")
            .field("attached", &state.callback.is_some())
            .field("finished", &state.used)
            .finish()
    }
}

/// The result of one background handler invocation.
pub enum BackgroundCommandOutcome<T> {
    Completed(T),
    Deferred(BackgroundCommandCompletion<T>),
}

impl<T> BackgroundCommandOutcome<T> {
    pub fn completed(value: T) -> Self {
        Self::Completed(value)
    }

    pub fn deferred() -> (Self, BackgroundCommandCompletion<T>) {
        let completion = BackgroundCommandCompletion::new();
        (Self::Deferred(completion.clone()), completion)
    }
}

pub(crate) type UiTerminalCallback =
    Box<dyn FnOnce(Result<CommandPayload, crate::task::DeferredError>)>;

pub(crate) type BackgroundTerminalCallback =
    Box<dyn FnOnce(Result<CommandPayload, crate::task::DeferredError>) + Send + 'static>;

pub(crate) enum UiDispatch {
    Completed(CommandPayload),
    Deferred(Box<dyn FnOnce(UiTerminalCallback) -> bool>),
}

pub(crate) enum BackgroundDispatch {
    Completed(CommandPayload),
    Deferred(Box<dyn FnOnce(BackgroundTerminalCallback) -> bool + Send>),
}

pub(crate) type UiHandlerFn =
    Rc<dyn Fn(CommandPayload, &mut UiCommandContext) -> Result<UiDispatch, SystemError>>;

pub(crate) type BackgroundHandlerFn = Arc<
    dyn Fn(CommandPayload, BackgroundCommandContext) -> Result<BackgroundDispatch, SystemError>
        + Send
        + Sync,
>;

#[derive(Clone)]
pub(crate) enum Handler {
    Ui(UiHandlerFn),
    Background(BackgroundHandlerFn),
}

impl Handler {
    pub(crate) const fn domain(&self) -> crate::ExecutionDomain {
        match self {
            Self::Ui(_) => crate::ExecutionDomain::Ui,
            Self::Background(_) => crate::ExecutionDomain::Background,
        }
    }
}

impl fmt::Debug for Handler {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Handler")
            .field("domain", &self.domain())
            .finish()
    }
}

pub(crate) fn ui_handler<I, O>(
    handler: impl Fn(I, &mut UiCommandContext) -> Result<O, SystemError> + 'static,
) -> UiHandlerFn
where
    I: DeserializeOwned + 'static,
    O: Serialize + 'static,
{
    Rc::new(move |payload, context| {
        let input: I = payload.into_typed()?;
        let output = handler(input, context)?;
        Ok(UiDispatch::Completed(CommandPayload::from_typed(&output)?))
    })
}

pub(crate) fn ui_deferred_handler<I, O>(
    handler: impl Fn(I, &mut UiCommandContext) -> Result<UiCommandOutcome<O>, SystemError> + 'static,
) -> UiHandlerFn
where
    I: DeserializeOwned + 'static,
    O: Serialize + 'static,
{
    Rc::new(move |payload, context| {
        let input: I = payload.into_typed()?;
        match handler(input, context)? {
            UiCommandOutcome::Completed(output) => {
                Ok(UiDispatch::Completed(CommandPayload::from_typed(&output)?))
            }
            UiCommandOutcome::Deferred(completion) => {
                Ok(UiDispatch::Deferred(Box::new(move |callback| {
                    completion.attach(Box::new(move |result| {
                        let result = result.and_then(|output| {
                            CommandPayload::from_typed(&output)
                                .map_err(crate::task::DeferredError::Failed)
                        });
                        callback(result);
                    }))
                })))
            }
        }
    })
}

pub(crate) fn background_handler<I, O>(
    handler: impl Fn(I, BackgroundCommandContext) -> Result<O, SystemError> + Send + Sync + 'static,
) -> BackgroundHandlerFn
where
    I: DeserializeOwned + 'static,
    O: Serialize + 'static,
{
    Arc::new(move |payload, context| {
        let input: I = payload.into_typed()?;
        let output = handler(input, context)?;
        Ok(BackgroundDispatch::Completed(CommandPayload::from_typed(
            &output,
        )?))
    })
}

pub(crate) fn background_deferred_handler<I, O>(
    handler: impl Fn(I, BackgroundCommandContext) -> Result<BackgroundCommandOutcome<O>, SystemError>
    + Send
    + Sync
    + 'static,
) -> BackgroundHandlerFn
where
    I: DeserializeOwned + 'static,
    O: Serialize + Send + 'static,
{
    Arc::new(move |payload, context| {
        let input: I = payload.into_typed()?;
        match handler(input, context)? {
            BackgroundCommandOutcome::Completed(output) => Ok(BackgroundDispatch::Completed(
                CommandPayload::from_typed(&output)?,
            )),
            BackgroundCommandOutcome::Deferred(completion) => {
                Ok(BackgroundDispatch::Deferred(Box::new(move |callback| {
                    completion.attach(Box::new(move |result| {
                        let result = result.and_then(|output| {
                            CommandPayload::from_typed(&output)
                                .map_err(crate::task::DeferredError::Failed)
                        });
                        callback(result);
                    }))
                })))
            }
        }
    })
}

#[derive(Clone, Debug)]
pub(crate) struct PendingCommand {
    pub descriptor: CommandDescriptor,
    pub handler: Handler,
}

/// Builder for contributions materialized from a lazy factory.
#[derive(Default)]
pub struct ContributionBundle {
    pub(crate) commands: Vec<PendingCommand>,
    pub(crate) menu: Vec<MenuNode>,
    pub(crate) keybindings: Vec<KeychordBinding>,
    pub(crate) buttons: Vec<ButtonCommandBinding>,
    pub(crate) system_actions: Vec<SystemActionBinding>,
}

impl ContributionBundle {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_command_ui<I, O>(
        mut self,
        descriptor: CommandDescriptor,
        handler: impl Fn(I, &mut UiCommandContext) -> Result<O, SystemError> + 'static,
    ) -> Self
    where
        I: DeserializeOwned + 'static,
        O: Serialize + 'static,
    {
        self.commands.push(PendingCommand {
            descriptor,
            handler: Handler::Ui(ui_handler(handler)),
        });
        self
    }

    pub fn with_command_ui_deferred<I, O>(
        mut self,
        descriptor: CommandDescriptor,
        handler: impl Fn(I, &mut UiCommandContext) -> Result<UiCommandOutcome<O>, SystemError> + 'static,
    ) -> Self
    where
        I: DeserializeOwned + 'static,
        O: Serialize + 'static,
    {
        self.commands.push(PendingCommand {
            descriptor,
            handler: Handler::Ui(ui_deferred_handler(handler)),
        });
        self
    }

    pub fn with_command_background<I, O>(
        mut self,
        descriptor: CommandDescriptor,
        handler: impl Fn(I, BackgroundCommandContext) -> Result<O, SystemError> + Send + Sync + 'static,
    ) -> Self
    where
        I: DeserializeOwned + 'static,
        O: Serialize + 'static,
    {
        self.commands.push(PendingCommand {
            descriptor,
            handler: Handler::Background(background_handler(handler)),
        });
        self
    }

    pub fn with_command_background_deferred<I, O>(
        mut self,
        descriptor: CommandDescriptor,
        handler: impl Fn(
            I,
            BackgroundCommandContext,
        ) -> Result<BackgroundCommandOutcome<O>, SystemError>
        + Send
        + Sync
        + 'static,
    ) -> Self
    where
        I: DeserializeOwned + 'static,
        O: Serialize + Send + 'static,
    {
        self.commands.push(PendingCommand {
            descriptor,
            handler: Handler::Background(background_deferred_handler(handler)),
        });
        self
    }

    pub fn with_menu_node(mut self, node: MenuNode) -> Self {
        self.menu.push(node);
        self
    }

    pub fn with_menu_nodes(mut self, nodes: impl IntoIterator<Item = MenuNode>) -> Self {
        self.menu.extend(nodes);
        self
    }

    pub fn with_keybinding(mut self, binding: KeychordBinding) -> Self {
        self.keybindings.push(binding);
        self
    }

    pub fn with_button(mut self, binding: ButtonCommandBinding) -> Self {
        self.buttons.push(binding);
        self
    }

    pub fn with_system_action(mut self, binding: SystemActionBinding) -> Self {
        self.system_actions.push(binding);
        self
    }
}

pub(crate) fn release_counter(counter: &AtomicUsize) {
    let _ = counter.try_update(Ordering::AcqRel, Ordering::Acquire, |value| {
        Some(value.saturating_sub(1))
    });
}

pub(crate) fn try_acquire(counter: &AtomicUsize, limit: usize) -> bool {
    if limit == 0 {
        return false;
    }
    counter
        .try_update(Ordering::AcqRel, Ordering::Acquire, |value| {
            (value < limit).then_some(value + 1)
        })
        .is_ok()
}

/// Placeholder used by tests and adapters that need an empty parameter map.
pub fn empty_parameters() -> BTreeMap<String, String> {
    BTreeMap::new()
}

/// Command identity plus domain carried by a typed registration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegisteredCommand {
    pub id: CommandId,
    pub domain: crate::ExecutionDomain,
}
