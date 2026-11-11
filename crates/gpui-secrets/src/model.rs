use gpui_system::{ErrorCode, SystemError};
use zeroize::Zeroizing;

pub const MAX_SECRET_BYTES: usize = 2560;

/// Deliberately has no Debug, Display, Clone or serialization implementation.
/// Borrow explicitly and retain only as long as needed. Clearing is best effort;
/// callers and operating systems may have made other copies.
/// ```compile_fail
/// let secret = gpui_secrets::SecretBytes::new(vec![1]).unwrap();
/// println!("{secret:?}");
/// ```
/// ```compile_fail
/// let secret = gpui_secrets::SecretBytes::new(vec![1]).unwrap();
/// let copied = secret.clone();
/// ```
pub struct SecretBytes(Zeroizing<Vec<u8>>);
impl SecretBytes {
    pub fn new(bytes: Vec<u8>) -> Result<Self, SystemError> {
        let bytes = Zeroizing::new(bytes);
        if bytes.len() > MAX_SECRET_BYTES {
            return Err(error(
                ErrorCode::InvalidInput,
                "Credential exceeds the size bound",
            ));
        }
        Ok(Self(bytes))
    }
    pub fn expose(&self) -> &[u8] {
        &self.0
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct CredentialKey {
    pub(crate) account: String,
    pub(crate) service: String,
    pub(crate) purpose: String,
}
impl CredentialKey {
    pub fn new(account: &str, service: &str, purpose: &str) -> Result<Self, SystemError> {
        validate(account)?;
        validate(service)?;
        validate(purpose)?;
        Ok(Self {
            account: account.into(),
            service: service.into(),
            purpose: purpose.into(),
        })
    }
}

/// A required guarantee is never silently weakened. These are requirements,
/// not permission prompts. Protected keys and user verification have other APIs.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct SecretPolicy {
    pub require_device_binding: bool,
    pub require_unlocked_device: bool,
    pub require_user_presence: bool,
    pub prohibit_synchronization: bool,
    pub prohibit_backup: bool,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SecretOperation {
    Put,
    Get,
    Delete,
    RemoveAccount,
}
/// Locked and invalidated are recoverable provider states, not a missing record.
pub enum SecretAccess<T> {
    Available(T),
    Locked,
    KeyInvalidated,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DeleteReceipt {
    pub existed: bool,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CleanupState {
    Complete,
    Cancelled(gpui_system::CancelReason),
    Locked,
    KeyInvalidated,
    Failed(ErrorCode),
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CleanupReceipt {
    pub deleted: usize,
    pub state: CleanupState,
}

pub(crate) fn validate(value: &str) -> Result<(), SystemError> {
    if value.is_empty() || value.len() > 128 || value.chars().any(char::is_control) {
        return Err(error(
            ErrorCode::InvalidInput,
            "Credential identifiers must be nonempty and bounded",
        ));
    }
    Ok(())
}
pub(crate) fn hex(value: &str) -> String {
    use std::fmt::Write;
    let mut result = String::with_capacity(value.len() * 2);
    for byte in value.as_bytes() {
        let _ = write!(result, "{byte:02x}");
    }
    result
}
pub(crate) fn error(code: ErrorCode, message: &'static str) -> SystemError {
    SystemError::new(code, message)
}
