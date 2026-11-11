//! Passkey / WebAuthn creation and assertion contracts.
//!
//! This module models browser/platform ceremonies and the explicit server
//! verification boundary. A provider ceremony is never trusted on its own: the
//! service always calls [`PasskeyServerVerifier`] on the background owner
//! before publishing a credential or account session.

use crate::pkce::base64url_encode;
use crate::url::validate_rp_id;
use crate::{
    AccountId, AccountLabel, AccountSessionSummary, AuthError, AuthOrigin, CeremonyId,
    IssuedAccountTokens, PresentationContext, ProviderId, SecretTokenMaterial,
};
use gpui_system::OperationContext;
use std::{
    fmt,
    hint::black_box,
    sync::Mutex,
    time::{Duration, Instant},
};

/// Default passkey ceremony lifetime.
pub const DEFAULT_PASSKEY_TIMEOUT: Duration = Duration::from_secs(120);
/// Maximum passkey ceremony lifetime.
pub const MAX_PASSKEY_TIMEOUT: Duration = Duration::from_secs(600);
/// Maximum number of algorithms/exclusions accepted in one request.
pub const MAX_CREDENTIAL_LIST: usize = 16;

/// A strict WebAuthn relying-party identifier.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RelyingPartyId(String);

impl RelyingPartyId {
    pub fn new(value: impl Into<String>) -> Result<Self, AuthError> {
        let value = value.into();
        if !validate_rp_id(&value) {
            return Err(AuthError::InvalidInput(
                "Relying party identifier must be a lowercase ASCII domain without a port, path or IP literal",
            ));
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn matches_origin(&self, origin: &AuthOrigin) -> bool {
        origin.matches_rp_id(&self.0)
    }
}

impl fmt::Debug for RelyingPartyId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_tuple("RelyingPartyId")
            .field(&self.0)
            .finish()
    }
}

/// A bounded WebAuthn challenge. Single-use semantics are enforced by the
/// service's ceremony/challenge registry, not by the value type.
#[derive(Clone, PartialEq, Eq)]
pub struct PasskeyChallenge(Vec<u8>);

impl PasskeyChallenge {
    pub fn new(bytes: impl Into<Vec<u8>>) -> Result<Self, AuthError> {
        let bytes = bytes.into();
        if !(16..=128).contains(&bytes.len()) {
            return Err(AuthError::InvalidInput(
                "Passkey challenge must be 16..=128 bytes",
            ));
        }
        Ok(Self(bytes))
    }

    pub fn expose(&self) -> &[u8] {
        &self.0
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        false
    }
}

impl fmt::Debug for PasskeyChallenge {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PasskeyChallenge")
            .field("length", &self.0.len())
            .finish()
    }
}

/// Opaque WebAuthn user handle (1..=64 bytes).
#[derive(PartialEq, Eq)]
pub struct PasskeyUserHandle(Vec<u8>);

impl PasskeyUserHandle {
    pub fn new(bytes: impl Into<Vec<u8>>) -> Result<Self, AuthError> {
        let bytes = bytes.into();
        if !(1..=64).contains(&bytes.len()) {
            return Err(AuthError::InvalidInput(
                "Passkey user handle must be 1..=64 bytes",
            ));
        }
        Ok(Self(bytes))
    }

    pub fn expose(&self) -> &[u8] {
        &self.0
    }
}

impl fmt::Debug for PasskeyUserHandle {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("PasskeyUserHandle([REDACTED])")
    }
}

impl Drop for PasskeyUserHandle {
    fn drop(&mut self) {
        self.0.fill(0);
        black_box(&self.0);
    }
}

/// A bounded WebAuthn credential identifier.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CredentialId(Vec<u8>);

impl CredentialId {
    pub fn new(bytes: impl Into<Vec<u8>>) -> Result<Self, AuthError> {
        let bytes = bytes.into();
        if !(1..=1023).contains(&bytes.len()) {
            return Err(AuthError::InvalidInput(
                "Credential identifier must be 1..=1023 bytes",
            ));
        }
        Ok(Self(bytes))
    }

    pub fn from_base64url(value: &str) -> Result<Self, AuthError> {
        let decoded = decode_base64url(value).ok_or(AuthError::InvalidInput(
            "Credential identifier is malformed",
        ))?;
        Self::new(decoded)
    }

    pub fn expose(&self) -> &[u8] {
        &self.0
    }

    pub fn to_base64url(&self) -> String {
        base64url_encode(&self.0)
    }
}

impl fmt::Debug for CredentialId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_tuple("CredentialId")
            .field(&self.to_base64url())
            .finish()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum AuthenticatorTransport {
    Usb,
    Nfc,
    Ble,
    Hybrid,
    Internal,
}

impl AuthenticatorTransport {
    pub const ALL: [Self; 5] = [
        Self::Usb,
        Self::Nfc,
        Self::Ble,
        Self::Hybrid,
        Self::Internal,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Usb => "usb",
            Self::Nfc => "nfc",
            Self::Ble => "ble",
            Self::Hybrid => "hybrid",
            Self::Internal => "internal",
        }
    }
}

/// A credential descriptor used by `exclude_credentials` / `allow_credentials`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CredentialDescriptor {
    pub id: CredentialId,
    pub transports: Vec<AuthenticatorTransport>,
}

impl CredentialDescriptor {
    pub fn new(
        id: CredentialId,
        transports: impl IntoIterator<Item = AuthenticatorTransport>,
    ) -> Self {
        let mut values = Vec::new();
        for transport in transports {
            if !values.contains(&transport) {
                values.push(transport);
            }
        }
        Self {
            id,
            transports: values,
        }
    }
}

/// WebAuthn COSE signature algorithm identifiers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum PublicKeyCoseAlgorithm {
    Es256,
    EdDsa,
    Rs256,
    Es384,
    Rs384,
    Es512,
    Rs512,
}

impl PublicKeyCoseAlgorithm {
    pub const DEFAULT: [Self; 2] = [Self::Es256, Self::Rs256];

    pub const fn as_i32(self) -> i32 {
        match self {
            Self::Es256 => -7,
            Self::EdDsa => -8,
            Self::Es384 => -35,
            Self::Rs256 => -257,
            Self::Rs384 => -258,
            Self::Rs512 => -259,
            Self::Es512 => -36,
        }
    }

    pub const fn from_i32(value: i32) -> Option<Self> {
        match value {
            -7 => Some(Self::Es256),
            -8 => Some(Self::EdDsa),
            -35 => Some(Self::Es384),
            -36 => Some(Self::Es512),
            -257 => Some(Self::Rs256),
            -258 => Some(Self::Rs384),
            -259 => Some(Self::Rs512),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ResidentKeyRequirement {
    Required,
    Preferred,
    Discouraged,
}

impl ResidentKeyRequirement {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Required => "required",
            Self::Preferred => "preferred",
            Self::Discouraged => "discouraged",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum UserVerificationRequirement {
    Required,
    Preferred,
    Discouraged,
}

impl UserVerificationRequirement {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Required => "required",
            Self::Preferred => "preferred",
            Self::Discouraged => "discouraged",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AttestationConveyancePreference {
    None,
    Indirect,
    Direct,
    Enterprise,
}

impl AttestationConveyancePreference {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Indirect => "indirect",
            Self::Direct => "direct",
            Self::Enterprise => "enterprise",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PasskeyMediation {
    Modal,
    Conditional,
}

impl PasskeyMediation {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Modal => "modal",
            Self::Conditional => "conditional",
        }
    }

    pub const fn is_conditional(self) -> bool {
        matches!(self, Self::Conditional)
    }
}

fn validate_algorithms(
    algorithms: impl IntoIterator<Item = PublicKeyCoseAlgorithm>,
) -> Result<Vec<PublicKeyCoseAlgorithm>, AuthError> {
    let mut values = Vec::new();
    for algorithm in algorithms {
        if !values.contains(&algorithm) {
            values.push(algorithm);
        }
    }
    if values.is_empty() || values.len() > 8 {
        return Err(AuthError::InvalidInput(
            "At least one and at most eight signature algorithms are required",
        ));
    }
    Ok(values)
}

fn validate_credentials(
    credentials: impl IntoIterator<Item = CredentialDescriptor>,
) -> Result<Vec<CredentialDescriptor>, AuthError> {
    let mut values: Vec<CredentialDescriptor> = Vec::new();
    for credential in credentials {
        if values.len() == MAX_CREDENTIAL_LIST {
            return Err(AuthError::InvalidInput(
                "Credential descriptor list exceeds the bound",
            ));
        }
        if !values.iter().any(|existing| existing.id == credential.id) {
            values.push(credential);
        }
    }
    Ok(values)
}

/// Validated passkey registration request.
pub struct PasskeyCreationRequest {
    rp_id: RelyingPartyId,
    origin: Option<AuthOrigin>,
    challenge: PasskeyChallenge,
    expires_at: Instant,
    presentation: PresentationContext,
    user_handle: PasskeyUserHandle,
    user_label: AccountLabel,
    algorithms: Vec<PublicKeyCoseAlgorithm>,
    exclude_credentials: Vec<CredentialDescriptor>,
    resident_key: ResidentKeyRequirement,
    user_verification: UserVerificationRequirement,
    attestation: AttestationConveyancePreference,
    mediation: PasskeyMediation,
}

impl PasskeyCreationRequest {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        rp_id: RelyingPartyId,
        origin: Option<AuthOrigin>,
        challenge: PasskeyChallenge,
        expires_at: Instant,
        presentation: PresentationContext,
        user_handle: PasskeyUserHandle,
        user_label: AccountLabel,
        algorithms: impl IntoIterator<Item = PublicKeyCoseAlgorithm>,
    ) -> Result<Self, AuthError> {
        if let Some(origin) = &origin
            && !rp_id.matches_origin(origin)
        {
            return Err(AuthError::InvalidInput(
                "Relying party identifier does not match the Web origin",
            ));
        }
        Ok(Self {
            rp_id,
            origin,
            challenge,
            expires_at,
            presentation,
            user_handle,
            user_label,
            algorithms: validate_algorithms(algorithms)?,
            exclude_credentials: Vec::new(),
            resident_key: ResidentKeyRequirement::Preferred,
            user_verification: UserVerificationRequirement::Required,
            attestation: AttestationConveyancePreference::None,
            mediation: PasskeyMediation::Modal,
        })
    }

    pub fn with_exclude_credentials(
        mut self,
        credentials: impl IntoIterator<Item = CredentialDescriptor>,
    ) -> Result<Self, AuthError> {
        self.exclude_credentials = validate_credentials(credentials)?;
        Ok(self)
    }

    pub fn with_requirements(
        mut self,
        resident_key: ResidentKeyRequirement,
        user_verification: UserVerificationRequirement,
        attestation: AttestationConveyancePreference,
    ) -> Self {
        self.resident_key = resident_key;
        self.user_verification = user_verification;
        self.attestation = attestation;
        self
    }

    pub fn with_mediation(mut self, mediation: PasskeyMediation) -> Self {
        self.mediation = mediation;
        self
    }

    pub fn rp_id(&self) -> &RelyingPartyId {
        &self.rp_id
    }

    pub fn origin(&self) -> Option<&AuthOrigin> {
        self.origin.as_ref()
    }

    pub fn challenge(&self) -> &PasskeyChallenge {
        &self.challenge
    }

    pub const fn expires_at(&self) -> Instant {
        self.expires_at
    }

    pub const fn presentation(&self) -> PresentationContext {
        self.presentation
    }

    pub fn user_handle(&self) -> &PasskeyUserHandle {
        &self.user_handle
    }

    pub fn user_label(&self) -> &AccountLabel {
        &self.user_label
    }

    pub fn algorithms(&self) -> &[PublicKeyCoseAlgorithm] {
        &self.algorithms
    }

    pub fn exclude_credentials(&self) -> &[CredentialDescriptor] {
        &self.exclude_credentials
    }

    pub const fn resident_key(&self) -> ResidentKeyRequirement {
        self.resident_key
    }

    pub const fn user_verification(&self) -> UserVerificationRequirement {
        self.user_verification
    }

    pub const fn attestation(&self) -> AttestationConveyancePreference {
        self.attestation
    }

    pub const fn mediation(&self) -> PasskeyMediation {
        self.mediation
    }
}

/// Validated passkey assertion request.
pub struct PasskeyAssertionRequest {
    rp_id: RelyingPartyId,
    origin: Option<AuthOrigin>,
    challenge: PasskeyChallenge,
    expires_at: Instant,
    presentation: PresentationContext,
    allow_credentials: Vec<CredentialDescriptor>,
    user_verification: UserVerificationRequirement,
    mediation: PasskeyMediation,
    expected_account: Option<AccountId>,
}

impl PasskeyAssertionRequest {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        rp_id: RelyingPartyId,
        origin: Option<AuthOrigin>,
        challenge: PasskeyChallenge,
        expires_at: Instant,
        presentation: PresentationContext,
    ) -> Result<Self, AuthError> {
        if let Some(origin) = &origin
            && !rp_id.matches_origin(origin)
        {
            return Err(AuthError::InvalidInput(
                "Relying party identifier does not match the Web origin",
            ));
        }
        Ok(Self {
            rp_id,
            origin,
            challenge,
            expires_at,
            presentation,
            allow_credentials: Vec::new(),
            user_verification: UserVerificationRequirement::Required,
            mediation: PasskeyMediation::Modal,
            expected_account: None,
        })
    }

    pub fn with_allow_credentials(
        mut self,
        credentials: impl IntoIterator<Item = CredentialDescriptor>,
    ) -> Result<Self, AuthError> {
        self.allow_credentials = validate_credentials(credentials)?;
        Ok(self)
    }

    pub fn with_user_verification(mut self, requirement: UserVerificationRequirement) -> Self {
        self.user_verification = requirement;
        self
    }

    pub fn with_mediation(mut self, mediation: PasskeyMediation) -> Self {
        self.mediation = mediation;
        self
    }

    pub fn with_expected_account(mut self, account_id: AccountId) -> Self {
        self.expected_account = Some(account_id);
        self
    }

    pub fn rp_id(&self) -> &RelyingPartyId {
        &self.rp_id
    }

    pub fn origin(&self) -> Option<&AuthOrigin> {
        self.origin.as_ref()
    }

    pub fn challenge(&self) -> &PasskeyChallenge {
        &self.challenge
    }

    pub const fn expires_at(&self) -> Instant {
        self.expires_at
    }

    pub const fn presentation(&self) -> PresentationContext {
        self.presentation
    }

    pub fn allow_credentials(&self) -> &[CredentialDescriptor] {
        &self.allow_credentials
    }

    pub const fn user_verification(&self) -> UserVerificationRequirement {
        self.user_verification
    }

    pub const fn mediation(&self) -> PasskeyMediation {
        self.mediation
    }

    pub fn expected_account(&self) -> Option<&AccountId> {
        self.expected_account.as_ref()
    }
}

/// Opaque attestation/assertion response bytes. Never formatted.
pub struct ProviderResponseBytes(Vec<u8>);

impl ProviderResponseBytes {
    pub fn new(bytes: impl Into<Vec<u8>>) -> Result<Self, AuthError> {
        let bytes = bytes.into();
        if bytes.is_empty() || bytes.len() > 8192 {
            return Err(AuthError::InvalidInput(
                "Authenticator response payload is empty or oversized",
            ));
        }
        Ok(Self(bytes))
    }

    pub fn expose(&self) -> &[u8] {
        &self.0
    }
}

impl fmt::Debug for ProviderResponseBytes {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("ProviderResponseBytes([REDACTED])")
    }
}

impl Drop for ProviderResponseBytes {
    fn drop(&mut self) {
        self.0.fill(0);
        black_box(&self.0);
    }
}

/// Provider-facing registration input.
pub struct ProviderCreationRequest {
    pub ceremony_id: CeremonyId,
    pub rp_id: RelyingPartyId,
    pub origin: Option<AuthOrigin>,
    pub challenge: Vec<u8>,
    pub user_handle: PasskeyUserHandle,
    pub user_label: AccountLabel,
    pub algorithms: Vec<PublicKeyCoseAlgorithm>,
    pub exclude_credentials: Vec<CredentialDescriptor>,
    pub resident_key: ResidentKeyRequirement,
    pub user_verification: UserVerificationRequirement,
    pub attestation: AttestationConveyancePreference,
    pub mediation: PasskeyMediation,
    pub now: Instant,
}

impl fmt::Debug for ProviderCreationRequest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ProviderCreationRequest")
            .field("ceremony_id", &self.ceremony_id)
            .field("rp_id", &self.rp_id)
            .field("challenge", &"[REDACTED]")
            .field("user_handle", &"[REDACTED]")
            .field("algorithm_count", &self.algorithms.len())
            .field("exclude_count", &self.exclude_credentials.len())
            .field("mediation", &self.mediation)
            .finish_non_exhaustive()
    }
}

/// Provider-facing assertion input.
pub struct ProviderAssertionRequest {
    pub ceremony_id: CeremonyId,
    pub rp_id: RelyingPartyId,
    pub origin: Option<AuthOrigin>,
    pub challenge: Vec<u8>,
    pub allow_credentials: Vec<CredentialDescriptor>,
    pub user_verification: UserVerificationRequirement,
    pub mediation: PasskeyMediation,
    pub now: Instant,
}

impl fmt::Debug for ProviderAssertionRequest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ProviderAssertionRequest")
            .field("ceremony_id", &self.ceremony_id)
            .field("rp_id", &self.rp_id)
            .field("challenge", &"[REDACTED]")
            .field("allow_count", &self.allow_credentials.len())
            .field("mediation", &self.mediation)
            .finish_non_exhaustive()
    }
}

/// Raw registration output from a native/browser provider.
pub struct ProviderPasskeyRegistration {
    pub ceremony_id: CeremonyId,
    pub credential_id: CredentialId,
    pub rp_id: RelyingPartyId,
    pub client_data_json: ProviderResponseBytes,
    pub attestation_object: ProviderResponseBytes,
    pub transports: Vec<AuthenticatorTransport>,
    pub user_verified: bool,
    pub backup_eligible: bool,
    pub backup_state: bool,
    pub sign_count: u32,
}

impl fmt::Debug for ProviderPasskeyRegistration {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ProviderPasskeyRegistration")
            .field("ceremony_id", &self.ceremony_id)
            .field("credential_id", &self.credential_id)
            .field("rp_id", &self.rp_id)
            .field("client_data_json", &"[REDACTED]")
            .field("attestation_object", &"[REDACTED]")
            .field("transports", &self.transports)
            .field("user_verified", &self.user_verified)
            .field("sign_count", &self.sign_count)
            .finish()
    }
}

/// Raw assertion output from a native/browser provider.
pub struct ProviderPasskeyAssertion {
    pub ceremony_id: CeremonyId,
    pub credential_id: CredentialId,
    pub rp_id: RelyingPartyId,
    pub client_data_json: ProviderResponseBytes,
    pub authenticator_data: ProviderResponseBytes,
    pub signature: ProviderResponseBytes,
    pub user_handle: Option<PasskeyUserHandle>,
    pub user_verified: bool,
    pub sign_count: u32,
}

impl fmt::Debug for ProviderPasskeyAssertion {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ProviderPasskeyAssertion")
            .field("ceremony_id", &self.ceremony_id)
            .field("credential_id", &self.credential_id)
            .field("rp_id", &self.rp_id)
            .field("client_data_json", &"[REDACTED]")
            .field("authenticator_data", &"[REDACTED]")
            .field("signature", &"[REDACTED]")
            .field(
                "user_handle",
                &self.user_handle.as_ref().map(|_| "[REDACTED]"),
            )
            .field("user_verified", &self.user_verified)
            .field("sign_count", &self.sign_count)
            .finish()
    }
}

/// Deterministic, value-free provider errors.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PasskeyProviderError {
    Cancelled,
    NoMatchingCredentials,
    CredentialExcluded,
    AlreadyExists,
    Unavailable(&'static str),
    Failed(&'static str),
    InvalidState,
}

impl PasskeyProviderError {
    pub fn to_auth_error(self) -> AuthError {
        match self {
            Self::Cancelled => AuthError::Cancelled(gpui_system::CancelReason::User),
            Self::NoMatchingCredentials => AuthError::NoMatchingCredentials,
            Self::CredentialExcluded | Self::AlreadyExists => AuthError::CredentialExcluded,
            Self::Unavailable(message) => AuthError::Unavailable(message),
            Self::Failed(message) => AuthError::ProviderFailure(message),
            Self::InvalidState => AuthError::StaleCeremony,
        }
    }
}

/// Native/browser passkey ceremony boundary. Calls must occur outside service
/// locks and are never treated as server verification.
pub trait PasskeyProvider: Send + Sync {
    fn create(
        &self,
        request: ProviderCreationRequest,
        context: &OperationContext,
    ) -> Result<ProviderPasskeyRegistration, PasskeyProviderError>;

    fn assert(
        &self,
        request: ProviderAssertionRequest,
        context: &OperationContext,
    ) -> Result<ProviderPasskeyAssertion, PasskeyProviderError>;

    fn cancel(&self, _ceremony_id: CeremonyId) -> Result<(), PasskeyProviderError> {
        Ok(())
    }
}

/// Registration material handed to the server verifier.
pub struct RegistrationVerificationRequest {
    pub ceremony_id: CeremonyId,
    pub rp_id: RelyingPartyId,
    pub origin: Option<AuthOrigin>,
    pub challenge: PasskeyChallenge,
    pub registration: ProviderPasskeyRegistration,
    pub now: Instant,
}

impl fmt::Debug for RegistrationVerificationRequest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RegistrationVerificationRequest")
            .field("ceremony_id", &self.ceremony_id)
            .field("rp_id", &self.rp_id)
            .field("challenge", &"[REDACTED]")
            .field("registration", &self.registration)
            .finish_non_exhaustive()
    }
}

/// Assertion material handed to the server verifier.
pub struct AssertionVerificationRequest {
    pub ceremony_id: CeremonyId,
    pub rp_id: RelyingPartyId,
    pub origin: Option<AuthOrigin>,
    pub challenge: PasskeyChallenge,
    pub assertion: ProviderPasskeyAssertion,
    pub allow_credentials: Vec<CredentialDescriptor>,
    pub expected_account: Option<AccountId>,
    pub now: Instant,
}

impl fmt::Debug for AssertionVerificationRequest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AssertionVerificationRequest")
            .field("ceremony_id", &self.ceremony_id)
            .field("rp_id", &self.rp_id)
            .field("challenge", &"[REDACTED]")
            .field("assertion", &self.assertion)
            .finish_non_exhaustive()
    }
}

/// Server-verified registration. Everything below is trusted metadata, not a
/// provider claim.
#[derive(Debug)]
pub struct VerifiedRegistration {
    pub credential_id: CredentialId,
    pub rp_id: RelyingPartyId,
    pub sign_count: u32,
    pub backup_eligible: bool,
    pub backup_state: bool,
    pub user_verified: bool,
    pub transports: Vec<AuthenticatorTransport>,
    pub session: Option<IssuedPasskeySession>,
}

/// Server-verified assertion.
#[derive(Debug)]
pub struct VerifiedAssertion {
    pub credential_id: CredentialId,
    pub rp_id: RelyingPartyId,
    pub sign_count: u32,
    pub user_verified: bool,
    pub account_id: AccountId,
    pub session: Option<IssuedPasskeySession>,
}

/// Optional session establishment performed by the verifier after a passkey
/// operation. Token fields still zeroize and redact.
pub struct IssuedPasskeySession {
    pub account_id: AccountId,
    pub tokens: IssuedAccountTokens,
}

impl fmt::Debug for IssuedPasskeySession {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("IssuedPasskeySession")
            .field("account_id", &self.account_id)
            .field("tokens", &"[REDACTED]")
            .finish()
    }
}

/// Server verification failure. No raw assertion data is retained.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PasskeyVerificationError {
    InvalidChallenge,
    InvalidOrigin,
    RelyingPartyMismatch,
    UserVerificationRequired,
    SignatureInvalid,
    CredentialNotAllowed,
    Replay,
    Unavailable(&'static str),
    Rejected(&'static str),
}

impl PasskeyVerificationError {
    pub fn to_auth_error(self) -> AuthError {
        match self {
            Self::InvalidChallenge => {
                AuthError::VerificationFailed("Passkey challenge does not match")
            }
            Self::InvalidOrigin => AuthError::VerificationFailed("Passkey origin is not trusted"),
            Self::RelyingPartyMismatch => {
                AuthError::VerificationFailed("Passkey relying party does not match")
            }
            Self::UserVerificationRequired => AuthError::UserPresenceRequired,
            Self::SignatureInvalid => {
                AuthError::VerificationFailed("Passkey assertion signature is invalid")
            }
            Self::CredentialNotAllowed => {
                AuthError::VerificationFailed("Passkey credential is not allowed")
            }
            Self::Replay => AuthError::VerificationFailed("Passkey response was replayed"),
            Self::Unavailable(message) => AuthError::Unavailable(message),
            Self::Rejected(message) => AuthError::VerificationFailed(message),
        }
    }
}

/// Explicit WebAuthn server verification boundary. Invoked on the background
/// owner and outside all service locks.
pub trait PasskeyServerVerifier: Send + Sync {
    fn verify_registration(
        &self,
        request: RegistrationVerificationRequest,
    ) -> Result<VerifiedRegistration, PasskeyVerificationError>;

    fn verify_assertion(
        &self,
        request: AssertionVerificationRequest,
    ) -> Result<VerifiedAssertion, PasskeyVerificationError>;
}

/// Public-safe registration result. No attestation object or client data.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PasskeyRegistrationResult {
    pub credential_id: CredentialId,
    pub rp_id: RelyingPartyId,
    pub sign_count: u32,
    pub backup_eligible: bool,
    pub backup_state: bool,
    pub user_verified: bool,
    pub transports: Vec<AuthenticatorTransport>,
    pub session: Option<AccountSessionSummary>,
}

/// Public-safe assertion result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PasskeyAssertionResult {
    pub credential_id: CredentialId,
    pub rp_id: RelyingPartyId,
    pub sign_count: u32,
    pub user_verified: bool,
    pub account_id: AccountId,
    pub session: Option<AccountSessionSummary>,
}

struct FakeProviderState {
    next_credential: u64,
    fail: Option<PasskeyProviderError>,
    mismatch_rp: bool,
    mismatch_credential: bool,
    user_verified: bool,
    credential_id: Option<CredentialId>,
    creations: usize,
    assertions: usize,
    cancelled: Vec<CeremonyId>,
    last_challenge: Option<Vec<u8>>,
    last_allow: Vec<CredentialId>,
}

/// Deterministic passkey provider for tests. It never verifies anything.
pub struct FakePasskeyProvider {
    state: Mutex<FakeProviderState>,
}

impl FakePasskeyProvider {
    pub fn new() -> Self {
        Self {
            state: Mutex::new(FakeProviderState {
                next_credential: 1,
                fail: None,
                mismatch_rp: false,
                mismatch_credential: false,
                user_verified: true,
                credential_id: None,
                creations: 0,
                assertions: 0,
                cancelled: Vec::new(),
                last_challenge: None,
                last_allow: Vec::new(),
            }),
        }
    }

    pub fn fail_next(&self, error: PasskeyProviderError) {
        lock(&self.state).fail = Some(error);
    }

    pub fn mismatch_rp_next(&self) {
        lock(&self.state).mismatch_rp = true;
    }

    pub fn user_verified_next(&self, verified: bool) {
        lock(&self.state).user_verified = verified;
    }

    pub fn mismatch_credential_next(&self) {
        lock(&self.state).mismatch_credential = true;
    }

    pub fn set_credential_id(&self, credential_id: CredentialId) {
        lock(&self.state).credential_id = Some(credential_id);
    }

    pub fn creations(&self) -> usize {
        lock(&self.state).creations
    }

    pub fn assertions(&self) -> usize {
        lock(&self.state).assertions
    }

    pub fn cancelled(&self) -> Vec<CeremonyId> {
        lock(&self.state).cancelled.clone()
    }

    pub fn last_challenge(&self) -> Option<Vec<u8>> {
        lock(&self.state).last_challenge.clone()
    }

    pub fn last_allow(&self) -> Vec<CredentialId> {
        lock(&self.state).last_allow.clone()
    }
}

impl Default for FakePasskeyProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl PasskeyProvider for FakePasskeyProvider {
    fn create(
        &self,
        request: ProviderCreationRequest,
        _context: &OperationContext,
    ) -> Result<ProviderPasskeyRegistration, PasskeyProviderError> {
        let mut state = lock(&self.state);
        state.creations += 1;
        state.last_challenge = Some(request.challenge.clone());
        if let Some(error) = state.fail.take() {
            return Err(error);
        }
        let rp_id = if state.mismatch_rp {
            state.mismatch_rp = false;
            RelyingPartyId::new("attacker.example").expect("static rp id")
        } else {
            request.rp_id
        };
        let credential_id = state.credential_id.clone().unwrap_or_else(|| {
            let id =
                CredentialId::new(format!("credential-{}", state.next_credential).into_bytes())
                    .expect("static credential id is valid");
            state.next_credential += 1;
            id
        });
        Ok(ProviderPasskeyRegistration {
            ceremony_id: request.ceremony_id,
            credential_id,
            rp_id,
            client_data_json: ProviderResponseBytes::new(b"client-data".to_vec()).expect("static"),
            attestation_object: ProviderResponseBytes::new(b"attestation".to_vec())
                .expect("static"),
            transports: vec![
                AuthenticatorTransport::Internal,
                AuthenticatorTransport::Hybrid,
            ],
            user_verified: state.user_verified,
            backup_eligible: true,
            backup_state: false,
            sign_count: 0,
        })
    }

    fn assert(
        &self,
        request: ProviderAssertionRequest,
        _context: &OperationContext,
    ) -> Result<ProviderPasskeyAssertion, PasskeyProviderError> {
        let mut state = lock(&self.state);
        state.assertions += 1;
        state.last_challenge = Some(request.challenge.clone());
        state.last_allow = request
            .allow_credentials
            .iter()
            .map(|credential| credential.id.clone())
            .collect();
        if let Some(error) = state.fail.take() {
            return Err(error);
        }
        let rp_id = if state.mismatch_rp {
            state.mismatch_rp = false;
            RelyingPartyId::new("attacker.example").expect("static rp id")
        } else {
            request.rp_id
        };
        let credential_id = if state.mismatch_credential {
            state.mismatch_credential = false;
            CredentialId::new(b"unlisted-credential".to_vec()).expect("static credential id")
        } else {
            request
                .allow_credentials
                .first()
                .map(|credential| credential.id.clone())
                .or_else(|| state.credential_id.clone())
                .unwrap_or_else(|| {
                    CredentialId::new(b"credential-1".to_vec()).expect("static credential id")
                })
        };
        Ok(ProviderPasskeyAssertion {
            ceremony_id: request.ceremony_id,
            credential_id,
            rp_id,
            client_data_json: ProviderResponseBytes::new(b"client-data".to_vec()).expect("static"),
            authenticator_data: ProviderResponseBytes::new(b"authenticator-data".to_vec())
                .expect("static"),
            signature: ProviderResponseBytes::new(b"signature".to_vec()).expect("static"),
            user_handle: None,
            user_verified: state.user_verified,
            sign_count: 7,
        })
    }

    fn cancel(&self, ceremony_id: CeremonyId) -> Result<(), PasskeyProviderError> {
        lock(&self.state).cancelled.push(ceremony_id);
        Ok(())
    }
}

impl fmt::Debug for FakePasskeyProvider {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let state = lock(&self.state);
        formatter
            .debug_struct("FakePasskeyProvider")
            .field("creations", &state.creations)
            .field("assertions", &state.assertions)
            .finish()
    }
}

struct FakeVerifierState {
    provider: ProviderId,
    account: Option<AccountId>,
    fail_registration: Option<PasskeyVerificationError>,
    fail_assertion: Option<PasskeyVerificationError>,
    registrations: usize,
    assertions: usize,
    last_challenge: Option<Vec<u8>>,
    issue_session: bool,
}

/// Deterministic server verifier. It is the only test component allowed to
/// declare a credential verified or to attach an account session.
pub struct FakePasskeyServerVerifier {
    state: Mutex<FakeVerifierState>,
}

impl FakePasskeyServerVerifier {
    pub fn new(provider: ProviderId) -> Self {
        Self {
            state: Mutex::new(FakeVerifierState {
                provider,
                account: None,
                fail_registration: None,
                fail_assertion: None,
                registrations: 0,
                assertions: 0,
                last_challenge: None,
                issue_session: false,
            }),
        }
    }

    pub fn with_account(provider: ProviderId, account: AccountId) -> Self {
        let verifier = Self::new(provider);
        lock(&verifier.state).account = Some(account);
        verifier
    }

    pub fn issue_session_next(&self) {
        lock(&self.state).issue_session = true;
    }

    pub fn fail_registration(&self, error: PasskeyVerificationError) {
        lock(&self.state).fail_registration = Some(error);
    }

    pub fn fail_assertion(&self, error: PasskeyVerificationError) {
        lock(&self.state).fail_assertion = Some(error);
    }

    pub fn registrations(&self) -> usize {
        lock(&self.state).registrations
    }

    pub fn assertions(&self) -> usize {
        lock(&self.state).assertions
    }

    pub fn last_challenge(&self) -> Option<Vec<u8>> {
        lock(&self.state).last_challenge.clone()
    }

    fn session(
        provider_id: &ProviderId,
        account: &AccountId,
        now: Instant,
    ) -> IssuedPasskeySession {
        let tokens = IssuedAccountTokens {
            account_id: account.clone(),
            provider_id: provider_id.clone(),
            label: AccountLabel::new("Passkey account").expect("static label"),
            scopes: vec![crate::AuthScope::new("openid").expect("static scope")],
            access_token: SecretTokenMaterial::from_string("passkey-access").expect("static"),
            refresh_token: Some(
                SecretTokenMaterial::from_string("passkey-refresh").expect("static"),
            ),
            id_token: None,
            authenticated_at: now,
            access_expires_at: Some(now + Duration::from_secs(900)),
            refresh_expires_at: Some(now + Duration::from_secs(86_400)),
        };
        IssuedPasskeySession {
            account_id: account.clone(),
            tokens,
        }
    }
}

impl PasskeyServerVerifier for FakePasskeyServerVerifier {
    fn verify_registration(
        &self,
        request: RegistrationVerificationRequest,
    ) -> Result<VerifiedRegistration, PasskeyVerificationError> {
        let mut state = lock(&self.state);
        state.registrations += 1;
        state.last_challenge = Some(request.challenge.expose().to_vec());
        if let Some(error) = state.fail_registration.take() {
            return Err(error);
        }
        if request.registration.ceremony_id != request.ceremony_id
            || request.registration.rp_id != request.rp_id
        {
            return Err(PasskeyVerificationError::RelyingPartyMismatch);
        }
        if !request.registration.user_verified {
            return Err(PasskeyVerificationError::UserVerificationRequired);
        }
        let account = state.account.clone();
        let issue_session = state.issue_session;
        let provider_id = state.provider.clone();
        drop(state);
        let session = account
            .as_ref()
            .filter(|_| issue_session)
            .map(|account| Self::session(&provider_id, account, request.now));
        Ok(VerifiedRegistration {
            credential_id: request.registration.credential_id,
            rp_id: request.registration.rp_id,
            sign_count: request.registration.sign_count,
            backup_eligible: request.registration.backup_eligible,
            backup_state: request.registration.backup_state,
            user_verified: request.registration.user_verified,
            transports: request.registration.transports,
            session,
        })
    }

    fn verify_assertion(
        &self,
        request: AssertionVerificationRequest,
    ) -> Result<VerifiedAssertion, PasskeyVerificationError> {
        let mut state = lock(&self.state);
        state.assertions += 1;
        state.last_challenge = Some(request.challenge.expose().to_vec());
        if let Some(error) = state.fail_assertion.take() {
            return Err(error);
        }
        if request.assertion.ceremony_id != request.ceremony_id
            || request.assertion.rp_id != request.rp_id
        {
            return Err(PasskeyVerificationError::RelyingPartyMismatch);
        }
        if !request.assertion.user_verified {
            return Err(PasskeyVerificationError::UserVerificationRequired);
        }
        if !request.allow_credentials.is_empty()
            && !request
                .allow_credentials
                .iter()
                .any(|credential| credential.id == request.assertion.credential_id)
        {
            return Err(PasskeyVerificationError::CredentialNotAllowed);
        }
        let account = state
            .account
            .clone()
            .ok_or(PasskeyVerificationError::Rejected(
                "No account is bound to this passkey",
            ))?;
        let issue_session = state.issue_session;
        let provider_id = state.provider.clone();
        drop(state);
        let session = issue_session.then(|| Self::session(&provider_id, &account, request.now));
        Ok(VerifiedAssertion {
            credential_id: request.assertion.credential_id,
            rp_id: request.assertion.rp_id,
            sign_count: request.assertion.sign_count,
            user_verified: request.assertion.user_verified,
            account_id: account,
            session,
        })
    }
}

impl fmt::Debug for FakePasskeyServerVerifier {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let state = lock(&self.state);
        formatter
            .debug_struct("FakePasskeyServerVerifier")
            .field("provider", &state.provider)
            .field("account", &state.account)
            .field("registrations", &state.registrations)
            .field("assertions", &state.assertions)
            .finish()
    }
}

/// Verify service-level invariants that no provider/verifier can waive.
pub(crate) fn check_provider_registration(
    request: &PasskeyCreationRequest,
    registration: &ProviderPasskeyRegistration,
    now: Instant,
) -> Result<(), AuthError> {
    if now >= request.expires_at() {
        return Err(AuthError::Expired);
    }
    if registration.rp_id != *request.rp_id() {
        return Err(AuthError::VerificationFailed(
            "Provider registration returned a different relying party",
        ));
    }
    if request.user_verification() == UserVerificationRequirement::Required
        && !registration.user_verified
    {
        return Err(AuthError::UserPresenceRequired);
    }
    if request
        .exclude_credentials()
        .iter()
        .any(|credential| credential.id == registration.credential_id)
    {
        return Err(AuthError::CredentialExcluded);
    }
    Ok(())
}

pub(crate) fn check_provider_assertion(
    request: &PasskeyAssertionRequest,
    assertion: &ProviderPasskeyAssertion,
    now: Instant,
) -> Result<(), AuthError> {
    if now >= request.expires_at() {
        return Err(AuthError::Expired);
    }
    if assertion.rp_id != *request.rp_id() {
        return Err(AuthError::VerificationFailed(
            "Provider assertion returned a different relying party",
        ));
    }
    if request.user_verification() == UserVerificationRequirement::Required
        && !assertion.user_verified
    {
        return Err(AuthError::UserPresenceRequired);
    }
    if !request.allow_credentials().is_empty()
        && !request
            .allow_credentials()
            .iter()
            .any(|credential| credential.id == assertion.credential_id)
    {
        return Err(AuthError::VerificationFailed(
            "Provider assertion returned an unlisted credential",
        ));
    }
    Ok(())
}

/// Decode unpadded base64url for credential identifiers. Kept local so cursor
/// state and bounds are explicit.
fn decode_base64url(value: &str) -> Option<Vec<u8>> {
    let mut output = Vec::with_capacity(value.len() * 3 / 4);
    let mut buffer = 0u32;
    let mut bits = 0u32;
    for byte in value.bytes() {
        let digit = match byte {
            b'A'..=b'Z' => byte - b'A',
            b'a'..=b'z' => byte - b'a' + 26,
            b'0'..=b'9' => byte - b'0' + 52,
            b'-' => 62,
            b'_' => 63,
            _ => return None,
        };
        buffer = (buffer << 6) | u32::from(digit);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            output.push((buffer >> bits) as u8);
            buffer &= (1 << bits) - 1;
        }
    }
    if output.is_empty() || value.len() > 1366 {
        return None;
    }
    Some(output)
}

fn lock<T>(value: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    value
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}
