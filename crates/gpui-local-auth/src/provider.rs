//! Explicit local-auth provider contract, prompt completions and honest
//! unavailable/platform contract providers.

use crate::{
    LocalAuthOperation, LocalAuthPrompt, LocalAuthReadiness, LocalAuthSnapshot,
    ProtectedSecretOutcome, ProtectedSecretPrompt, ProtectedSignatureOutcome, SignaturePrompt,
    SystemError, VerificationOutcome, VerificationPolicy,
};
use gpui_system::{
    Availability, Capability, Completion, CompletionPort, Configuration, Constraint, ErrorCode,
    Evidence, PermissionState, ProviderManifest, RecoveryAction, RequestId, RetryAdvice, Support,
    SurfaceId,
};
use std::{
    cell::Cell,
    fmt,
    rc::{Rc, Weak},
};

/// Conservative manifest for this revision. No native provider is installed by
/// this crate; platform providers remain separate work.
pub const PROVIDER_MANIFEST: [ProviderManifest; 5] = gpui_system::PROVIDER_MANIFEST;

struct CompletionState {
    completed: Cell<bool>,
}

/// Cloneable provider callback for one interactive local-auth operation.
///
/// Exactly one completion is accepted. The service validates the prompt token,
/// operation, surface generation, deadline and lifecycle before the value is
/// delivered. Late, cancelled, recreated or disposed callbacks return false and
/// are recorded as [`crate::RejectedAuthCallback`] values.
pub struct AuthCompletion<T> {
    port: CompletionPort<T>,
    service: Weak<crate::service::ServiceInner>,
    token: u64,
    operation: LocalAuthOperation,
    surface: SurfaceId,
    generation: u64,
    state: Rc<CompletionState>,
}

impl<T> Clone for AuthCompletion<T> {
    fn clone(&self) -> Self {
        Self {
            port: self.port.clone(),
            service: self.service.clone(),
            token: self.token,
            operation: self.operation,
            surface: self.surface,
            generation: self.generation,
            state: Rc::clone(&self.state),
        }
    }
}

impl<T> AuthCompletion<T> {
    pub(crate) fn new(
        port: CompletionPort<T>,
        service: Weak<crate::service::ServiceInner>,
        token: u64,
        operation: LocalAuthOperation,
        surface: SurfaceId,
        generation: u64,
    ) -> Self {
        Self {
            port,
            service,
            token,
            operation,
            surface,
            generation,
            state: Rc::new(CompletionState {
                completed: Cell::new(false),
            }),
        }
    }

    pub fn id(&self) -> RequestId {
        self.port.id()
    }

    pub const fn operation(&self) -> LocalAuthOperation {
        self.operation
    }

    pub const fn surface(&self) -> SurfaceId {
        self.surface
    }

    pub const fn generation(&self) -> u64 {
        self.generation
    }

    /// Deliver one accepted value. Returns false for a duplicate callback, a
    /// dropped service, an invalidated prompt or an expired request context.
    pub fn complete(&self, value: T) -> bool {
        if self.state.completed.replace(true) {
            return false;
        }
        let Some(service) = self.service.upgrade() else {
            return false;
        };
        if !service.finish_prompt(self.token, self.operation, self.surface, self.generation) {
            return false;
        }
        self.port.complete(Ok(Completion::Completed(value)))
    }

    /// Deliver a typed provider failure. A stale failure is discarded exactly
    /// like a stale success.
    pub fn fail(&self, error: SystemError) -> bool {
        if self.state.completed.replace(true) {
            return false;
        }
        let Some(service) = self.service.upgrade() else {
            return false;
        };
        if !service.finish_prompt(self.token, self.operation, self.surface, self.generation) {
            return false;
        }
        self.port.complete(Err(error))
    }
}

impl<T> fmt::Debug for AuthCompletion<T> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AuthCompletion")
            .field("request", &self.port.id())
            .field("operation", &self.operation)
            .field("surface", &self.surface)
            .field("generation", &self.generation)
            .field("completed", &self.state.completed.get())
            .finish()
    }
}

pub type LocalAuthPromptCompletion = AuthCompletion<VerificationOutcome>;
pub type ProtectedSecretCompletion = AuthCompletion<ProtectedSecretOutcome>;
pub type ProtectedSignatureCompletion = AuthCompletion<ProtectedSignatureOutcome>;

/// The explicit provider boundary for local authentication.
///
/// Interactive methods must not block the UI thread. They may retain the
/// completion and invoke it from the native platform callback, or complete it
/// synchronously when the platform resolves inline. A returned `Err` means no
/// prompt was presented.
pub trait LocalAuthProvider {
    /// Describe one operation without prompting or reading secrets.
    fn capability(&self, operation: LocalAuthOperation) -> Capability<LocalAuthOperation>;

    /// Probe readiness without a native prompt. This must never display UI,
    /// even when no factor is enrolled.
    fn query_availability(
        &self,
        policy: VerificationPolicy,
    ) -> Result<LocalAuthSnapshot, SystemError>;

    fn verify_user(
        &self,
        prompt: LocalAuthPrompt,
        completion: LocalAuthPromptCompletion,
    ) -> Result<(), SystemError>;

    fn unlock_protected_secret(
        &self,
        prompt: ProtectedSecretPrompt,
        completion: ProtectedSecretCompletion,
    ) -> Result<(), SystemError>;

    fn sign_challenge(
        &self,
        prompt: SignaturePrompt,
        completion: ProtectedSignatureCompletion,
    ) -> Result<(), SystemError>;
}

/// Linux backend selection. This crate ships no native backend; both shapes are
/// reported honestly instead of synthesizing a biometric prompt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinuxLocalAuthConfig {
    /// No portal, PAM or desktop backend was selected.
    Unconfigured,
    /// A host-selected backend identifier. Selection is not runtime evidence.
    ConfiguredBackend { id: &'static str },
}

impl LinuxLocalAuthConfig {
    pub const fn backend_id(self) -> Option<&'static str> {
        match self {
            Self::Unconfigured => None,
            Self::ConfiguredBackend { id } => Some(id),
        }
    }
}

/// An explicit provider that never fakes local verification or secret access.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct UnavailableLocalAuthProvider {
    linux: Option<LinuxLocalAuthConfig>,
}

impl UnavailableLocalAuthProvider {
    pub const fn new() -> Self {
        Self { linux: None }
    }

    pub const fn for_linux(config: LinuxLocalAuthConfig) -> Self {
        Self {
            linux: Some(config),
        }
    }

    pub const fn linux_config(self) -> Option<LinuxLocalAuthConfig> {
        self.linux
    }

    fn configuration(self) -> Configuration {
        match self.linux {
            Some(LinuxLocalAuthConfig::Unconfigured) => Configuration::Required,
            _ => Configuration::Ready,
        }
    }

    fn constraints(self, operation: LocalAuthOperation) -> Vec<Constraint> {
        let mut constraints = vec![Constraint::Provider("local-auth-unavailable")];
        match self.linux {
            Some(LinuxLocalAuthConfig::Unconfigured) => {
                constraints.push(Constraint::Provider("linux-unconfigured"));
            }
            Some(LinuxLocalAuthConfig::ConfiguredBackend { id }) => {
                constraints.push(Constraint::Provider(id));
            }
            None => {}
        }
        if operation.is_interactive() {
            constraints.push(Constraint::ForegroundOwner);
        }
        constraints
    }
}

impl LocalAuthProvider for UnavailableLocalAuthProvider {
    fn capability(&self, operation: LocalAuthOperation) -> Capability<LocalAuthOperation> {
        Capability {
            operation,
            support: Support::Conditional,
            availability: Availability::Unavailable,
            configuration: self.configuration(),
            permission: PermissionState::NotRequired,
            evidence: Evidence::ContractOnly,
            constraints: self.constraints(operation),
            observed_revision: 1,
        }
    }

    fn query_availability(
        &self,
        policy: VerificationPolicy,
    ) -> Result<LocalAuthSnapshot, SystemError> {
        Ok(LocalAuthSnapshot::new(
            LocalAuthReadiness::Unavailable,
            policy,
            1,
        ))
    }

    fn verify_user(
        &self,
        _prompt: LocalAuthPrompt,
        _completion: LocalAuthPromptCompletion,
    ) -> Result<(), SystemError> {
        Err(unavailable())
    }

    fn unlock_protected_secret(
        &self,
        _prompt: ProtectedSecretPrompt,
        _completion: ProtectedSecretCompletion,
    ) -> Result<(), SystemError> {
        Err(unavailable())
    }

    fn sign_challenge(
        &self,
        _prompt: SignaturePrompt,
        _completion: ProtectedSignatureCompletion,
    ) -> Result<(), SystemError> {
        Err(unavailable())
    }
}

fn unavailable() -> SystemError {
    let mut error = SystemError::new(
        ErrorCode::Unavailable,
        "Local authentication provider is unavailable",
    );
    error.recovery = Some(RecoveryAction::ProbeAgain);
    error.retry = RetryAdvice::AfterConditionsChange;
    error
}
