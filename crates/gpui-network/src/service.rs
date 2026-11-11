//! Capability-gated network policy service: proxy resolution, trust policy,
//! scoped clients, lifecycle deferral, bounded events and retry decisions.

use crate::client::ClientInner;
use crate::{
    AccountScopeKey, ClientId, ClientOwnerScope, ClientRevocationReason, EndpointDiagnostic,
    EndpointOrigin, InterfaceReachability, MAX_PROXY_AUTH_ATTEMPTS, NetworkClientLease,
    NetworkOperation, NetworkProvider, NetworkProviderListener, NetworkProviderNotice,
    NetworkSnapshot, NetworkWorkloadClass, PlatformTarget, PolicyEpoch, ProxyAuthChallenge,
    ProxyAuthHeader, ProxyAuthOutcome, ProxyAuthOutcomeKind, ProxyCandidate, ProxyConfig,
    ProxyCredentialSource, RetryDecision, RetryPolicy, RetryTracker, RevalidationDirective,
    RevalidationScopeFilter, ScopedRevalidation, SignalSource, TrustEvaluation, TrustProfile,
    TrustStorePolicy, WorkloadBudgetDecision, evaluate_tls_trust, validate_proxy_config,
    validate_trust_policy,
};
use gpui_lifecycle::{AppPhase, LifecycleState};
use gpui_secrets::SecretAccess;
use gpui_system::{
    Capability, Completion, ErrorCode, EventSender, EventSubscription, OperationContext,
    ServiceResult, SurfaceId, SystemError, SystemServices, event_channel,
};
use std::{
    cmp::Ordering,
    collections::BTreeMap,
    fmt,
    sync::{
        Arc, Mutex, Weak,
        atomic::{AtomicBool, Ordering as AtomicOrdering},
    },
    time::Instant,
};

/// Default bounded capacity of one network event stream.
pub const DEFAULT_NETWORK_EVENT_CAPACITY: usize = 64;

/// Public-safe network service observations. Account keys are fingerprints and
/// no credential, header value, SSID, MAC address, path or query is emitted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NetworkEvent {
    SnapshotUpdated {
        snapshot: NetworkSnapshot,
    },
    ReachabilityChanged {
        reachability: InterfaceReachability,
    },
    CaptivePortalChanged {
        status: crate::CaptivePortalStatus,
    },
    ConnectionCostChanged {
        cost: crate::ConnectionCost,
        data_saver: crate::DataSaverMode,
    },
    ProxyConfigChanged {
        config: ProxyConfig,
    },
    TrustPolicyChanged {
        policy: TrustStorePolicy,
    },
    ProxyCredentialsRotated {
        scope: Option<ClientOwnerScope>,
    },
    PolicyEpochAdvanced {
        epoch: PolicyEpoch,
    },
    ClientRegistered {
        client_id: ClientId,
        scope: ClientOwnerScope,
    },
    ClientRevoked {
        client_id: ClientId,
        scope: ClientOwnerScope,
        reason: ClientRevocationReason,
    },
    RevalidationDirective {
        client_id: ClientId,
        scope: ClientOwnerScope,
        directive: RevalidationDirective,
        epoch: PolicyEpoch,
    },
    ProxyAuthHandled {
        client_id: ClientId,
        proxy: EndpointOrigin,
        outcome: ProxyAuthOutcomeKind,
        epoch: PolicyEpoch,
    },
    AppPhaseChanged {
        phase: AppPhase,
    },
    OperationFailed {
        operation: NetworkOperation,
        code: ErrorCode,
    },
}

/// Result of one policy transition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PolicyUpdate {
    pub previous_epoch: PolicyEpoch,
    /// The current epoch after this call. Equal to `previous_epoch` when the
    /// semantic state did not change.
    pub epoch: PolicyEpoch,
    pub changed: bool,
    /// Global directives emitted by this transition; per-client copies are
    /// queued on live leases and observer callbacks.
    pub directives: Vec<RevalidationDirective>,
}

impl PolicyUpdate {
    fn unchanged(epoch: PolicyEpoch) -> Self {
        Self {
            previous_epoch: epoch,
            epoch,
            changed: false,
            directives: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct ProxyAuthAttemptKey {
    client_id: ClientId,
    proxy: EndpointOrigin,
    epoch: PolicyEpoch,
}

struct ObserverRecord {
    filter: RevalidationScopeFilter,
    callback: Arc<dyn Fn(&ScopedRevalidation) + Send + Sync + 'static>,
}

struct ServiceState {
    snapshot: NetworkSnapshot,
    proxy_config: ProxyConfig,
    trust_policy: TrustStorePolicy,
    trust_profile: TrustProfile,
    platform: PlatformTarget,
    epoch: PolicyEpoch,
    app_phase: AppPhase,
    clients: BTreeMap<ClientId, Weak<ClientInner>>,
    next_client_id: u64,
    subscribers: Vec<(Arc<AtomicBool>, EventSender<NetworkEvent>)>,
    proxy_auth_attempts: BTreeMap<ProxyAuthAttemptKey, u8>,
    observers: BTreeMap<u64, ObserverRecord>,
    next_observer_id: u64,
    pending_stale_revalidation: bool,
    shutting_down: bool,
}

pub(crate) struct ServiceInner {
    provider: Arc<dyn NetworkProvider>,
    state: Mutex<ServiceState>,
    event_capacity: usize,
    credential_source: Mutex<Option<Arc<dyn ProxyCredentialSource>>>,
    retry: Mutex<RetryTracker>,
}

fn lock<T>(value: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    value
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn is_background_phase(phase: AppPhase) -> bool {
    matches!(phase, AppPhase::Background | AppPhase::Suspended)
}

fn advance_epoch_locked(state: &mut ServiceState) -> (PolicyEpoch, PolicyEpoch) {
    let previous = state.epoch;
    state.epoch = state.epoch.next();
    state.proxy_auth_attempts.clear();
    (previous, state.epoch)
}

impl ServiceInner {
    pub(crate) fn client_released(&self, client_id: ClientId, reason: ClientRevocationReason) {
        let removed = {
            let mut state = lock(&self.state);
            state.clients.remove(&client_id)
        };
        if let Some(inner) = removed.and_then(|client| client.upgrade()) {
            self.emit(NetworkEvent::ClientRevoked {
                client_id,
                scope: inner.scope_value(),
                reason,
            });
        }
    }

    fn emit(&self, event: NetworkEvent) {
        let senders = {
            let mut state = lock(&self.state);
            state
                .subscribers
                .retain(|(live, _)| live.load(AtomicOrdering::Acquire));
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

    fn emit_failure(&self, operation: NetworkOperation, error: &SystemError) {
        self.emit(NetworkEvent::OperationFailed {
            operation,
            code: error.code,
        });
    }

    fn fail<T>(&self, operation: NetworkOperation, error: SystemError) -> ServiceResult<T> {
        self.emit_failure(operation, &error);
        Err(error)
    }

    fn check_context<T>(
        &self,
        operation: NetworkOperation,
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

    fn gate_capability(&self, operation: NetworkOperation) -> Result<(), SystemError> {
        self.provider.capability(operation).check()
    }

    /// Rotate proxy credentials globally or for one exact owner scope.
    pub fn rotate_proxy_credentials(&self, scope: Option<&ClientOwnerScope>) -> PolicyUpdate {
        let (previous_epoch, epoch, phase) = {
            let mut state = lock(&self.state);
            state.proxy_auth_attempts.clear();
            let (previous_epoch, epoch) = advance_epoch_locked(&mut state);
            (previous_epoch, epoch, state.app_phase)
        };
        self.emit(NetworkEvent::ProxyCredentialsRotated {
            scope: scope.copied(),
        });
        self.emit(NetworkEvent::PolicyEpochAdvanced { epoch });
        let directives = vec![
            RevalidationDirective::ReauthenticateProxy,
            RevalidationDirective::RecycleIdleConnections,
        ];
        self.dispatch_directives(&directives, scope.copied(), phase, epoch);
        self.sync_retry_epoch(epoch);
        PolicyUpdate {
            previous_epoch,
            epoch,
            changed: true,
            directives,
        }
    }

    /// Re-evaluate the host/PAC proxy configuration after an OS change.
    ///
    /// Only dynamic policies can be affected. Explicit direct, static and
    /// browser-opaque policies keep their configured meaning and are a no-op.
    pub fn invalidate_proxy_configuration(&self) -> PolicyUpdate {
        let (previous_epoch, epoch, phase, snapshot, config) = {
            let mut state = lock(&self.state);
            if !matches!(
                state.proxy_config,
                ProxyConfig::SystemDynamic | ProxyConfig::PacUrl { .. }
            ) {
                return PolicyUpdate::unchanged(state.epoch);
            }
            let snapshot = state.snapshot.clone();
            let config = state.proxy_config.clone();
            let (previous_epoch, epoch) = advance_epoch_locked(&mut state);
            (previous_epoch, epoch, state.app_phase, snapshot, config)
        };

        self.emit(NetworkEvent::ProxyConfigChanged {
            config: config.clone(),
        });
        self.emit(NetworkEvent::PolicyEpochAdvanced { epoch });
        let mut directives = vec![RevalidationDirective::DropAllPoolsAndReconnect];
        if snapshot.reachability.is_routable() && !snapshot.captive_portal.requires_attention() {
            directives.push(RevalidationDirective::RevalidateStaleQueries);
        }
        self.dispatch_directives(&directives, None, phase, epoch);
        self.sync_retry_epoch(epoch);
        if phase == AppPhase::ForegroundActive {
            self.flush_pending_revalidations();
        }
        PolicyUpdate {
            previous_epoch,
            epoch,
            changed: true,
            directives,
        }
    }

    fn sync_retry_epoch(&self, epoch: PolicyEpoch) {
        lock(&self.retry).reset_on_epoch_change(epoch);
    }

    /// Apply a semantically compared snapshot. An identical state never
    /// advances the policy epoch or emits a directive.
    pub fn apply_snapshot(
        &self,
        mut snapshot: NetworkSnapshot,
    ) -> Result<PolicyUpdate, SystemError> {
        snapshot.validate()?;
        let (changed, previous, epoch, previous_epoch, phase) = {
            let mut state = lock(&self.state);
            if snapshot.revision <= state.snapshot.revision {
                snapshot.revision = state.snapshot.revision.saturating_add(1);
            }
            let previous = state.snapshot.clone();
            let same = previous.semantic_eq(&snapshot);
            let previous_epoch = state.epoch;
            state.snapshot = snapshot.clone();
            if same {
                (
                    false,
                    previous,
                    state.epoch,
                    previous_epoch,
                    state.app_phase,
                )
            } else {
                let (_, epoch) = advance_epoch_locked(&mut state);
                let phase = state.app_phase;
                (true, previous, epoch, previous_epoch, phase)
            }
        };

        self.emit(NetworkEvent::SnapshotUpdated {
            snapshot: snapshot.clone(),
        });

        if !changed {
            return Ok(PolicyUpdate::unchanged(epoch));
        }

        if previous.reachability != snapshot.reachability {
            self.emit(NetworkEvent::ReachabilityChanged {
                reachability: snapshot.reachability,
            });
        }
        if previous.captive_portal != snapshot.captive_portal {
            self.emit(NetworkEvent::CaptivePortalChanged {
                status: snapshot.captive_portal.clone(),
            });
        }
        if previous.cost != snapshot.cost || previous.data_saver != snapshot.data_saver {
            self.emit(NetworkEvent::ConnectionCostChanged {
                cost: snapshot.cost,
                data_saver: snapshot.data_saver,
            });
        }
        self.emit(NetworkEvent::PolicyEpochAdvanced { epoch });
        let directives = snapshot_directives(&previous, &snapshot);
        self.dispatch_directives(&directives, None, phase, epoch);
        self.sync_retry_epoch(epoch);
        if phase == AppPhase::ForegroundActive {
            self.flush_pending_revalidations();
        }
        Ok(PolicyUpdate {
            previous_epoch,
            epoch,
            changed: true,
            directives,
        })
    }

    fn dispatch_observers(&self, scoped: &[ScopedRevalidation]) {
        if scoped.is_empty() {
            return;
        }
        let observers = {
            let state = lock(&self.state);
            state
                .observers
                .values()
                .map(|observer| (observer.filter, observer.callback.clone()))
                .collect::<Vec<_>>()
        };
        // No service, client or observer lock is held while a user callback
        // runs. A callback may safely call `current_snapshot`, `resolve_proxies`
        // or any other service method.
        for directive in scoped {
            for (filter, callback) in &observers {
                if filter.matches(directive.scope) {
                    callback(directive);
                }
            }
        }
    }

    fn dispatch_directives(
        &self,
        directives: &[RevalidationDirective],
        scope_filter: Option<ClientOwnerScope>,
        phase: AppPhase,
        epoch: PolicyEpoch,
    ) -> Vec<ScopedRevalidation> {
        if directives.is_empty() {
            return Vec::new();
        }
        let clients = {
            let state = lock(&self.state);
            state
                .clients
                .values()
                .filter_map(Weak::upgrade)
                .filter(|client| scope_filter.is_none_or(|scope| scope == client.scope_value()))
                .collect::<Vec<_>>()
        };
        let defer_stale = is_background_phase(phase)
            && directives.contains(&RevalidationDirective::RevalidateStaleQueries);
        let mut deferred = defer_stale;
        let mut scoped = Vec::new();
        for client in clients {
            for directive in directives {
                if *directive == RevalidationDirective::RevalidateStaleQueries
                    && is_background_phase(phase)
                {
                    // Mark the service pending even when no client is live yet;
                    // a client registered during the background interval must
                    // still receive exactly one coalesced flush on resume.
                    deferred = true;
                    client.defer_stale_revalidation();
                    continue;
                }
                if client.push_directive(*directive, epoch) {
                    scoped.push(ScopedRevalidation {
                        client_id: client.client_id(),
                        scope: client.scope_value(),
                        epoch,
                        directive: *directive,
                    });
                }
            }
        }
        if deferred {
            lock(&self.state).pending_stale_revalidation = true;
        }
        for directive in &scoped {
            self.emit(NetworkEvent::RevalidationDirective {
                client_id: directive.client_id,
                scope: directive.scope,
                directive: directive.directive,
                epoch: directive.epoch,
            });
        }
        self.dispatch_observers(&scoped);
        scoped
    }

    fn flush_pending_revalidations(&self) {
        let (epoch, clients, service_pending) = {
            let mut state = lock(&self.state);
            if state.app_phase != AppPhase::ForegroundActive
                || !state.snapshot.reachability.is_routable()
                || state.snapshot.captive_portal.requires_attention()
            {
                return;
            }
            let pending = state.pending_stale_revalidation;
            state.pending_stale_revalidation = false;
            let clients = state
                .clients
                .values()
                .filter_map(Weak::upgrade)
                .collect::<Vec<_>>();
            (state.epoch, clients, pending)
        };
        let mut scoped = Vec::new();
        for client in clients {
            let client_pending = client.take_pending_stale_revalidation();
            if !client_pending && !service_pending {
                continue;
            }
            if client.push_directive(RevalidationDirective::RevalidateStaleQueries, epoch) {
                scoped.push(ScopedRevalidation {
                    client_id: client.client_id(),
                    scope: client.scope_value(),
                    epoch,
                    directive: RevalidationDirective::RevalidateStaleQueries,
                });
            }
        }
        for directive in &scoped {
            self.emit(NetworkEvent::RevalidationDirective {
                client_id: directive.client_id,
                scope: directive.scope,
                directive: directive.directive,
                epoch: directive.epoch,
            });
        }
        self.dispatch_observers(&scoped);
    }

    fn revoke_clients_where(
        &self,
        matches: impl Fn(ClientOwnerScope) -> bool,
        reason: ClientRevocationReason,
    ) -> usize {
        let removed = {
            let mut state = lock(&self.state);
            let ids = state
                .clients
                .iter()
                .filter_map(|(id, weak)| {
                    let inner = weak.upgrade()?;
                    matches(inner.scope_value()).then_some(*id)
                })
                .collect::<Vec<_>>();
            ids.into_iter()
                .filter_map(|id| state.clients.remove(&id).map(|weak| (id, weak)))
                .collect::<Vec<_>>()
        };
        let mut revoked = 0;
        for (id, weak) in removed {
            if let Some(inner) = weak.upgrade()
                && inner.revoke(reason)
            {
                revoked += 1;
                self.emit(NetworkEvent::ClientRevoked {
                    client_id: id,
                    scope: inner.scope_value(),
                    reason,
                });
            }
        }
        revoked
    }
}

impl NetworkProviderListener for ServiceInner {
    fn on_network_provider_notice(&self, notice: NetworkProviderNotice) {
        match notice {
            NetworkProviderNotice::SnapshotUpdated(snapshot) => {
                let _ = self.apply_snapshot(snapshot);
            }
            NetworkProviderNotice::ProxyConfigurationInvalidated => {
                let _ = self.invalidate_proxy_configuration();
            }
            NetworkProviderNotice::ProxyCredentialsRotated => {
                let _ = self.rotate_proxy_credentials(None);
            }
        }
    }
}

fn snapshot_directives(
    previous: &NetworkSnapshot,
    current: &NetworkSnapshot,
) -> Vec<RevalidationDirective> {
    let mut directives = Vec::new();
    let reachability_changed = previous.reachability != current.reachability;
    let portal_changed = previous.captive_portal != current.captive_portal;
    if reachability_changed {
        directives.push(RevalidationDirective::DropAllPoolsAndReconnect);
        if current.reachability.is_routable() && !current.captive_portal.requires_attention() {
            directives.push(RevalidationDirective::RevalidateStaleQueries);
        }
    }

    if portal_changed {
        if current.captive_portal.requires_attention() {
            directives.push(RevalidationDirective::AwaitCaptivePortalClear);
            directives.push(RevalidationDirective::ThrottleBackgroundTransfers);
        } else if previous.captive_portal.requires_attention()
            && current.captive_portal.is_clear()
            && current.reachability.is_routable()
        {
            directives.push(RevalidationDirective::RecycleIdleConnections);
            directives.push(RevalidationDirective::RevalidateStaleQueries);
        }
    }

    let became_constrained = (!previous.cost.is_constrained() && current.cost.is_constrained())
        || (!previous.data_saver.is_constrained() && current.data_saver.is_constrained());
    if became_constrained {
        directives.push(RevalidationDirective::ThrottleBackgroundTransfers);
    }

    let relaxed = (previous.cost.is_constrained() && !current.cost.is_constrained())
        || (previous.data_saver.is_constrained() && !current.data_saver.is_constrained());
    if relaxed && current.reachability.is_routable() && current.captive_portal.is_clear() {
        directives.push(RevalidationDirective::RevalidateStaleQueries);
    }

    dedupe_directives(directives)
}

fn dedupe_directives(directives: Vec<RevalidationDirective>) -> Vec<RevalidationDirective> {
    let mut unique = Vec::with_capacity(directives.len());
    for directive in directives {
        if !unique.contains(&directive) {
            unique.push(directive);
        }
    }
    unique
}

/// Cloneable, `Send + Sync` network service suitable for `SystemServices`.
#[derive(Clone)]
pub struct NetworkService {
    inner: Arc<ServiceInner>,
}

impl NetworkService {
    pub fn new(provider: Arc<dyn NetworkProvider>) -> Self {
        Self::with_platform(
            provider,
            PlatformTarget::current().unwrap_or(PlatformTarget::Web),
        )
    }

    pub fn with_platform(provider: Arc<dyn NetworkProvider>, platform: PlatformTarget) -> Self {
        Self::with_policies_and_event_capacity(
            provider,
            platform,
            TrustProfile::Release,
            default_proxy_config(platform),
            default_trust_policy(platform),
            DEFAULT_NETWORK_EVENT_CAPACITY,
        )
        .expect("default network policy is valid for every platform")
    }

    pub fn with_event_capacity(provider: Arc<dyn NetworkProvider>, capacity: usize) -> Self {
        Self::with_platform_and_event_capacity(
            provider,
            PlatformTarget::current().unwrap_or(PlatformTarget::Web),
            capacity,
        )
    }

    pub fn with_platform_and_event_capacity(
        provider: Arc<dyn NetworkProvider>,
        platform: PlatformTarget,
        capacity: usize,
    ) -> Self {
        Self::with_policies_and_event_capacity(
            provider,
            platform,
            TrustProfile::Release,
            default_proxy_config(platform),
            default_trust_policy(platform),
            capacity,
        )
        .expect("default network policy is valid for every platform")
    }

    pub fn with_policies(
        provider: Arc<dyn NetworkProvider>,
        platform: PlatformTarget,
        profile: TrustProfile,
        proxy_config: ProxyConfig,
        trust_policy: TrustStorePolicy,
    ) -> Result<Self, SystemError> {
        Self::with_policies_and_event_capacity(
            provider,
            platform,
            profile,
            proxy_config,
            trust_policy,
            DEFAULT_NETWORK_EVENT_CAPACITY,
        )
    }

    pub fn with_policies_and_event_capacity(
        provider: Arc<dyn NetworkProvider>,
        platform: PlatformTarget,
        profile: TrustProfile,
        proxy_config: ProxyConfig,
        trust_policy: TrustStorePolicy,
        capacity: usize,
    ) -> Result<Self, SystemError> {
        validate_proxy_config(platform, &proxy_config)?;
        validate_trust_policy(platform, profile, &trust_policy)?;
        let mut snapshot = provider
            .snapshot()
            .ok()
            .filter(|snapshot| snapshot.validate().is_ok())
            .unwrap_or_else(|| NetworkSnapshot::unknown(SignalSource::Unavailable));
        if snapshot.revision == 0 {
            snapshot.revision = 1;
        }
        let inner = Arc::new(ServiceInner {
            provider: provider.clone(),
            state: Mutex::new(ServiceState {
                snapshot,
                proxy_config,
                trust_policy,
                trust_profile: profile,
                platform,
                epoch: PolicyEpoch::INITIAL,
                app_phase: AppPhase::Launching,
                clients: BTreeMap::new(),
                next_client_id: 0,
                subscribers: Vec::new(),
                proxy_auth_attempts: BTreeMap::new(),
                observers: BTreeMap::new(),
                next_observer_id: 0,
                pending_stale_revalidation: false,
                shutting_down: false,
            }),
            event_capacity: capacity.max(1),
            credential_source: Mutex::new(None),
            retry: Mutex::new(RetryTracker::new(RetryPolicy::default())),
        });
        let listener: Arc<dyn NetworkProviderListener> = inner.clone();
        provider.attach_listener(Arc::downgrade(&listener));
        Ok(Self { inner })
    }

    pub fn from_provider(provider: impl NetworkProvider + 'static) -> Self {
        Self::new(Arc::new(provider))
    }

    pub fn from_fake(fake: crate::FakeNetworkProvider) -> Self {
        Self::from_provider(fake)
    }

    pub fn unavailable() -> Self {
        Self::from_provider(crate::UnavailableNetworkProvider::new())
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

    pub fn provider(&self) -> &Arc<dyn NetworkProvider> {
        &self.inner.provider
    }

    pub fn fake(&self) -> Option<&crate::FakeNetworkProvider> {
        self.inner
            .provider
            .as_any()
            .downcast_ref::<crate::FakeNetworkProvider>()
    }

    pub fn platform(&self) -> PlatformTarget {
        lock(&self.inner.state).platform
    }

    pub fn policy_epoch(&self) -> PolicyEpoch {
        lock(&self.inner.state).epoch
    }

    /// Alias for [`Self::policy_epoch`].
    pub fn current_policy_epoch(&self) -> PolicyEpoch {
        self.policy_epoch()
    }

    pub fn app_phase(&self) -> AppPhase {
        lock(&self.inner.state).app_phase
    }

    pub fn pending_stale_revalidation(&self) -> bool {
        lock(&self.inner.state).pending_stale_revalidation
    }

    pub fn current_snapshot(&self) -> NetworkSnapshot {
        lock(&self.inner.state).snapshot.clone()
    }

    pub fn revision(&self) -> u64 {
        self.current_snapshot().revision
    }

    pub fn proxy_config(&self) -> ProxyConfig {
        lock(&self.inner.state).proxy_config.clone()
    }

    /// Invalidate host/PAC proxy state. Dynamic policies advance the epoch and
    /// reconnect; explicit direct, static and browser-opaque policies are a
    /// deliberate no-op.
    pub fn invalidate_proxy_configuration(&self) -> PolicyUpdate {
        self.inner.invalidate_proxy_configuration()
    }

    pub fn trust_policy(&self) -> TrustStorePolicy {
        lock(&self.inner.state).trust_policy.clone()
    }

    pub fn trust_profile(&self) -> TrustProfile {
        lock(&self.inner.state).trust_profile
    }

    pub fn capability(&self, operation: NetworkOperation) -> Capability<NetworkOperation> {
        self.inner.provider.capability(operation)
    }

    pub fn capabilities(&self) -> Vec<Capability<NetworkOperation>> {
        self.inner.provider.capabilities()
    }

    /// Refresh from the provider under the ObserveConnectivity capability gate.
    pub fn snapshot(&self, context: &OperationContext) -> ServiceResult<NetworkSnapshot> {
        let operation = NetworkOperation::ObserveConnectivity;
        if let Some(cancelled) = self.inner.check_context(operation, context)? {
            return Ok(cancelled);
        }
        if let Err(error) = self.inner.gate_capability(operation) {
            return self.inner.fail(operation, error);
        }
        let snapshot = match self.inner.provider.snapshot() {
            Ok(snapshot) => snapshot,
            Err(error) => return self.inner.fail(operation, error),
        };
        if let Err(error) = context.check_at(Instant::now()) {
            return self.inner.fail(operation, error);
        }
        if let Some(reason) = context.cancellation.reason() {
            return Ok(Completion::Cancelled(reason));
        }
        if let Err(error) = self.apply_snapshot(snapshot) {
            return self.inner.fail(operation, error);
        }
        Ok(Completion::Completed(self.current_snapshot()))
    }

    /// Apply a semantically compared snapshot. An identical state never
    /// advances the policy epoch or emits a directive.
    pub fn apply_snapshot(&self, snapshot: NetworkSnapshot) -> Result<PolicyUpdate, SystemError> {
        self.inner.apply_snapshot(snapshot)
    }

    /// Alias for [`Self::apply_snapshot`].
    pub fn set_snapshot(&self, snapshot: NetworkSnapshot) -> Result<PolicyUpdate, SystemError> {
        self.apply_snapshot(snapshot)
    }

    /// Alias for [`Self::apply_snapshot`].
    pub fn update_snapshot(&self, snapshot: NetworkSnapshot) -> Result<PolicyUpdate, SystemError> {
        self.apply_snapshot(snapshot)
    }

    pub fn set_proxy_config(&self, config: ProxyConfig) -> Result<PolicyUpdate, SystemError> {
        let platform = self.platform();
        validate_proxy_config(platform, &config)?;
        let (previous_epoch, epoch, phase, changed) = {
            let mut state = lock(&self.inner.state);
            if state.proxy_config == config {
                let epoch = state.epoch;
                (epoch, epoch, state.app_phase, false)
            } else {
                state.proxy_config = config.clone();
                let (previous_epoch, epoch) = advance_epoch_locked(&mut state);
                let phase = state.app_phase;
                (previous_epoch, epoch, phase, true)
            }
        };
        if !changed {
            return Ok(PolicyUpdate::unchanged(epoch));
        }
        self.inner.emit(NetworkEvent::ProxyConfigChanged {
            config: config.clone(),
        });
        self.inner.emit(NetworkEvent::PolicyEpochAdvanced { epoch });
        let snapshot = self.current_snapshot();
        let mut directives = vec![RevalidationDirective::DropAllPoolsAndReconnect];
        if snapshot.reachability.is_routable() && !snapshot.captive_portal.requires_attention() {
            directives.push(RevalidationDirective::RevalidateStaleQueries);
        }
        let directives = dedupe_directives(directives);
        self.inner
            .dispatch_directives(&directives, None, phase, epoch);
        self.inner.sync_retry_epoch(epoch);
        if phase == AppPhase::ForegroundActive {
            self.inner.flush_pending_revalidations();
        }
        Ok(PolicyUpdate {
            previous_epoch,
            epoch,
            changed: true,
            directives,
        })
    }

    pub fn set_trust_policy(&self, policy: TrustStorePolicy) -> Result<PolicyUpdate, SystemError> {
        let platform = self.platform();
        let profile = self.trust_profile();
        validate_trust_policy(platform, profile, &policy)?;
        let (previous_epoch, epoch, phase, changed) = {
            let mut state = lock(&self.inner.state);
            if state.trust_policy == policy {
                let epoch = state.epoch;
                (epoch, epoch, state.app_phase, false)
            } else {
                state.trust_policy = policy.clone();
                let (previous_epoch, epoch) = advance_epoch_locked(&mut state);
                let phase = state.app_phase;
                (previous_epoch, epoch, phase, true)
            }
        };
        if !changed {
            return Ok(PolicyUpdate::unchanged(epoch));
        }
        self.inner.emit(NetworkEvent::TrustPolicyChanged {
            policy: policy.clone(),
        });
        self.inner.emit(NetworkEvent::PolicyEpochAdvanced { epoch });
        let snapshot = self.current_snapshot();
        let mut directives = vec![RevalidationDirective::DropAllPoolsAndReconnect];
        if snapshot.reachability.is_routable() && !snapshot.captive_portal.requires_attention() {
            directives.push(RevalidationDirective::RevalidateStaleQueries);
        }
        let directives = dedupe_directives(directives);
        self.inner
            .dispatch_directives(&directives, None, phase, epoch);
        self.inner.sync_retry_epoch(epoch);
        if phase == AppPhase::ForegroundActive {
            self.inner.flush_pending_revalidations();
        }
        Ok(PolicyUpdate {
            previous_epoch,
            epoch,
            changed: true,
            directives,
        })
    }

    pub fn set_app_phase(&self, phase: AppPhase) {
        {
            let mut state = lock(&self.inner.state);
            if state.app_phase == phase {
                return;
            }
            state.app_phase = phase;
        }
        self.inner.emit(NetworkEvent::AppPhaseChanged { phase });
        if phase == AppPhase::ForegroundActive {
            self.inner.flush_pending_revalidations();
        }
    }

    pub fn apply_lifecycle_state(&self, lifecycle: &LifecycleState) {
        self.set_app_phase(lifecycle.phase());
    }

    /// Register one explicitly owned client lease.
    pub fn register_client(
        &self,
        scope: ClientOwnerScope,
        context: &OperationContext,
    ) -> ServiceResult<NetworkClientLease> {
        let operation = NetworkOperation::SubscribeRevalidation;
        if let Some(cancelled) = self.inner.check_context(operation, context)? {
            return Ok(cancelled);
        }
        if let ClientOwnerScope::Surface {
            surface,
            generation,
        } = scope
            && (surface == SurfaceId(0) || generation == 0)
        {
            return self.inner.fail(
                operation,
                SystemError::new(
                    ErrorCode::InvalidInput,
                    "Surface client scope requires non-zero SurfaceId and generation",
                ),
            );
        }
        let registration = {
            let mut state = lock(&self.inner.state);
            if state.shutting_down {
                None
            } else {
                state.next_client_id = state.next_client_id.saturating_add(1);
                Some((ClientId(state.next_client_id), state.epoch))
            }
        };
        let Some((id, epoch)) = registration else {
            return self.inner.fail(
                operation,
                SystemError::new(ErrorCode::Unavailable, "Network service is shutting down"),
            );
        };
        let inner = ClientInner::new(id, scope, epoch, Arc::downgrade(&self.inner));
        {
            let mut state = lock(&self.inner.state);
            state.clients.insert(id, Arc::downgrade(&inner));
        }
        self.inner.emit(NetworkEvent::ClientRegistered {
            client_id: id,
            scope,
        });
        Ok(Completion::Completed(NetworkClientLease::new(inner)))
    }

    pub fn active_client_count(&self) -> usize {
        let state = lock(&self.inner.state);
        state.clients.values().filter_map(Weak::upgrade).count()
    }

    pub fn dispose_surface(&self, surface: SurfaceId) -> usize {
        self.inner.revoke_clients_where(
            |scope| matches!(scope, ClientOwnerScope::Surface { surface: actual, .. } if actual == surface),
            ClientRevocationReason::SurfaceDisposed,
        )
    }

    pub fn dispose_surface_generation(&self, surface: SurfaceId, generation: u64) -> usize {
        self.inner.revoke_clients_where(
            |scope| {
                matches!(
                    scope,
                    ClientOwnerScope::Surface {
                        surface: actual_surface,
                        generation: actual_generation,
                    } if actual_surface == surface && actual_generation == generation
                )
            },
            ClientRevocationReason::SurfaceDisposed,
        )
    }

    pub fn revoke_stale_generations(&self, surface: SurfaceId, current_generation: u64) -> usize {
        self.inner.revoke_clients_where(
            |scope| {
                matches!(
                    scope,
                    ClientOwnerScope::Surface {
                        surface: actual_surface,
                        generation,
                    } if actual_surface == surface && generation != current_generation
                )
            },
            ClientRevocationReason::StaleGeneration,
        )
    }

    pub fn revoke_account_clients(&self, account_key: &AccountScopeKey) -> usize {
        self.inner.revoke_clients_where(
            |scope| {
                matches!(
                    scope,
                    ClientOwnerScope::Account {
                        account_key: actual
                    } if actual == *account_key
                )
            },
            ClientRevocationReason::AccountCleared,
        )
    }

    pub fn shutdown(&self) -> usize {
        {
            let mut state = lock(&self.inner.state);
            state.shutting_down = true;
        }
        self.inner
            .revoke_clients_where(|_| true, ClientRevocationReason::ServiceShutdown)
    }

    /// Capability-, platform- and context-gated proxy resolution.
    pub fn resolve_proxies(
        &self,
        target: &EndpointOrigin,
        context: &OperationContext,
    ) -> ServiceResult<Vec<ProxyCandidate>> {
        let operation = NetworkOperation::ResolveSystemProxy;
        if let Some(cancelled) = self.inner.check_context(operation, context)? {
            return Ok(cancelled);
        }
        let (platform, config) = {
            let state = lock(&self.inner.state);
            (state.platform, state.proxy_config.clone())
        };
        if let Err(error) = validate_proxy_config(platform, &config) {
            return self.inner.fail(operation, error);
        }
        if platform == PlatformTarget::Web || config.is_browser_opaque() {
            return self.inner.fail(
                operation,
                SystemError::new(
                    ErrorCode::Unsupported,
                    "The browser owns system proxy resolution",
                ),
            );
        }
        if let Err(error) = self.inner.gate_capability(operation) {
            return self.inner.fail(operation, error);
        }

        let result = match &config {
            ProxyConfig::Direct | ProxyConfig::StaticRules { .. } => {
                Ok(config.resolve_proxies(target))
            }
            _ => self.inner.provider.resolve_proxies(target, &config),
        };

        if let Err(error) = context.check_at(Instant::now()) {
            return self.inner.fail(operation, error);
        }
        if let Some(reason) = context.cancellation.reason() {
            return Ok(Completion::Cancelled(reason));
        }
        match result {
            Ok(candidates) => Ok(Completion::Completed(candidates)),
            Err(error) => self.inner.fail(operation, error),
        }
    }

    /// Capability- and context-gated endpoint diagnosis.
    pub fn diagnose_endpoint(
        &self,
        target: &EndpointOrigin,
        context: &OperationContext,
    ) -> ServiceResult<EndpointDiagnostic> {
        let operation = NetworkOperation::DiagnoseEndpointPath;
        if let Some(cancelled) = self.inner.check_context(operation, context)? {
            return Ok(cancelled);
        }
        if let Err(error) = self.inner.gate_capability(operation) {
            return self.inner.fail(operation, error);
        }
        let result = self.inner.provider.diagnose_endpoint(target);
        if let Err(error) = context.check_at(Instant::now()) {
            return self.inner.fail(operation, error);
        }
        if let Some(reason) = context.cancellation.reason() {
            return Ok(Completion::Cancelled(reason));
        }
        match result {
            Ok(diagnostic) => Ok(Completion::Completed(diagnostic)),
            Err(error) => self.inner.fail(operation, error),
        }
    }

    pub fn set_proxy_credential_source(&self, source: Arc<dyn ProxyCredentialSource>) {
        *lock(&self.inner.credential_source) = Some(source);
    }

    pub fn clear_proxy_credential_source(&self) {
        *lock(&self.inner.credential_source) = None;
    }

    pub fn with_proxy_credentials(self, source: Arc<dyn ProxyCredentialSource>) -> Self {
        self.set_proxy_credential_source(source);
        self
    }

    /// Resolve a 407 challenge using the configured credential source.
    ///
    /// This method is background-only: a UI-attached context returns
    /// `ErrorCode::InvalidInput` exactly like `gpui-secrets`.
    pub fn handle_proxy_auth(
        &self,
        challenge: &ProxyAuthChallenge,
        context: &OperationContext,
    ) -> ServiceResult<ProxyAuthOutcome> {
        let source = lock(&self.inner.credential_source).clone();
        let Some(source) = source else {
            let operation = NetworkOperation::HandleProxyAuth;
            if context.ui.is_some() {
                return Err(SystemError::new(
                    ErrorCode::InvalidInput,
                    "Proxy credentials require a background context without a UI attachment",
                ));
            }
            if let Some(cancelled) = self.inner.check_context(operation, context)? {
                return Ok(cancelled);
            }
            if let Err(error) = self.inner.gate_capability(operation) {
                return self.inner.fail(operation, error);
            }
            if self.platform() == PlatformTarget::Web {
                return self.inner.fail(
                    operation,
                    SystemError::new(
                        ErrorCode::Unsupported,
                        "The browser owns proxy authentication",
                    ),
                );
            }
            if self.proxy_auth_attempts(challenge) >= MAX_PROXY_AUTH_ATTEMPTS {
                self.inner.emit(NetworkEvent::ProxyAuthHandled {
                    client_id: challenge.client_id(),
                    proxy: challenge.proxy().clone(),
                    outcome: ProxyAuthOutcomeKind::Exhausted,
                    epoch: challenge.policy_epoch(),
                });
                return Ok(Completion::Completed(ProxyAuthOutcome::Exhausted));
            }
            self.inner.emit(NetworkEvent::ProxyAuthHandled {
                client_id: challenge.client_id(),
                proxy: challenge.proxy().clone(),
                outcome: ProxyAuthOutcomeKind::MissingCredentials,
                epoch: challenge.policy_epoch(),
            });
            return Ok(Completion::Completed(ProxyAuthOutcome::MissingCredentials));
        };
        self.handle_proxy_auth_with(challenge, source.as_ref(), context)
    }

    /// Resolve a 407 challenge with an explicit credential source.
    pub fn handle_proxy_auth_with(
        &self,
        challenge: &ProxyAuthChallenge,
        source: &dyn ProxyCredentialSource,
        context: &OperationContext,
    ) -> ServiceResult<ProxyAuthOutcome> {
        let operation = NetworkOperation::HandleProxyAuth;
        if context.ui.is_some() {
            return Err(SystemError::new(
                ErrorCode::InvalidInput,
                "Proxy credentials require a background context without a UI attachment",
            ));
        }
        if let Some(cancelled) = self.inner.check_context(operation, context)? {
            return Ok(cancelled);
        }
        if let Err(error) = self.inner.gate_capability(operation) {
            return self.inner.fail(operation, error);
        }
        if self.platform() == PlatformTarget::Web {
            return self.inner.fail(
                operation,
                SystemError::new(
                    ErrorCode::Unsupported,
                    "The browser owns proxy authentication",
                ),
            );
        }

        let attempt_key = ProxyAuthAttemptKey {
            client_id: challenge.client_id(),
            proxy: challenge.proxy().clone(),
            epoch: challenge.policy_epoch(),
        };
        let attempts = self.proxy_auth_attempts(challenge);
        if attempts >= MAX_PROXY_AUTH_ATTEMPTS {
            self.emit_proxy_auth_outcome(challenge, ProxyAuthOutcomeKind::Exhausted);
            return Ok(Completion::Completed(ProxyAuthOutcome::Exhausted));
        }

        // The source call happens with no service, client or credential-source
        // lock held. It may re-enter the service.
        let access = match source.credential(challenge, context) {
            Ok(completion) => completion,
            Err(error) => return self.inner.fail(operation, error),
        };
        if let Err(error) = context.check_at(Instant::now()) {
            return self.inner.fail(operation, error);
        }
        if let Some(reason) = context.cancellation.reason() {
            return Ok(Completion::Cancelled(reason));
        }

        match access {
            Completion::Cancelled(reason) => Ok(Completion::Cancelled(reason)),
            Completion::Completed(SecretAccess::Locked) => {
                self.emit_proxy_auth_outcome(challenge, ProxyAuthOutcomeKind::Locked);
                Ok(Completion::Completed(ProxyAuthOutcome::Locked))
            }
            Completion::Completed(SecretAccess::KeyInvalidated) => {
                self.emit_proxy_auth_outcome(challenge, ProxyAuthOutcomeKind::KeyInvalidated);
                Ok(Completion::Completed(ProxyAuthOutcome::KeyInvalidated))
            }
            Completion::Completed(SecretAccess::Available(None)) => {
                self.emit_proxy_auth_outcome(challenge, ProxyAuthOutcomeKind::MissingCredentials);
                Ok(Completion::Completed(ProxyAuthOutcome::MissingCredentials))
            }
            Completion::Completed(SecretAccess::Available(Some(bytes))) => {
                {
                    let mut state = lock(&self.inner.state);
                    let count = state.proxy_auth_attempts.entry(attempt_key).or_insert(0);
                    *count = count.saturating_add(1);
                }
                let header = ProxyAuthHeader::new(challenge.scheme(), bytes);
                self.emit_proxy_auth_outcome(challenge, ProxyAuthOutcomeKind::Header);
                Ok(Completion::Completed(ProxyAuthOutcome::Header(header)))
            }
        }
    }

    fn proxy_auth_attempts(&self, challenge: &ProxyAuthChallenge) -> u8 {
        let key = ProxyAuthAttemptKey {
            client_id: challenge.client_id(),
            proxy: challenge.proxy().clone(),
            epoch: challenge.policy_epoch(),
        };
        lock(&self.inner.state)
            .proxy_auth_attempts
            .get(&key)
            .copied()
            .unwrap_or(0)
    }

    fn emit_proxy_auth_outcome(
        &self,
        challenge: &ProxyAuthChallenge,
        outcome: ProxyAuthOutcomeKind,
    ) {
        self.inner.emit(NetworkEvent::ProxyAuthHandled {
            client_id: challenge.client_id(),
            proxy: challenge.proxy().clone(),
            outcome,
            epoch: challenge.policy_epoch(),
        });
    }

    /// Rotate proxy credentials globally or for one exact owner scope.
    pub fn rotate_proxy_credentials(&self, scope: Option<&ClientOwnerScope>) -> PolicyUpdate {
        self.inner.rotate_proxy_credentials(scope)
    }

    pub fn notify_proxy_credentials_rotated(
        &self,
        scope: Option<&ClientOwnerScope>,
    ) -> PolicyEpoch {
        self.rotate_proxy_credentials(scope).epoch
    }

    pub fn evaluate_tls_trust(
        &self,
        chain: &crate::CertificateChainObservation,
    ) -> TrustEvaluation {
        let policy = self.trust_policy();
        evaluate_tls_trust(chain, &policy)
    }

    pub fn evaluate_workload_budget(
        &self,
        workload: NetworkWorkloadClass,
    ) -> WorkloadBudgetDecision {
        self.current_snapshot().evaluate_workload_budget(workload)
    }

    /// Register a revalidation observer. Callbacks run outside every internal
    /// lock and may re-enter any service method.
    pub fn on_revalidation<F>(
        &self,
        scope_filter: impl Into<RevalidationScopeFilter>,
        callback: F,
    ) -> RevalidationObserverLease
    where
        F: Fn(&ScopedRevalidation) + Send + Sync + 'static,
    {
        let id = {
            let mut state = lock(&self.inner.state);
            state.next_observer_id = state.next_observer_id.saturating_add(1);
            let id = state.next_observer_id;
            state.observers.insert(
                id,
                ObserverRecord {
                    filter: scope_filter.into(),
                    callback: Arc::new(callback),
                },
            );
            id
        };
        RevalidationObserverLease {
            id,
            service: Arc::downgrade(&self.inner),
        }
    }

    pub fn subscribe(&self) -> EventSubscription<NetworkEvent> {
        let live = Arc::new(AtomicBool::new(true));
        let cleanup_live = live.clone();
        let (sender, subscription) = event_channel(self.inner.event_capacity, move || {
            cleanup_live.store(false, AtomicOrdering::Release);
        });
        {
            let mut state = lock(&self.inner.state);
            state.subscribers.push((live, sender));
        }
        subscription
    }

    pub fn retry_policy(&self) -> RetryPolicy {
        lock(&self.inner.retry).policy()
    }

    pub fn set_retry_policy(&self, policy: RetryPolicy) {
        lock(&self.inner.retry).set_policy(policy);
    }

    pub fn retry_decision(&self, diagnostic: &EndpointDiagnostic) -> RetryDecision {
        self.retry_decision_at(diagnostic, Instant::now())
    }

    pub fn retry_decision_at(
        &self,
        diagnostic: &EndpointDiagnostic,
        now: Instant,
    ) -> RetryDecision {
        lock(&self.inner.retry).evaluate(diagnostic, now)
    }

    pub fn on_network_change(&self, now: Instant) -> bool {
        lock(&self.inner.retry).on_network_change(now)
    }

    pub fn reset_retry(&self) {
        lock(&self.inner.retry).reset();
    }
}

impl fmt::Debug for NetworkService {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let snapshot = self.current_snapshot();
        formatter
            .debug_struct("NetworkService")
            .field("platform", &self.platform())
            .field("epoch", &self.policy_epoch())
            .field("reachability", &snapshot.reachability)
            .field("captive_portal", &snapshot.captive_portal)
            .field("clients", &self.active_client_count())
            .finish()
    }
}

/// Owns one revalidation observer registration.
pub struct RevalidationObserverLease {
    id: u64,
    service: Weak<ServiceInner>,
}

impl RevalidationObserverLease {
    pub const fn id(&self) -> u64 {
        self.id
    }

    pub fn is_active(&self) -> bool {
        self.service
            .upgrade()
            .is_some_and(|service| lock(&service.state).observers.contains_key(&self.id))
    }
}

impl Drop for RevalidationObserverLease {
    fn drop(&mut self) {
        if let Some(service) = self.service.upgrade() {
            lock(&service.state).observers.remove(&self.id);
        }
    }
}

impl fmt::Debug for RevalidationObserverLease {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RevalidationObserverLease")
            .field("id", &self.id)
            .field("active", &self.is_active())
            .finish()
    }
}

fn default_proxy_config(platform: PlatformTarget) -> ProxyConfig {
    if platform == PlatformTarget::Web {
        ProxyConfig::BrowserOpaque
    } else {
        ProxyConfig::Direct
    }
}

fn default_trust_policy(platform: PlatformTarget) -> TrustStorePolicy {
    if platform == PlatformTarget::Web {
        TrustStorePolicy::BrowserManagedTls
    } else {
        TrustStorePolicy::OsSystemRoots
    }
}

/// Compare epochs in ascending order for deterministic diagnostics.
pub fn compare_epochs(left: PolicyEpoch, right: PolicyEpoch) -> Ordering {
    left.cmp(&right)
}
