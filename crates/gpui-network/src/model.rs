//! Layered connectivity, captive-portal, cost and endpoint-health states.
//!
//! Link reachability is deliberately separate from captive-portal status, DNS,
//! proxy, TLS and endpoint availability. A routable interface never implies
//! that an application endpoint is usable.

use crate::EndpointOrigin;
use gpui_system::{ErrorCode, SystemError};
use serde::{Deserialize, Serialize};

fn invalid(message: &'static str) -> SystemError {
    SystemError::new(ErrorCode::InvalidInput, message)
}

/// Provenance for one observation.
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum SignalSource {
    /// The platform or an explicit provider reported a fresh value.
    Observed,
    /// The value was derived from a coarser platform signal.
    Inferred,
    /// The platform cannot expose this signal through a public API.
    Unavailable,
    /// No observation has been attempted or none is trustworthy.
    #[default]
    Unknown,
}

impl SignalSource {
    pub const fn is_observed(self) -> bool {
        matches!(self, Self::Observed)
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Observed => "observed",
            Self::Inferred => "inferred",
            Self::Unavailable => "unavailable",
            Self::Unknown => "unknown",
        }
    }
}

/// The primary interface carrying traffic.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TransportKind {
    WiFi,
    Cellular,
    Ethernet,
    VirtualTunnel,
    Loopback,
    Other,
}

impl TransportKind {
    pub const ALL: [Self; 6] = [
        Self::WiFi,
        Self::Cellular,
        Self::Ethernet,
        Self::VirtualTunnel,
        Self::Loopback,
        Self::Other,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::WiFi => "wifi",
            Self::Cellular => "cellular",
            Self::Ethernet => "ethernet",
            Self::VirtualTunnel => "virtual_tunnel",
            Self::Loopback => "loopback",
            Self::Other => "other",
        }
    }
}

/// Interface/link reachability. This says nothing about a specific endpoint.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum InterfaceReachability {
    /// No usable interface is reported.
    Offline,
    /// A link exists but no routable address family is available.
    LocalLinkOnly,
    /// At least one of IPv4 or IPv6 is routable.
    Routable {
        primary_transport: TransportKind,
        is_vpn: bool,
        ipv4: bool,
        ipv6: bool,
    },
}

impl InterfaceReachability {
    /// Build a validated routable observation.
    ///
    /// A routable interface must expose at least one address family. Passing
    /// `ipv4: false, ipv6: false` fails with [`ErrorCode::InvalidInput`].
    pub fn routable(
        primary_transport: TransportKind,
        is_vpn: bool,
        ipv4: bool,
        ipv6: bool,
    ) -> Result<Self, SystemError> {
        if !ipv4 && !ipv6 {
            return Err(invalid(
                "A routable interface requires IPv4 or IPv6 reachability",
            ));
        }
        Ok(Self::Routable {
            primary_transport,
            is_vpn,
            ipv4,
            ipv6,
        })
    }

    pub fn validate(self) -> Result<(), SystemError> {
        if let Self::Routable { ipv4, ipv6, .. } = self
            && !ipv4
            && !ipv6
        {
            return Err(invalid(
                "A routable interface requires IPv4 or IPv6 reachability",
            ));
        }
        Ok(())
    }

    pub const fn is_offline(self) -> bool {
        matches!(self, Self::Offline)
    }

    pub const fn is_local_link_only(self) -> bool {
        matches!(self, Self::LocalLinkOnly)
    }

    pub const fn is_routable(self) -> bool {
        matches!(self, Self::Routable { .. })
    }

    pub const fn transport(self) -> Option<TransportKind> {
        match self {
            Self::Routable {
                primary_transport, ..
            } => Some(primary_transport),
            _ => None,
        }
    }

    pub const fn is_vpn(self) -> bool {
        matches!(self, Self::Routable { is_vpn: true, .. })
    }

    pub const fn ipv4(self) -> bool {
        matches!(self, Self::Routable { ipv4: true, .. })
    }

    pub const fn ipv6(self) -> bool {
        matches!(self, Self::Routable { ipv6: true, .. })
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum InterfaceReachabilityRepr {
    Offline,
    LocalLinkOnly,
    Routable {
        primary_transport: TransportKind,
        is_vpn: bool,
        ipv4: bool,
        ipv6: bool,
    },
}

impl<'de> Deserialize<'de> for InterfaceReachability {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let repr = InterfaceReachabilityRepr::deserialize(deserializer)?;
        let value = match repr {
            InterfaceReachabilityRepr::Offline => Self::Offline,
            InterfaceReachabilityRepr::LocalLinkOnly => Self::LocalLinkOnly,
            InterfaceReachabilityRepr::Routable {
                primary_transport,
                is_vpn,
                ipv4,
                ipv6,
            } => Self::Routable {
                primary_transport,
                is_vpn,
                ipv4,
                ipv6,
            },
        };
        value.validate().map_err(serde::de::Error::custom)?;
        Ok(value)
    }
}

/// Captive-portal state. A detected portal stores only a validated origin.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CaptivePortalStatus {
    Unknown,
    Clear,
    Suspected,
    Detected {
        portal_origin: Option<EndpointOrigin>,
    },
}

impl CaptivePortalStatus {
    pub const fn detected(portal_origin: Option<EndpointOrigin>) -> Self {
        Self::Detected { portal_origin }
    }

    /// Sanitize a portal URL into a detected status.
    ///
    /// The path, query and fragment are stripped before constructing the
    /// stored origin. Userinfo is rejected rather than stripped. Session
    /// tokens, Wi-Fi SSIDs, BSSIDs/MAC addresses and filesystem paths are
    /// therefore never retained by this type.
    pub fn detect_from_url(url: &str) -> Result<Self, SystemError> {
        let origin = EndpointOrigin::sanitize_url_origin(url)?;
        Ok(Self::Detected {
            portal_origin: Some(origin),
        })
    }

    /// Alias for [`Self::detect_from_url`].
    pub fn from_portal_url(url: &str) -> Result<Self, SystemError> {
        Self::detect_from_url(url)
    }

    pub fn portal_origin(&self) -> Option<&EndpointOrigin> {
        match self {
            Self::Detected { portal_origin } => portal_origin.as_ref(),
            _ => None,
        }
    }

    pub const fn is_unknown(&self) -> bool {
        matches!(self, Self::Unknown)
    }

    pub const fn is_clear(&self) -> bool {
        matches!(self, Self::Clear)
    }

    pub const fn is_suspected(&self) -> bool {
        matches!(self, Self::Suspected)
    }

    pub const fn is_detected(&self) -> bool {
        matches!(self, Self::Detected { .. })
    }

    /// True for states that block all workload classes.
    pub const fn blocks_workloads(&self) -> bool {
        matches!(self, Self::Detected { .. })
    }

    /// True for states that require foreground interaction before background
    /// revalidation or prefetch may proceed.
    pub const fn requires_attention(&self) -> bool {
        matches!(self, Self::Detected { .. } | Self::Suspected)
    }
}

/// Connection cost classification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConnectionCost {
    Unmetered,
    Metered,
    CellularRoaming,
}

impl ConnectionCost {
    pub const fn is_constrained(self) -> bool {
        !matches!(self, Self::Unmetered)
    }
}

/// Data-saver mode reported by the host or provider.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DataSaverMode {
    Off,
    Constrained,
    Unavailable,
}

impl DataSaverMode {
    pub const fn is_constrained(self) -> bool {
        matches!(self, Self::Constrained)
    }
}

/// Workload class used for deterministic budget decisions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NetworkWorkloadClass {
    Interactive,
    Revalidation,
    Prefetch,
    BackgroundBulk,
}

impl NetworkWorkloadClass {
    pub const ALL: [Self; 4] = [
        Self::Interactive,
        Self::Revalidation,
        Self::Prefetch,
        Self::BackgroundBulk,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Interactive => "interactive",
            Self::Revalidation => "revalidation",
            Self::Prefetch => "prefetch",
            Self::BackgroundBulk => "background_bulk",
        }
    }
}

/// Deterministic admission decision for one workload class.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkloadBudgetDecision {
    Allow,
    DeferUntilUnmetered,
    BlockWhileOfflineOrCaptive,
}

/// Coherent network policy observation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NetworkSnapshot {
    pub reachability: InterfaceReachability,
    pub captive_portal: CaptivePortalStatus,
    pub cost: ConnectionCost,
    pub data_saver: DataSaverMode,
    pub source: SignalSource,
    pub revision: u64,
}

impl NetworkSnapshot {
    pub fn new(
        reachability: InterfaceReachability,
        captive_portal: CaptivePortalStatus,
        cost: ConnectionCost,
        data_saver: DataSaverMode,
        source: SignalSource,
    ) -> Result<Self, SystemError> {
        reachability.validate()?;
        Ok(Self {
            reachability,
            captive_portal,
            cost,
            data_saver,
            source,
            revision: 0,
        })
    }

    /// Alias for [`Self::new`] used by host adapters that describe parts.
    pub fn from_parts(
        reachability: InterfaceReachability,
        captive_portal: CaptivePortalStatus,
        cost: ConnectionCost,
        data_saver: DataSaverMode,
        source: SignalSource,
    ) -> Result<Self, SystemError> {
        Self::new(reachability, captive_portal, cost, data_saver, source)
    }

    pub fn unknown(source: SignalSource) -> Self {
        Self {
            reachability: InterfaceReachability::Offline,
            captive_portal: CaptivePortalStatus::Unknown,
            cost: ConnectionCost::Unmetered,
            data_saver: DataSaverMode::Unavailable,
            source,
            revision: 0,
        }
    }

    pub fn offline(source: SignalSource) -> Self {
        Self::unknown(source)
    }

    pub fn routable(
        primary_transport: TransportKind,
        is_vpn: bool,
        ipv4: bool,
        ipv6: bool,
    ) -> Result<Self, SystemError> {
        let reachability = InterfaceReachability::routable(primary_transport, is_vpn, ipv4, ipv6)?;
        Ok(Self {
            reachability,
            captive_portal: CaptivePortalStatus::Clear,
            cost: ConnectionCost::Unmetered,
            data_saver: DataSaverMode::Off,
            source: SignalSource::Observed,
            revision: 0,
        })
    }

    pub fn with_reachability(mut self, reachability: InterfaceReachability) -> Self {
        self.reachability = reachability;
        self
    }

    /// Alias for [`Self::with_reachability`].
    pub fn with_interface(self, reachability: InterfaceReachability) -> Self {
        self.with_reachability(reachability)
    }

    pub fn interface_reachability(&self) -> InterfaceReachability {
        self.reachability
    }

    pub fn captive_portal_status(&self) -> &CaptivePortalStatus {
        &self.captive_portal
    }

    pub const fn connection_cost(&self) -> ConnectionCost {
        self.cost
    }

    pub const fn data_saver_mode(&self) -> DataSaverMode {
        self.data_saver
    }

    pub const fn signal_source(&self) -> SignalSource {
        self.source
    }

    pub fn with_captive_portal(mut self, captive_portal: CaptivePortalStatus) -> Self {
        self.captive_portal = captive_portal;
        self
    }

    pub fn with_cost(mut self, cost: ConnectionCost) -> Self {
        self.cost = cost;
        self
    }

    pub fn with_data_saver(mut self, data_saver: DataSaverMode) -> Self {
        self.data_saver = data_saver;
        self
    }

    pub fn with_source(mut self, source: SignalSource) -> Self {
        self.source = source;
        self
    }

    pub fn with_revision(mut self, revision: u64) -> Self {
        self.revision = revision;
        self
    }

    pub fn validate(&self) -> Result<(), SystemError> {
        self.reachability.validate()
    }

    /// True when only observation metadata changed.
    pub fn semantic_eq(&self, other: &Self) -> bool {
        self.reachability == other.reachability
            && self.captive_portal == other.captive_portal
            && self.cost == other.cost
            && self.data_saver == other.data_saver
    }

    pub fn is_routable(&self) -> bool {
        self.reachability.is_routable()
    }

    pub fn is_captive_blocked(&self) -> bool {
        self.captive_portal.blocks_workloads() || self.captive_portal.is_suspected()
    }

    /// Deterministic workload admission policy.
    ///
    /// Interface reachability, captive-portal status, cost and data saver are
    /// evaluated in that precedence order. The same inputs always produce the
    /// same decision; no wall clock or provider call participates.
    pub fn evaluate_workload_budget(
        &self,
        workload: NetworkWorkloadClass,
    ) -> WorkloadBudgetDecision {
        if self.reachability.validate().is_err() {
            return WorkloadBudgetDecision::BlockWhileOfflineOrCaptive;
        }
        if matches!(
            self.reachability,
            InterfaceReachability::Offline | InterfaceReachability::LocalLinkOnly
        ) || self.captive_portal.blocks_workloads()
        {
            return WorkloadBudgetDecision::BlockWhileOfflineOrCaptive;
        }
        if self.captive_portal.is_suspected() {
            return match workload {
                NetworkWorkloadClass::Interactive => WorkloadBudgetDecision::Allow,
                NetworkWorkloadClass::Revalidation
                | NetworkWorkloadClass::Prefetch
                | NetworkWorkloadClass::BackgroundBulk => {
                    WorkloadBudgetDecision::BlockWhileOfflineOrCaptive
                }
            };
        }
        if !self.reachability.is_routable() {
            return WorkloadBudgetDecision::BlockWhileOfflineOrCaptive;
        }
        if self.data_saver.is_constrained() || self.cost == ConnectionCost::CellularRoaming {
            return match workload {
                NetworkWorkloadClass::Interactive => WorkloadBudgetDecision::Allow,
                NetworkWorkloadClass::Revalidation
                | NetworkWorkloadClass::Prefetch
                | NetworkWorkloadClass::BackgroundBulk => {
                    WorkloadBudgetDecision::DeferUntilUnmetered
                }
            };
        }
        if self.cost == ConnectionCost::Metered {
            return match workload {
                NetworkWorkloadClass::Interactive | NetworkWorkloadClass::Revalidation => {
                    WorkloadBudgetDecision::Allow
                }
                NetworkWorkloadClass::Prefetch | NetworkWorkloadClass::BackgroundBulk => {
                    WorkloadBudgetDecision::DeferUntilUnmetered
                }
            };
        }
        WorkloadBudgetDecision::Allow
    }
}

impl Default for NetworkSnapshot {
    fn default() -> Self {
        Self::unknown(SignalSource::Unknown)
    }
}

/// DNS resolver failure categories.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DnsFailureKind {
    NonExistentDomain,
    Timeout,
    ServerFailure,
    Refused,
    NoRecords,
}

/// TLS verification failure categories.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TlsFailureReason {
    UntrustedRoot,
    HostnameMismatch,
    ExpiredOrNotYetValid,
    Revoked,
    PinMismatch,
}

impl TlsFailureReason {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::UntrustedRoot => "untrusted_root",
            Self::HostnameMismatch => "hostname_mismatch",
            Self::ExpiredOrNotYetValid => "expired_or_not_yet_valid",
            Self::Revoked => "revoked",
            Self::PinMismatch => "pin_mismatch",
        }
    }
}

/// Typed outcome of DNS, proxy, TLS and endpoint path probing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EndpointDiagnostic {
    Offline,
    CaptivePortalBlocked,
    ProxyResolutionFailed,
    ProxyUnreachable,
    ProxyAuthRequired { realm: Option<String> },
    ProxyAuthExhausted,
    DnsFailure { kind: DnsFailureKind },
    TcpConnectTimeout,
    TlsVerificationFailed { reason: TlsFailureReason },
    EndpointReachable { latency_ms: u32 },
    EndpointHttpUnavailable { status_code: u16 },
}

impl EndpointDiagnostic {
    /// Build a proxy-auth-required outcome with a sanitized realm.
    ///
    /// The realm is bounded to 128 bytes and must not contain control
    /// characters. An empty realm is represented as `None`.
    pub fn proxy_auth_required(realm: Option<&str>) -> Result<Self, SystemError> {
        Ok(Self::ProxyAuthRequired {
            realm: sanitize_realm(realm)?,
        })
    }

    /// True when automatic retry is useful for this diagnostic.
    pub fn is_retriable(&self) -> bool {
        match self {
            Self::DnsFailure { .. }
            | Self::TcpConnectTimeout
            | Self::ProxyUnreachable
            | Self::ProxyResolutionFailed => true,
            Self::EndpointHttpUnavailable { status_code } => {
                *status_code == 408 || *status_code == 429 || (500..=599).contains(status_code)
            }
            _ => false,
        }
    }
}

pub(crate) fn sanitize_realm(realm: Option<&str>) -> Result<Option<String>, SystemError> {
    let Some(realm) = realm else {
        return Ok(None);
    };
    if realm.len() > 128 {
        return Err(invalid("A proxy authentication realm exceeds 128 bytes"));
    }
    if realm.chars().any(char::is_control) {
        return Err(invalid(
            "A proxy authentication realm cannot contain control characters",
        ));
    }
    if realm.trim().is_empty() {
        return Ok(None);
    }
    Ok(Some(realm.trim().to_owned()))
}
