//! Capability-gated power service, lifecycle-bound leases, bounded events and
//! deterministic pacing.

use crate::{
    EffectiveWakeStatus, MemoryCondition, MemoryStatus, PowerBudgetPolicy, PowerInhibitionRequest,
    PowerOperation, PowerProvider, PowerProviderListener, PowerProviderNotice, PowerSnapshot,
    ProviderInhibitionGrant, SessionLockState, SessionLockStatus, SignalSource, SleepResumeState,
    ThermalCondition, WakeInhibitionKind, WakeLeaseId, WakeLeaseRequest, WakeLeaseScope,
    WakeLeaseState, WakeReason, WakeRevocationReason, WorkPacingRecommendation,
};
use gpui_lifecycle::{AppPhase, LifecycleState, SurfaceSnapshot};
use gpui_permissions::PermissionStatus;
use gpui_system::{
    Capability, Completion, ErrorCode, EventSender, EventSubscription, OperationContext,
    ServiceResult, SurfaceId, SystemError, SystemServices, event_channel,
};
use std::{
    collections::{BTreeMap, HashMap, VecDeque},
    fmt,
    sync::{
        Arc, Mutex, Weak,
        atomic::{AtomicBool, Ordering},
    },
    time::Instant,
};

/// Default bounded capacity of one power observation stream.
pub const DEFAULT_POWER_EVENT_CAPACITY: usize = 64;

/// Upper bound on token revocations observed before their lease is registered.
const MAX_EARLY_REVOKED_TOKENS: usize = 64;

type Subscriber = (Arc<AtomicBool>, EventSender<PowerEvent>);

/// Public-safe service observations. Producers never block on consumers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PowerEvent {
    SnapshotUpdated {
        snapshot: PowerSnapshot,
    },
    SleepResumeChanged {
        state: SleepResumeState,
        resume_epoch: u64,
    },
    SessionLockChanged {
        state: SessionLockState,
    },
    ThermalChanged {
        condition: ThermalCondition,
    },
    MemoryChanged {
        condition: MemoryCondition,
    },
    WakeLeaseAcquired {
        id: WakeLeaseId,
        kind: WakeInhibitionKind,
    },
    WakeLeaseReleased {
        id: WakeLeaseId,
        kind: WakeInhibitionKind,
    },
    WakeLeaseRevoked {
        id: WakeLeaseId,
        kind: WakeInhibitionKind,
        reason: WakeRevocationReason,
    },
    EffectiveWakeStatusChanged {
        status: EffectiveWakeStatus,
    },
    PolicyUpdated {
        recommendation: WorkPacingRecommendation,
    },
    OperationFailed {
        operation: PowerOperation,
        code: ErrorCode,
    },
}

struct SurfaceRecord {
    generation: u64,
    visible: bool,
    disposed: bool,
}

struct ServiceState {
    snapshot: PowerSnapshot,
    phase: AppPhase,
    lifecycle_session_lock: Option<SessionLockState>,
    surfaces: HashMap<SurfaceId, SurfaceRecord>,
    saw_lifecycle_surfaces: bool,
    leases: BTreeMap<WakeLeaseId, Arc<LeaseInner>>,
    next_lease_id: u64,
    budget_policy: PowerBudgetPolicy,
    subscribers: Vec<Subscriber>,
    /// Incremented by an all-lease revocation or a denied wake-lock permission.
    all_revoked_epoch: u64,
    /// Reason for the most recent `all_revoked_epoch` increment.
    all_revoked_reason: Option<WakeRevocationReason>,
    /// Revocations that arrived before their token's lease was registered.
    early_revoked_tokens: VecDeque<(u64, WakeRevocationReason)>,
}

pub(crate) struct LeaseInner {
    id: WakeLeaseId,
    kind: WakeInhibitionKind,
    reason: WakeReason,
    scope: WakeLeaseScope,
    native_token: u64,
    state: Mutex<WakeLeaseState>,
}

impl LeaseInner {
    fn state(&self) -> WakeLeaseState {
        *lock(&self.state)
    }

    fn set_state(&self, state: WakeLeaseState) {
        *lock(&self.state) = state;
    }
}

pub(crate) struct ServiceInner {
    provider: Arc<dyn PowerProvider>,
    state: Mutex<ServiceState>,
    event_capacity: usize,
}

fn lock<T>(value: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    value
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn effective_status_locked(state: &ServiceState) -> EffectiveWakeStatus {
    let mut system_lease_count = 0;
    let mut display_lease_count = 0;
    for lease in state.leases.values() {
        match lease.kind {
            WakeInhibitionKind::SystemIdleSleep => system_lease_count += 1,
            WakeInhibitionKind::DisplayAndSystem => display_lease_count += 1,
        }
    }
    let active_lease_count = system_lease_count + display_lease_count;
    EffectiveWakeStatus {
        inhibits_system_sleep: active_lease_count > 0,
        inhibits_display_sleep: display_lease_count > 0,
        active_lease_count,
        system_lease_count,
        display_lease_count,
    }
}

fn capability_error(
    code: ErrorCode,
    message: &'static str,
    recovery: Option<gpui_system::RecoveryAction>,
) -> SystemError {
    let mut error = SystemError::new(code, message);
    error.recovery = recovery;
    error
}

fn revocation_error(reason: WakeRevocationReason) -> SystemError {
    match reason {
        WakeRevocationReason::PermissionRevoked => capability_error(
            ErrorCode::PermissionDenied,
            "Wake lock permission was revoked during acquisition",
            Some(gpui_system::RecoveryAction::OpenSettings),
        ),
        WakeRevocationReason::SurfaceDisposed | WakeRevocationReason::GenerationExpired => {
            capability_error(
                ErrorCode::ContextExpired,
                "Wake lease surface expired during acquisition",
                None,
            )
        }
        _ => capability_error(
            ErrorCode::Unavailable,
            "Wake inhibition was revoked during acquisition",
            None,
        ),
    }
}

fn record_early_revocation_locked(
    state: &mut ServiceState,
    native_token: u64,
    reason: WakeRevocationReason,
) {
    if state.early_revoked_tokens.len() >= MAX_EARLY_REVOKED_TOKENS {
        state.early_revoked_tokens.pop_front();
    }
    state.early_revoked_tokens.push_back((native_token, reason));
}

fn take_early_revocation_locked(
    state: &mut ServiceState,
    native_token: u64,
) -> Option<WakeRevocationReason> {
    let position = state
        .early_revoked_tokens
        .iter()
        .position(|(token, _)| *token == native_token)?;
    let reason = state
        .early_revoked_tokens
        .remove(position)
        .map(|(_, reason)| reason)?;
    state
        .early_revoked_tokens
        .retain(|(token, _)| *token != native_token);
    Some(reason)
}

impl ServiceInner {
    fn emit(&self, event: PowerEvent) {
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

    fn emit_failure(&self, operation: PowerOperation, error: &SystemError) {
        self.emit(PowerEvent::OperationFailed {
            operation,
            code: error.code,
        });
    }

    fn fail<T>(&self, operation: PowerOperation, error: SystemError) -> ServiceResult<T> {
        self.emit_failure(operation, &error);
        Err(error)
    }

    fn fail_grant<T>(
        &self,
        operation: PowerOperation,
        native_token: u64,
        error: SystemError,
    ) -> ServiceResult<T> {
        self.provider.release_inhibition(native_token);
        self.fail(operation, error)
    }

    fn check_context<T>(
        &self,
        operation: PowerOperation,
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

    fn gate_capability(&self, operation: PowerOperation) -> Result<(), SystemError> {
        self.provider.capability(operation).check()
    }

    fn gate_permission(&self) -> Result<(), SystemError> {
        let status = self.provider.permission_status(crate::WAKE_LOCK_PERMISSION);
        if status.allows_use() {
            return Ok(());
        }
        Err(status.error().unwrap_or_else(|| {
            capability_error(
                ErrorCode::PermissionDenied,
                "Wake lock permission is not authorized",
                Some(gpui_system::RecoveryAction::OpenSettings),
            )
        }))
    }

    fn effective_status(&self) -> EffectiveWakeStatus {
        let state = lock(&self.state);
        effective_status_locked(&state)
    }

    pub(crate) fn release_lease(&self, id: WakeLeaseId, next: WakeLeaseState) -> bool {
        let (lease, previous, current) = {
            let mut state = lock(&self.state);
            let previous = effective_status_locked(&state);
            let Some(lease) = state.leases.remove(&id) else {
                return false;
            };
            let current = effective_status_locked(&state);
            (lease, previous, current)
        };
        lease.set_state(next);
        self.provider.release_inhibition(lease.native_token);
        match next {
            WakeLeaseState::Released => self.emit(PowerEvent::WakeLeaseReleased {
                id: lease.id,
                kind: lease.kind,
            }),
            WakeLeaseState::Revoked(reason) => self.emit(PowerEvent::WakeLeaseRevoked {
                id: lease.id,
                kind: lease.kind,
                reason,
            }),
            WakeLeaseState::Effective => {}
        }
        if previous != current {
            self.emit(PowerEvent::EffectiveWakeStatusChanged { status: current });
        }
        true
    }

    fn revoke_where(&self, matches: impl Fn(&LeaseInner) -> bool, reason: WakeRevocationReason) {
        let (revoked, previous, current) = {
            let mut state = lock(&self.state);
            let previous = effective_status_locked(&state);
            let ids = state
                .leases
                .iter()
                .filter(|(_, lease)| matches(lease))
                .map(|(id, _)| *id)
                .collect::<Vec<_>>();
            let revoked = ids
                .into_iter()
                .filter_map(|id| state.leases.remove(&id))
                .collect::<Vec<_>>();
            let current = effective_status_locked(&state);
            (revoked, previous, current)
        };
        if revoked.is_empty() {
            return;
        }
        for lease in revoked {
            lease.set_state(WakeLeaseState::Revoked(reason));
            self.provider.release_inhibition(lease.native_token);
            self.emit(PowerEvent::WakeLeaseRevoked {
                id: lease.id,
                kind: lease.kind,
                reason,
            });
        }
        if previous != current {
            self.emit(PowerEvent::EffectiveWakeStatusChanged { status: current });
        }
    }

    fn revoke_token(&self, native_token: u64, reason: WakeRevocationReason) {
        let id = {
            let mut state = lock(&self.state);
            let id = state
                .leases
                .iter()
                .find(|(_, lease)| lease.native_token == native_token)
                .map(|(id, _)| *id);
            if id.is_none() {
                record_early_revocation_locked(&mut state, native_token, reason);
            }
            id
        };
        if let Some(id) = id {
            self.release_lease(id, WakeLeaseState::Revoked(reason));
        }
    }

    fn revoke_for_snapshot(&self, snapshot: &PowerSnapshot) {
        if snapshot.sleep_resume.state.is_sleeping() {
            self.revoke_where(|_| true, WakeRevocationReason::SystemSleeping);
        }
        if snapshot.session_lock.state.is_locked() {
            self.revoke_where(
                |lease| lease.scope.policy().revokes_on_session_lock_for(lease.kind),
                WakeRevocationReason::SessionLocked,
            );
        }
        if snapshot.battery.low_power_mode == crate::LowPowerMode::Enabled {
            self.revoke_where(
                |lease| lease.scope.policy().revoke_on_low_power_mode,
                WakeRevocationReason::LowPowerSaver,
            );
        }
    }

    fn apply_snapshot(&self, mut snapshot: PowerSnapshot) {
        let previous = {
            let mut state = lock(&self.state);
            let previous = state.snapshot.clone();
            if snapshot == previous {
                return;
            }
            if snapshot.revision <= previous.revision {
                snapshot.revision = previous.revision.saturating_add(1);
            }
            state.snapshot = snapshot.clone();
            previous
        };
        self.revoke_for_snapshot(&snapshot);
        self.emit(PowerEvent::SnapshotUpdated {
            snapshot: snapshot.clone(),
        });
        if previous.sleep_resume != snapshot.sleep_resume {
            self.emit(PowerEvent::SleepResumeChanged {
                state: snapshot.sleep_resume.state,
                resume_epoch: snapshot.sleep_resume.resume_epoch,
            });
        }
        if previous.session_lock != snapshot.session_lock {
            self.emit(PowerEvent::SessionLockChanged {
                state: snapshot.session_lock.state,
            });
        }
        if previous.thermal != snapshot.thermal {
            self.emit(PowerEvent::ThermalChanged {
                condition: snapshot.thermal.condition,
            });
        }
        if previous.memory != snapshot.memory {
            self.emit(PowerEvent::MemoryChanged {
                condition: snapshot.memory.condition,
            });
        }
    }

    fn sync_phase(&self, phase: AppPhase) {
        let changed = {
            let mut state = lock(&self.state);
            let changed = state.phase != phase;
            state.phase = phase;
            changed
        };
        if changed
            && matches!(
                phase,
                AppPhase::Background | AppPhase::Suspended | AppPhase::Terminating
            )
        {
            self.revoke_where(
                |lease| lease.scope.policy().revokes_on_background_for(lease.kind),
                WakeRevocationReason::AppBackgrounded,
            );
        }
    }

    fn observe_surface(&self, surface: SurfaceId, generation: u64, visible: bool) {
        let generation_changed = {
            let mut state = lock(&self.state);
            let record = state
                .surfaces
                .entry(surface)
                .or_insert_with(|| SurfaceRecord {
                    generation,
                    visible,
                    disposed: false,
                });
            let generation_changed = record.generation != generation;
            record.generation = generation;
            record.visible = visible;
            record.disposed = false;
            generation_changed
        };
        if generation_changed {
            self.revoke_where(
                |lease| {
                    lease.scope.surface_id() == Some(surface)
                        && lease.scope.generation() != Some(generation)
                },
                WakeRevocationReason::GenerationExpired,
            );
        }
        if !visible {
            self.revoke_where(
                |lease| {
                    lease.scope.surface_id() == Some(surface)
                        && lease.scope.policy().revokes_on_background_for(lease.kind)
                },
                WakeRevocationReason::AppBackgrounded,
            );
        }
    }

    fn sync_lifecycle_surfaces(&self, surfaces: &[(SurfaceId, SurfaceSnapshot)]) {
        let mut tracked = {
            let state = lock(&self.state);
            state.surfaces.keys().copied().collect::<Vec<_>>()
        };
        tracked.sort_by_key(|surface| surface.0);
        for surface in tracked {
            if !surfaces.iter().any(|(present, _)| *present == surface) {
                self.dispose_surface_inner(surface);
            }
        }
        for (surface, snapshot) in surfaces {
            self.observe_surface(
                *surface,
                snapshot.generation,
                snapshot.visibility.is_visible(),
            );
        }
    }

    fn dispose_surface_inner(&self, surface: SurfaceId) {
        {
            let mut state = lock(&self.state);
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
        }
        self.revoke_where(
            |lease| lease.scope.surface_id() == Some(surface),
            WakeRevocationReason::SurfaceDisposed,
        );
    }

    fn preconditions(&self, request: &WakeLeaseRequest) -> Result<(), SystemError> {
        let state = lock(&self.state);
        Self::preconditions_locked(&state, request)
    }

    fn preconditions_locked(
        state: &ServiceState,
        request: &WakeLeaseRequest,
    ) -> Result<(), SystemError> {
        let policy = request.policy();
        let snapshot = &state.snapshot;
        if snapshot.sleep_resume.state.is_sleeping() {
            return Err(capability_error(
                ErrorCode::Unavailable,
                "Cannot inhibit sleep while the system is preparing for or in sleep",
                None,
            ));
        }
        let locked = snapshot.session_lock.state.is_locked()
            || state
                .lifecycle_session_lock
                .is_some_and(SessionLockState::is_locked);
        if policy.revokes_on_session_lock_for(request.kind) && locked {
            return Err(capability_error(
                ErrorCode::Unavailable,
                "Cannot inhibit sleep while the session is locking or locked",
                None,
            ));
        }
        if policy.revokes_on_background_for(request.kind) {
            let backgrounded = matches!(
                state.phase,
                AppPhase::Background | AppPhase::Suspended | AppPhase::Terminating
            );
            let hidden = request.scope.surface_id().is_some_and(|surface| {
                state
                    .surfaces
                    .get(&surface)
                    .is_some_and(|record| !record.visible || record.disposed)
            });
            if backgrounded || hidden {
                return Err(capability_error(
                    ErrorCode::Unavailable,
                    "Cannot inhibit wake while the app or surface is backgrounded",
                    None,
                ));
            }
        }
        if policy.revoke_on_low_power_mode
            && snapshot.battery.low_power_mode == crate::LowPowerMode::Enabled
        {
            return Err(capability_error(
                ErrorCode::Unavailable,
                "Cannot inhibit wake while low-power mode is enabled",
                None,
            ));
        }
        Ok(())
    }

    fn validate_scope(
        &self,
        request: &WakeLeaseRequest,
        context: &OperationContext,
    ) -> Result<(), SystemError> {
        let state = lock(&self.state);
        Self::validate_scope_locked(&state, request, context)
    }

    fn validate_scope_locked(
        state: &ServiceState,
        request: &WakeLeaseRequest,
        context: &OperationContext,
    ) -> Result<(), SystemError> {
        let Some((surface, generation)) =
            request.scope.surface_id().zip(request.scope.generation())
        else {
            return Ok(());
        };
        if let Some(ui) = &context.ui {
            if !ui.is_current() {
                return Err(capability_error(
                    ErrorCode::ContextExpired,
                    "Wake lease UI context is stale",
                    None,
                ));
            }
            if ui.surface != surface {
                return Err(capability_error(
                    ErrorCode::InvalidInput,
                    "Wake lease surface does not match the UI context",
                    None,
                ));
            }
            if ui.generation != generation {
                return Err(capability_error(
                    ErrorCode::ContextExpired,
                    "Wake lease generation is stale",
                    None,
                ));
            }
        }
        match state.surfaces.get(&surface) {
            None => Err(capability_error(
                ErrorCode::ContextExpired,
                "Wake lease surface is not attached",
                None,
            )),
            Some(record) if record.disposed => Err(capability_error(
                ErrorCode::ContextExpired,
                "Wake lease surface is disposed",
                None,
            )),
            Some(record) if record.generation != generation => Err(capability_error(
                ErrorCode::ContextExpired,
                "Wake lease generation is stale",
                None,
            )),
            Some(_) => Ok(()),
        }
    }

    fn acquire(
        self: &Arc<Self>,
        request: WakeLeaseRequest,
        context: &OperationContext,
    ) -> ServiceResult<WakePreventionLease> {
        let operation = request.kind.operation();
        if let Some(cancelled) = self.check_context(operation, context)? {
            return Ok(cancelled);
        }
        if let Err(error) = self.validate_scope(&request, context) {
            return self.fail(operation, error);
        }
        if let Err(error) = self.gate_capability(operation) {
            return self.fail(operation, error);
        }
        if let Err(error) = self.gate_permission() {
            return self.fail(operation, error);
        }
        if let Err(error) = self.preconditions(&request) {
            return self.fail(operation, error);
        }
        let provider_request = PowerInhibitionRequest {
            kind: request.kind,
            reason: request.reason.clone(),
            surface: request.scope.surface_id(),
            generation: request.scope.generation(),
        };
        let pre_epoch = lock(&self.state).all_revoked_epoch;
        let grant = match self.provider.acquire_inhibition(&provider_request) {
            Ok(grant) => grant,
            Err(error) => return self.fail(operation, error),
        };
        let native_token = match grant {
            ProviderInhibitionGrant::Effective { native_token } => native_token,
            ProviderInhibitionGrant::Ineffective { reason } => {
                return self.fail(operation, reason.error());
            }
        };

        // Re-probe authorization after the provider granted the token. This
        // catches a denial that landed between the pre-acquire gate and the
        // epoch snapshot; a concurrent notice is caught by the epoch below.
        let permission_error = self.gate_permission().err();

        let mut state = lock(&self.state);

        // Cancellation may arrive while the provider is acquiring the native
        // token. It takes precedence over state conflicts and must release the
        // token rather than returning a cancelled-but-live grant.
        if let Some(reason) = context.cancellation.reason() {
            let _ = take_early_revocation_locked(&mut state, native_token);
            drop(state);
            self.provider.release_inhibition(native_token);
            return Ok(Completion::Cancelled(reason));
        }

        // A provider-wide revocation or denied wake-lock permission can race
        // with acquisition. Any increment means this grant is already stale.
        if state.all_revoked_epoch != pre_epoch {
            let reason = state.all_revoked_reason;
            let _ = take_early_revocation_locked(&mut state, native_token);
            drop(state);
            let error = reason.map_or_else(
                || {
                    capability_error(
                        ErrorCode::Unavailable,
                        "Wake inhibition was revoked during acquisition",
                        None,
                    )
                },
                revocation_error,
            );
            return self.fail_grant(operation, native_token, error);
        }

        // A targeted revocation can arrive before this token has a lease to
        // find. Consume the tombstone and fail the acquisition instead of
        // registering an effective lease for an already-revoked token.
        if let Some(reason) = take_early_revocation_locked(&mut state, native_token) {
            drop(state);
            return self.fail_grant(operation, native_token, revocation_error(reason));
        }

        if let Some(error) = permission_error {
            drop(state);
            return self.fail_grant(operation, native_token, error);
        }

        // Surface disposal, generation advance, sleep, lock, background and
        // low-power changes can all happen while the provider is acquiring.
        if let Err(error) = Self::validate_scope_locked(&state, &request, context) {
            drop(state);
            return self.fail_grant(operation, native_token, error);
        }
        if let Err(error) = Self::preconditions_locked(&state, &request) {
            drop(state);
            return self.fail_grant(operation, native_token, error);
        }

        let id = WakeLeaseId(state.next_lease_id.max(1));
        let Some(next) = state.next_lease_id.checked_add(1) else {
            drop(state);
            return self.fail_grant(
                operation,
                native_token,
                SystemError::new(ErrorCode::ResourceExpired, "Wake lease identity exhausted"),
            );
        };
        state.next_lease_id = next;
        let previous = effective_status_locked(&state);
        let lease = Arc::new(LeaseInner {
            id,
            kind: request.kind,
            reason: request.reason,
            scope: request.scope,
            native_token,
            state: Mutex::new(WakeLeaseState::Effective),
        });
        state.leases.insert(id, lease.clone());
        let current = effective_status_locked(&state);
        drop(state);

        self.emit(PowerEvent::WakeLeaseAcquired {
            id,
            kind: provider_request.kind,
        });
        if previous != current {
            self.emit(PowerEvent::EffectiveWakeStatusChanged { status: current });
        }
        Ok(Completion::Completed(WakePreventionLease {
            inner: lease,
            service: self.clone(),
        }))
    }
}

impl PowerProviderListener for ServiceInner {
    fn on_power_provider_notice(&self, notice: PowerProviderNotice) {
        match notice {
            PowerProviderNotice::SnapshotUpdated(snapshot) => {
                self.apply_snapshot(snapshot);
            }
            PowerProviderNotice::TokenRevoked {
                native_token,
                reason,
            } => self.revoke_token(native_token, reason),
            PowerProviderNotice::AllTokensRevoked { reason } => {
                {
                    let mut state = lock(&self.state);
                    state.all_revoked_epoch = state.all_revoked_epoch.wrapping_add(1);
                    state.all_revoked_reason = Some(reason);
                }
                self.revoke_where(|_| true, reason);
            }
            PowerProviderNotice::PermissionChanged { kind, status } => {
                if kind == crate::WAKE_LOCK_PERMISSION && !status.allows_use() {
                    {
                        let mut state = lock(&self.state);
                        state.all_revoked_epoch = state.all_revoked_epoch.wrapping_add(1);
                        state.all_revoked_reason = Some(WakeRevocationReason::PermissionRevoked);
                    }
                    self.revoke_where(|_| true, WakeRevocationReason::PermissionRevoked);
                }
            }
        }
    }
}

/// An effective, scoped wake-prevention lease.
///
/// Dropping or releasing the lease is idempotent: the provider token is released
/// at most once, including when the OS or lifecycle already revoked it.
pub struct WakePreventionLease {
    inner: Arc<LeaseInner>,
    service: Arc<ServiceInner>,
}

impl WakePreventionLease {
    pub fn id(&self) -> WakeLeaseId {
        self.inner.id
    }

    pub fn kind(&self) -> WakeInhibitionKind {
        self.inner.kind
    }

    pub fn reason(&self) -> &WakeReason {
        &self.inner.reason
    }

    pub fn scope(&self) -> &WakeLeaseScope {
        &self.inner.scope
    }

    pub fn state(&self) -> WakeLeaseState {
        self.inner.state()
    }

    pub fn is_effective(&self) -> bool {
        self.inner.state().is_effective()
    }

    pub fn revocation_reason(&self) -> Option<WakeRevocationReason> {
        self.inner.state().revocation_reason()
    }

    /// Release this lease. Returns false when it was already released or
    /// revoked; no native token is released twice.
    pub fn release(&self) -> bool {
        self.service
            .release_lease(self.inner.id, WakeLeaseState::Released)
    }
}

impl Drop for WakePreventionLease {
    fn drop(&mut self) {
        let _ = self.release();
    }
}

impl fmt::Debug for WakePreventionLease {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("WakePreventionLease")
            .field("id", &self.inner.id)
            .field("kind", &self.inner.kind)
            .field("state", &self.inner.state())
            .finish()
    }
}

/// Cloneable, `Send + Sync` power service suitable for `SystemServices`.
#[derive(Clone)]
pub struct PowerService {
    inner: Arc<ServiceInner>,
}

impl PowerService {
    pub fn new(provider: Arc<dyn PowerProvider>) -> Self {
        Self::with_event_capacity(provider, DEFAULT_POWER_EVENT_CAPACITY)
    }

    /// Build a service with an explicit bounded observation capacity.
    pub fn with_event_capacity(provider: Arc<dyn PowerProvider>, capacity: usize) -> Self {
        let snapshot = provider
            .snapshot()
            .unwrap_or_else(|_| PowerSnapshot::unknown(SignalSource::ProviderUnavailable));
        let inner = Arc::new(ServiceInner {
            provider: provider.clone(),
            state: Mutex::new(ServiceState {
                snapshot,
                phase: AppPhase::Launching,
                lifecycle_session_lock: None,
                surfaces: HashMap::new(),
                saw_lifecycle_surfaces: false,
                leases: BTreeMap::new(),
                next_lease_id: 1,
                budget_policy: PowerBudgetPolicy::default(),
                subscribers: Vec::new(),
                all_revoked_epoch: 0,
                all_revoked_reason: None,
                early_revoked_tokens: VecDeque::new(),
            }),
            event_capacity: capacity.max(1),
        });
        let listener: Weak<dyn PowerProviderListener> = {
            let shared: Arc<dyn PowerProviderListener> = inner.clone();
            Arc::downgrade(&shared)
        };
        provider.attach_listener(listener);
        Self { inner }
    }

    pub fn from_provider(provider: impl PowerProvider + 'static) -> Self {
        Self::new(Arc::new(provider))
    }

    pub fn from_fake(fake: crate::FakePowerProvider) -> Self {
        Self::from_provider(fake)
    }

    /// Convenience constructor for the contract-only provider.
    pub fn unavailable() -> Self {
        Self::from_provider(crate::UnavailablePowerProvider::new())
    }

    pub fn install(self, services: &mut SystemServices) -> Result<(), SystemError> {
        services.register(self)
    }

    pub fn install_cloned(&self, services: &mut SystemServices) -> Result<(), SystemError> {
        services.register(self.clone())
    }

    pub fn shares_state_with(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.inner, &other.inner)
    }

    pub fn provider(&self) -> &Arc<dyn PowerProvider> {
        &self.inner.provider
    }

    pub fn fake(&self) -> Option<&crate::FakePowerProvider> {
        self.inner
            .provider
            .as_any()
            .downcast_ref::<crate::FakePowerProvider>()
    }

    /// Latest cached coherent observation. Never blocks on the provider.
    pub fn current_snapshot(&self) -> PowerSnapshot {
        lock(&self.inner.state).snapshot.clone()
    }

    pub fn revision(&self) -> u64 {
        self.current_snapshot().revision
    }

    /// Capability/context-gated refresh from the provider.
    pub fn snapshot(&self, context: &OperationContext) -> ServiceResult<PowerSnapshot> {
        let operation = PowerOperation::ObservePower;
        if let Some(cancelled) = self.inner.check_context(operation, context)? {
            return Ok(cancelled);
        }
        if let Err(error) = self.inner.gate_capability(operation) {
            return self.inner.fail(operation, error);
        }
        match self.inner.provider.snapshot() {
            Ok(snapshot) => {
                self.inner.apply_snapshot(snapshot);
                Ok(Completion::Completed(self.current_snapshot()))
            }
            Err(error) => self.inner.fail(operation, error),
        }
    }

    /// Refresh from the provider without an `OperationContext`. Intended for
    /// provider-owned push callbacks and deterministic host drivers.
    pub fn sync_from_provider(&self) -> Result<PowerSnapshot, SystemError> {
        match self.inner.provider.snapshot() {
            Ok(snapshot) => {
                self.inner.apply_snapshot(snapshot);
                Ok(self.current_snapshot())
            }
            Err(error) => {
                self.inner
                    .emit_failure(PowerOperation::ObservePower, &error);
                Err(error)
            }
        }
    }

    pub fn capability(&self, operation: PowerOperation) -> Capability<PowerOperation> {
        self.inner.provider.capability(operation)
    }

    pub fn capabilities(&self) -> Vec<Capability<PowerOperation>> {
        self.inner.provider.capabilities()
    }

    pub fn permission_status(&self) -> PermissionStatus {
        self.inner
            .provider
            .permission_status(crate::WAKE_LOCK_PERMISSION)
    }

    pub fn acquire_wake_lease(
        &self,
        request: WakeLeaseRequest,
        context: &OperationContext,
    ) -> ServiceResult<WakePreventionLease> {
        self.inner.acquire(request, context)
    }

    pub fn effective_wake_status(&self) -> EffectiveWakeStatus {
        self.inner.effective_status()
    }

    pub fn active_lease_count(&self) -> usize {
        self.effective_wake_status().active_lease_count
    }

    pub fn budget_policy(&self) -> PowerBudgetPolicy {
        lock(&self.inner.state).budget_policy.clone()
    }

    pub fn set_budget_policy(&self, policy: PowerBudgetPolicy) {
        {
            let mut state = lock(&self.inner.state);
            state.budget_policy = policy;
        }
        let recommendation = self.recommend_pacing();
        self.inner
            .emit(PowerEvent::PolicyUpdated { recommendation });
    }

    pub fn recommend_pacing(&self) -> WorkPacingRecommendation {
        let state = lock(&self.inner.state);
        state.budget_policy.evaluate(&state.snapshot)
    }

    /// Apply a coherent lifecycle snapshot.
    ///
    /// Backgrounding, session lock, surface disposal, generation expiry,
    /// hidden surfaces and memory pressure are all correlated here. The
    /// provider's power/thermal observations remain authoritative for the
    /// domains it owns.
    pub fn notify_lifecycle(&self, lifecycle: &LifecycleState) {
        self.inner.sync_phase(lifecycle.phase());
        let mut surfaces = lifecycle.surfaces().collect::<Vec<_>>();
        surfaces.sort_by_key(|(surface, _)| surface.0);
        if !surfaces.is_empty() {
            lock(&self.inner.state).saw_lifecycle_surfaces = true;
        }
        let saw_surfaces = lock(&self.inner.state).saw_lifecycle_surfaces;
        if saw_surfaces {
            self.inner.sync_lifecycle_surfaces(&surfaces);
        } else {
            for (surface, snapshot) in &surfaces {
                self.inner.observe_surface(
                    *surface,
                    snapshot.generation,
                    snapshot.visibility.is_visible(),
                );
            }
        }
        if lifecycle.revision() == 0 {
            return;
        }
        let mut snapshot = self.current_snapshot();
        let mut changed = false;
        let lock_status =
            SessionLockStatus::new(lifecycle.session_lock().into(), SignalSource::Observed);
        if snapshot.session_lock != lock_status {
            snapshot.session_lock = lock_status;
            changed = true;
        }
        let memory = MemoryStatus::new(lifecycle.memory_pressure().into(), SignalSource::Observed);
        if snapshot.memory != memory {
            snapshot.memory = memory;
            changed = true;
        }
        {
            let mut state = lock(&self.inner.state);
            state.lifecycle_session_lock = Some(lifecycle.session_lock().into());
        }
        if changed {
            snapshot.revision = snapshot.revision.saturating_add(1);
            self.inner.apply_snapshot(snapshot);
        } else {
            self.inner.revoke_for_snapshot(&snapshot);
        }
    }

    pub fn notify_app_phase(&self, phase: AppPhase) {
        self.inner.sync_phase(phase);
    }

    pub fn attach_surface(&self, surface: SurfaceId, generation: u64, visible: bool) {
        self.inner.observe_surface(surface, generation, visible);
    }

    pub fn advance_surface_generation(&self, surface: SurfaceId, generation: u64) {
        let visible = {
            let state = lock(&self.inner.state);
            state
                .surfaces
                .get(&surface)
                .map(|record| record.visible && !record.disposed)
                .unwrap_or(true)
        };
        self.inner.observe_surface(surface, generation, visible);
    }

    pub fn dispose_surface(&self, surface: SurfaceId) {
        self.inner.dispose_surface_inner(surface);
    }

    pub fn surface_is_tracked(&self, surface: SurfaceId) -> bool {
        lock(&self.inner.state)
            .surfaces
            .get(&surface)
            .is_some_and(|record| !record.disposed)
    }

    /// Create an independent bounded observation stream.
    pub fn subscribe(&self) -> EventSubscription<PowerEvent> {
        let live = Arc::new(AtomicBool::new(true));
        let cleanup = Arc::clone(&live);
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

impl fmt::Debug for PowerService {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PowerService")
            .field("revision", &self.revision())
            .field("effective", &self.effective_wake_status())
            .finish()
    }
}
