//! Deterministic headless provider used by tests, examples and gallery harnesses.
//!
//! It never calls a native API and never fabricates platform evidence. Tests can
//! drive immediate outcomes or retain pending prompt handles to dispose owners,
//! advance generations, background the app, lock the session, cancel or expire
//! deadlines before firing late native completions.

use crate::provider::LocalAuthProvider;
use crate::{
    AccountId, LocalAuthOperation, LocalAuthPrompt, LocalAuthPromptCompletion, LocalAuthReadiness,
    LocalAuthSnapshot, LocalVerificationReceipt, ProtectedKeyId, ProtectedSecretCompletion,
    ProtectedSecretOutcome, ProtectedSecretPrompt, ProtectedSignatureCompletion,
    ProtectedSignatureOutcome, RecoveryAction, SignatureAlgorithm, SignaturePrompt,
    SignatureReceipt, SystemError, VerificationOutcome, VerificationPolicy, VerifiedFactor,
};
use gpui_secrets::SecretBytes;
use gpui_system::{
    Availability, Capability, Configuration, Constraint, Evidence, PermissionState, Support,
};
use std::{
    cell::RefCell,
    collections::{BTreeMap, VecDeque},
    fmt,
    rc::{Rc, Weak},
    time::Instant,
};

/// A scripted immediate user-verification result.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FakeVerificationMode {
    Biometric,
    DeviceCredentialFallback,
    UserCancelled,
    NotEnrolled { recovery: Option<RecoveryAction> },
    LockedOut { permanent: bool },
    HardwareMissing,
}

struct SealedSecret {
    epoch: u64,
    secret: SecretBytes,
}

struct FakeKey {
    epoch: u64,
    algorithm: SignatureAlgorithm,
    material: Vec<u8>,
}

struct FakeState {
    readiness: LocalAuthReadiness,
    revision: u64,
    enrollment_epoch: u64,
    fallback_required: bool,
    auto_verify: Option<FakeVerificationMode>,
    auto_secrets: bool,
    auto_signatures: bool,
    pending_verify: VecDeque<PendingLocalAuthPrompt>,
    pending_secrets: VecDeque<PendingProtectedSecretPrompt>,
    pending_signatures: VecDeque<PendingSignaturePrompt>,
    secrets: BTreeMap<(String, String), SealedSecret>,
    keys: BTreeMap<(String, String), FakeKey>,
    verify_requests: Vec<LocalAuthPrompt>,
    secret_requests: Vec<ProtectedSecretPrompt>,
    signature_requests: Vec<SignaturePrompt>,
    support_overrides: BTreeMap<LocalAuthOperation, Support>,
    configuration_overrides: BTreeMap<LocalAuthOperation, Configuration>,
}

impl Default for FakeState {
    fn default() -> Self {
        Self {
            readiness: LocalAuthReadiness::Available {
                biometric_enrolled: true,
                device_credential_configured: true,
            },
            revision: 1,
            enrollment_epoch: 0,
            fallback_required: false,
            auto_verify: None,
            auto_secrets: true,
            auto_signatures: true,
            pending_verify: VecDeque::new(),
            pending_secrets: VecDeque::new(),
            pending_signatures: VecDeque::new(),
            secrets: BTreeMap::new(),
            keys: BTreeMap::new(),
            verify_requests: Vec::new(),
            secret_requests: Vec::new(),
            signature_requests: Vec::new(),
            support_overrides: BTreeMap::new(),
            configuration_overrides: BTreeMap::new(),
        }
    }
}

struct FakeInner {
    state: RefCell<FakeState>,
}

/// Deterministic local-auth provider. No native prompt is ever displayed.
pub struct FakeLocalAuthProvider {
    inner: Rc<FakeInner>,
}

impl fmt::Debug for FakeLocalAuthProvider {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let state = self.inner.state.borrow();
        formatter
            .debug_struct("FakeLocalAuthProvider")
            .field("readiness", &state.readiness)
            .field("revision", &state.revision)
            .field("enrollment_epoch", &state.enrollment_epoch)
            .field("fallback_required", &state.fallback_required)
            .field("pending_verify", &state.pending_verify.len())
            .field("pending_secrets", &state.pending_secrets.len())
            .field("pending_signatures", &state.pending_signatures.len())
            .field("secrets", &state.secrets.len())
            .field("keys", &state.keys.len())
            .field("capability_overrides", &state.support_overrides.len())
            .finish_non_exhaustive()
    }
}

impl Default for FakeLocalAuthProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl FakeLocalAuthProvider {
    pub fn new() -> Self {
        Self {
            inner: Rc::new(FakeInner {
                state: RefCell::new(FakeState::default()),
            }),
        }
    }

    fn bump(state: &mut FakeState) {
        state.revision = state.revision.saturating_add(1);
    }

    pub fn set_readiness(&self, readiness: LocalAuthReadiness) {
        let mut state = self.inner.state.borrow_mut();
        state.readiness = readiness;
        Self::bump(&mut state);
    }

    pub fn set_available(&self) {
        self.set_readiness(LocalAuthReadiness::Available {
            biometric_enrolled: true,
            device_credential_configured: true,
        });
    }

    pub fn set_not_enrolled(
        &self,
        device_credential_configured: bool,
        recovery: Option<RecoveryAction>,
    ) {
        self.set_readiness(LocalAuthReadiness::NotEnrolled {
            device_credential_configured,
            recovery,
        });
    }

    pub fn set_locked_out(&self, permanent: bool) {
        self.set_readiness(LocalAuthReadiness::LockedOut { permanent });
    }

    pub fn set_hardware_missing(&self) {
        self.set_readiness(LocalAuthReadiness::HardwareMissing);
    }

    pub fn set_policy_restricted(&self, device_credential_required: bool) {
        self.set_readiness(LocalAuthReadiness::PolicyRestricted {
            device_credential_required,
        });
    }

    pub fn set_unavailable(&self) {
        self.set_readiness(LocalAuthReadiness::Unavailable);
    }

    /// Simulate an OS/device policy that rejects biometric-only verification and
    /// requires the device-credential fallback.
    pub fn set_policy_fallback_required(&self, required: bool) {
        self.inner.state.borrow_mut().fallback_required = required;
    }

    pub fn set_auto_verify(&self, mode: Option<FakeVerificationMode>) {
        self.inner.state.borrow_mut().auto_verify = mode;
    }

    pub fn set_auto_secret_unlock(&self, auto: bool) {
        self.inner.state.borrow_mut().auto_secrets = auto;
    }

    pub fn set_auto_signing(&self, auto: bool) {
        self.inner.state.borrow_mut().auto_signatures = auto;
    }

    /// Override the advertised support for one operation without changing readiness.
    pub fn set_operation_support(&self, operation: LocalAuthOperation, support: Support) {
        let mut state = self.inner.state.borrow_mut();
        state.support_overrides.insert(operation, support);
        Self::bump(&mut state);
    }

    /// Override the advertised configuration for one operation.
    pub fn set_operation_configuration(
        &self,
        operation: LocalAuthOperation,
        configuration: Configuration,
    ) {
        let mut state = self.inner.state.borrow_mut();
        state
            .configuration_overrides
            .insert(operation, configuration);
        Self::bump(&mut state);
    }

    /// Drop every per-operation capability override.
    pub fn clear_capability_overrides(&self) {
        let mut state = self.inner.state.borrow_mut();
        state.support_overrides.clear();
        state.configuration_overrides.clear();
        Self::bump(&mut state);
    }

    pub fn revision(&self) -> u64 {
        self.inner.state.borrow().revision
    }

    pub fn pending_prompt_count(&self) -> usize {
        self.inner.state.borrow().pending_verify.len()
    }

    pub fn take_pending_prompt(&self) -> Option<PendingLocalAuthPrompt> {
        self.inner.state.borrow_mut().pending_verify.pop_front()
    }

    pub fn pending_secret_prompt_count(&self) -> usize {
        self.inner.state.borrow().pending_secrets.len()
    }

    pub fn take_pending_secret_prompt(&self) -> Option<PendingProtectedSecretPrompt> {
        self.inner.state.borrow_mut().pending_secrets.pop_front()
    }

    pub fn pending_signature_prompt_count(&self) -> usize {
        self.inner.state.borrow().pending_signatures.len()
    }

    pub fn take_pending_signature_prompt(&self) -> Option<PendingSignaturePrompt> {
        self.inner.state.borrow_mut().pending_signatures.pop_front()
    }

    pub fn verify_requests(&self) -> Vec<LocalAuthPrompt> {
        self.inner.state.borrow().verify_requests.clone()
    }

    pub fn secret_requests(&self) -> Vec<ProtectedSecretPrompt> {
        self.inner.state.borrow().secret_requests.clone()
    }

    pub fn signature_requests(&self) -> Vec<SignaturePrompt> {
        self.inner.state.borrow().signature_requests.clone()
    }

    /// Store an authentication-bound secret for the current enrollment epoch.
    pub fn seal_protected_secret(
        &self,
        account: &AccountId,
        key: &ProtectedKeyId,
        secret: SecretBytes,
    ) {
        let mut state = self.inner.state.borrow_mut();
        let epoch = state.enrollment_epoch;
        state.secrets.insert(
            (account.as_str().to_owned(), key.as_str().to_owned()),
            SealedSecret { epoch, secret },
        );
        Self::bump(&mut state);
    }

    /// Register a deterministic protected signing key for the current epoch.
    pub fn register_signing_key(
        &self,
        account: &AccountId,
        key: &ProtectedKeyId,
        algorithm: SignatureAlgorithm,
    ) {
        let mut state = self.inner.state.borrow_mut();
        let epoch = state.enrollment_epoch;
        let material = format!("{algorithm:?}:{}:{}", account.as_str(), key.as_str()).into_bytes();
        state.keys.insert(
            (account.as_str().to_owned(), key.as_str().to_owned()),
            FakeKey {
                epoch,
                algorithm,
                material,
            },
        );
        Self::bump(&mut state);
    }

    /// Simulate re-enrollment. Keys and secrets sealed under the prior epoch are
    /// reported as `KeyInvalidated` instead of being silently released.
    pub fn advance_enrollment_epoch(&self) {
        let mut state = self.inner.state.borrow_mut();
        state.enrollment_epoch = state.enrollment_epoch.saturating_add(1);
        Self::bump(&mut state);
    }

    fn receipt(
        prompt: &LocalAuthPrompt,
        revision: u64,
        factor: VerifiedFactor,
    ) -> LocalVerificationReceipt {
        LocalVerificationReceipt::new(
            factor,
            prompt.policy,
            Instant::now(),
            prompt.surface,
            prompt.generation,
            revision,
        )
    }

    fn auto_outcome(
        state: &FakeState,
        prompt: &LocalAuthPrompt,
        mode: FakeVerificationMode,
        revision: u64,
    ) -> VerificationOutcome {
        match mode {
            FakeVerificationMode::Biometric => {
                if state.fallback_required {
                    if prompt.policy.allows_device_credential() {
                        VerificationOutcome::Verified(Self::receipt(
                            prompt,
                            revision,
                            VerifiedFactor::DeviceCredentialFallback,
                        ))
                    } else {
                        VerificationOutcome::PolicyFallbackRequired
                    }
                } else {
                    VerificationOutcome::Verified(Self::receipt(
                        prompt,
                        revision,
                        VerifiedFactor::Biometric,
                    ))
                }
            }
            FakeVerificationMode::DeviceCredentialFallback => {
                if prompt.policy.allows_device_credential() {
                    VerificationOutcome::Verified(Self::receipt(
                        prompt,
                        revision,
                        VerifiedFactor::DeviceCredentialFallback,
                    ))
                } else {
                    VerificationOutcome::PolicyFallbackRequired
                }
            }
            FakeVerificationMode::UserCancelled => VerificationOutcome::UserCancelled,
            FakeVerificationMode::NotEnrolled { recovery } => {
                VerificationOutcome::NotEnrolled { recovery }
            }
            FakeVerificationMode::LockedOut { permanent } => {
                VerificationOutcome::LockedOut { permanent }
            }
            FakeVerificationMode::HardwareMissing => VerificationOutcome::HardwareMissing,
        }
    }
}

fn secret_outcome(state: &FakeState, prompt: &ProtectedSecretPrompt) -> ProtectedSecretOutcome {
    match state.readiness {
        LocalAuthReadiness::HardwareMissing => ProtectedSecretOutcome::HardwareMissing,
        LocalAuthReadiness::NotEnrolled { recovery, .. } => {
            ProtectedSecretOutcome::NotEnrolled { recovery }
        }
        LocalAuthReadiness::LockedOut { permanent } => {
            ProtectedSecretOutcome::LockedOut { permanent }
        }
        LocalAuthReadiness::PolicyRestricted {
            device_credential_required: true,
        } if !prompt.policy.allows_device_credential() => {
            ProtectedSecretOutcome::PolicyFallbackRequired
        }
        _ => {
            if state.fallback_required && !prompt.policy.allows_device_credential() {
                return ProtectedSecretOutcome::PolicyFallbackRequired;
            }
            let lookup = (
                prompt.account.as_str().to_owned(),
                prompt.key.as_str().to_owned(),
            );
            let Some(sealed) = state.secrets.get(&lookup) else {
                return ProtectedSecretOutcome::SecretNotFound;
            };
            if sealed.epoch != state.enrollment_epoch {
                return ProtectedSecretOutcome::KeyInvalidated;
            }
            match SecretBytes::new(sealed.secret.expose().to_vec()) {
                Ok(secret) => ProtectedSecretOutcome::Unlocked(secret),
                Err(_) => ProtectedSecretOutcome::SecretNotFound,
            }
        }
    }
}

fn signature_outcome(state: &FakeState, prompt: &SignaturePrompt) -> ProtectedSignatureOutcome {
    match state.readiness {
        LocalAuthReadiness::HardwareMissing => ProtectedSignatureOutcome::HardwareMissing,
        LocalAuthReadiness::NotEnrolled { recovery, .. } => {
            ProtectedSignatureOutcome::NotEnrolled { recovery }
        }
        LocalAuthReadiness::LockedOut { permanent } => {
            ProtectedSignatureOutcome::LockedOut { permanent }
        }
        LocalAuthReadiness::PolicyRestricted {
            device_credential_required: true,
        } if !prompt.policy.allows_device_credential() => {
            ProtectedSignatureOutcome::PolicyFallbackRequired
        }
        _ => {
            if state.fallback_required && !prompt.policy.allows_device_credential() {
                return ProtectedSignatureOutcome::PolicyFallbackRequired;
            }
            let lookup = (
                prompt.account.as_str().to_owned(),
                prompt.key.as_str().to_owned(),
            );
            let Some(key) = state.keys.get(&lookup) else {
                return ProtectedSignatureOutcome::KeyNotFound;
            };
            if key.epoch != state.enrollment_epoch {
                return ProtectedSignatureOutcome::KeyInvalidated;
            }
            if key.algorithm != prompt.algorithm {
                return ProtectedSignatureOutcome::KeyNotFound;
            }
            let factor = if state.fallback_required {
                VerifiedFactor::DeviceCredentialFallback
            } else {
                VerifiedFactor::Biometric
            };
            ProtectedSignatureOutcome::Signed(SignatureReceipt {
                algorithm: prompt.algorithm,
                signature: fake_signature(&key.material, prompt.challenge.as_bytes()),
                verified_at: Instant::now(),
                factor,
            })
        }
    }
}

fn fake_signature(material: &[u8], challenge: &[u8]) -> Vec<u8> {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for byte in material.iter().chain(challenge.iter()) {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    let mut signature = Vec::with_capacity(32);
    for _ in 0..4 {
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
        signature.extend_from_slice(&hash.to_le_bytes());
    }
    signature
}

impl LocalAuthProvider for FakeLocalAuthProvider {
    fn capability(&self, operation: LocalAuthOperation) -> Capability<LocalAuthOperation> {
        let state = self.inner.state.borrow();
        let (support, availability, permission) = match state.readiness {
            LocalAuthReadiness::Unavailable | LocalAuthReadiness::HardwareMissing => (
                Support::Conditional,
                Availability::Unavailable,
                PermissionState::NotRequired,
            ),
            LocalAuthReadiness::LockedOut { permanent: true } => (
                Support::Native,
                Availability::Available,
                PermissionState::Denied,
            ),
            _ => (
                Support::Native,
                Availability::Available,
                PermissionState::NotRequired,
            ),
        };
        let support = state
            .support_overrides
            .get(&operation)
            .copied()
            .unwrap_or(support);
        let configuration = state
            .configuration_overrides
            .get(&operation)
            .copied()
            .unwrap_or(Configuration::Ready);
        let mut constraints = vec![Constraint::Provider("fake")];
        if operation.is_interactive() {
            constraints.push(Constraint::ForegroundOwner);
        }
        Capability {
            operation,
            support,
            availability,
            configuration,
            permission,
            evidence: Evidence::ContractOnly,
            constraints,
            observed_revision: state.revision,
        }
    }

    fn query_availability(
        &self,
        policy: VerificationPolicy,
    ) -> Result<LocalAuthSnapshot, SystemError> {
        let state = self.inner.state.borrow();
        Ok(LocalAuthSnapshot::new(
            state.readiness,
            policy,
            state.revision,
        ))
    }

    fn verify_user(
        &self,
        prompt: LocalAuthPrompt,
        completion: LocalAuthPromptCompletion,
    ) -> Result<(), SystemError> {
        let (readiness, auto, revision) = {
            let mut state = self.inner.state.borrow_mut();
            state.verify_requests.push(prompt.clone());
            (state.readiness, state.auto_verify, state.revision)
        };
        match readiness {
            LocalAuthReadiness::Unavailable => {
                return Err(SystemError::new(
                    gpui_system::ErrorCode::Unavailable,
                    "Local authentication provider is unavailable",
                ));
            }
            LocalAuthReadiness::HardwareMissing => {
                completion.complete(VerificationOutcome::HardwareMissing);
                return Ok(());
            }
            LocalAuthReadiness::NotEnrolled { recovery, .. } => {
                completion.complete(VerificationOutcome::NotEnrolled { recovery });
                return Ok(());
            }
            LocalAuthReadiness::LockedOut { permanent } => {
                completion.complete(VerificationOutcome::LockedOut { permanent });
                return Ok(());
            }
            LocalAuthReadiness::PolicyRestricted {
                device_credential_required: true,
            } if !prompt.policy.allows_device_credential() => {
                completion.complete(VerificationOutcome::PolicyFallbackRequired);
                return Ok(());
            }
            _ => {}
        }
        if let Some(mode) = auto {
            let outcome = {
                let state = self.inner.state.borrow();
                Self::auto_outcome(&state, &prompt, mode, revision)
            };
            completion.complete(outcome);
            return Ok(());
        }
        let pending = PendingLocalAuthPrompt {
            prompt,
            completion,
            revision,
        };
        self.inner
            .state
            .borrow_mut()
            .pending_verify
            .push_back(pending);
        Ok(())
    }

    fn unlock_protected_secret(
        &self,
        prompt: ProtectedSecretPrompt,
        completion: ProtectedSecretCompletion,
    ) -> Result<(), SystemError> {
        let (readiness, auto) = {
            let mut state = self.inner.state.borrow_mut();
            state.secret_requests.push(prompt.clone());
            (state.readiness, state.auto_secrets)
        };
        if matches!(readiness, LocalAuthReadiness::Unavailable) {
            return Err(SystemError::new(
                gpui_system::ErrorCode::Unavailable,
                "Local authentication provider is unavailable",
            ));
        }
        if auto {
            let outcome = {
                let state = self.inner.state.borrow();
                secret_outcome(&state, &prompt)
            };
            completion.complete(outcome);
            return Ok(());
        }
        let pending = PendingProtectedSecretPrompt {
            inner: Rc::downgrade(&self.inner),
            prompt,
            completion,
        };
        self.inner
            .state
            .borrow_mut()
            .pending_secrets
            .push_back(pending);
        Ok(())
    }

    fn sign_challenge(
        &self,
        prompt: SignaturePrompt,
        completion: ProtectedSignatureCompletion,
    ) -> Result<(), SystemError> {
        let (readiness, auto) = {
            let mut state = self.inner.state.borrow_mut();
            state.signature_requests.push(prompt.clone());
            (state.readiness, state.auto_signatures)
        };
        if matches!(readiness, LocalAuthReadiness::Unavailable) {
            return Err(SystemError::new(
                gpui_system::ErrorCode::Unavailable,
                "Local authentication provider is unavailable",
            ));
        }
        if auto {
            let outcome = {
                let state = self.inner.state.borrow();
                signature_outcome(&state, &prompt)
            };
            completion.complete(outcome);
            return Ok(());
        }
        let pending = PendingSignaturePrompt {
            inner: Rc::downgrade(&self.inner),
            prompt,
            completion,
        };
        self.inner
            .state
            .borrow_mut()
            .pending_signatures
            .push_back(pending);
        Ok(())
    }
}

/// A retained fake verification prompt. Dropping it does not complete anything.
#[derive(Clone, Debug)]
pub struct PendingLocalAuthPrompt {
    prompt: LocalAuthPrompt,
    completion: LocalAuthPromptCompletion,
    revision: u64,
}

impl PendingLocalAuthPrompt {
    pub fn prompt(&self) -> &LocalAuthPrompt {
        &self.prompt
    }

    pub fn policy(&self) -> VerificationPolicy {
        self.prompt.policy
    }

    pub fn completion(&self) -> LocalAuthPromptCompletion {
        self.completion.clone()
    }

    pub fn verify(&self, factor: VerifiedFactor) -> bool {
        let receipt = FakeLocalAuthProvider::receipt(&self.prompt, self.revision, factor);
        self.completion
            .complete(VerificationOutcome::Verified(receipt))
    }

    pub fn cancel(&self) -> bool {
        self.completion.complete(VerificationOutcome::UserCancelled)
    }

    pub fn not_enrolled(&self, recovery: Option<RecoveryAction>) -> bool {
        self.completion
            .complete(VerificationOutcome::NotEnrolled { recovery })
    }

    pub fn locked_out(&self, permanent: bool) -> bool {
        self.completion
            .complete(VerificationOutcome::LockedOut { permanent })
    }

    pub fn hardware_missing(&self) -> bool {
        self.completion
            .complete(VerificationOutcome::HardwareMissing)
    }

    pub fn policy_fallback_required(&self) -> bool {
        self.completion
            .complete(VerificationOutcome::PolicyFallbackRequired)
    }

    pub fn complete(&self, outcome: VerificationOutcome) -> bool {
        self.completion.complete(outcome)
    }

    pub fn fail(&self, error: SystemError) -> bool {
        self.completion.fail(error)
    }
}

/// A retained fake protected-secret prompt.
#[derive(Clone)]
pub struct PendingProtectedSecretPrompt {
    inner: Weak<FakeInner>,
    prompt: ProtectedSecretPrompt,
    completion: ProtectedSecretCompletion,
}

impl fmt::Debug for PendingProtectedSecretPrompt {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PendingProtectedSecretPrompt")
            .field("request", &self.prompt.request)
            .field("policy", &self.prompt.policy)
            .field("surface", &self.prompt.surface)
            .field("generation", &self.prompt.generation)
            .finish_non_exhaustive()
    }
}

impl PendingProtectedSecretPrompt {
    pub fn prompt(&self) -> &ProtectedSecretPrompt {
        &self.prompt
    }

    pub fn completion(&self) -> ProtectedSecretCompletion {
        self.completion.clone()
    }

    /// Resolve the prompt against the provider's current enrollment epoch and
    /// secret store. A stale service rejects the value after this call.
    pub fn unlock(&self) -> bool {
        let outcome = match self.inner.upgrade() {
            Some(inner) => secret_outcome(&inner.state.borrow(), &self.prompt),
            None => ProtectedSecretOutcome::SecretNotFound,
        };
        self.completion.complete(outcome)
    }

    pub fn complete(&self, outcome: ProtectedSecretOutcome) -> bool {
        self.completion.complete(outcome)
    }

    pub fn fail(&self, error: SystemError) -> bool {
        self.completion.fail(error)
    }
}

/// A retained fake protected-signature prompt.
#[derive(Clone)]
pub struct PendingSignaturePrompt {
    inner: Weak<FakeInner>,
    prompt: SignaturePrompt,
    completion: ProtectedSignatureCompletion,
}

impl fmt::Debug for PendingSignaturePrompt {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PendingSignaturePrompt")
            .field("request", &self.prompt.request)
            .field("policy", &self.prompt.policy)
            .field("algorithm", &self.prompt.algorithm)
            .field("challenge_len", &self.prompt.challenge.len())
            .field("surface", &self.prompt.surface)
            .field("generation", &self.prompt.generation)
            .finish_non_exhaustive()
    }
}

impl PendingSignaturePrompt {
    pub fn prompt(&self) -> &SignaturePrompt {
        &self.prompt
    }

    pub fn completion(&self) -> ProtectedSignatureCompletion {
        self.completion.clone()
    }

    pub fn sign(&self) -> bool {
        let outcome = match self.inner.upgrade() {
            Some(inner) => signature_outcome(&inner.state.borrow(), &self.prompt),
            None => ProtectedSignatureOutcome::KeyNotFound,
        };
        self.completion.complete(outcome)
    }

    pub fn complete(&self, outcome: ProtectedSignatureOutcome) -> bool {
        self.completion.complete(outcome)
    }

    pub fn fail(&self, error: SystemError) -> bool {
        self.completion.fail(error)
    }
}
