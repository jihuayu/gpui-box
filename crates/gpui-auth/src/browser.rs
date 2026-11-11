//! Browser authorization requests, provider boundary and deterministic fakes.

use crate::{
    AccountId, AuthEndpointUrl, AuthError, AuthScope, AuthScopes, BrowserSessionPreference,
    CeremonyId, OAuthState, OidcNonce, PkcePair, PkceVerifier, PresentationContext, ProviderId,
    RedirectTarget, SecretTokenMaterial,
};
use gpui_system::OperationContext;
use std::{
    fmt,
    sync::Mutex,
    time::{Duration, Instant},
};

/// Default browser ceremony lifetime.
pub const DEFAULT_BROWSER_CEREMONY_TTL: Duration = Duration::from_secs(300);
/// Maximum browser ceremony lifetime.
pub const MAX_BROWSER_CEREMONY_TTL: Duration = Duration::from_secs(3600);

/// A validated browser authorization request.
pub struct BrowserAuthRequest {
    provider_id: ProviderId,
    client_id: String,
    endpoint: AuthEndpointUrl,
    redirect: RedirectTarget,
    scopes: AuthScopes,
    state: OAuthState,
    pkce: PkcePair,
    nonce: Option<OidcNonce>,
    presentation: PresentationContext,
    preference: BrowserSessionPreference,
    ttl: Duration,
    expected_account: Option<AccountId>,
}

impl BrowserAuthRequest {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        provider_id: ProviderId,
        client_id: impl Into<String>,
        endpoint: AuthEndpointUrl,
        redirect: RedirectTarget,
        scopes: AuthScopes,
        state: OAuthState,
        pkce: PkcePair,
        presentation: PresentationContext,
    ) -> Result<Self, AuthError> {
        let client_id = client_id.into();
        if client_id.is_empty()
            || client_id.len() > 256
            || client_id
                .bytes()
                .any(|byte| byte.is_ascii_control() || byte == b' ')
        {
            return Err(AuthError::InvalidInput(
                "OAuth client identifier must be bounded, visible ASCII",
            ));
        }
        Ok(Self {
            provider_id,
            client_id,
            endpoint,
            redirect,
            scopes,
            state,
            pkce,
            nonce: None,
            presentation,
            preference: BrowserSessionPreference::Shared,
            ttl: DEFAULT_BROWSER_CEREMONY_TTL,
            expected_account: None,
        })
    }

    pub fn with_nonce(mut self, nonce: OidcNonce) -> Self {
        self.nonce = Some(nonce);
        self
    }

    pub fn with_preference(mut self, preference: BrowserSessionPreference) -> Self {
        self.preference = preference;
        self
    }

    pub fn with_ttl(mut self, ttl: Duration) -> Result<Self, AuthError> {
        validate_ttl(ttl)?;
        self.ttl = ttl;
        Ok(self)
    }

    pub fn with_expected_account(mut self, account_id: AccountId) -> Self {
        self.expected_account = Some(account_id);
        self
    }

    pub fn provider_id(&self) -> &ProviderId {
        &self.provider_id
    }

    pub fn client_id(&self) -> &str {
        &self.client_id
    }

    pub fn endpoint(&self) -> &AuthEndpointUrl {
        &self.endpoint
    }

    pub fn redirect(&self) -> &RedirectTarget {
        &self.redirect
    }

    pub fn scopes(&self) -> &AuthScopes {
        &self.scopes
    }

    pub fn state(&self) -> &OAuthState {
        &self.state
    }

    pub fn nonce(&self) -> Option<&OidcNonce> {
        self.nonce.as_ref()
    }

    pub fn pkce_challenge(&self) -> &crate::PkceChallenge {
        self.pkce.challenge()
    }

    pub(crate) fn take_pkce(self) -> PkcePair {
        self.pkce
    }

    pub const fn presentation(&self) -> PresentationContext {
        self.presentation
    }

    pub const fn preference(&self) -> BrowserSessionPreference {
        self.preference
    }

    pub const fn ttl(&self) -> Duration {
        self.ttl
    }

    pub fn expected_account(&self) -> Option<&AccountId> {
        self.expected_account.as_ref()
    }
}

fn validate_ttl(ttl: Duration) -> Result<(), AuthError> {
    if ttl.is_zero() || ttl > MAX_BROWSER_CEREMONY_TTL {
        return Err(AuthError::InvalidInput(
            "Browser ceremony TTL must be non-zero and bounded",
        ));
    }
    Ok(())
}

/// Result of one successful launch.
#[derive(Clone, PartialEq, Eq)]
pub struct BrowserAuthHandle {
    ceremony_id: CeremonyId,
    authorize_url: String,
    resolved_redirect: RedirectTarget,
}

impl BrowserAuthHandle {
    pub(crate) fn new(
        ceremony_id: CeremonyId,
        authorize_url: String,
        resolved_redirect: RedirectTarget,
    ) -> Self {
        Self {
            ceremony_id,
            authorize_url,
            resolved_redirect,
        }
    }

    pub const fn ceremony_id(&self) -> CeremonyId {
        self.ceremony_id
    }

    /// Explicit sensitive access to the authorize URL. It contains CSRF state
    /// and a PKCE challenge but never a verifier or token.
    pub fn authorize_url(&self) -> &str {
        &self.authorize_url
    }

    pub fn resolved_redirect(&self) -> &RedirectTarget {
        &self.resolved_redirect
    }
}

impl fmt::Debug for BrowserAuthHandle {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("BrowserAuthHandle")
            .field("ceremony_id", &self.ceremony_id)
            .field("resolved_redirect", &self.resolved_redirect)
            .finish_non_exhaustive()
    }
}

/// Provider-facing launch input. The provider must not log the authorize URL.
#[derive(Clone)]
pub struct BrowserLaunchRequest {
    pub(crate) ceremony_id: CeremonyId,
    pub(crate) authorize_url: String,
    pub(crate) redirect: RedirectTarget,
    pub(crate) preference: BrowserSessionPreference,
    pub(crate) presentation: PresentationContext,
}

impl BrowserLaunchRequest {
    pub const fn ceremony_id(&self) -> CeremonyId {
        self.ceremony_id
    }

    /// Explicit sensitive access for native browser-session setup.
    pub fn authorize_url(&self) -> &str {
        &self.authorize_url
    }

    pub fn redirect(&self) -> &RedirectTarget {
        &self.redirect
    }

    pub const fn preference(&self) -> BrowserSessionPreference {
        self.preference
    }

    pub const fn presentation(&self) -> PresentationContext {
        self.presentation
    }
}

impl fmt::Debug for BrowserLaunchRequest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("BrowserLaunchRequest")
            .field("ceremony_id", &self.ceremony_id)
            .field("redirect", &self.redirect)
            .field("preference", &self.preference)
            .field("presentation", &self.presentation)
            .finish_non_exhaustive()
    }
}

/// An open provider session. Zero is never a valid native token.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BrowserSessionHandle {
    session_id: u64,
    resolved_redirect: RedirectTarget,
}

impl BrowserSessionHandle {
    pub fn new(session_id: u64, resolved_redirect: RedirectTarget) -> Result<Self, AuthError> {
        if session_id == 0 {
            return Err(AuthError::ProviderFailure(
                "Browser provider returned a zero native session token",
            ));
        }
        Ok(Self {
            session_id,
            resolved_redirect,
        })
    }

    pub const fn session_id(&self) -> u64 {
        self.session_id
    }

    pub fn resolved_redirect(&self) -> &RedirectTarget {
        &self.resolved_redirect
    }
}

/// Native/browser session boundary. Implementations may present UI and must
/// execute outside internal service locks.
pub trait BrowserAuthProvider: Send + Sync {
    /// Bind a listener or register a callback before the final authorize URL is
    /// built. The default leaves the target unchanged.
    fn resolve_redirect(&self, target: &RedirectTarget) -> Result<RedirectTarget, AuthError> {
        match target {
            RedirectTarget::Loopback { port: None, .. } => Err(AuthError::Unavailable(
                "Browser provider cannot resolve an ephemeral loopback port",
            )),
            _ => Ok(target.clone()),
        }
    }

    fn launch(
        &self,
        request: &BrowserLaunchRequest,
        context: &OperationContext,
    ) -> Result<BrowserSessionHandle, AuthError>;

    fn cancel(&self, session_id: u64) -> Result<(), AuthError>;
}

/// Alias used by implementations that describe the same provider boundary.
pub use BrowserAuthProvider as BrowserAuthSessionProvider;

/// Deterministic in-process browser provider for tests and contract consumers.
pub struct FakeBrowserAuthProvider {
    state: Mutex<FakeBrowserState>,
}

struct FakeBrowserState {
    next_session: u64,
    ephemeral_port: u16,
    launches: Vec<BrowserLaunchRecord>,
    cancelled: Vec<u64>,
    failure: Option<AuthError>,
}

#[derive(Debug, Clone)]
struct BrowserLaunchRecord {
    ceremony_id: CeremonyId,
    redirect: RedirectTarget,
    preference: BrowserSessionPreference,
    authorize_url: String,
}

impl FakeBrowserAuthProvider {
    pub fn new(ephemeral_port: u16) -> Self {
        Self::with_ephemeral_port(ephemeral_port)
    }

    pub fn with_ephemeral_port(ephemeral_port: u16) -> Self {
        Self {
            state: Mutex::new(FakeBrowserState {
                next_session: 1,
                ephemeral_port: ephemeral_port.max(1),
                launches: Vec::new(),
                cancelled: Vec::new(),
                failure: None,
            }),
        }
    }

    pub fn fail_next(&self, error: AuthError) {
        lock(&self.state).failure = Some(error);
    }

    pub fn launch_count(&self) -> usize {
        lock(&self.state).launches.len()
    }

    pub fn last_ceremony_id(&self) -> Option<CeremonyId> {
        lock(&self.state)
            .launches
            .last()
            .map(|launch| launch.ceremony_id)
    }

    pub fn last_redirect(&self) -> Option<RedirectTarget> {
        lock(&self.state)
            .launches
            .last()
            .map(|launch| launch.redirect.clone())
    }

    pub fn last_authorize_url(&self) -> Option<String> {
        lock(&self.state)
            .launches
            .last()
            .map(|launch| launch.authorize_url.clone())
    }

    pub fn cancelled_sessions(&self) -> Vec<u64> {
        lock(&self.state).cancelled.clone()
    }

    pub fn last_preference(&self) -> Option<BrowserSessionPreference> {
        lock(&self.state)
            .launches
            .last()
            .map(|launch| launch.preference)
    }
}

impl BrowserAuthProvider for FakeBrowserAuthProvider {
    fn resolve_redirect(&self, target: &RedirectTarget) -> Result<RedirectTarget, AuthError> {
        match target {
            RedirectTarget::Loopback { port: None, .. } => {
                let port = lock(&self.state).ephemeral_port;
                target.resolve_loopback_port(port)
            }
            _ => Ok(target.clone()),
        }
    }

    fn launch(
        &self,
        request: &BrowserLaunchRequest,
        _context: &OperationContext,
    ) -> Result<BrowserSessionHandle, AuthError> {
        let mut state = lock(&self.state);
        if let Some(error) = state.failure.take() {
            return Err(error);
        }
        let session_id = state.next_session;
        state.next_session = state.next_session.saturating_add(1);
        state.launches.push(BrowserLaunchRecord {
            ceremony_id: request.ceremony_id,
            redirect: request.redirect.clone(),
            preference: request.preference,
            authorize_url: request.authorize_url.clone(),
        });
        BrowserSessionHandle::new(session_id, request.redirect.clone())
    }

    fn cancel(&self, session_id: u64) -> Result<(), AuthError> {
        if session_id == 0 {
            return Err(AuthError::ProviderFailure(
                "Cannot cancel a zero browser session token",
            ));
        }
        lock(&self.state).cancelled.push(session_id);
        Ok(())
    }
}

impl fmt::Debug for FakeBrowserAuthProvider {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let state = lock(&self.state);
        formatter
            .debug_struct("FakeBrowserAuthProvider")
            .field("launches", &state.launches.len())
            .field("cancelled", &state.cancelled.len())
            .field("next_session", &state.next_session)
            .finish()
    }
}

fn lock<T>(value: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    value
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Build the final OIDC authorize URL with S256 PKCE.
#[allow(clippy::too_many_arguments)]
pub(crate) fn authorize_url(
    request: &BrowserAuthRequest,
    redirect: &RedirectTarget,
    challenge: &crate::PkceChallenge,
) -> Result<String, AuthError> {
    let redirect_uri = redirect.redirect_uri()?;
    let scope = request
        .scopes()
        .as_slice()
        .iter()
        .map(AuthScope::as_str)
        .collect::<Vec<_>>()
        .join(" ");
    let mut parameters = vec![
        ("response_type", "code"),
        ("client_id", request.client_id()),
        ("redirect_uri", redirect_uri.as_str()),
        ("scope", scope.as_str()),
        ("state", request.state().expose()),
        ("code_challenge", challenge.as_str()),
        ("code_challenge_method", "S256"),
    ];
    if let Some(nonce) = request.nonce() {
        parameters.push(("nonce", nonce.expose()));
    }
    Ok(request.endpoint().with_query(&parameters))
}

/// One authorization-code exchange. The code and verifier zeroize on drop.
pub struct TokenExchangeRequest {
    pub ceremony_id: CeremonyId,
    pub provider_id: ProviderId,
    pub account_hint: Option<AccountId>,
    pub code: SecretTokenMaterial,
    pub redirect: RedirectTarget,
    pub code_verifier: PkceVerifier,
    pub nonce: Option<OidcNonce>,
    pub scopes: Vec<AuthScope>,
    pub now: Instant,
}

impl fmt::Debug for TokenExchangeRequest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("TokenExchangeRequest")
            .field("ceremony_id", &self.ceremony_id)
            .field("provider_id", &self.provider_id)
            .field("account_hint", &self.account_hint)
            .field("redirect", &self.redirect)
            .field("code", &"[REDACTED]")
            .field("code_verifier", &"[REDACTED]")
            .field("nonce", &self.nonce.as_ref().map(|_| "[REDACTED]"))
            .field("scopes", &self.scopes)
            .finish_non_exhaustive()
    }
}

/// One refresh request. The refresh token zeroizes on drop.
pub struct TokenRefreshRequest {
    pub account_id: AccountId,
    pub provider_id: ProviderId,
    pub refresh_token: SecretTokenMaterial,
    pub now: Instant,
}

impl fmt::Debug for TokenRefreshRequest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("TokenRefreshRequest")
            .field("account_id", &self.account_id)
            .field("provider_id", &self.provider_id)
            .field("refresh_token", &"[REDACTED]")
            .finish_non_exhaustive()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RevocationKind {
    AccessToken,
    RefreshToken,
}

/// Optional revocation request.
pub struct TokenRevocationRequest {
    pub account_id: AccountId,
    pub provider_id: ProviderId,
    pub token: SecretTokenMaterial,
    pub kind: RevocationKind,
}

impl fmt::Debug for TokenRevocationRequest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("TokenRevocationRequest")
            .field("account_id", &self.account_id)
            .field("provider_id", &self.provider_id)
            .field("token", &"[REDACTED]")
            .field("kind", &self.kind)
            .finish()
    }
}

/// Alias matching the documented backend error vocabulary.
pub type AuthBackendError = AuthError;

/// Remote token exchange/refresh boundary. Invoked on the background owner and
/// never while a service lock is held.
pub trait TokenExchangeBackend: Send + Sync {
    fn exchange_authorization_code(
        &self,
        request: TokenExchangeRequest,
    ) -> Result<crate::IssuedAccountTokens, AuthBackendError>;

    fn refresh_tokens(
        &self,
        request: TokenRefreshRequest,
    ) -> Result<crate::RefreshedAccountTokens, AuthBackendError>;

    fn revoke_tokens(&self, _request: TokenRevocationRequest) -> Result<(), AuthBackendError> {
        Ok(())
    }
}

/// Deterministic token backend. It validates that the PKCE challenge it sees
/// matches the verifier and can be configured to fail one operation.
pub struct FakeTokenExchangeBackend {
    state: Mutex<FakeBackendState>,
}

struct FakeBackendState {
    account: AccountId,
    provider: ProviderId,
    label: crate::AccountLabel,
    issue_refresh: bool,
    rotate_refresh: bool,
    access_token: String,
    exchange_failure: Option<AuthError>,
    refresh_failure: Option<AuthError>,
    exchanged: usize,
    refreshed: usize,
    revoked: usize,
    last_challenge: Option<String>,
    fail_expected_account: bool,
}

impl FakeTokenExchangeBackend {
    pub fn new(account: AccountId, provider: ProviderId) -> Self {
        Self::with_label(
            account,
            provider,
            crate::AccountLabel::new("Test account").expect("static label is valid"),
        )
    }

    pub fn with_label(
        account: AccountId,
        provider: ProviderId,
        label: crate::AccountLabel,
    ) -> Self {
        Self {
            state: Mutex::new(FakeBackendState {
                account,
                provider,
                label,
                issue_refresh: true,
                rotate_refresh: true,
                access_token: "access-token".to_owned(),
                exchange_failure: None,
                refresh_failure: None,
                exchanged: 0,
                refreshed: 0,
                revoked: 0,
                last_challenge: None,
                fail_expected_account: false,
            }),
        }
    }

    pub fn without_refresh_token(&self) {
        lock(&self.state).issue_refresh = false;
    }

    /// Simulate an RFC 6749 section 6 refresh response that omits a new refresh
    /// token while leaving the existing token valid.
    pub fn without_refresh_rotation(&self) {
        lock(&self.state).rotate_refresh = false;
    }

    /// Change the access token returned by the next authorization-code
    /// exchanges. Useful for distinguishing concurrent login generations.
    pub fn set_access_token(&self, value: &str) {
        lock(&self.state).access_token = value.to_owned();
    }

    pub fn fail_exchange(&self, error: AuthError) {
        lock(&self.state).exchange_failure = Some(error);
    }

    pub fn fail_refresh(&self, error: AuthError) {
        lock(&self.state).refresh_failure = Some(error);
    }

    pub fn fail_expected_account(&self) {
        lock(&self.state).fail_expected_account = true;
    }

    pub fn exchanged(&self) -> usize {
        lock(&self.state).exchanged
    }

    pub fn refreshed(&self) -> usize {
        lock(&self.state).refreshed
    }

    pub fn revoked(&self) -> usize {
        lock(&self.state).revoked
    }

    pub fn last_challenge(&self) -> Option<String> {
        lock(&self.state).last_challenge.clone()
    }
}

impl TokenExchangeBackend for FakeTokenExchangeBackend {
    fn exchange_authorization_code(
        &self,
        request: TokenExchangeRequest,
    ) -> Result<crate::IssuedAccountTokens, AuthError> {
        let mut state = lock(&self.state);
        if let Some(error) = state.exchange_failure.take() {
            return Err(error);
        }
        let expected = request.code_verifier.challenge();
        state.last_challenge = Some(expected.as_str().to_owned());
        state.exchanged += 1;
        let account = if state.fail_expected_account {
            AccountId::new("unexpected-account")?
        } else {
            request
                .account_hint
                .clone()
                .unwrap_or_else(|| state.account.clone())
        };
        Ok(crate::IssuedAccountTokens {
            account_id: account,
            provider_id: state.provider.clone(),
            label: state.label.clone(),
            scopes: request.scopes,
            access_token: SecretTokenMaterial::from_string(&state.access_token)?,
            refresh_token: state
                .issue_refresh
                .then(|| SecretTokenMaterial::from_string("refresh-token"))
                .transpose()?,
            id_token: Some(SecretTokenMaterial::from_string("id-token")?),
            authenticated_at: request.now,
            access_expires_at: Some(request.now + Duration::from_secs(900)),
            refresh_expires_at: Some(request.now + Duration::from_secs(86_400)),
        })
    }

    fn refresh_tokens(
        &self,
        request: TokenRefreshRequest,
    ) -> Result<crate::RefreshedAccountTokens, AuthError> {
        let mut state = lock(&self.state);
        if let Some(error) = state.refresh_failure.take() {
            return Err(error);
        }
        state.refreshed += 1;
        Ok(crate::RefreshedAccountTokens {
            access_token: SecretTokenMaterial::from_string("refreshed-access-token")?,
            refresh_token: state
                .rotate_refresh
                .then(|| SecretTokenMaterial::from_string("rotated-refresh-token"))
                .transpose()?,
            id_token: Some(SecretTokenMaterial::from_string("refreshed-id-token")?),
            access_expires_at: Some(request.now + Duration::from_secs(900)),
            refresh_expires_at: Some(request.now + Duration::from_secs(86_400)),
        })
    }

    fn revoke_tokens(&self, _request: TokenRevocationRequest) -> Result<(), AuthError> {
        lock(&self.state).revoked += 1;
        Ok(())
    }
}

impl fmt::Debug for FakeTokenExchangeBackend {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let state = lock(&self.state);
        formatter
            .debug_struct("FakeTokenExchangeBackend")
            .field("account", &state.account)
            .field("exchanged", &state.exchanged)
            .field("refreshed", &state.refreshed)
            .field("revoked", &state.revoked)
            .finish()
    }
}
