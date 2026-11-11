mod support;

use gpui_network::*;
use gpui_system::ErrorCode;
use std::collections::BTreeSet;
use support::*;

fn pin(value: u8) -> SpkiPinSha256 {
    SpkiPinSha256::from_bytes([value; 32])
}

#[test]
fn spki_pins_round_trip_hex_order_and_debug() {
    let value =
        SpkiPinSha256::from_hex("000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f")
            .expect("hex pin");
    assert_eq!(
        value.to_hex(),
        "000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f"
    );
    assert_eq!(
        SpkiPinSha256::from_hex(&value.to_hex().to_uppercase()).unwrap(),
        value
    );
    assert!(format!("{value:?}").contains(&value.to_hex()));
    assert!(value < pin(1));
    assert_eq!(pin(2), pin(2));

    for invalid in ["", "00", &"0".repeat(63), &"g".repeat(64)] {
        assert_eq!(
            SpkiPinSha256::from_hex(invalid).unwrap_err().code,
            ErrorCode::InvalidInput
        );
    }

    let json = serde_json::to_string(&value).unwrap();
    assert_eq!(json, format!("\"{}\"", value.to_hex()));
    let decoded: SpkiPinSha256 = serde_json::from_str(&json).unwrap();
    assert_eq!(decoded, value);
    assert!(serde_json::from_str::<SpkiPinSha256>("\"zz\"").is_err());
}

#[test]
fn platform_trust_policy_validation_is_fail_closed() {
    let strict = TrustStorePolicy::strict_pins([pin(1)], true, false).unwrap();
    let empty: BTreeSet<SpkiPinSha256> = BTreeSet::new();
    let empty_strict = TrustStorePolicy::StrictSpkiPinSet {
        pins: empty,
        include_system_roots: true,
        allow_enterprise_bypass: false,
    };

    for platform in [
        PlatformTarget::Windows,
        PlatformTarget::MacOs,
        PlatformTarget::Linux,
        PlatformTarget::Android,
        PlatformTarget::Ios,
    ] {
        assert!(
            validate_trust_policy(
                platform,
                TrustProfile::Release,
                &TrustStorePolicy::OsSystemRoots
            )
            .is_ok()
        );
        assert!(validate_trust_policy(platform, TrustProfile::Release, &strict).is_ok());
        assert_eq!(
            validate_trust_policy(
                platform,
                TrustProfile::Release,
                &TrustStorePolicy::BrowserManagedTls
            )
            .unwrap_err()
            .code,
            ErrorCode::Unsupported
        );
        assert_eq!(
            validate_trust_policy(platform, TrustProfile::Release, &empty_strict)
                .unwrap_err()
                .code,
            ErrorCode::InvalidInput
        );
    }

    assert_eq!(
        validate_trust_policy(PlatformTarget::Web, TrustProfile::Release, &strict)
            .unwrap_err()
            .code,
        ErrorCode::Unsupported
    );
    assert!(
        validate_trust_policy(
            PlatformTarget::Web,
            TrustProfile::Release,
            &TrustStorePolicy::BrowserManagedTls
        )
        .is_ok()
    );

    let user_roots = TrustStorePolicy::OsPlusEnterpriseRoots {
        allow_user_installed_roots: true,
    };
    for mobile in [PlatformTarget::Android, PlatformTarget::Ios] {
        assert_eq!(
            validate_trust_policy(mobile, TrustProfile::Release, &user_roots)
                .unwrap_err()
                .code,
            ErrorCode::InvalidInput
        );
        assert!(validate_trust_policy(mobile, TrustProfile::DebugOverride, &user_roots).is_ok());
    }
    for desktop in [
        PlatformTarget::Windows,
        PlatformTarget::MacOs,
        PlatformTarget::Linux,
    ] {
        assert!(validate_trust_policy(desktop, TrustProfile::Release, &user_roots).is_ok());
    }
}

#[test]
fn certificate_failure_precedence_is_exact() {
    let policy = TrustStorePolicy::OsSystemRoots;
    assert_eq!(
        evaluate_tls_trust(&CertificateChainObservation::empty(), &policy),
        TrustEvaluation::Failed(TlsFailureReason::UntrustedRoot)
    );

    let revoked = CertificateChainObservation::from_root(ObservedRootKind::System)
        .with_revoked(true)
        .with_expired_or_not_yet_valid(true)
        .with_hostname_matches(false);
    assert_eq!(
        evaluate_tls_trust(&revoked, &policy),
        TrustEvaluation::Failed(TlsFailureReason::Revoked)
    );

    let expired = CertificateChainObservation::from_root(ObservedRootKind::System)
        .with_expired_or_not_yet_valid(true)
        .with_hostname_matches(false);
    assert_eq!(
        evaluate_tls_trust(&expired, &policy),
        TrustEvaluation::Failed(TlsFailureReason::ExpiredOrNotYetValid)
    );

    let mismatch = CertificateChainObservation::from_root(ObservedRootKind::System)
        .with_hostname_matches(false);
    assert_eq!(
        evaluate_tls_trust(&mismatch, &policy),
        TrustEvaluation::Failed(TlsFailureReason::HostnameMismatch)
    );
}

#[test]
fn system_and_enterprise_root_policies_are_distinct() {
    let system = CertificateChainObservation::from_root(ObservedRootKind::System);
    let enterprise = CertificateChainObservation::from_root(ObservedRootKind::Enterprise);
    let user = CertificateChainObservation::from_root(ObservedRootKind::UserInstalled);
    let unknown = CertificateChainObservation::from_root(ObservedRootKind::Unknown);

    let os = TrustStorePolicy::OsSystemRoots;
    assert_eq!(
        evaluate_tls_trust(&system, &os),
        TrustEvaluation::Verified(VerifiedAnchorKind::SystemAnchor)
    );
    for chain in [&enterprise, &user, &unknown] {
        assert_eq!(
            evaluate_tls_trust(chain, &os),
            TrustEvaluation::Failed(TlsFailureReason::UntrustedRoot)
        );
    }

    let strict_enterprise = TrustStorePolicy::OsPlusEnterpriseRoots {
        allow_user_installed_roots: false,
    };
    assert_eq!(
        evaluate_tls_trust(&system, &strict_enterprise),
        TrustEvaluation::Verified(VerifiedAnchorKind::SystemAnchor)
    );
    assert_eq!(
        evaluate_tls_trust(&enterprise, &strict_enterprise),
        TrustEvaluation::Verified(VerifiedAnchorKind::EnterpriseAnchor)
    );
    assert_eq!(
        evaluate_tls_trust(&user, &strict_enterprise),
        TrustEvaluation::Failed(TlsFailureReason::UntrustedRoot)
    );
    assert_eq!(
        evaluate_tls_trust(&unknown, &strict_enterprise),
        TrustEvaluation::Failed(TlsFailureReason::UntrustedRoot)
    );

    let permissive = TrustStorePolicy::OsPlusEnterpriseRoots {
        allow_user_installed_roots: true,
    };
    assert_eq!(
        evaluate_tls_trust(&user, &permissive),
        TrustEvaluation::Verified(VerifiedAnchorKind::EnterpriseAnchor)
    );
    assert_eq!(
        evaluate_tls_trust(&unknown, &permissive),
        TrustEvaluation::Failed(TlsFailureReason::UntrustedRoot)
    );
}

#[test]
fn strict_spki_pins_honor_system_roots_and_enterprise_bypass() {
    let pinned = pin(7);
    let other = pin(9);
    let matching = CertificateChainObservation::from_root(ObservedRootKind::System)
        .with_certificate(
            CertificateObservation::new(ObservedRootKind::System).with_spki_sha256(pinned),
        );
    let not_matching = CertificateChainObservation::from_root(ObservedRootKind::System)
        .with_certificate(
            CertificateObservation::new(ObservedRootKind::System).with_spki_sha256(other),
        );

    let include = TrustStorePolicy::strict_pins([pinned], true, false).unwrap();
    assert_eq!(
        evaluate_tls_trust(&matching, &include),
        TrustEvaluation::Verified(VerifiedAnchorKind::SystemAnchor)
    );
    assert_eq!(
        evaluate_tls_trust(&not_matching, &include),
        TrustEvaluation::Failed(TlsFailureReason::PinMismatch)
    );

    let enterprise = CertificateChainObservation::from_root(ObservedRootKind::Enterprise)
        .with_certificate(
            CertificateObservation::new(ObservedRootKind::Enterprise).with_spki_sha256(other),
        );
    let bypass = TrustStorePolicy::strict_pins([pinned], true, true).unwrap();
    assert_eq!(
        evaluate_tls_trust(&enterprise, &bypass),
        TrustEvaluation::Verified(VerifiedAnchorKind::EnterpriseAnchor)
    );

    let no_bypass = TrustStorePolicy::strict_pins([pinned], true, false).unwrap();
    assert_eq!(
        evaluate_tls_trust(&enterprise, &no_bypass),
        TrustEvaluation::Failed(TlsFailureReason::PinMismatch)
    );

    let enterprise_matching = CertificateChainObservation::from_root(ObservedRootKind::Enterprise)
        .with_certificate(
            CertificateObservation::new(ObservedRootKind::Enterprise).with_spki_sha256(pinned),
        );
    assert_eq!(
        evaluate_tls_trust(&enterprise_matching, &no_bypass),
        TrustEvaluation::Verified(VerifiedAnchorKind::EnterpriseAnchor)
    );

    let pin_only = TrustStorePolicy::strict_pins([pinned], false, false).unwrap();
    assert_eq!(
        evaluate_tls_trust(&matching, &pin_only),
        TrustEvaluation::Verified(VerifiedAnchorKind::PinnedDirectAnchor)
    );
    assert_eq!(
        evaluate_tls_trust(&not_matching, &pin_only),
        TrustEvaluation::Failed(TlsFailureReason::PinMismatch)
    );

    let pin_only_enterprise_bypass = TrustStorePolicy::strict_pins([pinned], false, true).unwrap();
    assert_eq!(
        evaluate_tls_trust(&enterprise, &pin_only_enterprise_bypass),
        TrustEvaluation::Verified(VerifiedAnchorKind::EnterpriseAnchor)
    );

    let empty = TrustStorePolicy::StrictSpkiPinSet {
        pins: BTreeSet::new(),
        include_system_roots: false,
        allow_enterprise_bypass: true,
    };
    assert_eq!(
        evaluate_tls_trust(&matching, &empty),
        TrustEvaluation::Failed(TlsFailureReason::PinMismatch)
    );

    for root in [ObservedRootKind::UserInstalled, ObservedRootKind::Unknown] {
        let chain = CertificateChainObservation::from_root(root)
            .with_certificate(CertificateObservation::new(root).with_spki_sha256(pinned));
        assert_eq!(
            evaluate_tls_trust(&chain, &include),
            TrustEvaluation::Failed(TlsFailureReason::UntrustedRoot)
        );
    }
}

#[test]
fn browser_managed_tls_is_explicit_and_service_uses_current_policy() {
    let browser = CertificateChainObservation::from_root(ObservedRootKind::Unknown);
    assert_eq!(
        evaluate_tls_trust(&browser, &TrustStorePolicy::BrowserManagedTls),
        TrustEvaluation::Verified(VerifiedAnchorKind::BrowserManaged)
    );
    assert_eq!(
        evaluate_tls_trust(
            &browser.clone().with_browser_verified(false),
            &TrustStorePolicy::BrowserManagedTls
        ),
        TrustEvaluation::Failed(TlsFailureReason::UntrustedRoot)
    );

    let provider = std::sync::Arc::new(FakeNetworkProvider::with_snapshot(routable(
        TransportKind::WiFi,
    )));
    let service = NetworkService::with_policies(
        provider,
        PlatformTarget::Web,
        TrustProfile::Release,
        ProxyConfig::BrowserOpaque,
        TrustStorePolicy::BrowserManagedTls,
    )
    .expect("web service");
    assert_eq!(
        service.evaluate_tls_trust(&browser),
        TrustEvaluation::Verified(VerifiedAnchorKind::BrowserManaged)
    );

    let native = linux_service(FakeNetworkProvider::new());
    assert_eq!(
        native.evaluate_tls_trust(&CertificateChainObservation::from_root(
            ObservedRootKind::System
        )),
        TrustEvaluation::Verified(VerifiedAnchorKind::SystemAnchor)
    );
    assert_eq!(
        native
            .set_trust_policy(TrustStorePolicy::BrowserManagedTls)
            .unwrap_err()
            .code,
        ErrorCode::Unsupported
    );
}
