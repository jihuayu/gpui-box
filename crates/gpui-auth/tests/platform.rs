//! Six-platform × ten-operation capability matrix invariants.

use gpui_auth::*;
use gpui_system::Evidence;

#[test]
fn matrix_has_sixty_unique_contract_only_cells() {
    let matrix = platform_support_matrix();
    assert_eq!(matrix.len(), 60);
    for platform in AuthPlatform::ALL {
        let operations = platform_operations(platform);
        assert_eq!(operations.len(), AuthOperation::ALL.len());
        assert_eq!(operations.len(), 10);
        for cell in operations {
            assert_eq!(cell.platform, platform);
            assert_eq!(cell.evidence, Evidence::ContractOnly);
            assert!(!cell.native_api.is_empty());
            assert!(!cell.prerequisites.is_empty());
        }
    }
    for operation in AuthOperation::ALL {
        assert_eq!(operation_support(operation).len(), 6);
    }
    for manifest in AUTH_PLATFORM_MANIFEST {
        assert!(!manifest.native_adapter_installed);
        assert_eq!(manifest.evidence, Evidence::ContractOnly);
    }
    let unique: std::collections::BTreeSet<_> = matrix
        .iter()
        .map(|cell| (cell.platform.as_str(), cell.operation.as_str()))
        .collect();
    assert_eq!(unique.len(), 60);
}

#[test]
fn redirect_support_reflects_platform_reality() {
    use AuthOperation as Op;
    use AuthPlatform as P;

    for platform in [P::Windows, P::MacOs, P::Linux] {
        assert_eq!(
            support(platform, Op::LoopbackRedirect),
            SupportLevel::Supported
        );
    }
    for platform in [P::Android, P::Ios, P::Web] {
        assert_eq!(
            support(platform, Op::LoopbackRedirect),
            SupportLevel::Unsupported
        );
    }
    assert_eq!(
        support(P::Web, Op::WebSameOriginRedirect),
        SupportLevel::Supported
    );
    for platform in [P::Windows, P::MacOs, P::Linux, P::Android, P::Ios] {
        assert_eq!(
            support(platform, Op::WebSameOriginRedirect),
            SupportLevel::Unsupported
        );
    }
    for platform in [P::Android, P::Ios] {
        assert_eq!(
            support(platform, Op::UniversalLinkRedirect),
            SupportLevel::Supported
        );
    }
    for platform in [P::Windows, P::MacOs, P::Linux, P::Web] {
        assert_eq!(
            support(platform, Op::UniversalLinkRedirect),
            SupportLevel::Unsupported
        );
    }
    assert_eq!(
        support(P::Web, Op::CustomSchemeRedirect),
        SupportLevel::Unsupported
    );
    for platform in [P::Windows, P::MacOs, P::Linux, P::Android, P::Ios] {
        assert_eq!(
            support(platform, Op::CustomSchemeRedirect),
            SupportLevel::Supported
        );
    }
    assert_eq!(
        support(P::Linux, Op::PasskeyRegistration),
        SupportLevel::Conditional
    );
    assert_eq!(
        support(P::Linux, Op::PasskeyAssertion),
        SupportLevel::Conditional
    );
    assert_eq!(
        support(P::Linux, Op::PasskeyConditionalMediation),
        SupportLevel::Conditional
    );
    assert_eq!(
        support(P::Web, Op::PasskeyRegistration),
        SupportLevel::Conditional
    );
    assert_eq!(
        support(P::Web, Op::PasskeyAssertion),
        SupportLevel::Conditional
    );
    assert_eq!(
        support(P::Web, Op::PasskeyConditionalMediation),
        SupportLevel::Conditional
    );
}

#[test]
fn native_api_names_are_authentic_and_platform_specific() {
    let windows = platform_operations(AuthPlatform::Windows)
        .into_iter()
        .map(|cell| cell.native_api)
        .collect::<Vec<_>>()
        .join(" ");
    assert!(windows.contains("WebAuthn API"));
    assert!(windows.contains("Credential Manager"));
    assert!(windows.contains("WebView2"));

    let macos = platform_operations(AuthPlatform::MacOs)
        .into_iter()
        .map(|cell| cell.native_api)
        .collect::<Vec<_>>()
        .join(" ");
    assert!(macos.contains("ASWebAuthenticationSession"));
    assert!(macos.contains("AuthenticationServices"));
    assert!(macos.contains("Keychain Services"));

    let linux = platform_operations(AuthPlatform::Linux)
        .into_iter()
        .map(|cell| cell.native_api)
        .collect::<Vec<_>>()
        .join(" ");
    assert!(linux.contains("xdg-open"));
    assert!(linux.contains("libfido2"));
    assert!(linux.contains("Secret Service"));

    let android = platform_operations(AuthPlatform::Android)
        .into_iter()
        .map(|cell| cell.native_api)
        .collect::<Vec<_>>()
        .join(" ");
    assert!(android.contains("Custom Tabs"));
    assert!(android.contains("Credential Manager"));
    assert!(android.contains("Android Keystore"));

    let ios = platform_operations(AuthPlatform::Ios)
        .into_iter()
        .map(|cell| cell.native_api)
        .collect::<Vec<_>>()
        .join(" ");
    assert!(ios.contains("Universal Links"));
    assert!(ios.contains("ASWebAuthenticationSession"));
    assert!(ios.contains("AuthenticationServices"));

    let web = platform_operations(AuthPlatform::Web)
        .into_iter()
        .map(|cell| cell.native_api)
        .collect::<Vec<_>>()
        .join(" ");
    assert!(web.contains("window.open"));
    assert!(web.contains("navigator.credentials"));
    assert!(web.contains("WebCrypto"));
}

#[test]
fn operation_labels_and_system_platform_mapping_are_stable() {
    assert_eq!(AuthPlatform::ALL.len(), 6);
    assert_eq!(AuthOperation::ALL.len(), 10);
    assert!(AuthPlatform::Web.native_platform().is_none());
    for platform in AuthPlatform::ALL
        .into_iter()
        .filter(|platform| *platform != AuthPlatform::Web)
    {
        assert!(platform.native_platform().is_some());
    }
    for operation in AuthOperation::ALL {
        assert!(!operation.as_str().is_empty());
    }
}
