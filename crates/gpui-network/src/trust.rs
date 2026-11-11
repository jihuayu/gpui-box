//! Trust-store policy, pins and deterministic certificate-chain evaluation.

use crate::{PlatformTarget, TlsFailureReason};
use gpui_system::{ErrorCode, SystemError};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::{collections::BTreeSet, fmt, str::FromStr};

fn invalid(message: &'static str) -> SystemError {
    SystemError::new(ErrorCode::InvalidInput, message)
}

fn unsupported(message: &'static str) -> SystemError {
    SystemError::new(ErrorCode::Unsupported, message)
}

/// A SHA-256 SPKI pin. The value is public-safe and never secret.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SpkiPinSha256(pub [u8; 32]);

impl SpkiPinSha256 {
    pub const fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    pub fn from_hex(value: &str) -> Result<Self, SystemError> {
        if value.len() != 64 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err(invalid("An SPKI pin must be exactly 64 hexadecimal digits"));
        }
        let mut bytes = [0u8; 32];
        for (index, byte) in bytes.iter_mut().enumerate() {
            *byte = u8::from_str_radix(&value[index * 2..index * 2 + 2], 16)
                .map_err(|_| invalid("An SPKI pin contains an invalid hexadecimal pair"))?;
        }
        Ok(Self(bytes))
    }

    pub fn to_hex(self) -> String {
        use std::fmt::Write;
        let mut output = String::with_capacity(64);
        for byte in self.0 {
            let _ = write!(output, "{byte:02x}");
        }
        output
    }
}

impl fmt::Debug for SpkiPinSha256 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "SpkiPinSha256({})", self.to_hex())
    }
}

impl fmt::Display for SpkiPinSha256 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.to_hex())
    }
}

impl FromStr for SpkiPinSha256 {
    type Err = SystemError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::from_hex(value)
    }
}

impl Serialize for SpkiPinSha256 {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.to_hex())
    }
}

impl<'de> Deserialize<'de> for SpkiPinSha256 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::from_hex(&value).map_err(serde::de::Error::custom)
    }
}

/// Release discipline for trust policy. A debug override must be selected
/// explicitly; it is never inferred from a debug build.
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum TrustProfile {
    #[default]
    Release,
    DebugOverride,
}

/// The anchor store used to validate a server chain.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TrustStorePolicy {
    /// Operating-system trust anchors only.
    #[default]
    OsSystemRoots,
    /// System anchors plus enterprise-delivered anchors.
    OsPlusEnterpriseRoots { allow_user_installed_roots: bool },
    /// Strict SHA-256 SPKI pin set with explicit system-root inclusion.
    StrictSpkiPinSet {
        pins: BTreeSet<SpkiPinSha256>,
        include_system_roots: bool,
        allow_enterprise_bypass: bool,
    },
    /// The browser owns TLS verification. Mandatory on Web.
    BrowserManagedTls,
}

impl TrustStorePolicy {
    pub fn strict_pins(
        pins: impl IntoIterator<Item = SpkiPinSha256>,
        include_system_roots: bool,
        allow_enterprise_bypass: bool,
    ) -> Result<Self, SystemError> {
        let pins: BTreeSet<_> = pins.into_iter().collect();
        if pins.is_empty() {
            return Err(invalid("A strict SPKI pin set cannot be empty"));
        }
        Ok(Self::StrictSpkiPinSet {
            pins,
            include_system_roots,
            allow_enterprise_bypass,
        })
    }

    pub const fn is_browser_managed(&self) -> bool {
        matches!(self, Self::BrowserManagedTls)
    }

    pub fn validate(&self) -> Result<(), SystemError> {
        if let Self::StrictSpkiPinSet { pins, .. } = self
            && pins.is_empty()
        {
            return Err(invalid("A strict SPKI pin set cannot be empty"));
        }
        Ok(())
    }
}

/// Validate the platform/profile combination before installing a service.
///
/// Web accepts only [`TrustStorePolicy::BrowserManagedTls`]: silently degrading
/// a pinned native policy to unpinned browser TLS is forbidden. Native targets
/// reject the browser-managed representation. Android and iOS model their
/// release user-root restrictions honestly; a release profile rejects
/// `allow_user_installed_roots: true` unless `DebugOverride` is explicit.
pub fn validate_trust_policy(
    platform: PlatformTarget,
    profile: TrustProfile,
    policy: &TrustStorePolicy,
) -> Result<(), SystemError> {
    policy.validate()?;
    match platform {
        PlatformTarget::Web => {
            if !policy.is_browser_managed() {
                return Err(unsupported(
                    "Web trust must be browser-managed; custom pins and native roots are unavailable",
                ));
            }
        }
        PlatformTarget::Windows
        | PlatformTarget::MacOs
        | PlatformTarget::Linux
        | PlatformTarget::Android
        | PlatformTarget::Ios => {
            if policy.is_browser_managed() {
                return Err(unsupported(
                    "BrowserManagedTls is only valid for the Web platform",
                ));
            }
            if let TrustStorePolicy::StrictSpkiPinSet { pins, .. } = policy
                && pins.is_empty()
            {
                return Err(invalid("A strict SPKI pin set cannot be empty"));
            }
            if matches!(platform, PlatformTarget::Android | PlatformTarget::Ios)
                && profile == TrustProfile::Release
                && matches!(
                    policy,
                    TrustStorePolicy::OsPlusEnterpriseRoots {
                        allow_user_installed_roots: true
                    }
                )
            {
                return Err(invalid(
                    "Release Android/iOS network security policy cannot accept user-installed roots",
                ));
            }
        }
    }
    Ok(())
}

/// The observed root class in a certificate chain.
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum ObservedRootKind {
    System,
    Enterprise,
    UserInstalled,
    #[default]
    Unknown,
}

/// One certificate observation. The SPKI pin is optional because only the leaf
/// or an intermediate may carry a configured pin.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CertificateObservation {
    pub root_kind: ObservedRootKind,
    pub spki_sha256: Option<SpkiPinSha256>,
}

impl CertificateObservation {
    pub const fn new(root_kind: ObservedRootKind) -> Self {
        Self {
            root_kind,
            spki_sha256: None,
        }
    }

    pub const fn pinned(root_kind: ObservedRootKind, pin: SpkiPinSha256) -> Self {
        Self {
            root_kind,
            spki_sha256: Some(pin),
        }
    }

    pub const fn with_spki_sha256(mut self, pin: SpkiPinSha256) -> Self {
        self.spki_sha256 = Some(pin);
        self
    }
}

/// A host-side certificate-chain observation. Validity, revocation and name
/// checks are reported separately from anchor selection so failures keep their
/// exact reason.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CertificateChainObservation {
    pub certificates: Vec<CertificateObservation>,
    pub hostname_matches: bool,
    pub revoked: bool,
    pub expired_or_not_yet_valid: bool,
    pub browser_verified: bool,
}

impl CertificateChainObservation {
    pub fn new(certificates: Vec<CertificateObservation>) -> Self {
        Self {
            certificates,
            hostname_matches: true,
            revoked: false,
            expired_or_not_yet_valid: false,
            browser_verified: true,
        }
    }

    pub fn empty() -> Self {
        Self::new(Vec::new())
    }

    pub fn from_root(root_kind: ObservedRootKind) -> Self {
        Self::new(vec![CertificateObservation::new(root_kind)])
    }

    pub fn with_hostname_matches(mut self, hostname_matches: bool) -> Self {
        self.hostname_matches = hostname_matches;
        self
    }

    pub fn with_revoked(mut self, revoked: bool) -> Self {
        self.revoked = revoked;
        self
    }

    pub fn with_expired_or_not_yet_valid(mut self, expired: bool) -> Self {
        self.expired_or_not_yet_valid = expired;
        self
    }

    pub fn with_browser_verified(mut self, browser_verified: bool) -> Self {
        self.browser_verified = browser_verified;
        self
    }

    pub fn with_certificate(mut self, certificate: CertificateObservation) -> Self {
        self.certificates.push(certificate);
        self
    }

    pub fn root_kind(&self) -> ObservedRootKind {
        self.certificates
            .last()
            .map_or(ObservedRootKind::Unknown, |certificate| {
                certificate.root_kind
            })
    }

    pub fn pin_matches(&self, pins: &BTreeSet<SpkiPinSha256>) -> bool {
        self.certificates.iter().any(|certificate| {
            certificate
                .spki_sha256
                .is_some_and(|pin| pins.contains(&pin))
        })
    }

    pub fn is_empty(&self) -> bool {
        self.certificates.is_empty()
    }
}

/// The kind of anchor that verified the chain.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VerifiedAnchorKind {
    SystemAnchor,
    EnterpriseAnchor,
    PinnedDirectAnchor,
    BrowserManaged,
}

/// Deterministic trust evaluation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TrustEvaluation {
    Verified(VerifiedAnchorKind),
    Failed(TlsFailureReason),
}

impl TrustEvaluation {
    pub const fn is_verified(self) -> bool {
        matches!(self, Self::Verified(_))
    }

    pub const fn anchor(self) -> Option<VerifiedAnchorKind> {
        match self {
            Self::Verified(anchor) => Some(anchor),
            Self::Failed(_) => None,
        }
    }

    pub const fn failure(self) -> Option<TlsFailureReason> {
        match self {
            Self::Failed(reason) => Some(reason),
            Self::Verified(_) => None,
        }
    }
}

/// Evaluate a chain observation against a trust-store policy.
///
/// Precedence is fixed: empty chain, revocation, validity, hostname, then anchor
/// selection and pin matching. A revoked or expired chain is never rescued by a
/// matching pin or an enterprise bypass.
pub fn evaluate_tls_trust(
    chain_observation: &CertificateChainObservation,
    policy: &TrustStorePolicy,
) -> TrustEvaluation {
    if chain_observation.certificates.is_empty() {
        return TrustEvaluation::Failed(TlsFailureReason::UntrustedRoot);
    }
    if chain_observation.revoked {
        return TrustEvaluation::Failed(TlsFailureReason::Revoked);
    }
    if chain_observation.expired_or_not_yet_valid {
        return TrustEvaluation::Failed(TlsFailureReason::ExpiredOrNotYetValid);
    }
    if !chain_observation.hostname_matches {
        return TrustEvaluation::Failed(TlsFailureReason::HostnameMismatch);
    }

    let root = chain_observation.root_kind();
    match policy {
        TrustStorePolicy::BrowserManagedTls => {
            if chain_observation.browser_verified {
                TrustEvaluation::Verified(VerifiedAnchorKind::BrowserManaged)
            } else {
                TrustEvaluation::Failed(TlsFailureReason::UntrustedRoot)
            }
        }
        TrustStorePolicy::OsSystemRoots => match root {
            ObservedRootKind::System => TrustEvaluation::Verified(VerifiedAnchorKind::SystemAnchor),
            ObservedRootKind::Enterprise
            | ObservedRootKind::UserInstalled
            | ObservedRootKind::Unknown => TrustEvaluation::Failed(TlsFailureReason::UntrustedRoot),
        },
        TrustStorePolicy::OsPlusEnterpriseRoots {
            allow_user_installed_roots,
        } => match root {
            ObservedRootKind::System => TrustEvaluation::Verified(VerifiedAnchorKind::SystemAnchor),
            ObservedRootKind::Enterprise => {
                TrustEvaluation::Verified(VerifiedAnchorKind::EnterpriseAnchor)
            }
            ObservedRootKind::UserInstalled => {
                if *allow_user_installed_roots {
                    TrustEvaluation::Verified(VerifiedAnchorKind::EnterpriseAnchor)
                } else {
                    TrustEvaluation::Failed(TlsFailureReason::UntrustedRoot)
                }
            }
            ObservedRootKind::Unknown => TrustEvaluation::Failed(TlsFailureReason::UntrustedRoot),
        },
        TrustStorePolicy::StrictSpkiPinSet {
            pins,
            include_system_roots,
            allow_enterprise_bypass,
        } => {
            if pins.is_empty() {
                return TrustEvaluation::Failed(TlsFailureReason::PinMismatch);
            }
            let pin_matches = chain_observation.pin_matches(pins);
            if root == ObservedRootKind::Enterprise && *allow_enterprise_bypass {
                return TrustEvaluation::Verified(VerifiedAnchorKind::EnterpriseAnchor);
            }
            if *include_system_roots {
                match root {
                    ObservedRootKind::System => {
                        if pin_matches {
                            TrustEvaluation::Verified(VerifiedAnchorKind::SystemAnchor)
                        } else {
                            TrustEvaluation::Failed(TlsFailureReason::PinMismatch)
                        }
                    }
                    ObservedRootKind::Enterprise => {
                        if pin_matches {
                            TrustEvaluation::Verified(VerifiedAnchorKind::EnterpriseAnchor)
                        } else {
                            TrustEvaluation::Failed(TlsFailureReason::PinMismatch)
                        }
                    }
                    ObservedRootKind::UserInstalled | ObservedRootKind::Unknown => {
                        TrustEvaluation::Failed(TlsFailureReason::UntrustedRoot)
                    }
                }
            } else if pin_matches {
                TrustEvaluation::Verified(VerifiedAnchorKind::PinnedDirectAnchor)
            } else {
                TrustEvaluation::Failed(TlsFailureReason::PinMismatch)
            }
        }
    }
}
