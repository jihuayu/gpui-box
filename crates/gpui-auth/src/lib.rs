//! Browser account authentication, OAuth 2.0/OIDC S256 PKCE, Passkey/WebAuthn
//! ceremony contracts and `gpui-secrets`-backed account sessions.
//!
//! The default build has no GPUI dependency and installs no native provider.
//! Every row of the six-platform matrix is [`Evidence::ContractOnly`]; native
//! adapters remain separate work under #8, #9, #70 and #74.
//!
//! This crate performs **remote** account authentication only. Local user
//! verification belongs to `gpui-local-auth` (#28): a local biometric or device
//! credential receipt is never an account session, OAuth token, Passkey
//! assertion or server-verified identity. The optional
//! [`LocalUserPresenceGate`] hook models that separation explicitly.
//!
//! See `docs/S_ACCOUNT_AUTH.md`.

#![forbid(unsafe_code)]

mod browser;
mod model;
mod passkey;
mod pkce;
mod platform;
mod redirect;
mod service;
mod url;
mod vault;

pub use browser::{
    AuthBackendError, BrowserAuthHandle, BrowserAuthProvider, BrowserAuthRequest,
    BrowserAuthSessionProvider, BrowserLaunchRequest, BrowserSessionHandle,
    DEFAULT_BROWSER_CEREMONY_TTL, FakeBrowserAuthProvider, FakeTokenExchangeBackend,
    MAX_BROWSER_CEREMONY_TTL, RevocationKind, TokenExchangeBackend, TokenExchangeRequest,
    TokenRefreshRequest, TokenRevocationRequest,
};
pub use model::{
    AccountId, AccountLabel, AccountSessionState, AccountSessionSummary, AuthError, AuthMethodKind,
    AuthScope, AuthScopes, AuthSnapshot, BrowserSessionPreference, CeremonyId, ExecutionOwner,
    IssuedAccountTokens, LocalPresencePurpose, LocalPresenceReceipt, LocalUserPresenceGate,
    MAX_ACCOUNT_ID_LEN, MAX_ACCOUNTS, MAX_ACTIVE_CEREMONIES, MAX_SCOPES, MAX_TOKEN_BYTES,
    OAuthState, OidcNonce, PresentationContext, ProviderId, RefreshedAccountTokens,
    SecretTokenMaterial, TokenLease, remaining_ttl,
};
pub use passkey::{
    AssertionVerificationRequest, AttestationConveyancePreference, AuthenticatorTransport,
    CredentialDescriptor, CredentialId, DEFAULT_PASSKEY_TIMEOUT, FakePasskeyProvider,
    FakePasskeyServerVerifier, IssuedPasskeySession, MAX_CREDENTIAL_LIST, MAX_PASSKEY_TIMEOUT,
    PasskeyAssertionRequest, PasskeyAssertionResult, PasskeyChallenge, PasskeyCreationRequest,
    PasskeyMediation, PasskeyProvider, PasskeyProviderError, PasskeyRegistrationResult,
    PasskeyServerVerifier, PasskeyUserHandle, PasskeyVerificationError, ProviderAssertionRequest,
    ProviderCreationRequest, ProviderPasskeyAssertion, ProviderPasskeyRegistration,
    ProviderResponseBytes, PublicKeyCoseAlgorithm, RegistrationVerificationRequest, RelyingPartyId,
    ResidentKeyRequirement, UserVerificationRequirement, VerifiedAssertion, VerifiedRegistration,
};
pub use pkce::{
    MAX_PKCE_VERIFIER_LEN, MIN_PKCE_VERIFIER_LEN, PKCE_CHALLENGE_LEN, PkceChallenge, PkceError,
    PkcePair, PkceVerifier, base64url_encode, sha256,
};
pub use platform::{
    AUTH_PLATFORM_MANIFEST, AuthOperation, AuthPlatform, AuthPlatformManifest,
    PlatformOperationSupport, SupportLevel, native_api, operation_support, platform_operations,
    platform_support_matrix, prerequisites, support,
};
pub use redirect::{
    CallbackQueryKind, ColdRedirectDescriptor, LoopbackHost, OAuthErrorCode,
    RedirectCaptureOutcome, RedirectTarget, classify_callback_query,
};
pub use service::{
    AuthConfig, AuthEvent, AuthEventBatch, AuthEventCursor, AuthService, AuthServiceProviders,
    DEFAULT_AUTH_EVENT_CAPACITY, MAX_CONSUMED_STATES, ResyncRequired,
};
pub use url::{
    AuthEndpointUrl, AuthOrigin, CallbackUrl, Host, MAX_AUTH_URL_LEN, MAX_REDIRECT_PATH_LEN,
    OriginScheme, UrlError, encode_query_component, validate_rp_id,
};
pub use vault::{
    ColdCeremonyStore, LoadedColdCeremony, SecretKey, SecretService, SecretTokenVault,
};

/// The coordinated upstream GPUI revision used by this distribution.
pub const UPSTREAM_GPUI_REVISION: &str = "f8cfc7ed9e57503def9479138894f3139f5400cc";

/// The gpui-auth package version.
pub const PACKAGE_VERSION: &str = env!("CARGO_PKG_VERSION");

#[cfg(feature = "gpui")]
pub mod gpui_adapter;
