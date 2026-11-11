//! Proxy authentication hooks backed by `gpui-secrets`.
//!
//! Header values are never cloned, formatted or serialized. The only access is
//! an explicit [`ProxyAuthHeader::expose_header_value`] borrow.

use crate::{ClientId, PolicyEpoch};
use gpui_secrets::{CredentialKey, SecretAccess, SecretBytes, Secrets};
use gpui_system::{OperationContext, ServiceResult};
use serde::{Deserialize, Serialize};
use std::fmt;

/// Maximum credential-bearing proxy authentication attempts per
/// `(client, proxy, policy epoch)`.
pub const MAX_PROXY_AUTH_ATTEMPTS: u8 = 2;

/// Proxy authentication scheme.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProxyAuthScheme {
    Basic,
    Bearer,
}

impl ProxyAuthScheme {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Basic => "basic",
            Self::Bearer => "bearer",
        }
    }

    pub const fn header_name(self) -> &'static str {
        "Proxy-Authorization"
    }
}

/// A validated proxy authentication challenge.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProxyAuthChallenge {
    client_id: ClientId,
    proxy: crate::EndpointOrigin,
    policy_epoch: PolicyEpoch,
    scheme: ProxyAuthScheme,
    realm: Option<String>,
}

impl ProxyAuthChallenge {
    pub fn new(
        client_id: ClientId,
        proxy: crate::EndpointOrigin,
        policy_epoch: PolicyEpoch,
        scheme: ProxyAuthScheme,
    ) -> Self {
        Self {
            client_id,
            proxy,
            policy_epoch,
            scheme,
            realm: None,
        }
    }

    /// Attach a sanitized realm. Control characters and over-length realms are
    /// rejected; an empty realm becomes `None`.
    pub fn with_realm(mut self, realm: Option<&str>) -> Result<Self, gpui_system::SystemError> {
        self.realm = crate::model::sanitize_realm(realm)?;
        Ok(self)
    }

    pub const fn client_id(&self) -> ClientId {
        self.client_id
    }

    pub fn proxy(&self) -> &crate::EndpointOrigin {
        &self.proxy
    }

    pub const fn policy_epoch(&self) -> PolicyEpoch {
        self.policy_epoch
    }

    pub const fn scheme(&self) -> ProxyAuthScheme {
        self.scheme
    }

    pub fn realm(&self) -> Option<&str> {
        self.realm.as_deref()
    }
}

/// A background-only source of proxy credentials.
pub trait ProxyCredentialSource: Send + Sync + 'static {
    fn credential(
        &self,
        challenge: &ProxyAuthChallenge,
        context: &OperationContext,
    ) -> ServiceResult<SecretAccess<Option<SecretBytes>>>;
}

/// Alias used by callers that describe the same boundary as a resolver.
pub use ProxyCredentialSource as ProxyCredentialResolver;

/// A credential source backed directly by `gpui_secrets::Secrets` and a
/// [`CredentialKey`].
#[derive(Clone)]
pub struct SecretsProxyCredentialSource {
    secrets: Secrets,
    key: CredentialKey,
}

impl SecretsProxyCredentialSource {
    pub const fn new(secrets: Secrets, key: CredentialKey) -> Self {
        Self { secrets, key }
    }

    pub fn secrets(&self) -> &Secrets {
        &self.secrets
    }

    pub fn key(&self) -> &CredentialKey {
        &self.key
    }
}

impl ProxyCredentialSource for SecretsProxyCredentialSource {
    fn credential(
        &self,
        _challenge: &ProxyAuthChallenge,
        context: &OperationContext,
    ) -> ServiceResult<SecretAccess<Option<SecretBytes>>> {
        self.secrets.get(&self.key, context)
    }
}

/// A zeroizing proxy authorization header.
///
/// This type deliberately does not implement `Clone` or `Serialize`. `Debug`
/// prints `"[REDACTED]"` for the secret payload.
pub struct ProxyAuthHeader {
    scheme: ProxyAuthScheme,
    value: SecretBytes,
}

impl ProxyAuthHeader {
    pub fn new(scheme: ProxyAuthScheme, value: SecretBytes) -> Self {
        Self { scheme, value }
    }

    pub fn basic(value: SecretBytes) -> Self {
        Self::new(ProxyAuthScheme::Basic, value)
    }

    pub fn bearer(value: SecretBytes) -> Self {
        Self::new(ProxyAuthScheme::Bearer, value)
    }

    pub const fn scheme(&self) -> ProxyAuthScheme {
        self.scheme
    }

    pub const fn header_name(&self) -> &'static str {
        "Proxy-Authorization"
    }

    /// Explicit, short-lived access to the header payload.
    pub fn expose_header_value(&self) -> &[u8] {
        self.value.expose()
    }
}

impl fmt::Debug for ProxyAuthHeader {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ProxyAuthHeader")
            .field("scheme", &self.scheme)
            .field("value", &"[REDACTED]")
            .finish()
    }
}

/// Outcome of one 407 challenge.
pub enum ProxyAuthOutcome {
    Header(ProxyAuthHeader),
    Exhausted,
    Locked,
    KeyInvalidated,
    MissingCredentials,
}

impl ProxyAuthOutcome {
    pub fn kind(&self) -> ProxyAuthOutcomeKind {
        match self {
            Self::Header(_) => ProxyAuthOutcomeKind::Header,
            Self::Exhausted => ProxyAuthOutcomeKind::Exhausted,
            Self::Locked => ProxyAuthOutcomeKind::Locked,
            Self::KeyInvalidated => ProxyAuthOutcomeKind::KeyInvalidated,
            Self::MissingCredentials => ProxyAuthOutcomeKind::MissingCredentials,
        }
    }

    pub const fn is_authenticated(&self) -> bool {
        matches!(self, Self::Header(_))
    }
}

impl fmt::Debug for ProxyAuthOutcome {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_tuple("ProxyAuthOutcome")
            .field(&self.kind())
            .finish()
    }
}

/// Public-safe classification of a proxy authentication outcome.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProxyAuthOutcomeKind {
    Header,
    Exhausted,
    Locked,
    KeyInvalidated,
    MissingCredentials,
}
