//! Capability-gated tray service, scoped RAII leases, bounded events, stale
//! callback hardening and close-to-tray residency safety.

use crate::{
    CloseInterceptionOutcome, EffectiveStatusSurface, ErrorCode, FakeTrayProvider, PlatformTarget,
    ResidencyBrokenReason, ResidencyFallbackReason, ResidencyRecoveryAction, ShellAbsenceFallback,
    ShellTrayStatus, StaleCallbackReason, StatusItemId, StatusItemKey, StatusItemSpec,
    TrayClickAction, TrayClickEvent, TrayClickTrigger, TrayCreateRequest, TrayItemProjection,
    TrayItemUpdate, TrayLeaseScope, TrayMenuItemClick, TrayOperation, TrayProvider,
    TrayProviderListener, TrayProviderNotice, TrayResidencyPolicy, UnsupportedTrayProvider,
    ValidatedStatusItemSpec, WindowClosePolicy, menu::TrayContextMenuResolver,
};
use gpui_lifecycle::{AppPhase, LifecycleState, SystemPreferences};
use gpui_menus::{
    CommandId, CommandPayload, ContextMenu, InvocationContext, MenuCommandService,
    ResolvedContextMenu, ResolvedMenuNode, SystemMenuRole, TriggerSource,
};
use gpui_system::{
    Capability, Completion, EventSender, EventSubscription, OperationContext, Request,
    ServiceResult, SurfaceId, SystemError, UiContext, UiOwner, event_channel,
};
use std::{
    collections::{BTreeMap, HashMap},
    future::Future,
    pin::Pin,
    rc::{Rc, Weak},
    sync::{
        Mutex,
        atomic::{AtomicBool, Ordering},
    },
    task::{Context, Poll, Waker},
    time::Instant,
};

/// Default bounded capacity of one tray event stream.
pub const DEFAULT_TRAY_EVENT_CAPACITY: usize = 64;

type Subscriber = (Rc<AtomicBool>, EventSender<TrayEvent>);

/// Public-safe tray observations. Producers never block on consumers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TrayEvent {
    StatusItemCreated {
        id: StatusItemId,
        key: StatusItemKey,
    },
    StatusItemUpdated {
        id: StatusItemId,
        revision: u64,
    },
    StatusItemRemoved {
        id: StatusItemId,
    },
    MenuPresented {
        id: StatusItemId,
        item_count: usize,
    },
    ClickDispatched {
        id: StatusItemId,
        trigger: TrayClickTrigger,
        action: TrayClickAction,
    },
    ActionFailed {
        id: StatusItemId,
        code: ErrorCode,
    },
    CallbackIgnored {
        item_id: Option<StatusItemId>,
        reason: StaleCallbackReason,
    },
    ResidencyChanged {
        residing_in_tray: bool,
    },
    ResidencyBroken {
        reason: ResidencyBrokenReason,
        recovery: ResidencyRecoveryAction,
    },
    CloseIntercepted {
        outcome: CloseInterceptionOutcome,
    },
    ShellLost,
    ShellRestartRecovered {
        shell_epoch: u64,
        restored_count: usize,
    },
    IconsReprojected {
        count: usize,
    },
    QuitStarted {
        removed_count: usize,
    },
    OperationFailed {
        operation: TrayOperation,
        code: ErrorCode,
    },
}

/// The result of a provider click after correlation and dispatch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TrayClickOutcome {
    NoAction,
    MenuPresented,
    RestoreRequested,
    CommandDispatched { command_id: CommandId },
    CommandCancelled { command_id: CommandId },
    SystemActionDispatched { action_id: String },
    SystemActionCancelled { action_id: String },
    Ignored(StaleCallbackReason),
}

/// Public snapshot of one live status item.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatusItemSnapshot {
    pub id: StatusItemId,
    pub key: StatusItemKey,
    pub scope: TrayLeaseScope,
    pub revision: u64,
    pub native_item_token: u64,
    pub shell_epoch: u64,
    pub hosted: bool,
    pub visible: bool,
}

struct SurfaceRecord {
    generation: u64,
    visible: bool,
    disposed: bool,
}

struct ItemRuntime {
    spec: ValidatedStatusItemSpec,
    native_item_token: u64,
    shell_epoch: u64,
    revision: u64,
    removed: bool,
    hosted: bool,
}

pub(crate) struct ItemInner {
    id: StatusItemId,
    scope: TrayLeaseScope,
    ui: Option<UiContext>,
    runtime: Mutex<ItemRuntime>,
}

fn lock<T>(value: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    value
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

impl ItemInner {
    fn runtime(&self) -> std::sync::MutexGuard<'_, ItemRuntime> {
        lock(&self.runtime)
    }

    fn is_removed(&self) -> bool {
        self.runtime().removed
    }

    fn snapshot(&self, runtime: &ItemRuntime) -> StatusItemSnapshot {
        StatusItemSnapshot {
            id: self.id,
            key: runtime.spec.key.clone(),
            scope: self.scope,
            revision: runtime.revision,
            native_item_token: runtime.native_item_token,
            shell_epoch: runtime.shell_epoch,
            hosted: runtime.hosted,
            visible: runtime.spec.visible,
        }
    }

    fn current_snapshot(&self) -> StatusItemSnapshot {
        let runtime = self.runtime();
        self.snapshot(&runtime)
    }
}

struct ServiceState {
    phase: AppPhase,
    quitting: bool,
    residing_in_tray: bool,
    shell: ShellTrayStatus,
    last_shell_epoch: u64,
    surfaces: HashMap<SurfaceId, SurfaceRecord>,
    items: BTreeMap<StatusItemId, Rc<ItemInner>>,
    keys: BTreeMap<StatusItemKey, StatusItemId>,
    reserved_keys: BTreeMap<StatusItemKey, u64>,
    next_item_id: u64,
    next_reservation: u64,
    preferences: SystemPreferences,
    scale_factor_milli: u32,
    policy: TrayResidencyPolicy,
    menus: Option<MenuCommandService>,
    subscribers: Vec<Subscriber>,
    stale_callback_count: u64,
    last_stale_callback: Option<StaleCallbackReason>,
    recovering_shell: bool,
    pending_shell_epoch: Option<u64>,
}

pub(crate) struct ServiceInner {
    provider: Rc<dyn TrayProvider>,
    state: Mutex<ServiceState>,
    app_owner: UiOwner,
    event_capacity: usize,
}

fn capability_error(code: ErrorCode, message: &'static str) -> SystemError {
    SystemError::new(code, message)
}

fn recovery_for(fallback: ShellAbsenceFallback) -> ResidencyRecoveryAction {
    match fallback {
        ShellAbsenceFallback::RestorePrimarySurface => {
            ResidencyRecoveryAction::RestorePrimarySurface
        }
        ShellAbsenceFallback::QuitCleanly => ResidencyRecoveryAction::RequestCleanQuit,
    }
}

fn fallback_outcome(
    reason: ResidencyFallbackReason,
    fallback: ShellAbsenceFallback,
) -> CloseInterceptionOutcome {
    match fallback {
        ShellAbsenceFallback::RestorePrimarySurface => {
            CloseInterceptionOutcome::PreventHideFallbackRestore { reason }
        }
        ShellAbsenceFallback::QuitCleanly => {
            CloseInterceptionOutcome::PreventHideFallbackQuit { reason }
        }
    }
}

impl ServiceInner {
    fn emit(&self, event: TrayEvent) {
        let senders = {
            let mut state = lock(&self.state);
            state
                .subscribers
                .retain(|(live, _)| live.load(Ordering::Acquire));
            state
                .subscribers
                .iter()
                .map(|(_, sender)| sender.clone())
                .collect::<Vec<_>>()
        };
        for sender in senders {
            sender.send(event.clone());
        }
    }

    fn emit_failure(&self, operation: TrayOperation, error: &SystemError) {
        self.emit(TrayEvent::OperationFailed {
            operation,
            code: error.code,
        });
    }

    fn fail<T>(&self, operation: TrayOperation, error: SystemError) -> ServiceResult<T> {
        self.emit_failure(operation, &error);
        Err(error)
    }

    fn check_context<T>(
        &self,
        operation: TrayOperation,
        context: &OperationContext,
    ) -> Result<Option<Completion<T>>, SystemError> {
        if let Err(error) = context.check_at(Instant::now()) {
            self.emit_failure(operation, &error);
            return Err(error);
        }
        if let Some(reason) = context.cancellation.reason() {
            return Ok(Some(Completion::Cancelled(reason)));
        }
        Ok(None)
    }

    fn gate_capability(&self, operation: TrayOperation) -> Result<(), SystemError> {
        if self.provider.platform().is_mobile_or_web() {
            return Err(capability_error(
                ErrorCode::Unsupported,
                "Native tray operations are unsupported on mobile and Web targets",
            ));
        }
        self.provider.capability(operation).check()
    }

    fn require_shell(&self) -> Result<u64, SystemError> {
        match self.provider.shell_status() {
            ShellTrayStatus::Available { shell_epoch } => Ok(shell_epoch),
            ShellTrayStatus::Absent => Err(capability_error(
                ErrorCode::Unavailable,
                "No system tray shell is currently available",
            )),
        }
    }

    fn record_stale(&self, item_id: Option<StatusItemId>, reason: StaleCallbackReason) {
        {
            let mut state = lock(&self.state);
            state.stale_callback_count = state.stale_callback_count.saturating_add(1);
            state.last_stale_callback = Some(reason);
        }
        self.emit(TrayEvent::CallbackIgnored { item_id, reason });
    }

    fn stale_click(
        &self,
        item_id: Option<StatusItemId>,
        reason: StaleCallbackReason,
    ) -> ServiceResult<TrayClickOutcome> {
        self.record_stale(item_id, reason);
        Ok(Completion::Completed(TrayClickOutcome::Ignored(reason)))
    }

    fn cleanup_reservation(&self, key: &StatusItemKey, reservation: u64) {
        let mut state = lock(&self.state);
        if state.reserved_keys.get(key).copied() == Some(reservation) {
            state.reserved_keys.remove(key);
        }
    }

    fn cleanup_orphan(&self, native_item_token: u64) {
        if native_item_token != 0 {
            let _ = self.provider.remove_item(native_item_token);
        }
    }

    fn validate_scope_locked(
        &self,
        state: &ServiceState,
        scope: TrayLeaseScope,
        ui: Option<&UiContext>,
    ) -> Result<(), SystemError> {
        // An explicit caller context is always authoritative for thread and
        // lifetime. This check must happen before the Application early return,
        // otherwise a disposed/foreign context could create an item that is
        // later silently attributed to the internal application owner.
        if let Some(ui) = ui
            && !ui.is_current()
        {
            return Err(capability_error(
                ErrorCode::ContextExpired,
                "Status item UI context is stale",
            ));
        }
        let Some((surface, generation)) = scope.surface_id().zip(scope.generation()) else {
            return Ok(());
        };
        if generation == 0 {
            return Err(capability_error(
                ErrorCode::InvalidInput,
                "A surface-scoped status item requires a nonzero generation",
            ));
        }
        if let Some(ui) = ui {
            self.ensure_surface_ui_locked(state, ui, surface, generation)?;
        }
        self.ensure_surface_attached_locked(state, surface, generation)
    }

    fn ensure_surface_ui_locked(
        &self,
        state: &ServiceState,
        ui: &UiContext,
        surface: SurfaceId,
        generation: u64,
    ) -> Result<(), SystemError> {
        if !ui.is_current() {
            return Err(capability_error(
                ErrorCode::ContextExpired,
                "Status item UI context is stale",
            ));
        }
        if ui.surface != surface || ui.generation != generation {
            return Err(capability_error(
                ErrorCode::InvalidInput,
                "Status item surface does not match the UI context",
            ));
        }
        self.ensure_surface_attached_locked(state, surface, generation)
    }

    fn ensure_surface_attached_locked(
        &self,
        state: &ServiceState,
        surface: SurfaceId,
        generation: u64,
    ) -> Result<(), SystemError> {
        match state.surfaces.get(&surface) {
            None => Err(capability_error(
                ErrorCode::ContextExpired,
                "Status item surface is not attached",
            )),
            Some(record) if record.disposed => Err(capability_error(
                ErrorCode::ContextExpired,
                "Status item surface is disposed",
            )),
            Some(record) if record.generation != generation => Err(capability_error(
                ErrorCode::ContextExpired,
                "Status item generation is stale",
            )),
            Some(_) => Ok(()),
        }
    }

    /// Resolve the UI context used to execute menu/command work for an item.
    ///
    /// Explicit caller contexts are strict. Without one, surface items use
    /// their creating UI context, while application items may fall back to the
    /// service-owned UI-thread owner after their creating window is gone.
    fn effective_ui_locked(
        &self,
        state: &ServiceState,
        item: &ItemInner,
        explicit: Option<&UiContext>,
    ) -> Result<UiContext, SystemError> {
        if let Some(ui) = explicit {
            if !ui.is_current() {
                return Err(capability_error(
                    ErrorCode::ContextExpired,
                    "Tray UI context is stale",
                ));
            }
            if let TrayLeaseScope::Surface {
                surface,
                generation,
            } = item.scope
            {
                self.ensure_surface_ui_locked(state, ui, surface, generation)?;
            }
            return Ok(ui.clone());
        }

        match item.scope {
            TrayLeaseScope::Application => {
                if let Some(ui) = item.ui.as_ref().filter(|ui| ui.is_current()) {
                    let surface_superseded = state
                        .surfaces
                        .get(&ui.surface)
                        .is_some_and(|record| record.disposed || record.generation > ui.generation);
                    let menu_superseded = state
                        .menus
                        .as_ref()
                        .and_then(|menus| menus.surface_generation(ui.surface))
                        .is_some_and(|generation| generation > ui.generation);
                    if !surface_superseded && !menu_superseded {
                        return Ok(ui.clone());
                    }
                }
                self.app_owner.context(None).map_err(|_| {
                    capability_error(
                        ErrorCode::ContextExpired,
                        "Application tray UI context is unavailable on this thread",
                    )
                })
            }
            TrayLeaseScope::Surface {
                surface,
                generation,
            } => {
                let ui = item.ui.as_ref().ok_or_else(|| {
                    capability_error(
                        ErrorCode::ContextExpired,
                        "Surface status item has no UI context",
                    )
                })?;
                self.ensure_surface_ui_locked(state, ui, surface, generation)?;
                Ok(ui.clone())
            }
        }
    }

    fn scope_is_attached_locked(&self, state: &ServiceState, scope: TrayLeaseScope) -> bool {
        match scope {
            TrayLeaseScope::Application => true,
            TrayLeaseScope::Surface {
                surface,
                generation,
            } => state
                .surfaces
                .get(&surface)
                .is_some_and(|record| !record.disposed && record.generation == generation),
        }
    }

    fn scope_is_live_locked(
        &self,
        state: &ServiceState,
        scope: TrayLeaseScope,
        excluded_surface: Option<SurfaceId>,
    ) -> bool {
        match scope {
            TrayLeaseScope::Application => true,
            TrayLeaseScope::Surface { surface, .. } => {
                if excluded_surface == Some(surface) {
                    return false;
                }
                self.scope_is_attached_locked(state, scope)
                    && state
                        .surfaces
                        .get(&surface)
                        .is_some_and(|record| record.visible)
            }
        }
    }
}

impl TrayProviderListener for ServiceInner {
    fn on_tray_provider_notice(&self, notice: TrayProviderNotice) {
        match notice {
            TrayProviderNotice::ShellLost => self.handle_shell_lost(),
            TrayProviderNotice::ShellRestarted { shell_epoch } => {
                self.handle_shell_restarted(shell_epoch)
            }
            TrayProviderNotice::Clicked(event) => {
                if let Err(error) = self.handle_click_event(&event) {
                    self.emit(TrayEvent::ActionFailed {
                        id: event.item_id,
                        code: error.code,
                    });
                }
            }
            TrayProviderNotice::MenuItemClicked(event) => {
                if let Err(error) = self.handle_menu_item_click_event(&event) {
                    self.emit(TrayEvent::ActionFailed {
                        id: event.item_id,
                        code: error.code,
                    });
                }
            }
        }
    }
}

/// An effective, scoped status-item lease.
///
/// Dropping or removing the lease is idempotent: the native status item is
/// removed at most once, including when a shell restart, surface disposal or
/// explicit quit already removed it.
pub struct StatusItemLease {
    inner: Rc<ItemInner>,
    service: TrayService,
}

impl StatusItemLease {
    pub fn id(&self) -> StatusItemId {
        self.inner.id
    }

    pub fn key(&self) -> StatusItemKey {
        self.inner.runtime().spec.key.clone()
    }

    pub fn scope(&self) -> TrayLeaseScope {
        self.inner.scope
    }

    pub fn native_item_token(&self) -> u64 {
        self.inner.runtime().native_item_token
    }

    pub fn shell_epoch(&self) -> u64 {
        self.inner.runtime().shell_epoch
    }

    pub fn revision(&self) -> u64 {
        self.inner.runtime().revision
    }

    pub fn is_active(&self) -> bool {
        !self.inner.is_removed() && self.service.status_item_snapshot(self.inner.id).is_some()
    }

    pub fn snapshot(&self) -> Option<StatusItemSnapshot> {
        if self.is_active() {
            Some(self.inner.current_snapshot())
        } else {
            None
        }
    }

    /// Update this lease. The key cannot change.
    pub fn update(
        &self,
        spec: StatusItemSpec,
        context: &OperationContext,
    ) -> ServiceResult<StatusItemSnapshot> {
        self.service
            .update_status_item(self.inner.id, spec, context)
    }

    /// Remove the native status item. Returns true exactly once.
    pub fn remove(&self) -> bool {
        self.service.remove_status_item(self.inner.id)
    }
}

impl Drop for StatusItemLease {
    fn drop(&mut self) {
        let _ = self.remove();
    }
}

impl std::fmt::Debug for StatusItemLease {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("StatusItemLease")
            .field("id", &self.inner.id)
            .field("scope", &self.inner.scope)
            .field("snapshot", &self.snapshot())
            .finish()
    }
}

/// Cloneable tray service suitable for `SystemServices` and the GPUI adapter.
#[derive(Clone)]
pub struct TrayService {
    inner: Rc<ServiceInner>,
}

impl TrayService {
    pub fn new(provider: Rc<dyn TrayProvider>) -> Self {
        Self::with_options(provider, None, DEFAULT_TRAY_EVENT_CAPACITY)
    }

    pub fn with_menus(provider: Rc<dyn TrayProvider>, menus: MenuCommandService) -> Self {
        Self::with_options(provider, Some(menus), DEFAULT_TRAY_EVENT_CAPACITY)
    }

    pub fn with_event_capacity(provider: Rc<dyn TrayProvider>, capacity: usize) -> Self {
        Self::with_options(provider, None, capacity)
    }

    fn with_options(
        provider: Rc<dyn TrayProvider>,
        menus: Option<MenuCommandService>,
        capacity: usize,
    ) -> Self {
        let shell = provider.shell_status();
        let last_shell_epoch = shell.shell_epoch().unwrap_or(0);
        let app_owner = UiOwner::new(SurfaceId(u64::MAX - 1));
        let inner = Rc::new(ServiceInner {
            provider,
            app_owner,
            state: Mutex::new(ServiceState {
                phase: AppPhase::Launching,
                quitting: false,
                residing_in_tray: false,
                shell,
                last_shell_epoch,
                surfaces: HashMap::new(),
                items: BTreeMap::new(),
                keys: BTreeMap::new(),
                reserved_keys: BTreeMap::new(),
                next_item_id: 1,
                next_reservation: 1,
                preferences: SystemPreferences::default(),
                scale_factor_milli: 1000,
                policy: TrayResidencyPolicy::default(),
                menus,
                subscribers: Vec::new(),
                stale_callback_count: 0,
                last_stale_callback: None,
                recovering_shell: false,
                pending_shell_epoch: None,
            }),
            event_capacity: capacity.max(1),
        });
        let listener: Weak<dyn TrayProviderListener> = {
            let shared: Rc<dyn TrayProviderListener> = inner.clone();
            Rc::downgrade(&shared)
        };
        inner.provider.attach_listener(listener);
        Self { inner }
    }

    pub fn from_provider(provider: impl TrayProvider + 'static) -> Self {
        Self::new(Rc::new(provider))
    }

    pub fn from_fake(fake: FakeTrayProvider) -> Self {
        Self::new(Rc::new(fake))
    }

    pub fn from_fake_with_menus(fake: FakeTrayProvider, menus: MenuCommandService) -> Self {
        Self::with_menus(Rc::new(fake), menus)
    }

    pub fn unavailable(platform: PlatformTarget) -> Self {
        Self::from_provider(UnsupportedTrayProvider::new(platform))
    }

    pub fn install(self, services: &mut gpui_system::SystemServices) -> Result<(), SystemError> {
        services.register(self)
    }

    pub fn install_cloned(
        &self,
        services: &mut gpui_system::SystemServices,
    ) -> Result<(), SystemError> {
        services.register(self.clone())
    }

    pub fn shares_state_with(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.inner, &other.inner)
    }

    pub fn provider(&self) -> &Rc<dyn TrayProvider> {
        &self.inner.provider
    }

    pub fn fake(&self) -> Option<&FakeTrayProvider> {
        self.inner
            .provider
            .as_any()
            .downcast_ref::<FakeTrayProvider>()
    }

    pub fn menus(&self) -> Option<MenuCommandService> {
        lock(&self.inner.state).menus.clone()
    }

    pub fn set_menus(&self, menus: Option<MenuCommandService>) {
        lock(&self.inner.state).menus = menus;
    }

    pub fn capability(&self, operation: TrayOperation) -> Capability<TrayOperation> {
        self.inner.provider.capability(operation)
    }

    pub fn capabilities(&self) -> Vec<Capability<TrayOperation>> {
        self.inner.provider.capabilities()
    }

    pub fn shell_status(&self) -> ShellTrayStatus {
        self.inner.provider.shell_status()
    }

    pub fn shell_epoch(&self) -> Option<u64> {
        self.shell_status().shell_epoch()
    }

    pub fn residency_policy(&self) -> TrayResidencyPolicy {
        lock(&self.inner.state).policy.clone()
    }

    pub fn set_residency_policy(&self, policy: TrayResidencyPolicy) {
        lock(&self.inner.state).policy = policy;
    }

    pub fn preferences(&self) -> SystemPreferences {
        lock(&self.inner.state).preferences.clone()
    }

    pub fn scale_factor_milli(&self) -> u32 {
        lock(&self.inner.state).scale_factor_milli
    }

    pub fn is_quitting(&self) -> bool {
        lock(&self.inner.state).quitting
    }

    pub fn phase(&self) -> AppPhase {
        lock(&self.inner.state).phase
    }

    pub fn residing_in_tray(&self) -> bool {
        lock(&self.inner.state).residing_in_tray
    }

    pub fn visible_surface_count(&self) -> usize {
        lock(&self.inner.state)
            .surfaces
            .values()
            .filter(|record| record.visible && !record.disposed)
            .count()
    }

    pub fn surface_is_tracked(&self, surface: SurfaceId) -> bool {
        lock(&self.inner.state)
            .surfaces
            .get(&surface)
            .is_some_and(|record| !record.disposed)
    }

    pub fn active_status_items(&self) -> Vec<StatusItemSnapshot> {
        let state = lock(&self.inner.state);
        state
            .items
            .values()
            .map(|item| item.current_snapshot())
            .collect()
    }

    pub fn active_item_count(&self) -> usize {
        lock(&self.inner.state).items.len()
    }

    pub fn status_item_snapshot(&self, id: StatusItemId) -> Option<StatusItemSnapshot> {
        let item = lock(&self.inner.state).items.get(&id).cloned()?;
        Some(item.current_snapshot())
    }

    pub fn stale_callback_count(&self) -> u64 {
        lock(&self.inner.state).stale_callback_count
    }

    pub fn last_stale_callback(&self) -> Option<StaleCallbackReason> {
        lock(&self.inner.state).last_stale_callback
    }

    pub fn create_application_status_item(
        &self,
        spec: StatusItemSpec,
        context: &OperationContext,
    ) -> ServiceResult<StatusItemLease> {
        self.create_status_item(spec, TrayLeaseScope::Application, context)
    }

    pub fn create_surface_status_item(
        &self,
        spec: StatusItemSpec,
        surface: SurfaceId,
        generation: u64,
        context: &OperationContext,
    ) -> ServiceResult<StatusItemLease> {
        self.create_status_item(spec, TrayLeaseScope::surface(surface, generation), context)
    }

    /// Create a validated status item after capability, shell, lifecycle,
    /// surface and reservation gates. The scoped key is reserved before the
    /// provider call and re-validated after the grant.
    pub fn create_status_item(
        &self,
        spec: StatusItemSpec,
        scope: TrayLeaseScope,
        context: &OperationContext,
    ) -> ServiceResult<StatusItemLease> {
        let operation = TrayOperation::CreateStatusItem;
        if let Some(cancelled) = self.inner.check_context(operation, context)? {
            return Ok(cancelled);
        }
        let validated = match spec.validate() {
            Ok(validated) => validated,
            Err(error) => return self.inner.fail(operation, error),
        };
        if let Err(error) = self.inner.gate_capability(operation) {
            return self.inner.fail(operation, error);
        }
        let shell_epoch = match self.inner.require_shell() {
            Ok(epoch) => epoch,
            Err(error) => return self.inner.fail(operation, error),
        };

        let reservation = {
            let mut state = lock(&self.inner.state);
            if state.quitting || matches!(state.phase, AppPhase::Terminating) {
                Err(capability_error(
                    ErrorCode::Unavailable,
                    "Cannot create a status item while the application is quitting",
                ))
            } else if state.keys.contains_key(&validated.key)
                || state.reserved_keys.contains_key(&validated.key)
            {
                Err(capability_error(
                    ErrorCode::Busy,
                    "Status item key is already active or being created",
                ))
            } else if let Err(error) =
                self.inner
                    .validate_scope_locked(&state, scope, context.ui.as_ref())
            {
                Err(error)
            } else {
                let item_id = StatusItemId(state.next_item_id.max(1));
                match (
                    state.next_item_id.checked_add(1),
                    state.next_reservation.checked_add(1),
                ) {
                    (Some(next_id), Some(next_reservation)) => {
                        let reservation = state.next_reservation.max(1);
                        state.next_item_id = next_id;
                        state.next_reservation = next_reservation;
                        state.last_shell_epoch = state.last_shell_epoch.max(shell_epoch);
                        state.shell = ShellTrayStatus::Available { shell_epoch };
                        state
                            .reserved_keys
                            .insert(validated.key.clone(), reservation);
                        let icon = validated
                            .select_icon(&state.preferences, state.scale_factor_milli)
                            .clone();
                        Ok(Reservation {
                            item_id,
                            reservation,
                            key: validated.key.clone(),
                            request: TrayCreateRequest {
                                item_id,
                                key: validated.key.clone(),
                                projection: TrayItemProjection::from_validated(&validated, &icon),
                                scope,
                                shell_epoch,
                                revision: 1,
                            },
                        })
                    }
                    _ => Err(capability_error(
                        ErrorCode::ResourceExpired,
                        "Status item identity exhausted",
                    )),
                }
            }
        };
        let reservation = match reservation {
            Ok(reservation) => reservation,
            Err(error) => return self.inner.fail(operation, error),
        };

        let grant = match self.inner.provider.create_item(&reservation.request) {
            Ok(grant) => grant,
            Err(error) => {
                self.inner
                    .cleanup_reservation(&reservation.key, reservation.reservation);
                return self.inner.fail(operation, error);
            }
        };
        if grant.native_item_token == 0 {
            self.inner
                .cleanup_reservation(&reservation.key, reservation.reservation);
            return self.inner.fail(
                operation,
                capability_error(
                    ErrorCode::PlatformFailure,
                    "Provider returned a zero native status-item token",
                ),
            );
        }

        if let Err(error) = context.check_at(Instant::now()) {
            self.inner.cleanup_orphan(grant.native_item_token);
            self.inner
                .cleanup_reservation(&reservation.key, reservation.reservation);
            return self.inner.fail(operation, error);
        }
        if let Some(reason) = context.cancellation.reason() {
            self.inner.cleanup_orphan(grant.native_item_token);
            self.inner
                .cleanup_reservation(&reservation.key, reservation.reservation);
            return Ok(Completion::Cancelled(reason));
        }
        if let Err(error) = self.inner.gate_capability(operation) {
            self.inner.cleanup_orphan(grant.native_item_token);
            self.inner
                .cleanup_reservation(&reservation.key, reservation.reservation);
            return self.inner.fail(operation, error);
        }

        let expected_shell = self.inner.provider.shell_status();
        let final_check = {
            let mut state = lock(&self.inner.state);
            if state.quitting || matches!(state.phase, AppPhase::Terminating) {
                Err(capability_error(
                    ErrorCode::Unavailable,
                    "Application started quitting while the status item was created",
                ))
            } else if state.reserved_keys.get(&reservation.key).copied()
                != Some(reservation.reservation)
            {
                Err(capability_error(
                    ErrorCode::IntegrityFailure,
                    "Status item reservation was lost during creation",
                ))
            } else {
                state.reserved_keys.remove(&reservation.key);
                if !matches!(
                    expected_shell,
                    ShellTrayStatus::Available { shell_epoch: epoch } if epoch == shell_epoch
                ) {
                    Err(capability_error(
                        ErrorCode::ContextExpired,
                        "System tray shell epoch changed while the status item was created",
                    ))
                } else if let Err(error) =
                    self.inner
                        .validate_scope_locked(&state, scope, context.ui.as_ref())
                {
                    Err(error)
                } else {
                    let runtime = ItemRuntime {
                        spec: validated.clone(),
                        native_item_token: grant.native_item_token,
                        shell_epoch,
                        revision: 1,
                        removed: false,
                        hosted: true,
                    };
                    let item = Rc::new(ItemInner {
                        id: reservation.item_id,
                        scope,
                        ui: context.ui.clone(),
                        runtime: Mutex::new(runtime),
                    });
                    state.items.insert(reservation.item_id, item);
                    state
                        .keys
                        .insert(validated.key.clone(), reservation.item_id);
                    state.shell = expected_shell;
                    state.last_shell_epoch = state.last_shell_epoch.max(shell_epoch);
                    Ok(())
                }
            }
        };
        if let Err(error) = final_check {
            self.inner.cleanup_orphan(grant.native_item_token);
            self.inner
                .cleanup_reservation(&reservation.key, reservation.reservation);
            return self.inner.fail(operation, error);
        }

        self.inner.emit(TrayEvent::StatusItemCreated {
            id: reservation.item_id,
            key: validated.key.clone(),
        });
        let item = {
            let state = lock(&self.inner.state);
            state.items.get(&reservation.item_id).cloned()
        };
        let Some(item) = item else {
            self.inner.cleanup_orphan(grant.native_item_token);
            return self.inner.fail(
                operation,
                capability_error(
                    ErrorCode::IntegrityFailure,
                    "Status item registration was lost",
                ),
            );
        };
        self.inner
            .refresh_residency_if_needed(ResidencyBrokenReason::ReachabilityChanged);
        Ok(Completion::Completed(StatusItemLease {
            inner: item,
            service: self.clone(),
        }))
    }

    /// Update an existing item. The key is immutable.
    pub fn update_status_item(
        &self,
        id: StatusItemId,
        spec: StatusItemSpec,
        context: &OperationContext,
    ) -> ServiceResult<StatusItemSnapshot> {
        let operation = TrayOperation::UpdateStatusItem;
        if let Some(cancelled) = self.inner.check_context(operation, context)? {
            return Ok(cancelled);
        }
        let validated = match spec.validate() {
            Ok(validated) => validated,
            Err(error) => return self.inner.fail(operation, error),
        };
        if let Err(error) = self.inner.gate_capability(operation) {
            return self.inner.fail(operation, error);
        }
        let shell_epoch = match self.inner.require_shell() {
            Ok(epoch) => epoch,
            Err(error) => return self.inner.fail(operation, error),
        };

        let prepared = {
            let state = lock(&self.inner.state);
            if state.quitting || matches!(state.phase, AppPhase::Terminating) {
                Err(capability_error(
                    ErrorCode::Unavailable,
                    "Cannot update a status item while the application is quitting",
                ))
            } else if let Some(item) = state.items.get(&id).cloned() {
                let runtime = item.runtime();
                if runtime.removed {
                    Err(capability_error(
                        ErrorCode::ContextExpired,
                        "Status item lease is no longer active",
                    ))
                } else if runtime.spec.key != validated.key {
                    Err(capability_error(
                        ErrorCode::InvalidInput,
                        "Status item key cannot change during update",
                    ))
                } else if !runtime.hosted || runtime.shell_epoch != shell_epoch {
                    Err(capability_error(
                        ErrorCode::Unavailable,
                        "Status item is not hosted in the current shell epoch",
                    ))
                } else if let Err(error) =
                    self.inner
                        .validate_scope_locked(&state, item.scope, context.ui.as_ref())
                {
                    Err(error)
                } else {
                    let icon = validated
                        .select_icon(&state.preferences, state.scale_factor_milli)
                        .clone();
                    let expected_revision = runtime.revision;
                    let update = TrayItemUpdate {
                        projection: TrayItemProjection::from_validated(&validated, &icon),
                        revision: expected_revision.saturating_add(1),
                    };
                    Ok(PreparedUpdate {
                        token: runtime.native_item_token,
                        shell_epoch,
                        expected_revision,
                        validated,
                        update,
                        item: item.clone(),
                    })
                }
            } else {
                Err(capability_error(
                    ErrorCode::ContextExpired,
                    "Status item lease is no longer active",
                ))
            }
        };
        let prepared = match prepared {
            Ok(prepared) => prepared,
            Err(error) => return self.inner.fail(operation, error),
        };

        if let Err(error) = self
            .inner
            .provider
            .update_item(prepared.token, &prepared.update)
        {
            return self.inner.fail(operation, error);
        }

        if let Err(error) = context.check_at(Instant::now()) {
            return self.inner.fail(operation, error);
        }
        if let Some(reason) = context.cancellation.reason() {
            return Ok(Completion::Cancelled(reason));
        }
        if let Err(error) = self.inner.gate_capability(operation) {
            return self.inner.fail(operation, error);
        }
        let current_shell = self.inner.provider.shell_status();
        enum UpdateFinal {
            Applied(StatusItemSnapshot),
            Cleanup(ErrorCode, &'static str),
            NoCleanup(ErrorCode, &'static str),
            Remove(ErrorCode, &'static str),
            Busy,
        }
        let final_state = {
            let state = lock(&self.inner.state);
            if state.quitting || matches!(state.phase, AppPhase::Terminating) {
                UpdateFinal::NoCleanup(
                    ErrorCode::Unavailable,
                    "Application started quitting while the status item was updated",
                )
            } else if !state
                .items
                .get(&id)
                .is_some_and(|current| Rc::ptr_eq(current, &prepared.item))
            {
                UpdateFinal::NoCleanup(
                    ErrorCode::ContextExpired,
                    "Status item lease was removed during update",
                )
            } else if !matches!(
                current_shell,
                ShellTrayStatus::Available { shell_epoch: epoch } if epoch == prepared.shell_epoch
            ) {
                let mut runtime = prepared.item.runtime();
                runtime.hosted = false;
                UpdateFinal::Cleanup(
                    ErrorCode::ContextExpired,
                    "System tray shell epoch changed during update",
                )
            } else if let Err(_error) =
                self.inner
                    .validate_scope_locked(&state, prepared.item.scope, context.ui.as_ref())
            {
                UpdateFinal::Remove(
                    ErrorCode::ContextExpired,
                    "Status item surface expired during update",
                )
            } else {
                let mut runtime = prepared.item.runtime();
                if runtime.removed {
                    UpdateFinal::NoCleanup(
                        ErrorCode::ContextExpired,
                        "Status item lease was removed during update",
                    )
                } else if runtime.revision != prepared.expected_revision {
                    UpdateFinal::Busy
                } else {
                    runtime.spec = prepared.validated;
                    runtime.revision = prepared.expected_revision.saturating_add(1);
                    UpdateFinal::Applied(prepared.item.snapshot(&runtime))
                }
            }
        };
        let snapshot = match final_state {
            UpdateFinal::Applied(snapshot) => snapshot,
            UpdateFinal::Cleanup(code, message) => {
                self.inner.cleanup_orphan(prepared.token);
                return self.inner.fail(operation, capability_error(code, message));
            }
            UpdateFinal::NoCleanup(code, message) => {
                return self.inner.fail(operation, capability_error(code, message));
            }
            UpdateFinal::Remove(code, message) => {
                self.remove_item_inner(id, ResidencyBrokenReason::SurfaceDisposed);
                return self.inner.fail(operation, capability_error(code, message));
            }
            UpdateFinal::Busy => {
                return self.inner.fail(
                    operation,
                    capability_error(ErrorCode::Busy, "Status item was updated concurrently"),
                );
            }
        };

        self.inner.emit(TrayEvent::StatusItemUpdated {
            id,
            revision: snapshot.revision,
        });
        self.inner
            .refresh_residency_if_needed(ResidencyBrokenReason::ReachabilityChanged);
        Ok(Completion::Completed(snapshot))
    }

    fn remove_item_inner(&self, id: StatusItemId, reason: ResidencyBrokenReason) -> bool {
        let removed = {
            let mut state = lock(&self.inner.state);
            let Some(item) = state.items.remove(&id) else {
                return false;
            };
            let mut runtime = item.runtime();
            if runtime.removed {
                return false;
            }
            runtime.removed = true;
            runtime.hosted = false;
            let key = runtime.spec.key.clone();
            let token = runtime.native_item_token;
            state.keys.remove(&key);
            RemovedItem { id, token }
        };
        let _ = self.inner.provider.remove_item(removed.token);
        self.inner
            .emit(TrayEvent::StatusItemRemoved { id: removed.id });
        self.inner.refresh_residency_if_needed(reason);
        true
    }

    pub fn remove_status_item(&self, id: StatusItemId) -> bool {
        self.remove_item_inner(id, ResidencyBrokenReason::LastStatusItemRemoved)
    }

    /// Attach or replace a surface snapshot. A generation change removes every
    /// lease bound to the old generation.
    pub fn attach_surface(&self, surface: SurfaceId, generation: u64, visible: bool) {
        let (old_generation, was_residing) = {
            let mut state = lock(&self.inner.state);
            let was_residing = state.residing_in_tray;
            let record = state
                .surfaces
                .entry(surface)
                .or_insert_with(|| SurfaceRecord {
                    generation,
                    visible,
                    disposed: false,
                });
            let old_generation = (record.generation != generation).then_some(record.generation);
            record.generation = generation;
            record.visible = visible;
            record.disposed = false;
            if visible && !state.quitting {
                state.residing_in_tray = false;
            }
            (old_generation, was_residing)
        };
        if let Some(old_generation) = old_generation {
            self.remove_surface_items(surface, old_generation);
        }
        let now_residing = lock(&self.inner.state).residing_in_tray;
        if visible && was_residing && !now_residing {
            self.inner.emit(TrayEvent::ResidencyChanged {
                residing_in_tray: false,
            });
        }
    }

    pub fn advance_surface_generation(&self, surface: SurfaceId, generation: u64) {
        let visible = lock(&self.inner.state)
            .surfaces
            .get(&surface)
            .map(|record| record.visible && !record.disposed)
            .unwrap_or(true);
        self.attach_surface(surface, generation, visible);
    }

    pub fn notify_surface_visible(&self, surface: SurfaceId, generation: u64) {
        self.attach_surface(surface, generation, true);
    }

    pub fn dispose_surface(&self, surface: SurfaceId) {
        let (ids, was_residing) = {
            let mut state = lock(&self.inner.state);
            let was_residing = state.residing_in_tray;
            if let Some(record) = state.surfaces.get_mut(&surface) {
                record.disposed = true;
                record.visible = false;
            } else {
                state.surfaces.insert(
                    surface,
                    SurfaceRecord {
                        generation: 0,
                        visible: false,
                        disposed: true,
                    },
                );
            }
            let ids = state
                .items
                .iter()
                .filter(|(_, item)| item.scope.surface_id() == Some(surface))
                .map(|(id, _)| *id)
                .collect::<Vec<_>>();
            (ids, was_residing)
        };
        for id in ids {
            self.remove_item_inner(id, ResidencyBrokenReason::SurfaceDisposed);
        }
        if was_residing {
            self.inner
                .refresh_residency_if_needed(ResidencyBrokenReason::SurfaceDisposed);
        }
    }

    /// Dispose only the exact surface generation. A stale scope dropping after
    /// the same `SurfaceId` was recreated must not tear down the new generation.
    pub fn dispose_surface_generation(&self, surface: SurfaceId, generation: u64) {
        let (ids, was_residing) = {
            let mut state = lock(&self.inner.state);
            let was_residing = state.residing_in_tray;
            if state
                .surfaces
                .get(&surface)
                .is_some_and(|record| record.generation == generation)
                && let Some(record) = state.surfaces.get_mut(&surface)
            {
                record.disposed = true;
                record.visible = false;
            }
            let ids = state
                .items
                .iter()
                .filter(|(_, item)| {
                    item.scope.surface_id() == Some(surface)
                        && item.scope.generation() == Some(generation)
                })
                .map(|(id, _)| *id)
                .collect::<Vec<_>>();
            (ids, was_residing)
        };
        for id in ids {
            self.remove_item_inner(id, ResidencyBrokenReason::SurfaceDisposed);
        }
        if was_residing {
            self.inner
                .refresh_residency_if_needed(ResidencyBrokenReason::SurfaceDisposed);
        }
    }

    fn remove_surface_items(&self, surface: SurfaceId, generation: u64) {
        let ids = {
            let state = lock(&self.inner.state);
            state
                .items
                .iter()
                .filter(|(_, item)| {
                    item.scope.surface_id() == Some(surface)
                        && item.scope.generation() == Some(generation)
                })
                .map(|(id, _)| *id)
                .collect::<Vec<_>>()
        };
        for id in ids {
            self.remove_item_inner(id, ResidencyBrokenReason::SurfaceDisposed);
        }
    }

    pub fn notify_app_phase(&self, phase: AppPhase) {
        if matches!(phase, AppPhase::Terminating) {
            self.request_quit();
            return;
        }
        let mut state = lock(&self.inner.state);
        if !state.quitting {
            state.phase = phase;
        }
    }

    pub fn notify_lifecycle(&self, lifecycle: &LifecycleState) {
        let phase = lifecycle.phase();
        if matches!(phase, AppPhase::Terminating) {
            self.request_quit();
            return;
        }
        {
            let mut state = lock(&self.inner.state);
            if !state.quitting {
                state.phase = phase;
            }
        }
        let mut incoming = lifecycle.surfaces().collect::<Vec<_>>();
        incoming.sort_by_key(|(surface, _)| surface.0);
        let tracked = {
            let state = lock(&self.inner.state);
            let mut tracked = state.surfaces.keys().copied().collect::<Vec<_>>();
            tracked.sort_by_key(|surface| surface.0);
            tracked
        };
        for surface in tracked {
            if !incoming.iter().any(|(present, _)| *present == surface) {
                self.dispose_surface(surface);
            }
        }
        for (surface, snapshot) in incoming {
            self.attach_surface(
                surface,
                snapshot.generation,
                snapshot.visibility.is_visible(),
            );
        }
    }

    /// Explicit quit always wins. It clears hidden residency, removes every
    /// native item at most once and makes later close evaluation return quit.
    pub fn request_quit(&self) {
        let (items, was_residing) = {
            let mut state = lock(&self.inner.state);
            if state.quitting {
                return;
            }
            state.quitting = true;
            state.phase = AppPhase::Terminating;
            let was_residing = std::mem::replace(&mut state.residing_in_tray, false);
            let items = state
                .items
                .values()
                .map(|item| {
                    let mut runtime = item.runtime();
                    runtime.removed = true;
                    runtime.hosted = false;
                    (item.id, runtime.native_item_token)
                })
                .collect::<Vec<_>>();
            state.items.clear();
            state.keys.clear();
            state.reserved_keys.clear();
            (items, was_residing)
        };
        self.inner.app_owner.dispose();
        for (id, token) in &items {
            let _ = self.inner.provider.remove_item(*token);
            self.inner.emit(TrayEvent::StatusItemRemoved { id: *id });
        }
        self.inner.emit(TrayEvent::QuitStarted {
            removed_count: items.len(),
        });
        if was_residing {
            self.inner.emit(TrayEvent::ResidencyChanged {
                residing_in_tray: false,
            });
        }
    }

    /// Evaluate a close request for `surface` at `generation`.
    pub fn evaluate_close(&self, surface: SurfaceId, generation: u64) -> CloseInterceptionOutcome {
        let (quitting, visible_count, known, is_visible, policy) = {
            let state = lock(&self.inner.state);
            let record = state.surfaces.get(&surface);
            (
                state.quitting || matches!(state.phase, AppPhase::Terminating),
                state
                    .surfaces
                    .values()
                    .filter(|record| record.visible && !record.disposed)
                    .count(),
                record.is_some_and(|record| !record.disposed),
                record.is_some_and(|record| {
                    !record.disposed && record.visible && record.generation == generation
                }),
                state.policy.clone(),
            )
        };
        if quitting {
            let outcome = CloseInterceptionOutcome::AllowWindowCloseAndQuit;
            self.inner.emit(TrayEvent::CloseIntercepted {
                outcome: outcome.clone(),
            });
            return outcome;
        }
        if !known {
            let outcome = if visible_count > 0 {
                CloseInterceptionOutcome::AllowWindowCloseKeepRunning
            } else {
                CloseInterceptionOutcome::AllowWindowCloseAndQuit
            };
            self.inner.emit(TrayEvent::CloseIntercepted {
                outcome: outcome.clone(),
            });
            return outcome;
        }
        if !is_visible {
            let outcome = if visible_count > 0 {
                CloseInterceptionOutcome::AllowWindowCloseKeepRunning
            } else {
                CloseInterceptionOutcome::AllowWindowCloseAndQuit
            };
            self.inner.emit(TrayEvent::CloseIntercepted {
                outcome: outcome.clone(),
            });
            return outcome;
        }
        if visible_count > 1 {
            {
                let mut state = lock(&self.inner.state);
                if let Some(record) = state.surfaces.get_mut(&surface) {
                    record.visible = false;
                }
            }
            let outcome = CloseInterceptionOutcome::AllowWindowCloseKeepRunning;
            self.inner.emit(TrayEvent::CloseIntercepted {
                outcome: outcome.clone(),
            });
            return outcome;
        }
        match policy.window_close_policy {
            WindowClosePolicy::CloseNormally => {
                {
                    let mut state = lock(&self.inner.state);
                    if let Some(record) = state.surfaces.get_mut(&surface) {
                        record.visible = false;
                    }
                }
                let outcome = CloseInterceptionOutcome::AllowWindowCloseAndQuit;
                self.inner.emit(TrayEvent::CloseIntercepted {
                    outcome: outcome.clone(),
                });
                outcome
            }
            WindowClosePolicy::MinimizeToTrayWhenAvailable => {
                match self.inner.find_reachable_item(Some(surface), &policy) {
                    Ok(item_id) => {
                        let became_residing = {
                            let mut state = lock(&self.inner.state);
                            if state.quitting {
                                false
                            } else if let Some(record) = state.surfaces.get_mut(&surface) {
                                record.visible = false;
                                let previous = state.residing_in_tray;
                                state.residing_in_tray = true;
                                !previous
                            } else {
                                false
                            }
                        };
                        if became_residing {
                            self.inner.emit(TrayEvent::ResidencyChanged {
                                residing_in_tray: true,
                            });
                        }
                        let outcome = CloseInterceptionOutcome::HideWindowToTray { item_id };
                        self.inner.emit(TrayEvent::CloseIntercepted {
                            outcome: outcome.clone(),
                        });
                        outcome
                    }
                    Err(reason) => {
                        let outcome = fallback_outcome(reason, policy.shell_absence_fallback);
                        self.inner.emit(TrayEvent::CloseIntercepted {
                            outcome: outcome.clone(),
                        });
                        outcome
                    }
                }
            }
        }
    }

    /// Resolve the status surface this host should use.
    pub fn resolve_surface_strategy(&self) -> EffectiveStatusSurface {
        let policy = lock(&self.inner.state).policy.clone();
        let platform = self.inner.provider.platform();
        let alternative = policy.alternative_surface;
        if platform.is_mobile_or_web() {
            return if !alternative.is_none() && alternative.is_applicable_to(platform) {
                EffectiveStatusSurface::MobileAlternative(alternative)
            } else {
                EffectiveStatusSurface::UnsupportedNone
            };
        }
        let native = self
            .inner
            .provider
            .capability(TrayOperation::CreateStatusItem)
            .check()
            .is_ok()
            && self.inner.provider.shell_status().is_available();
        if native {
            return EffectiveStatusSurface::NativeTray;
        }
        if !alternative.is_none() && alternative.is_applicable_to(platform) {
            EffectiveStatusSurface::MobileAlternative(alternative)
        } else {
            EffectiveStatusSurface::UnsupportedNone
        }
    }

    /// Resolve an attached menu live for query or presentation.
    pub fn resolve_status_menu(
        &self,
        id: StatusItemId,
        context: Option<&OperationContext>,
    ) -> Result<Option<ResolvedContextMenu>, SystemError> {
        let (item, menus) = {
            let state = lock(&self.inner.state);
            let Some(item) = state.items.get(&id).cloned() else {
                return Err(capability_error(
                    ErrorCode::ContextExpired,
                    "Status item lease is no longer active",
                ));
            };
            (item, state.menus.clone())
        };
        let spec = item.runtime().spec.clone();
        let Some(menu) = spec.menu else {
            return Ok(None);
        };
        let menus = menus.ok_or_else(|| {
            capability_error(
                ErrorCode::Unavailable,
                "MenuCommandService is not installed for this tray service",
            )
        })?;
        let ui = {
            let state = lock(&self.inner.state);
            self.inner.effective_ui_locked(
                &state,
                &item,
                context.and_then(|context| context.ui.as_ref()),
            )?
        };
        ensure_menu_surface(&menus, &ui)?;
        let invocation = InvocationContext::new(OperationContext {
            ui: Some(ui),
            ..OperationContext::default()
        });
        Ok(Some(menus.resolve_context_menu(&menu, &invocation)?))
    }

    /// Update preferences and re-project every hosted icon deterministically.
    pub fn notify_preferences(&self, preferences: SystemPreferences) {
        lock(&self.inner.state).preferences = preferences;
        self.reproject_icons();
    }

    pub fn notify_scale_factor_milli(&self, scale_factor_milli: u32) {
        lock(&self.inner.state).scale_factor_milli = scale_factor_milli;
        self.reproject_icons();
    }

    /// Re-evaluate hidden residency after an external menu-context or
    /// capability change. Call this after mutating `MenuCommandService`
    /// context while the app is resident.
    pub fn refresh_residency(&self) {
        self.inner
            .refresh_residency_if_needed(ResidencyBrokenReason::ReachabilityChanged);
    }

    fn reproject_icons(&self) {
        let (preferences, scale, snapshots) = {
            let state = lock(&self.inner.state);
            let snapshots = state
                .items
                .values()
                .map(|item| {
                    let runtime = item.runtime();
                    ReprojItem {
                        item: item.clone(),
                        token: runtime.native_item_token,
                        shell_epoch: runtime.shell_epoch,
                        revision: runtime.revision,
                        spec: runtime.spec.clone(),
                        hosted: runtime.hosted,
                    }
                })
                .collect::<Vec<_>>();
            (
                state.preferences.clone(),
                state.scale_factor_milli,
                snapshots,
            )
        };
        let current_shell = self.inner.provider.shell_status().shell_epoch();
        let mut count = 0;
        for entry in snapshots {
            if !entry.hosted || current_shell != Some(entry.shell_epoch) {
                continue;
            }
            let icon = entry.spec.select_icon(&preferences, scale).clone();
            let update = TrayItemUpdate {
                projection: TrayItemProjection::from_validated(&entry.spec, &icon),
                revision: entry.revision.saturating_add(1),
            };
            if self
                .inner
                .provider
                .update_item(entry.token, &update)
                .is_err()
            {
                continue;
            }
            let state = lock(&self.inner.state);
            if !state
                .items
                .get(&entry.item.id)
                .is_some_and(|current| Rc::ptr_eq(current, &entry.item))
            {
                // The removal path already asked the provider to remove this
                // token; do not issue a duplicate remove.
                continue;
            }
            let mut runtime = entry.item.runtime();
            if runtime.removed || !runtime.hosted || runtime.shell_epoch != entry.shell_epoch {
                // A shell restart/loss owns cleanup of the old token.
                continue;
            }
            runtime.revision = update.revision;
            count += 1;
        }
        if count > 0 {
            self.inner.emit(TrayEvent::IconsReprojected { count });
        }
    }
}

impl TrayService {
    /// Present the attached menu through the provider after live resolution.
    pub fn present_status_menu(
        &self,
        id: StatusItemId,
        context: &OperationContext,
    ) -> ServiceResult<ResolvedContextMenu> {
        self.inner.present_status_menu(id, context)
    }

    /// Handle one provider click after correlation checks.
    pub fn handle_click_event(&self, event: &TrayClickEvent) -> ServiceResult<TrayClickOutcome> {
        self.inner.handle_click_event(event)
    }

    /// Handle a menu-item click after correlation and live re-evaluation.
    pub fn handle_menu_item_click_event(
        &self,
        event: &TrayMenuItemClick,
    ) -> ServiceResult<TrayClickOutcome> {
        self.inner.handle_menu_item_click_event(event)
    }
}

impl ServiceInner {
    fn effective_ui(
        &self,
        item: &ItemInner,
        explicit: Option<&UiContext>,
    ) -> Result<UiContext, SystemError> {
        let state = lock(&self.state);
        self.effective_ui_locked(&state, item, explicit)
    }

    pub fn present_status_menu(
        &self,
        id: StatusItemId,
        context: &OperationContext,
    ) -> ServiceResult<ResolvedContextMenu> {
        let operation = TrayOperation::PresentStatusMenu;
        if let Some(cancelled) = self.check_context(operation, context)? {
            return Ok(cancelled);
        }
        if let Err(error) = self.gate_capability(operation) {
            return self.fail(operation, error);
        }
        let shell_epoch = match self.require_shell() {
            Ok(epoch) => epoch,
            Err(error) => return self.fail(operation, error),
        };
        let prepared = {
            let state = lock(&self.state);
            if state.quitting || matches!(state.phase, AppPhase::Terminating) {
                Err(capability_error(
                    ErrorCode::Unavailable,
                    "Cannot present a tray menu while the application is quitting",
                ))
            } else if let Some(item) = state.items.get(&id).cloned() {
                let runtime = item.runtime();
                if runtime.removed || !runtime.hosted {
                    Err(capability_error(
                        ErrorCode::ContextExpired,
                        "Status item is not hosted",
                    ))
                } else if runtime.shell_epoch != shell_epoch {
                    Err(capability_error(
                        ErrorCode::ContextExpired,
                        "Status item belongs to an expired shell epoch",
                    ))
                } else {
                    match self.effective_ui_locked(&state, &item, context.ui.as_ref()) {
                        Err(error) => Err(error),
                        Ok(_) if runtime.spec.menu.is_none() => Err(capability_error(
                            ErrorCode::InvalidInput,
                            "Status item has no attached context menu",
                        )),
                        Ok(ui) => match state.menus.clone() {
                            Some(menus) => Ok(PreparedMenu {
                                ui,
                                menu: runtime.spec.menu.clone().expect("menu checked above"),
                                menus,
                                token: runtime.native_item_token,
                            }),
                            None => Err(capability_error(
                                ErrorCode::Unavailable,
                                "MenuCommandService is not installed for this tray service",
                            )),
                        },
                    }
                }
            } else {
                Err(capability_error(
                    ErrorCode::ContextExpired,
                    "Status item lease is no longer active",
                ))
            }
        };
        let prepared = match prepared {
            Ok(prepared) => prepared,
            Err(error) => return self.fail(operation, error),
        };

        if let Err(error) = ensure_menu_surface(&prepared.menus, &prepared.ui) {
            return self.fail(operation, error);
        }
        let invocation = InvocationContext::new(OperationContext {
            ui: Some(prepared.ui.clone()),
            ..OperationContext::default()
        })
        .with_trigger(TriggerSource::ContextMenu);
        let resolved = match prepared
            .menus
            .resolve_context_menu(&prepared.menu, &invocation)
        {
            Ok(resolved) => resolved,
            Err(error) => return self.fail(operation, error),
        };
        if let Some(reason) = context.cancellation.reason() {
            return Ok(Completion::Cancelled(reason));
        }
        if let Err(error) = self.provider.present_menu(prepared.token, &resolved) {
            return self.fail(operation, error);
        }
        self.emit(TrayEvent::MenuPresented {
            id,
            item_count: resolved.all_items().len(),
        });
        Ok(Completion::Completed(resolved))
    }

    /// Handle one provider click after correlation checks. Unknown, removed,
    /// stale-token, stale-epoch, disposed-surface and quitting callbacks are
    /// ignored without side effects.
    pub fn handle_click_event(&self, event: &TrayClickEvent) -> ServiceResult<TrayClickOutcome> {
        let correlation =
            match self.correlate(event.item_id, event.native_item_token, event.shell_epoch) {
                Ok(correlation) => correlation,
                Err(reason) => return self.stale_click(Some(event.item_id), reason),
            };
        let spec = correlation.spec;
        if !spec.visible {
            return self.stale_click(Some(event.item_id), StaleCallbackReason::DisabledOrHidden);
        }
        let Some(action) = spec.click_actions.get(&event.trigger).cloned() else {
            return Ok(Completion::Completed(TrayClickOutcome::NoAction));
        };
        let outcome = match action.clone() {
            TrayClickAction::OpenMenu => {
                let ui = match self.effective_ui(&correlation.item, None) {
                    Ok(ui) => ui,
                    Err(_) => {
                        return self.stale_click(
                            Some(event.item_id),
                            StaleCallbackReason::UiContextUnavailable,
                        );
                    }
                };
                let context = OperationContext {
                    ui: Some(ui),
                    ..OperationContext::default()
                };
                match self.present_status_menu(event.item_id, &context) {
                    Ok(Completion::Completed(_)) => {
                        Ok(Completion::Completed(TrayClickOutcome::MenuPresented))
                    }
                    Ok(Completion::Cancelled(reason)) => Ok(Completion::Cancelled(reason)),
                    Err(error) => Err(error),
                }
            }
            TrayClickAction::RestoreOrFocusApp => {
                let was_residing = {
                    let mut state = lock(&self.state);
                    std::mem::replace(&mut state.residing_in_tray, false)
                };
                if was_residing {
                    self.emit(TrayEvent::ResidencyChanged {
                        residing_in_tray: false,
                    });
                }
                Ok(Completion::Completed(TrayClickOutcome::RestoreRequested))
            }
            TrayClickAction::InvokeCommand {
                command_id,
                payload,
            } => {
                let ui = match self.effective_ui(&correlation.item, None) {
                    Ok(ui) => ui,
                    Err(_) => {
                        return self.stale_click(
                            Some(event.item_id),
                            StaleCallbackReason::UiContextUnavailable,
                        );
                    }
                };
                let Some(menus) = correlation.menus.clone() else {
                    return self.stale_click(
                        Some(event.item_id),
                        StaleCallbackReason::UiContextUnavailable,
                    );
                };
                if let Err(error) = ensure_menu_surface(&menus, &ui) {
                    return self.fail(TrayOperation::PresentStatusMenu, error);
                }
                let invocation = InvocationContext::new(OperationContext {
                    ui: Some(ui.clone()),
                    ..OperationContext::default()
                })
                .with_trigger(TriggerSource::SystemAction);
                match poll_command_request(menus.invoke_command(&command_id, payload, invocation)) {
                    Ok(PolledCommand::Completed) | Ok(PolledCommand::Pending) => {
                        Ok(Completion::Completed(TrayClickOutcome::CommandDispatched {
                            command_id,
                        }))
                    }
                    Ok(PolledCommand::Cancelled) => {
                        Ok(Completion::Completed(TrayClickOutcome::CommandCancelled {
                            command_id,
                        }))
                    }
                    Err(error) => Err(error),
                }
            }
            TrayClickAction::DispatchSystemAction { action_id } => {
                let ui = match self.effective_ui(&correlation.item, None) {
                    Ok(ui) => ui,
                    Err(_) => {
                        return self.stale_click(
                            Some(event.item_id),
                            StaleCallbackReason::UiContextUnavailable,
                        );
                    }
                };
                let Some(menus) = correlation.menus.clone() else {
                    return self.stale_click(
                        Some(event.item_id),
                        StaleCallbackReason::UiContextUnavailable,
                    );
                };
                if let Err(error) = ensure_menu_surface(&menus, &ui) {
                    return self.fail(TrayOperation::PresentStatusMenu, error);
                }
                let invocation = InvocationContext::new(OperationContext {
                    ui: Some(ui.clone()),
                    ..OperationContext::default()
                })
                .with_trigger(TriggerSource::SystemAction);
                match poll_command_request(menus.dispatch_system_action(
                    &action_id,
                    &BTreeMap::new(),
                    invocation,
                )) {
                    Ok(PolledCommand::Completed) | Ok(PolledCommand::Pending) => {
                        Ok(Completion::Completed(
                            TrayClickOutcome::SystemActionDispatched { action_id },
                        ))
                    }
                    Ok(PolledCommand::Cancelled) => Ok(Completion::Completed(
                        TrayClickOutcome::SystemActionCancelled { action_id },
                    )),
                    Err(error) => Err(error),
                }
            }
        };
        if let Ok(Completion::Completed(outcome)) = &outcome
            && !matches!(
                outcome,
                TrayClickOutcome::NoAction | TrayClickOutcome::Ignored(_)
            )
        {
            self.emit(TrayEvent::ClickDispatched {
                id: event.item_id,
                trigger: event.trigger,
                action,
            });
        }
        outcome
    }

    /// Handle a menu-item click after correlation and live menu re-evaluation.
    pub fn handle_menu_item_click_event(
        &self,
        event: &TrayMenuItemClick,
    ) -> ServiceResult<TrayClickOutcome> {
        let correlation =
            match self.correlate(event.item_id, event.native_item_token, event.shell_epoch) {
                Ok(correlation) => correlation,
                Err(reason) => return self.stale_click(Some(event.item_id), reason),
            };
        let spec = correlation.spec;
        if !spec.visible {
            return self.stale_click(Some(event.item_id), StaleCallbackReason::DisabledOrHidden);
        }
        let Some(menu) = spec.menu else {
            return self.stale_click(Some(event.item_id), StaleCallbackReason::DisabledOrHidden);
        };
        let ui = match self.effective_ui(&correlation.item, None) {
            Ok(ui) => ui,
            Err(_) => {
                return self.stale_click(
                    Some(event.item_id),
                    StaleCallbackReason::UiContextUnavailable,
                );
            }
        };
        let Some(menus) = correlation.menus.clone() else {
            return self.stale_click(
                Some(event.item_id),
                StaleCallbackReason::UiContextUnavailable,
            );
        };
        if let Err(error) = ensure_menu_surface(&menus, &ui) {
            return self.fail(TrayOperation::PresentStatusMenu, error);
        }
        let invocation = InvocationContext::new(OperationContext {
            ui: Some(ui.clone()),
            ..OperationContext::default()
        })
        .with_trigger(TriggerSource::ContextMenu);
        let resolved = match menus.resolve_context_menu(&menu, &invocation) {
            Ok(resolved) => resolved,
            Err(error) => return self.fail(TrayOperation::PresentStatusMenu, error),
        };
        let Some(item) = resolved
            .all_items()
            .into_iter()
            .find(|item| item.command_id == event.command_id && item.visible && item.enabled)
            .cloned()
        else {
            return self.stale_click(Some(event.item_id), StaleCallbackReason::DisabledOrHidden);
        };
        match poll_command_request(menus.activate_context_menu_item(&item, invocation)) {
            Ok(PolledCommand::Completed) | Ok(PolledCommand::Pending) => {
                self.emit(TrayEvent::ClickDispatched {
                    id: event.item_id,
                    trigger: TrayClickTrigger::PrimaryClick,
                    action: TrayClickAction::invoke_command_with_payload(
                        item.command_id.clone(),
                        item.payload.clone(),
                    ),
                });
                Ok(Completion::Completed(TrayClickOutcome::CommandDispatched {
                    command_id: item.command_id,
                }))
            }
            Ok(PolledCommand::Cancelled) => {
                Ok(Completion::Completed(TrayClickOutcome::CommandCancelled {
                    command_id: item.command_id,
                }))
            }
            Err(error) => Err(error),
        }
    }
}

impl ServiceInner {
    fn correlate(
        &self,
        item_id: StatusItemId,
        native_item_token: u64,
        shell_epoch: u64,
    ) -> Result<Correlation, StaleCallbackReason> {
        let state = lock(&self.state);
        if state.quitting || matches!(state.phase, AppPhase::Terminating) {
            return Err(StaleCallbackReason::AppQuitting);
        }
        let Some(item) = state.items.get(&item_id).cloned() else {
            return Err(StaleCallbackReason::UnknownStatusItem);
        };
        let runtime = item.runtime();
        if runtime.removed || !runtime.hosted {
            return Err(StaleCallbackReason::RemovedStatusItem);
        }
        if runtime.shell_epoch != shell_epoch {
            return Err(StaleCallbackReason::ShellEpochMismatch);
        }
        if runtime.native_item_token != native_item_token {
            return Err(StaleCallbackReason::NativeTokenMismatch);
        }
        if !matches!(
            state.shell,
            ShellTrayStatus::Available { shell_epoch: active } if active == shell_epoch
        ) {
            return Err(StaleCallbackReason::ShellEpochMismatch);
        }
        if let Some((surface, generation)) = item.scope.surface_id().zip(item.scope.generation()) {
            match state.surfaces.get(&surface) {
                Some(record) if !record.disposed && record.generation == generation => {}
                _ => return Err(StaleCallbackReason::SurfaceExpired),
            }
        }
        let spec = runtime.spec.clone();
        drop(runtime);
        Ok(Correlation {
            item,
            spec,
            menus: state.menus.clone(),
        })
    }

    fn handle_shell_lost(&self) {
        {
            let mut state = lock(&self.state);
            state.shell = ShellTrayStatus::Absent;
            for item in state.items.values() {
                let mut runtime = item.runtime();
                runtime.hosted = false;
            }
        }
        self.emit(TrayEvent::ShellLost);
        self.refresh_residency_if_needed(ResidencyBrokenReason::ShellLost);
    }

    fn handle_shell_restarted(&self, shell_epoch: u64) {
        if shell_epoch == 0 {
            return;
        }
        let start = {
            let mut state = lock(&self.state);
            if shell_epoch <= state.last_shell_epoch {
                return;
            }
            state.last_shell_epoch = shell_epoch;
            state.shell = ShellTrayStatus::Available { shell_epoch };
            state.pending_shell_epoch = Some(shell_epoch);
            for item in state.items.values() {
                let mut runtime = item.runtime();
                runtime.hosted = false;
            }
            if state.recovering_shell {
                false
            } else {
                state.recovering_shell = true;
                true
            }
        };
        if !start {
            return;
        }
        loop {
            let epoch = {
                let mut state = lock(&self.state);
                state.pending_shell_epoch.take()
            };
            let Some(epoch) = epoch else {
                break;
            };
            let restored = self.recover_shell_epoch(epoch);
            self.emit(TrayEvent::ShellRestartRecovered {
                shell_epoch: epoch,
                restored_count: restored,
            });
            if lock(&self.state).pending_shell_epoch.is_none() {
                break;
            }
        }
        lock(&self.state).recovering_shell = false;
        self.refresh_residency_if_needed(ResidencyBrokenReason::ShellRestartRecoveryFailed);
    }

    fn recover_shell_epoch(&self, shell_epoch: u64) -> usize {
        let (preferences, scale, entries) = {
            let state = lock(&self.state);
            let entries = state
                .items
                .values()
                .map(|item| {
                    let runtime = item.runtime();
                    RecoveryEntry {
                        item: item.clone(),
                        spec: runtime.spec.clone(),
                        revision: runtime.revision,
                        hosted: runtime.hosted,
                    }
                })
                .collect::<Vec<_>>();
            (state.preferences.clone(), state.scale_factor_milli, entries)
        };
        let mut restored = 0;
        for entry in entries {
            let still_needed = {
                let state = lock(&self.state);
                !state.quitting
                    && matches!(
                        state.shell,
                        ShellTrayStatus::Available { shell_epoch: active }
                            if active == shell_epoch
                    )
                    && state
                        .items
                        .get(&entry.item.id)
                        .is_some_and(|current| Rc::ptr_eq(current, &entry.item))
                    && !entry.hosted
                    && self.scope_is_attached_locked(&state, entry.item.scope)
            };
            if !still_needed {
                continue;
            }
            let icon = entry.spec.select_icon(&preferences, scale).clone();
            let request = TrayCreateRequest {
                item_id: entry.item.id,
                key: entry.spec.key.clone(),
                projection: TrayItemProjection::from_validated(&entry.spec, &icon),
                scope: entry.item.scope,
                shell_epoch,
                revision: entry.revision,
            };
            let grant = match self.provider.create_item(&request) {
                Ok(grant) => grant,
                Err(error) => {
                    self.emit_failure(TrayOperation::CreateStatusItem, &error);
                    continue;
                }
            };
            if grant.native_item_token == 0 {
                continue;
            }
            let current_shell = self.provider.shell_status();
            let accepted = {
                let state = lock(&self.state);
                let accepted = !state.quitting
                    && matches!(
                        current_shell,
                        ShellTrayStatus::Available { shell_epoch: active }
                            if active == shell_epoch
                    )
                    && matches!(
                        state.shell,
                        ShellTrayStatus::Available { shell_epoch: active }
                            if active == shell_epoch
                    )
                    && state
                        .items
                        .get(&entry.item.id)
                        .is_some_and(|current| Rc::ptr_eq(current, &entry.item))
                    && self.scope_is_attached_locked(&state, entry.item.scope);
                if accepted {
                    let mut runtime = entry.item.runtime();
                    if runtime.removed || runtime.revision != entry.revision {
                        false
                    } else {
                        runtime.native_item_token = grant.native_item_token;
                        runtime.shell_epoch = shell_epoch;
                        runtime.hosted = true;
                        true
                    }
                } else {
                    false
                }
            };
            if accepted {
                restored += 1;
            } else {
                self.cleanup_orphan(grant.native_item_token);
            }
        }
        restored
    }

    fn find_reachable_item(
        &self,
        excluded_surface: Option<SurfaceId>,
        policy: &TrayResidencyPolicy,
    ) -> Result<StatusItemId, ResidencyFallbackReason> {
        let shell = match self.provider.shell_status() {
            ShellTrayStatus::Available { shell_epoch } => shell_epoch,
            ShellTrayStatus::Absent => return Err(ResidencyFallbackReason::ShellAbsent),
        };
        if self
            .provider
            .capability(TrayOperation::CreateStatusItem)
            .check()
            .is_err()
            || self
                .provider
                .capability(TrayOperation::ManageCloseToTrayResidency)
                .check()
                .is_err()
        {
            return Err(ResidencyFallbackReason::CapabilityUnavailable);
        }
        let present_menu_supported = self
            .provider
            .capability(TrayOperation::PresentStatusMenu)
            .check()
            .is_ok();
        let (items, menus) = {
            let state = lock(&self.state);
            if state.quitting || matches!(state.phase, AppPhase::Terminating) {
                return Err(ResidencyFallbackReason::CapabilityUnavailable);
            }
            let items = state
                .items
                .values()
                .map(|item| {
                    let runtime = item.runtime();
                    ReachabilityItem {
                        id: item.id,
                        scope: item.scope,
                        effective_ui: self.effective_ui_locked(&state, item, None).ok(),
                        spec: runtime.spec.clone(),
                        shell_epoch: runtime.shell_epoch,
                        hosted: runtime.hosted,
                    }
                })
                .collect::<Vec<_>>();
            (items, state.menus.clone())
        };
        if items.is_empty() {
            return Err(ResidencyFallbackReason::NoHostedStatusItem);
        }
        let mut any_eligible = false;
        for item in items {
            if !item.hosted || !item.spec.visible || item.shell_epoch != shell {
                continue;
            }
            let live_scope = {
                let state = lock(&self.state);
                self.scope_is_live_locked(&state, item.scope, excluded_surface)
            };
            if !live_scope {
                continue;
            }
            any_eligible = true;
            if click_reachability(&item, policy) {
                return Ok(item.id);
            }
            if let Some(menu) = item.spec.menu.as_ref()
                && present_menu_supported
                && let Some(menus) = menus.clone()
                && let Some(ui) = item.effective_ui.as_ref()
            {
                if ensure_menu_surface(&menus, ui).is_err() {
                    continue;
                }
                let invocation = InvocationContext::new(OperationContext {
                    ui: Some(ui.clone()),
                    ..OperationContext::default()
                });
                if let Ok(resolved) = menus.resolve_context_menu(menu, &invocation)
                    && menu_reachability(&resolved, policy)
                {
                    return Ok(item.id);
                }
            }
        }
        if any_eligible {
            Err(ResidencyFallbackReason::NoReachableRestoreOrQuit)
        } else {
            Err(ResidencyFallbackReason::NoEligibleStatusItem)
        }
    }

    fn refresh_residency_if_needed(&self, reason: ResidencyBrokenReason) {
        let (residing, policy) = {
            let state = lock(&self.state);
            if !state.residing_in_tray {
                return;
            }
            (true, state.policy.clone())
        };
        if !residing || self.find_reachable_item(None, &policy).is_ok() {
            return;
        }
        let recovery = recovery_for(policy.shell_absence_fallback);
        let changed = {
            let mut state = lock(&self.state);
            if state.residing_in_tray {
                state.residing_in_tray = false;
                true
            } else {
                false
            }
        };
        if changed {
            self.emit(TrayEvent::ResidencyBroken { reason, recovery });
            self.emit(TrayEvent::ResidencyChanged {
                residing_in_tray: false,
            });
        }
    }
}

fn click_reachability(item: &ReachabilityItem, policy: &TrayResidencyPolicy) -> bool {
    let ui_available = item.effective_ui.is_some();
    item.spec.click_actions.values().any(|action| match action {
        TrayClickAction::RestoreOrFocusApp => true,
        TrayClickAction::InvokeCommand { command_id, .. } => {
            ui_available
                && (policy.is_restore_command(command_id) || policy.is_quit_command(command_id))
        }
        TrayClickAction::DispatchSystemAction { action_id } => {
            ui_available
                && (policy.is_restore_system_action(action_id)
                    || policy.is_quit_system_action(action_id))
        }
        TrayClickAction::OpenMenu => false,
    })
}

fn menu_reachability(resolved: &ResolvedContextMenu, policy: &TrayResidencyPolicy) -> bool {
    fn node_has_quit(node: &ResolvedMenuNode) -> bool {
        match node {
            ResolvedMenuNode::SystemRole(SystemMenuRole::Quit) => true,
            ResolvedMenuNode::Submenu(submenu) => submenu.nodes.iter().any(node_has_quit),
            ResolvedMenuNode::CommandItem(_)
            | ResolvedMenuNode::CheckItem(_)
            | ResolvedMenuNode::Separator
            | ResolvedMenuNode::SystemRole(_) => false,
        }
    }
    if resolved.nodes.iter().any(node_has_quit) {
        return true;
    }
    resolved.all_items().into_iter().any(|item| {
        item.visible
            && item.enabled
            && (policy.is_quit_command(&item.command_id)
                || policy.is_restore_command(&item.command_id))
    })
}

fn ensure_menu_surface(menus: &MenuCommandService, ui: &UiContext) -> Result<(), SystemError> {
    match menus.surface_generation(ui.surface) {
        Some(active) if active == ui.generation => Ok(()),
        Some(_) => Err(capability_error(
            ErrorCode::ContextExpired,
            "Tray menu surface generation is stale",
        )),
        None => menus.register_surface(ui.surface, ui.generation),
    }
}

struct Reservation {
    item_id: StatusItemId,
    reservation: u64,
    key: StatusItemKey,
    request: TrayCreateRequest,
}

struct PreparedUpdate {
    item: Rc<ItemInner>,
    token: u64,
    shell_epoch: u64,
    expected_revision: u64,
    validated: ValidatedStatusItemSpec,
    update: TrayItemUpdate,
}

struct RemovedItem {
    id: StatusItemId,
    token: u64,
}

struct PreparedMenu {
    ui: UiContext,
    menu: ContextMenu,
    menus: MenuCommandService,
    token: u64,
}

struct ReprojItem {
    item: Rc<ItemInner>,
    token: u64,
    shell_epoch: u64,
    revision: u64,
    spec: ValidatedStatusItemSpec,
    hosted: bool,
}

struct RecoveryEntry {
    item: Rc<ItemInner>,
    spec: ValidatedStatusItemSpec,
    revision: u64,
    hosted: bool,
}

struct ReachabilityItem {
    id: StatusItemId,
    scope: TrayLeaseScope,
    effective_ui: Option<UiContext>,
    spec: ValidatedStatusItemSpec,
    shell_epoch: u64,
    hosted: bool,
}

struct Correlation {
    item: Rc<ItemInner>,
    spec: ValidatedStatusItemSpec,
    menus: Option<MenuCommandService>,
}

enum PolledCommand {
    Completed,
    Cancelled,
    Pending,
}

fn poll_command_request(
    mut request: Request<CommandPayload>,
) -> Result<PolledCommand, SystemError> {
    let waker = Waker::noop();
    let mut context = Context::from_waker(waker);
    match Pin::new(&mut request).poll(&mut context) {
        Poll::Ready(Ok(Completion::Completed(_))) => Ok(PolledCommand::Completed),
        Poll::Ready(Ok(Completion::Cancelled(_))) => Ok(PolledCommand::Cancelled),
        Poll::Ready(Err(error)) => Err(error),
        Poll::Pending => Ok(PolledCommand::Pending),
    }
}

impl TrayService {
    pub fn subscribe(&self) -> EventSubscription<TrayEvent> {
        let live = Rc::new(AtomicBool::new(true));
        let cleanup = live.clone();
        let (sender, subscription) = event_channel(self.inner.event_capacity, move || {
            cleanup.store(false, Ordering::Release);
        });
        let mut state = lock(&self.inner.state);
        state
            .subscribers
            .retain(|(live, _)| live.load(Ordering::Acquire));
        state.subscribers.push((live, sender));
        subscription
    }

    pub fn event_revision(&self) -> u64 {
        let mut state = lock(&self.inner.state);
        state
            .subscribers
            .retain(|(live, _)| live.load(Ordering::Acquire));
        state
            .subscribers
            .iter()
            .map(|(_, sender)| sender.revision())
            .max()
            .unwrap_or(0)
    }
}

impl std::fmt::Debug for TrayService {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("TrayService")
            .field("shell", &self.shell_status())
            .field("items", &self.active_item_count())
            .field("residing_in_tray", &self.residing_in_tray())
            .finish()
    }
}
