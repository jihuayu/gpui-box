//! Background-only token persistence, cold ceremony storage and lease reads.

use crate::{
    AccountId, AuthError, ColdRedirectDescriptor, ExecutionOwner, IssuedAccountTokens,
    MAX_ACTIVE_CEREMONIES, OidcNonce, PkceVerifier, ProviderId, RefreshedAccountTokens,
    SecretTokenMaterial,
};
use gpui_secrets::{CredentialKey, SecretAccess, SecretBytes, SecretPolicy, Secrets};
use gpui_system::{Completion, OperationContext, SystemError};
use std::time::Duration;

const SERVICE: &str = "gpui-auth/v1";
const ACCESS: &str = "access-token";
const REFRESH: &str = "refresh-token";
const ID: &str = "id-token";
const PKCE: &str = "pkce-verifier";
const COLD: &str = "cold-ceremony";
const COLD_INDEX: &str = "cold-index";
const COLD_ACCOUNT: &str = "gpui-auth/cold";
const DESCRIPTOR_VERSION: u8 = 1;

/// Typed aliases that mirror the documented secret vocabulary while binding to
/// the workspace's existing `gpui-secrets` implementation.
pub type SecretService = Secrets;
pub type SecretKey = CredentialKey;

/// Token vault backed by `gpui-secrets`. Every method requires the background
/// execution owner; no UI or renderer caller can read token bytes.
#[derive(Clone)]
pub struct SecretTokenVault {
    secrets: Secrets,
}

impl SecretTokenVault {
    pub const fn new(secrets: Secrets) -> Self {
        Self { secrets }
    }

    pub fn secrets(&self) -> &Secrets {
        &self.secrets
    }

    pub fn store_issued(
        &self,
        owner: ExecutionOwner,
        tokens: &IssuedAccountTokens,
        context: &OperationContext,
    ) -> Result<(), AuthError> {
        owner.require_background("persist account tokens")?;
        self.put(
            &tokens.account_id,
            ACCESS,
            tokens.access_token.expose().to_vec(),
            context,
        )?;
        if let Some(refresh) = &tokens.refresh_token {
            self.put(
                &tokens.account_id,
                REFRESH,
                refresh.expose().to_vec(),
                context,
            )?;
        } else {
            self.delete_purpose(&tokens.account_id, REFRESH, context)?;
        }
        if let Some(id_token) = &tokens.id_token {
            self.put(&tokens.account_id, ID, id_token.expose().to_vec(), context)?;
        } else {
            self.delete_purpose(&tokens.account_id, ID, context)?;
        }
        Ok(())
    }

    pub fn store_refreshed(
        &self,
        owner: ExecutionOwner,
        account_id: &AccountId,
        tokens: &RefreshedAccountTokens,
        context: &OperationContext,
    ) -> Result<(), AuthError> {
        owner.require_background("persist refreshed tokens")?;
        self.put(
            account_id,
            ACCESS,
            tokens.access_token.expose().to_vec(),
            context,
        )?;
        if let Some(refresh) = &tokens.refresh_token {
            self.put(account_id, REFRESH, refresh.expose().to_vec(), context)?;
        }
        // RFC 6749 section 6: an omitted refresh token means the existing token
        // stays valid. Preserve it; service.rs keeps `has_refresh_token = true`.
        if let Some(id_token) = &tokens.id_token {
            self.put(account_id, ID, id_token.expose().to_vec(), context)?;
        } else {
            self.delete_purpose(account_id, ID, context)?;
        }
        Ok(())
    }

    fn put(
        &self,
        account: &AccountId,
        purpose: &'static str,
        value: Vec<u8>,
        context: &OperationContext,
    ) -> Result<(), AuthError> {
        ensure_background_context(context)?;
        let key = account_key(account, purpose)?;
        let bytes = SecretBytes::new(value)?;
        match self
            .secrets
            .put(&key, bytes, SecretPolicy::default(), context)?
        {
            Completion::Completed(SecretAccess::Available(())) => Ok(()),
            Completion::Completed(SecretAccess::Locked) => Err(AuthError::Unavailable(
                "The secret vault is locked; unlock the device and retry",
            )),
            Completion::Completed(SecretAccess::KeyInvalidated) => Err(AuthError::Unavailable(
                "The secret vault key was invalidated; authenticate again",
            )),
            Completion::Cancelled(reason) => Err(AuthError::Cancelled(reason)),
        }
    }

    fn delete_purpose(
        &self,
        account: &AccountId,
        purpose: &'static str,
        context: &OperationContext,
    ) -> Result<(), AuthError> {
        let _ = self.delete_purpose_count(account, purpose, context)?;
        Ok(())
    }

    fn delete_purpose_count(
        &self,
        account: &AccountId,
        purpose: &'static str,
        context: &OperationContext,
    ) -> Result<bool, AuthError> {
        ensure_background_context(context)?;
        let key = account_key(account, purpose)?;
        match self.secrets.delete(&key, context)? {
            Completion::Completed(SecretAccess::Available(receipt)) => Ok(receipt.existed),
            Completion::Completed(SecretAccess::Locked) => Err(AuthError::Unavailable(
                "The secret vault is locked; unlock the device and retry",
            )),
            Completion::Completed(SecretAccess::KeyInvalidated) => Err(AuthError::Unavailable(
                "The secret vault key was invalidated; authenticate again",
            )),
            Completion::Cancelled(reason) => Err(AuthError::Cancelled(reason)),
        }
    }

    pub fn read_access_token(
        &self,
        owner: ExecutionOwner,
        account: &AccountId,
        context: &OperationContext,
    ) -> Result<Option<SecretTokenMaterial>, AuthError> {
        owner.require_background("read an access token")?;
        let key = account_key(account, ACCESS)?;
        self.read(&key, context)
    }

    pub fn read_refresh_token(
        &self,
        owner: ExecutionOwner,
        account: &AccountId,
        context: &OperationContext,
    ) -> Result<Option<SecretTokenMaterial>, AuthError> {
        owner.require_background("read a refresh token")?;
        let key = account_key(account, REFRESH)?;
        self.read(&key, context)
    }

    fn read(
        &self,
        key: &CredentialKey,
        context: &OperationContext,
    ) -> Result<Option<SecretTokenMaterial>, AuthError> {
        ensure_background_context(context)?;
        match self.secrets.get(key, context)? {
            Completion::Completed(SecretAccess::Available(Some(bytes))) => {
                Ok(Some(SecretTokenMaterial::new(bytes.expose().to_vec())?))
            }
            Completion::Completed(SecretAccess::Available(None)) => Ok(None),
            Completion::Completed(SecretAccess::Locked) => Err(AuthError::Unavailable(
                "The secret vault is locked; unlock the device and retry",
            )),
            Completion::Completed(SecretAccess::KeyInvalidated) => Err(AuthError::Unavailable(
                "The secret vault key was invalidated; authenticate again",
            )),
            Completion::Cancelled(reason) => Err(AuthError::Cancelled(reason)),
        }
    }

    /// Remove this crate's three token entries for an account. Never calls
    /// `remove_account`, because that would delete the same account name in
    /// unrelated service namespaces (and could target `gpui-auth/cold`).
    pub fn delete_account(
        &self,
        owner: ExecutionOwner,
        account: &AccountId,
        context: &OperationContext,
    ) -> Result<usize, AuthError> {
        owner.require_background("delete account tokens")?;
        ensure_background_context(context)?;
        let mut deleted = 0usize;
        for purpose in [ACCESS, REFRESH, ID] {
            if self.delete_purpose_count(account, purpose, context)? {
                deleted = deleted.saturating_add(1);
            }
        }
        Ok(deleted)
    }

    /// Best-effort removal of only the token entries, used when a refresh races
    /// a logout and newly written tokens must be discarded immediately.
    pub fn discard_tokens(
        &self,
        owner: ExecutionOwner,
        account: &AccountId,
        context: &OperationContext,
    ) -> Result<usize, AuthError> {
        owner.require_background("discard late tokens")?;
        self.delete_account(owner, account, context)
    }
}

/// Outcome of consuming a cold ceremony from the vault.
pub struct LoadedColdCeremony {
    pub descriptor: ColdRedirectDescriptor,
    pub verifier: PkceVerifier,
}

/// Persistent cold-redirect store. The verifier is secret; the descriptor is
/// non-secret state but is kept under the same account namespace so removal is
/// atomic with respect to one ceremony.
#[derive(Clone)]
pub struct ColdCeremonyStore {
    vault: SecretTokenVault,
}

impl ColdCeremonyStore {
    pub const fn new(vault: SecretTokenVault) -> Self {
        Self { vault }
    }

    /// Persist a verifier and descriptor, then publish the ceremony id in the
    /// bounded cold index. The caller must already have chosen an explicit TTL.
    pub fn persist(
        &self,
        owner: ExecutionOwner,
        descriptor: &ColdRedirectDescriptor,
        verifier: &PkceVerifier,
        context: &OperationContext,
    ) -> Result<(), AuthError> {
        owner.require_background("persist a cold redirect")?;
        let id = descriptor.ceremony_id;
        self.put(
            COLD_ACCOUNT,
            &cold_key(id, PKCE),
            verifier.expose().as_bytes().to_vec(),
            context,
        )?;
        let encoded = encode_descriptor(descriptor);
        if let Err(error) = self.put(COLD_ACCOUNT, &cold_key(id, COLD), encoded, context) {
            let _ = self.delete_key(COLD_ACCOUNT, &cold_key(id, PKCE), context);
            return Err(error);
        }
        if let Err(error) = self.add_to_index(owner, id, context) {
            let _ = self.delete_key(COLD_ACCOUNT, &cold_key(id, COLD), context);
            let _ = self.delete_key(COLD_ACCOUNT, &cold_key(id, PKCE), context);
            return Err(error);
        }
        Ok(())
    }

    /// Load and validate one cold ceremony. Malformed, orphaned or
    /// version-mismatched records are deleted rather than trusted.
    pub fn load(
        &self,
        owner: ExecutionOwner,
        ceremony_id: crate::CeremonyId,
        context: &OperationContext,
    ) -> Result<Option<LoadedColdCeremony>, AuthError> {
        owner.require_background("load a cold redirect")?;
        let verifier = match self.get(COLD_ACCOUNT, &cold_key(ceremony_id, PKCE), context)? {
            Some(bytes) => match std::str::from_utf8(bytes.expose()) {
                Ok(value) => PkceVerifier::new(value).map_err(|_| {
                    AuthError::IntegrityFailure("Stored PKCE verifier is malformed")
                })?,
                Err(_) => {
                    let _ = self.delete(owner, ceremony_id, context);
                    return Ok(None);
                }
            },
            None => return Ok(None),
        };
        let descriptor = match self.get(COLD_ACCOUNT, &cold_key(ceremony_id, COLD), context)? {
            Some(bytes) => match decode_descriptor(bytes.expose(), ceremony_id) {
                Some(descriptor) if descriptor.ceremony_id == ceremony_id => descriptor,
                _ => {
                    let _ = self.delete(owner, ceremony_id, context);
                    return Ok(None);
                }
            },
            None => {
                let _ = self.delete_key(COLD_ACCOUNT, &cold_key(ceremony_id, PKCE), context);
                return Ok(None);
            }
        };
        Ok(Some(LoadedColdCeremony {
            descriptor,
            verifier,
        }))
    }

    /// Ceremony ids currently published by the bounded index.
    pub fn list(
        &self,
        owner: ExecutionOwner,
        context: &OperationContext,
    ) -> Vec<crate::CeremonyId> {
        if owner.require_background("list cold redirects").is_err() {
            return Vec::new();
        }
        let key = match cold_index_key() {
            Ok(key) => key,
            Err(_) => return Vec::new(),
        };
        let mut ids = Vec::new();
        if let Ok(Completion::Completed(SecretAccess::Available(Some(bytes)))) =
            self.vault.secrets.get(&key, context)
        {
            let mut index = 0;
            let data = bytes.expose();
            while index + 8 <= data.len() && ids.len() < MAX_ACTIVE_CEREMONIES {
                let mut field = [0u8; 8];
                field.copy_from_slice(&data[index..index + 8]);
                index += 8;
                if let Ok(id) = crate::CeremonyId::new(u64::from_be_bytes(field))
                    && !ids.contains(&id)
                {
                    ids.push(id);
                }
            }
        }
        ids
    }

    pub fn delete(
        &self,
        owner: ExecutionOwner,
        ceremony_id: crate::CeremonyId,
        context: &OperationContext,
    ) -> Result<(), AuthError> {
        owner.require_background("delete a cold redirect")?;
        self.delete_key(COLD_ACCOUNT, &cold_key(ceremony_id, COLD), context)?;
        self.delete_key(COLD_ACCOUNT, &cold_key(ceremony_id, PKCE), context)?;
        self.remove_from_index(owner, ceremony_id, context)
    }

    fn put(
        &self,
        account: &str,
        purpose: &str,
        value: Vec<u8>,
        context: &OperationContext,
    ) -> Result<(), AuthError> {
        ensure_background_context(context)?;
        let key = CredentialKey::new(account, SERVICE, purpose)?;
        let bytes = SecretBytes::new(value)?;
        match self.secrets_put(&key, bytes, context)? {
            Completion::Completed(SecretAccess::Available(())) => Ok(()),
            Completion::Completed(SecretAccess::Locked) => Err(AuthError::Unavailable(
                "The secret vault is locked; unlock the device and retry",
            )),
            Completion::Completed(SecretAccess::KeyInvalidated) => Err(AuthError::Unavailable(
                "The secret vault key was invalidated; authenticate again",
            )),
            Completion::Cancelled(reason) => Err(AuthError::Cancelled(reason)),
        }
    }

    fn secrets_put(
        &self,
        key: &CredentialKey,
        bytes: SecretBytes,
        context: &OperationContext,
    ) -> Result<Completion<SecretAccess<()>>, AuthError> {
        Ok(self
            .vault
            .secrets
            .put(key, bytes, SecretPolicy::default(), context)?)
    }

    fn get(
        &self,
        account: &str,
        purpose: &str,
        context: &OperationContext,
    ) -> Result<Option<SecretBytes>, AuthError> {
        ensure_background_context(context)?;
        let key = CredentialKey::new(account, SERVICE, purpose)?;
        match self.vault.secrets.get(&key, context)? {
            Completion::Completed(SecretAccess::Available(value)) => Ok(value),
            Completion::Completed(SecretAccess::Locked) => Err(AuthError::Unavailable(
                "The secret vault is locked; cold redirect state is unavailable",
            )),
            Completion::Completed(SecretAccess::KeyInvalidated) => Err(AuthError::Unavailable(
                "The secret vault key was invalidated; cold redirect state is unavailable",
            )),
            Completion::Cancelled(reason) => Err(AuthError::Cancelled(reason)),
        }
    }

    fn delete_key(
        &self,
        account: &str,
        purpose: &str,
        context: &OperationContext,
    ) -> Result<(), AuthError> {
        ensure_background_context(context)?;
        let key = CredentialKey::new(account, SERVICE, purpose)?;
        match self.vault.secrets.delete(&key, context)? {
            Completion::Completed(SecretAccess::Available(_)) => Ok(()),
            Completion::Completed(SecretAccess::Locked) => Err(AuthError::Unavailable(
                "The secret vault is locked; unlock the device and retry",
            )),
            Completion::Completed(SecretAccess::KeyInvalidated) => Err(AuthError::Unavailable(
                "The secret vault key was invalidated; authenticate again",
            )),
            Completion::Cancelled(reason) => Err(AuthError::Cancelled(reason)),
        }
    }

    fn add_to_index(
        &self,
        owner: ExecutionOwner,
        ceremony_id: crate::CeremonyId,
        context: &OperationContext,
    ) -> Result<(), AuthError> {
        owner.require_background("index a cold redirect")?;
        let mut ids = self.list(owner, context);
        if ids.contains(&ceremony_id) {
            return Ok(());
        }
        if ids.len() >= MAX_ACTIVE_CEREMONIES {
            let oldest = ids.remove(0);
            let _ = self.delete_key(COLD_ACCOUNT, &cold_key(oldest, COLD), context);
            let _ = self.delete_key(COLD_ACCOUNT, &cold_key(oldest, PKCE), context);
        }
        ids.push(ceremony_id);
        let mut encoded = Vec::with_capacity(ids.len() * 8);
        for id in ids {
            encoded.extend_from_slice(&id.get().to_be_bytes());
        }
        self.put(COLD_ACCOUNT, COLD_INDEX, encoded, context)
    }

    fn remove_from_index(
        &self,
        owner: ExecutionOwner,
        ceremony_id: crate::CeremonyId,
        context: &OperationContext,
    ) -> Result<(), AuthError> {
        owner.require_background("unindex a cold redirect")?;
        let ids: Vec<_> = self
            .list(owner, context)
            .into_iter()
            .filter(|id| *id != ceremony_id)
            .collect();
        let mut encoded = Vec::with_capacity(ids.len() * 8);
        for id in ids {
            encoded.extend_from_slice(&id.get().to_be_bytes());
        }
        self.put(COLD_ACCOUNT, COLD_INDEX, encoded, context)
    }
}

fn account_key(account: &AccountId, purpose: &'static str) -> Result<CredentialKey, AuthError> {
    CredentialKey::new(account.as_str(), SERVICE, purpose).map_err(AuthError::System)
}

fn cold_key(ceremony_id: crate::CeremonyId, purpose: &str) -> String {
    format!("{}-{}", ceremony_id.get(), purpose)
}

fn cold_index_key() -> Result<CredentialKey, SystemError> {
    CredentialKey::new(COLD_ACCOUNT, SERVICE, COLD_INDEX)
}

fn ensure_background_context(context: &OperationContext) -> Result<(), AuthError> {
    if context.ui.is_some() {
        return Err(AuthError::WrongExecutionOwner(
            "secret access requires a background context without a UI attachment",
        ));
    }
    Ok(())
}

fn encode_descriptor(descriptor: &ColdRedirectDescriptor) -> Vec<u8> {
    let mut encoder = Encoder::new();
    encoder.u8(DESCRIPTOR_VERSION);
    encoder.u64(descriptor.ceremony_id.get());
    encoder.redirect(&descriptor.redirect);
    encoder.string(descriptor.state.expose());
    encoder.optional_string(descriptor.nonce.as_ref().map(OidcNonce::expose));
    encoder.u16(descriptor.scopes.len() as u16);
    for scope in &descriptor.scopes {
        encoder.string(scope.as_str());
    }
    encoder.u64(descriptor.presentation.surface().0);
    encoder.u64(descriptor.presentation.generation());
    encoder.u64(
        u64::try_from(descriptor.remaining_ttl.as_millis())
            .unwrap_or(u64::MAX)
            .min(86_400_000),
    );
    encoder.optional_string(descriptor.expected_account.as_ref().map(AccountId::as_str));
    encoder.string(descriptor.provider_id.as_str());
    encoder.finish()
}

fn decode_descriptor(
    data: &[u8],
    ceremony_id: crate::CeremonyId,
) -> Option<ColdRedirectDescriptor> {
    let mut decoder = Decoder::new(data);
    if decoder.u8()? != DESCRIPTOR_VERSION {
        return None;
    }
    let encoded_id = crate::CeremonyId::new(decoder.u64()?).ok()?;
    if encoded_id != ceremony_id {
        return None;
    }
    let redirect = decoder.redirect()?;
    let state = crate::OAuthState::new(decoder.string()?).ok()?;
    let nonce = decoder
        .optional_string()?
        .map(crate::OidcNonce::new)
        .transpose()
        .ok()?;
    let scope_count = decoder.u16()? as usize;
    if scope_count == 0 || scope_count > crate::MAX_SCOPES {
        return None;
    }
    let mut scopes = Vec::with_capacity(scope_count);
    for _ in 0..scope_count {
        scopes.push(crate::AuthScope::new(decoder.string()?).ok()?);
    }
    let surface = gpui_system::SurfaceId(decoder.u64()?);
    let generation = decoder.u64()?;
    let presentation = crate::PresentationContext::new(surface, generation).ok()?;
    let ttl_millis = decoder.u64()?;
    if ttl_millis == 0 {
        return None;
    }
    let expected_account = decoder
        .optional_string()?
        .map(AccountId::new)
        .transpose()
        .ok()?;
    let provider_id = ProviderId::new(decoder.string()?).ok()?;
    if !decoder.finished() {
        return None;
    }
    Some(ColdRedirectDescriptor {
        ceremony_id: encoded_id,
        redirect,
        state,
        nonce,
        scopes,
        presentation,
        remaining_ttl: Duration::from_millis(ttl_millis),
        expected_account,
        provider_id,
    })
}

struct Encoder {
    bytes: Vec<u8>,
}

impl Encoder {
    fn new() -> Self {
        Self { bytes: Vec::new() }
    }

    fn u8(&mut self, value: u8) {
        self.bytes.push(value);
    }

    fn u16(&mut self, value: u16) {
        self.bytes.extend_from_slice(&value.to_be_bytes());
    }

    fn u64(&mut self, value: u64) {
        self.bytes.extend_from_slice(&value.to_be_bytes());
    }

    fn string(&mut self, value: &str) {
        self.u16(u16::try_from(value.len()).unwrap_or(u16::MAX));
        self.bytes.extend_from_slice(value.as_bytes());
    }

    fn optional_string(&mut self, value: Option<&str>) {
        match value {
            Some(value) => {
                self.u8(1);
                self.string(value);
            }
            None => self.u8(0),
        }
    }

    fn redirect(&mut self, redirect: &crate::RedirectTarget) {
        match redirect {
            crate::RedirectTarget::Loopback { host, port, path } => {
                self.u8(1);
                self.u8(match host {
                    crate::LoopbackHost::Ipv4 => 1,
                    crate::LoopbackHost::Ipv6 => 2,
                });
                self.u8(u8::from(port.is_some()));
                self.u16(port.unwrap_or(0));
                self.string(path);
            }
            crate::RedirectTarget::CustomScheme { scheme, host, path } => {
                self.u8(2);
                self.string(scheme);
                self.optional_string(host.as_deref());
                self.string(path);
            }
            crate::RedirectTarget::UniversalLink { host, port, path } => {
                self.u8(3);
                self.string(host);
                self.u8(u8::from(port.is_some()));
                self.u16(port.unwrap_or(0));
                self.string(path);
            }
            crate::RedirectTarget::WebSameOrigin { origin, path } => {
                self.u8(4);
                self.string(&origin.as_string());
                self.string(path);
            }
        }
    }

    fn finish(self) -> Vec<u8> {
        self.bytes
    }
}

struct Decoder<'a> {
    bytes: &'a [u8],
    index: usize,
}

impl<'a> Decoder<'a> {
    const fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, index: 0 }
    }

    fn u8(&mut self) -> Option<u8> {
        let value = *self.bytes.get(self.index)?;
        self.index += 1;
        Some(value)
    }

    fn u16(&mut self) -> Option<u16> {
        let end = self.index.checked_add(2)?;
        let slice = self.bytes.get(self.index..end)?;
        self.index = end;
        Some(u16::from_be_bytes([slice[0], slice[1]]))
    }

    fn u64(&mut self) -> Option<u64> {
        let end = self.index.checked_add(8)?;
        let slice = self.bytes.get(self.index..end)?;
        self.index = end;
        let mut field = [0u8; 8];
        field.copy_from_slice(slice);
        Some(u64::from_be_bytes(field))
    }

    fn string(&mut self) -> Option<&'a str> {
        let length = self.u16()? as usize;
        let end = self.index.checked_add(length)?;
        let slice = self.bytes.get(self.index..end)?;
        self.index = end;
        std::str::from_utf8(slice).ok()
    }

    fn optional_string(&mut self) -> Option<Option<&'a str>> {
        match self.u8()? {
            0 => Some(None),
            1 => Some(Some(self.string()?)),
            _ => None,
        }
    }

    fn redirect(&mut self) -> Option<crate::RedirectTarget> {
        match self.u8()? {
            1 => {
                let host = match self.u8()? {
                    1 => crate::LoopbackHost::Ipv4,
                    2 => crate::LoopbackHost::Ipv6,
                    _ => return None,
                };
                let has_port = self.u8()?;
                let port = self.u16()?;
                let path = self.string()?;
                let port = match has_port {
                    0 => None,
                    1 => Some(port),
                    _ => return None,
                };
                crate::RedirectTarget::loopback(host, port, path).ok()
            }
            2 => {
                let scheme = self.string()?;
                let host = self.optional_string()?;
                let path = self.string()?;
                crate::RedirectTarget::custom_scheme(scheme, host, path).ok()
            }
            3 => {
                let host = self.string()?;
                let has_port = self.u8()?;
                let port = self.u16()?;
                let path = self.string()?;
                let port = match has_port {
                    0 => None,
                    1 => Some(port),
                    _ => return None,
                };
                crate::RedirectTarget::universal_link(host, port, path).ok()
            }
            4 => {
                let origin = crate::AuthOrigin::parse(self.string()?).ok()?;
                let path = self.string()?;
                crate::RedirectTarget::web_same_origin(origin, path).ok()
            }
            _ => None,
        }
    }

    fn finished(&self) -> bool {
        self.index == self.bytes.len()
    }
}
