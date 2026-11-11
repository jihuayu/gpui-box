//! Authentication service: browser ceremonies, cold/warm redirects, token
//! exchange, multi-account sessions, refresh/logout and Passkey flows.
//!
//! Provider, backend, verifier, presence-gate and secret-vault calls always run
//! outside internal locks. A cancelled, expired or disposed ceremony discards
//! late tokens and never publishes a phantom session.

use crate::browser::{
    BrowserAuthHandle, BrowserAuthProvider, BrowserAuthRequest, BrowserLaunchRequest,
    FakeBrowserAuthProvider, FakeTokenExchangeBackend, TokenExchangeBackend, TokenExchangeRequest,
    TokenRefreshRequest, TokenRevocationRequest, authorize_url,
};
use crate::model::{
    AccountId, AccountLabel, AccountSessionState, AccountSessionSummary, AuthError, AuthMethodKind,
    AuthScope, AuthSnapshot, CeremonyId, ExecutionOwner, IssuedAccountTokens, LocalPresencePurpose,
    LocalPresenceReceipt, LocalUserPresenceGate, OAuthState, OidcNonce, PresentationContext,
    ProviderId, SecretTokenMaterial, TokenLease,
};
use crate::passkey::{
    AssertionVerificationRequest, PasskeyAssertionRequest, PasskeyAssertionResult,
    PasskeyCreationRequest, PasskeyProvider, PasskeyRegistrationResult, PasskeyServerVerifier,
    ProviderAssertionRequest, ProviderCreationRequest, RegistrationVerificationRequest,
    VerifiedAssertion, VerifiedRegistration, check_provider_assertion, check_provider_registration,
};
use crate::platform::{AuthOperation, AuthPlatform, SupportLevel, support};
use crate::redirect::{
    CallbackQueryKind, ColdRedirectDescriptor, RedirectCaptureOutcome, RedirectTarget,
    classify_callback_query,
};
use crate::vault::{ColdCeremonyStore, SecretTokenVault};
use crate::{CallbackUrl, MAX_ACCOUNTS, MAX_ACTIVE_CEREMONIES, UrlError};
use gpui_activation::{ActivationEvent, ActivationPayload, ActivationSource};
use gpui_secrets::Secrets;
use gpui_system::{CancelReason, ErrorCode, OperationContext, SurfaceId};
use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    fmt,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

/// Default bounded authentication event capacity.
pub const DEFAULT_AUTH_EVENT_CAPACITY: usize = 64;
/// Maximum number of consumed states retained for replay classification.
pub const MAX_CONSUMED_STATES: usize = 256;

/// Service limits and optional local-presence policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AuthConfig {
    pub platform: AuthPlatform,
    pub max_accounts: usize,
    pub max_ceremonies: usize,
    pub event_capacity: usize,
    pub cold_redirects: bool,
    pub require_local_presence_for_sensitive_actions: bool,
}

impl Default for AuthConfig {
    fn default() -> Self {
        Self {
            platform: AuthPlatform::Linux,
            max_accounts: MAX_ACCOUNTS,
            max_ceremonies: MAX_ACTIVE_CEREMONIES,
            event_capacity: DEFAULT_AUTH_EVENT_CAPACITY,
            cold_redirects: true,
            require_local_presence_for_sensitive_actions: false,
        }
    }
}

/// Explicit provider bundle. No native implementation is selected implicitly.
pub struct AuthServiceProviders {
    pub browser: Arc<dyn BrowserAuthProvider>,
    pub token_exchange: Arc<dyn TokenExchangeBackend>,
    pub passkey: Arc<dyn PasskeyProvider>,
    pub passkey_verifier: Arc<dyn PasskeyServerVerifier>,
    pub local_presence: Option<Arc<dyn LocalUserPresenceGate>>,
    pub secrets: Secrets,
}

/// Public-safe authentication events. No token, code, verifier or raw
/// authenticator payload is ever included.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuthEvent {
    CeremonyLaunched {
        ceremony_id: CeremonyId,
        method: AuthMethodKind,
        surface: SurfaceId,
        generation: u64,
    },
    CeremonyCancelled {
        ceremony_id: CeremonyId,
        reason: CancelReason,
    },
    CeremonyExpired {
        ceremony_id: CeremonyId,
    },
    RedirectCaptured {
        ceremony_id: CeremonyId,
        outcome: RedirectCaptureOutcome,
    },
    AccountAuthenticated {
        account_id: AccountId,
        method: AuthMethodKind,
    },
    AccountRefreshed {
        account_id: AccountId,
    },
    AccountReauthRequired {
        account_id: AccountId,
    },
    AccountLoggedOut {
        account_id: AccountId,
    },
    PrimaryAccountChanged {
        account_id: Option<AccountId>,
    },
    PasskeyRegistered {
        account_id: Option<AccountId>,
        credential_id: crate::CredentialId,
    },
    PasskeyAsserted {
        account_id: AccountId,
        credential_id: crate::CredentialId,
    },
    OperationFailed {
        operation: AuthOperation,
        code: ErrorCode,
    },
}

/// Cursor into the bounded authentication event ring.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct AuthEventCursor(u64);

impl AuthEventCursor {
    /// Cursor before the first event.
    pub const fn start() -> Self {
        Self(0)
    }

    pub const fn position(self) -> u64 {
        self.0
    }
}

/// A resynchronization marker emitted when events older than the cursor were
/// dropped by the bounded ring.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResyncRequired {
    pub dropped_events: u64,
}

/// One non-blocking drain of the event ring.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthEventBatch {
    pub events: Vec<AuthEvent>,
    pub next_cursor: AuthEventCursor,
    pub resync: Option<ResyncRequired>,
}

impl AuthEventBatch {
    pub fn is_empty(&self) -> bool {
        self.events.is_empty() && self.resync.is_none()
    }
}

struct EventRing {
    capacity: usize,
    next_sequence: u64,
    entries: VecDeque<(u64, AuthEvent)>,
}

impl EventRing {
    fn new(capacity: usize) -> Self {
        Self {
            capacity: capacity.max(1),
            next_sequence: 1,
            entries: VecDeque::new(),
        }
    }

    fn push(&mut self, event: AuthEvent) {
        let sequence = self.next_sequence;
        self.next_sequence = self.next_sequence.saturating_add(1);
        if self.entries.len() == self.capacity {
            self.entries.pop_front();
        }
        self.entries.push_back((sequence, event));
    }

    fn since(&self, cursor: AuthEventCursor) -> AuthEventBatch {
        let oldest = self.entries.front().map(|(sequence, _)| *sequence);
        let last_sequence = self.next_sequence.saturating_sub(1);
        let mut resync = None;
        let first_available = oldest.unwrap_or(last_sequence.saturating_add(1));
        if cursor.0 < first_available.saturating_sub(1) {
            let dropped_events = first_available.saturating_sub(cursor.0 + 1);
            resync = Some(ResyncRequired { dropped_events });
        }
        let events = self
            .entries
            .iter()
            .filter(|(sequence, _)| *sequence > cursor.0)
            .map(|(_, event)| event.clone())
            .collect::<Vec<_>>();
        let next = events
            .last()
            .map(|_| last_sequence)
            .unwrap_or_else(|| last_sequence.max(cursor.0));
        let next_cursor = if resync.is_some() {
            AuthEventCursor(last_sequence)
        } else {
            AuthEventCursor(next)
        };
        AuthEventBatch {
            events,
            next_cursor,
            resync,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BrowserStatus {
    Prepared,
    Launched { session_id: u64 },
    ReadyForExchange,
    Exchanging,
    Committing,
}

struct BrowserCeremony {
    id: CeremonyId,
    provider_id: ProviderId,
    state: OAuthState,
    nonce: Option<OidcNonce>,
    scopes: Vec<AuthScope>,
    expected_account: Option<AccountId>,
    presentation: PresentationContext,
    preference: crate::BrowserSessionPreference,
    redirect: RedirectTarget,
    authorize_url: String,
    verifier: Option<crate::PkceVerifier>,
    code: Option<SecretTokenMaterial>,
    expires_at: Instant,
    status: BrowserStatus,
    cold: bool,
}

impl BrowserCeremony {
    fn session_id(&self) -> Option<u64> {
        match self.status {
            BrowserStatus::Launched { session_id } => Some(session_id),
            _ => None,
        }
    }
}

enum PendingVerification {
    Registration(RegistrationVerificationRequest),
    Assertion(AssertionVerificationRequest),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PasskeyStatus {
    InProvider,
    Ready,
    Verifying,
    Committing,
}

struct PasskeyCeremony {
    presentation: PresentationContext,
    expires_at: Instant,
    status: PasskeyStatus,
    expected_account: Option<AccountId>,
    verification: Option<PendingVerification>,
}

struct AccountRecord {
    provider_id: ProviderId,
    label: AccountLabel,
    scopes: Vec<AuthScope>,
    state: AccountSessionState,
    authenticated_at: Instant,
    access_expires_at: Option<Instant>,
    refresh_expires_at: Option<Instant>,
    has_refresh_token: bool,
    auth_method: AuthMethodKind,
}

struct ServiceState {
    platform: AuthPlatform,
    max_ceremonies: usize,
    accounts: BTreeMap<AccountId, AccountRecord>,
    epochs: BTreeMap<AccountId, u64>,
    primary: Option<AccountId>,
    ceremonies: BTreeMap<CeremonyId, BrowserCeremony>,
    passkey_ceremonies: BTreeMap<CeremonyId, PasskeyCeremony>,
    refreshing: BTreeSet<AccountId>,
    used_states: VecDeque<[u8; 32]>,
    used_challenges: VecDeque<[u8; 32]>,
    pending_cold_cleanup: BTreeSet<CeremonyId>,
    next_ceremony: u64,
    revision: u64,
}

impl ServiceState {
    fn next_ceremony_id(&mut self) -> Result<CeremonyId, AuthError> {
        for _ in 0..=self.max_ceremonies.saturating_add(1) {
            let value = self.next_ceremony.max(1);
            self.next_ceremony = value.saturating_add(1);
            if let Ok(id) = CeremonyId::new(value)
                && !self.ceremonies.contains_key(&id)
                && !self.passkey_ceremonies.contains_key(&id)
            {
                return Ok(id);
            }
        }
        Err(AuthError::CeremonyLimit)
    }

    fn has_interactive_ceremony(&self, presentation: PresentationContext) -> bool {
        self.ceremonies.values().any(|ceremony| {
            ceremony.presentation == presentation
                && !matches!(
                    ceremony.status,
                    BrowserStatus::Exchanging | BrowserStatus::Committing
                )
        }) || self
            .passkey_ceremonies
            .values()
            .any(|ceremony| ceremony.presentation == presentation)
    }

    fn account_summary(&self, account_id: &AccountId) -> Option<AccountSessionSummary> {
        let record = self.accounts.get(account_id)?;
        Some(AccountSessionSummary::new(
            account_id.clone(),
            record.provider_id.clone(),
            record.label.clone(),
            record.scopes.clone(),
            record.state,
            record.authenticated_at,
            record.access_expires_at,
            record.refresh_expires_at,
            record.has_refresh_token,
            record.auth_method,
            self.primary.as_ref() == Some(account_id),
        ))
    }

    fn snapshot(&self) -> AuthSnapshot {
        let accounts = self
            .accounts
            .keys()
            .filter_map(|id| self.account_summary(id))
            .collect();
        AuthSnapshot {
            accounts,
            primary_account: self.primary.clone(),
            pending_ceremonies: self.ceremonies.len(),
            pending_passkey_ceremonies: self.passkey_ceremonies.len(),
            revision: self.revision,
        }
    }

    fn mark_account_epoch(&mut self, account_id: &AccountId) {
        let epoch = self.epochs.get(account_id).copied().unwrap_or(0);
        self.epochs
            .insert(account_id.clone(), epoch.saturating_add(1));
    }

    fn epoch(&self, account_id: &AccountId) -> u64 {
        self.epochs.get(account_id).copied().unwrap_or(0)
    }

    fn record_state(&mut self, state: &str) {
        let digest = crate::pkce::sha256(state.as_bytes());
        if self.used_states.len() == MAX_CONSUMED_STATES {
            self.used_states.pop_front();
        }
        self.used_states.push_back(digest);
    }

    fn state_was_consumed(&self, state: &str) -> bool {
        let digest = crate::pkce::sha256(state.as_bytes());
        self.used_states.contains(&digest)
    }

    fn state_is_available(&self, state: &crate::OAuthState) -> bool {
        !self.state_was_consumed(state.expose())
            && !self
                .ceremonies
                .values()
                .any(|ceremony| ceremony.state.matches(state.expose()))
    }

    fn consume_challenge(&mut self, challenge: &crate::PasskeyChallenge) -> bool {
        let digest = crate::pkce::sha256(challenge.expose());
        if self.used_challenges.contains(&digest) {
            return false;
        }
        if self.used_challenges.len() == MAX_CONSUMED_STATES {
            self.used_challenges.pop_front();
        }
        self.used_challenges.push_back(digest);
        true
    }
}

struct ServiceInner {
    config: AuthConfig,
    browser: Arc<dyn BrowserAuthProvider>,
    backend: Arc<dyn TokenExchangeBackend>,
    passkey: Arc<dyn PasskeyProvider>,
    verifier: Arc<dyn PasskeyServerVerifier>,
    presence: Option<Arc<dyn LocalUserPresenceGate>>,
    vault: SecretTokenVault,
    cold: ColdCeremonyStore,
    /// Serializes token-vault mutations (install, refresh, logout cleanup).
    /// Lock order is always `vault_gate` -> `state`; never the reverse.
    vault_gate: Mutex<()>,
    state: Mutex<ServiceState>,
    events: Mutex<EventRing>,
}

/// Clonable, `Send + Sync` authentication service.
#[derive(Clone)]
pub struct AuthService(Arc<ServiceInner>);

impl AuthService {
    pub fn new(providers: AuthServiceProviders, config: AuthConfig) -> Result<Self, AuthError> {
        if config.max_accounts == 0
            || config.max_accounts > MAX_ACCOUNTS
            || config.max_ceremonies == 0
            || config.max_ceremonies > MAX_ACTIVE_CEREMONIES
            || config.event_capacity == 0
            || config.event_capacity > 1024
        {
            return Err(AuthError::InvalidConfiguration(
                "Authentication limits are outside the supported bounds",
            ));
        }
        let vault = SecretTokenVault::new(providers.secrets);
        let cold = ColdCeremonyStore::new(vault.clone());
        Ok(Self(Arc::new(ServiceInner {
            config,
            browser: providers.browser,
            backend: providers.token_exchange,
            passkey: providers.passkey,
            verifier: providers.passkey_verifier,
            presence: providers.local_presence,
            vault,
            cold,
            vault_gate: Mutex::new(()),
            state: Mutex::new(ServiceState {
                platform: config.platform,
                max_ceremonies: config.max_ceremonies,
                accounts: BTreeMap::new(),
                epochs: BTreeMap::new(),
                primary: None,
                ceremonies: BTreeMap::new(),
                passkey_ceremonies: BTreeMap::new(),
                refreshing: BTreeSet::new(),
                used_states: VecDeque::new(),
                used_challenges: VecDeque::new(),
                pending_cold_cleanup: BTreeSet::new(),
                next_ceremony: 1,
                revision: 0,
            }),
            events: Mutex::new(EventRing::new(config.event_capacity)),
        })))
    }

    /// Build a service with deterministic fakes (useful for hosts and tests).
    pub fn with_fakes(
        platform: AuthPlatform,
        browser: FakeBrowserAuthProvider,
        backend: FakeTokenExchangeBackend,
        passkey: crate::FakePasskeyProvider,
        verifier: crate::FakePasskeyServerVerifier,
        secrets: Secrets,
    ) -> Result<Self, AuthError> {
        Self::new(
            AuthServiceProviders {
                browser: Arc::new(browser),
                token_exchange: Arc::new(backend),
                passkey: Arc::new(passkey),
                passkey_verifier: Arc::new(verifier),
                local_presence: None,
                secrets,
            },
            AuthConfig {
                platform,
                ..AuthConfig::default()
            },
        )
    }

    pub fn shares_state_with(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }

    pub fn platform(&self) -> AuthPlatform {
        self.0.config.platform
    }

    pub fn subscribe(&self) -> AuthEventCursor {
        AuthEventCursor::start()
    }

    pub fn events_since(&self, cursor: AuthEventCursor) -> AuthEventBatch {
        lock(&self.0.events).since(cursor)
    }

    /// Zero-secret snapshot. Reading advances access-token expiry states.
    pub fn snapshot(&self, owner: ExecutionOwner, now: Instant) -> AuthSnapshot {
        if owner != ExecutionOwner::Renderer {
            self.evaluate_expiries(owner, now);
        }
        lock(&self.0.state).snapshot()
    }

    pub fn account(&self, account_id: &AccountId, now: Instant) -> Option<AccountSessionSummary> {
        let _ = self.evaluate_expiries(ExecutionOwner::Ui, now);
        lock(&self.0.state).account_summary(account_id)
    }

    fn check_operation(&self, operation: AuthOperation) -> Result<(), AuthError> {
        match support(self.0.config.platform, operation) {
            SupportLevel::Unsupported => Err(AuthError::Unsupported(
                "Authentication operation is not supported on this platform",
            )),
            SupportLevel::Supported | SupportLevel::Conditional => Ok(()),
        }
    }

    fn check_redirect_support(&self, redirect: &RedirectTarget) -> Result<(), AuthError> {
        let operation = match redirect {
            RedirectTarget::Loopback { .. } => AuthOperation::LoopbackRedirect,
            RedirectTarget::CustomScheme { .. } => AuthOperation::CustomSchemeRedirect,
            RedirectTarget::UniversalLink { .. } => AuthOperation::UniversalLinkRedirect,
            RedirectTarget::WebSameOrigin { .. } => AuthOperation::WebSameOriginRedirect,
        };
        self.check_operation(operation)
    }

    fn emit(&self, event: AuthEvent) {
        lock(&self.0.events).push(event);
    }

    fn emit_failure(&self, operation: AuthOperation, error: &AuthError) {
        self.emit(AuthEvent::OperationFailed {
            operation,
            code: error.code(),
        });
    }

    fn fail<T>(&self, operation: AuthOperation, error: AuthError) -> Result<T, AuthError> {
        self.emit_failure(operation, &error);
        Err(error)
    }

    /// Prepare and immediately launch a warm browser ceremony from a UI owner.
    pub fn begin_browser_auth(
        &self,
        owner: ExecutionOwner,
        request: BrowserAuthRequest,
        now: Instant,
    ) -> Result<BrowserAuthHandle, AuthError> {
        self.require_interactive(owner, "launch a browser ceremony")?;
        self.check_operation(AuthOperation::BrowserOAuthSession)?;
        self.check_redirect_support(request.redirect())?;
        if request.preference() == crate::BrowserSessionPreference::Ephemeral {
            self.check_operation(AuthOperation::EphemeralBrowserSession)?;
        }
        if request.expected_account().is_some() {
            self.require_local_presence(LocalPresencePurpose::StepUpCeremony)?;
        }
        let handle = self.prepare_browser(request, now, false)?;
        self.launch_prepared_inner(owner, handle.ceremony_id(), now)
    }

    /// Persist a cold-resumable ceremony on the background owner. The UI then
    /// calls [`Self::launch_prepared_browser_auth`] to open the browser.
    pub fn begin_cold_resumable_browser_auth(
        &self,
        owner: ExecutionOwner,
        request: BrowserAuthRequest,
        now: Instant,
    ) -> Result<BrowserAuthHandle, AuthError> {
        owner.require_background("persist a cold browser ceremony")?;
        if !self.0.config.cold_redirects {
            return Err(AuthError::Unsupported(
                "Cold redirect resumption is disabled by configuration",
            ));
        }
        self.check_operation(AuthOperation::BrowserOAuthSession)?;
        self.check_redirect_support(request.redirect())?;
        if request.preference() == crate::BrowserSessionPreference::Ephemeral {
            self.check_operation(AuthOperation::EphemeralBrowserSession)?;
        }
        if request.expected_account().is_some() {
            self.require_local_presence(LocalPresencePurpose::StepUpCeremony)?;
        }
        let handle = self.prepare_browser(request, now, true)?;
        let (descriptor, verifier) = {
            let state = lock(&self.0.state);
            let Some(ceremony) = state.ceremonies.get(&handle.ceremony_id()) else {
                return Err(AuthError::StaleCeremony);
            };
            let descriptor = ColdRedirectDescriptor {
                ceremony_id: ceremony.id,
                redirect: ceremony.redirect.clone(),
                state: ceremony.state.clone(),
                nonce: ceremony.nonce.clone(),
                scopes: ceremony.scopes.clone(),
                presentation: ceremony.presentation,
                remaining_ttl: ceremony.expires_at.saturating_duration_since(now),
                expected_account: ceremony.expected_account.clone(),
                provider_id: ceremony.provider_id.clone(),
            };
            let verifier = ceremony
                .verifier
                .as_ref()
                .ok_or(AuthError::StaleCeremony)?
                .expose();
            let verifier = crate::PkceVerifier::new(verifier)
                .map_err(|_| AuthError::IntegrityFailure("Persisted verifier is malformed"))?;
            (descriptor, verifier)
        };
        let context = background_context();
        if let Err(error) = self.0.cold.persist(owner, &descriptor, &verifier, &context) {
            lock(&self.0.state).ceremonies.remove(&handle.ceremony_id());
            return Err(error);
        }
        Ok(handle)
    }

    fn prepare_browser(
        &self,
        request: BrowserAuthRequest,
        now: Instant,
        cold: bool,
    ) -> Result<BrowserAuthHandle, AuthError> {
        let presentation = request.presentation();
        if presentation.surface().0 == 0 || presentation.generation() == 0 {
            return Err(AuthError::InvalidInput(
                "Browser ceremony requires a live non-zero surface generation",
            ));
        }
        let expires_at = now
            .checked_add(request.ttl())
            .ok_or(AuthError::InvalidInput(
                "Browser ceremony expiry overflows the monotonic clock",
            ))?;
        if now >= expires_at {
            return Err(AuthError::Expired);
        }
        let resolved = self.0.browser.resolve_redirect(request.redirect())?;
        let challenge = request.pkce_challenge().clone();
        let authorize_url = authorize_url(&request, &resolved, &challenge)?;
        let provider_id = request.provider_id().clone();
        let oauth_state = request.state().clone();
        let nonce = request.nonce().cloned();
        let scopes = request.scopes().as_slice().to_vec();
        let expected_account = request.expected_account().cloned();
        let preference = request.preference();
        let (verifier, _) = request.take_pkce().into_parts();
        let ceremony_id = {
            let mut state = lock(&self.0.state);
            if state.ceremonies.len() + state.passkey_ceremonies.len()
                >= self.0.config.max_ceremonies
            {
                return Err(AuthError::CeremonyLimit);
            }
            if state.has_interactive_ceremony(presentation)
                || !state.state_is_available(&oauth_state)
            {
                return Err(AuthError::AlreadyActive);
            }
            let id = state.next_ceremony_id()?;
            state.ceremonies.insert(
                id,
                BrowserCeremony {
                    id,
                    provider_id,
                    state: oauth_state,
                    nonce,
                    scopes,
                    expected_account,
                    presentation,
                    preference,
                    redirect: resolved.clone(),
                    authorize_url: authorize_url.clone(),
                    verifier: Some(verifier),
                    code: None,
                    expires_at,
                    status: BrowserStatus::Prepared,
                    cold,
                },
            );
            state.revision = state.revision.saturating_add(1);
            id
        };
        Ok(BrowserAuthHandle::new(ceremony_id, authorize_url, resolved))
    }

    /// Launch a ceremony previously prepared on the background owner.
    pub fn launch_prepared_browser_auth(
        &self,
        owner: ExecutionOwner,
        ceremony_id: CeremonyId,
        now: Instant,
    ) -> Result<BrowserAuthHandle, AuthError> {
        self.require_interactive(owner, "launch a prepared browser ceremony")?;
        self.launch_prepared_inner(owner, ceremony_id, now)
    }

    fn launch_prepared_inner(
        &self,
        _owner: ExecutionOwner,
        ceremony_id: CeremonyId,
        now: Instant,
    ) -> Result<BrowserAuthHandle, AuthError> {
        let (authorize_url, redirect, preference, presentation) = {
            let mut state = lock(&self.0.state);
            let ceremony = state
                .ceremonies
                .get(&ceremony_id)
                .ok_or(AuthError::StaleCeremony)?;
            if ceremony.expires_at <= now {
                let cold = ceremony.cold;
                state.ceremonies.remove(&ceremony_id);
                state.revision = state.revision.saturating_add(1);
                drop(state);
                if cold {
                    self.delete_cold_record(ceremony_id);
                }
                return self.fail(AuthOperation::BrowserOAuthSession, AuthError::Expired);
            }
            if ceremony.status != BrowserStatus::Prepared {
                return Err(AuthError::Busy(
                    "Browser ceremony has already been launched or consumed",
                ));
            }
            if ceremony.authorize_url.is_empty() {
                return Err(AuthError::StaleCeremony);
            }
            (
                ceremony.authorize_url.clone(),
                ceremony.redirect.clone(),
                ceremony.preference,
                ceremony.presentation,
            )
        };
        let launch = BrowserLaunchRequest {
            ceremony_id,
            authorize_url: authorize_url.clone(),
            redirect: redirect.clone(),
            preference,
            presentation,
        };
        let session = match self.0.browser.launch(&launch, &OperationContext::default()) {
            Ok(session) => session,
            Err(error) => {
                let cold = {
                    let mut state = lock(&self.0.state);
                    let cold = state
                        .ceremonies
                        .remove(&ceremony_id)
                        .is_some_and(|ceremony| ceremony.cold);
                    state.revision = state.revision.saturating_add(1);
                    cold
                };
                if cold {
                    self.delete_cold_record(ceremony_id);
                }
                return self.fail(AuthOperation::BrowserOAuthSession, error);
            }
        };
        if session.resolved_redirect() != &redirect {
            let _ = self.0.browser.cancel(session.session_id());
            let cold = {
                let mut state = lock(&self.0.state);
                let cold = state
                    .ceremonies
                    .remove(&ceremony_id)
                    .is_some_and(|ceremony| ceremony.cold);
                state.revision = state.revision.saturating_add(1);
                cold
            };
            if cold {
                self.delete_cold_record(ceremony_id);
            }
            return self.fail(
                AuthOperation::BrowserOAuthSession,
                AuthError::ProviderFailure("Browser provider changed the resolved redirect target"),
            );
        }
        {
            let mut state = lock(&self.0.state);
            let Some(ceremony) = state.ceremonies.get_mut(&ceremony_id) else {
                drop(state);
                let _ = self.0.browser.cancel(session.session_id());
                return Err(AuthError::StaleCeremony);
            };
            if ceremony.status != BrowserStatus::Prepared {
                drop(state);
                let _ = self.0.browser.cancel(session.session_id());
                return Err(AuthError::StaleCeremony);
            }
            ceremony.status = BrowserStatus::Launched {
                session_id: session.session_id(),
            };
        }
        self.emit(AuthEvent::CeremonyLaunched {
            ceremony_id,
            method: AuthMethodKind::BrowserOAuth,
            surface: presentation.surface(),
            generation: presentation.generation(),
        });
        Ok(BrowserAuthHandle::new(ceremony_id, authorize_url, redirect))
    }

    /// Ingest a raw callback URI from a UI/background owner.
    pub fn submit_callback(
        &self,
        owner: ExecutionOwner,
        raw: &str,
        now: Instant,
    ) -> RedirectCaptureOutcome {
        match CallbackUrl::parse(raw) {
            Ok(callback) => self.capture_callback(owner, &callback, false, now),
            Err(UrlError::Fragment) => RedirectCaptureOutcome::RejectedMalformedCallback,
            Err(_) => RedirectCaptureOutcome::IgnoredUnmatchedUri,
        }
    }

    /// Ingest an activation event. `ColdLaunch` first hydrates persisted
    /// ceremonies; `WarmLaunch` matches the in-memory registry directly.
    pub fn ingest_activation(
        &self,
        owner: ExecutionOwner,
        event: &ActivationEvent,
        now: Instant,
    ) -> RedirectCaptureOutcome {
        let ActivationPayload::OpenUrl(url) = event.payload() else {
            return RedirectCaptureOutcome::IgnoredUnmatchedUri;
        };
        let cold = matches!(event.source(), ActivationSource::ColdLaunch);
        let callback = match CallbackUrl::parse(url.as_str()) {
            Ok(callback) => callback,
            Err(UrlError::Fragment) => {
                return RedirectCaptureOutcome::RejectedMalformedCallback;
            }
            Err(_) => return RedirectCaptureOutcome::IgnoredUnmatchedUri,
        };
        if cold && self.0.config.cold_redirects && owner.is_background() {
            let _ = self.hydrate_cold_ceremonies(owner, now);
        }
        self.capture_callback(owner, &callback, cold, now)
    }

    fn capture_callback(
        &self,
        owner: ExecutionOwner,
        callback: &CallbackUrl,
        cold_expected: bool,
        now: Instant,
    ) -> RedirectCaptureOutcome {
        let mut hydrated = false;
        loop {
            let candidate = {
                let state = lock(&self.0.state);
                let mut matched = None;
                let mut target_conflict = false;
                for ceremony in state.ceremonies.values() {
                    if !ceremony.redirect.matches_callback(callback) {
                        continue;
                    }
                    target_conflict = true;
                    if matches!(
                        ceremony.status,
                        BrowserStatus::ReadyForExchange
                            | BrowserStatus::Exchanging
                            | BrowserStatus::Committing
                    ) {
                        continue;
                    }
                    let Some(state_value) = callback.parameter("state") else {
                        continue;
                    };
                    if ceremony.state.matches(state_value) {
                        matched = Some(ceremony.id);
                        break;
                    }
                }
                (matched, target_conflict)
            };
            let (matched, target_conflict) = candidate;
            if let Some(ceremony_id) = matched {
                return self.consume_callback(owner, ceremony_id, callback, now);
            }
            if !hydrated && cold_expected && owner.is_background() && self.0.config.cold_redirects {
                hydrated = true;
                let _ = self.hydrate_cold_ceremonies(owner, now);
                continue;
            }
            if target_conflict {
                return RedirectCaptureOutcome::RejectedStaleOrUnknownState;
            }
            if callback
                .parameter("state")
                .is_some_and(|state| lock(&self.0.state).state_was_consumed(state))
            {
                return RedirectCaptureOutcome::RejectedStaleOrUnknownState;
            }
            if cold_expected {
                return RedirectCaptureOutcome::ColdCeremonyUnavailable;
            }
            return RedirectCaptureOutcome::IgnoredUnmatchedUri;
        }
    }

    fn consume_callback(
        &self,
        owner: ExecutionOwner,
        ceremony_id: CeremonyId,
        callback: &CallbackUrl,
        now: Instant,
    ) -> RedirectCaptureOutcome {
        let mut state = lock(&self.0.state);
        let Some(ceremony) = state.ceremonies.get_mut(&ceremony_id) else {
            return RedirectCaptureOutcome::RejectedStaleOrUnknownState;
        };
        if !ceremony.redirect.matches_callback(callback) {
            return RedirectCaptureOutcome::IgnoredUnmatchedUri;
        }
        let Some(callback_state) = callback.parameter("state") else {
            return RedirectCaptureOutcome::RejectedMalformedCallback;
        };
        if !ceremony.state.matches(callback_state) {
            return RedirectCaptureOutcome::RejectedStaleOrUnknownState;
        }
        if ceremony.expires_at <= now {
            state.record_state(callback_state);
            let removed = state.ceremonies.remove(&ceremony_id);
            state.revision = state.revision.saturating_add(1);
            drop(state);
            self.after_ceremony_removal(owner, removed);
            self.emit(AuthEvent::CeremonyExpired { ceremony_id });
            return RedirectCaptureOutcome::RejectedStaleOrUnknownState;
        }
        match classify_callback_query(callback) {
            CallbackQueryKind::OAuthError(code) => {
                state.record_state(callback_state);
                let removed = state.ceremonies.remove(&ceremony_id);
                state.revision = state.revision.saturating_add(1);
                drop(state);
                self.after_ceremony_removal(owner, removed);
                if code.is_user_cancellation() {
                    self.emit(AuthEvent::CeremonyCancelled {
                        ceremony_id,
                        reason: CancelReason::User,
                    });
                    RedirectCaptureOutcome::CeremonyCancelled {
                        ceremony_id,
                        reason: CancelReason::User,
                    }
                } else {
                    self.emit(AuthEvent::CeremonyCancelled {
                        ceremony_id,
                        reason: CancelReason::Caller,
                    });
                    RedirectCaptureOutcome::RejectedOAuthError { code }
                }
            }
            CallbackQueryKind::AuthorizationCode => {
                let Some(code) = callback.parameter("code") else {
                    return RedirectCaptureOutcome::RejectedMalformedCallback;
                };
                let Ok(code) = SecretTokenMaterial::from_string(code) else {
                    return RedirectCaptureOutcome::RejectedMalformedCallback;
                };
                ceremony.code = Some(code);
                ceremony.status = BrowserStatus::ReadyForExchange;
                let cold = ceremony.cold;
                state.record_state(callback_state);
                state.revision = state.revision.saturating_add(1);
                drop(state);
                if cold && owner.is_background() {
                    let _ = self
                        .0
                        .cold
                        .delete(owner, ceremony_id, &background_context());
                }
                let outcome = RedirectCaptureOutcome::ReadyForExchange { ceremony_id };
                self.emit(AuthEvent::RedirectCaptured {
                    ceremony_id,
                    outcome,
                });
                outcome
            }
            CallbackQueryKind::MissingParameters => {
                RedirectCaptureOutcome::RejectedMalformedCallback
            }
        }
    }

    fn after_ceremony_removal(&self, owner: ExecutionOwner, removed: Option<BrowserCeremony>) {
        let Some(ceremony) = removed else {
            return;
        };
        if let Some(session_id) = ceremony.session_id() {
            let _ = self.0.browser.cancel(session_id);
        }
        if ceremony.cold && owner.is_background() && self.0.config.cold_redirects {
            let _ = self
                .0
                .cold
                .delete(owner, ceremony.id, &background_context());
        }
    }

    /// Exchange a captured code on the background owner and install the
    /// account session. The PKCE verifier and code are consumed once.
    pub fn complete_browser_ceremony(
        &self,
        owner: ExecutionOwner,
        ceremony_id: CeremonyId,
        now: Instant,
    ) -> Result<AccountSessionSummary, AuthError> {
        owner.require_background("exchange a browser authorization code")?;
        let (exchange, expected_account, provider_id, expiry) = {
            let mut state = lock(&self.0.state);
            let Some(ceremony) = state.ceremonies.get_mut(&ceremony_id) else {
                return Err(AuthError::StaleCeremony);
            };
            if ceremony.expires_at <= now {
                state.ceremonies.remove(&ceremony_id);
                state.revision = state.revision.saturating_add(1);
                return self.fail(AuthOperation::BrowserOAuthSession, AuthError::Expired);
            }
            match ceremony.status {
                BrowserStatus::ReadyForExchange => {}
                BrowserStatus::Exchanging | BrowserStatus::Committing => {
                    return Err(AuthError::Busy(
                        "Browser token exchange is already in progress",
                    ));
                }
                _ => return Err(AuthError::StaleCeremony),
            }
            let Some(code) = ceremony.code.take() else {
                return Err(AuthError::StaleCeremony);
            };
            let Some(verifier) = ceremony.verifier.take() else {
                return Err(AuthError::StaleCeremony);
            };
            ceremony.status = BrowserStatus::Exchanging;
            (
                TokenExchangeRequest {
                    ceremony_id,
                    provider_id: ceremony.provider_id.clone(),
                    account_hint: ceremony.expected_account.clone(),
                    code,
                    redirect: ceremony.redirect.clone(),
                    code_verifier: verifier,
                    nonce: ceremony.nonce.clone(),
                    scopes: ceremony.scopes.clone(),
                    now,
                },
                ceremony.expected_account.clone(),
                ceremony.provider_id.clone(),
                ceremony.expires_at,
            )
        };
        let tokens = match self.0.backend.exchange_authorization_code(exchange) {
            Ok(tokens) => tokens,
            Err(error) => {
                self.abort_exchange(ceremony_id, &error);
                return self.fail(AuthOperation::BrowserOAuthSession, error);
            }
        };
        if let Some(expected) = &expected_account
            && &tokens.account_id != expected
        {
            let error = AuthError::VerificationFailed(
                "Token exchange returned a different account than requested",
            );
            self.abort_exchange(ceremony_id, &error);
            return self.fail(AuthOperation::BrowserOAuthSession, error);
        }
        if tokens.provider_id != provider_id {
            let error = AuthError::VerificationFailed(
                "Token exchange returned a different provider identity",
            );
            self.abort_exchange(ceremony_id, &error);
            return self.fail(AuthOperation::BrowserOAuthSession, error);
        }
        if expiry <= now {
            let error = AuthError::Expired;
            self.abort_exchange(ceremony_id, &error);
            return self.fail(AuthOperation::BrowserOAuthSession, error);
        }
        {
            let mut state = lock(&self.0.state);
            let Some(ceremony) = state.ceremonies.get_mut(&ceremony_id) else {
                return Err(AuthError::StaleCeremony);
            };
            if ceremony.status != BrowserStatus::Exchanging {
                return Err(AuthError::StaleCeremony);
            }
            ceremony.status = BrowserStatus::Committing;
        }
        self.install_tokens(
            tokens,
            AuthMethodKind::BrowserOAuth,
            now,
            InstallGuard::Browser(ceremony_id),
        )
    }

    fn abort_exchange(&self, ceremony_id: CeremonyId, error: &AuthError) {
        let mut state = lock(&self.0.state);
        if let Some(ceremony) = state.ceremonies.remove(&ceremony_id) {
            state.record_state(ceremony.state.expose());
            state.revision = state.revision.saturating_add(1);
        }
        drop(state);
        self.emit_failure(AuthOperation::BrowserOAuthSession, error);
    }

    fn install_tokens(
        &self,
        tokens: IssuedAccountTokens,
        method: AuthMethodKind,
        now: Instant,
        guard: InstallGuard,
    ) -> Result<AccountSessionSummary, AuthError> {
        let account_id = tokens.account_id.clone();
        let context = background_context();
        let vault_gate = lock(&self.0.vault_gate);

        let (captured_epoch, had_live_account) = {
            let mut state = lock(&self.0.state);
            if state
                .accounts
                .get(&account_id)
                .is_some_and(|record| record.state == AccountSessionState::LoggingOut)
            {
                self.remove_guard(&mut state, guard);
                state.revision = state.revision.saturating_add(1);
                return Err(AuthError::Busy("Account logout is awaiting vault cleanup"));
            }
            if self.0.config.max_accounts <= state.accounts.len()
                && !state.accounts.contains_key(&account_id)
            {
                self.remove_guard(&mut state, guard);
                state.revision = state.revision.saturating_add(1);
                return Err(AuthError::AccountLimit);
            }
            if let Err(error) = self.guard_validation(&state, guard, now) {
                self.remove_guard(&mut state, guard);
                state.revision = state.revision.saturating_add(1);
                return Err(error);
            }
            let had_live_account = state
                .accounts
                .get(&account_id)
                .is_some_and(|record| record.state != AccountSessionState::LoggingOut);
            (state.epoch(&account_id), had_live_account)
        };

        if let Err(error) = self
            .0
            .vault
            .store_issued(ExecutionOwner::Background, &tokens, &context)
        {
            let mut state = lock(&self.0.state);
            self.remove_guard(&mut state, guard);
            state.revision = state.revision.saturating_add(1);
            return Err(error);
        }

        let summary = {
            let mut state = lock(&self.0.state);
            let validation = if state.epoch(&account_id) != captured_epoch {
                Err(AuthError::StaleCeremony)
            } else {
                self.guard_validation(&state, guard, now)
            };
            let validation = match validation {
                Err(error) => Err(error),
                Ok(())
                    if state
                        .accounts
                        .get(&account_id)
                        .is_some_and(|record| record.state == AccountSessionState::LoggingOut) =>
                {
                    Err(AuthError::StaleCeremony)
                }
                Ok(()) => Ok(()),
            };
            if let Err(error) = validation {
                self.remove_guard(&mut state, guard);
                let should_discard = !had_live_account
                    || state
                        .accounts
                        .get(&account_id)
                        .is_none_or(|record| record.state == AccountSessionState::LoggingOut);
                state.revision = state.revision.saturating_add(1);
                drop(state);
                if should_discard {
                    let _ = self.0.vault.discard_tokens(
                        ExecutionOwner::Background,
                        &account_id,
                        &context,
                    );
                }
                return Err(error);
            }
            let record = AccountRecord {
                provider_id: tokens.provider_id.clone(),
                label: tokens.label.clone(),
                scopes: tokens.scopes.clone(),
                state: AccountSessionState::Active,
                authenticated_at: tokens.authenticated_at,
                access_expires_at: tokens.access_expires_at,
                refresh_expires_at: tokens.refresh_expires_at,
                has_refresh_token: tokens.refresh_token.is_some(),
                auth_method: method,
            };
            state.accounts.insert(account_id.clone(), record);
            state.mark_account_epoch(&account_id);
            if state.primary.is_none() {
                state.primary = Some(account_id.clone());
            }
            self.remove_guard(&mut state, guard);
            state.revision = state.revision.saturating_add(1);
            state
                .account_summary(&account_id)
                .ok_or(AuthError::IntegrityFailure(
                    "Account session disappeared during installation",
                ))?
        };
        drop(vault_gate);
        self.emit(AuthEvent::AccountAuthenticated { account_id, method });
        Ok(summary)
    }

    fn guard_validation(
        &self,
        state: &ServiceState,
        guard: InstallGuard,
        now: Instant,
    ) -> Result<(), AuthError> {
        match guard {
            InstallGuard::Browser(id) => match state.ceremonies.get(&id) {
                Some(ceremony) if ceremony.expires_at <= now => Err(AuthError::Expired),
                Some(ceremony) if ceremony.status == BrowserStatus::Committing => Ok(()),
                _ => Err(AuthError::StaleCeremony),
            },
            InstallGuard::Passkey(id) => match state.passkey_ceremonies.get(&id) {
                Some(ceremony) if ceremony.expires_at <= now => Err(AuthError::Expired),
                Some(ceremony) if ceremony.status == PasskeyStatus::Committing => Ok(()),
                _ => Err(AuthError::StaleCeremony),
            },
        }
    }

    fn remove_guard(&self, state: &mut ServiceState, guard: InstallGuard) {
        match guard {
            InstallGuard::Browser(id) => {
                state.ceremonies.remove(&id);
            }
            InstallGuard::Passkey(id) => {
                state.passkey_ceremonies.remove(&id);
            }
        }
    }

    /// Cancel one browser ceremony. Returns `false` for an unknown or already
    /// finished ceremony; unknown ids never disturb live ceremonies.
    pub fn cancel_ceremony(
        &self,
        _owner: ExecutionOwner,
        ceremony_id: CeremonyId,
        _now: Instant,
    ) -> Result<bool, AuthError> {
        let mut session_id = None;
        let mut cold = false;
        let removed = {
            let mut state = lock(&self.0.state);
            let removed = state.ceremonies.remove(&ceremony_id);
            if let Some(ceremony) = &removed {
                state.record_state(ceremony.state.expose());
                session_id = ceremony.session_id();
                cold = ceremony.cold;
                state.revision = state.revision.saturating_add(1);
            }
            removed
        };
        let Some(_ceremony) = removed else {
            return Ok(false);
        };
        if let Some(session_id) = session_id {
            let _ = self.0.browser.cancel(session_id);
        }
        if cold {
            self.delete_cold_record(ceremony_id);
        }
        self.emit(AuthEvent::CeremonyCancelled {
            ceremony_id,
            reason: CancelReason::Caller,
        });
        Ok(true)
    }

    /// Cancel all ceremonies bound to an exact surface generation. Zero ids and
    /// stale generations are ignored.
    pub fn dispose_surface_generation(&self, surface: SurfaceId, generation: u64) -> usize {
        if surface.0 == 0 || generation == 0 {
            return 0;
        }
        let mut cancelled_sessions = Vec::new();
        let mut cancelled_passkeys = Vec::new();
        let mut cancelled_cold = Vec::new();
        let mut count = 0;
        {
            let mut state = lock(&self.0.state);
            let ids: Vec<_> = state
                .ceremonies
                .iter()
                .filter(|(_, ceremony)| {
                    ceremony.presentation.surface() == surface
                        && ceremony.presentation.generation() == generation
                })
                .map(|(id, _)| *id)
                .collect();
            for id in ids {
                if let Some(ceremony) = state.ceremonies.remove(&id) {
                    state.record_state(ceremony.state.expose());
                    if let Some(session_id) = ceremony.session_id() {
                        cancelled_sessions.push(session_id);
                    }
                    if ceremony.cold {
                        cancelled_cold.push(ceremony.id);
                    }
                    count += 1;
                }
            }
            let passkey_ids: Vec<_> = state
                .passkey_ceremonies
                .iter()
                .filter(|(_, ceremony)| {
                    ceremony.presentation.surface() == surface
                        && ceremony.presentation.generation() == generation
                })
                .map(|(id, _)| *id)
                .collect();
            for id in passkey_ids {
                state.passkey_ceremonies.remove(&id);
                cancelled_passkeys.push(id);
                count += 1;
            }
            if count > 0 {
                state.revision = state.revision.saturating_add(1);
            }
        }
        for session_id in cancelled_sessions {
            let _ = self.0.browser.cancel(session_id);
        }
        for ceremony_id in cancelled_passkeys {
            let _ = self.0.passkey.cancel(ceremony_id);
        }
        for ceremony_id in cancelled_cold {
            self.delete_cold_record(ceremony_id);
        }
        count
    }

    /// Discover and hydrate cold ceremonies from `gpui-secrets`. Expired and
    /// orphaned records are deleted.
    pub fn hydrate_cold_ceremonies(
        &self,
        owner: ExecutionOwner,
        now: Instant,
    ) -> Result<usize, AuthError> {
        owner.require_background("hydrate cold browser ceremonies")?;
        if !self.0.config.cold_redirects {
            return Ok(0);
        }
        let context = background_context();
        let ids = self.0.cold.list(owner, &context);
        let mut hydrated = 0;
        for ceremony_id in ids {
            if lock(&self.0.state).ceremonies.contains_key(&ceremony_id) {
                continue;
            }
            let Some(loaded) = self.0.cold.load(owner, ceremony_id, &context)? else {
                let _ = self.0.cold.delete(owner, ceremony_id, &context);
                continue;
            };
            let expires_at = now.checked_add(loaded.descriptor.remaining_ttl);
            let Some(expires_at) = expires_at else {
                let _ = self.0.cold.delete(owner, ceremony_id, &context);
                continue;
            };
            if expires_at <= now {
                let _ = self.0.cold.delete(owner, ceremony_id, &context);
                continue;
            }
            let mut state = lock(&self.0.state);
            if state.ceremonies.len() + state.passkey_ceremonies.len()
                >= self.0.config.max_ceremonies
            {
                break;
            }
            let descriptor = loaded.descriptor;
            state.ceremonies.insert(
                ceremony_id,
                BrowserCeremony {
                    id: ceremony_id,
                    provider_id: descriptor.provider_id,
                    state: descriptor.state,
                    nonce: descriptor.nonce,
                    scopes: descriptor.scopes,
                    expected_account: descriptor.expected_account,
                    presentation: descriptor.presentation,
                    preference: crate::BrowserSessionPreference::Shared,
                    redirect: descriptor.redirect,
                    authorize_url: String::new(),
                    verifier: Some(loaded.verifier),
                    code: None,
                    expires_at,
                    status: BrowserStatus::Prepared,
                    cold: true,
                },
            );
            state.revision = state.revision.saturating_add(1);
            hydrated += 1;
        }
        Ok(hydrated)
    }

    /// Change the selected primary account (UI-safe; no token access).
    pub fn select_primary_account(
        &self,
        owner: ExecutionOwner,
        account_id: &AccountId,
    ) -> Result<(), AuthError> {
        if owner != ExecutionOwner::Ui {
            return Err(AuthError::WrongExecutionOwner(
                "select a primary account from the UI",
            ));
        }
        let mut state = lock(&self.0.state);
        if !state
            .accounts
            .get(account_id)
            .is_some_and(|record| record.state != AccountSessionState::LoggingOut)
        {
            return Err(AuthError::NotAuthenticated);
        }
        if state.primary.as_ref() == Some(account_id) {
            return Ok(());
        }
        state.primary = Some(account_id.clone());
        state.revision = state.revision.saturating_add(1);
        drop(state);
        self.emit(AuthEvent::PrimaryAccountChanged {
            account_id: Some(account_id.clone()),
        });
        Ok(())
    }

    /// Advance monotonic expiry states. Returns accounts whose state changed.
    pub fn evaluate_expiries(&self, owner: ExecutionOwner, now: Instant) -> Vec<AccountId> {
        let mut changed = Vec::new();
        let mut reauth = Vec::new();
        let mut expired_ceremonies = Vec::new();
        let mut expired_passkeys = Vec::new();
        {
            let mut state = lock(&self.0.state);
            for (account_id, record) in &mut state.accounts {
                if matches!(
                    record.state,
                    AccountSessionState::ReauthRequired | AccountSessionState::LoggingOut
                ) {
                    continue;
                }
                let access_expired = record.access_expires_at.is_some_and(|expiry| expiry <= now);
                let refresh_expired = record.has_refresh_token
                    && record
                        .refresh_expires_at
                        .is_some_and(|expiry| expiry <= now);
                if record.state == AccountSessionState::Active && access_expired {
                    record.state = AccountSessionState::AccessTokenExpired;
                    changed.push(account_id.clone());
                }
                if refresh_expired || (access_expired && !record.has_refresh_token) {
                    record.state = AccountSessionState::ReauthRequired;
                    if !reauth.contains(account_id) {
                        reauth.push(account_id.clone());
                    }
                }
            }
            let ids: Vec<_> = state
                .ceremonies
                .iter()
                .filter(|(_, ceremony)| ceremony.expires_at <= now)
                .map(|(id, _)| *id)
                .collect();
            for id in ids {
                if let Some(ceremony) = state.ceremonies.remove(&id) {
                    state.record_state(ceremony.state.expose());
                    expired_ceremonies.push((id, ceremony.session_id(), ceremony.cold));
                }
            }
            let passkeys: Vec<_> = state
                .passkey_ceremonies
                .iter()
                .filter(|(_, ceremony)| ceremony.expires_at <= now)
                .map(|(id, _)| *id)
                .collect();
            for id in passkeys {
                state.passkey_ceremonies.remove(&id);
                expired_passkeys.push(id);
            }
            if !changed.is_empty()
                || !reauth.is_empty()
                || !expired_ceremonies.is_empty()
                || !expired_passkeys.is_empty()
            {
                state.revision = state.revision.saturating_add(1);
            }
        }
        for (ceremony_id, session_id, cold) in expired_ceremonies {
            if let Some(session_id) = session_id {
                let _ = self.0.browser.cancel(session_id);
            }
            if cold && owner.is_background() && self.0.config.cold_redirects {
                let _ = self
                    .0
                    .cold
                    .delete(owner, ceremony_id, &background_context());
            }
            self.emit(AuthEvent::CeremonyExpired { ceremony_id });
        }
        for ceremony_id in expired_passkeys {
            let _ = self.0.passkey.cancel(ceremony_id);
            self.emit(AuthEvent::CeremonyExpired { ceremony_id });
        }
        for account_id in &reauth {
            if !changed.contains(account_id) {
                changed.push(account_id.clone());
            }
            self.emit(AuthEvent::AccountReauthRequired {
                account_id: account_id.clone(),
            });
        }
        changed
    }

    /// Background-only single-flight refresh with rotation and logout-race
    /// handling.
    pub fn refresh_account(
        &self,
        owner: ExecutionOwner,
        account_id: &AccountId,
        now: Instant,
    ) -> Result<AccountSessionSummary, AuthError> {
        owner.require_background("refresh account tokens")?;
        let (provider_id, captured_epoch) = {
            let mut state = lock(&self.0.state);
            let (provider_id, record_state, has_refresh_token, refresh_expired) = {
                let Some(record) = state.accounts.get(account_id) else {
                    return Err(AuthError::NotAuthenticated);
                };
                (
                    record.provider_id.clone(),
                    record.state,
                    record.has_refresh_token,
                    record
                        .refresh_expires_at
                        .is_some_and(|expiry| expiry <= now),
                )
            };
            if record_state == AccountSessionState::LoggingOut {
                return Err(AuthError::NotAuthenticated);
            }
            if !has_refresh_token || refresh_expired {
                if let Some(record) = state.accounts.get_mut(account_id) {
                    record.state = AccountSessionState::ReauthRequired;
                    record.has_refresh_token = false;
                }
                state.revision = state.revision.saturating_add(1);
                return Err(AuthError::ReauthRequired);
            }
            if state.refreshing.contains(account_id) {
                return Err(AuthError::Busy("Account refresh is already in flight"));
            }
            state.refreshing.insert(account_id.clone());
            if let Some(record) = state.accounts.get_mut(account_id) {
                record.state = AccountSessionState::Refreshing;
            }
            state.revision = state.revision.saturating_add(1);
            (provider_id, state.epoch(account_id))
        };
        let context = background_context();
        let refresh_token =
            match self
                .0
                .vault
                .read_refresh_token(ExecutionOwner::Background, account_id, &context)
            {
                Ok(Some(token)) => token,
                Ok(None) => {
                    self.finish_refresh_failure(account_id, AuthError::ReauthRequired, true);
                    return Err(AuthError::ReauthRequired);
                }
                Err(error) => {
                    self.finish_refresh_failure(account_id, error.clone(), false);
                    return Err(error);
                }
            };
        let refreshed = self.0.backend.refresh_tokens(TokenRefreshRequest {
            account_id: account_id.clone(),
            provider_id,
            refresh_token,
            now,
        });
        let refreshed = match refreshed {
            Ok(tokens) => tokens,
            Err(error) => {
                let permanent = matches!(
                    error,
                    AuthError::ReauthRequired
                        | AuthError::ProviderFailure(_)
                        | AuthError::VerificationFailed(_)
                );
                self.finish_refresh_failure(account_id, error.clone(), permanent);
                return Err(error);
            }
        };
        let vault_gate = lock(&self.0.vault_gate);
        {
            let mut state = lock(&self.0.state);
            let still_ours = state.epoch(account_id) == captured_epoch
                && state
                    .accounts
                    .get(account_id)
                    .is_some_and(|record| record.state == AccountSessionState::Refreshing);
            if !still_ours {
                if state.refreshing.remove(account_id) {
                    state.revision = state.revision.saturating_add(1);
                }
                drop(state);
                drop(vault_gate);
                return Err(AuthError::StaleCeremony);
            }
        }
        if let Err(error) = self.0.vault.store_refreshed(
            ExecutionOwner::Background,
            account_id,
            &refreshed,
            &context,
        ) {
            drop(vault_gate);
            self.finish_refresh_failure(account_id, error.clone(), false);
            return Err(error);
        }
        let summary = {
            let mut state = lock(&self.0.state);
            let live = state.epoch(account_id) == captured_epoch
                && state
                    .accounts
                    .get(account_id)
                    .is_some_and(|record| record.state != AccountSessionState::LoggingOut);
            if !live {
                state.refreshing.remove(account_id);
                state.revision = state.revision.saturating_add(1);
                drop(state);
                let _ =
                    self.0
                        .vault
                        .discard_tokens(ExecutionOwner::Background, account_id, &context);
                drop(vault_gate);
                return Err(AuthError::StaleCeremony);
            }
            let record = state
                .accounts
                .get_mut(account_id)
                .ok_or(AuthError::NotAuthenticated)?;
            record.access_expires_at = refreshed.access_expires_at;
            if refreshed.refresh_token.is_some() {
                record.has_refresh_token = true;
                record.refresh_expires_at = refreshed.refresh_expires_at;
            }
            record.state = AccountSessionState::Active;
            state.mark_account_epoch(account_id);
            state.refreshing.remove(account_id);
            state.revision = state.revision.saturating_add(1);
            state
                .account_summary(account_id)
                .ok_or(AuthError::IntegrityFailure(
                    "Account disappeared during refresh",
                ))?
        };
        drop(vault_gate);
        self.emit(AuthEvent::AccountRefreshed {
            account_id: account_id.clone(),
        });
        Ok(summary)
    }

    fn finish_refresh_failure(&self, account_id: &AccountId, error: AuthError, permanent: bool) {
        let mut state = lock(&self.0.state);
        state.refreshing.remove(account_id);
        let mut reauth_required = false;
        if let Some(record) = state.accounts.get_mut(account_id) {
            // Logout phase 1 owns this account until vault cleanup completes.
            // A late refresh failure must not move it out of `LoggingOut`.
            if record.state != AccountSessionState::LoggingOut {
                if permanent || matches!(error, AuthError::ReauthRequired) {
                    record.state = AccountSessionState::ReauthRequired;
                    record.has_refresh_token = false;
                    reauth_required = true;
                } else if record.state == AccountSessionState::Refreshing {
                    record.state = AccountSessionState::Active;
                }
            }
        }
        state.revision = state.revision.saturating_add(1);
        drop(state);
        if reauth_required {
            self.emit(AuthEvent::AccountReauthRequired {
                account_id: account_id.clone(),
            });
        }
        self.emit_failure(AuthOperation::TokenVaultPersistence, &error);
    }

    /// Background-only logout with token deletion and optional revocation.
    pub fn logout_account(
        &self,
        owner: ExecutionOwner,
        account_id: &AccountId,
        _now: Instant,
    ) -> Result<(), AuthError> {
        owner.require_background("log out an account")?;
        self.require_local_presence(LocalPresencePurpose::RemoveAccount)?;

        let mut cancelled_sessions = Vec::new();
        let mut cancelled_passkeys = Vec::new();
        let provider_id = {
            let mut state = lock(&self.0.state);
            let Some(record) = state.accounts.get(account_id) else {
                return Ok(());
            };
            let provider_id = record.provider_id.clone();
            let mut changed = false;
            if record.state != AccountSessionState::LoggingOut {
                if let Some(record) = state.accounts.get_mut(account_id) {
                    record.state = AccountSessionState::LoggingOut;
                }
                state.mark_account_epoch(account_id);
                changed = true;
            }
            if state.refreshing.remove(account_id) {
                changed = true;
            }
            if state.primary.as_ref() == Some(account_id) {
                state.primary = state
                    .accounts
                    .iter()
                    .find(|(id, record)| {
                        *id != account_id && record.state != AccountSessionState::LoggingOut
                    })
                    .map(|(id, _)| id.clone());
                changed = true;
            }
            let ceremony_ids: Vec<_> = state
                .ceremonies
                .iter()
                .filter(|(_, ceremony)| ceremony.expected_account.as_ref() == Some(account_id))
                .map(|(id, _)| *id)
                .collect();
            for id in ceremony_ids {
                if let Some(ceremony) = state.ceremonies.remove(&id) {
                    state.record_state(ceremony.state.expose());
                    if let Some(session_id) = ceremony.session_id() {
                        cancelled_sessions.push(session_id);
                    }
                    if ceremony.cold {
                        state.pending_cold_cleanup.insert(ceremony.id);
                    }
                    changed = true;
                }
            }
            let passkey_ids: Vec<_> = state
                .passkey_ceremonies
                .iter()
                .filter(|(_, ceremony)| ceremony.expected_account.as_ref() == Some(account_id))
                .map(|(id, _)| *id)
                .collect();
            for id in passkey_ids {
                state.passkey_ceremonies.remove(&id);
                cancelled_passkeys.push(id);
                changed = true;
            }
            if changed {
                state.revision = state.revision.saturating_add(1);
            }
            provider_id
        };

        for session_id in cancelled_sessions {
            let _ = self.0.browser.cancel(session_id);
        }
        self.flush_pending_cold_cleanup(&background_context());
        for ceremony_id in cancelled_passkeys {
            let _ = self.0.passkey.cancel(ceremony_id);
        }

        let context = background_context();
        // Revocation is best effort: a locked vault still must be revisited by
        // `delete_account`, which reports the retryable error below.
        if let Ok(Some(refresh)) =
            self.0
                .vault
                .read_refresh_token(ExecutionOwner::Background, account_id, &context)
        {
            let _ = self.0.backend.revoke_tokens(TokenRevocationRequest {
                account_id: account_id.clone(),
                provider_id,
                token: refresh,
                kind: crate::browser::RevocationKind::RefreshToken,
            });
        }

        let vault_gate = lock(&self.0.vault_gate);
        if let Err(error) =
            self.0
                .vault
                .delete_account(ExecutionOwner::Background, account_id, &context)
        {
            drop(vault_gate);
            self.emit_failure(AuthOperation::TokenVaultPersistence, &error);
            return Err(error);
        }

        {
            let mut state = lock(&self.0.state);
            state.accounts.remove(account_id);
            if state.primary.as_ref() == Some(account_id) {
                state.primary = state
                    .accounts
                    .iter()
                    .find(|(_, record)| record.state != AccountSessionState::LoggingOut)
                    .map(|(id, _)| id.clone());
            }
            state.revision = state.revision.saturating_add(1);
        }
        drop(vault_gate);
        self.emit(AuthEvent::AccountLoggedOut {
            account_id: account_id.clone(),
        });
        Ok(())
    }

    /// Log out every account, then cancel unbound ceremonies.
    pub fn logout_all(&self, owner: ExecutionOwner, now: Instant) -> Result<usize, AuthError> {
        owner.require_background("log out all accounts")?;
        let ids: Vec<_> = lock(&self.0.state).accounts.keys().cloned().collect();
        let mut count = 0;
        for account_id in ids {
            self.logout_account(owner, &account_id, now)?;
            count += 1;
        }
        let orphaned: (Vec<CeremonyId>, Vec<BrowserCeremony>, Vec<CeremonyId>) = {
            let mut state = lock(&self.0.state);
            let removed_ceremonies = std::mem::take(&mut state.ceremonies);
            let ids: Vec<_> = removed_ceremonies.keys().copied().collect();
            let removed: Vec<_> = removed_ceremonies.into_values().collect();
            let passkey_ids: Vec<_> = state.passkey_ceremonies.keys().copied().collect();
            state.passkey_ceremonies.clear();
            if !ids.is_empty() || !passkey_ids.is_empty() {
                state.revision = state.revision.saturating_add(1);
            }
            (ids, removed, passkey_ids)
        };
        let (browser_ids, removed, passkey_ids) = orphaned;
        for ceremony in removed {
            self.after_ceremony_removal(owner, Some(ceremony));
        }
        for ceremony_id in passkey_ids {
            let _ = self.0.passkey.cancel(ceremony_id);
        }
        for ceremony_id in browser_ids {
            self.emit(AuthEvent::CeremonyCancelled {
                ceremony_id,
                reason: CancelReason::Caller,
            });
        }
        Ok(count)
    }

    /// Background-only, explicit token lease for consumers such as a bearer
    /// injector. UI/renderer owners and expired sessions are rejected.
    pub fn access_token(
        &self,
        owner: ExecutionOwner,
        account_id: &AccountId,
        now: Instant,
    ) -> Result<TokenLease, AuthError> {
        owner.require_background("read an access token lease")?;
        let (captured_epoch, expires_at) = {
            let mut state = lock(&self.0.state);
            let captured_epoch = state.epoch(account_id);
            let Some(record) = state.accounts.get_mut(account_id) else {
                return Err(AuthError::NotAuthenticated);
            };
            if record.state == AccountSessionState::AccessTokenExpired {
                return Err(AuthError::ReauthRequired);
            }
            if record.state != AccountSessionState::Active {
                return Err(AuthError::NotAuthenticated);
            }
            if record.access_expires_at.is_some_and(|expiry| expiry <= now) {
                record.state = AccountSessionState::AccessTokenExpired;
                state.revision = state.revision.saturating_add(1);
                return Err(AuthError::ReauthRequired);
            }
            (captured_epoch, record.access_expires_at)
        };
        let context = background_context();
        let token = self
            .0
            .vault
            .read_access_token(ExecutionOwner::Background, account_id, &context)?
            .ok_or(AuthError::NotAuthenticated)?;
        {
            let mut state = lock(&self.0.state);
            let valid = state.epoch(account_id) == captured_epoch
                && state
                    .accounts
                    .get(account_id)
                    .is_some_and(|record| record.state == AccountSessionState::Active);
            if !valid {
                return Err(AuthError::StaleCeremony);
            }
            state.revision = state.revision.saturating_add(1);
        }
        Ok(TokenLease::new(account_id.clone(), token, expires_at))
    }

    /// Explicit local-presence gate invocation. A successful receipt never
    /// creates an account session and is never accepted as a remote assertion.
    pub fn verify_local_presence(
        &self,
        _owner: ExecutionOwner,
        purpose: LocalPresencePurpose,
        now: Instant,
    ) -> Result<LocalPresenceReceipt, AuthError> {
        let gate = self.0.presence.as_ref().ok_or(AuthError::Unavailable(
            "No local user-presence gate is configured",
        ))?;
        let receipt = gate.verify(purpose, &OperationContext::default())?;
        if !receipt.verified {
            return Err(AuthError::UserPresenceDenied);
        }
        let _ = now;
        Ok(receipt)
    }

    fn require_local_presence(&self, purpose: LocalPresencePurpose) -> Result<(), AuthError> {
        if !self.0.config.require_local_presence_for_sensitive_actions {
            return Ok(());
        }
        let gate = self.0.presence.as_ref().ok_or(AuthError::Unavailable(
            "This operation requires a configured local user-presence gate",
        ))?;
        let receipt = gate.verify(purpose, &OperationContext::default())?;
        if receipt.verified {
            Ok(())
        } else {
            Err(AuthError::UserPresenceDenied)
        }
    }

    /// Start a passkey registration ceremony from the UI owner.
    pub fn begin_passkey_registration(
        &self,
        owner: ExecutionOwner,
        request: PasskeyCreationRequest,
        now: Instant,
    ) -> Result<CeremonyId, AuthError> {
        self.require_interactive(owner, "start a passkey registration")?;
        self.start_passkey_registration(owner, request, now)
    }

    /// Full background-capable registration (provider, server verifier and
    /// optional account session).
    pub fn register_passkey(
        &self,
        owner: ExecutionOwner,
        request: PasskeyCreationRequest,
        now: Instant,
    ) -> Result<PasskeyRegistrationResult, AuthError> {
        owner.require_background("complete a passkey registration")?;
        let ceremony_id = self.start_passkey_registration(owner, request, now)?;
        self.complete_passkey_registration(owner, ceremony_id, now)
    }

    fn start_passkey_registration(
        &self,
        owner: ExecutionOwner,
        request: PasskeyCreationRequest,
        now: Instant,
    ) -> Result<CeremonyId, AuthError> {
        self.check_operation(AuthOperation::PasskeyRegistration)?;
        if self.0.config.platform == AuthPlatform::Web && request.origin().is_none() {
            return Err(AuthError::InvalidInput(
                "Web passkey registration requires an explicit secure origin",
            ));
        }
        if request.mediation().is_conditional() {
            self.check_operation(AuthOperation::PasskeyConditionalMediation)?;
        }
        if now >= request.expires_at() {
            return Err(AuthError::Expired);
        }
        let presentation = request.presentation();
        let ceremony_id = {
            let mut state = lock(&self.0.state);
            if state.ceremonies.len() + state.passkey_ceremonies.len()
                >= self.0.config.max_ceremonies
            {
                return Err(AuthError::CeremonyLimit);
            }
            if state.has_interactive_ceremony(presentation) {
                return Err(AuthError::AlreadyActive);
            }
            if !state.consume_challenge(request.challenge()) {
                return Err(AuthError::StaleOrUnknownState);
            }
            let id = state.next_ceremony_id()?;
            state.passkey_ceremonies.insert(
                id,
                PasskeyCeremony {
                    presentation,
                    expires_at: request.expires_at(),
                    status: PasskeyStatus::InProvider,
                    expected_account: None,
                    verification: None,
                },
            );
            state.revision = state.revision.saturating_add(1);
            id
        };
        let provider_request = ProviderCreationRequest {
            ceremony_id,
            rp_id: request.rp_id().clone(),
            origin: request.origin().cloned(),
            challenge: request.challenge().expose().to_vec(),
            user_handle: crate::PasskeyUserHandle::new(request.user_handle().expose().to_vec())?,
            user_label: request.user_label().clone(),
            algorithms: request.algorithms().to_vec(),
            exclude_credentials: request.exclude_credentials().to_vec(),
            resident_key: request.resident_key(),
            user_verification: request.user_verification(),
            attestation: request.attestation(),
            mediation: request.mediation(),
            now,
        };
        let registration = match self
            .0
            .passkey
            .create(provider_request, &OperationContext::default())
        {
            Ok(registration) => registration,
            Err(error) => {
                let mut state = lock(&self.0.state);
                state.passkey_ceremonies.remove(&ceremony_id);
                state.revision = state.revision.saturating_add(1);
                drop(state);
                return self.fail(AuthOperation::PasskeyRegistration, error.to_auth_error());
            }
        };
        if now >= request.expires_at() {
            let _ = self.0.passkey.cancel(ceremony_id);
            let mut state = lock(&self.0.state);
            state.passkey_ceremonies.remove(&ceremony_id);
            state.revision = state.revision.saturating_add(1);
            return self.fail(AuthOperation::PasskeyRegistration, AuthError::Expired);
        }
        if let Err(error) = check_provider_registration(&request, &registration, now) {
            let _ = self.0.passkey.cancel(ceremony_id);
            lock(&self.0.state).passkey_ceremonies.remove(&ceremony_id);
            return self.fail(AuthOperation::PasskeyRegistration, error);
        }
        let verification = RegistrationVerificationRequest {
            ceremony_id,
            rp_id: request.rp_id().clone(),
            origin: request.origin().cloned(),
            challenge: request.challenge().clone(),
            registration,
            now,
        };
        {
            let mut state = lock(&self.0.state);
            let Some(ceremony) = state.passkey_ceremonies.get_mut(&ceremony_id) else {
                drop(state);
                let _ = self.0.passkey.cancel(ceremony_id);
                return Err(AuthError::StaleCeremony);
            };
            ceremony.status = PasskeyStatus::Ready;
            ceremony.verification = Some(PendingVerification::Registration(verification));
            state.revision = state.revision.saturating_add(1);
        }
        self.emit(AuthEvent::CeremonyLaunched {
            ceremony_id,
            method: AuthMethodKind::Passkey,
            surface: presentation.surface(),
            generation: presentation.generation(),
        });
        let _ = owner;
        Ok(ceremony_id)
    }

    /// Complete a passkey registration using the explicit server verifier.
    pub fn complete_passkey_registration(
        &self,
        owner: ExecutionOwner,
        ceremony_id: CeremonyId,
        now: Instant,
    ) -> Result<PasskeyRegistrationResult, AuthError> {
        owner.require_background("verify a passkey registration")?;
        let verification = {
            let mut state = lock(&self.0.state);
            let Some(ceremony) = state.passkey_ceremonies.get_mut(&ceremony_id) else {
                return Err(AuthError::StaleCeremony);
            };
            if ceremony.expires_at <= now {
                state.passkey_ceremonies.remove(&ceremony_id);
                state.revision = state.revision.saturating_add(1);
                return self.fail(AuthOperation::PasskeyRegistration, AuthError::Expired);
            }
            if ceremony.status != PasskeyStatus::Ready {
                return Err(AuthError::StaleCeremony);
            }
            let Some(PendingVerification::Registration(verification)) =
                ceremony.verification.take()
            else {
                return Err(AuthError::StaleCeremony);
            };
            ceremony.status = PasskeyStatus::Verifying;
            verification
        };
        let expected_rp = verification.rp_id.clone();
        let verified = match self.0.verifier.verify_registration(verification) {
            Ok(verified) => verified,
            Err(error) => {
                let mapped = error.to_auth_error();
                self.fail_passkey(
                    ceremony_id,
                    AuthOperation::PasskeyRegistration,
                    mapped.clone(),
                );
                return Err(mapped);
            }
        };
        if verified.rp_id != expected_rp {
            let error =
                AuthError::VerificationFailed("Server verifier returned a different relying party");
            self.fail_passkey(
                ceremony_id,
                AuthOperation::PasskeyRegistration,
                error.clone(),
            );
            return Err(error);
        }
        if verified.session.is_some() {
            self.commit_passkey(ceremony_id)?;
        } else {
            self.finish_passkey_without_session(ceremony_id)?;
        }
        let VerifiedRegistration {
            credential_id,
            rp_id,
            sign_count,
            backup_eligible,
            backup_state,
            user_verified,
            transports,
            session,
        } = verified;
        let summary = if let Some(issued) = session {
            let account_id = issued.account_id.clone();
            if issued.tokens.account_id != account_id {
                let error =
                    AuthError::VerificationFailed("Passkey tokens belong to a different account");
                self.fail_passkey(
                    ceremony_id,
                    AuthOperation::PasskeyRegistration,
                    error.clone(),
                );
                return Err(error);
            }
            Some(self.install_tokens(
                issued.tokens,
                AuthMethodKind::Passkey,
                now,
                InstallGuard::Passkey(ceremony_id),
            )?)
        } else {
            None
        };
        let account_id = summary.as_ref().map(|summary| summary.account_id().clone());
        self.emit(AuthEvent::PasskeyRegistered {
            account_id,
            credential_id: credential_id.clone(),
        });
        Ok(PasskeyRegistrationResult {
            credential_id,
            rp_id,
            sign_count,
            backup_eligible,
            backup_state,
            user_verified,
            transports,
            session: summary,
        })
    }

    fn commit_passkey(&self, ceremony_id: CeremonyId) -> Result<(), AuthError> {
        let mut state = lock(&self.0.state);
        let Some(ceremony) = state.passkey_ceremonies.get_mut(&ceremony_id) else {
            return Err(AuthError::StaleCeremony);
        };
        if ceremony.status != PasskeyStatus::Verifying {
            return Err(AuthError::StaleCeremony);
        }
        ceremony.status = PasskeyStatus::Committing;
        Ok(())
    }

    fn fail_passkey(&self, ceremony_id: CeremonyId, operation: AuthOperation, error: AuthError) {
        let mut state = lock(&self.0.state);
        state.passkey_ceremonies.remove(&ceremony_id);
        state.revision = state.revision.saturating_add(1);
        drop(state);
        let _ = self.0.passkey.cancel(ceremony_id);
        self.emit_failure(operation, &error);
    }

    /// Start a passkey assertion ceremony from the UI owner.
    pub fn begin_passkey_assertion(
        &self,
        owner: ExecutionOwner,
        request: PasskeyAssertionRequest,
        now: Instant,
    ) -> Result<CeremonyId, AuthError> {
        self.require_interactive(owner, "start a passkey assertion")?;
        self.start_passkey_assertion(request, now)
    }

    /// Full background-capable assertion.
    pub fn assert_passkey(
        &self,
        owner: ExecutionOwner,
        request: PasskeyAssertionRequest,
        now: Instant,
    ) -> Result<PasskeyAssertionResult, AuthError> {
        owner.require_background("complete a passkey assertion")?;
        let ceremony_id = self.start_passkey_assertion(request, now)?;
        self.complete_passkey_assertion(owner, ceremony_id, now)
    }

    fn start_passkey_assertion(
        &self,
        request: PasskeyAssertionRequest,
        now: Instant,
    ) -> Result<CeremonyId, AuthError> {
        self.check_operation(AuthOperation::PasskeyAssertion)?;
        if self.0.config.platform == AuthPlatform::Web && request.origin().is_none() {
            return Err(AuthError::InvalidInput(
                "Web passkey assertion requires an explicit secure origin",
            ));
        }
        if request.mediation().is_conditional() {
            self.check_operation(AuthOperation::PasskeyConditionalMediation)?;
        }
        if now >= request.expires_at() {
            return Err(AuthError::Expired);
        }
        let presentation = request.presentation();
        let ceremony_id = {
            let mut state = lock(&self.0.state);
            if state.ceremonies.len() + state.passkey_ceremonies.len()
                >= self.0.config.max_ceremonies
            {
                return Err(AuthError::CeremonyLimit);
            }
            if state.has_interactive_ceremony(presentation) {
                return Err(AuthError::AlreadyActive);
            }
            if !state.consume_challenge(request.challenge()) {
                return Err(AuthError::StaleOrUnknownState);
            }
            let id = state.next_ceremony_id()?;
            state.passkey_ceremonies.insert(
                id,
                PasskeyCeremony {
                    presentation,
                    expires_at: request.expires_at(),
                    status: PasskeyStatus::InProvider,
                    expected_account: request.expected_account().cloned(),
                    verification: None,
                },
            );
            state.revision = state.revision.saturating_add(1);
            id
        };
        let provider_request = ProviderAssertionRequest {
            ceremony_id,
            rp_id: request.rp_id().clone(),
            origin: request.origin().cloned(),
            challenge: request.challenge().expose().to_vec(),
            allow_credentials: request.allow_credentials().to_vec(),
            user_verification: request.user_verification(),
            mediation: request.mediation(),
            now,
        };
        let assertion = match self
            .0
            .passkey
            .assert(provider_request, &OperationContext::default())
        {
            Ok(assertion) => assertion,
            Err(error) => {
                let mut state = lock(&self.0.state);
                state.passkey_ceremonies.remove(&ceremony_id);
                state.revision = state.revision.saturating_add(1);
                drop(state);
                return self.fail(AuthOperation::PasskeyAssertion, error.to_auth_error());
            }
        };
        if now >= request.expires_at() {
            let _ = self.0.passkey.cancel(ceremony_id);
            let mut state = lock(&self.0.state);
            state.passkey_ceremonies.remove(&ceremony_id);
            state.revision = state.revision.saturating_add(1);
            return self.fail(AuthOperation::PasskeyAssertion, AuthError::Expired);
        }
        if let Err(error) = check_provider_assertion(&request, &assertion, now) {
            let _ = self.0.passkey.cancel(ceremony_id);
            lock(&self.0.state).passkey_ceremonies.remove(&ceremony_id);
            return self.fail(AuthOperation::PasskeyAssertion, error);
        }
        let verification = AssertionVerificationRequest {
            ceremony_id,
            rp_id: request.rp_id().clone(),
            origin: request.origin().cloned(),
            challenge: request.challenge().clone(),
            assertion,
            allow_credentials: request.allow_credentials().to_vec(),
            expected_account: request.expected_account().cloned(),
            now,
        };
        {
            let mut state = lock(&self.0.state);
            let Some(ceremony) = state.passkey_ceremonies.get_mut(&ceremony_id) else {
                drop(state);
                let _ = self.0.passkey.cancel(ceremony_id);
                return Err(AuthError::StaleCeremony);
            };
            ceremony.status = PasskeyStatus::Ready;
            ceremony.verification = Some(PendingVerification::Assertion(verification));
            state.revision = state.revision.saturating_add(1);
        }
        self.emit(AuthEvent::CeremonyLaunched {
            ceremony_id,
            method: AuthMethodKind::Passkey,
            surface: presentation.surface(),
            generation: presentation.generation(),
        });
        Ok(ceremony_id)
    }

    /// Complete a passkey assertion with explicit server verification.
    pub fn complete_passkey_assertion(
        &self,
        owner: ExecutionOwner,
        ceremony_id: CeremonyId,
        now: Instant,
    ) -> Result<PasskeyAssertionResult, AuthError> {
        owner.require_background("verify a passkey assertion")?;
        let verification = {
            let mut state = lock(&self.0.state);
            let Some(ceremony) = state.passkey_ceremonies.get_mut(&ceremony_id) else {
                return Err(AuthError::StaleCeremony);
            };
            if ceremony.expires_at <= now {
                state.passkey_ceremonies.remove(&ceremony_id);
                state.revision = state.revision.saturating_add(1);
                return self.fail(AuthOperation::PasskeyAssertion, AuthError::Expired);
            }
            if ceremony.status != PasskeyStatus::Ready {
                return Err(AuthError::StaleCeremony);
            }
            let Some(PendingVerification::Assertion(verification)) = ceremony.verification.take()
            else {
                return Err(AuthError::StaleCeremony);
            };
            ceremony.status = PasskeyStatus::Verifying;
            verification
        };
        let expected_rp = verification.rp_id.clone();
        let expected_account = verification.expected_account.clone();
        let verified = match self.0.verifier.verify_assertion(verification) {
            Ok(verified) => verified,
            Err(error) => {
                let mapped = error.to_auth_error();
                self.fail_passkey(ceremony_id, AuthOperation::PasskeyAssertion, mapped.clone());
                return Err(mapped);
            }
        };
        if verified.rp_id != expected_rp {
            let error =
                AuthError::VerificationFailed("Server verifier returned a different relying party");
            self.fail_passkey(ceremony_id, AuthOperation::PasskeyAssertion, error.clone());
            return Err(error);
        }
        if let Some(expected) = &expected_account
            && &verified.account_id != expected
        {
            let error = AuthError::VerificationFailed(
                "Server verifier returned a different account than expected",
            );
            self.fail_passkey(ceremony_id, AuthOperation::PasskeyAssertion, error.clone());
            return Err(error);
        }
        if verified.session.is_some() {
            self.commit_passkey(ceremony_id)?;
        } else {
            self.finish_passkey_without_session(ceremony_id)?;
        }
        let VerifiedAssertion {
            credential_id,
            rp_id,
            sign_count,
            user_verified,
            account_id,
            session,
        } = verified;
        let summary = if let Some(issued) = session {
            if issued.account_id != account_id || issued.tokens.account_id != account_id {
                let error =
                    AuthError::VerificationFailed("Passkey tokens belong to a different account");
                self.fail_passkey(ceremony_id, AuthOperation::PasskeyAssertion, error.clone());
                return Err(error);
            }
            Some(self.install_tokens(
                issued.tokens,
                AuthMethodKind::Passkey,
                now,
                InstallGuard::Passkey(ceremony_id),
            )?)
        } else {
            None
        };
        self.emit(AuthEvent::PasskeyAsserted {
            account_id: account_id.clone(),
            credential_id: credential_id.clone(),
        });
        Ok(PasskeyAssertionResult {
            credential_id,
            rp_id,
            sign_count,
            user_verified,
            account_id,
            session: summary,
        })
    }

    fn finish_passkey_without_session(&self, ceremony_id: CeremonyId) -> Result<(), AuthError> {
        let mut state = lock(&self.0.state);
        let Some(ceremony) = state.passkey_ceremonies.get(&ceremony_id) else {
            return Err(AuthError::StaleCeremony);
        };
        if ceremony.status != PasskeyStatus::Verifying {
            return Err(AuthError::StaleCeremony);
        }
        state.passkey_ceremonies.remove(&ceremony_id);
        state.revision = state.revision.saturating_add(1);
        Ok(())
    }

    /// Cancel a passkey ceremony. Unknown/finished ids return `false`.
    pub fn cancel_passkey_ceremony(&self, ceremony_id: CeremonyId) -> Result<bool, AuthError> {
        let removed = lock(&self.0.state).passkey_ceremonies.remove(&ceremony_id);
        if removed.is_some() {
            let _ = self.0.passkey.cancel(ceremony_id);
            let mut state = lock(&self.0.state);
            state.revision = state.revision.saturating_add(1);
            drop(state);
            self.emit(AuthEvent::CeremonyCancelled {
                ceremony_id,
                reason: CancelReason::Caller,
            });
            Ok(true)
        } else {
            Ok(false)
        }
    }

    pub fn passkey_ceremony_active(&self, ceremony_id: CeremonyId) -> bool {
        lock(&self.0.state)
            .passkey_ceremonies
            .contains_key(&ceremony_id)
    }

    pub fn active_ceremony_count(&self) -> usize {
        let state = lock(&self.0.state);
        state.ceremonies.len() + state.passkey_ceremonies.len()
    }

    pub fn is_refreshing(&self, account_id: &AccountId) -> bool {
        lock(&self.0.state).refreshing.contains(account_id)
    }

    fn require_interactive(
        &self,
        owner: ExecutionOwner,
        operation: &'static str,
    ) -> Result<(), AuthError> {
        if owner == ExecutionOwner::Ui {
            Ok(())
        } else {
            Err(AuthError::WrongExecutionOwner(operation))
        }
    }

    fn flush_pending_cold_cleanup(&self, context: &OperationContext) {
        let pending: Vec<_> = {
            let state = lock(&self.0.state);
            state
                .pending_cold_cleanup
                .iter()
                .copied()
                .collect::<Vec<_>>()
        };
        for ceremony_id in pending {
            if self
                .0
                .cold
                .delete(ExecutionOwner::Background, ceremony_id, context)
                .is_ok()
            {
                lock(&self.0.state)
                    .pending_cold_cleanup
                    .remove(&ceremony_id);
            }
        }
    }

    fn delete_cold_record(&self, ceremony_id: CeremonyId) {
        if self.0.config.cold_redirects {
            let _ = self.0.cold.delete(
                ExecutionOwner::Background,
                ceremony_id,
                &background_context(),
            );
        }
    }
}

/// Guard proving that the ceremony which initiated an install is still live.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InstallGuard {
    Browser(CeremonyId),
    Passkey(CeremonyId),
}

impl fmt::Debug for AuthService {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let state = lock(&self.0.state);
        formatter
            .debug_struct("AuthService")
            .field("platform", &state.platform)
            .field("accounts", &state.accounts.len())
            .field("ceremonies", &state.ceremonies.len())
            .field("passkey_ceremonies", &state.passkey_ceremonies.len())
            .field("revision", &state.revision)
            .finish()
    }
}

fn lock<T>(value: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    value
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn background_context() -> OperationContext {
    OperationContext {
        deadline: Some(Instant::now() + Duration::from_secs(30)),
        ..OperationContext::default()
    }
}
