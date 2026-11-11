//! Public local-authentication vocabulary and redacted identifier types.
//!
//! Operation availability, local user presence and authentication-bound key or
//! secret access are deliberately separate. A capability snapshot or a
//! verification receipt is not a durable authorization token.

use gpui_lifecycle::{
    AppPhase, LifecycleState, SessionLockState, SurfaceSnapshot, SurfaceVisibility,
};
use gpui_secrets::SecretBytes;
use gpui_system::{
    DiagnosticPhase, ErrorCode, RecoveryAction, RetryAdvice, SurfaceId, SystemError,
};
use std::{fmt, time::Instant};

/// Maximum UTF-8 byte length of a verification purpose.
pub const MAX_PURPOSE_BYTES: usize = 256;
/// Maximum UTF-8 byte length of an account identifier.
pub const MAX_ACCOUNT_BYTES: usize = 128;
/// Maximum UTF-8 byte length of a protected-key identifier.
pub const MAX_KEY_ID_BYTES: usize = 128;
/// Maximum challenge size accepted for protected-key signing.
pub const MAX_CHALLENGE_BYTES: usize = 4096;

/// The local factor a provider may use for user verification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum VerificationPolicy {
    /// Only a biometric factor is acceptable.
    BiometricOnly,
    /// Biometrics or the operating system's device-credential fallback.
    BiometricOrDeviceCredential,
}

impl VerificationPolicy {
    /// Whether this policy permits an OS device-credential fallback.
    pub const fn allows_device_credential(self) -> bool {
        matches!(self, Self::BiometricOrDeviceCredential)
    }
}

/// The factor actually used by a successful verification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum VerifiedFactor {
    Biometric,
    DeviceCredentialFallback,
}

/// A validated, redacted purpose shown to the user during verification.
///
/// Purpose text is trimmed, nonempty, at most [`MAX_PURPOSE_BYTES`] UTF-8 bytes
/// and control-character free. `Debug` never prints the text.
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct VerificationPurpose(String);

impl VerificationPurpose {
    pub fn new(value: impl Into<String>) -> Result<Self, SystemError> {
        let value = value.into();
        let trimmed = value.trim();
        validate_identifier(
            trimmed,
            MAX_PURPOSE_BYTES,
            "Verification purpose must be nonempty, bounded and control-character free",
        )?;
        Ok(Self(trimmed.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for VerificationPurpose {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("VerificationPurpose([REDACTED])")
    }
}

/// A validated account identity. `Debug` is redacted.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct AccountId(String);

impl AccountId {
    pub fn new(value: impl Into<String>) -> Result<Self, SystemError> {
        let value = value.into();
        validate_identifier(
            &value,
            MAX_ACCOUNT_BYTES,
            "Account identifier must be nonempty, bounded and control-character free",
        )?;
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for AccountId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("AccountId([REDACTED])")
    }
}

/// A validated protected-key identity. `Debug` is redacted.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ProtectedKeyId(String);

impl ProtectedKeyId {
    pub fn new(value: impl Into<String>) -> Result<Self, SystemError> {
        let value = value.into();
        validate_identifier(
            &value,
            MAX_KEY_ID_BYTES,
            "Protected-key identifier must be nonempty, bounded and control-character free",
        )?;
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for ProtectedKeyId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("ProtectedKeyId([REDACTED])")
    }
}

/// Bounded challenge bytes for authentication-bound signing.
#[derive(Clone, PartialEq, Eq)]
pub struct ChallengeBytes(Vec<u8>);

impl ChallengeBytes {
    pub fn new(bytes: impl Into<Vec<u8>>) -> Result<Self, SystemError> {
        let bytes = bytes.into();
        if bytes.is_empty() || bytes.len() > MAX_CHALLENGE_BYTES {
            return Err(invalid_input(
                "Challenge must contain between 1 and 4096 bytes",
            ));
        }
        Ok(Self(bytes))
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl fmt::Debug for ChallengeBytes {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ChallengeBytes")
            .field("len", &self.0.len())
            .finish()
    }
}

/// Supported asymmetric algorithms for protected-key signing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SignatureAlgorithm {
    EcdsaP256Sha256,
    Ed25519,
}

/// One independently published local-auth operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum LocalAuthOperation {
    /// Non-interactive readiness probe; never displays a native prompt.
    QueryAvailability(VerificationPolicy),
    /// Interactive local user presence verification.
    VerifyUser(VerificationPolicy),
    /// Authentication-bound release of a protected secret.
    UnlockProtectedSecret,
    /// Authentication-bound signature with a protected key.
    SignChallenge,
}

impl LocalAuthOperation {
    pub const ALL: [Self; 6] = [
        Self::QueryAvailability(VerificationPolicy::BiometricOnly),
        Self::QueryAvailability(VerificationPolicy::BiometricOrDeviceCredential),
        Self::VerifyUser(VerificationPolicy::BiometricOnly),
        Self::VerifyUser(VerificationPolicy::BiometricOrDeviceCredential),
        Self::UnlockProtectedSecret,
        Self::SignChallenge,
    ];

    pub const fn is_interactive(self) -> bool {
        matches!(
            self,
            Self::VerifyUser(_) | Self::UnlockProtectedSecret | Self::SignChallenge
        )
    }

    pub const fn policy(self) -> Option<VerificationPolicy> {
        match self {
            Self::QueryAvailability(policy) | Self::VerifyUser(policy) => Some(policy),
            Self::UnlockProtectedSecret | Self::SignChallenge => None,
        }
    }
}

/// Non-interactive readiness for the current verification policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LocalAuthReadiness {
    Available {
        biometric_enrolled: bool,
        device_credential_configured: bool,
    },
    NotEnrolled {
        device_credential_configured: bool,
        recovery: Option<RecoveryAction>,
    },
    LockedOut {
        permanent: bool,
    },
    HardwareMissing,
    PolicyRestricted {
        device_credential_required: bool,
    },
    Unavailable,
}

/// A side-effect-free readiness observation pinned to a provider revision.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LocalAuthSnapshot {
    pub readiness: LocalAuthReadiness,
    pub policy: VerificationPolicy,
    pub revision: u64,
}

impl LocalAuthSnapshot {
    pub const fn new(
        readiness: LocalAuthReadiness,
        policy: VerificationPolicy,
        revision: u64,
    ) -> Self {
        Self {
            readiness,
            policy,
            revision,
        }
    }

    pub fn is_available(&self) -> bool {
        matches!(self.readiness, LocalAuthReadiness::Available { .. })
    }
}

/// A narrow, unforgeable record of one successful local verification.
///
/// Fields are private and the constructor is crate-private. Consumers can only
/// inspect the receipt; an application can never synthesize one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalVerificationReceipt {
    factor: VerifiedFactor,
    policy: VerificationPolicy,
    verified_at: Instant,
    surface: SurfaceId,
    generation: u64,
    revision: u64,
}

impl LocalVerificationReceipt {
    pub(crate) fn new(
        factor: VerifiedFactor,
        policy: VerificationPolicy,
        verified_at: Instant,
        surface: SurfaceId,
        generation: u64,
        revision: u64,
    ) -> Self {
        Self {
            factor,
            policy,
            verified_at,
            surface,
            generation,
            revision,
        }
    }

    pub const fn factor(&self) -> VerifiedFactor {
        self.factor
    }

    pub const fn policy(&self) -> VerificationPolicy {
        self.policy
    }

    pub const fn verified_at(&self) -> Instant {
        self.verified_at
    }

    pub const fn surface(&self) -> SurfaceId {
        self.surface
    }

    pub const fn generation(&self) -> u64 {
        self.generation
    }

    pub const fn revision(&self) -> u64 {
        self.revision
    }
}

/// The result of one interactive local user verification.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VerificationOutcome {
    Verified(LocalVerificationReceipt),
    UserCancelled,
    NotEnrolled {
        recovery: Option<RecoveryAction>,
    },
    LockedOut {
        permanent: bool,
    },
    HardwareMissing,
    /// The caller demanded biometric-only, but OS or device policy required a
    /// device-credential fallback.
    PolicyFallbackRequired,
}

/// The result of an authentication-bound protected-secret release.
///
/// The `Unlocked` payload is zeroizing [`SecretBytes`], which has no `Debug`,
/// `Clone` or serialization implementation.
///
/// ```compile_fail
/// let secret = gpui_secrets::SecretBytes::new(vec![1]).unwrap();
/// println!("{secret:?}");
/// ```
/// ```compile_fail
/// let secret = gpui_secrets::SecretBytes::new(vec![1]).unwrap();
/// let copied = secret.clone();
/// ```
/// ```compile_fail
/// let outcome = gpui_local_auth::ProtectedSecretOutcome::UserCancelled;
/// let copied = outcome.clone();
/// ```
pub enum ProtectedSecretOutcome {
    Unlocked(SecretBytes),
    UserCancelled,
    NotEnrolled { recovery: Option<RecoveryAction> },
    LockedOut { permanent: bool },
    HardwareMissing,
    PolicyFallbackRequired,
    KeyInvalidated,
    SecretNotFound,
}

impl fmt::Debug for ProtectedSecretOutcome {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unlocked(_) => formatter.write_str("Unlocked([REDACTED])"),
            Self::UserCancelled => formatter.write_str("UserCancelled"),
            Self::NotEnrolled { recovery } => formatter
                .debug_struct("NotEnrolled")
                .field("recovery", recovery)
                .finish(),
            Self::LockedOut { permanent } => formatter
                .debug_struct("LockedOut")
                .field("permanent", permanent)
                .finish(),
            Self::HardwareMissing => formatter.write_str("HardwareMissing"),
            Self::PolicyFallbackRequired => formatter.write_str("PolicyFallbackRequired"),
            Self::KeyInvalidated => formatter.write_str("KeyInvalidated"),
            Self::SecretNotFound => formatter.write_str("SecretNotFound"),
        }
    }
}

/// A provider signature receipt. Signatures are public data, not secrets.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignatureReceipt {
    pub algorithm: SignatureAlgorithm,
    pub signature: Vec<u8>,
    pub verified_at: Instant,
    pub factor: VerifiedFactor,
}

/// The result of an authentication-bound challenge signature.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProtectedSignatureOutcome {
    Signed(SignatureReceipt),
    UserCancelled,
    NotEnrolled { recovery: Option<RecoveryAction> },
    LockedOut { permanent: bool },
    HardwareMissing,
    PolicyFallbackRequired,
    KeyInvalidated,
    KeyNotFound,
}

/// Why an active native prompt was invalidated.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LocalAuthPromptCancelReason {
    OwnerChanged,
    SurfaceDetached,
    GenerationAdvanced,
    Backgrounded,
    SessionLocked,
    Cancelled,
    DeadlineElapsed,
}

/// A redacted record of a rejected native callback.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RejectedAuthCallback {
    pub surface: SurfaceId,
    pub generation: u64,
    pub phase: DiagnosticPhase,
}

impl RejectedAuthCallback {
    pub(crate) const fn late(surface: SurfaceId, generation: u64) -> Self {
        Self {
            surface,
            generation,
            phase: DiagnosticPhase::LateCallbackDiscarded,
        }
    }
}

/// A safe local-auth observation. No account identifier, purpose text or secret
/// payload is ever included.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LocalAuthEvent {
    AvailabilityChanged {
        snapshot: LocalAuthSnapshot,
    },
    PromptCancelled {
        surface: SurfaceId,
        generation: u64,
        reason: LocalAuthPromptCancelReason,
    },
    CallbackRejected(RejectedAuthCallback),
}

/// A host lifecycle observation used by prompt serialization and relocking.
///
/// The merged `gpui-lifecycle` crate exposes its state through `LifecycleState`
/// getters rather than an enumerable service snapshot. Profiles can build this
/// value from a `LifecycleState` with [`Self::from_lifecycle`], or configure a
/// deterministic surface directly for tests and headless hosts.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LocalAuthHostState {
    pub phase: AppPhase,
    pub session_lock: SessionLockState,
    pub surfaces: Vec<(SurfaceId, SurfaceSnapshot)>,
    pub revision: u64,
}

impl LocalAuthHostState {
    pub fn new(phase: AppPhase, session_lock: SessionLockState) -> Self {
        Self {
            phase,
            session_lock,
            ..Self::default()
        }
    }

    pub fn foreground(surface: SurfaceId, generation: u64) -> Self {
        Self::new(AppPhase::ForegroundActive, SessionLockState::Unlocked)
            .with_surface(surface, generation, true)
    }

    pub fn from_lifecycle(state: &LifecycleState) -> Self {
        Self {
            phase: state.phase(),
            session_lock: state.session_lock(),
            surfaces: state.surfaces().collect(),
            revision: state.revision(),
        }
    }

    pub fn with_phase(mut self, phase: AppPhase) -> Self {
        self.phase = phase;
        self
    }

    pub fn with_session_lock(mut self, session_lock: SessionLockState) -> Self {
        self.session_lock = session_lock;
        self
    }

    pub fn with_surface(mut self, surface: SurfaceId, generation: u64, visible: bool) -> Self {
        let snapshot = SurfaceSnapshot {
            visibility: if visible {
                SurfaceVisibility::Visible { focused: true }
            } else {
                SurfaceVisibility::Hidden
            },
            generation,
        };
        self.surfaces.retain(|(existing, _)| *existing != surface);
        self.surfaces.push((surface, snapshot));
        self
    }

    pub fn without_surface(mut self, surface: SurfaceId) -> Self {
        self.surfaces.retain(|(existing, _)| *existing != surface);
        self
    }

    pub fn surface(&self, surface: SurfaceId) -> Option<SurfaceSnapshot> {
        self.surfaces
            .iter()
            .find(|(existing, _)| *existing == surface)
            .map(|(_, snapshot)| *snapshot)
    }

    pub fn is_foreground(&self) -> bool {
        self.phase.is_foreground()
    }

    pub fn is_session_locked(&self) -> bool {
        self.session_lock == SessionLockState::Locked
    }
}

/// A provider-facing description of one interactive user-verification prompt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalAuthPrompt {
    pub request: gpui_system::RequestId,
    pub policy: VerificationPolicy,
    pub purpose: VerificationPurpose,
    pub surface: SurfaceId,
    pub generation: u64,
}

/// The application request for local user verification.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerificationRequest {
    pub policy: VerificationPolicy,
    pub purpose: VerificationPurpose,
}

impl VerificationRequest {
    pub fn new(policy: VerificationPolicy, purpose: VerificationPurpose) -> Self {
        Self { policy, purpose }
    }
}

/// The application request for an authentication-bound protected secret.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProtectedSecretRequest {
    pub account: AccountId,
    pub key: ProtectedKeyId,
    pub policy: VerificationPolicy,
    pub purpose: VerificationPurpose,
}

impl ProtectedSecretRequest {
    pub fn new(
        account: AccountId,
        key: ProtectedKeyId,
        policy: VerificationPolicy,
        purpose: VerificationPurpose,
    ) -> Self {
        Self {
            account,
            key,
            policy,
            purpose,
        }
    }
}

/// A provider-facing protected-secret prompt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProtectedSecretPrompt {
    pub request: gpui_system::RequestId,
    pub account: AccountId,
    pub key: ProtectedKeyId,
    pub policy: VerificationPolicy,
    pub purpose: VerificationPurpose,
    pub surface: SurfaceId,
    pub generation: u64,
}

/// The application request for an authentication-bound signature.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignatureRequest {
    pub account: AccountId,
    pub key: ProtectedKeyId,
    pub algorithm: SignatureAlgorithm,
    pub challenge: ChallengeBytes,
    pub policy: VerificationPolicy,
    pub purpose: VerificationPurpose,
}

impl SignatureRequest {
    pub fn new(
        account: AccountId,
        key: ProtectedKeyId,
        algorithm: SignatureAlgorithm,
        challenge: ChallengeBytes,
        policy: VerificationPolicy,
        purpose: VerificationPurpose,
    ) -> Self {
        Self {
            account,
            key,
            algorithm,
            challenge,
            policy,
            purpose,
        }
    }
}

/// A provider-facing signature prompt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignaturePrompt {
    pub request: gpui_system::RequestId,
    pub account: AccountId,
    pub key: ProtectedKeyId,
    pub algorithm: SignatureAlgorithm,
    pub challenge: ChallengeBytes,
    pub policy: VerificationPolicy,
    pub purpose: VerificationPurpose,
    pub surface: SurfaceId,
    pub generation: u64,
}

pub(crate) fn error(code: ErrorCode, message: &'static str) -> SystemError {
    SystemError::new(code, message)
}

pub(crate) fn invalid_input(message: &'static str) -> SystemError {
    error(ErrorCode::InvalidInput, message)
}

pub(crate) fn context_expired(message: &'static str) -> SystemError {
    let mut error = error(ErrorCode::ContextExpired, message);
    error.retry = RetryAdvice::AfterConditionsChange;
    error
}

pub(crate) fn busy() -> SystemError {
    let mut error = error(
        ErrorCode::Busy,
        "A local authentication prompt is already active",
    );
    error.retry = RetryAdvice::AfterConditionsChange;
    error
}

fn validate_identifier(
    value: &str,
    maximum: usize,
    message: &'static str,
) -> Result<(), SystemError> {
    if value.is_empty() || value.len() > maximum || value.chars().any(char::is_control) {
        return Err(invalid_input(message));
    }
    Ok(())
}
