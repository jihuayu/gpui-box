//! Explicit tray-provider boundary plus unavailable and deterministic fake
//! providers.
//!
//! A provider owns native status-item handles. It must never hold its own state
//! lock while notifying a listener, and it must never report a created item when
//! the platform refused the request. The service re-validates shell epoch,
//! lease liveness and lifecycle state after every grant.

use crate::{
    ErrorCode, PlatformTarget, StatusIconVariant, StatusItemId, StatusItemKey, TrayClickTrigger,
    TrayLeaseScope, TrayOperation, ValidatedStatusItemSpec,
};
use gpui_menus::{CommandId, ResolvedContextMenu};
use gpui_system::{
    Availability, Capability, Configuration, ErrorCode as SystemErrorCode, Evidence,
    PermissionState, ProviderManifest, Support, SystemError,
};
use std::{
    any::Any,
    cell::RefCell,
    collections::BTreeMap,
    fmt,
    rc::{Rc, Weak},
};

/// Conservative manifest: this contract crate installs no native provider.
pub const PROVIDER_MANIFEST: [ProviderManifest; 5] = gpui_system::PROVIDER_MANIFEST;

/// The shell host's current tray availability and monotonic epoch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ShellTrayStatus {
    /// The shell is accepting status items for this epoch.
    Available { shell_epoch: u64 },
    /// No StatusNotifierWatcher, Explorer notification area or menu-bar host.
    Absent,
}

impl ShellTrayStatus {
    pub const fn available(shell_epoch: u64) -> Self {
        Self::Available { shell_epoch }
    }

    pub const fn absent() -> Self {
        Self::Absent
    }

    pub const fn is_available(self) -> bool {
        matches!(self, Self::Available { .. })
    }

    pub const fn shell_epoch(self) -> Option<u64> {
        match self {
            Self::Available { shell_epoch } => Some(shell_epoch),
            Self::Absent => None,
        }
    }
}

/// A provider-facing projection of a validated status item. It contains no
/// command state; commands live exclusively in `gpui-menus`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrayItemProjection {
    pub title: Option<String>,
    pub tooltip: Option<String>,
    pub accessibility_label: String,
    pub icon: StatusIconVariant,
    pub visible: bool,
}

impl TrayItemProjection {
    pub fn from_validated(spec: &ValidatedStatusItemSpec, icon: &StatusIconVariant) -> Self {
        Self {
            title: spec.title.clone(),
            tooltip: spec.tooltip.clone(),
            accessibility_label: spec.accessibility_label.clone(),
            icon: icon.clone(),
            visible: spec.visible,
        }
    }
}

/// Everything a provider needs to create one native status item.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrayCreateRequest {
    pub item_id: StatusItemId,
    pub key: StatusItemKey,
    pub projection: TrayItemProjection,
    pub scope: TrayLeaseScope,
    pub shell_epoch: u64,
    pub revision: u64,
}

/// Everything a provider needs to update one native status item.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrayItemUpdate {
    pub projection: TrayItemProjection,
    pub revision: u64,
}

/// The native token returned by a successful provider create.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ProviderItemGrant {
    pub native_item_token: u64,
}

impl ProviderItemGrant {
    pub const fn new(native_item_token: u64) -> Self {
        Self { native_item_token }
    }
}

/// One physical activation callback. Correlation fields must match the live
/// lease before the service dispatches any action.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrayClickEvent {
    pub item_id: StatusItemId,
    pub native_item_token: u64,
    pub shell_epoch: u64,
    pub trigger: TrayClickTrigger,
}

/// One selected menu item callback. Resolution and enablement are re-evaluated
/// against the live `gpui-menus` context before invocation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrayMenuItemClick {
    pub item_id: StatusItemId,
    pub native_item_token: u64,
    pub shell_epoch: u64,
    pub command_id: CommandId,
}

/// Provider-to-service observations pushed synchronously.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TrayProviderNotice {
    /// The shell host disappeared; native items no longer exist.
    ShellLost,
    /// Explorer or `StatusNotifierWatcher` restarted with a strictly newer epoch.
    ShellRestarted { shell_epoch: u64 },
    /// A status item was clicked.
    Clicked(TrayClickEvent),
    /// An item in a presented context menu was selected.
    MenuItemClicked(TrayMenuItemClick),
}

/// Listener registered by a service on its provider.
///
/// Providers must notify outside their own state lock so a listener can safely
/// query capabilities, dispose a lease or remove a token.
pub trait TrayProviderListener {
    fn on_tray_provider_notice(&self, notice: TrayProviderNotice);
}

/// Explicit native/provider boundary.
///
/// Status-item methods run on the owning UI thread. `remove_item` must be
/// idempotent and must return false for an already-removed or shell-lost token.
pub trait TrayProvider {
    fn platform(&self) -> PlatformTarget;

    fn capability(&self, operation: TrayOperation) -> Capability<TrayOperation>;

    fn capabilities(&self) -> Vec<Capability<TrayOperation>> {
        TrayOperation::ALL
            .into_iter()
            .map(|operation| self.capability(operation))
            .collect()
    }

    fn shell_status(&self) -> ShellTrayStatus;

    fn create_item(&self, request: &TrayCreateRequest) -> Result<ProviderItemGrant, SystemError>;

    fn update_item(
        &self,
        native_item_token: u64,
        update: &TrayItemUpdate,
    ) -> Result<(), SystemError>;

    /// Remove a native item. Returns true exactly once for a live token.
    fn remove_item(&self, native_item_token: u64) -> bool;

    /// Present an already-resolved context menu. No command state is duplicated
    /// in the provider; selected ids are routed back through `gpui-menus`.
    fn present_menu(
        &self,
        native_item_token: u64,
        menu: &ResolvedContextMenu,
    ) -> Result<(), SystemError>;

    /// Attach a service listener. Weak retention avoids provider/service cycles.
    fn attach_listener(&self, listener: Weak<dyn TrayProviderListener>);

    fn as_any(&self) -> &dyn Any;
}

/// Contract-only provider used for mobile, Web and headless hosts.
///
/// Every operation is unsupported, the shell is absent, and creation never
/// fabricates a status item.
#[derive(Debug, Clone, Copy)]
pub struct UnsupportedTrayProvider {
    platform: PlatformTarget,
}

impl Default for UnsupportedTrayProvider {
    fn default() -> Self {
        Self::new(PlatformTarget::current().unwrap_or(PlatformTarget::Linux))
    }
}

impl UnsupportedTrayProvider {
    pub fn new(platform: PlatformTarget) -> Self {
        Self { platform }
    }
}

impl TrayProvider for UnsupportedTrayProvider {
    fn platform(&self) -> PlatformTarget {
        self.platform
    }

    fn capability(&self, operation: TrayOperation) -> Capability<TrayOperation> {
        Capability {
            operation,
            support: Support::Unsupported,
            availability: Availability::Unavailable,
            configuration: Configuration::Ready,
            permission: PermissionState::NotRequired,
            evidence: Evidence::ContractOnly,
            constraints: Vec::new(),
            observed_revision: 0,
        }
    }

    fn shell_status(&self) -> ShellTrayStatus {
        ShellTrayStatus::Absent
    }

    fn create_item(&self, _request: &TrayCreateRequest) -> Result<ProviderItemGrant, SystemError> {
        Err(SystemError::new(
            ErrorCode::Unsupported,
            "Native tray status items are unsupported on this target",
        ))
    }

    fn update_item(
        &self,
        _native_item_token: u64,
        _update: &TrayItemUpdate,
    ) -> Result<(), SystemError> {
        Err(SystemError::new(
            ErrorCode::Unsupported,
            "Native tray status items are unsupported on this target",
        ))
    }

    fn remove_item(&self, _native_item_token: u64) -> bool {
        false
    }

    fn present_menu(
        &self,
        _native_item_token: u64,
        _menu: &ResolvedContextMenu,
    ) -> Result<(), SystemError> {
        Err(SystemError::new(
            ErrorCode::Unsupported,
            "Native tray menus are unsupported on this target",
        ))
    }

    fn attach_listener(&self, _listener: Weak<dyn TrayProviderListener>) {}

    fn as_any(&self) -> &dyn Any {
        self
    }
}

fn capability_for(
    platform: PlatformTarget,
    operation: TrayOperation,
    revision: u64,
) -> Capability<TrayOperation> {
    let support = platform.support(operation);
    let supported = matches!(support, Support::Native | Support::Conditional);
    Capability {
        operation,
        support,
        availability: if supported {
            Availability::Available
        } else {
            Availability::Unavailable
        },
        configuration: Configuration::Ready,
        permission: PermissionState::NotRequired,
        evidence: Evidence::ContractOnly,
        constraints: Vec::new(),
        observed_revision: revision,
    }
}

/// The callback payload delivered to a `FakeTrayProvider` mid-create hook.
///
/// The hook runs after the token is allocated but before the grant reaches the
/// service. It may cancel a token, simulate shell loss or restart, dispose a
/// surface or request quit; the service must remove the returned token and
/// register no lease when any of those invalidate the create.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FakeCreateEvent {
    pub item_id: StatusItemId,
    pub key: StatusItemKey,
    pub native_item_token: u64,
    pub shell_epoch: u64,
}

/// Callback invoked by a fake provider between token allocation and grant.
pub type FakeCreateHook = Rc<dyn Fn(&FakeCreateEvent)>;

/// One active hosted item as observed by the fake provider.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FakeHostedItem {
    pub native_item_token: u64,
    pub item_id: StatusItemId,
    pub key: StatusItemKey,
    pub shell_epoch: u64,
    pub scope: TrayLeaseScope,
    pub revision: u64,
    pub projection: TrayItemProjection,
}

struct FakeState {
    platform: PlatformTarget,
    capabilities: BTreeMap<TrayOperation, Capability<TrayOperation>>,
    shell: ShellTrayStatus,
    last_shell_epoch: u64,
    preferences: gpui_lifecycle::SystemPreferences,
    scale_factor_milli: u32,
    listeners: Vec<Weak<dyn TrayProviderListener>>,
    next_token: u64,
    hosted: BTreeMap<u64, FakeHostedItem>,
    create_calls: Vec<TrayCreateRequest>,
    update_calls: Vec<(u64, TrayItemUpdate)>,
    remove_calls: Vec<u64>,
    menu_calls: Vec<(u64, ResolvedContextMenu)>,
    create_failure: Option<SystemError>,
    update_failure: Option<SystemError>,
    present_failure: Option<SystemError>,
    mid_create_hook: Option<FakeCreateHook>,
}

/// Deterministic provider for headless tests and examples.
///
/// The handle is `Clone` and interior-mutable, so a test can drive transitions
/// while the service owns its `Rc<dyn TrayProvider>`. All callbacks are
/// delivered synchronously and outside the fake's state lock.
#[derive(Clone)]
pub struct FakeTrayProvider {
    state: Rc<RefCell<FakeState>>,
}

fn lock<T>(value: &RefCell<T>) -> std::cell::RefMut<'_, T> {
    value.borrow_mut()
}

impl Default for FakeTrayProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl FakeTrayProvider {
    pub fn new() -> Self {
        Self::with_platform(PlatformTarget::current().unwrap_or(PlatformTarget::Linux))
    }

    pub fn with_platform(platform: PlatformTarget) -> Self {
        let preferences = gpui_lifecycle::SystemPreferences::default();
        let capabilities = TrayOperation::ALL
            .into_iter()
            .map(|operation| (operation, capability_for(platform, operation, 0)))
            .collect();
        Self {
            state: Rc::new(RefCell::new(FakeState {
                platform,
                capabilities,
                shell: ShellTrayStatus::Available { shell_epoch: 1 },
                last_shell_epoch: 1,
                preferences,
                scale_factor_milli: 1000,
                listeners: Vec::new(),
                next_token: 1,
                hosted: BTreeMap::new(),
                create_calls: Vec::new(),
                update_calls: Vec::new(),
                remove_calls: Vec::new(),
                menu_calls: Vec::new(),
                create_failure: None,
                update_failure: None,
                present_failure: None,
                mid_create_hook: None,
            })),
        }
    }

    pub fn platform(&self) -> PlatformTarget {
        lock(&self.state).platform
    }

    fn notify(&self, notice: TrayProviderNotice) {
        let listeners = {
            let mut state = lock(&self.state);
            state
                .listeners
                .retain(|listener| listener.strong_count() > 0);
            state
                .listeners
                .iter()
                .filter_map(Weak::upgrade)
                .collect::<Vec<_>>()
        };
        for listener in listeners {
            listener.on_tray_provider_notice(notice.clone());
        }
    }

    pub fn shell_status(&self) -> ShellTrayStatus {
        lock(&self.state).shell
    }

    pub fn set_shell_status(&self, status: ShellTrayStatus) {
        let mut state = lock(&self.state);
        if let ShellTrayStatus::Available { shell_epoch } = status {
            state.last_shell_epoch = state.last_shell_epoch.max(shell_epoch);
        }
        state.shell = status;
        if !status.is_available() {
            state.hosted.clear();
        }
    }

    pub fn shell_epoch(&self) -> Option<u64> {
        self.shell_status().shell_epoch()
    }

    /// Simulate the shell host disappearing and all native items vanishing.
    pub fn simulate_shell_lost(&self) {
        lock(&self.state).shell = ShellTrayStatus::Absent;
        {
            lock(&self.state).hosted.clear();
        }
        self.notify(TrayProviderNotice::ShellLost);
    }

    /// Simulate Explorer `TaskbarCreated` or a new `StatusNotifierWatcher`
    /// D-Bus owner. Returns the strictly newer epoch.
    pub fn simulate_shell_restarted(&self) -> u64 {
        let epoch = {
            let mut state = lock(&self.state);
            let next = state.last_shell_epoch.saturating_add(1).max(1);
            state.last_shell_epoch = next;
            state.shell = ShellTrayStatus::Available { shell_epoch: next };
            state.hosted.clear();
            next
        };
        self.notify(TrayProviderNotice::ShellRestarted { shell_epoch: epoch });
        epoch
    }

    pub fn set_capability(&self, operation: TrayOperation, capability: Capability<TrayOperation>) {
        lock(&self.state).capabilities.insert(operation, capability);
    }

    pub fn set_support(&self, operation: TrayOperation, support: Support) {
        let mut state = lock(&self.state);
        if let Some(capability) = state.capabilities.get_mut(&operation) {
            capability.support = support;
            capability.availability = if matches!(support, Support::Native | Support::Conditional) {
                Availability::Available
            } else {
                Availability::Unavailable
            };
        }
    }

    pub fn set_availability(&self, operation: TrayOperation, availability: Availability) {
        if let Some(capability) = lock(&self.state).capabilities.get_mut(&operation) {
            capability.availability = availability;
        }
    }

    pub fn set_configuration(&self, operation: TrayOperation, configuration: Configuration) {
        if let Some(capability) = lock(&self.state).capabilities.get_mut(&operation) {
            capability.configuration = configuration;
        }
    }

    pub fn set_preferences(&self, preferences: gpui_lifecycle::SystemPreferences) {
        lock(&self.state).preferences = preferences;
    }

    pub fn preferences(&self) -> gpui_lifecycle::SystemPreferences {
        lock(&self.state).preferences.clone()
    }

    pub fn set_scale_factor_milli(&self, scale_factor_milli: u32) {
        lock(&self.state).scale_factor_milli = scale_factor_milli;
    }

    pub fn scale_factor_milli(&self) -> u32 {
        lock(&self.state).scale_factor_milli
    }

    pub fn set_create_failure(&self, failure: Option<SystemError>) {
        lock(&self.state).create_failure = failure;
    }

    pub fn set_update_failure(&self, failure: Option<SystemError>) {
        lock(&self.state).update_failure = failure;
    }

    pub fn set_present_failure(&self, failure: Option<SystemError>) {
        lock(&self.state).present_failure = failure;
    }

    pub fn set_on_create_item(&self, hook: Option<FakeCreateHook>) {
        lock(&self.state).mid_create_hook = hook;
    }

    pub fn with_on_create_item(self, hook: FakeCreateHook) -> Self {
        self.set_on_create_item(Some(hook));
        self
    }

    pub fn create_calls(&self) -> Vec<TrayCreateRequest> {
        lock(&self.state).create_calls.clone()
    }

    pub fn update_calls(&self) -> Vec<(u64, TrayItemUpdate)> {
        lock(&self.state).update_calls.clone()
    }

    pub fn remove_calls(&self) -> Vec<u64> {
        lock(&self.state).remove_calls.clone()
    }

    pub fn menu_calls(&self) -> Vec<(u64, ResolvedContextMenu)> {
        lock(&self.state).menu_calls.clone()
    }

    pub fn hosted_items(&self) -> Vec<FakeHostedItem> {
        lock(&self.state).hosted.values().cloned().collect()
    }

    pub fn active_item_tokens(&self) -> Vec<u64> {
        lock(&self.state).hosted.keys().copied().collect()
    }

    pub fn create_call_count(&self) -> usize {
        lock(&self.state).create_calls.len()
    }

    pub fn update_call_count(&self) -> usize {
        lock(&self.state).update_calls.len()
    }

    pub fn remove_call_count(&self) -> usize {
        lock(&self.state).remove_calls.len()
    }

    pub fn find_hosted_by_item(&self, item_id: StatusItemId) -> Option<FakeHostedItem> {
        lock(&self.state)
            .hosted
            .values()
            .find(|item| item.item_id == item_id)
            .cloned()
    }

    pub fn simulate_click(&self, item_id: StatusItemId, trigger: TrayClickTrigger) -> bool {
        let Some(item) = self.find_hosted_by_item(item_id) else {
            return false;
        };
        self.simulate_click_with_correlation(
            item_id,
            item.native_item_token,
            item.shell_epoch,
            trigger,
        );
        true
    }

    pub fn simulate_click_with_correlation(
        &self,
        item_id: StatusItemId,
        native_item_token: u64,
        shell_epoch: u64,
        trigger: TrayClickTrigger,
    ) {
        self.simulate_click_event(TrayClickEvent {
            item_id,
            native_item_token,
            shell_epoch,
            trigger,
        });
    }

    pub fn simulate_click_event(&self, event: TrayClickEvent) {
        self.notify(TrayProviderNotice::Clicked(event));
    }

    pub fn simulate_menu_item_click(&self, item_id: StatusItemId, command_id: CommandId) -> bool {
        let Some(item) = self.find_hosted_by_item(item_id) else {
            return false;
        };
        self.simulate_menu_item_click_with_correlation(
            item_id,
            item.native_item_token,
            item.shell_epoch,
            command_id,
        );
        true
    }

    pub fn simulate_menu_item_click_with_correlation(
        &self,
        item_id: StatusItemId,
        native_item_token: u64,
        shell_epoch: u64,
        command_id: CommandId,
    ) {
        self.simulate_menu_item_click_event(TrayMenuItemClick {
            item_id,
            native_item_token,
            shell_epoch,
            command_id,
        });
    }

    pub fn simulate_menu_item_click_event(&self, event: TrayMenuItemClick) {
        self.notify(TrayProviderNotice::MenuItemClicked(event));
    }
}

impl TrayProvider for FakeTrayProvider {
    fn platform(&self) -> PlatformTarget {
        FakeTrayProvider::platform(self)
    }

    fn capability(&self, operation: TrayOperation) -> Capability<TrayOperation> {
        let state = lock(&self.state);
        state
            .capabilities
            .get(&operation)
            .cloned()
            .unwrap_or_else(|| capability_for(state.platform, operation, 0))
    }

    fn shell_status(&self) -> ShellTrayStatus {
        FakeTrayProvider::shell_status(self)
    }

    fn create_item(&self, request: &TrayCreateRequest) -> Result<ProviderItemGrant, SystemError> {
        let (token, hook) = {
            let mut state = lock(&self.state);
            state.create_calls.push(request.clone());
            if let Some(failure) = state.create_failure.clone() {
                return Err(failure);
            }
            let token = state.next_token;
            state.next_token = state.next_token.saturating_add(1);
            let hook = state.mid_create_hook.clone();
            (token, hook)
        };
        if let Some(hook) = hook {
            hook(&FakeCreateEvent {
                item_id: request.item_id,
                key: request.key.clone(),
                native_item_token: token,
                shell_epoch: request.shell_epoch,
            });
        }
        {
            let mut state = lock(&self.state);
            state.hosted.insert(
                token,
                FakeHostedItem {
                    native_item_token: token,
                    item_id: request.item_id,
                    key: request.key.clone(),
                    shell_epoch: request.shell_epoch,
                    scope: request.scope,
                    revision: request.revision,
                    projection: request.projection.clone(),
                },
            );
        }
        Ok(ProviderItemGrant::new(token))
    }

    fn update_item(
        &self,
        native_item_token: u64,
        update: &TrayItemUpdate,
    ) -> Result<(), SystemError> {
        let mut state = lock(&self.state);
        state.update_calls.push((native_item_token, update.clone()));
        if let Some(failure) = state.update_failure.clone() {
            return Err(failure);
        }
        let Some(item) = state.hosted.get_mut(&native_item_token) else {
            return Err(SystemError::new(
                SystemErrorCode::Unavailable,
                "Native status item is not hosted",
            ));
        };
        item.projection = update.projection.clone();
        item.revision = update.revision;
        Ok(())
    }

    fn remove_item(&self, native_item_token: u64) -> bool {
        let mut state = lock(&self.state);
        state.remove_calls.push(native_item_token);
        state.hosted.remove(&native_item_token).is_some()
    }

    fn present_menu(
        &self,
        native_item_token: u64,
        menu: &ResolvedContextMenu,
    ) -> Result<(), SystemError> {
        let mut state = lock(&self.state);
        state.menu_calls.push((native_item_token, menu.clone()));
        if let Some(failure) = state.present_failure.clone() {
            return Err(failure);
        }
        if !state.hosted.contains_key(&native_item_token) {
            return Err(SystemError::new(
                SystemErrorCode::Unavailable,
                "Native status item is not hosted",
            ));
        }
        Ok(())
    }

    fn attach_listener(&self, listener: Weak<dyn TrayProviderListener>) {
        let mut state = lock(&self.state);
        state
            .listeners
            .retain(|existing| existing.strong_count() > 0);
        state.listeners.push(listener);
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

impl fmt::Debug for FakeTrayProvider {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let state = lock(&self.state);
        formatter
            .debug_struct("FakeTrayProvider")
            .field("platform", &state.platform)
            .field("shell", &state.shell)
            .field("hosted_items", &state.hosted.len())
            .field("create_calls", &state.create_calls.len())
            .finish()
    }
}
