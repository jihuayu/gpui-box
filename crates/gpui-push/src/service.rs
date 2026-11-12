//! Push service: provider selection, device registration, account binding,
//! retryable logout and the four-stage routing pipeline.
//!
//! Lock discipline: service state is a single mutex. No guard is held while
//! calling a [`PushProvider`], `gpui-secrets`, the presenter, the permission
//! probe, the activation sink or an event subscriber. Every mutex acquisition,
//! including `Drop` paths, recovers a poisoned guard.

use crate::clock::{PushClock, SystemPushClock};
use crate::dedup::DedupJournal;
use crate::model::{
    AccountTombstone, AppDeliveryCondition, BackgroundWorkBudget, BackgroundWorkClass,
    BackgroundWorkHandoff, BackgroundWorkRecommendation, DEFAULT_DEDUP_CAPACITY,
    DEFAULT_DEDUP_TTL_MS, DEFAULT_PENDING_BACKGROUND_HANDOFFS, DEFAULT_PENDING_COLD_ACTIVATIONS,
    DEFAULT_PUSH_EVENT_CAPACITY, DEFAULT_RECENT_MESSAGE_CAPACITY, DeviceRegistration,
    ForegroundPresentationPolicy, IncomingPushFrame, LocalNotification, LocalPresentationOutcome,
    LocalPresentationReceipt, MAX_BACKGROUND_WORK_BUDGET_MS, MAX_PENDING_BACKGROUND_HANDOFFS,
    MAX_PENDING_COLD_ACTIVATIONS, MAX_PUSH_PAYLOAD_BYTES, MessageReceipt, PendingActivation,
    ProviderHandoff, ProviderHandoffOutcome, ProviderHealth, ProviderSelectionPolicy,
    PushAccountId, PushActivationEnvelope, PushActivationOutcome, PushActivationRequest,
    PushCounters, PushDurableState, PushEvent, PushMessageId, PushNotificationSummary,
    PushPayloadKind, PushProviderDescriptor, PushProviderKind, PushSnapshot, PushSubscription,
    PushTopic, RegistrationSnapshot, RegistrationSyncState, SubscriptionOwner, TokenFingerprint,
    TokenRotationTrigger, TombstoneState,
};
use crate::platform::{PushOperation, PushOperationSupport, PushPlatform, platform_support_matrix};
use crate::provider::{ProviderRevocation, PushProvider};
use crate::secrets::{
    PushSecretVault, RegistrationCredentialLease, SecretVaultError, secret_account_name,
};
use crate::store::PushStore;
use crate::validation::{PushClientConfiguration, WebPushHostState, validate_activation_route};
use gpui_app_data::{AppData, DirectoryRoots};
use gpui_permissions::{PermissionKind, PermissionStatus};
use gpui_secrets::Secrets;
use gpui_system::{
    CancelReason, Completion, ErrorCode, EventSender, EventSubscription, OperationContext,
    RetryAdvice, ServiceResult, SurfaceId, SystemError, event_channel,
};
use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    fmt,
    path::Path,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Instant,
};
use tempfile::TempDir;

fn error(code: ErrorCode, message: &'static str) -> SystemError {
    SystemError::new(code, message)
}

fn tombstone_error() -> SystemError {
    let mut failure = error(
        ErrorCode::Unavailable,
        "A push account logout is pending secret cleanup",
    );
    failure.retry = RetryAdvice::AfterConditionsChange;
    failure
}

fn not_registered() -> SystemError {
    error(ErrorCode::InvalidInput, "No push device is registered")
}

fn stale_registration() -> SystemError {
    let mut failure = error(
        ErrorCode::ContextExpired,
        "Push registration changed during the operation",
    );
    failure.retry = RetryAdvice::Retry;
    failure
}

fn cancelled<T>(reason: CancelReason) -> ServiceResult<T> {
    Ok(Completion::Cancelled(reason))
}

fn completed<T>(value: T) -> ServiceResult<T> {
    Ok(Completion::Completed(value))
}

fn lock<T>(value: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    value
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn background_gate(
    owner: crate::ExecutionOwner,
    context: &OperationContext,
) -> Result<Option<CancelReason>, SystemError> {
    owner.require_background(context)?;
    if let Some(reason) = context.cancellation.reason() {
        return Ok(Some(reason));
    }
    context.check_at(Instant::now())?;
    Ok(None)
}

/// The local notification boundary. A receipt is not proof of display.
pub trait LocalNotificationPresenter: Send + Sync + 'static {
    fn present(
        &self,
        notification: &LocalNotification,
    ) -> Result<LocalPresentationReceipt, SystemError>;
}

/// The activation handoff boundary. A host maps the envelope onto its
/// `gpui-activation` route registry and durable inbox.
pub trait ActivationHandoffSink: Send + Sync + 'static {
    fn handoff(&self, envelope: &PushActivationEnvelope) -> Result<(), SystemError>;
}

/// The notification-permission observation boundary, compatible with
/// `gpui-permissions` [`PermissionKind::Notifications`].
pub trait PushPermissionProbe: Send + Sync + 'static {
    fn notification_status(&self) -> PermissionStatus;
}

/// The permission kind every Stage 3 presentation is gated on.
pub const fn notification_permission() -> PermissionKind {
    PermissionKind::Notifications
}

/// Honest presenter used when no host adapter is installed.
#[derive(Debug, Clone, Copy, Default)]
pub struct UnavailablePresenter;

impl LocalNotificationPresenter for UnavailablePresenter {
    fn present(
        &self,
        _notification: &LocalNotification,
    ) -> Result<LocalPresentationReceipt, SystemError> {
        Err(error(
            ErrorCode::Unavailable,
            "No local notification presenter is installed",
        ))
    }
}

/// Honest activation sink used when no host adapter is installed.
#[derive(Debug, Clone, Copy, Default)]
pub struct UnavailableActivationSink;

impl ActivationHandoffSink for UnavailableActivationSink {
    fn handoff(&self, _envelope: &PushActivationEnvelope) -> Result<(), SystemError> {
        Err(error(
            ErrorCode::Unavailable,
            "No push activation handoff sink is installed",
        ))
    }
}

/// An activation sink that accepts and discards validated envelopes.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoopActivationHandoffSink;

impl ActivationHandoffSink for NoopActivationHandoffSink {
    fn handoff(&self, _envelope: &PushActivationEnvelope) -> Result<(), SystemError> {
        Ok(())
    }
}

/// A deterministic permission probe.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FixedPermissionProbe {
    status: PermissionStatus,
}

impl FixedPermissionProbe {
    pub const fn new(status: PermissionStatus) -> Self {
        Self { status }
    }

    pub const fn status(self) -> PermissionStatus {
        self.status
    }
}

impl Default for FixedPermissionProbe {
    fn default() -> Self {
        Self::new(PermissionStatus::NotDetermined)
    }
}

impl PushPermissionProbe for FixedPermissionProbe {
    fn notification_status(&self) -> PermissionStatus {
        self.status
    }
}

/// A recording presenter for contract tests. It never displays anything.
#[derive(Clone, Default)]
pub struct RecordingPresenter {
    calls: Arc<Mutex<Vec<LocalNotification>>>,
    failure: Arc<Mutex<Option<SystemError>>>,
    delegate: Arc<AtomicBool>,
}

impl RecordingPresenter {
    pub fn new() -> Self {
        Self::default()
    }

    /// Report a host-owned presentation instead of a platform submission.
    pub fn delegate_to_host(&self) {
        self.delegate.store(true, Ordering::SeqCst);
    }

    pub fn fail_with(&self, failure: SystemError) {
        *lock(&self.failure) = Some(failure);
    }

    pub fn clear_failure(&self) {
        *lock(&self.failure) = None;
    }

    pub fn call_count(&self) -> usize {
        lock(&self.calls).len()
    }

    pub fn notifications(&self) -> Vec<LocalNotification> {
        lock(&self.calls).clone()
    }

    pub fn last(&self) -> Option<LocalNotification> {
        lock(&self.calls).last().cloned()
    }
}

impl LocalNotificationPresenter for RecordingPresenter {
    fn present(
        &self,
        notification: &LocalNotification,
    ) -> Result<LocalPresentationReceipt, SystemError> {
        if let Some(failure) = lock(&self.failure).clone() {
            return Err(failure);
        }
        lock(&self.calls).push(notification.clone());
        if self.delegate.load(Ordering::SeqCst) {
            return Ok(LocalPresentationReceipt::DelegatedToHost);
        }
        Ok(LocalPresentationReceipt::Presented {
            notification_id: format!("local-{}", notification.message_id.as_str()),
        })
    }
}

impl fmt::Debug for RecordingPresenter {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RecordingPresenter")
            .field("calls", &self.call_count())
            .field("delegate", &self.delegate.load(Ordering::SeqCst))
            .finish()
    }
}

/// A recording activation sink for contract tests.
#[derive(Clone, Default)]
pub struct RecordingActivationSink {
    envelopes: Arc<Mutex<Vec<PushActivationEnvelope>>>,
    failure: Arc<Mutex<Option<SystemError>>>,
}

impl RecordingActivationSink {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn fail_with(&self, failure: SystemError) {
        *lock(&self.failure) = Some(failure);
    }

    pub fn clear_failure(&self) {
        *lock(&self.failure) = None;
    }

    pub fn call_count(&self) -> usize {
        lock(&self.envelopes).len()
    }

    pub fn envelopes(&self) -> Vec<PushActivationEnvelope> {
        lock(&self.envelopes).clone()
    }

    pub fn last(&self) -> Option<PushActivationEnvelope> {
        lock(&self.envelopes).last().cloned()
    }
}

impl ActivationHandoffSink for RecordingActivationSink {
    fn handoff(&self, envelope: &PushActivationEnvelope) -> Result<(), SystemError> {
        if let Some(failure) = lock(&self.failure).clone() {
            return Err(failure);
        }
        lock(&self.envelopes).push(envelope.clone());
        Ok(())
    }
}

impl fmt::Debug for RecordingActivationSink {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RecordingActivationSink")
            .field("envelopes", &self.call_count())
            .finish()
    }
}

/// Host-injected runtime boundaries.
pub struct PushRuntime {
    pub clock: Arc<dyn PushClock>,
    pub presenter: Arc<dyn LocalNotificationPresenter>,
    pub permissions: Arc<dyn PushPermissionProbe>,
    pub activation: Arc<dyn ActivationHandoffSink>,
}

impl PushRuntime {
    pub fn new(
        clock: Arc<dyn PushClock>,
        presenter: Arc<dyn LocalNotificationPresenter>,
        permissions: Arc<dyn PushPermissionProbe>,
        activation: Arc<dyn ActivationHandoffSink>,
    ) -> Self {
        Self {
            clock,
            presenter,
            permissions,
            activation,
        }
    }

    /// A fail-closed runtime: real clock, no presenter, unknown permission and
    /// no activation sink.
    pub fn unavailable() -> Self {
        Self {
            clock: Arc::new(SystemPushClock),
            presenter: Arc::new(UnavailablePresenter),
            permissions: Arc::new(FixedPermissionProbe::default()),
            activation: Arc::new(UnavailableActivationSink),
        }
    }
}

impl Default for PushRuntime {
    fn default() -> Self {
        Self::unavailable()
    }
}

/// Everything a host can configure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PushConfig {
    pub platform: PushPlatform,
    pub client: PushClientConfiguration,
    pub selection: ProviderSelectionPolicy,
    pub presentation: ForegroundPresentationPolicy,
    pub dedup_capacity: usize,
    pub dedup_ttl_ms: u64,
    pub event_capacity: usize,
    pub max_pending_background_handoffs: usize,
    pub max_pending_cold_activations: usize,
    pub recent_message_capacity: usize,
    pub background_budget: BackgroundWorkBudget,
}

impl Default for PushConfig {
    fn default() -> Self {
        Self {
            platform: PushPlatform::current().unwrap_or(PushPlatform::Linux),
            client: PushClientConfiguration {
                application_id: String::from("application"),
                sender_id: None,
                team_id: None,
                package_sid: None,
                vapid_public_key: None,
                client_material: None,
                web: WebPushHostState::default(),
            },
            selection: ProviderSelectionPolicy::default(),
            presentation: ForegroundPresentationPolicy::default(),
            dedup_capacity: DEFAULT_DEDUP_CAPACITY,
            dedup_ttl_ms: DEFAULT_DEDUP_TTL_MS,
            event_capacity: DEFAULT_PUSH_EVENT_CAPACITY,
            max_pending_background_handoffs: DEFAULT_PENDING_BACKGROUND_HANDOFFS,
            max_pending_cold_activations: DEFAULT_PENDING_COLD_ACTIVATIONS,
            recent_message_capacity: DEFAULT_RECENT_MESSAGE_CAPACITY,
            background_budget: BackgroundWorkBudget::default(),
        }
    }
}

impl PushConfig {
    pub fn new(platform: PushPlatform, client: PushClientConfiguration) -> Self {
        Self {
            platform,
            client,
            ..Self::default()
        }
    }

    pub fn validate(&self) -> Result<(), SystemError> {
        self.client.validate()?;
        DedupJournal::new(self.dedup_capacity, self.dedup_ttl_ms)?;
        if self.event_capacity == 0 || self.event_capacity > 1024 {
            return Err(error(
                ErrorCode::InvalidConfiguration,
                "Push event capacity must be between one and 1024",
            ));
        }
        if self.max_pending_background_handoffs == 0
            || self.max_pending_background_handoffs > MAX_PENDING_BACKGROUND_HANDOFFS
        {
            return Err(error(
                ErrorCode::InvalidConfiguration,
                "Push background handoff capacity is outside its supported bounds",
            ));
        }
        if self.max_pending_cold_activations == 0
            || self.max_pending_cold_activations > MAX_PENDING_COLD_ACTIVATIONS
        {
            return Err(error(
                ErrorCode::InvalidConfiguration,
                "Push cold activation capacity is outside its supported bounds",
            ));
        }
        if self.recent_message_capacity == 0 || self.recent_message_capacity > 4096 {
            return Err(error(
                ErrorCode::InvalidConfiguration,
                "Push recent message capacity is outside its supported bounds",
            ));
        }
        if self.background_budget.advisory_budget_ms == 0
            || self.background_budget.advisory_budget_ms > MAX_BACKGROUND_WORK_BUDGET_MS
        {
            return Err(error(
                ErrorCode::InvalidConfiguration,
                "Push background work budget is outside its supported bounds",
            ));
        }
        Ok(())
    }
}

/// The result of one `deliver` call: Stages 1-3 with explicit evidence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PushPipelineOutcome {
    pub handoff: ProviderHandoff,
    pub receipt: Option<MessageReceipt>,
    pub presentation: Option<LocalPresentationOutcome>,
}

/// The receipt of an explicit unregistration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnregistrationReceipt {
    pub provider_revocation: ProviderRevocation,
    pub secrets_deleted: usize,
    pub registration_epoch: u64,
}

/// The receipt of an account logout.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogoutReport {
    pub account: PushAccountId,
    pub registration_epoch: u64,
    pub secrets_deleted: usize,
    pub was_active: bool,
}

#[derive(Debug, Clone)]
struct AcceptedMessage {
    message_id: PushMessageId,
    kind: PushProviderKind,
    condition: AppDeliveryCondition,
    payload_kind: PushPayloadKind,
    account: Option<PushAccountId>,
    registration_epoch: u64,
    notification: Option<PushNotificationSummary>,
    activation_route: Option<String>,
    received_at_ms: u64,
    payload_bytes: usize,
    presented: bool,
}

struct EventSubscriber {
    live: Arc<AtomicBool>,
    sender: EventSender<PushEvent>,
}

struct ServiceState {
    durable: PushDurableState,
    descriptors: Vec<PushProviderDescriptor>,
    selected: Option<PushProviderDescriptor>,
    journal: DedupJournal,
    accepted: BTreeMap<PushMessageId, AcceptedMessage>,
    accepted_order: VecDeque<PushMessageId>,
    activated: BTreeSet<PushMessageId>,
    activated_order: VecDeque<PushMessageId>,
    background: VecDeque<BackgroundWorkHandoff>,
    surfaces: BTreeMap<u64, u64>,
    ui_attached: bool,
    counters: PushCounters,
    subscribers: BTreeMap<u64, EventSubscriber>,
    next_subscriber: u64,
    revision: u64,
}

impl ServiceState {
    fn new(durable: PushDurableState, journal: DedupJournal) -> Self {
        Self {
            durable,
            descriptors: Vec::new(),
            selected: None,
            journal,
            accepted: BTreeMap::new(),
            accepted_order: VecDeque::new(),
            activated: BTreeSet::new(),
            activated_order: VecDeque::new(),
            background: VecDeque::new(),
            surfaces: BTreeMap::new(),
            ui_attached: false,
            counters: PushCounters::default(),
            subscribers: BTreeMap::new(),
            next_subscriber: 1,
            revision: 0,
        }
    }
}

struct ServiceInner {
    state: Mutex<ServiceState>,
    /// Serializes background mutations. Never held across a host callback.
    mutation_gate: Mutex<()>,
    store: PushStore,
    vault: PushSecretVault,
    providers: Vec<Arc<dyn PushProvider>>,
    config: PushConfig,
    runtime: PushRuntime,
    _app: AppData,
    _temp: Option<Arc<TempDir>>,
}

impl ServiceInner {
    fn begin_mutation(&self) -> std::sync::MutexGuard<'_, ()> {
        lock(&self.mutation_gate)
    }

    fn now_ms(&self) -> u64 {
        self.runtime.clock.now_ms()
    }

    fn emit(&self, events: Vec<PushEvent>) {
        if events.is_empty() {
            return;
        }
        let senders = {
            let mut state = lock(&self.state);
            state
                .subscribers
                .retain(|_, subscriber| subscriber.live.load(Ordering::Acquire));
            state
                .subscribers
                .values()
                .map(|subscriber| subscriber.sender.clone())
                .collect::<Vec<_>>()
        };
        for event in events {
            for sender in &senders {
                sender.send(event.clone());
            }
        }
    }

    fn descriptors(&self) -> Result<DiscoveredProviders, SystemError> {
        let mut discovered = Vec::with_capacity(self.providers.len());
        for provider in &self.providers {
            let descriptor = provider.descriptor();
            descriptor.validate()?;
            discovered.push((descriptor, Arc::clone(provider)));
        }
        Ok(discovered)
    }
}

struct ResolvedProvider {
    descriptor: PushProviderDescriptor,
    provider: Arc<dyn PushProvider>,
}

/// Every installed provider with the descriptor it reported at discovery time.
type DiscoveredProviders = Vec<(PushProviderDescriptor, Arc<dyn PushProvider>)>;

/// Thread-safe push coordinator.
#[derive(Clone)]
pub struct PushService {
    inner: Arc<ServiceInner>,
}

impl PushService {
    /// Open a durable push profile in the application's private data directory.
    pub fn open(
        app: &AppData,
        providers: Vec<Arc<dyn PushProvider>>,
        secrets: Secrets,
        config: PushConfig,
        runtime: PushRuntime,
    ) -> Result<Self, SystemError> {
        Self::build(app.clone(), None, providers, secrets, config, runtime)
    }

    /// Open a private temporary store. The directory is removed when the last
    /// clone of the service is dropped.
    pub fn in_memory(
        providers: Vec<Arc<dyn PushProvider>>,
        secrets: Secrets,
        config: PushConfig,
        runtime: PushRuntime,
    ) -> Result<Self, SystemError> {
        let temp = Arc::new(TempDir::new().map_err(|_| {
            error(
                ErrorCode::PlatformFailure,
                "Temporary push storage is unavailable",
            )
        })?);
        let roots = DirectoryRoots::under(temp.path());
        let app = AppData::open(roots)?;
        Self::build(app, Some(temp), providers, secrets, config, runtime)
    }

    fn build(
        app: AppData,
        temp: Option<Arc<TempDir>>,
        providers: Vec<Arc<dyn PushProvider>>,
        secrets: Secrets,
        config: PushConfig,
        runtime: PushRuntime,
    ) -> Result<Self, SystemError> {
        config.validate()?;
        let store = PushStore::open(&app)?;
        let fallback = DedupJournal::new(config.dedup_capacity, config.dedup_ttl_ms)?;
        let journal_load = store.load_journal(fallback)?;
        let state_load = store.load_state()?;
        let journal_quarantine = journal_load.quarantined.clone();
        let state_quarantine = state_load.quarantined.clone();
        let mut counters = PushCounters::default();
        let mut diagnostics = Vec::new();
        if let Some(quarantined) = journal_quarantine {
            counters.quarantined_payloads = counters.quarantined_payloads.saturating_add(1);
            diagnostics.push(PushEvent::DedupJournalQuarantined {
                bytes: quarantined.bytes,
            });
        }
        if state_quarantine.is_some() {
            counters.quarantined_payloads = counters.quarantined_payloads.saturating_add(1);
            diagnostics.push(PushEvent::OperationFailed {
                operation: PushOperation::DiscoverPushProviders,
                code: ErrorCode::IntegrityFailure,
            });
        }
        let mut journal = journal_load.value;
        if journal.capacity != config.dedup_capacity || journal.ttl_ms != config.dedup_ttl_ms {
            journal.capacity = config.dedup_capacity;
            journal.ttl_ms = config.dedup_ttl_ms;
            journal.enforce_capacity();
            store.save_journal(&journal)?;
        }
        let mut state = ServiceState::new(state_load.value, journal);
        state.counters = counters;
        let inner = Arc::new(ServiceInner {
            state: Mutex::new(state),
            mutation_gate: Mutex::new(()),
            store,
            vault: PushSecretVault::new(secrets),
            providers,
            config,
            runtime,
            _app: app,
            _temp: temp,
        });
        let service = Self { inner };
        service.inner.emit(diagnostics);
        Ok(service)
    }

    /// True when both handles share the same state.
    pub fn shares_state_with(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.inner, &other.inner)
    }

    pub fn namespace_path(&self) -> &Path {
        self.inner.store.directory()
    }

    pub fn config(&self) -> &PushConfig {
        &self.inner.config
    }

    pub fn platform(&self) -> PushPlatform {
        self.inner.config.platform
    }

    pub fn support_matrix(&self) -> Vec<PushOperationSupport> {
        platform_support_matrix()
    }

    pub fn clock_now_ms(&self) -> u64 {
        self.inner.now_ms()
    }

    pub fn revision(&self) -> u64 {
        lock(&self.inner.state).revision
    }

    /// Subscribe to the bounded event stream. Each subscription is independent
    /// and never replays history.
    pub fn subscribe(&self) -> EventSubscription<PushEvent> {
        let capacity = self.inner.config.event_capacity.max(1);
        let live = Arc::new(AtomicBool::new(true));
        let flag = Arc::clone(&live);
        let (sender, subscription) = event_channel(capacity, move || {
            flag.store(false, Ordering::Release);
        });
        let mut state = lock(&self.inner.state);
        let id = state.next_subscriber;
        state.next_subscriber = id.saturating_add(1);
        state
            .subscribers
            .insert(id, EventSubscriber { live, sender });
        drop(state);
        subscription
    }

    pub fn subscriber_count(&self) -> usize {
        lock(&self.inner.state).subscribers.len()
    }

    // -----------------------------------------------------------------
    // Public snapshots
    // -----------------------------------------------------------------

    /// A public-safe snapshot. It never carries registration material.
    pub fn snapshot(&self) -> PushSnapshot {
        let state = lock(&self.inner.state);
        PushSnapshot {
            selected: state.selected.clone(),
            registration: state.durable.registration(),
            subscriptions: state.durable.subscriptions.clone(),
            pending_secret_cleanups: state.durable.tombstones.clone(),
            pending_background_handoffs: state.background.len(),
            pending_cold_activations: state.durable.pending_cold_activations.len(),
            counters: state.counters,
        }
    }

    pub fn registration(&self) -> RegistrationSnapshot {
        lock(&self.inner.state).durable.registration()
    }

    pub fn counters(&self) -> PushCounters {
        lock(&self.inner.state).counters
    }

    pub fn subscriptions(&self) -> Vec<PushSubscription> {
        lock(&self.inner.state).durable.subscriptions.clone()
    }

    pub fn pending_secret_cleanup(&self, account: &PushAccountId) -> Option<AccountTombstone> {
        lock(&self.inner.state)
            .durable
            .tombstone_for(account)
            .cloned()
    }

    pub fn pending_secret_cleanups(&self) -> Vec<AccountTombstone> {
        lock(&self.inner.state).durable.tombstones.clone()
    }

    pub fn pending_background_handoffs(&self) -> Vec<BackgroundWorkHandoff> {
        lock(&self.inner.state).background.iter().cloned().collect()
    }

    pub fn take_background_handoff(&self) -> Option<BackgroundWorkHandoff> {
        lock(&self.inner.state).background.pop_front()
    }

    pub fn pending_cold_activations(&self) -> Vec<PendingActivation> {
        lock(&self.inner.state)
            .durable
            .pending_cold_activations
            .clone()
    }

    pub fn journal_len(&self) -> usize {
        lock(&self.inner.state).journal.len()
    }

    pub fn ui_surface_attached(&self) -> bool {
        lock(&self.inner.state).ui_attached
    }

    /// Mark a UI surface as attached and route every retained cold-launch
    /// activation. Returns the number of envelopes handed to the sink.
    pub fn attach_ui_surface(
        &self,
        owner: crate::ExecutionOwner,
        context: &OperationContext,
    ) -> ServiceResult<usize> {
        if let Some(reason) = background_gate(owner, context)? {
            return cancelled(reason);
        }
        let pending = {
            let mutation = self.inner.begin_mutation();
            let mut state_guard = lock(&self.inner.state);
            let pending = state_guard.durable.pending_cold_activations.clone();
            if pending.is_empty() {
                state_guard.ui_attached = true;
                drop(state_guard);
                drop(mutation);
                return completed(0);
            }
            let mut next = state_guard.durable.clone();
            next.pending_cold_activations.clear();
            drop(state_guard);
            // Persist the drained queue before the flag so an I/O failure never
            // leaves `ui_attached` true with the activations still on disk.
            self.inner.store.save_state(&next)?;
            {
                let mut state = lock(&self.inner.state);
                state.durable = next;
                state.ui_attached = true;
                state.revision = state.revision.saturating_add(1);
            }
            drop(mutation);
            pending
        };
        self.route_drained_activations(pending)
    }

    /// Re-fence every drained cold-launch activation against current state
    /// before it crosses the host activation boundary.
    fn route_drained_activations(&self, pending: Vec<PendingActivation>) -> ServiceResult<usize> {
        let mut routed = 0usize;
        let mut events = Vec::new();
        for activation in pending {
            let durable = lock(&self.inner.state).durable.clone();
            if durable.token_revision == 0 {
                events.push(self.rejected_activation_event(
                    &activation.message_id,
                    PushActivationOutcome::RejectedNotRegistered,
                ));
                continue;
            }
            if durable
                .tombstones
                .iter()
                .any(|tombstone| tombstone.device_scope)
            {
                events.push(self.rejected_activation_event(
                    &activation.message_id,
                    PushActivationOutcome::RejectedAccountMismatch,
                ));
                continue;
            }
            if validate_activation_route(&activation.route).is_err() {
                events.push(self.rejected_activation_event(
                    &activation.message_id,
                    PushActivationOutcome::RejectedInvalidRoute,
                ));
                continue;
            }
            match activation.account.as_ref() {
                Some(account)
                    if durable.is_tombstoned(account)
                        || durable.account.as_ref() != Some(account) =>
                {
                    events.push(self.rejected_activation_event(
                        &activation.message_id,
                        PushActivationOutcome::RejectedAccountMismatch,
                    ));
                    continue;
                }
                None if durable.account.is_some() => {
                    events.push(self.rejected_activation_event(
                        &activation.message_id,
                        PushActivationOutcome::RejectedAccountMismatch,
                    ));
                    continue;
                }
                _ => {}
            }
            if activation.registration_epoch != durable.registration_epoch {
                events.push(self.rejected_activation_event(
                    &activation.message_id,
                    PushActivationOutcome::RejectedStaleEpoch {
                        expected: durable.registration_epoch,
                        received: activation.registration_epoch,
                    },
                ));
                continue;
            }
            if lock(&self.inner.state)
                .activated
                .contains(&activation.message_id)
            {
                events.push(self.rejected_activation_event(
                    &activation.message_id,
                    PushActivationOutcome::RejectedDuplicateActivation,
                ));
                continue;
            }
            let envelope = PushActivationEnvelope {
                message_id: activation.message_id.clone(),
                route: activation.route.clone(),
                account: activation.account.clone(),
                registration_epoch: activation.registration_epoch,
                condition: activation.condition,
            };
            match self.inner.runtime.activation.handoff(&envelope) {
                Ok(()) => {
                    routed = routed.saturating_add(1);
                    self.mark_activated(&activation.message_id);
                    events.push(PushEvent::ActivationRouted {
                        message_id: activation.message_id.clone(),
                        registration_epoch: activation.registration_epoch,
                    });
                }
                Err(failure) => {
                    events.push(self.rejected_activation_event(
                        &activation.message_id,
                        PushActivationOutcome::HandoffFailed { code: failure.code },
                    ));
                }
            }
        }
        {
            let mut state = lock(&self.inner.state);
            state.counters.activations_routed = state
                .counters
                .activations_routed
                .saturating_add(u64::try_from(routed).unwrap_or(u64::MAX));
        }
        self.inner.emit(events);
        completed(routed)
    }

    pub fn detach_ui_surface(&self) {
        lock(&self.inner.state).ui_attached = false;
    }

    /// Dispose exactly one `(surface, generation)`. A stale generation never
    /// clobbers a newer generation of the same surface.
    pub fn dispose_surface_generation(&self, surface: SurfaceId, generation: u64) -> bool {
        let mut state = lock(&self.inner.state);
        match state.surfaces.get(&surface.0).copied() {
            Some(current) if current == generation => {
                state.surfaces.remove(&surface.0);
                true
            }
            _ => false,
        }
    }

    pub fn register_surface_generation(&self, surface: SurfaceId, generation: u64) {
        let mut state = lock(&self.inner.state);
        let entry = state.surfaces.entry(surface.0).or_insert(generation);
        if *entry < generation {
            *entry = generation;
        }
    }

    pub fn surface_generation(&self, surface: SurfaceId) -> Option<u64> {
        lock(&self.inner.state).surfaces.get(&surface.0).copied()
    }

    // -----------------------------------------------------------------
    // Provider discovery and selection
    // -----------------------------------------------------------------

    pub fn discover_providers(
        &self,
        owner: crate::ExecutionOwner,
        context: &OperationContext,
    ) -> ServiceResult<Vec<PushProviderDescriptor>> {
        if let Some(reason) = background_gate(owner, context)? {
            return cancelled(reason);
        }
        let discovered = self.inner.descriptors()?;
        let mut descriptors = Vec::with_capacity(discovered.len());
        for (descriptor, _) in discovered {
            if let Some(reason) = context.cancellation.reason() {
                self.inner.emit(vec![PushEvent::OperationCancelled {
                    operation: PushOperation::DiscoverPushProviders,
                    reason,
                }]);
                return cancelled(reason);
            }
            descriptors.push(descriptor);
        }
        let count = u32::try_from(descriptors.len()).unwrap_or(u32::MAX);
        {
            let mut state = lock(&self.inner.state);
            state.descriptors = descriptors.clone();
            state.revision = state.revision.saturating_add(1);
        }
        self.inner.emit(vec![PushEvent::ProvidersDiscovered {
            count,
            platform: self.inner.config.platform,
        }]);
        completed(descriptors)
    }

    pub fn select_provider(
        &self,
        owner: crate::ExecutionOwner,
        context: &OperationContext,
    ) -> ServiceResult<PushProviderDescriptor> {
        if let Some(reason) = background_gate(owner, context)? {
            return cancelled(reason);
        }
        let resolved = self.resolve_provider()?;
        if let Some(reason) = context.cancellation.reason() {
            return cancelled(reason);
        }
        self.record_selection(&resolved.descriptor);
        completed(resolved.descriptor)
    }

    fn resolve_provider(&self) -> Result<ResolvedProvider, SystemError> {
        let discovered = self.inner.descriptors()?;
        if discovered.is_empty() {
            return Err(error(
                ErrorCode::Unavailable,
                "No push provider is installed",
            ));
        }
        let selected = match &self.inner.config.selection {
            ProviderSelectionPolicy::HostSelected {
                provider_id,
                channel_id,
            } => discovered
                .into_iter()
                .find(|(descriptor, _)| {
                    descriptor.provider_id == *provider_id && descriptor.channel_id == *channel_id
                })
                .ok_or_else(|| {
                    error(
                        ErrorCode::Unavailable,
                        "The selected push provider is not installed",
                    )
                })?,
            ProviderSelectionPolicy::SoleCandidate => {
                let mut candidates = discovered
                    .into_iter()
                    .filter(|(descriptor, _)| descriptor.configuration.is_ready());
                let first = candidates.next().ok_or_else(|| {
                    error(
                        ErrorCode::Unavailable,
                        "No configured push provider is available",
                    )
                })?;
                if candidates.next().is_some() {
                    return Err(error(
                        ErrorCode::InvalidConfiguration,
                        "Multiple push providers are configured; an explicit host selection policy is required",
                    ));
                }
                first
            }
        };
        Self::check_provider_ready(&selected.0)?;
        Ok(ResolvedProvider {
            descriptor: selected.0,
            provider: selected.1,
        })
    }

    fn check_provider_ready(descriptor: &PushProviderDescriptor) -> Result<(), SystemError> {
        if !descriptor.configuration.is_ready() {
            let mut failure = error(
                ErrorCode::InvalidConfiguration,
                "The selected push provider is not configured",
            );
            failure.recovery = Some(gpui_system::RecoveryAction::RepairConfiguration);
            return Err(failure);
        }
        match descriptor.health {
            ProviderHealth::Outage => Err(error(
                ErrorCode::Unavailable,
                "The selected push provider is in outage",
            )),
            ProviderHealth::Unavailable => Err(error(
                ErrorCode::Unavailable,
                "The selected push provider is unavailable",
            )),
            ProviderHealth::Ready | ProviderHealth::Degraded => Ok(()),
        }
    }

    fn record_selection(&self, descriptor: &PushProviderDescriptor) {
        {
            let mut state = lock(&self.inner.state);
            state.selected = Some(descriptor.clone());
            state.revision = state.revision.saturating_add(1);
        }
        self.inner.emit(vec![PushEvent::ProviderSelected {
            kind: descriptor.kind,
            provider_id: descriptor.provider_id.clone(),
            channel_id: descriptor.channel_id.clone(),
        }]);
    }

    // -----------------------------------------------------------------
    // Device registration, rotation and deletion
    // -----------------------------------------------------------------

    pub fn register_device(
        &self,
        owner: crate::ExecutionOwner,
        context: &OperationContext,
    ) -> ServiceResult<DeviceRegistration> {
        if let Some(reason) = background_gate(owner, context)? {
            return cancelled(reason);
        }
        let mutation = self.inner.begin_mutation();
        self.inner.config.client.validate()?;
        let resolved = self.resolve_provider()?;
        let (account, previous_revision) = {
            let state = lock(&self.inner.state);
            (state.durable.account.clone(), state.durable.token_revision)
        };
        if let Some(account) = account.as_ref()
            && state_is_tombstoned(&self.inner.state, account)
        {
            return Err(tombstone_error());
        }
        let material = resolved.provider.register()?;
        if let Some(reason) = context.cancellation.reason() {
            drop(mutation);
            self.inner.emit(vec![PushEvent::OperationCancelled {
                operation: PushOperation::RegisterAndRotateToken,
                reason,
            }]);
            return cancelled(reason);
        }
        let (fingerprint, snapshot) = self
            .inner
            .vault
            .store_registration_reversible(account.as_ref(), material, context)
            .map_err(SecretVaultError::into_system_error)?;
        let revision = previous_revision
            .checked_add(1)
            .ok_or_else(|| error(ErrorCode::InvalidInput, "Push token revision overflowed"))?;
        let mut next = lock(&self.inner.state).durable.clone();
        next.token_revision = revision;
        next.fingerprint = Some(fingerprint.clone());
        next.kind = Some(resolved.descriptor.kind);
        next.provider_id = Some(resolved.descriptor.provider_id.clone());
        next.channel_id = resolved.descriptor.channel_id.clone();
        next.sync = RegistrationSyncState::PendingServerSync { revision };
        if next.registration_epoch == 0 {
            next.registration_epoch = 1;
        }
        if let Err(failure) = self.inner.store.save_state(&next) {
            let _ = self.inner.vault.restore_registration(&snapshot, context);
            return Err(failure);
        }
        let registration_epoch = next.registration_epoch;
        {
            let mut state = lock(&self.inner.state);
            state.durable = next;
            state.selected = Some(resolved.descriptor.clone());
            state.revision = state.revision.saturating_add(1);
        }
        drop(mutation);
        self.inner.emit(vec![PushEvent::DeviceRegistered {
            kind: resolved.descriptor.kind,
            token_revision: revision,
            token_fingerprint: fingerprint.clone(),
            registration_epoch,
        }]);
        completed(DeviceRegistration {
            token_revision: revision,
            token_fingerprint: fingerprint,
            registration_epoch,
            account,
            sync: RegistrationSyncState::PendingServerSync { revision },
        })
    }

    pub fn acknowledge_server_sync(
        &self,
        owner: crate::ExecutionOwner,
        context: &OperationContext,
        revision: u64,
    ) -> ServiceResult<RegistrationSyncState> {
        if let Some(reason) = background_gate(owner, context)? {
            return cancelled(reason);
        }
        let mutation = self.inner.begin_mutation();
        let mut next = {
            let state = lock(&self.inner.state);
            let durable = state.durable.clone();
            match durable.sync {
                RegistrationSyncState::Unregistered => return Err(not_registered()),
                RegistrationSyncState::SyncedAtRevision { revision: synced }
                    if synced == revision && durable.token_revision == revision =>
                {
                    return completed(RegistrationSyncState::SyncedAtRevision { revision });
                }
                RegistrationSyncState::PendingServerSync { revision: pending }
                    if pending == revision && durable.token_revision == revision => {}
                _ => {
                    let mut failure = error(
                        ErrorCode::ContextExpired,
                        "Push sync acknowledgement revision is stale",
                    );
                    failure.retry = RetryAdvice::Never;
                    return Err(failure);
                }
            }
            let mut next = durable;
            next.sync = RegistrationSyncState::SyncedAtRevision { revision };
            next
        };
        self.inner.store.save_state(&next)?;
        {
            let mut state = lock(&self.inner.state);
            if state.durable.token_revision != revision
                || state.durable.sync != (RegistrationSyncState::PendingServerSync { revision })
            {
                let mut failure = error(
                    ErrorCode::ContextExpired,
                    "Push sync acknowledgement revision is stale",
                );
                failure.retry = RetryAdvice::Never;
                return Err(failure);
            }
            next.sync = RegistrationSyncState::SyncedAtRevision { revision };
            state.durable = next;
            state.revision = state.revision.saturating_add(1);
        }
        drop(mutation);
        self.inner
            .emit(vec![PushEvent::RegistrationSyncAcknowledged { revision }]);
        completed(RegistrationSyncState::SyncedAtRevision { revision })
    }

    pub fn rotate_token(
        &self,
        owner: crate::ExecutionOwner,
        context: &OperationContext,
        trigger: TokenRotationTrigger,
    ) -> ServiceResult<DeviceRegistration> {
        if let Some(reason) = background_gate(owner, context)? {
            return cancelled(reason);
        }
        let mutation = self.inner.begin_mutation();
        let (account, epoch, revision) = {
            let state = lock(&self.inner.state);
            let durable = &state.durable;
            if durable.token_revision == 0 {
                return Err(not_registered());
            }
            (
                durable.account.clone(),
                durable.registration_epoch,
                durable.token_revision,
            )
        };
        if let Some(account) = account.as_ref()
            && state_is_tombstoned(&self.inner.state, account)
        {
            return Err(tombstone_error());
        }
        let resolved = self.resolve_provider()?;
        let material = resolved.provider.rotate()?;
        if let Some(reason) = context.cancellation.reason() {
            drop(mutation);
            self.inner.emit(vec![PushEvent::OperationCancelled {
                operation: PushOperation::RegisterAndRotateToken,
                reason,
            }]);
            return cancelled(reason);
        }
        {
            let state = lock(&self.inner.state);
            if state.durable.registration_epoch != epoch
                || state.durable.account != account
                || state.durable.token_revision != revision
            {
                return Err(stale_registration());
            }
        }
        let (next_fingerprint, snapshot) = self
            .inner
            .vault
            .store_registration_reversible(account.as_ref(), material, context)
            .map_err(SecretVaultError::into_system_error)?;
        {
            let state = lock(&self.inner.state);
            if state.durable.registration_epoch != epoch || state.durable.account != account {
                drop(state);
                let _ = self.inner.vault.restore_registration(&snapshot, context);
                return Err(stale_registration());
            }
        }
        let next_revision = revision
            .checked_add(1)
            .ok_or_else(|| error(ErrorCode::InvalidInput, "Push token revision overflowed"))?;
        let mut next = lock(&self.inner.state).durable.clone();
        next.token_revision = next_revision;
        next.fingerprint = Some(next_fingerprint.clone());
        next.kind = Some(resolved.descriptor.kind);
        next.provider_id = Some(resolved.descriptor.provider_id.clone());
        next.channel_id = resolved.descriptor.channel_id.clone();
        next.sync = RegistrationSyncState::PendingServerSync {
            revision: next_revision,
        };
        if let Err(failure) = self.inner.store.save_state(&next) {
            let _ = self.inner.vault.restore_registration(&snapshot, context);
            return Err(failure);
        }
        let registration_epoch = next.registration_epoch;
        {
            let mut state = lock(&self.inner.state);
            state.durable = next;
            state.revision = state.revision.saturating_add(1);
        }
        drop(mutation);
        self.inner.emit(vec![PushEvent::TokenRotated {
            trigger,
            token_revision: next_revision,
            token_fingerprint: next_fingerprint.clone(),
            registration_epoch,
        }]);
        completed(DeviceRegistration {
            token_revision: next_revision,
            token_fingerprint: next_fingerprint,
            registration_epoch,
            account,
            sync: RegistrationSyncState::PendingServerSync {
                revision: next_revision,
            },
        })
    }

    pub fn unregister_device(
        &self,
        owner: crate::ExecutionOwner,
        context: &OperationContext,
    ) -> ServiceResult<UnregistrationReceipt> {
        self.remove_registration(owner, context, true)
    }

    pub fn delete_token(
        &self,
        owner: crate::ExecutionOwner,
        context: &OperationContext,
    ) -> ServiceResult<UnregistrationReceipt> {
        self.remove_registration(owner, context, false)
    }

    fn remove_registration(
        &self,
        owner: crate::ExecutionOwner,
        context: &OperationContext,
        revoke_at_provider: bool,
    ) -> ServiceResult<UnregistrationReceipt> {
        if let Some(reason) = background_gate(owner, context)? {
            return cancelled(reason);
        }
        let mutation = self.inner.begin_mutation();
        let (account, epoch, fingerprint, provider_id) = {
            let state = lock(&self.inner.state);
            let durable = &state.durable;
            if durable.token_revision == 0 {
                return Err(not_registered());
            }
            let Some(fingerprint) = durable.fingerprint.clone() else {
                return Err(error(
                    ErrorCode::IntegrityFailure,
                    "Push registration has no token fingerprint",
                ));
            };
            (
                durable.account.clone(),
                durable.registration_epoch,
                fingerprint,
                durable.provider_id.clone(),
            )
        };
        let mut revocation = ProviderRevocation::Unsupported;
        if revoke_at_provider {
            let provider = provider_id.and_then(|provider_id| {
                self.inner
                    .providers
                    .iter()
                    .find(|provider| provider.descriptor().provider_id == provider_id)
                    .cloned()
            });
            revocation = provider
                .and_then(|provider| provider.revoke(&fingerprint).ok())
                .unwrap_or(ProviderRevocation::Unreachable);
        }
        let mut events = Vec::new();
        let deleted = match self.inner.vault.purge(account.as_ref(), true, context) {
            Ok(deleted) => deleted,
            Err(failure) => {
                if let Some(blocker) = failure.blocker() {
                    self.persist_tombstone(
                        account.as_ref(),
                        epoch.saturating_add(1),
                        true,
                        TombstoneState::PendingSecretCleanup,
                    );
                    if let Some(account) = account.as_ref() {
                        events.push(PushEvent::SecretCleanupPending {
                            account: account.clone(),
                            blocker,
                            device_scope: true,
                        });
                    }
                }
                drop(mutation);
                self.inner.emit(events);
                return Err(failure.into_system_error());
            }
        };
        let mut next = lock(&self.inner.state).durable.clone();
        next.token_revision = 0;
        next.fingerprint = None;
        next.kind = None;
        next.provider_id = None;
        next.channel_id = None;
        next.sync = RegistrationSyncState::Unregistered;
        next.subscriptions.clear();
        next.registration_epoch = next.registration_epoch.saturating_add(1);
        self.inner.store.save_state(&next)?;
        let registration_epoch = next.registration_epoch;
        {
            let mut state = lock(&self.inner.state);
            state.durable = next;
            state.accepted.clear();
            state.accepted_order.clear();
            state.activated.clear();
            state.activated_order.clear();
            state.background.clear();
            state.revision = state.revision.saturating_add(1);
        }
        drop(mutation);
        self.inner.emit(vec![PushEvent::DeviceUnregistered {
            token_revision: 0,
            registration_epoch,
        }]);
        completed(UnregistrationReceipt {
            provider_revocation: revocation,
            secrets_deleted: deleted,
            registration_epoch,
        })
    }

    fn persist_tombstone(
        &self,
        account: Option<&PushAccountId>,
        registration_epoch: u64,
        device_scope: bool,
        state: TombstoneState,
    ) {
        let Some(account) = account else {
            return;
        };
        let mut next = lock(&self.inner.state).durable.clone();
        next.registration_epoch = next.registration_epoch.max(registration_epoch);
        next.tombstones
            .retain(|current| &current.account != account);
        next.tombstones.push(AccountTombstone {
            account: account.clone(),
            registration_epoch: next.registration_epoch,
            state,
            device_scope,
            recorded_at_ms: self.inner.now_ms(),
        });
        // A failed durable write must not leave the in-memory state claiming a
        // tombstone the disk does not have.
        if self.inner.store.save_state(&next).is_ok() {
            lock(&self.inner.state).durable = next;
        }
    }

    // -----------------------------------------------------------------
    // Account binding, switching and retryable logout
    // -----------------------------------------------------------------

    pub fn bind_account(
        &self,
        owner: crate::ExecutionOwner,
        context: &OperationContext,
        account: PushAccountId,
    ) -> ServiceResult<RegistrationSnapshot> {
        if let Some(reason) = background_gate(owner, context)? {
            return cancelled(reason);
        }
        let mutation = self.inner.begin_mutation();
        let result = self.bind_account_locked(context, account);
        drop(mutation);
        match result {
            Ok((snapshot, events)) => {
                self.inner.emit(events);
                completed(snapshot)
            }
            Err(failure) => Err(failure),
        }
    }

    fn bind_account_locked(
        &self,
        context: &OperationContext,
        account: PushAccountId,
    ) -> Result<(RegistrationSnapshot, Vec<PushEvent>), SystemError> {
        if state_is_tombstoned(&self.inner.state, &account) {
            return Err(tombstone_error());
        }
        let current = lock(&self.inner.state).durable.account.clone();
        if let Some(current) = current
            && current != account
        {
            return self.switch_account_locked(context, account);
        }
        let (previous_account, epoch, revision, fingerprint) = {
            let state = lock(&self.inner.state);
            (
                state.durable.account.clone(),
                state.durable.registration_epoch,
                state.durable.token_revision,
                state.durable.fingerprint.clone(),
            )
        };
        let mut events = Vec::new();
        if let Some(fingerprint) = fingerprint.as_ref()
            && revision > 0
            && let Err(failure) = self.rekey_secrets(
                previous_account.as_ref(),
                Some(&account),
                fingerprint,
                epoch,
                revision,
                context,
            )
        {
            return Err(self.cleanup_failure(
                &previous_account,
                epoch,
                false,
                failure,
                &mut events,
            ));
        }
        let mut next = lock(&self.inner.state).durable.clone();
        next.account = Some(account.clone());
        if next.token_revision > 0 {
            next.sync = RegistrationSyncState::PendingServerSync {
                revision: next.token_revision,
            };
        }
        self.inner.store.save_state(&next)?;
        let snapshot = next.registration();
        {
            let mut state = lock(&self.inner.state);
            state.durable = next;
            state.revision = state.revision.saturating_add(1);
        }
        events.push(PushEvent::AccountBound {
            account,
            registration_epoch: snapshot.registration_epoch,
        });
        Ok((snapshot, events))
    }

    pub fn switch_account(
        &self,
        owner: crate::ExecutionOwner,
        context: &OperationContext,
        account: PushAccountId,
    ) -> ServiceResult<RegistrationSnapshot> {
        if let Some(reason) = background_gate(owner, context)? {
            return cancelled(reason);
        }
        let mutation = self.inner.begin_mutation();
        let result = self.switch_account_locked(context, account);
        drop(mutation);
        match result {
            Ok((snapshot, events)) => {
                self.inner.emit(events);
                completed(snapshot)
            }
            Err(failure) => Err(failure),
        }
    }

    fn switch_account_locked(
        &self,
        context: &OperationContext,
        account: PushAccountId,
    ) -> Result<(RegistrationSnapshot, Vec<PushEvent>), SystemError> {
        if state_is_tombstoned(&self.inner.state, &account) {
            return Err(tombstone_error());
        }
        let (previous, epoch, revision, fingerprint) = {
            let state = lock(&self.inner.state);
            (
                state.durable.account.clone(),
                state.durable.registration_epoch,
                state.durable.token_revision,
                state.durable.fingerprint.clone(),
            )
        };
        let mut events = Vec::new();
        if previous.as_ref() == Some(&account) {
            let mut next = lock(&self.inner.state).durable.clone();
            if next.token_revision > 0 {
                next.sync = RegistrationSyncState::PendingServerSync {
                    revision: next.token_revision,
                };
            }
            self.inner.store.save_state(&next)?;
            let snapshot = next.registration();
            lock(&self.inner.state).durable = next;
            events.push(PushEvent::AccountBound {
                account,
                registration_epoch: snapshot.registration_epoch,
            });
            return Ok((snapshot, events));
        }
        if let Some(fingerprint) = fingerprint.as_ref()
            && revision > 0
            && let Err(failure) = self.rekey_secrets(
                previous.as_ref(),
                Some(&account),
                fingerprint,
                epoch,
                revision,
                context,
            )
        {
            return Err(self.cleanup_failure(&previous, epoch, false, failure, &mut events));
        }
        let mut next = lock(&self.inner.state).durable.clone();
        let next_epoch = next.registration_epoch.saturating_add(1);
        next.registration_epoch = next_epoch;
        next.account = Some(account.clone());
        next.subscriptions
            .retain(|subscription| subscription.owner.is_device());
        for subscription in &mut next.subscriptions {
            subscription.registration_epoch = next_epoch;
        }
        if next.token_revision > 0 {
            next.sync = RegistrationSyncState::PendingServerSync {
                revision: next.token_revision,
            };
        }
        self.inner.store.save_state(&next)?;
        let snapshot = next.registration();
        {
            let mut state = lock(&self.inner.state);
            state.durable = next;
            state.revision = state.revision.saturating_add(1);
        }
        events.push(PushEvent::AccountSwitched {
            previous,
            account,
            registration_epoch: next_epoch,
        });
        Ok((snapshot, events))
    }

    fn cleanup_failure(
        &self,
        account: &Option<PushAccountId>,
        registration_epoch: u64,
        device_scope: bool,
        failure: SecretVaultError,
        events: &mut Vec<PushEvent>,
    ) -> SystemError {
        if let Some(blocker) = failure.blocker() {
            self.persist_tombstone(
                account.as_ref(),
                registration_epoch.saturating_add(1),
                device_scope,
                TombstoneState::PendingSecretCleanup,
            );
            if let Some(account) = account {
                events.push(PushEvent::SecretCleanupPending {
                    account: account.clone(),
                    blocker,
                    device_scope,
                });
            }
            return blocker.error();
        }
        failure.into_system_error()
    }

    /// Retryable logout. The tombstone is durable before any secret deletion.
    pub fn logout_account(
        &self,
        owner: crate::ExecutionOwner,
        context: &OperationContext,
        account: &PushAccountId,
    ) -> ServiceResult<LogoutReport> {
        if let Some(reason) = background_gate(owner, context)? {
            return cancelled(reason);
        }
        let mutation = self.inner.begin_mutation();
        let mut next = lock(&self.inner.state).durable.clone();
        let was_active = next.account.as_ref() == Some(account);
        let next_epoch = next.registration_epoch.saturating_add(1);
        next.registration_epoch = next_epoch;
        if was_active {
            next.account = None;
            next.token_revision = 0;
            next.fingerprint = None;
            next.kind = None;
            next.provider_id = None;
            next.channel_id = None;
            next.sync = RegistrationSyncState::Unregistered;
            for subscription in &mut next.subscriptions {
                subscription.registration_epoch = next_epoch;
            }
        }
        next.subscriptions
            .retain(|subscription| subscription.owner.account() != Some(account));
        next.tombstones
            .retain(|tombstone| &tombstone.account != account);
        next.tombstones.push(AccountTombstone {
            account: account.clone(),
            registration_epoch: next_epoch,
            state: TombstoneState::LoggingOut,
            device_scope: was_active,
            recorded_at_ms: self.inner.now_ms(),
        });
        self.inner.store.save_state(&next)?;
        {
            let mut state = lock(&self.inner.state);
            state.durable = next.clone();
            state.activated.clear();
            state.activated_order.clear();
            state.revision = state.revision.saturating_add(1);
        }
        let mut events = Vec::new();
        let result = match self.inner.vault.purge(Some(account), was_active, context) {
            Ok(deleted) => {
                next.tombstones
                    .retain(|tombstone| &tombstone.account != account);
                match self.inner.store.save_state(&next) {
                    Ok(()) => {
                        {
                            let mut state = lock(&self.inner.state);
                            state.durable = next;
                            state.revision = state.revision.saturating_add(1);
                        }
                        events.push(PushEvent::AccountLoggedOut {
                            account: account.clone(),
                            registration_epoch: next_epoch,
                            secrets_deleted: u32::try_from(deleted).unwrap_or(u32::MAX),
                        });
                        Ok(deleted)
                    }
                    Err(failure) => Err(failure),
                }
            }
            Err(failure) => match failure.blocker() {
                Some(blocker) => {
                    next.tombstones
                        .retain(|tombstone| &tombstone.account != account);
                    next.tombstones.push(AccountTombstone {
                        account: account.clone(),
                        registration_epoch: next_epoch,
                        state: TombstoneState::PendingSecretCleanup,
                        device_scope: was_active,
                        recorded_at_ms: self.inner.now_ms(),
                    });
                    // Never claim a cleanup tombstone in memory that the disk
                    // rejected.
                    if self.inner.store.save_state(&next).is_ok() {
                        lock(&self.inner.state).durable = next;
                    }
                    events.push(PushEvent::SecretCleanupPending {
                        account: account.clone(),
                        blocker,
                        device_scope: was_active,
                    });
                    Err(blocker.error())
                }
                None => match failure {
                    SecretVaultError::Cancelled(reason) => {
                        drop(mutation);
                        return cancelled(reason);
                    }
                    other => Err(other.into_system_error()),
                },
            },
        };
        drop(mutation);
        self.inner.emit(events);
        match result {
            Ok(deleted) => completed(LogoutReport {
                account: account.clone(),
                registration_epoch: next_epoch,
                secrets_deleted: deleted,
                was_active,
            }),
            Err(failure) => Err(failure),
        }
    }

    /// Complete a logout whose secret cleanup was blocked by a locked vault.
    pub fn retry_pending_secret_cleanup(
        &self,
        owner: crate::ExecutionOwner,
        context: &OperationContext,
        account: &PushAccountId,
    ) -> ServiceResult<usize> {
        if let Some(reason) = background_gate(owner, context)? {
            return cancelled(reason);
        }
        let mutation = self.inner.begin_mutation();
        let tombstone = lock(&self.inner.state)
            .durable
            .tombstone_for(account)
            .cloned()
            .ok_or_else(|| {
                error(
                    ErrorCode::InvalidInput,
                    "No push secret cleanup is pending for this account",
                )
            })?;
        let deleted = match self
            .inner
            .vault
            .purge(Some(account), tombstone.device_scope, context)
        {
            Ok(deleted) => deleted,
            Err(failure) => match failure.blocker() {
                Some(blocker) => {
                    drop(mutation);
                    self.inner.emit(vec![PushEvent::SecretCleanupPending {
                        account: account.clone(),
                        blocker,
                        device_scope: tombstone.device_scope,
                    }]);
                    return Err(blocker.error());
                }
                None => return Err(failure.into_system_error()),
            },
        };
        let mut next = lock(&self.inner.state).durable.clone();
        next.tombstones
            .retain(|current| &current.account != account);
        self.inner.store.save_state(&next)?;
        {
            let mut state = lock(&self.inner.state);
            state.durable = next;
            state.revision = state.revision.saturating_add(1);
        }
        drop(mutation);
        self.inner.emit(vec![PushEvent::SecretCleanupCompleted {
            account: account.clone(),
        }]);
        completed(deleted)
    }

    fn rekey_secrets(
        &self,
        from: Option<&PushAccountId>,
        to: Option<&PushAccountId>,
        fingerprint: &TokenFingerprint,
        registration_epoch: u64,
        token_revision: u64,
        context: &OperationContext,
    ) -> Result<(), SecretVaultError> {
        let Some(lease) = self.inner.vault.read_lease(
            from,
            registration_epoch,
            token_revision,
            fingerprint,
            context,
        )?
        else {
            return Ok(());
        };
        let material = lease.into_material()?;
        self.inner.vault.store_registration(to, material, context)?;
        if secret_account_name(from) != secret_account_name(to) {
            self.inner.vault.delete_registration(from, context)?;
        }
        Ok(())
    }

    // -----------------------------------------------------------------
    // Topic subscriptions
    // -----------------------------------------------------------------

    pub fn add_subscription(
        &self,
        owner: crate::ExecutionOwner,
        context: &OperationContext,
        topic: PushTopic,
        subscription_owner: SubscriptionOwner,
    ) -> ServiceResult<PushSubscription> {
        if let Some(reason) = background_gate(owner, context)? {
            return cancelled(reason);
        }
        let mutation = self.inner.begin_mutation();
        let mut next = {
            let state = lock(&self.inner.state);
            if state.durable.token_revision == 0 {
                return Err(not_registered());
            }
            if let Some(account) = subscription_owner.account()
                && state.durable.account.as_ref() != Some(account)
            {
                return Err(error(
                    ErrorCode::InvalidInput,
                    "Push topic subscription targets a different account",
                ));
            }
            state.durable.clone()
        };
        let subscription = PushSubscription::new(
            topic.clone(),
            subscription_owner.clone(),
            next.registration_epoch,
        );
        next.subscriptions.retain(|current| current.topic != topic);
        next.subscriptions.push(subscription.clone());
        self.inner.store.save_state(&next)?;
        {
            let mut state = lock(&self.inner.state);
            state.durable = next;
            state.revision = state.revision.saturating_add(1);
        }
        drop(mutation);
        self.inner.emit(vec![PushEvent::SubscriptionAdded {
            topic,
            owner: subscription_owner,
            registration_epoch: subscription.registration_epoch,
        }]);
        completed(subscription)
    }

    pub fn remove_subscription(
        &self,
        owner: crate::ExecutionOwner,
        context: &OperationContext,
        topic: &PushTopic,
    ) -> ServiceResult<bool> {
        if let Some(reason) = background_gate(owner, context)? {
            return cancelled(reason);
        }
        let mutation = self.inner.begin_mutation();
        let mut next = lock(&self.inner.state).durable.clone();
        let before = next.subscriptions.len();
        next.subscriptions
            .retain(|subscription| &subscription.topic != topic);
        let removed = next.subscriptions.len() != before;
        if removed {
            self.inner.store.save_state(&next)?;
            let registration_epoch = next.registration_epoch;
            {
                let mut state = lock(&self.inner.state);
                state.durable = next;
                state.revision = state.revision.saturating_add(1);
            }
            drop(mutation);
            self.inner.emit(vec![PushEvent::SubscriptionRemoved {
                topic: topic.clone(),
                registration_epoch,
            }]);
        }
        completed(removed)
    }

    // -----------------------------------------------------------------
    // Background-only credential lease
    // -----------------------------------------------------------------

    /// Read the active registration credentials for an authorized server-sync
    /// upload worker. Requires a background owner, rejects a tombstoned or
    /// logged-out account and verifies the active registration epoch.
    pub fn lease_registration_credentials(
        &self,
        owner: crate::ExecutionOwner,
        context: &OperationContext,
    ) -> ServiceResult<RegistrationCredentialLease> {
        if let Some(reason) = background_gate(owner, context)? {
            return cancelled(reason);
        }
        let (account, epoch, revision, fingerprint) = {
            let state = lock(&self.inner.state);
            let durable = &state.durable;
            if durable.token_revision == 0 {
                return Err(not_registered());
            }
            if let Some(account) = &durable.account
                && durable.is_tombstoned(account)
            {
                return Err(tombstone_error());
            }
            if durable
                .tombstones
                .iter()
                .any(|tombstone| tombstone.device_scope)
            {
                return Err(tombstone_error());
            }
            let Some(fingerprint) = durable.fingerprint.clone() else {
                return Err(error(
                    ErrorCode::IntegrityFailure,
                    "Push registration has no token fingerprint",
                ));
            };
            (
                durable.account.clone(),
                durable.registration_epoch,
                durable.token_revision,
                fingerprint,
            )
        };
        let lease = self
            .inner
            .vault
            .read_lease(account.as_ref(), epoch, revision, &fingerprint, context)
            .map_err(SecretVaultError::into_system_error)?
            .ok_or_else(|| {
                error(
                    ErrorCode::Unavailable,
                    "Push registration credentials are not available",
                )
            })?;
        completed(lease)
    }

    // -----------------------------------------------------------------
    // Stage 1: provider handoff
    // -----------------------------------------------------------------

    pub fn provider_handoff(
        &self,
        owner: crate::ExecutionOwner,
        context: &OperationContext,
        frame: IncomingPushFrame,
    ) -> ServiceResult<ProviderHandoff> {
        if let Some(reason) = background_gate(owner, context)? {
            return cancelled(reason);
        }
        let now = self.inner.now_ms();
        let discovered = self.inner.descriptors()?;
        let descriptor = discovered
            .into_iter()
            .find(|(descriptor, _)| descriptor.provider_id == frame.provider_id)
            .map(|(descriptor, _)| descriptor);
        let durable = lock(&self.inner.state).durable.clone();
        let outcome = Self::evaluate_frame(&frame, descriptor.as_ref(), &durable, now);
        if !outcome.is_accepted() {
            self.record_rejection(&frame, &outcome);
            return completed(ProviderHandoff::new(frame.message_id, frame.kind, outcome));
        }
        if let Some(reason) = context.cancellation.reason() {
            return cancelled(reason);
        }
        match self.commit_frame(&frame, now) {
            Ok(true) => {}
            Ok(false) => {
                let first_seen = lock(&self.inner.state)
                    .journal
                    .first_seen(&frame.message_id, now)
                    .unwrap_or(now);
                let duplicate = ProviderHandoffOutcome::DuplicateSuppressed {
                    first_seen_at_ms: first_seen,
                };
                self.record_rejection(&frame, &duplicate);
                return completed(ProviderHandoff::new(
                    frame.message_id,
                    frame.kind,
                    duplicate,
                ));
            }
            Err(failure) => return Err(failure),
        }
        lock(&self.inner.state).revision += 1;
        completed(ProviderHandoff::new(
            frame.message_id,
            frame.kind,
            ProviderHandoffOutcome::Accepted,
        ))
    }

    fn evaluate_frame(
        frame: &IncomingPushFrame,
        descriptor: Option<&PushProviderDescriptor>,
        durable: &PushDurableState,
        now: u64,
    ) -> ProviderHandoffOutcome {
        if frame.payload.len() > MAX_PUSH_PAYLOAD_BYTES {
            return ProviderHandoffOutcome::OversizePayload {
                bytes: frame.payload.len(),
                limit: MAX_PUSH_PAYLOAD_BYTES,
            };
        }
        if frame.expires_at_ms.is_some_and(|expiry| expiry <= now) {
            return ProviderHandoffOutcome::ExpiredDrop;
        }
        if durable.token_revision == 0
            || durable
                .tombstones
                .iter()
                .any(|tombstone| tombstone.device_scope)
        {
            return ProviderHandoffOutcome::DeviceNotRegistered;
        }
        let Some(descriptor) = descriptor else {
            return ProviderHandoffOutcome::UnknownProvider;
        };
        if descriptor.kind != frame.kind
            || frame
                .channel_id
                .as_ref()
                .is_some_and(|channel| descriptor.channel_id.as_ref() != Some(channel))
        {
            return ProviderHandoffOutcome::UnknownProvider;
        }
        // Frames from a provider the host has not selected for this device are
        // not ours, even when that provider is installed and healthy.
        if durable.provider_id.as_deref() != Some(frame.provider_id.as_str()) {
            return ProviderHandoffOutcome::UnknownProvider;
        }
        match (&durable.channel_id, &frame.channel_id) {
            (Some(registered), Some(frame_channel)) if registered != frame_channel => {
                return ProviderHandoffOutcome::UnknownProvider;
            }
            (Some(_), None) | (None, Some(_)) => {
                return ProviderHandoffOutcome::UnknownProvider;
            }
            _ => {}
        }
        if !descriptor.configuration.is_ready() || !descriptor.health.is_selectable() {
            return ProviderHandoffOutcome::ProviderNotReady {
                health: descriptor.health,
            };
        }
        if !descriptor.capabilities.supports_payload(frame.payload_kind) {
            return ProviderHandoffOutcome::UnsupportedPayloadKind;
        }
        if frame.payload_kind == PushPayloadKind::NotificationAndData
            && frame.notification.is_none()
        {
            return ProviderHandoffOutcome::MissingNotificationContent;
        }
        if let Some(account) = &frame.target_account {
            if durable.is_tombstoned(account) || durable.account.as_ref() != Some(account) {
                return ProviderHandoffOutcome::AccountMismatchOrLoggedOut;
            }
            match frame.registration_epoch {
                Some(epoch) if epoch == durable.registration_epoch => {}
                Some(epoch) => {
                    return ProviderHandoffOutcome::StaleRegistrationEpoch {
                        expected: durable.registration_epoch,
                        received: epoch,
                    };
                }
                None => {
                    return ProviderHandoffOutcome::StaleRegistrationEpoch {
                        expected: durable.registration_epoch,
                        received: 0,
                    };
                }
            }
        } else if let Some(epoch) = frame.registration_epoch
            && epoch != durable.registration_epoch
        {
            return ProviderHandoffOutcome::StaleRegistrationEpoch {
                expected: durable.registration_epoch,
                received: epoch,
            };
        }
        if let Some(route) = &frame.activation_route
            && validate_activation_route(route).is_err()
        {
            return ProviderHandoffOutcome::InvalidActivationRoute;
        }
        ProviderHandoffOutcome::Accepted
    }

    fn record_rejection(&self, frame: &IncomingPushFrame, outcome: &ProviderHandoffOutcome) {
        {
            let mut state = lock(&self.inner.state);
            match outcome {
                ProviderHandoffOutcome::DuplicateSuppressed { .. } => {
                    state.counters.duplicate_suppressed =
                        state.counters.duplicate_suppressed.saturating_add(1);
                }
                ProviderHandoffOutcome::ExpiredDrop => {
                    state.counters.expired_dropped =
                        state.counters.expired_dropped.saturating_add(1);
                }
                ProviderHandoffOutcome::OversizePayload { .. } => {
                    state.counters.oversize_rejected =
                        state.counters.oversize_rejected.saturating_add(1);
                }
                ProviderHandoffOutcome::AccountMismatchOrLoggedOut
                | ProviderHandoffOutcome::StaleRegistrationEpoch { .. }
                | ProviderHandoffOutcome::DeviceNotRegistered => {
                    state.counters.account_rejected =
                        state.counters.account_rejected.saturating_add(1);
                }
                ProviderHandoffOutcome::InvalidActivationRoute => {
                    state.counters.invalid_route_rejected =
                        state.counters.invalid_route_rejected.saturating_add(1);
                }
                _ => {}
            }
        }
        self.inner.emit(vec![PushEvent::ProviderHandoffRejected {
            message_id: frame.message_id.clone(),
            outcome: outcome.clone(),
        }]);
    }

    fn commit_frame(&self, frame: &IncomingPushFrame, now: u64) -> Result<bool, SystemError> {
        let _mutation = self.inner.begin_mutation();
        let mut journal = lock(&self.inner.state).journal.clone();
        if journal.first_seen(&frame.message_id, now).is_some() {
            return Ok(false);
        }
        journal.record(frame.message_id.clone(), now);
        self.inner.store.save_journal(&journal)?;
        let mut state = lock(&self.inner.state);
        state.journal = journal;
        let registration_epoch = if let Some(epoch) = frame.registration_epoch {
            epoch
        } else if let Some(account) = &frame.target_account {
            if state.durable.account.as_ref() == Some(account) {
                state.durable.registration_epoch
            } else {
                return Ok(false);
            }
        } else {
            state.durable.registration_epoch
        };
        let accepted = AcceptedMessage {
            message_id: frame.message_id.clone(),
            kind: frame.kind,
            condition: frame.condition,
            payload_kind: frame.payload_kind,
            account: frame.target_account.clone(),
            registration_epoch,
            notification: frame.notification.clone(),
            activation_route: frame.activation_route.clone(),
            received_at_ms: now,
            payload_bytes: frame.payload.len(),
            presented: false,
        };
        state.accepted.insert(frame.message_id.clone(), accepted);
        state.accepted_order.push_back(frame.message_id.clone());
        while state.accepted_order.len() > self.inner.config.recent_message_capacity {
            if let Some(expired) = state.accepted_order.pop_front() {
                state.accepted.remove(&expired);
            }
        }
        Ok(true)
    }

    // -----------------------------------------------------------------
    // Stage 2: client receipt
    // -----------------------------------------------------------------

    pub fn accept_message(
        &self,
        owner: crate::ExecutionOwner,
        context: &OperationContext,
        frame: IncomingPushFrame,
    ) -> ServiceResult<PushPipelineOutcome> {
        let handoff = match self.provider_handoff(owner, context, frame)? {
            Completion::Cancelled(reason) => return cancelled(reason),
            Completion::Completed(handoff) => handoff,
        };
        if !handoff.is_accepted() {
            return completed(PushPipelineOutcome {
                handoff,
                receipt: None,
                presentation: None,
            });
        }
        let receipt = self.build_receipt(&handoff.message_id)?;
        completed(PushPipelineOutcome {
            handoff,
            receipt: Some(receipt),
            presentation: None,
        })
    }

    /// Stages 1-3. Stage 4 is never triggered implicitly.
    pub fn deliver(
        &self,
        owner: crate::ExecutionOwner,
        context: &OperationContext,
        frame: IncomingPushFrame,
    ) -> ServiceResult<PushPipelineOutcome> {
        let message_id = frame.message_id.clone();
        let outcome = match self.accept_message(owner, context, frame)? {
            Completion::Cancelled(reason) => return cancelled(reason),
            Completion::Completed(outcome) => outcome,
        };
        if outcome.receipt.is_none() {
            return completed(outcome);
        }
        let presentation = match self.present_message(owner, context, &message_id)? {
            Completion::Cancelled(reason) => return cancelled(reason),
            Completion::Completed(presentation) => presentation,
        };
        completed(PushPipelineOutcome {
            handoff: outcome.handoff,
            receipt: outcome.receipt,
            presentation: Some(presentation),
        })
    }

    /// Alias for [`Self::deliver`] used by host wake handlers.
    pub fn receive_push(
        &self,
        owner: crate::ExecutionOwner,
        context: &OperationContext,
        frame: IncomingPushFrame,
    ) -> ServiceResult<PushPipelineOutcome> {
        self.deliver(owner, context, frame)
    }

    fn build_receipt(&self, message_id: &PushMessageId) -> Result<MessageReceipt, SystemError> {
        let (receipt, queued) = {
            let mut state = lock(&self.inner.state);
            let Some(accepted) = state.accepted.get(message_id).cloned() else {
                return Err(error(
                    ErrorCode::IntegrityFailure,
                    "Accepted push message is missing from the retained set",
                ));
            };
            let needs_work =
                accepted.payload_kind.is_silent() || accepted.condition.requires_background_work();
            let handoff = if needs_work {
                let class = if accepted.payload_kind.is_silent() {
                    BackgroundWorkClass::SilentData
                } else {
                    BackgroundWorkClass::NotificationData
                };
                let descriptor = BackgroundWorkHandoff {
                    message_id: accepted.message_id.clone(),
                    account: accepted.account.clone(),
                    registration_epoch: accepted.registration_epoch,
                    class,
                    recommendation: BackgroundWorkRecommendation::RunSoon,
                    budget: self.inner.config.background_budget,
                    payload_bytes: accepted.payload_bytes,
                };
                let capacity = self.inner.config.max_pending_background_handoffs;
                let mut dropped = 0usize;
                while state.background.len() >= capacity {
                    if state.background.pop_front().is_some() {
                        dropped = dropped.saturating_add(1);
                    } else {
                        break;
                    }
                }
                if dropped > 0 {
                    state.counters.background_handoffs_dropped = state
                        .counters
                        .background_handoffs_dropped
                        .saturating_add(u64::try_from(dropped).unwrap_or(u64::MAX));
                }
                state.background.push_back(descriptor.clone());
                state.counters.background_handoffs_queued =
                    state.counters.background_handoffs_queued.saturating_add(1);
                Some(descriptor)
            } else {
                None
            };
            let depth = state.background.len();
            let receipt = MessageReceipt {
                message_id: accepted.message_id,
                kind: accepted.kind,
                condition: accepted.condition,
                payload_kind: accepted.payload_kind,
                account: accepted.account,
                registration_epoch: accepted.registration_epoch,
                received_at_ms: accepted.received_at_ms,
                payload_bytes: accepted.payload_bytes,
                background_handoff: handoff.clone(),
            };
            (receipt, handoff.map(|handoff| (handoff.class, depth)))
        };
        if let Some((class, depth)) = queued {
            self.inner.emit(vec![PushEvent::BackgroundWorkQueued {
                message_id: message_id.clone(),
                class,
                depth,
            }]);
        }
        self.inner.emit(vec![PushEvent::MessageAccepted {
            message_id: message_id.clone(),
            condition: receipt.condition,
            payload_kind: receipt.payload_kind,
            registration_epoch: receipt.registration_epoch,
        }]);
        Ok(receipt)
    }

    // -----------------------------------------------------------------
    // Stage 3: local presentation
    // -----------------------------------------------------------------

    pub fn present_message(
        &self,
        owner: crate::ExecutionOwner,
        context: &OperationContext,
        message_id: &PushMessageId,
    ) -> ServiceResult<LocalPresentationOutcome> {
        if let Some(reason) = background_gate(owner, context)? {
            return cancelled(reason);
        }
        let (durable, accepted) = {
            let state = lock(&self.inner.state);
            (
                state.durable.clone(),
                state.accepted.get(message_id).cloned(),
            )
        };
        let Some(accepted) = accepted else {
            return completed(LocalPresentationOutcome::NotAcceptedMessage);
        };
        // A delayed presentation after logout, account switch or unregistration
        // must never surface a stale account's notification.
        let registration_changed = durable.token_revision == 0
            || durable
                .tombstones
                .iter()
                .any(|tombstone| tombstone.device_scope)
            || accepted.registration_epoch != durable.registration_epoch
            || match (&accepted.account, &durable.account) {
                (Some(accepted_account), Some(account)) => {
                    accepted_account != account || durable.is_tombstoned(account)
                }
                (None, None) => false,
                _ => true,
            };
        if registration_changed {
            let outcome = LocalPresentationOutcome::SuppressedRegistrationChanged;
            self.finish_presentation(message_id, outcome.clone());
            return completed(outcome);
        }
        if accepted.payload_kind.is_silent() {
            let outcome = LocalPresentationOutcome::NotApplicableSilentPush;
            self.finish_presentation(message_id, outcome.clone());
            return completed(outcome);
        }
        if accepted.condition == AppDeliveryCondition::ForegroundActive {
            match self.inner.config.presentation {
                ForegroundPresentationPolicy::SuppressInForeground => {
                    let outcome = LocalPresentationOutcome::SuppressedForegroundPolicy;
                    self.finish_presentation(message_id, outcome.clone());
                    return completed(outcome);
                }
                ForegroundPresentationPolicy::DelegateToHost => {
                    let outcome = LocalPresentationOutcome::DelegatedToHost;
                    self.finish_presentation(message_id, outcome.clone());
                    return completed(outcome);
                }
                ForegroundPresentationPolicy::PresentBanner => {}
            }
        }
        let status = self.inner.runtime.permissions.notification_status();
        if !status.allows_use() {
            let outcome = LocalPresentationOutcome::SuppressedPermissionDenied { status };
            self.finish_presentation(message_id, outcome.clone());
            return completed(outcome);
        }
        if let Some(reason) = context.cancellation.reason() {
            return cancelled(reason);
        }
        let Some(notification) = accepted.notification.as_ref() else {
            let outcome = LocalPresentationOutcome::PresenterError {
                code: ErrorCode::InvalidInput,
            };
            self.finish_presentation(message_id, outcome.clone());
            return completed(outcome);
        };
        let local = LocalNotification {
            message_id: accepted.message_id.clone(),
            title: notification.title().to_owned(),
            body: notification.body().to_owned(),
            account: accepted.account.clone(),
        };
        let outcome = match self.inner.runtime.presenter.present(&local) {
            Ok(LocalPresentationReceipt::Presented { notification_id }) => {
                LocalPresentationOutcome::Presented { notification_id }
            }
            Ok(LocalPresentationReceipt::DelegatedToHost) => {
                LocalPresentationOutcome::DelegatedToHost
            }
            Err(failure) => LocalPresentationOutcome::PresenterError { code: failure.code },
        };
        self.finish_presentation(message_id, outcome.clone());
        completed(outcome)
    }

    fn finish_presentation(&self, message_id: &PushMessageId, outcome: LocalPresentationOutcome) {
        {
            let mut state = lock(&self.inner.state);
            if let Some(accepted) = state.accepted.get_mut(message_id) {
                accepted.presented = outcome.was_presented();
            }
            match &outcome {
                LocalPresentationOutcome::Presented { .. } => {
                    state.counters.presented = state.counters.presented.saturating_add(1);
                }
                LocalPresentationOutcome::SuppressedPermissionDenied { .. }
                | LocalPresentationOutcome::SuppressedForegroundPolicy => {
                    state.counters.presentations_suppressed =
                        state.counters.presentations_suppressed.saturating_add(1);
                }
                _ => {}
            }
        }
        self.inner.emit(vec![PushEvent::LocalPresentation {
            message_id: message_id.clone(),
            outcome,
        }]);
    }

    // -----------------------------------------------------------------
    // Stage 4: user activation
    // -----------------------------------------------------------------

    pub fn activate_notification(
        &self,
        owner: crate::ExecutionOwner,
        context: &OperationContext,
        request: PushActivationRequest,
    ) -> ServiceResult<PushActivationOutcome> {
        self.route_push_activation(owner, context, request)
    }

    pub fn route_push_activation(
        &self,
        owner: crate::ExecutionOwner,
        context: &OperationContext,
        request: PushActivationRequest,
    ) -> ServiceResult<PushActivationOutcome> {
        if let Some(reason) = background_gate(owner, context)? {
            return cancelled(reason);
        }
        let (durable, accepted, ui_attached) = {
            let state = lock(&self.inner.state);
            (
                state.durable.clone(),
                state.accepted.get(&request.message_id).cloned(),
                state.ui_attached,
            )
        };
        if durable.token_revision == 0 {
            return self.reject_activation(&request, PushActivationOutcome::RejectedNotRegistered);
        }
        if durable
            .tombstones
            .iter()
            .any(|tombstone| tombstone.device_scope)
        {
            return self
                .reject_activation(&request, PushActivationOutcome::RejectedAccountMismatch);
        }
        let route = request.route.clone().or_else(|| {
            accepted
                .as_ref()
                .and_then(|accepted| accepted.activation_route.clone())
        });
        let Some(route) = route else {
            return self.reject_activation(&request, PushActivationOutcome::RejectedMissingRoute);
        };
        if validate_activation_route(&route).is_err() {
            return self.reject_activation(&request, PushActivationOutcome::RejectedInvalidRoute);
        }
        if let Some(account) = &request.account
            && (durable.is_tombstoned(account) || durable.account.as_ref() != Some(account))
        {
            return self
                .reject_activation(&request, PushActivationOutcome::RejectedAccountMismatch);
        }
        if let Some(expected) = request.registration_epoch
            && expected != durable.registration_epoch
        {
            return self.reject_activation(
                &request,
                PushActivationOutcome::RejectedStaleEpoch {
                    expected: durable.registration_epoch,
                    received: expected,
                },
            );
        }
        if let Some(accepted) = &accepted {
            if let (Some(request_account), Some(accepted_account)) =
                (&request.account, &accepted.account)
                && request_account != accepted_account
            {
                return self
                    .reject_activation(&request, PushActivationOutcome::RejectedAccountMismatch);
            }
            if let Some(accepted_account) = &accepted.account
                && (durable.is_tombstoned(accepted_account)
                    || durable.account.as_ref() != Some(accepted_account))
            {
                return self
                    .reject_activation(&request, PushActivationOutcome::RejectedAccountMismatch);
            }
            if let Some(request_epoch) = request.registration_epoch
                && request_epoch != accepted.registration_epoch
            {
                return self.reject_activation(
                    &request,
                    PushActivationOutcome::RejectedStaleEpoch {
                        expected: accepted.registration_epoch,
                        received: request_epoch,
                    },
                );
            }
            if accepted.registration_epoch != durable.registration_epoch {
                return self.reject_activation(
                    &request,
                    PushActivationOutcome::RejectedStaleEpoch {
                        expected: durable.registration_epoch,
                        received: accepted.registration_epoch,
                    },
                );
            }
            if accepted.payload_kind.is_silent() {
                return self
                    .reject_activation(&request, PushActivationOutcome::RejectedSilentMessage);
            }
        }
        if accepted.is_none() && !request.condition.is_cold_launch() {
            return self.reject_activation(&request, PushActivationOutcome::RejectedUnknownMessage);
        }
        let already_activated = {
            let state = lock(&self.inner.state);
            state.activated.contains(&request.message_id)
        };
        if already_activated {
            return self
                .reject_activation(&request, PushActivationOutcome::RejectedDuplicateActivation);
        }
        let registration_epoch = request
            .registration_epoch
            .unwrap_or(durable.registration_epoch);
        let account = request.account.clone().or(durable.account.clone());
        if request.condition.is_cold_launch() && !ui_attached {
            return self.queue_cold_activation(request, route, account, registration_epoch);
        }
        let envelope = PushActivationEnvelope {
            message_id: request.message_id.clone(),
            route,
            account,
            registration_epoch,
            condition: request.condition,
        };
        match self.inner.runtime.activation.handoff(&envelope) {
            Ok(()) => {
                self.mark_activated(&request.message_id);
                {
                    let mut state = lock(&self.inner.state);
                    state.counters.activations_routed =
                        state.counters.activations_routed.saturating_add(1);
                }
                self.inner.emit(vec![PushEvent::ActivationRouted {
                    message_id: request.message_id.clone(),
                    registration_epoch,
                }]);
                completed(PushActivationOutcome::Routed)
            }
            Err(failure) => {
                let outcome = PushActivationOutcome::HandoffFailed { code: failure.code };
                self.inner.emit(vec![PushEvent::ActivationRejected {
                    message_id: request.message_id.clone(),
                    outcome: outcome.clone(),
                }]);
                completed(outcome)
            }
        }
    }

    fn rejected_activation_event(
        &self,
        message_id: &PushMessageId,
        outcome: PushActivationOutcome,
    ) -> PushEvent {
        PushEvent::ActivationRejected {
            message_id: message_id.clone(),
            outcome,
        }
    }

    fn reject_activation(
        &self,
        request: &PushActivationRequest,
        outcome: PushActivationOutcome,
    ) -> ServiceResult<PushActivationOutcome> {
        self.inner.emit(vec![PushEvent::ActivationRejected {
            message_id: request.message_id.clone(),
            outcome: outcome.clone(),
        }]);
        completed(outcome)
    }

    fn queue_cold_activation(
        &self,
        request: PushActivationRequest,
        route: String,
        account: Option<PushAccountId>,
        registration_epoch: u64,
    ) -> ServiceResult<PushActivationOutcome> {
        let mutation = self.inner.begin_mutation();
        let mut next = lock(&self.inner.state).durable.clone();
        if next.token_revision == 0 {
            drop(mutation);
            return self.reject_activation(&request, PushActivationOutcome::RejectedNotRegistered);
        }
        if next
            .tombstones
            .iter()
            .any(|tombstone| tombstone.device_scope)
        {
            drop(mutation);
            return self
                .reject_activation(&request, PushActivationOutcome::RejectedAccountMismatch);
        }
        if next
            .pending_cold_activations
            .iter()
            .any(|pending| pending.message_id == request.message_id)
        {
            drop(mutation);
            return self
                .reject_activation(&request, PushActivationOutcome::RejectedDuplicateActivation);
        }
        let sequence = next.next_activation_sequence;
        next.next_activation_sequence = sequence.saturating_add(1);
        let capacity = self.inner.config.max_pending_cold_activations;
        while next.pending_cold_activations.len() >= capacity
            && !next.pending_cold_activations.is_empty()
        {
            next.pending_cold_activations.remove(0);
        }
        next.pending_cold_activations.push(PendingActivation {
            sequence,
            message_id: request.message_id.clone(),
            route,
            account,
            registration_epoch,
            condition: request.condition,
            queued_at_ms: self.inner.now_ms(),
        });
        self.inner.store.save_state(&next)?;
        let depth = next.pending_cold_activations.len();
        {
            let mut state = lock(&self.inner.state);
            state.durable = next;
            state.counters.activations_queued = state.counters.activations_queued.saturating_add(1);
            state.revision = state.revision.saturating_add(1);
        }
        drop(mutation);
        self.inner.emit(vec![PushEvent::ActivationQueued {
            message_id: request.message_id,
            depth,
        }]);
        completed(PushActivationOutcome::QueuedForColdLaunch { depth })
    }

    fn mark_activated(&self, message_id: &PushMessageId) {
        let capacity = self.inner.config.recent_message_capacity;
        let mut state = lock(&self.inner.state);
        if state.activated.insert(message_id.clone()) {
            state.activated_order.push_back(message_id.clone());
            while state.activated_order.len() > capacity {
                if let Some(expired) = state.activated_order.pop_front() {
                    state.activated.remove(&expired);
                }
            }
        }
    }
}

fn state_is_tombstoned(state: &Mutex<ServiceState>, account: &PushAccountId) -> bool {
    lock(state).durable.is_tombstoned(account)
}

impl fmt::Debug for PushService {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let state = lock(&self.inner.state);
        formatter
            .debug_struct("PushService")
            .field("platform", &self.inner.config.platform)
            .field("providers", &self.inner.providers.len())
            .field("registered", &(state.durable.token_revision > 0))
            .field("token_revision", &state.durable.token_revision)
            .field("registration_epoch", &state.durable.registration_epoch)
            .field("subscriptions", &state.durable.subscriptions.len())
            .field("journal_entries", &state.journal.len())
            .field("revision", &state.revision)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn secrets() -> Secrets {
        Secrets::with_provider(
            "gpui-push-unit",
            Arc::new(crate::secrets::MemoryCredentials::new()),
        )
        .expect("secrets")
    }

    #[test]
    fn configuration_bounds_are_enforced() {
        let config = PushConfig {
            event_capacity: 0,
            ..PushConfig::default()
        };
        assert!(config.validate().is_err());
        let config = PushConfig {
            dedup_capacity: 0,
            ..PushConfig::default()
        };
        assert!(config.validate().is_err());
        assert!(PushConfig::default().validate().is_ok());
    }

    #[test]
    fn selected_provider_fails_closed_without_a_policy() {
        use crate::provider::FakePushProvider;
        let service = PushService::in_memory(
            vec![
                Arc::new(FakePushProvider::ready(PushProviderKind::Fcm, "fcm-a")),
                Arc::new(FakePushProvider::ready(PushProviderKind::Apns, "apns-b")),
            ],
            secrets(),
            PushConfig::default(),
            PushRuntime::default(),
        )
        .expect("service");
        let context = OperationContext::default();
        let failure = match service.select_provider(crate::ExecutionOwner::Background, &context) {
            Err(failure) => failure,
            Ok(_) => panic!("ambiguous selection must fail closed"),
        };
        assert_eq!(failure.code, ErrorCode::InvalidConfiguration);
    }

    #[test]
    fn execution_owner_is_rejected_before_side_effects() {
        use crate::provider::FakePushProvider;
        let service = PushService::in_memory(
            vec![Arc::new(FakePushProvider::ready(
                PushProviderKind::Fcm,
                "fcm-a",
            ))],
            secrets(),
            PushConfig::default(),
            PushRuntime::default(),
        )
        .expect("service");
        let context = OperationContext::default();
        for owner in [crate::ExecutionOwner::Ui, crate::ExecutionOwner::Renderer] {
            assert_eq!(
                service
                    .register_device(owner, &context)
                    .expect_err("owner rejection")
                    .code,
                ErrorCode::InvalidInput
            );
        }
        assert_eq!(service.registration().token_revision, 0);
    }
}
