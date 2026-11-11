//! Explicit provider boundary plus unavailable and deterministic fake
//! providers.

use crate::{
    BatteryStatus, ChargingState, IneffectiveInhibitionReason, LowPowerMode, MemoryCondition,
    MemoryStatus, PowerSnapshot, PowerSource, PowerSourceStatus, ProviderInhibitionGrant,
    SessionLockState, SessionLockStatus, SignalSource, SleepResumeState, SleepResumeStatus,
    ThermalCondition, ThermalStatus, WakeInhibitionKind, WakeReason, WakeRevocationReason,
};
use gpui_permissions::{PermissionKind, PermissionStatus};
use gpui_system::{
    Availability, Capability, Configuration, ErrorCode, Evidence, PermissionState,
    ProviderManifest, Support, SurfaceId, SystemError,
};
use serde::{Deserialize, Serialize};
use std::{
    any::Any,
    collections::BTreeMap,
    fmt,
    sync::{Arc, Mutex, Weak},
};

/// The permission domain used for background wake inhibition.
pub const WAKE_LOCK_PERMISSION: PermissionKind = PermissionKind::SpecialAccess("wake_lock");

/// Capability-shaped operations exposed by a power provider.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PowerOperation {
    ObservePower,
    ObserveSession,
    ObserveThermalMemory,
    InhibitSystemSleep,
    InhibitDisplaySleep,
}

impl PowerOperation {
    pub const ALL: [Self; 5] = [
        Self::ObservePower,
        Self::ObserveSession,
        Self::ObserveThermalMemory,
        Self::InhibitSystemSleep,
        Self::InhibitDisplaySleep,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ObservePower => "observe_power",
            Self::ObserveSession => "observe_session",
            Self::ObserveThermalMemory => "observe_thermal_memory",
            Self::InhibitSystemSleep => "inhibit_system_sleep",
            Self::InhibitDisplaySleep => "inhibit_display_sleep",
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::ObservePower => "ObservePower",
            Self::ObserveSession => "ObserveSession",
            Self::ObserveThermalMemory => "ObserveThermalMemory",
            Self::InhibitSystemSleep => "InhibitSystemSleep",
            Self::InhibitDisplaySleep => "InhibitDisplaySleep",
        }
    }

    /// True for operations that hold a native wake-prevention token.
    pub const fn is_inhibition(self) -> bool {
        matches!(self, Self::InhibitSystemSleep | Self::InhibitDisplaySleep)
    }

    /// True when the operation requires the `wake_lock` special permission on
    /// platforms that declare one.
    pub const fn requires_wake_lock(self) -> bool {
        self.is_inhibition()
    }

    /// The operation used to acquire a lease of `kind`.
    pub const fn for_kind(kind: WakeInhibitionKind) -> Self {
        kind.operation()
    }
}

impl fmt::Display for PowerOperation {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// Conservative manifest: this contract crate installs no native provider.
pub const PROVIDER_MANIFEST: [ProviderManifest; 5] = gpui_system::PROVIDER_MANIFEST;

/// The provider-facing shape of one acquisition attempt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PowerInhibitionRequest {
    pub kind: WakeInhibitionKind,
    pub reason: WakeReason,
    pub surface: Option<SurfaceId>,
    pub generation: Option<u64>,
}

/// Provider-to-service observations pushed synchronously.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PowerProviderNotice {
    /// A fresh coherent snapshot.
    SnapshotUpdated(PowerSnapshot),
    /// The OS revoked one native token.
    TokenRevoked {
        native_token: u64,
        reason: WakeRevocationReason,
    },
    /// The OS revoked every native token.
    AllTokensRevoked { reason: WakeRevocationReason },
    /// Permission status changed; a lost wake-lock permission revokes leases.
    PermissionChanged {
        kind: PermissionKind,
        status: PermissionStatus,
    },
}

/// Listener registered by a service on its provider.
///
/// Providers must notify outside their own state lock so a listener can safely
/// query capabilities or release a token.
pub trait PowerProviderListener: Send + Sync + 'static {
    fn on_power_provider_notice(&self, notice: PowerProviderNotice);
}

/// Explicit native/provider boundary.
///
/// Every method is safe to call from background threads. Capability snapshots
/// are advisory; the service rechecks permission and live state immediately
/// before acquisition and again before registering the returned token.
/// `acquire_inhibition` must never report `Effective` when the underlying
/// platform request was refused.
pub trait PowerProvider: Send + Sync + 'static {
    fn capability(&self, operation: PowerOperation) -> Capability<PowerOperation>;

    fn capabilities(&self) -> Vec<Capability<PowerOperation>> {
        PowerOperation::ALL
            .into_iter()
            .map(|operation| self.capability(operation))
            .collect()
    }

    /// Read the current coherent observation.
    fn snapshot(&self) -> Result<PowerSnapshot, SystemError>;

    /// Authoritative permission status for `kind`.
    fn permission_status(&self, kind: PermissionKind) -> PermissionStatus;

    /// Ask for a native inhibition.
    fn acquire_inhibition(
        &self,
        request: &PowerInhibitionRequest,
    ) -> Result<ProviderInhibitionGrant, SystemError>;

    /// Release a previously granted token. Returns false when the token is not
    /// active, including after an OS-initiated revocation.
    fn release_inhibition(&self, native_token: u64) -> bool;

    /// Attach a service listener. Weak retention avoids provider/service cycles.
    fn attach_listener(&self, listener: Weak<dyn PowerProviderListener>);

    fn as_any(&self) -> &dyn Any;
}

/// Contract-only provider used when no native host adapter is installed.
///
/// Observations stay explicitly unknown and every operation is unsupported.
/// It never fabricates AC power, an unlocked session or a nominal thermal state.
#[derive(Debug, Clone, Copy, Default)]
pub struct UnavailablePowerProvider;

impl UnavailablePowerProvider {
    pub const fn new() -> Self {
        Self
    }
}

impl PowerProvider for UnavailablePowerProvider {
    fn capability(&self, operation: PowerOperation) -> Capability<PowerOperation> {
        Capability {
            operation,
            support: Support::Conditional,
            availability: Availability::Unavailable,
            configuration: Configuration::Ready,
            permission: PermissionState::NotRequired,
            evidence: Evidence::ContractOnly,
            constraints: Vec::new(),
            observed_revision: 0,
        }
    }

    fn snapshot(&self) -> Result<PowerSnapshot, SystemError> {
        Ok(PowerSnapshot::unknown(SignalSource::ProviderUnavailable))
    }

    fn permission_status(&self, _kind: PermissionKind) -> PermissionStatus {
        PermissionStatus::Unavailable
    }

    fn acquire_inhibition(
        &self,
        _request: &PowerInhibitionRequest,
    ) -> Result<ProviderInhibitionGrant, SystemError> {
        Err(SystemError::new(
            ErrorCode::Unavailable,
            "No native power provider is installed",
        ))
    }

    fn release_inhibition(&self, _native_token: u64) -> bool {
        false
    }

    fn attach_listener(&self, _listener: Weak<dyn PowerProviderListener>) {}

    fn as_any(&self) -> &dyn Any {
        self
    }
}

fn capability_for(
    operation: PowerOperation,
    permission: PermissionStatus,
    revision: u64,
) -> Capability<PowerOperation> {
    Capability {
        operation,
        support: Support::Native,
        availability: Availability::Available,
        configuration: Configuration::Ready,
        permission: if operation.requires_wake_lock() {
            permission.permission_state()
        } else {
            PermissionState::NotRequired
        },
        evidence: Evidence::ContractOnly,
        constraints: Vec::new(),
        observed_revision: revision,
    }
}

struct FakeState {
    snapshot: PowerSnapshot,
    capabilities: BTreeMap<PowerOperation, Capability<PowerOperation>>,
    permission: PermissionStatus,
    inhibition_modes: BTreeMap<WakeInhibitionKind, IneffectiveInhibitionReason>,
    hidden_document: bool,
    battery_saver_override: bool,
    listeners: Vec<Weak<dyn PowerProviderListener>>,
    next_token: u64,
    active_tokens: BTreeMap<u64, WakeInhibitionKind>,
    released_tokens: Vec<u64>,
    revoked_tokens: Vec<(u64, WakeRevocationReason)>,
    release_calls: u64,
    acquire_calls: u64,
}

/// Deterministic provider for headless tests and examples.
///
/// The handle is `Clone` and interior-mutable, so a test can drive transitions
/// while the service still owns its `Arc<dyn PowerProvider>`.
#[derive(Clone)]
pub struct FakePowerProvider {
    state: Arc<Mutex<FakeState>>,
}

fn lock<T>(value: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    value
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

impl Default for FakePowerProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl FakePowerProvider {
    pub fn new() -> Self {
        let snapshot = PowerSnapshot::unknown(SignalSource::Unknown);
        let permission = PermissionStatus::NotRequired;
        let capabilities = PowerOperation::ALL
            .into_iter()
            .map(|operation| {
                (
                    operation,
                    capability_for(operation, permission, snapshot.revision),
                )
            })
            .collect();
        Self {
            state: Arc::new(Mutex::new(FakeState {
                snapshot,
                capabilities,
                permission,
                inhibition_modes: BTreeMap::new(),
                hidden_document: false,
                battery_saver_override: false,
                listeners: Vec::new(),
                next_token: 1,
                active_tokens: BTreeMap::new(),
                released_tokens: Vec::new(),
                revoked_tokens: Vec::new(),
                release_calls: 0,
                acquire_calls: 0,
            })),
        }
    }

    fn notify(&self, notice: PowerProviderNotice) {
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
            listener.on_power_provider_notice(notice.clone());
        }
    }

    fn mutate_snapshot(&self, update: impl FnOnce(&mut PowerSnapshot)) {
        let notice = {
            let mut state = lock(&self.state);
            let previous = state.snapshot.clone();
            update(&mut state.snapshot);
            if state.snapshot == previous {
                None
            } else {
                state.snapshot.revision = previous.revision.saturating_add(1);
                Some(PowerProviderNotice::SnapshotUpdated(state.snapshot.clone()))
            }
        };
        if let Some(notice) = notice {
            self.notify(notice);
        }
    }

    /// The current observation without a `Result` wrapper.
    pub fn snapshot(&self) -> PowerSnapshot {
        lock(&self.state).snapshot.clone()
    }

    pub fn set_snapshot(&self, snapshot: PowerSnapshot) {
        {
            let mut state = lock(&self.state);
            state.snapshot = snapshot;
        }
        let snapshot = self.snapshot();
        self.notify(PowerProviderNotice::SnapshotUpdated(snapshot));
    }

    pub fn set_power_source(&self, source: PowerSource) {
        self.mutate_snapshot(|snapshot| {
            snapshot.power_source = PowerSourceStatus::new(source, SignalSource::Observed);
        });
    }

    pub fn set_battery(&self, battery: BatteryStatus) {
        self.mutate_snapshot(|snapshot| {
            snapshot.battery = battery;
        });
    }

    pub fn set_battery_status(&self, battery: BatteryStatus) {
        self.set_battery(battery);
    }

    pub fn set_charging(&self, charging: ChargingState) {
        self.mutate_snapshot(|snapshot| {
            snapshot.battery.charging = charging;
            snapshot.battery.source = SignalSource::Observed;
        });
    }

    pub fn set_low_power_mode(&self, mode: LowPowerMode) {
        self.mutate_snapshot(|snapshot| {
            snapshot.battery.low_power_mode = mode;
            snapshot.battery.source = SignalSource::Observed;
        });
    }

    pub fn set_sleep_resume_state(&self, state: SleepResumeState) {
        self.mutate_snapshot(|snapshot| {
            let previous = snapshot.sleep_resume.state;
            let mut epoch = snapshot.sleep_resume.resume_epoch;
            if previous.is_sleeping()
                && matches!(state, SleepResumeState::Resumed | SleepResumeState::Awake)
            {
                epoch = epoch.saturating_add(1);
            }
            snapshot.sleep_resume = SleepResumeStatus::new(state, epoch, SignalSource::Observed);
        });
    }

    /// Convenience for the wake side of a sleep cycle.
    pub fn resume(&self) {
        self.set_sleep_resume_state(SleepResumeState::Resumed);
    }

    pub fn set_session_lock(&self, state: SessionLockState) {
        self.mutate_snapshot(|snapshot| {
            snapshot.session_lock = SessionLockStatus::new(state, SignalSource::Observed);
        });
    }

    pub fn set_thermal(&self, condition: ThermalCondition) {
        self.mutate_snapshot(|snapshot| {
            snapshot.thermal = ThermalStatus::new(condition, SignalSource::Observed);
        });
    }

    /// Alias for [`Self::set_thermal`].
    pub fn set_thermal_condition(&self, condition: ThermalCondition) {
        self.set_thermal(condition);
    }

    pub fn set_memory(&self, condition: MemoryCondition) {
        self.mutate_snapshot(|snapshot| {
            snapshot.memory = MemoryStatus::new(condition, SignalSource::Observed);
        });
    }

    /// Alias for [`Self::set_memory`].
    pub fn set_memory_condition(&self, condition: MemoryCondition) {
        self.set_memory(condition);
    }

    /// Alias for [`Self::set_charging`].
    pub fn set_charging_state(&self, charging: ChargingState) {
        self.set_charging(charging);
    }

    pub fn set_inhibition_mode(
        &self,
        kind: WakeInhibitionKind,
        reason: IneffectiveInhibitionReason,
    ) {
        lock(&self.state).inhibition_modes.insert(kind, reason);
    }

    pub fn clear_inhibition_mode(&self, kind: WakeInhibitionKind) {
        lock(&self.state).inhibition_modes.remove(&kind);
    }

    /// Alias emphasizing that the configured grant is ineffective.
    pub fn set_inhibition_ineffective(
        &self,
        kind: WakeInhibitionKind,
        reason: IneffectiveInhibitionReason,
    ) {
        self.set_inhibition_mode(kind, reason);
    }

    /// Simulate a hidden browser document or unfocused surface.
    ///
    /// Becoming hidden revokes active native tokens with
    /// [`WakeRevocationReason::AppBackgrounded`], matching the browser screen
    /// wake-lock release behavior.
    pub fn set_hidden_document(&self, hidden: bool) {
        let tokens = {
            let mut state = lock(&self.state);
            state.hidden_document = hidden;
            if hidden {
                state.active_tokens.keys().copied().collect::<Vec<_>>()
            } else {
                Vec::new()
            }
        };
        for token in tokens {
            self.revoke_token(token, WakeRevocationReason::AppBackgrounded);
        }
    }

    /// Alias used by Web wake-lock tests.
    pub fn set_document_hidden(&self, hidden: bool) {
        self.set_hidden_document(hidden);
    }

    /// Alias for [`Self::set_hidden_document`] using surface terminology.
    pub fn set_hidden_surface(&self, hidden: bool) {
        self.set_hidden_document(hidden);
    }

    /// Simulate an OS battery-saver override refusing a display inhibition.
    ///
    /// Enabling the override revokes active native tokens with
    /// [`WakeRevocationReason::LowPowerSaver`].
    pub fn set_battery_saver_override(&self, enabled: bool) {
        let tokens = {
            let mut state = lock(&self.state);
            state.battery_saver_override = enabled;
            if enabled {
                state.active_tokens.keys().copied().collect::<Vec<_>>()
            } else {
                Vec::new()
            }
        };
        for token in tokens {
            self.revoke_token(token, WakeRevocationReason::LowPowerSaver);
        }
    }

    /// Alias for [`Self::set_battery_saver_override`].
    pub fn set_os_battery_saver(&self, enabled: bool) {
        self.set_battery_saver_override(enabled);
    }

    pub fn set_permission_status(&self, status: PermissionStatus) {
        let changed = {
            let mut state = lock(&self.state);
            let changed = state.permission != status;
            state.permission = status;
            for operation in PowerOperation::ALL {
                if operation.requires_wake_lock()
                    && let Some(capability) = state.capabilities.get_mut(&operation)
                {
                    capability.permission = status.permission_state();
                }
            }
            changed
        };
        if changed {
            self.notify(PowerProviderNotice::PermissionChanged {
                kind: WAKE_LOCK_PERMISSION,
                status,
            });
        }
    }

    pub fn set_permission(&self, status: PermissionStatus) {
        self.set_permission_status(status);
    }

    pub fn set_capability(
        &self,
        operation: PowerOperation,
        capability: Capability<PowerOperation>,
    ) {
        lock(&self.state).capabilities.insert(operation, capability);
    }

    pub fn set_support(&self, operation: PowerOperation, support: Support) {
        let mut state = lock(&self.state);
        if let Some(capability) = state.capabilities.get_mut(&operation) {
            capability.support = support;
        }
    }

    pub fn set_availability(&self, operation: PowerOperation, availability: Availability) {
        let mut state = lock(&self.state);
        if let Some(capability) = state.capabilities.get_mut(&operation) {
            capability.availability = availability;
        }
    }

    pub fn set_configuration(&self, operation: PowerOperation, configuration: Configuration) {
        let mut state = lock(&self.state);
        if let Some(capability) = state.capabilities.get_mut(&operation) {
            capability.configuration = configuration;
        }
    }

    /// Revoke a native token as the OS would, without a service request.
    pub fn revoke_token(&self, native_token: u64, reason: WakeRevocationReason) -> bool {
        let revoked = {
            let mut state = lock(&self.state);
            if state.active_tokens.remove(&native_token).is_some() {
                state.revoked_tokens.push((native_token, reason));
                true
            } else {
                false
            }
        };
        if revoked {
            self.notify(PowerProviderNotice::TokenRevoked {
                native_token,
                reason,
            });
        }
        revoked
    }

    /// Revoke every active native token as the OS would.
    pub fn revoke_all_tokens(&self, reason: WakeRevocationReason) -> usize {
        let revoked = {
            let mut state = lock(&self.state);
            let tokens = std::mem::take(&mut state.active_tokens);
            for token in tokens.keys() {
                state.revoked_tokens.push((*token, reason));
            }
            tokens.len()
        };
        if revoked > 0 {
            self.notify(PowerProviderNotice::AllTokensRevoked { reason });
        }
        revoked
    }

    pub fn active_native_tokens(&self) -> Vec<u64> {
        lock(&self.state).active_tokens.keys().copied().collect()
    }

    /// Alias for [`Self::active_native_tokens`].
    pub fn active_tokens(&self) -> Vec<u64> {
        self.active_native_tokens()
    }

    pub fn released_native_tokens(&self) -> Vec<u64> {
        lock(&self.state).released_tokens.clone()
    }

    /// Alias for [`Self::released_native_tokens`].
    pub fn released_tokens(&self) -> Vec<u64> {
        self.released_native_tokens()
    }

    pub fn revoked_native_tokens(&self) -> Vec<(u64, WakeRevocationReason)> {
        lock(&self.state).revoked_tokens.clone()
    }

    pub fn release_call_count(&self) -> u64 {
        lock(&self.state).release_calls
    }

    /// Alias for [`Self::release_call_count`].
    pub fn release_count(&self) -> u64 {
        self.release_call_count()
    }

    pub fn acquire_call_count(&self) -> u64 {
        lock(&self.state).acquire_calls
    }

    pub fn permission_status(&self) -> PermissionStatus {
        lock(&self.state).permission
    }

    pub fn with_power_source(self, source: PowerSource) -> Self {
        self.set_power_source(source);
        self
    }

    pub fn with_battery(self, battery: BatteryStatus) -> Self {
        self.set_battery(battery);
        self
    }

    pub fn with_low_power_mode(self, mode: LowPowerMode) -> Self {
        self.set_low_power_mode(mode);
        self
    }

    pub fn with_sleep_resume_state(self, state: SleepResumeState) -> Self {
        self.set_sleep_resume_state(state);
        self
    }

    pub fn with_session_lock(self, state: SessionLockState) -> Self {
        self.set_session_lock(state);
        self
    }

    pub fn with_thermal(self, condition: ThermalCondition) -> Self {
        self.set_thermal(condition);
        self
    }

    pub fn with_memory(self, condition: MemoryCondition) -> Self {
        self.set_memory(condition);
        self
    }

    pub fn with_permission_status(self, status: PermissionStatus) -> Self {
        self.set_permission_status(status);
        self
    }

    pub fn with_capability(
        self,
        operation: PowerOperation,
        capability: Capability<PowerOperation>,
    ) -> Self {
        self.set_capability(operation, capability);
        self
    }
}

impl PowerProvider for FakePowerProvider {
    fn capability(&self, operation: PowerOperation) -> Capability<PowerOperation> {
        let state = lock(&self.state);
        state
            .capabilities
            .get(&operation)
            .cloned()
            .unwrap_or_else(|| capability_for(operation, state.permission, state.snapshot.revision))
    }

    fn snapshot(&self) -> Result<PowerSnapshot, SystemError> {
        Ok(FakePowerProvider::snapshot(self))
    }

    fn permission_status(&self, _kind: PermissionKind) -> PermissionStatus {
        lock(&self.state).permission
    }

    fn acquire_inhibition(
        &self,
        request: &PowerInhibitionRequest,
    ) -> Result<ProviderInhibitionGrant, SystemError> {
        let mut state = lock(&self.state);
        state.acquire_calls = state.acquire_calls.saturating_add(1);
        if let Some(reason) = state.inhibition_modes.get(&request.kind).copied() {
            return Ok(ProviderInhibitionGrant::Ineffective { reason });
        }
        if state.hidden_document && matches!(request.kind, WakeInhibitionKind::DisplayAndSystem) {
            return Ok(ProviderInhibitionGrant::Ineffective {
                reason: IneffectiveInhibitionReason::HiddenSurfaceOrDocument,
            });
        }
        if state.battery_saver_override {
            return Ok(ProviderInhibitionGrant::Ineffective {
                reason: IneffectiveInhibitionReason::BatterySaverOverride,
            });
        }
        if !state.permission.allows_use() {
            return Ok(ProviderInhibitionGrant::Ineffective {
                reason: IneffectiveInhibitionReason::PermissionDenied,
            });
        }
        if state.snapshot.sleep_resume.state.is_sleeping() {
            return Ok(ProviderInhibitionGrant::Ineffective {
                reason: IneffectiveInhibitionReason::SystemSleeping,
            });
        }
        if matches!(request.kind, WakeInhibitionKind::DisplayAndSystem)
            && state.snapshot.session_lock.state.is_locked()
        {
            return Ok(ProviderInhibitionGrant::Ineffective {
                reason: IneffectiveInhibitionReason::SessionLocked,
            });
        }
        let native_token = state.next_token;
        state.next_token = state.next_token.saturating_add(1);
        state.active_tokens.insert(native_token, request.kind);
        Ok(ProviderInhibitionGrant::Effective { native_token })
    }

    fn release_inhibition(&self, native_token: u64) -> bool {
        let mut state = lock(&self.state);
        if state.active_tokens.remove(&native_token).is_some() {
            state.released_tokens.push(native_token);
            state.release_calls = state.release_calls.saturating_add(1);
            true
        } else {
            false
        }
    }

    fn attach_listener(&self, listener: Weak<dyn PowerProviderListener>) {
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

impl fmt::Debug for FakePowerProvider {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let state = lock(&self.state);
        formatter
            .debug_struct("FakePowerProvider")
            .field("revision", &state.snapshot.revision)
            .field("active_tokens", &state.active_tokens.len())
            .field("permission", &state.permission)
            .finish()
    }
}
