//! Six-platform × six-operation support policy and browser capability limits.
//!
//! Every row is [`Evidence::ContractOnly`]. Native providers are separate work;
//! no row claims compiled or runtime qualification.

use gpui_system::{Evidence, Support};
use serde::{Deserialize, Serialize};
use std::fmt;

/// The six targets covered by the network support matrix.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlatformTarget {
    Windows,
    MacOs,
    Linux,
    Android,
    Ios,
    Web,
}

/// Compatibility name used by the network policy document.
pub type NetworkPlatform = PlatformTarget;

impl PlatformTarget {
    pub const ALL: [Self; 6] = [
        Self::Windows,
        Self::MacOs,
        Self::Linux,
        Self::Android,
        Self::Ios,
        Self::Web,
    ];

    /// The compile-time target running this code, if it is one of the rows.
    pub const fn current() -> Option<Self> {
        if cfg!(target_arch = "wasm32") {
            Some(Self::Web)
        } else if cfg!(target_os = "windows") {
            Some(Self::Windows)
        } else if cfg!(target_os = "macos") {
            Some(Self::MacOs)
        } else if cfg!(target_os = "linux") {
            Some(Self::Linux)
        } else if cfg!(target_os = "android") {
            Some(Self::Android)
        } else if cfg!(target_os = "ios") {
            Some(Self::Ios)
        } else {
            None
        }
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Windows => "windows",
            Self::MacOs => "macos",
            Self::Linux => "linux",
            Self::Android => "android",
            Self::Ios => "ios",
            Self::Web => "web",
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::Windows => "Windows",
            Self::MacOs => "macOS",
            Self::Linux => "Linux",
            Self::Android => "Android",
            Self::Ios => "iOS",
            Self::Web => "Web",
        }
    }

    pub fn operations(self) -> Vec<PlatformOperationSupport> {
        platform_operations(self)
    }
}

impl fmt::Display for PlatformTarget {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// The six network operations covered by the qualification matrix.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NetworkOperation {
    ObserveConnectivity,
    ResolveSystemProxy,
    HandleProxyAuth,
    EvaluateTrustPolicy,
    DiagnoseEndpointPath,
    SubscribeRevalidation,
}

impl NetworkOperation {
    pub const ALL: [Self; 6] = [
        Self::ObserveConnectivity,
        Self::ResolveSystemProxy,
        Self::HandleProxyAuth,
        Self::EvaluateTrustPolicy,
        Self::DiagnoseEndpointPath,
        Self::SubscribeRevalidation,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ObserveConnectivity => "observe_connectivity",
            Self::ResolveSystemProxy => "resolve_system_proxy",
            Self::HandleProxyAuth => "handle_proxy_auth",
            Self::EvaluateTrustPolicy => "evaluate_trust_policy",
            Self::DiagnoseEndpointPath => "diagnose_endpoint_path",
            Self::SubscribeRevalidation => "subscribe_revalidation",
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::ObserveConnectivity => "ObserveConnectivity",
            Self::ResolveSystemProxy => "ResolveSystemProxy",
            Self::HandleProxyAuth => "HandleProxyAuth",
            Self::EvaluateTrustPolicy => "EvaluateTrustPolicy",
            Self::DiagnoseEndpointPath => "DiagnoseEndpointPath",
            Self::SubscribeRevalidation => "SubscribeRevalidation",
        }
    }
}

impl fmt::Display for NetworkOperation {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// One `(platform, operation)` policy row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlatformOperationSupport {
    pub platform: PlatformTarget,
    pub operation: NetworkOperation,
    pub support: Support,
    pub required_host_configuration: &'static str,
    pub evidence: Evidence,
    pub notes: &'static str,
    pub qualification_links: &'static [&'static str],
}

impl PlatformOperationSupport {
    pub const fn new(
        platform: PlatformTarget,
        operation: NetworkOperation,
        support: Support,
        required_host_configuration: &'static str,
        evidence: Evidence,
        notes: &'static str,
        qualification_links: &'static [&'static str],
    ) -> Self {
        Self {
            platform,
            operation,
            support,
            required_host_configuration,
            evidence,
            notes,
            qualification_links,
        }
    }

    pub const fn is_supported(&self) -> bool {
        matches!(self.support, Support::Native | Support::Conditional)
    }

    pub fn has_link(&self, link: &str) -> bool {
        self.qualification_links.contains(&link)
    }
}

const WINDOWS_LINKS: &[&str] = &["#69", "#19", "#21", "#27", "#8"];
const MACOS_LINKS: &[&str] = &["#69", "#19", "#21", "#27", "#8"];
const LINUX_LINKS: &[&str] = &["#69", "#19", "#21", "#27", "#8", "#54"];
const ANDROID_LINKS: &[&str] = &["#69", "#19", "#21", "#27", "#2"];
const IOS_LINKS: &[&str] = &["#69", "#19", "#21", "#27", "#9"];
const WEB_LINKS: &[&str] = &["#69", "#19", "#21", "#18", "#43", "#3"];

fn platform_links(platform: PlatformTarget) -> &'static [&'static str] {
    match platform {
        PlatformTarget::Windows => WINDOWS_LINKS,
        PlatformTarget::MacOs => MACOS_LINKS,
        PlatformTarget::Linux => LINUX_LINKS,
        PlatformTarget::Android => ANDROID_LINKS,
        PlatformTarget::Ios => IOS_LINKS,
        PlatformTarget::Web => WEB_LINKS,
    }
}

fn platform_support(platform: PlatformTarget, operation: NetworkOperation) -> Support {
    match (platform, operation) {
        (
            PlatformTarget::Web,
            NetworkOperation::ObserveConnectivity | NetworkOperation::DiagnoseEndpointPath,
        ) => Support::Conditional,
        (
            PlatformTarget::Web,
            NetworkOperation::ResolveSystemProxy
            | NetworkOperation::HandleProxyAuth
            | NetworkOperation::EvaluateTrustPolicy,
        ) => Support::Unsupported,
        (PlatformTarget::Android | PlatformTarget::Ios, NetworkOperation::EvaluateTrustPolicy) => {
            Support::Conditional
        }
        _ => Support::Native,
    }
}

fn platform_configuration(platform: PlatformTarget, operation: NetworkOperation) -> &'static str {
    match (platform, operation) {
        (PlatformTarget::Windows, NetworkOperation::ObserveConnectivity) => {
            "INetworkListManager.GetConnectivity / NLM_CONNECTIVITY for interface reachability; Windows.Networking.Connectivity.NetworkInformation.GetInternetConnectionProfile for NetworkCostType and metered status."
        }
        (PlatformTarget::Windows, NetworkOperation::ResolveSystemProxy) => {
            "WinHttpGetIEProxyConfigForCurrentUser plus WinHttpGetProxyForUrl (WinHTTP) with AutoDetect/AutoConfigURL and per-URL bypass evaluation. Resolve only the origin; never retain a PAC query token."
        }
        (PlatformTarget::Windows, NetworkOperation::HandleProxyAuth) => {
            "Handle HTTP 407 from WinHTTP/WinINet, read the Proxy-Authenticate realm, resolve credentials through gpui-secrets and bound attempts per (client, proxy, policy epoch)."
        }
        (PlatformTarget::Windows, NetworkOperation::EvaluateTrustPolicy) => {
            "CertGetCertificateChain with CERT_CHAIN_POLICY_SSL against the system store and Group Policy enterprise roots; map CERT_TRUST_IS_UNTRUSTED_ROOT, REVOKED, EXPIRED and hostname mismatch to typed reasons."
        }
        (PlatformTarget::Windows, NetworkOperation::DiagnoseEndpointPath) => {
            "GetAddrInfoEx and ConnectEx with explicit deadlines, WinHTTP proxy tunnel diagnostics; return typed DNS, TCP, TLS and HTTP outcomes rather than a single reachable flag."
        }
        (PlatformTarget::Windows, NetworkOperation::SubscribeRevalidation) => {
            "Network List Manager COM connection/media/cost events on a message-pumping UI thread; dispatch scoped directives outside internal locks."
        }
        (PlatformTarget::MacOs, NetworkOperation::ObserveConnectivity) => {
            "NWPathMonitor (Network.framework) for interface status, NWPath.isExpensive and NWPath.isConstrained; map NWInterface.InterfaceType to TransportKind."
        }
        (PlatformTarget::MacOs, NetworkOperation::ResolveSystemProxy) => {
            "CFNetworkCopyProxiesForURL and SCDynamicStoreCopyProxies for system proxy/PAC state; validate every returned proxy as an EndpointOrigin."
        }
        (PlatformTarget::MacOs, NetworkOperation::HandleProxyAuth) => {
            "Handle 407 through CFNetwork/URLSession, read the bounded Proxy-Authenticate realm and resolve credentials from Keychain-backed gpui-secrets on a background context."
        }
        (PlatformTarget::MacOs, NetworkOperation::EvaluateTrustPolicy) => {
            "SecTrustEvaluateWithError plus SecTrustCopyAnchorCertificates for system anchors and admin-keychain enterprise anchors; never downgrade a pinned policy to unpinned TLS."
        }
        (PlatformTarget::MacOs, NetworkOperation::DiagnoseEndpointPath) => {
            "NWConnection/CFHost with explicit timeouts for DNS and TCP, SecTrust for TLS and CFNetwork for TLS/HTTP status; preserve the exact failure reason."
        }
        (PlatformTarget::MacOs, NetworkOperation::SubscribeRevalidation) => {
            "NWPathMonitor update handler on a serial queue plus SystemConfiguration change notifications; coalesce background changes and flush one revalidation on foreground."
        }
        (PlatformTarget::Linux, NetworkOperation::ObserveConnectivity) => {
            "NetworkManager D-Bus org.freedesktop.NetworkManager Connectivity/Metered state and org.freedesktop.portal.NetworkMonitor; environment-only hosts must report inferred/unavailable provenance."
        }
        (PlatformTarget::Linux, NetworkOperation::ResolveSystemProxy) => {
            "org.freedesktop.portal.ProxyResolver or NetworkManager proxy configuration, with HTTPS_PROXY, HTTP_PROXY, ALL_PROXY and NO_PROXY environment fallback; keep /etc/ssl/certs and SSL_CERT_FILE inside native providers."
        }
        (PlatformTarget::Linux, NetworkOperation::HandleProxyAuth) => {
            "Handle 407 from the HTTP stack, use Secret Service through gpui-secrets, and bound retries by policy epoch. Environment proxy URLs must be validated as origins before use."
        }
        (PlatformTarget::Linux, NetworkOperation::EvaluateTrustPolicy) => {
            "OpenSSL/GnuTLS system trust from /etc/ssl/certs or SSL_CERT_FILE plus explicitly configured enterprise anchors; never silently fall back to an unpinned browser root set."
        }
        (PlatformTarget::Linux, NetworkOperation::DiagnoseEndpointPath) => {
            "getaddrinfo/res_query with deadlines, non-blocking connect timeouts and TLS verification; distinguish NXDOMAIN, timeout, refused and no-record outcomes."
        }
        (PlatformTarget::Linux, NetworkOperation::SubscribeRevalidation) => {
            "Subscribe to NetworkManager and xdg-desktop-portal NetworkMonitor D-Bus signals; coalesce costs and captive-portal changes before delivering scoped directives."
        }
        (PlatformTarget::Android, NetworkOperation::ObserveConnectivity) => {
            "ConnectivityManager.NetworkCallback with NET_CAPABILITY_INTERNET, NET_CAPABILITY_VALIDATED, NET_CAPABILITY_CAPTIVE_PORTAL, NET_CAPABILITY_NOT_METERED and NET_CAPABILITY_NOT_VPN; ConnectivityManager.getRestrictBackgroundStatus() for data saver."
        }
        (PlatformTarget::Android, NetworkOperation::ResolveSystemProxy) => {
            "LinkProperties.getHttpProxy() / ProxyInfo per active Network; captive-portal probe URLs are reduced to a validated EndpointOrigin and never retain SSIDs, BSSIDs or MAC addresses."
        }
        (PlatformTarget::Android, NetworkOperation::HandleProxyAuth) => {
            "Handle ProxyInfo authentication challenges with credentials from EncryptedSharedPreferences/Keystore-backed gpui-secrets; enforce the per-epoch two-attempt cap."
        }
        (PlatformTarget::Android, NetworkOperation::EvaluateTrustPolicy) => {
            "Android Network Security Config (network_security_config.xml) with user-CA exclusion in release builds by default; DebugOverride may explicitly permit user roots. ATS/NSC restrictions are never bypassed implicitly."
        }
        (PlatformTarget::Android, NetworkOperation::DiagnoseEndpointPath) => {
            "Network.getAllByName / InetAddress with timeouts, Socket connect deadlines, platform TLS verification and HttpURLConnection status; map NET_CAPABILITY_CAPTIVE_PORTAL before endpoint failures."
        }
        (PlatformTarget::Android, NetworkOperation::SubscribeRevalidation) => {
            "Retain ConnectivityManager.NetworkCallback registrations for the visible lifecycle and unregister on surface disposal; deliver directives on the app executor."
        }
        (PlatformTarget::Ios, NetworkOperation::ObserveConnectivity) => {
            "NWPathMonitor with NWPath.isExpensive, NWPath.isConstrained and NWPath.usesInterfaceType; never infer metered status from a coarse online flag."
        }
        (PlatformTarget::Ios, NetworkOperation::ResolveSystemProxy) => {
            "CFNetworkCopySystemProxySettings for HTTP/HTTPS/SOCKS proxies and exclusions; validate every proxy string as an EndpointOrigin."
        }
        (PlatformTarget::Ios, NetworkOperation::HandleProxyAuth) => {
            "Handle 407 through URLSession authentication challenges and Keychain-backed gpui-secrets; cap attempts per policy epoch and never persist a header."
        }
        (PlatformTarget::Ios, NetworkOperation::EvaluateTrustPolicy) => {
            "App Transport Security (NSAppTransportSecurity) plus SecTrustEvaluateWithError and user-anchor constraints; release builds reject user-installed roots unless DebugOverride is explicit."
        }
        (PlatformTarget::Ios, NetworkOperation::DiagnoseEndpointPath) => {
            "NWConnection/CFHost deadlines and URLSession task metrics; distinguish DNS, TCP, TLS and HTTP status while ATS may block cleartext before a connection."
        }
        (PlatformTarget::Ios, NetworkOperation::SubscribeRevalidation) => {
            "NWPathMonitor updates plus app scene lifecycle; suppress revalidation while backgrounded and flush one coalesced directive on active."
        }
        (PlatformTarget::Web, NetworkOperation::ObserveConnectivity) => {
            "navigator.onLine plus online/offline events are a coarse browser hint only; there is no interface, VPN, captive-portal or metered detail. Report Support::Conditional and never claim endpoint reachability."
        }
        (PlatformTarget::Web, NetworkOperation::ResolveSystemProxy) => {
            "Unsupported: the browser owns proxy, PAC and bypass resolution. Only ProxyConfig::BrowserOpaque is valid and the service fails closed with ErrorCode::Unsupported."
        }
        (PlatformTarget::Web, NetworkOperation::HandleProxyAuth) => {
            "Unsupported: 407 challenges, Proxy-Authorization and proxy credentials are owned by the browser sandbox and are not exposed to the application."
        }
        (PlatformTarget::Web, NetworkOperation::EvaluateTrustPolicy) => {
            "Unsupported for custom or pinned policy: browser TLS owns roots, pins and certificate errors. Only TrustStorePolicy::BrowserManagedTls is accepted and custom SPKI pins fail closed."
        }
        (PlatformTarget::Web, NetworkOperation::DiagnoseEndpointPath) => {
            "Conditional: fetch/WebSocket observe only application-level success or failure. DNS, TCP, TLS and proxy stages remain opaque, so diagnostics are coarse and non-authoritative."
        }
        (PlatformTarget::Web, NetworkOperation::SubscribeRevalidation) => {
            "online/offline events can wake a scoped revalidation loop; consumers must still coalesce, respect visibility and re-check navigator.onLine before background work."
        }
    }
}

fn platform_notes(platform: PlatformTarget, operation: NetworkOperation) -> &'static str {
    match (platform, operation) {
        (_, NetworkOperation::ObserveConnectivity) => {
            "Link up is not endpoint availability. A routable interface can still be captive, proxy-blocked, DNS-broken or TLS-invalid."
        }
        (_, NetworkOperation::ResolveSystemProxy) => {
            "Resolution returns ordered candidates; an unsupported or browser-owned policy fails closed rather than silently connecting directly."
        }
        (_, NetworkOperation::HandleProxyAuth) => {
            "Credentials are background-only, zeroized and redacted. More than two 407 challenges in one policy epoch exhaust without resending stale credentials."
        }
        (PlatformTarget::Web, NetworkOperation::EvaluateTrustPolicy) => {
            "Browser-managed TLS is an honest contract: the application cannot implement custom pinning on Web."
        }
        (_, NetworkOperation::EvaluateTrustPolicy) => {
            "Trust evaluation is deterministic and fail-closed; revocation and expiry take precedence over pins and enterprise bypass."
        }
        (_, NetworkOperation::DiagnoseEndpointPath) => {
            "DNS, proxy, TCP, TLS and HTTP outcomes stay distinct so a captive portal is never reported as an unreachable server."
        }
        (_, NetworkOperation::SubscribeRevalidation) => {
            "Directives are scoped to live client leases; background phases defer stale-query invalidation and coalesce it on resume."
        }
    }
}

/// The complete six-target, six-operation support matrix (36 cells).
pub fn platform_support_matrix() -> Vec<PlatformOperationSupport> {
    let mut rows = Vec::with_capacity(36);
    for platform in PlatformTarget::ALL {
        for operation in NetworkOperation::ALL {
            rows.push(PlatformOperationSupport::new(
                platform,
                operation,
                platform_support(platform, operation),
                platform_configuration(platform, operation),
                Evidence::ContractOnly,
                platform_notes(platform, operation),
                platform_links(platform),
            ));
        }
    }
    rows
}

/// Look up one exact `(platform, operation)` row.
pub fn operation_support(
    platform: PlatformTarget,
    operation: NetworkOperation,
) -> PlatformOperationSupport {
    platform_support_matrix()
        .into_iter()
        .find(|row| row.platform == platform && row.operation == operation)
        .expect("the support matrix contains every platform/operation pair")
}

/// All six operation rows for one platform in declaration order.
pub fn platform_operations(platform: PlatformTarget) -> Vec<PlatformOperationSupport> {
    NetworkOperation::ALL
        .into_iter()
        .map(|operation| operation_support(platform, operation))
        .collect()
}

/// Maturity of the browser backend.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WebBackendMaturity {
    Experimental,
    Preview,
    Stable,
}

impl WebBackendMaturity {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Experimental => "experimental",
            Self::Preview => "preview",
            Self::Stable => "stable",
        }
    }
}

/// Browser network capability policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WebNetworkPolicy {
    pub maturity: WebBackendMaturity,
    pub backend_name: &'static str,
    pub connectivity_event: &'static str,
    pub observe_connectivity: Support,
    pub resolve_system_proxy: Support,
    pub handle_proxy_auth: Support,
    pub evaluate_trust_policy: Support,
    pub diagnose_endpoint_path: Support,
    pub subscribe_revalidation: Support,
    pub proxy_owner: &'static str,
    pub custom_tls_pins_available: bool,
    pub browser_managed_tls_only: bool,
    pub online_hint_is_coarse: bool,
}

impl WebNetworkPolicy {
    pub const fn support_for(self, operation: NetworkOperation) -> Support {
        match operation {
            NetworkOperation::ObserveConnectivity => self.observe_connectivity,
            NetworkOperation::ResolveSystemProxy => self.resolve_system_proxy,
            NetworkOperation::HandleProxyAuth => self.handle_proxy_auth,
            NetworkOperation::EvaluateTrustPolicy => self.evaluate_trust_policy,
            NetworkOperation::DiagnoseEndpointPath => self.diagnose_endpoint_path,
            NetworkOperation::SubscribeRevalidation => self.subscribe_revalidation,
        }
    }

    pub const fn supports(self, operation: NetworkOperation) -> bool {
        matches!(
            self.support_for(operation),
            Support::Native | Support::Conditional
        )
    }
}

/// The Web policy aligned with browser reality.
pub fn web_capability_policy(maturity: WebBackendMaturity) -> WebNetworkPolicy {
    WebNetworkPolicy {
        maturity,
        backend_name: "navigator.onLine",
        connectivity_event: "online/offline",
        observe_connectivity: Support::Conditional,
        resolve_system_proxy: Support::Unsupported,
        handle_proxy_auth: Support::Unsupported,
        evaluate_trust_policy: Support::Unsupported,
        diagnose_endpoint_path: Support::Conditional,
        subscribe_revalidation: Support::Native,
        proxy_owner: "browser",
        custom_tls_pins_available: false,
        browser_managed_tls_only: true,
        online_hint_is_coarse: true,
    }
}
