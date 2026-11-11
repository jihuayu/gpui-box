mod support;

use gpui_network::*;
use std::collections::HashSet;

#[test]
fn matrix_has_all_thirty_six_unique_contract_only_cells() {
    assert_eq!(NetworkOperation::ALL.len(), 6);
    assert_eq!(PlatformTarget::ALL.len(), 6);
    let matrix = platform_support_matrix();
    assert_eq!(matrix.len(), 36);
    let unique: HashSet<_> = matrix
        .iter()
        .map(|row| (row.platform, row.operation))
        .collect();
    assert_eq!(unique.len(), 36);
    assert!(
        matrix
            .iter()
            .all(|row| row.evidence == Evidence::ContractOnly)
    );
    for platform in PlatformTarget::ALL {
        let rows = platform_operations(platform);
        assert_eq!(rows.len(), 6);
        assert!(rows.iter().all(|row| row.platform == platform));
    }
    for operation in NetworkOperation::ALL {
        let row = operation_support(PlatformTarget::Windows, operation);
        assert_eq!(row.operation, operation);
        assert!(row.has_link("#69"));
        assert!(row.has_link("#19"));
        assert!(row.has_link("#21"));
        assert!(row.has_link("#27"));
    }
}

#[test]
fn native_platform_contracts_are_authentic() {
    let web_proxy = operation_support(
        PlatformTarget::Windows,
        NetworkOperation::ResolveSystemProxy,
    );
    assert!(
        web_proxy
            .required_host_configuration
            .contains("WinHttpGetProxyForUrl")
    );
    assert!(
        web_proxy
            .required_host_configuration
            .contains("WinHttpGetIEProxyConfigForCurrentUser")
    );

    let windows_connectivity = operation_support(
        PlatformTarget::Windows,
        NetworkOperation::ObserveConnectivity,
    );
    assert!(
        windows_connectivity
            .required_host_configuration
            .contains("INetworkListManager")
    );
    assert!(
        windows_connectivity
            .required_host_configuration
            .contains("NLM_CONNECTIVITY")
    );
    assert!(
        windows_connectivity
            .required_host_configuration
            .contains("NetworkCostType")
    );

    let windows_trust = operation_support(
        PlatformTarget::Windows,
        NetworkOperation::EvaluateTrustPolicy,
    );
    assert!(
        windows_trust
            .required_host_configuration
            .contains("CertGetCertificateChain")
    );
    assert!(
        windows_trust
            .required_host_configuration
            .contains("Group Policy")
    );

    let macos = operation_support(PlatformTarget::MacOs, NetworkOperation::ObserveConnectivity);
    assert!(macos.required_host_configuration.contains("NWPathMonitor"));
    assert!(macos.required_host_configuration.contains("isExpensive"));
    assert!(macos.required_host_configuration.contains("isConstrained"));
    let macos_proxy =
        operation_support(PlatformTarget::MacOs, NetworkOperation::ResolveSystemProxy);
    assert!(
        macos_proxy
            .required_host_configuration
            .contains("CFNetworkCopyProxiesForURL")
    );
    assert!(
        macos_proxy
            .required_host_configuration
            .contains("SCDynamicStoreCopyProxies")
    );
    let macos_trust =
        operation_support(PlatformTarget::MacOs, NetworkOperation::EvaluateTrustPolicy);
    assert!(
        macos_trust
            .required_host_configuration
            .contains("SecTrustEvaluateWithError")
    );

    let linux = operation_support(PlatformTarget::Linux, NetworkOperation::ObserveConnectivity);
    assert!(
        linux
            .required_host_configuration
            .contains("org.freedesktop.NetworkManager")
    );
    assert!(
        linux
            .required_host_configuration
            .contains("org.freedesktop.portal.NetworkMonitor")
    );
    let linux_proxy =
        operation_support(PlatformTarget::Linux, NetworkOperation::ResolveSystemProxy);
    assert!(
        linux_proxy
            .required_host_configuration
            .contains("org.freedesktop.portal.ProxyResolver")
    );
    assert!(
        linux_proxy
            .required_host_configuration
            .contains("HTTPS_PROXY")
    );
    assert!(
        linux_proxy
            .required_host_configuration
            .contains("HTTP_PROXY")
    );
    assert!(
        linux_proxy
            .required_host_configuration
            .contains("ALL_PROXY")
    );
    assert!(linux_proxy.required_host_configuration.contains("NO_PROXY"));
    let linux_trust =
        operation_support(PlatformTarget::Linux, NetworkOperation::EvaluateTrustPolicy);
    assert!(
        linux_trust
            .required_host_configuration
            .contains("/etc/ssl/certs")
    );
    assert!(
        linux_trust
            .required_host_configuration
            .contains("SSL_CERT_FILE")
    );

    let android = operation_support(
        PlatformTarget::Android,
        NetworkOperation::ObserveConnectivity,
    );
    assert!(
        android
            .required_host_configuration
            .contains("ConnectivityManager.NetworkCallback")
    );
    assert!(
        android
            .required_host_configuration
            .contains("NET_CAPABILITY_INTERNET")
    );
    assert!(
        android
            .required_host_configuration
            .contains("NET_CAPABILITY_VALIDATED")
    );
    assert!(
        android
            .required_host_configuration
            .contains("NET_CAPABILITY_CAPTIVE_PORTAL")
    );
    assert!(
        android
            .required_host_configuration
            .contains("NET_CAPABILITY_NOT_METERED")
    );
    assert!(
        android
            .required_host_configuration
            .contains("NET_CAPABILITY_NOT_VPN")
    );
    assert!(
        android
            .required_host_configuration
            .contains("getRestrictBackgroundStatus")
    );
    let android_trust = operation_support(
        PlatformTarget::Android,
        NetworkOperation::EvaluateTrustPolicy,
    );
    assert!(
        android_trust
            .required_host_configuration
            .contains("network_security_config.xml")
    );

    let ios = operation_support(PlatformTarget::Ios, NetworkOperation::ObserveConnectivity);
    assert!(ios.required_host_configuration.contains("NWPathMonitor"));
    assert!(ios.required_host_configuration.contains("isExpensive"));
    assert!(ios.required_host_configuration.contains("isConstrained"));
    assert!(
        ios.required_host_configuration
            .contains("usesInterfaceType")
    );
    let ios_trust = operation_support(PlatformTarget::Ios, NetworkOperation::EvaluateTrustPolicy);
    assert!(
        ios_trust
            .required_host_configuration
            .contains("NSAppTransportSecurity")
    );
    assert!(
        ios_trust
            .required_host_configuration
            .contains("SecTrustEvaluateWithError")
    );
}

#[test]
fn web_support_is_conditional_and_browser_owned_operations_fail_closed() {
    let web = NetworkPlatform::Web;
    assert_eq!(
        operation_support(web, NetworkOperation::ObserveConnectivity).support,
        Support::Conditional
    );
    assert_eq!(
        operation_support(web, NetworkOperation::DiagnoseEndpointPath).support,
        Support::Conditional
    );
    for operation in [
        NetworkOperation::ResolveSystemProxy,
        NetworkOperation::HandleProxyAuth,
        NetworkOperation::EvaluateTrustPolicy,
    ] {
        let row = operation_support(web, operation);
        assert_eq!(row.support, Support::Unsupported);
        assert!(!row.is_supported());
        assert!(
            row.required_host_configuration.contains("browser")
                || row.required_host_configuration.contains("Browser")
                || row.required_host_configuration.contains("Unsupported")
        );
    }
    assert_eq!(
        operation_support(web, NetworkOperation::SubscribeRevalidation).support,
        Support::Native
    );
    assert!(
        operation_support(web, NetworkOperation::ObserveConnectivity)
            .required_host_configuration
            .contains("navigator.onLine")
    );

    let policy = web_capability_policy(WebBackendMaturity::Experimental);
    assert_eq!(policy.maturity, WebBackendMaturity::Experimental);
    assert_eq!(policy.backend_name, "navigator.onLine");
    assert_eq!(policy.connectivity_event, "online/offline");
    assert_eq!(policy.proxy_owner, "browser");
    assert_eq!(policy.observe_connectivity, Support::Conditional);
    assert_eq!(policy.resolve_system_proxy, Support::Unsupported);
    assert_eq!(policy.handle_proxy_auth, Support::Unsupported);
    assert_eq!(policy.evaluate_trust_policy, Support::Unsupported);
    assert_eq!(policy.diagnose_endpoint_path, Support::Conditional);
    assert_eq!(policy.subscribe_revalidation, Support::Native);
    assert!(!policy.custom_tls_pins_available);
    assert!(policy.browser_managed_tls_only);
    assert!(policy.online_hint_is_coarse);
    assert!(policy.supports(NetworkOperation::ObserveConnectivity));
    assert!(!policy.supports(NetworkOperation::ResolveSystemProxy));
}

#[test]
fn mobile_trust_policy_is_conditional_and_desktop_is_native() {
    for mobile in [PlatformTarget::Android, PlatformTarget::Ios] {
        assert_eq!(
            operation_support(mobile, NetworkOperation::EvaluateTrustPolicy).support,
            Support::Conditional
        );
    }
    for desktop in [
        PlatformTarget::Windows,
        PlatformTarget::MacOs,
        PlatformTarget::Linux,
    ] {
        assert_eq!(
            operation_support(desktop, NetworkOperation::EvaluateTrustPolicy).support,
            Support::Native
        );
    }
}

#[test]
fn platform_labels_and_targets_are_stable() {
    assert!(PlatformTarget::current().is_some());
    assert_eq!(NetworkPlatform::Web.as_str(), "web");
    assert_eq!(PlatformTarget::Ios.label(), "iOS");
    assert_eq!(
        NetworkOperation::ObserveConnectivity.as_str(),
        "observe_connectivity"
    );
    assert_eq!(NetworkOperation::HandleProxyAuth.label(), "HandleProxyAuth");
    assert_eq!(Support::Conditional.as_str_for_test(), "conditional");
}

trait SupportLabel {
    fn as_str_for_test(&self) -> &'static str;
}

impl SupportLabel for Support {
    fn as_str_for_test(&self) -> &'static str {
        match self {
            Support::Native => "native",
            Support::Conditional => "conditional",
            Support::FallbackAvailable => "fallback_available",
            Support::Unsupported => "unsupported",
            Support::Unknown => "unknown",
        }
    }
}
