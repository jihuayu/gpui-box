//! Core account, execution-owner, token and public snapshot contracts.
//!
//! Public summaries and events contain no access token, refresh token, ID
//! token, authorization code, PKCE verifier, WebAuthn signature or raw user
//! handle. Sensitive byte buffers zeroize on drop and redact `Debug`.

use crate::pkce::constant_time_eq;
use gpui_system::{
    CancelReason, ErrorCode, OperationContext, RecoveryAction, RetryAdvice, SurfaceId, SystemError,
};
use std::{
    fmt,
    hint::black_box,
    time::{Duration, Instant},
};

/// Maximum number of accounts retained by one service.
pub const MAX_ACCOUNTS: usize = 16;
/// Maximum number of simultaneously active browser/passkey ceremonies.
pub const MAX_ACTIVE_CEREMONIES: usize = 16;
/// Maximum number of scopes attached to one authorization request.
pub const MAX_SCOPES: usize = 16;
/// Maximum length of a provider account identifier.
pub const MAX_ACCOUNT_ID_LEN: usize = 128;
/// Maximum length of one stored token or authorization code.
pub const MAX_TOKEN_BYTES: usize = 2048;

/// The thread/execution domain that initiated a sensitive operation.
///
/// `Ui` and `Renderer` owners may read public snapshots and drive interactive
/// presentation, but may never read the secret vault. Secret persistence,
/// token exchange, refresh and revocation require [`ExecutionOwner::Background`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ExecutionOwner {
    Ui,
    Background,
    Renderer,
}

impl ExecutionOwner {
    pub const fn is_background(self) -> bool {
        matches!(self, Self::Background)
    }

    pub fn require_background(self, operation: &'static str) -> Result<(), AuthError> {
        if self.is_background() {
            Ok(())
        } else {
            Err(AuthError::WrongExecutionOwner(operation))
        }
    }
}

/// Non-secret summary of one established account session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccountSessionSummary {
    account_id: AccountId,
    provider_id: ProviderId,
    label: AccountLabel,
    scopes: Vec<AuthScope>,
    state: AccountSessionState,
    authenticated_at: Instant,
    access_expires_at: Option<Instant>,
    refresh_expires_at: Option<Instant>,
    has_refresh_token: bool,
    auth_method: AuthMethodKind,
    primary: bool,
}

impl AccountSessionSummary {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        account_id: AccountId,
        provider_id: ProviderId,
        label: AccountLabel,
        scopes: Vec<AuthScope>,
        state: AccountSessionState,
        authenticated_at: Instant,
        access_expires_at: Option<Instant>,
        refresh_expires_at: Option<Instant>,
        has_refresh_token: bool,
        auth_method: AuthMethodKind,
        primary: bool,
    ) -> Self {
        Self {
            account_id,
            provider_id,
            label,
            scopes,
            state,
            authenticated_at,
            access_expires_at,
            refresh_expires_at,
            has_refresh_token,
            auth_method,
            primary,
        }
    }

    pub fn account_id(&self) -> &AccountId {
        &self.account_id
    }

    pub fn provider_id(&self) -> &ProviderId {
        &self.provider_id
    }

    pub fn label(&self) -> &AccountLabel {
        &self.label
    }

    pub fn scopes(&self) -> &[AuthScope] {
        &self.scopes
    }

    pub const fn state(&self) -> AccountSessionState {
        self.state
    }

    pub const fn authenticated_at(&self) -> Instant {
        self.authenticated_at
    }

    pub const fn access_expires_at(&self) -> Option<Instant> {
        self.access_expires_at
    }

    pub const fn refresh_expires_at(&self) -> Option<Instant> {
        self.refresh_expires_at
    }

    pub const fn has_refresh_token(&self) -> bool {
        self.has_refresh_token
    }

    pub const fn auth_method(&self) -> AuthMethodKind {
        self.auth_method
    }

    pub const fn is_primary(&self) -> bool {
        self.primary
    }

    pub const fn is_usable(&self) -> bool {
        matches!(self.state, AccountSessionState::Active)
    }
}

/// Public-safe lifecycle of one account session.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum AccountSessionState {
    Active,
    AccessTokenExpired,
    Refreshing,
    ReauthRequired,
    LoggingOut,
}

impl AccountSessionState {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::AccessTokenExpired => "access_token_expired",
            Self::Refreshing => "refreshing",
            Self::ReauthRequired => "reauth_required",
            Self::LoggingOut => "logging_out",
        }
    }
}

/// How an account session was established.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum AuthMethodKind {
    BrowserOAuth,
    Passkey,
}

impl AuthMethodKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::BrowserOAuth => "browser_oauth",
            Self::Passkey => "passkey",
        }
    }
}

/// Zero-secret snapshot safe to publish to UI and renderer owners.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthSnapshot {
    pub accounts: Vec<AccountSessionSummary>,
    pub primary_account: Option<AccountId>,
    pub pending_ceremonies: usize,
    pub pending_passkey_ceremonies: usize,
    pub revision: u64,
}

impl AuthSnapshot {
    pub fn find(&self, account_id: &AccountId) -> Option<&AccountSessionSummary> {
        self.accounts
            .iter()
            .find(|summary| summary.account_id() == account_id)
    }

    pub fn is_empty(&self) -> bool {
        self.accounts.is_empty()
    }
}

/// Stable, opaque account identifier. Callers must not pass an email address,
/// display name or other personally identifying value.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct AccountId(String);

impl AccountId {
    pub fn new(value: impl Into<String>) -> Result<Self, AuthError> {
        let value = value.into();
        if value.is_empty()
            || value.len() > MAX_ACCOUNT_ID_LEN
            || !value.bytes().all(|byte| {
                byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b':')
            })
        {
            return Err(AuthError::InvalidInput(
                "Account identifier must be opaque, bounded ASCII",
            ));
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for AccountId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.debug_tuple("AccountId").field(&self.0).finish()
    }
}

impl fmt::Display for AccountId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

/// Stable identity of the authorization server / relying party integration.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ProviderId(String);

impl ProviderId {
    pub fn new(value: impl Into<String>) -> Result<Self, AuthError> {
        let value = value.into();
        if value.is_empty()
            || value.len() > 128
            || !value.bytes().all(|byte| {
                byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'/')
            })
        {
            return Err(AuthError::InvalidInput(
                "Provider identifier must be bounded ASCII",
            ));
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for ProviderId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.debug_tuple("ProviderId").field(&self.0).finish()
    }
}

/// Non-PII account label for display. The host is responsible for choosing a
/// non-identifying label; this crate never derives one from an email address.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct AccountLabel(String);

impl AccountLabel {
    pub fn new(value: impl Into<String>) -> Result<Self, AuthError> {
        let value = value.into();
        if value.is_empty()
            || value.len() > 128
            || value.chars().any(char::is_control)
            || value.trim() != value
        {
            return Err(AuthError::InvalidInput(
                "Account label must be non-empty, trimmed and bounded",
            ));
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for AccountLabel {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_tuple("AccountLabel")
            .field(&self.0)
            .finish()
    }
}

/// One validated OAuth/OIDC scope token.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct AuthScope(String);

impl AuthScope {
    pub fn new(value: impl Into<String>) -> Result<Self, AuthError> {
        let value = value.into();
        if value.is_empty()
            || value.len() > 128
            || !value
                .bytes()
                .all(|byte| (0x21..=0x7e).contains(&byte) && !matches!(byte, b'"' | b'\\'))
        {
            return Err(AuthError::InvalidInput("Scope token is malformed"));
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for AuthScope {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.debug_tuple("AuthScope").field(&self.0).finish()
    }
}

/// Bounded, order-preserving, deduplicated scope list.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AuthScopes(Vec<AuthScope>);

impl AuthScopes {
    pub fn new(scopes: impl IntoIterator<Item = AuthScope>) -> Result<Self, AuthError> {
        let mut values = Vec::new();
        for scope in scopes {
            if !values.contains(&scope) {
                values.push(scope);
            }
        }
        if values.is_empty() {
            return Err(AuthError::InvalidInput(
                "At least one authorization scope is required",
            ));
        }
        if values.len() > MAX_SCOPES {
            return Err(AuthError::InvalidInput(
                "Authorization scope count exceeds the bound",
            ));
        }
        Ok(Self(values))
    }

    pub fn as_slice(&self) -> &[AuthScope] {
        &self.0
    }

    pub fn joined(&self) -> String {
        self.0
            .iter()
            .map(AuthScope::as_str)
            .collect::<Vec<_>>()
            .join(" ")
    }
}

/// CSRF state token. Single-use and compared in constant time.
#[derive(Clone, PartialEq, Eq)]
pub struct OAuthState(String);

impl OAuthState {
    pub fn new(value: impl Into<String>) -> Result<Self, AuthError> {
        let value = value.into();
        if !(16..=128).contains(&value.len())
            || !value.bytes().all(|byte| {
                byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~')
            })
        {
            return Err(AuthError::InvalidInput(
                "OAuth state must be 16..=128 URL-safe ASCII bytes",
            ));
        }
        Ok(Self(value))
    }

    /// Explicit sensitive access for authorize-URL construction.
    pub fn expose(&self) -> &str {
        &self.0
    }

    pub(crate) fn matches(&self, candidate: &str) -> bool {
        constant_time_eq(self.0.as_bytes(), candidate.as_bytes())
    }
}

impl fmt::Debug for OAuthState {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("OAuthState([REDACTED])")
    }
}

/// Optional OIDC nonce echoed through the token exchange backend.
#[derive(Clone, PartialEq, Eq)]
pub struct OidcNonce(String);

impl OidcNonce {
    pub fn new(value: impl Into<String>) -> Result<Self, AuthError> {
        let value = value.into();
        if !(16..=128).contains(&value.len())
            || !value.bytes().all(|byte| {
                byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~')
            })
        {
            return Err(AuthError::InvalidInput(
                "OIDC nonce must be 16..=128 URL-safe ASCII bytes",
            ));
        }
        Ok(Self(value))
    }

    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for OidcNonce {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("OidcNonce([REDACTED])")
    }
}

/// Browser profile preference for an authorization ceremony.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BrowserSessionPreference {
    Ephemeral,
    Shared,
}

impl BrowserSessionPreference {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Ephemeral => "ephemeral",
            Self::Shared => "shared",
        }
    }
}

/// Non-zero, explicit surface generation that owns an interactive ceremony.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PresentationContext {
    surface: SurfaceId,
    generation: u64,
    interactive: bool,
}

impl PresentationContext {
    pub fn new(surface: SurfaceId, generation: u64) -> Result<Self, AuthError> {
        if surface.0 == 0 || generation == 0 {
            return Err(AuthError::InvalidInput(
                "Presentation requires a non-zero surface and generation",
            ));
        }
        Ok(Self {
            surface,
            generation,
            interactive: true,
        })
    }

    /// Build from an explicit UI context, preserving the owner's generation.
    pub fn from_ui(ui: &gpui_system::UiContext) -> Result<Self, AuthError> {
        if !ui.is_current() {
            return Err(AuthError::ContextExpired);
        }
        Self::new(ui.surface, ui.generation)
    }

    pub const fn surface(self) -> SurfaceId {
        self.surface
    }

    pub const fn generation(self) -> u64 {
        self.generation
    }

    pub const fn is_interactive(self) -> bool {
        self.interactive
    }
}

/// Zeroizing token/code buffer with redacted `Debug` and no `Clone`.
pub struct SecretTokenMaterial {
    bytes: Vec<u8>,
}

impl SecretTokenMaterial {
    pub fn new(bytes: impl Into<Vec<u8>>) -> Result<Self, AuthError> {
        let bytes = bytes.into();
        if bytes.is_empty() {
            return Err(AuthError::InvalidInput("Token material must be non-empty"));
        }
        if bytes.len() > MAX_TOKEN_BYTES {
            return Err(AuthError::InvalidInput("Token material exceeds the bound"));
        }
        Ok(Self { bytes })
    }

    pub fn from_string(value: impl Into<String>) -> Result<Self, AuthError> {
        Self::new(value.into().into_bytes())
    }

    /// Explicit sensitive access. Never format or log the returned bytes.
    pub fn expose(&self) -> &[u8] {
        &self.bytes
    }

    pub fn len(&self) -> usize {
        self.bytes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.bytes.is_empty()
    }
}

impl fmt::Debug for SecretTokenMaterial {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("SecretTokenMaterial([REDACTED])")
    }
}

impl PartialEq for SecretTokenMaterial {
    fn eq(&self, other: &Self) -> bool {
        constant_time_eq(&self.bytes, &other.bytes)
    }
}

impl Eq for SecretTokenMaterial {}

impl Drop for SecretTokenMaterial {
    fn drop(&mut self) {
        self.bytes.fill(0);
        black_box(&self.bytes);
    }
}

/// Tokens issued by a browser code exchange.
pub struct IssuedAccountTokens {
    pub account_id: AccountId,
    pub provider_id: ProviderId,
    pub label: AccountLabel,
    pub scopes: Vec<AuthScope>,
    pub access_token: SecretTokenMaterial,
    pub refresh_token: Option<SecretTokenMaterial>,
    pub id_token: Option<SecretTokenMaterial>,
    pub authenticated_at: Instant,
    pub access_expires_at: Option<Instant>,
    pub refresh_expires_at: Option<Instant>,
}

impl fmt::Debug for IssuedAccountTokens {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("IssuedAccountTokens")
            .field("account_id", &self.account_id)
            .field("provider_id", &self.provider_id)
            .field("label", &self.label)
            .field("scopes", &self.scopes)
            .field("access_token", &"[REDACTED]")
            .field(
                "refresh_token",
                &self.refresh_token.as_ref().map(|_| "[REDACTED]"),
            )
            .field("id_token", &self.id_token.as_ref().map(|_| "[REDACTED]"))
            .field("authenticated_at", &self.authenticated_at)
            .field("access_expires_at", &self.access_expires_at)
            .field("refresh_expires_at", &self.refresh_expires_at)
            .finish()
    }
}

/// Tokens returned by a refresh. Rotation is expressed by a new refresh token.
pub struct RefreshedAccountTokens {
    pub access_token: SecretTokenMaterial,
    pub refresh_token: Option<SecretTokenMaterial>,
    pub id_token: Option<SecretTokenMaterial>,
    pub access_expires_at: Option<Instant>,
    pub refresh_expires_at: Option<Instant>,
}

impl fmt::Debug for RefreshedAccountTokens {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RefreshedAccountTokens")
            .field("access_token", &"[REDACTED]")
            .field(
                "refresh_token",
                &self.refresh_token.as_ref().map(|_| "[REDACTED]"),
            )
            .field("id_token", &self.id_token.as_ref().map(|_| "[REDACTED]"))
            .field("access_expires_at", &self.access_expires_at)
            .field("refresh_expires_at", &self.refresh_expires_at)
            .finish()
    }
}

/// A short explicit borrow of a still-valid access token for a background
/// consumer such as the network bearer injector.
pub struct TokenLease {
    account_id: AccountId,
    access_token: SecretTokenMaterial,
    expires_at: Option<Instant>,
}

impl TokenLease {
    pub(crate) fn new(
        account_id: AccountId,
        access_token: SecretTokenMaterial,
        expires_at: Option<Instant>,
    ) -> Self {
        Self {
            account_id,
            access_token,
            expires_at,
        }
    }

    pub fn account_id(&self) -> &AccountId {
        &self.account_id
    }

    pub const fn expires_at(&self) -> Option<Instant> {
        self.expires_at
    }

    pub fn expose_access_token(&self) -> &[u8] {
        self.access_token.expose()
    }
}

impl fmt::Debug for TokenLease {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("TokenLease")
            .field("account_id", &self.account_id)
            .field("access_token", &"[REDACTED]")
            .field("expires_at", &self.expires_at)
            .finish()
    }
}

/// Why a local-user-presence check was requested. This gate models an opaque
/// local device check only; it never authorizes a remote token.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LocalPresencePurpose {
    StepUpCeremony,
    RemoveAccount,
}

/// Opaque receipt from a local presence gate. It proves only that the local
/// gate reported success; it is not a remote credential or server assertion.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LocalPresenceReceipt {
    pub verified: bool,
    pub issued_at: Instant,
}

impl LocalPresenceReceipt {
    pub const fn verified(issued_at: Instant) -> Self {
        Self {
            verified: true,
            issued_at,
        }
    }

    pub const fn denied(issued_at: Instant) -> Self {
        Self {
            verified: false,
            issued_at,
        }
    }
}

/// Optional local device-presence hook. Implementations belong to
/// `gpui-local-auth` (#28) or the host; `gpui-auth` deliberately does not depend
/// on that crate. A successful receipt cannot mint a session or token.
pub trait LocalUserPresenceGate: Send + Sync {
    fn verify(
        &self,
        purpose: LocalPresencePurpose,
        context: &OperationContext,
    ) -> Result<LocalPresenceReceipt, AuthError>;
}

/// Typed authentication failure with static, value-free messages.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuthError {
    System(SystemError),
    Cancelled(CancelReason),
    InvalidInput(&'static str),
    InvalidConfiguration(&'static str),
    Unsupported(&'static str),
    Unavailable(&'static str),
    WrongExecutionOwner(&'static str),
    ContextExpired,
    AlreadyActive,
    Busy(&'static str),
    Expired,
    StaleOrUnknownState,
    StaleCeremony,
    NoMatchingCredentials,
    CredentialExcluded,
    UserPresenceRequired,
    UserPresenceDenied,
    ReauthRequired,
    NotAuthenticated,
    AccountLimit,
    CeremonyLimit,
    ProviderFailure(&'static str),
    VerificationFailed(&'static str),
    IntegrityFailure(&'static str),
}

impl AuthError {
    pub const fn code(&self) -> ErrorCode {
        match self {
            Self::System(error) => error.code,
            Self::Cancelled(_) => ErrorCode::Unavailable,
            Self::InvalidInput(_) | Self::StaleOrUnknownState => ErrorCode::InvalidInput,
            Self::InvalidConfiguration(_) => ErrorCode::InvalidConfiguration,
            Self::Unsupported(_) => ErrorCode::Unsupported,
            Self::Unavailable(_)
            | Self::WrongExecutionOwner(_)
            | Self::NotAuthenticated
            | Self::NoMatchingCredentials => ErrorCode::Unavailable,
            Self::ContextExpired | Self::Expired => ErrorCode::ContextExpired,
            Self::AlreadyActive | Self::Busy(_) | Self::CeremonyLimit | Self::AccountLimit => {
                ErrorCode::Busy
            }
            Self::StaleCeremony => ErrorCode::ResourceExpired,
            Self::CredentialExcluded => ErrorCode::Busy,
            Self::UserPresenceRequired | Self::UserPresenceDenied | Self::ReauthRequired => {
                ErrorCode::PermissionDenied
            }
            Self::ProviderFailure(_) | Self::VerificationFailed(_) => ErrorCode::PlatformFailure,
            Self::IntegrityFailure(_) => ErrorCode::IntegrityFailure,
        }
    }

    /// Convert to the shared system error vocabulary without copying secrets.
    pub fn to_system_error(&self) -> SystemError {
        match self {
            Self::System(error) => error.clone(),
            Self::Cancelled(reason) => {
                let message = match reason {
                    CancelReason::User => "Authentication was cancelled by the user",
                    CancelReason::Caller => "Authentication was cancelled by the caller",
                    CancelReason::OwnerDisposed => "Authentication owner was disposed",
                    CancelReason::ProcessPolicy => "Authentication was cancelled by process policy",
                };
                SystemError::new(ErrorCode::Unavailable, message)
            }
            Self::InvalidInput(message) => SystemError::new(ErrorCode::InvalidInput, message),
            Self::InvalidConfiguration(message) => {
                SystemError::new(ErrorCode::InvalidConfiguration, message)
            }
            Self::Unsupported(message) => SystemError::new(ErrorCode::Unsupported, message),
            Self::Unavailable(message) => SystemError::new(ErrorCode::Unavailable, message),
            Self::WrongExecutionOwner(operation) => {
                let _ = operation;
                SystemError::new(
                    ErrorCode::InvalidInput,
                    "Sensitive authentication work requires a background execution owner",
                )
            }
            Self::ContextExpired => {
                SystemError::new(ErrorCode::ContextExpired, "Authentication context expired")
            }
            Self::AlreadyActive => SystemError::new(
                ErrorCode::Busy,
                "An interactive authentication ceremony is already active",
            ),
            Self::Busy(message) => SystemError::new(ErrorCode::Busy, message),
            Self::Expired => {
                SystemError::new(ErrorCode::ContextExpired, "Authentication ceremony expired")
            }
            Self::StaleOrUnknownState => SystemError::new(
                ErrorCode::InvalidInput,
                "Callback state is unknown, stale or already consumed",
            ),
            Self::StaleCeremony => SystemError::new(
                ErrorCode::ResourceExpired,
                "Authentication ceremony was cancelled, disposed or superseded",
            ),
            Self::NoMatchingCredentials => SystemError::new(
                ErrorCode::Unavailable,
                "No matching passkey credential is available",
            ),
            Self::CredentialExcluded => {
                SystemError::new(ErrorCode::Busy, "Passkey credential is already registered")
            }
            Self::UserPresenceRequired => SystemError::new(
                ErrorCode::PermissionDenied,
                "Interactive user presence is required",
            ),
            Self::UserPresenceDenied => SystemError::new(
                ErrorCode::PermissionDenied,
                "Local user presence was not verified",
            ),
            Self::ReauthRequired => {
                let mut error = SystemError::new(
                    ErrorCode::PermissionDenied,
                    "Account requires reauthentication",
                );
                error.recovery = Some(RecoveryAction::Authenticate);
                error.retry = RetryAdvice::AfterConditionsChange;
                error
            }
            Self::NotAuthenticated => {
                SystemError::new(ErrorCode::Unavailable, "Account is not authenticated")
            }
            Self::AccountLimit => {
                SystemError::new(ErrorCode::Busy, "The bounded account registry is full")
            }
            Self::CeremonyLimit => {
                SystemError::new(ErrorCode::Busy, "The bounded ceremony registry is full")
            }
            Self::ProviderFailure(message) => SystemError::new(ErrorCode::PlatformFailure, message),
            Self::VerificationFailed(message) => {
                SystemError::new(ErrorCode::IntegrityFailure, message)
            }
            Self::IntegrityFailure(message) => {
                SystemError::new(ErrorCode::IntegrityFailure, message)
            }
        }
    }
}

impl From<SystemError> for AuthError {
    fn from(error: SystemError) -> Self {
        Self::System(error)
    }
}

impl fmt::Display for AuthError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.to_system_error().message)
    }
}

impl std::error::Error for AuthError {}

/// A bounded remaining lifetime, clamped to the caller's configured maximum.
pub fn remaining_ttl(now: Instant, expires_at: Instant) -> Duration {
    expires_at.saturating_duration_since(now)
}

/// Non-zero ceremony identity scoped to one service process.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CeremonyId(u64);

impl CeremonyId {
    pub fn new(value: u64) -> Result<Self, AuthError> {
        if value == 0 {
            return Err(AuthError::InvalidInput(
                "Ceremony identity must be non-zero",
            ));
        }
        Ok(Self(value))
    }

    pub const fn get(self) -> u64 {
        self.0
    }
}
