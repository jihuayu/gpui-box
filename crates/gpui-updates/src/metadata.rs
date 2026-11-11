use crate::{UpdateChannel, background, error, value};
use ed25519_dalek::{Signature, VerifyingKey};
use gpui_app_data::{AppData, DataLocation, DataScope, SettingsStore};
use gpui_system::{Completion, ErrorCode, OperationContext, ServiceResult, SystemError};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, fs, io::Read, sync::Mutex};
use url::Url;

/// Sign the domain followed by the exact UTF-8 bytes of SignedMetadata.payload.
/// JSON is parsed only after authentication. No JSON reserialization is signed.
pub const SIGNATURE_DOMAIN: &[u8] = b"gpui-box/update-metadata/v1\0";
pub const MAX_ENVELOPE_BYTES: usize = 128 * 1024;
pub const MAX_PAYLOAD_BYTES: usize = 32 * 1024;
const MAX_LIFETIME_SECONDS: u64 = 7 * 24 * 60 * 60;
const CLOCK_SKEW_SECONDS: u64 = 300;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SignedMetadata {
    pub schema_version: u32,
    pub key_id: String,
    pub payload: String,
    /// Lowercase hexadecimal Ed25519 signature over the domain and payload.
    pub signature: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArtifactMetadata {
    pub url: String,
    pub sha256: String,
    pub size: u64,
    pub signer_sha256: String,
    pub abis: Vec<String>,
    pub minimum_api: u32,
    pub maximum_api: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UpdateMetadata {
    pub schema_version: u32,
    pub channel: UpdateChannel,
    pub application_id: String,
    pub version: String,
    pub version_code: u32,
    pub sequence: u64,
    pub issued_at: u64,
    pub expires_at: u64,
    pub artifact: ArtifactMetadata,
}

#[derive(Debug, Clone)]
pub struct InstalledIdentity {
    pub application_id: String,
    pub version_code: u32,
    pub signer_sha256: String,
    pub android_api: u32,
    pub abi: String,
}

#[derive(Clone)]
pub struct UpdateTrustPolicy {
    pub installed: InstalledIdentity,
    /// Application/CI pins. Metadata cannot add or replace trusted keys.
    pub keys: BTreeMap<String, [u8; 32]>,
    /// Exact HTTPS origins, including any non-default port. Paths are rejected.
    pub artifact_origins: Vec<String>,
    pub maximum_artifact_bytes: u64,
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Checkpoint {
    application_id: String,
    sequence: u64,
    payload_sha256: String,
    version_code: u32,
    observed_time: u64,
}

/// Holds an OS file lock for one application's trust checkpoint. Clones should
/// share an Arc<UpdateVerifier>; a second verifier on this AppData returns Busy.
/// Blocking I/O and signature/hash work must run on a background executor.
pub struct UpdateVerifier {
    policy: UpdateTrustPolicy,
    origins: Vec<url::Origin>,
    store: SettingsStore,
    checkpoint: Mutex<Checkpoint>,
    _lock: fs::File,
}

/// Authenticated metadata plus a checkpoint committed before exposure to callers.
/// Fields are private so parsed/untrusted JSON cannot be promoted into this type.
#[derive(Debug, Clone)]
pub struct VerifiedUpdate {
    metadata: UpdateMetadata,
    payload_sha256: String,
    verified_at: u64,
    key_id: String,
    public_key: [u8; 32],
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArtifactVerification {
    pub(crate) payload_sha256: String,
    pub(crate) artifact_sha256: String,
    pub(crate) size: u64,
}

impl VerifiedUpdate {
    pub fn metadata(&self) -> &UpdateMetadata {
        &self.metadata
    }
    pub fn payload_sha256(&self) -> &str {
        &self.payload_sha256
    }
    pub fn verified_at(&self) -> u64 {
        self.verified_at
    }

    /// Verifies the bounded byte stream only. The installer provider must bind
    /// these bytes to its private file, recheck APK identity/signature and avoid
    /// file substitution before handoff. This receipt grants no OS authorization.
    pub fn verify_artifact(
        &self,
        mut reader: impl Read,
        context: &OperationContext,
    ) -> ServiceResult<ArtifactVerification> {
        if let Some(cancelled) = background(context)? {
            return Ok(cancelled);
        }
        let mut hash = Sha256::new();
        let mut count = 0u64;
        let mut buffer = [0; 65536];
        loop {
            if let Some(cancelled) = background(context)? {
                return Ok(cancelled);
            }
            let length = reader
                .read(&mut buffer)
                .map_err(|_| error(ErrorCode::PlatformFailure, "Update artifact read failed"))?;
            if length == 0 {
                break;
            }
            count = count
                .checked_add(length as u64)
                .ok_or_else(|| error(ErrorCode::IntegrityFailure, "Artifact size overflow"))?;
            if count > self.metadata.artifact.size {
                return Err(error(
                    ErrorCode::IntegrityFailure,
                    "Artifact exceeds authenticated size",
                ));
            }
            hash.update(&buffer[..length]);
        }
        let digest = format!("{:x}", hash.finalize());
        if count != self.metadata.artifact.size || digest != self.metadata.artifact.sha256 {
            return Err(error(
                ErrorCode::IntegrityFailure,
                "Artifact digest or size does not match signed metadata",
            ));
        }
        if let Some(cancelled) = background(context)? {
            return Ok(cancelled);
        }
        Ok(Completion::Completed(ArtifactVerification {
            payload_sha256: self.payload_sha256.clone(),
            artifact_sha256: digest,
            size: count,
        }))
    }
}

impl UpdateVerifier {
    /// Recheck immediately before native presentation. A newer accepted payload
    /// invalidates older verified offers, including clones held by another view.
    pub fn revalidate(&self, update: &VerifiedUpdate, now: u64) -> Result<(), SystemError> {
        self.validate(update.metadata(), now)?;
        if self.policy.keys.get(&update.key_id) != Some(&update.public_key) {
            return Err(error(
                ErrorCode::IntegrityFailure,
                "Update signing key was removed or changed",
            ));
        }
        let checkpoint = self
            .checkpoint
            .lock()
            .map_err(|_| error(ErrorCode::Unavailable, "Update verifier is unavailable"))?;
        if checkpoint.payload_sha256 != update.payload_sha256
            || checkpoint.sequence != update.metadata.sequence
            || now.saturating_add(CLOCK_SKEW_SECONDS) < checkpoint.observed_time
        {
            return Err(error(
                ErrorCode::IntegrityFailure,
                "Update offer was superseded or clock rolled back",
            ));
        }
        Ok(())
    }

    pub fn open(policy: UpdateTrustPolicy, app: &AppData) -> Result<Self, SystemError> {
        let origins = validate_policy(&policy)?;
        let path = app
            .location(DataLocation::Config)
            .child("update-trust.lock")?;
        let lock = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(path)
            .map_err(|_| {
                error(
                    ErrorCode::PlatformFailure,
                    "Update trust lock could not be opened",
                )
            })?;
        lock.try_lock().map_err(|_| {
            error(
                ErrorCode::Busy,
                "Another update verifier owns the application checkpoint",
            )
        })?;
        let store = app.settings(DataScope::Application, "update-trust", 1, 8192)?;
        let checkpoint: Checkpoint =
            value(store.load(&OperationContext::default()))?.unwrap_or_default();
        if checkpoint.application_id.is_empty() {
            if checkpoint.sequence != 0
                || !checkpoint.payload_sha256.is_empty()
                || checkpoint.version_code != 0
                || checkpoint.observed_time != 0
            {
                return Err(error(
                    ErrorCode::IntegrityFailure,
                    "Empty update checkpoint contains committed state",
                ));
            }
        } else if checkpoint.sequence == 0
            || checkpoint.version_code == 0
            || hex::<32>(&checkpoint.payload_sha256).is_err()
        {
            return Err(error(
                ErrorCode::IntegrityFailure,
                "Update checkpoint is inconsistent",
            ));
        }
        if !checkpoint.application_id.is_empty()
            && checkpoint.application_id != policy.installed.application_id
        {
            return Err(error(
                ErrorCode::IntegrityFailure,
                "Update checkpoint belongs to another application",
            ));
        }
        Ok(Self {
            policy,
            origins,
            store,
            checkpoint: Mutex::new(checkpoint),
            _lock: lock,
        })
    }

    /// Authenticate and commit a replay/time checkpoint. Equal sequence is an
    /// idempotent retry only for byte-identical authenticated payloads. Installed
    /// version checks still reject a completed/downgraded release after restart.
    pub fn verify(
        &self,
        envelope: &[u8],
        now: u64,
        context: &OperationContext,
    ) -> ServiceResult<VerifiedUpdate> {
        if let Some(cancelled) = background(context)? {
            return Ok(cancelled);
        }
        if envelope.len() > MAX_ENVELOPE_BYTES {
            return Err(error(
                ErrorCode::InvalidInput,
                "Update metadata envelope exceeds its bound",
            ));
        }
        let envelope: SignedMetadata = serde_json::from_slice(envelope).map_err(|_| {
            error(
                ErrorCode::IntegrityFailure,
                "Malformed signed update envelope",
            )
        })?;
        if envelope.schema_version != 1 || envelope.payload.len() > MAX_PAYLOAD_BYTES {
            return Err(error(
                ErrorCode::IntegrityFailure,
                "Unsupported update envelope schema or payload size",
            ));
        }
        let key = self.policy.keys.get(&envelope.key_id).ok_or_else(|| {
            error(
                ErrorCode::IntegrityFailure,
                "Update signing key is not pinned",
            )
        })?;
        let key = VerifyingKey::from_bytes(key).map_err(|_| {
            error(
                ErrorCode::InvalidConfiguration,
                "Pinned update key is invalid",
            )
        })?;
        let signature = Signature::from_bytes(&hex::<64>(&envelope.signature)?);
        let mut message = SIGNATURE_DOMAIN.to_vec();
        message.extend_from_slice(envelope.payload.as_bytes());
        key.verify_strict(&message, &signature).map_err(|_| {
            error(
                ErrorCode::IntegrityFailure,
                "Update metadata signature is invalid",
            )
        })?;
        let metadata: UpdateMetadata = serde_json::from_str(&envelope.payload).map_err(|_| {
            error(
                ErrorCode::IntegrityFailure,
                "Malformed authenticated update metadata",
            )
        })?;
        self.validate(&metadata, now)?;
        let digest = format!("{:x}", Sha256::digest(envelope.payload.as_bytes()));
        let mut checkpoint = self
            .checkpoint
            .lock()
            .map_err(|_| error(ErrorCode::Unavailable, "Update verifier is unavailable"))?;
        if now.saturating_add(CLOCK_SKEW_SECONDS) < checkpoint.observed_time
            || metadata.sequence < checkpoint.sequence
            || metadata.version_code < checkpoint.version_code
            || (metadata.sequence == checkpoint.sequence && digest != checkpoint.payload_sha256)
        {
            return Err(error(
                ErrorCode::IntegrityFailure,
                "Update metadata replay or clock rollback rejected",
            ));
        }
        let next = Checkpoint {
            application_id: metadata.application_id.clone(),
            sequence: metadata.sequence,
            payload_sha256: digest.clone(),
            version_code: metadata.version_code,
            observed_time: now.max(checkpoint.observed_time),
        };
        match self.store.save(&next, context)? {
            Completion::Cancelled(reason) => return Ok(Completion::Cancelled(reason)),
            Completion::Completed(_) => *checkpoint = next,
        }
        Ok(Completion::Completed(VerifiedUpdate {
            metadata,
            payload_sha256: digest,
            verified_at: now,
            key_id: envelope.key_id,
            public_key: key.to_bytes(),
        }))
    }

    fn validate(&self, metadata: &UpdateMetadata, now: u64) -> Result<(), SystemError> {
        let installed = &self.policy.installed;
        let artifact = &metadata.artifact;
        if metadata.schema_version != 1
            || metadata.channel != UpdateChannel::AndroidDirectApk
            || metadata.application_id != installed.application_id
            || metadata.version_code <= installed.version_code
            || metadata.version_code > 2_100_000_000
            || metadata.sequence == 0
            || metadata.version.trim().is_empty()
            || metadata.version.len() > 64
            || metadata.version.chars().any(char::is_control)
        {
            return Err(error(
                ErrorCode::IntegrityFailure,
                "Update identity, channel or version is incompatible",
            ));
        }
        if metadata.expires_at <= now
            || metadata.expires_at <= metadata.issued_at
            || metadata.expires_at - metadata.issued_at > MAX_LIFETIME_SECONDS
            || metadata.issued_at > now.saturating_add(CLOCK_SKEW_SECONDS)
        {
            return Err(error(
                ErrorCode::IntegrityFailure,
                "Update metadata is expired or outside the trusted time window",
            ));
        }
        hex::<32>(&artifact.sha256)?;
        hex::<32>(&artifact.signer_sha256)?;
        if artifact.signer_sha256 != installed.signer_sha256
            || artifact.size == 0
            || artifact.size > self.policy.maximum_artifact_bytes
            || artifact.minimum_api < 35
            || artifact.minimum_api > installed.android_api
            || artifact
                .maximum_api
                .is_some_and(|api| api < installed.android_api || api < artifact.minimum_api)
            || artifact.abis.is_empty()
            || artifact.abis.len() > 2
            || artifact
                .abis
                .iter()
                .any(|abi| abi != "arm64-v8a" && abi != "x86_64")
            || !artifact.abis.contains(&installed.abi)
            || (artifact.abis.len() == 2 && artifact.abis[0] == artifact.abis[1])
        {
            return Err(error(
                ErrorCode::IntegrityFailure,
                "Update signer, size or platform compatibility is invalid",
            ));
        }
        let url = artifact_url(&artifact.url)?;
        if !self.origins.contains(&url.origin()) {
            return Err(error(
                ErrorCode::IntegrityFailure,
                "Update artifact origin is not allowed",
            ));
        }
        Ok(())
    }
}

fn validate_policy(policy: &UpdateTrustPolicy) -> Result<Vec<url::Origin>, SystemError> {
    let installed = &policy.installed;
    if installed.application_id.is_empty()
        || installed.application_id.len() > 255
        || installed.version_code == 0
        || installed.version_code > 2_100_000_000
        || !installed
            .application_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'.' || byte == b'_')
        || installed.android_api < 35
        || !["arm64-v8a", "x86_64"].contains(&installed.abi.as_str())
        || policy.keys.is_empty()
        || policy.keys.len() > 8
        || policy.artifact_origins.is_empty()
        || policy.artifact_origins.len() > 8
        || policy.maximum_artifact_bytes == 0
        || policy.maximum_artifact_bytes > 2 * 1024 * 1024 * 1024
    {
        return Err(error(
            ErrorCode::InvalidConfiguration,
            "Update trust policy is incomplete or exceeds bounds",
        ));
    }
    hex::<32>(&installed.signer_sha256)?;
    for (id, key) in &policy.keys {
        if id.is_empty()
            || id.len() > 64
            || !id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"_-".contains(&byte))
            || VerifyingKey::from_bytes(key).map_or(true, |key| key.is_weak())
        {
            return Err(error(
                ErrorCode::InvalidConfiguration,
                "Pinned update key or identifier is invalid",
            ));
        }
    }
    policy
        .artifact_origins
        .iter()
        .map(|origin| {
            let url = artifact_url(origin)?;
            if url.path() != "/" || url.query().is_some() {
                return Err(error(
                    ErrorCode::InvalidConfiguration,
                    "Artifact policy requires exact HTTPS origins",
                ));
            }
            Ok(url.origin())
        })
        .collect()
}

fn artifact_url(text: &str) -> Result<Url, SystemError> {
    if text.len() > 2048
        || text
            .chars()
            .any(|character| character.is_control() || character.is_whitespace())
    {
        return Err(error(
            ErrorCode::IntegrityFailure,
            "Artifact URL exceeds its bound or contains controls",
        ));
    }
    let url = Url::parse(text)
        .map_err(|_| error(ErrorCode::IntegrityFailure, "Artifact URL is invalid"))?;
    if url.scheme() != "https"
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.fragment().is_some()
    {
        return Err(error(
            ErrorCode::IntegrityFailure,
            "Artifact URL must be HTTPS without credentials or a fragment",
        ));
    }
    Ok(url)
}

pub(crate) fn hex<const N: usize>(text: &str) -> Result<[u8; N], SystemError> {
    if text.len() != N * 2
        || !text
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(error(
            ErrorCode::IntegrityFailure,
            "Update digest or signature encoding is invalid",
        ));
    }
    let mut bytes = [0; N];
    for (index, byte) in bytes.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&text[index * 2..index * 2 + 2], 16).map_err(|_| {
            error(
                ErrorCode::IntegrityFailure,
                "Invalid update hexadecimal encoding",
            )
        })?;
    }
    Ok(bytes)
}
