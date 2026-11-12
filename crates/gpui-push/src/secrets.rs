//! Strict `gpui-secrets` isolation for push registration material.
//!
//! Raw device tokens, Web Push endpoint capability URLs, `p256dh` keys and
//! `auth` secrets are written only here, under the [`PUSH_SECRET_SERVICE`]
//! namespace and explicit per-purpose keys. This module never calls
//! [`gpui_secrets::Secrets::remove_account`]: that would delete the same account
//! name in unrelated service namespaces. Deletion always targets the exact
//! `gpui-push/v1` purposes.

use crate::model::{PushAccountId, SecretCleanupBlocker, TokenFingerprint};
use crate::provider::{RegistrationMaterial, WebPushRegistrationMaterial};
use gpui_secrets::{
    CredentialKey, CredentialProvider, DeleteReceipt, ProviderResult, SecretAccess, SecretBytes,
    SecretOperation, SecretPolicy, Secrets,
};
use gpui_system::{
    Availability, CancelReason, Capability, Completion, Configuration, ErrorCode, Evidence,
    OperationContext, PermissionState, Support, SystemError,
};
use std::{
    collections::BTreeMap,
    fmt,
    sync::{
        Mutex,
        atomic::{AtomicU8, Ordering},
    },
    time::Instant,
};

/// Service namespace used for every push credential.
pub const PUSH_SECRET_SERVICE: &str = "gpui-push/v1";
/// Purpose for the raw APNs/FCM/WNS device token.
pub const PUSH_PURPOSE_DEVICE_TOKEN: &str = "device-token";
/// Purpose for the Web Push endpoint capability URL.
pub const PUSH_PURPOSE_WEB_PUSH_ENDPOINT: &str = "web-push-endpoint";
/// Purpose for the Web Push `p256dh` subscription key.
pub const PUSH_PURPOSE_WEB_PUSH_P256DH: &str = "web-push-p256dh";
/// Purpose for the Web Push `auth` subscription secret.
pub const PUSH_PURPOSE_WEB_PUSH_AUTH: &str = "web-push-auth";
/// Every purpose this crate may write or delete.
pub const PUSH_SECRET_PURPOSES: [&str; 4] = [
    PUSH_PURPOSE_DEVICE_TOKEN,
    PUSH_PURPOSE_WEB_PUSH_ENDPOINT,
    PUSH_PURPOSE_WEB_PUSH_P256DH,
    PUSH_PURPOSE_WEB_PUSH_AUTH,
];

/// Reserved credential account for an unbound device. `/` is valid for a
/// `gpui-secrets` account component but invalid for a [`PushAccountId`], so a
/// real account can never collide with this namespace.
const DEVICE_SECRET_ACCOUNT: &str = "gpui-push/device";

/// Project a push account into its `gpui-secrets` account segment.
///
/// A bound account uses the opaque account identity directly, exactly like
/// `gpui-auth` and `gpui-network`, so sibling secrets under the same account
/// share one namespace. `None` maps to the reserved device namespace.
pub fn secret_account_name(account: Option<&PushAccountId>) -> String {
    match account {
        Some(account) => account.as_str().to_owned(),
        None => String::from(DEVICE_SECRET_ACCOUNT),
    }
}

/// A typed secret-vault failure. `Locked` and `KeyInvalidated` are recoverable
/// provider states, never a missing record and never success.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SecretVaultError {
    Locked,
    KeyInvalidated,
    Cancelled(CancelReason),
    System(SystemError),
}

impl SecretVaultError {
    pub const fn blocker(&self) -> Option<SecretCleanupBlocker> {
        match self {
            Self::Locked => Some(SecretCleanupBlocker::Locked),
            Self::KeyInvalidated => Some(SecretCleanupBlocker::KeyInvalidated),
            _ => None,
        }
    }

    pub fn into_system_error(self) -> SystemError {
        match self {
            Self::Locked => SecretCleanupBlocker::Locked.error(),
            Self::KeyInvalidated => SecretCleanupBlocker::KeyInvalidated.error(),
            Self::Cancelled(reason) => SystemError::new(
                ErrorCode::ContextExpired,
                match reason {
                    CancelReason::User => "Push credential access was cancelled by the user",
                    CancelReason::Caller => "Push credential access was cancelled by the caller",
                    CancelReason::OwnerDisposed => {
                        "Push credential access was cancelled because the owner was disposed"
                    }
                    CancelReason::ProcessPolicy => {
                        "Push credential access was cancelled by process policy"
                    }
                },
            ),
            Self::System(error) => error,
        }
    }
}

impl From<SystemError> for SecretVaultError {
    fn from(error: SystemError) -> Self {
        Self::System(error)
    }
}

impl fmt::Display for SecretVaultError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Locked => formatter.write_str("push secret vault is locked"),
            Self::KeyInvalidated => formatter.write_str("push secret vault key was invalidated"),
            Self::Cancelled(reason) => {
                write!(formatter, "push secret access cancelled: {reason:?}")
            }
            Self::System(error) => write!(formatter, "{error}"),
        }
    }
}

/// Web Push subscription credentials exposed only through a background lease.
#[derive(Clone)]
pub struct WebPushCredentials {
    endpoint: Vec<u8>,
    p256dh: Vec<u8>,
    auth: Vec<u8>,
}

impl WebPushCredentials {
    pub fn new(
        endpoint: impl Into<Vec<u8>>,
        p256dh: impl Into<Vec<u8>>,
        auth: impl Into<Vec<u8>>,
    ) -> Result<Self, SystemError> {
        let endpoint = endpoint.into();
        let p256dh = p256dh.into();
        let auth = auth.into();
        if endpoint.is_empty() || p256dh.is_empty() || auth.is_empty() {
            return Err(SystemError::new(
                ErrorCode::IntegrityFailure,
                "Stored Web Push credentials are incomplete",
            ));
        }
        Ok(Self {
            endpoint,
            p256dh,
            auth,
        })
    }

    pub fn endpoint(&self) -> &[u8] {
        &self.endpoint
    }

    pub fn p256dh(&self) -> &[u8] {
        &self.p256dh
    }

    pub fn auth(&self) -> &[u8] {
        &self.auth
    }

    /// Take the three secrets, leaving the struct inert for its zeroizing drop.
    pub fn into_material(
        mut self,
    ) -> Result<crate::provider::WebPushRegistrationMaterial, SystemError> {
        crate::provider::WebPushRegistrationMaterial::new(
            std::mem::take(&mut self.endpoint),
            std::mem::take(&mut self.p256dh),
            std::mem::take(&mut self.auth),
        )
    }
}

impl Drop for WebPushCredentials {
    fn drop(&mut self) {
        self.endpoint.fill(0);
        self.p256dh.fill(0);
        self.auth.fill(0);
    }
}

impl fmt::Debug for WebPushCredentials {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("WebPushCredentials")
            .field("endpoint_bytes", &self.endpoint.len())
            .field("p256dh_bytes", &self.p256dh.len())
            .field("auth_bytes", &self.auth.len())
            .finish()
    }
}

/// Background-only lease over the active registration credentials.
///
/// It has no `Clone`, no serialization and a redacted `Debug`, and it zeroizes
/// on drop. Hosts must not retain or log the borrowed slices.
pub struct RegistrationCredentialLease {
    account: Option<PushAccountId>,
    registration_epoch: u64,
    token_revision: u64,
    fingerprint: TokenFingerprint,
    device_token: Option<Vec<u8>>,
    web_push: Option<WebPushCredentials>,
}

impl RegistrationCredentialLease {
    pub fn account(&self) -> Option<&PushAccountId> {
        self.account.as_ref()
    }

    pub const fn registration_epoch(&self) -> u64 {
        self.registration_epoch
    }

    pub const fn token_revision(&self) -> u64 {
        self.token_revision
    }

    pub fn fingerprint(&self) -> &TokenFingerprint {
        &self.fingerprint
    }

    pub fn device_token(&self) -> Option<&[u8]> {
        self.device_token.as_deref()
    }

    pub fn web_push(&self) -> Option<&WebPushCredentials> {
        self.web_push.as_ref()
    }

    /// Take the raw registration material, leaving the lease inert for its
    /// zeroizing drop. Used only to re-key the same device credentials when the
    /// active account changes.
    pub fn into_material(mut self) -> Result<RegistrationMaterial, SystemError> {
        let web_push = match self.web_push.take() {
            Some(web_push) => Some(web_push.into_material()?),
            None => None,
        };
        RegistrationMaterial::new(self.device_token.take(), web_push)
    }
}

impl Drop for RegistrationCredentialLease {
    fn drop(&mut self) {
        if let Some(token) = &mut self.device_token {
            token.fill(0);
        }
    }
}

impl fmt::Debug for RegistrationCredentialLease {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RegistrationCredentialLease")
            .field("account", &self.account)
            .field("registration_epoch", &self.registration_epoch)
            .field("token_revision", &self.token_revision)
            .field("fingerprint", &self.fingerprint)
            .field(
                "device_token_bytes",
                &self.device_token.as_ref().map(Vec::len),
            )
            .field("web_push", &self.web_push)
            .finish()
    }
}

/// The push purposes stored for one account before a reversible write.
///
/// It exists only to roll a failed rotation back to the previous revision. It
/// has no `Clone`, no serialization and a redacted `Debug`, and it zeroizes its
/// bytes on drop.
pub struct RegistrationSecretSnapshot {
    account: Option<PushAccountId>,
    values: Vec<(&'static str, Vec<u8>)>,
}

impl RegistrationSecretSnapshot {
    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }

    pub fn len(&self) -> usize {
        self.values.len()
    }

    pub fn account(&self) -> Option<&PushAccountId> {
        self.account.as_ref()
    }
}

impl Drop for RegistrationSecretSnapshot {
    fn drop(&mut self) {
        for (_, value) in &mut self.values {
            value.fill(0);
        }
        self.values.clear();
    }
}

impl fmt::Debug for RegistrationSecretSnapshot {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RegistrationSecretSnapshot")
            .field("account", &self.account)
            .field("purposes", &self.values.len())
            .finish()
    }
}

/// Thin, isolated wrapper around [`Secrets`].
#[derive(Clone)]
pub struct PushSecretVault {
    secrets: Secrets,
}

impl PushSecretVault {
    pub fn new(secrets: Secrets) -> Self {
        Self { secrets }
    }

    pub fn secrets(&self) -> &Secrets {
        &self.secrets
    }

    /// Persist registration material under the account's push namespace.
    pub fn store_registration(
        &self,
        account: Option<&PushAccountId>,
        material: RegistrationMaterial,
        context: &OperationContext,
    ) -> Result<TokenFingerprint, SecretVaultError> {
        self.store_registration_reversible(account, material, context)
            .map(|(fingerprint, _)| fingerprint)
    }

    /// Persist registration material and return the prior purpose snapshot so a
    /// caller can restore it if the matching durable-state write fails.
    ///
    /// The write is transactional across all four purposes: unused purposes are
    /// deleted so a provider-type switch never leaves both `device-token` and
    /// `web-push-*` behind, and any failure restores the prior snapshot instead
    /// of wiping a previously valid registration.
    pub fn store_registration_reversible(
        &self,
        account: Option<&PushAccountId>,
        material: RegistrationMaterial,
        context: &OperationContext,
    ) -> Result<(TokenFingerprint, RegistrationSecretSnapshot), SecretVaultError> {
        ensure_background(context)?;
        let fingerprint = material.fingerprint().clone();
        let snapshot = self.snapshot_registration(account, context)?;
        let (device_token, web_push) = material.into_parts();
        let mut written: Vec<&'static str> = Vec::new();
        let written_result = (|| {
            if let Some(token) = device_token {
                self.put(account, PUSH_PURPOSE_DEVICE_TOKEN, token, context)?;
                written.push(PUSH_PURPOSE_DEVICE_TOKEN);
            }
            if let Some(web_push) = web_push {
                self.write_web_push(account, web_push, context, &mut written)?;
            }
            Ok(())
        })();
        if let Err(failure) = written_result {
            let _ = self.restore_registration(&snapshot, context);
            return Err(failure);
        }
        let unused: Vec<&'static str> = PUSH_SECRET_PURPOSES
            .into_iter()
            .filter(|purpose| !written.contains(purpose))
            .collect();
        if let Err(failure) = self.delete_purposes(account, &unused, context) {
            let _ = self.restore_registration(&snapshot, context);
            return Err(failure);
        }
        Ok((fingerprint, snapshot))
    }

    /// Snapshot the four push purposes currently stored for one account.
    pub fn snapshot_registration(
        &self,
        account: Option<&PushAccountId>,
        context: &OperationContext,
    ) -> Result<RegistrationSecretSnapshot, SecretVaultError> {
        ensure_background(context)?;
        context.check_at(Instant::now())?;
        let mut values = Vec::with_capacity(PUSH_SECRET_PURPOSES.len());
        for purpose in PUSH_SECRET_PURPOSES {
            if let Some(value) = self.get(account, purpose, context)? {
                values.push((purpose, value));
            }
        }
        Ok(RegistrationSecretSnapshot {
            account: account.cloned(),
            values,
        })
    }

    /// Best-effort rollback to a previous snapshot. Every purpose is deleted and
    /// only the snapshotted values are rewritten, so a partial prior write cannot
    /// survive. A vault that locks mid-restore leaves the cleanup to the caller's
    /// tombstone path.
    pub fn restore_registration(
        &self,
        snapshot: &RegistrationSecretSnapshot,
        context: &OperationContext,
    ) -> Result<(), SecretVaultError> {
        ensure_background(context)?;
        let account = snapshot.account.as_ref();
        self.delete_purposes(account, &PUSH_SECRET_PURPOSES, context)?;
        for (purpose, value) in &snapshot.values {
            self.put(account, purpose, value.clone(), context)?;
        }
        Ok(())
    }

    fn write_web_push(
        &self,
        account: Option<&PushAccountId>,
        web_push: WebPushRegistrationMaterial,
        context: &OperationContext,
        written: &mut Vec<&'static str>,
    ) -> Result<(), SecretVaultError> {
        let (endpoint, p256dh, auth) = web_push.into_parts();
        self.put(account, PUSH_PURPOSE_WEB_PUSH_ENDPOINT, endpoint, context)?;
        written.push(PUSH_PURPOSE_WEB_PUSH_ENDPOINT);
        self.put(account, PUSH_PURPOSE_WEB_PUSH_P256DH, p256dh, context)?;
        written.push(PUSH_PURPOSE_WEB_PUSH_P256DH);
        self.put(account, PUSH_PURPOSE_WEB_PUSH_AUTH, auth, context)?;
        written.push(PUSH_PURPOSE_WEB_PUSH_AUTH);
        Ok(())
    }

    fn put(
        &self,
        account: Option<&PushAccountId>,
        purpose: &'static str,
        value: Vec<u8>,
        context: &OperationContext,
    ) -> Result<(), SecretVaultError> {
        ensure_background(context)?;
        let key = self.key(account, purpose)?;
        let bytes = SecretBytes::new(value)?;
        match self
            .secrets
            .put(&key, bytes, SecretPolicy::default(), context)?
        {
            Completion::Completed(SecretAccess::Available(())) => Ok(()),
            Completion::Completed(SecretAccess::Locked) => Err(SecretVaultError::Locked),
            Completion::Completed(SecretAccess::KeyInvalidated) => {
                Err(SecretVaultError::KeyInvalidated)
            }
            Completion::Cancelled(reason) => Err(SecretVaultError::Cancelled(reason)),
        }
    }

    fn get(
        &self,
        account: Option<&PushAccountId>,
        purpose: &'static str,
        context: &OperationContext,
    ) -> Result<Option<Vec<u8>>, SecretVaultError> {
        ensure_background(context)?;
        let key = self.key(account, purpose)?;
        match self.secrets.get(&key, context)? {
            Completion::Completed(SecretAccess::Available(Some(bytes))) => {
                Ok(Some(bytes.expose().to_vec()))
            }
            Completion::Completed(SecretAccess::Available(None)) => Ok(None),
            Completion::Completed(SecretAccess::Locked) => Err(SecretVaultError::Locked),
            Completion::Completed(SecretAccess::KeyInvalidated) => {
                Err(SecretVaultError::KeyInvalidated)
            }
            Completion::Cancelled(reason) => Err(SecretVaultError::Cancelled(reason)),
        }
    }

    fn delete_purposes(
        &self,
        account: Option<&PushAccountId>,
        purposes: &[&'static str],
        context: &OperationContext,
    ) -> Result<usize, SecretVaultError> {
        ensure_background(context)?;
        let mut deleted = 0usize;
        for purpose in purposes {
            let key = self.key(account, purpose)?;
            match self.secrets.delete(&key, context)? {
                Completion::Completed(SecretAccess::Available(receipt)) => {
                    deleted = deleted.saturating_add(usize::from(receipt.existed));
                }
                Completion::Completed(SecretAccess::Locked) => {
                    return Err(SecretVaultError::Locked);
                }
                Completion::Completed(SecretAccess::KeyInvalidated) => {
                    return Err(SecretVaultError::KeyInvalidated);
                }
                Completion::Cancelled(reason) => return Err(SecretVaultError::Cancelled(reason)),
            }
            if let Some(reason) = context.cancellation.reason() {
                return Err(SecretVaultError::Cancelled(reason));
            }
        }
        Ok(deleted)
    }

    /// Delete every `gpui-push/v1` purpose for one account namespace.
    ///
    /// This is the only deletion path. It never calls `remove_account`.
    pub fn delete_registration(
        &self,
        account: Option<&PushAccountId>,
        context: &OperationContext,
    ) -> Result<usize, SecretVaultError> {
        self.delete_purposes(account, &PUSH_SECRET_PURPOSES, context)
    }

    /// Delete both the account namespace and, when `device_scope` is set, the
    /// reserved device namespace. `gpui-push/v1` entries only.
    pub fn purge(
        &self,
        account: Option<&PushAccountId>,
        device_scope: bool,
        context: &OperationContext,
    ) -> Result<usize, SecretVaultError> {
        let mut deleted = self.delete_registration(account, context)?;
        if device_scope && account.is_some() {
            deleted = deleted.saturating_add(self.delete_registration(None, context)?);
        }
        Ok(deleted)
    }

    /// Read the active registration credential lease. The caller must already
    /// have verified the account, tombstone and registration epoch.
    pub fn read_lease(
        &self,
        account: Option<&PushAccountId>,
        registration_epoch: u64,
        token_revision: u64,
        fingerprint: &TokenFingerprint,
        context: &OperationContext,
    ) -> Result<Option<RegistrationCredentialLease>, SecretVaultError> {
        ensure_background(context)?;
        context.check_at(Instant::now())?;
        let device_token = self.get(account, PUSH_PURPOSE_DEVICE_TOKEN, context)?;
        let endpoint = self.get(account, PUSH_PURPOSE_WEB_PUSH_ENDPOINT, context)?;
        let p256dh = self.get(account, PUSH_PURPOSE_WEB_PUSH_P256DH, context)?;
        let auth = self.get(account, PUSH_PURPOSE_WEB_PUSH_AUTH, context)?;
        let web_push = match (endpoint, p256dh, auth) {
            (None, None, None) => None,
            (Some(endpoint), Some(p256dh), Some(auth)) => {
                Some(WebPushCredentials::new(endpoint, p256dh, auth)?)
            }
            _ => {
                return Err(SecretVaultError::System(SystemError::new(
                    ErrorCode::IntegrityFailure,
                    "Stored Web Push credentials are incomplete",
                )));
            }
        };
        if device_token.is_none() && web_push.is_none() {
            return Ok(None);
        }
        Ok(Some(RegistrationCredentialLease {
            account: account.cloned(),
            registration_epoch,
            token_revision,
            fingerprint: fingerprint.clone(),
            device_token,
            web_push,
        }))
    }

    fn key(
        &self,
        account: Option<&PushAccountId>,
        purpose: &'static str,
    ) -> Result<CredentialKey, SecretVaultError> {
        Ok(CredentialKey::new(
            &secret_account_name(account),
            PUSH_SECRET_SERVICE,
            purpose,
        )?)
    }
}

impl fmt::Debug for PushSecretVault {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PushSecretVault")
            .field("service", &PUSH_SECRET_SERVICE)
            .finish_non_exhaustive()
    }
}

/// Deterministic in-memory credential provider for contract tests.
///
/// It can simulate a locked or invalidated vault, counts stored values and
/// searches stored bytes so tests can prove that raw material never appears in
/// durable state or public output.
#[derive(Default)]
pub struct MemoryCredentials {
    records: Mutex<BTreeMap<String, Vec<u8>>>,
    mode: AtomicU8,
    /// One-shot put failure injection, in `remaining` put calls.
    fail_put: std::sync::atomic::AtomicUsize,
}

impl MemoryCredentials {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set_locked(&self, locked: bool) {
        self.mode.store(u8::from(locked), Ordering::SeqCst);
    }

    pub fn set_key_invalidated(&self, invalidated: bool) {
        self.mode
            .store(if invalidated { 2 } else { 0 }, Ordering::SeqCst);
    }

    pub fn unlock(&self) {
        self.mode.store(0, Ordering::SeqCst);
    }

    pub fn is_locked(&self) -> bool {
        self.mode.load(Ordering::SeqCst) == 1
    }

    /// Fail exactly the `after`-th upcoming `put` call, once, reporting a locked
    /// vault. Used to prove a mid-rotation failure rolls back to the previous
    /// revision instead of wiping it.
    pub fn fail_put_after(&self, after: usize) {
        self.fail_put.store(after, Ordering::SeqCst);
    }

    pub fn put_failure_pending(&self) -> bool {
        self.fail_put.load(Ordering::SeqCst) > 0
    }

    fn consume_put_failure(&self) -> bool {
        loop {
            let remaining = self.fail_put.load(Ordering::SeqCst);
            if remaining == 0 {
                return false;
            }
            if self
                .fail_put
                .compare_exchange(remaining, remaining - 1, Ordering::SeqCst, Ordering::SeqCst)
                .is_ok()
            {
                return remaining == 1;
            }
        }
    }

    pub fn value_count(&self) -> usize {
        lock(&self.records).len()
    }

    pub fn is_empty(&self) -> bool {
        lock(&self.records).is_empty()
    }

    pub fn targets(&self) -> Vec<String> {
        lock(&self.records).keys().cloned().collect()
    }

    /// True when any stored value contains the needle.
    pub fn values_contain(&self, needle: &[u8]) -> bool {
        if needle.is_empty() {
            return false;
        }
        lock(&self.records)
            .values()
            .any(|value| value.windows(needle.len()).any(|window| window == needle))
    }

    /// Number of stored targets that belong to one service namespace.
    pub fn service_target_count(&self, service: &str) -> usize {
        let encoded = hex_text(service);
        lock(&self.records)
            .keys()
            .filter(|target| target.contains(&encoded))
            .count()
    }

    pub fn clear(&self) {
        lock(&self.records).clear();
    }

    fn access<T>(&self, value: T) -> ProviderResult<T> {
        match self.mode.load(Ordering::SeqCst) {
            1 => Ok(SecretAccess::Locked),
            2 => Ok(SecretAccess::KeyInvalidated),
            _ => Ok(SecretAccess::Available(value)),
        }
    }
}

impl CredentialProvider for MemoryCredentials {
    fn capabilities(&self) -> Vec<Capability<SecretOperation>> {
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
            availability: Availability::Available,
            configuration: Configuration::Ready,
            permission: PermissionState::NotRequired,
            evidence: Evidence::ContractOnly,
            constraints: Vec::new(),
            observed_revision: 1,
        })
        .collect()
    }

    fn validate_policy(&self, _policy: SecretPolicy) -> Result<(), SystemError> {
        Ok(())
    }

    fn put(&self, target: &str, value: &SecretBytes) -> ProviderResult<()> {
        if self.consume_put_failure() {
            return Ok(SecretAccess::Locked);
        }
        if self.mode.load(Ordering::SeqCst) != 0 {
            return self.access(());
        }
        lock(&self.records).insert(target.to_owned(), value.expose().to_vec());
        Ok(SecretAccess::Available(()))
    }

    fn get(&self, target: &str) -> ProviderResult<Option<SecretBytes>> {
        if self.mode.load(Ordering::SeqCst) != 0 {
            return self.access(None);
        }
        let value = lock(&self.records).get(target).cloned();
        match value {
            Some(value) => Ok(SecretAccess::Available(Some(SecretBytes::new(value)?))),
            None => Ok(SecretAccess::Available(None)),
        }
    }

    fn delete(&self, target: &str) -> ProviderResult<DeleteReceipt> {
        if self.mode.load(Ordering::SeqCst) != 0 {
            return self.access(DeleteReceipt { existed: false });
        }
        let existed = lock(&self.records).remove(target).is_some();
        Ok(SecretAccess::Available(DeleteReceipt { existed }))
    }

    fn list(&self, prefix: &str) -> ProviderResult<Vec<String>> {
        if self.mode.load(Ordering::SeqCst) != 0 {
            return self.access(Vec::new());
        }
        Ok(SecretAccess::Available(
            lock(&self.records)
                .keys()
                .filter(|target| target.starts_with(prefix))
                .cloned()
                .collect(),
        ))
    }
}

impl fmt::Debug for MemoryCredentials {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("MemoryCredentials")
            .field("values", &self.value_count())
            .field("mode", &self.mode.load(Ordering::SeqCst))
            .finish()
    }
}

fn hex_text(value: &str) -> String {
    let mut encoded = String::with_capacity(value.len() * 2);
    for byte in value.as_bytes() {
        encoded.push_str(&format!("{byte:02x}"));
    }
    encoded
}

fn lock<T>(value: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    value
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn ensure_background(context: &OperationContext) -> Result<(), SecretVaultError> {
    if context.ui.is_some() {
        return Err(SecretVaultError::System(SystemError::new(
            ErrorCode::InvalidInput,
            "Push credentials require a background context without a UI attachment",
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::WebPushRegistrationMaterial;
    use std::sync::Arc;

    fn vault() -> (PushSecretVault, Arc<MemoryCredentials>) {
        let credentials = Arc::new(MemoryCredentials::new());
        let secrets =
            Secrets::with_provider("gpui-push-secret-tests", credentials.clone()).expect("secrets");
        (PushSecretVault::new(secrets), credentials)
    }

    fn web_material(endpoint: &[u8]) -> RegistrationMaterial {
        RegistrationMaterial::new(
            None,
            Some(
                WebPushRegistrationMaterial::new(
                    endpoint.to_vec(),
                    b"p256dh-key".to_vec(),
                    b"auth-secret".to_vec(),
                )
                .expect("web push material"),
            ),
        )
        .expect("material")
    }

    #[test]
    fn switching_material_cleans_unused_purposes() {
        let (vault, credentials) = vault();
        let context = OperationContext::default();
        let account = PushAccountId::new("switch-user").expect("account");
        let first = vault
            .store_registration(
                Some(&account),
                RegistrationMaterial::new(Some(b"device-token-1".to_vec()), None)
                    .expect("token material"),
                &context,
            )
            .expect("store token");
        assert_eq!(credentials.value_count(), 1);
        assert_eq!(
            vault
                .read_lease(Some(&account), 1, 1, &first, &context)
                .expect("lease")
                .expect("present")
                .device_token(),
            Some(&b"device-token-1"[..])
        );

        let second = vault
            .store_registration(
                Some(&account),
                web_material(b"https://push.test/one"),
                &context,
            )
            .expect("store web push");
        let lease = vault
            .read_lease(Some(&account), 1, 2, &second, &context)
            .expect("lease")
            .expect("present");
        assert!(lease.device_token().is_none());
        assert_eq!(
            lease.web_push().expect("web push").endpoint(),
            b"https://push.test/one"
        );
        assert_eq!(credentials.value_count(), 3);
        assert!(!credentials.values_contain(b"device-token-1"));

        // And back again: the three web-push purposes are removed.
        let third = vault
            .store_registration(
                Some(&account),
                RegistrationMaterial::new(Some(b"device-token-3".to_vec()), None)
                    .expect("token material"),
                &context,
            )
            .expect("store token again");
        assert_eq!(credentials.value_count(), 1);
        assert!(!credentials.values_contain(b"p256dh-key"));
        assert_eq!(
            vault
                .read_lease(Some(&account), 1, 3, &third, &context)
                .expect("lease")
                .expect("present")
                .device_token(),
            Some(&b"device-token-3"[..])
        );
    }

    #[test]
    fn mid_rotation_failure_restores_the_previous_revision() {
        let (vault, credentials) = vault();
        let context = OperationContext::default();
        let account = PushAccountId::new("rollback-user").expect("account");
        let first = vault
            .store_registration(
                Some(&account),
                web_material(b"https://push.test/first"),
                &context,
            )
            .expect("store first");
        assert_eq!(credentials.value_count(), 3);

        // The endpoint write succeeds, the `p256dh` write fails.
        credentials.fail_put_after(2);
        let failure = vault
            .store_registration(
                Some(&account),
                web_material(b"https://push.test/second"),
                &context,
            )
            .expect_err("mid-rotation failure");
        assert!(matches!(failure, SecretVaultError::Locked));
        assert!(!credentials.put_failure_pending());

        let lease = vault
            .read_lease(Some(&account), 1, 1, &first, &context)
            .expect("lease")
            .expect("present");
        assert_eq!(
            lease.web_push().expect("web push").endpoint(),
            b"https://push.test/first"
        );
        assert_eq!(lease.web_push().expect("web push").p256dh(), b"p256dh-key");
        assert_eq!(credentials.value_count(), 3);
        assert!(!credentials.values_contain(b"push.test/second"));
    }

    #[test]
    fn snapshot_is_redacted_and_restores_exactly() {
        let (vault, credentials) = vault();
        let context = OperationContext::default();
        let account = PushAccountId::new("snapshot-user").expect("account");
        let fingerprint = vault
            .store_registration(
                Some(&account),
                RegistrationMaterial::new(Some(b"snapshot-token".to_vec()), None)
                    .expect("token material"),
                &context,
            )
            .expect("store");
        let snapshot = vault
            .snapshot_registration(Some(&account), &context)
            .expect("snapshot");
        assert_eq!(snapshot.len(), 1);
        assert!(!format!("{snapshot:?}").contains("snapshot-token"));

        vault
            .store_registration(
                Some(&account),
                web_material(b"https://push.test/x"),
                &context,
            )
            .expect("replace");
        assert_eq!(credentials.value_count(), 3);
        vault
            .restore_registration(&snapshot, &context)
            .expect("restore");
        assert_eq!(credentials.value_count(), 1);
        assert_eq!(
            vault
                .read_lease(Some(&account), 1, 1, &fingerprint, &context)
                .expect("lease")
                .expect("present")
                .device_token(),
            Some(&b"snapshot-token"[..])
        );
    }

    #[test]
    fn secret_account_names_are_namespaced_and_bounded() {
        let account = PushAccountId::new("box-user").expect("account");
        assert_eq!(secret_account_name(None), "gpui-push/device");
        assert_eq!(secret_account_name(Some(&account)), "box-user");
        assert_ne!(
            secret_account_name(None),
            secret_account_name(Some(
                &PushAccountId::new("gpui-push").expect("reserved prefix account")
            ))
        );
    }

    #[test]
    fn lease_debug_never_exposes_material() {
        let lease = RegistrationCredentialLease {
            account: None,
            registration_epoch: 2,
            token_revision: 1,
            fingerprint: TokenFingerprint::of(b"secret-token"),
            device_token: Some(b"secret-token".to_vec()),
            web_push: None,
        };
        let rendered = format!("{lease:?}");
        assert!(!rendered.contains("secret-token"));
        assert!(rendered.contains("device_token_bytes"));
    }

    #[test]
    fn web_push_material_debug_never_exposes_secrets() {
        let material = WebPushRegistrationMaterial::new(
            b"https://push.test/x".to_vec(),
            b"P256DH-SECRET".to_vec(),
            b"AUTH-SECRET".to_vec(),
        )
        .expect("material");
        let rendered = format!("{material:?}");
        assert!(!rendered.contains("push.test"));
        assert!(!rendered.contains("P256DH-SECRET"));
        assert!(!rendered.contains("AUTH-SECRET"));
        assert!(rendered.contains("endpoint_bytes"));
    }
}
