use crate::{
    model::{error, hex, validate},
    *,
};
use gpui_system::{
    Availability, Capability, Completion, Configuration, ErrorCode, Evidence, OperationContext,
    PermissionState, ServiceResult, Support, SystemError,
};
use std::{sync::Arc, time::Instant};

pub type ProviderResult<T> = Result<SecretAccess<T>, SystemError>;
/// Provider calls are blocking, must not log names/content, and must preserve
/// the target isolation boundary. Enumeration returns target names, never values.
/// Native stores must provide atomic replacement for a single target.
pub trait CredentialProvider: Send + Sync {
    fn capabilities(&self) -> Vec<Capability<SecretOperation>>;
    fn validate_policy(&self, policy: SecretPolicy) -> Result<(), SystemError>;
    fn put(&self, target: &str, value: &SecretBytes) -> ProviderResult<()>;
    fn get(&self, target: &str) -> ProviderResult<Option<SecretBytes>>;
    fn delete(&self, target: &str) -> ProviderResult<DeleteReceipt>;
    fn list(&self, prefix: &str) -> ProviderResult<Vec<String>>;
}
#[derive(Clone)]
pub struct Secrets {
    prefix: String,
    provider: Arc<dyn CredentialProvider>,
}
impl Secrets {
    pub fn with_provider(
        app_id: &str,
        provider: Arc<dyn CredentialProvider>,
    ) -> Result<Self, SystemError> {
        validate(app_id)?;
        Ok(Self {
            prefix: format!("gpui-box/v1/{}/", hex(app_id)),
            provider,
        })
    }
    /// Selects a secure native provider or reports an explicitly unavailable one.
    pub fn native(app_id: &str) -> Result<Self, SystemError> {
        #[cfg(target_os = "windows")]
        let provider: Arc<dyn CredentialProvider> = Arc::new(WindowsCredentials);
        #[cfg(not(target_os = "windows"))]
        let provider: Arc<dyn CredentialProvider> = Arc::new(UnavailableCredentials);
        Self::with_provider(app_id, provider)
    }
    pub fn capabilities(&self) -> Vec<Capability<SecretOperation>> {
        self.provider.capabilities()
    }
    fn target(&self, key: &CredentialKey) -> String {
        format!(
            "{}{}/{}/{}",
            self.prefix,
            hex(&key.account),
            hex(&key.service),
            hex(&key.purpose)
        )
    }
    fn check(
        &self,
        operation: SecretOperation,
        context: &OperationContext,
    ) -> Result<Option<Completion<SecretAccess<()>>>, SystemError> {
        if context.ui.is_some() {
            return Err(error(
                ErrorCode::InvalidInput,
                "Credentials require a background context without a UI attachment",
            ));
        }
        context.check_at(Instant::now())?;
        if let Some(reason) = context.cancellation.reason() {
            return Ok(Some(Completion::Cancelled(reason)));
        }
        self.provider
            .capabilities()
            .into_iter()
            .find(|cap| cap.operation == operation)
            .ok_or_else(|| {
                error(
                    ErrorCode::Unsupported,
                    "Credential operation is unsupported",
                )
            })?
            .check()?;
        context.check_at(Instant::now())?;
        if let Some(reason) = context.cancellation.reason() {
            return Ok(Some(Completion::Cancelled(reason)));
        }
        Ok(None)
    }
    pub fn put(
        &self,
        key: &CredentialKey,
        value: SecretBytes,
        policy: SecretPolicy,
        context: &OperationContext,
    ) -> ServiceResult<SecretAccess<()>> {
        if let Some(cancelled) = self.check(SecretOperation::Put, context)? {
            return Ok(cancelled);
        }
        self.provider.validate_policy(policy)?;
        if let Some(cancelled) = self.check(SecretOperation::Put, context)? {
            return Ok(cancelled);
        }
        self.provider
            .put(&self.target(key), &value)
            .map(Completion::Completed)
    }
    pub fn get(
        &self,
        key: &CredentialKey,
        context: &OperationContext,
    ) -> ServiceResult<SecretAccess<Option<SecretBytes>>> {
        if let Some(Completion::Cancelled(reason)) = self.check(SecretOperation::Get, context)? {
            return Ok(Completion::Cancelled(reason));
        }
        self.provider
            .get(&self.target(key))
            .map(Completion::Completed)
    }
    pub fn delete(
        &self,
        key: &CredentialKey,
        context: &OperationContext,
    ) -> ServiceResult<SecretAccess<DeleteReceipt>> {
        if let Some(Completion::Cancelled(reason)) = self.check(SecretOperation::Delete, context)? {
            return Ok(Completion::Cancelled(reason));
        }
        self.provider
            .delete(&self.target(key))
            .map(Completion::Completed)
    }
    /// Account cleanup is resumable and explicitly reports partial side effects.
    /// Stop issuing writes to this account before starting logout cleanup.
    pub fn remove_account(
        &self,
        account: &str,
        context: &OperationContext,
    ) -> ServiceResult<CleanupReceipt> {
        validate(account)?;
        if let Some(Completion::Cancelled(reason)) =
            self.check(SecretOperation::RemoveAccount, context)?
        {
            return Ok(Completion::Cancelled(reason));
        }
        let prefix = format!("{}{}/", self.prefix, hex(account));
        let targets = match self.provider.list(&prefix)? {
            SecretAccess::Available(targets) => targets,
            SecretAccess::Locked => {
                return Ok(Completion::Completed(CleanupReceipt {
                    deleted: 0,
                    state: CleanupState::Locked,
                }));
            }
            SecretAccess::KeyInvalidated => {
                return Ok(Completion::Completed(CleanupReceipt {
                    deleted: 0,
                    state: CleanupState::KeyInvalidated,
                }));
            }
        };
        if targets.len() > 4096 || targets.iter().any(|target| !valid_target(target, &prefix)) {
            return Err(error(
                ErrorCode::IntegrityFailure,
                "Credential enumeration violated its namespace bound",
            ));
        }
        let mut receipt = CleanupReceipt {
            deleted: 0,
            state: CleanupState::Complete,
        };
        for target in targets {
            if let Some(reason) = context.cancellation.reason() {
                receipt.state = CleanupState::Cancelled(reason);
                break;
            }
            if let Err(failure) = context.check_at(Instant::now()) {
                receipt.state = CleanupState::Failed(failure.code);
                break;
            }
            match self.provider.delete(&target) {
                Ok(SecretAccess::Available(result)) => {
                    receipt.deleted += usize::from(result.existed)
                }
                Ok(SecretAccess::Locked) => {
                    receipt.state = CleanupState::Locked;
                    break;
                }
                Ok(SecretAccess::KeyInvalidated) => {
                    receipt.state = CleanupState::KeyInvalidated;
                    break;
                }
                Err(failure) => {
                    receipt.state = CleanupState::Failed(failure.code);
                    break;
                }
            }
        }
        Ok(Completion::Completed(receipt))
    }
}
fn valid_target(target: &str, prefix: &str) -> bool {
    let Some(suffix) = target.strip_prefix(prefix) else {
        return false;
    };
    let parts: Vec<_> = suffix.split('/').collect();
    parts.len() == 2
        && parts.iter().all(|part| {
            !part.is_empty()
                && part.len() <= 256
                && part.len() % 2 == 0
                && part
                    .bytes()
                    .all(|b| b.is_ascii_digit() || matches!(b, b'a'..=b'f'))
        })
}
/// No insecure fallback is ever selected on platforms without an adapter.
pub struct UnavailableCredentials;
impl CredentialProvider for UnavailableCredentials {
    fn capabilities(&self) -> Vec<Capability<SecretOperation>> {
        capabilities(Availability::Unavailable, Evidence::ContractOnly)
    }
    fn validate_policy(&self, _: SecretPolicy) -> Result<(), SystemError> {
        Err(error(
            ErrorCode::Unavailable,
            "Secure credential provider is unavailable",
        ))
    }
    fn put(&self, _: &str, _: &SecretBytes) -> ProviderResult<()> {
        Err(unavailable())
    }
    fn get(&self, _: &str) -> ProviderResult<Option<SecretBytes>> {
        Err(unavailable())
    }
    fn delete(&self, _: &str) -> ProviderResult<DeleteReceipt> {
        Err(unavailable())
    }
    fn list(&self, _: &str) -> ProviderResult<Vec<String>> {
        Err(unavailable())
    }
}
fn unavailable() -> SystemError {
    error(
        ErrorCode::Unavailable,
        "Secure credential provider is unavailable",
    )
}
pub(crate) fn capabilities(
    availability: Availability,
    evidence: Evidence,
) -> Vec<Capability<SecretOperation>> {
    [
        SecretOperation::Put,
        SecretOperation::Get,
        SecretOperation::Delete,
        SecretOperation::RemoveAccount,
    ]
    .into_iter()
    .map(|operation| Capability {
        operation,
        support: Support::Native,
        availability,
        configuration: Configuration::Ready,
        permission: PermissionState::NotRequired,
        evidence,
        constraints: vec![],
        observed_revision: 1,
    })
    .collect()
}
