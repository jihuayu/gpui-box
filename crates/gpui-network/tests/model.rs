mod support;

use gpui_network::*;
use gpui_system::ErrorCode;
use support::*;

#[test]
fn endpoints_are_strict_ascii_origins_with_default_ports() {
    let https = origin("HTTPS://Example.COM");
    assert_eq!(https.scheme(), OriginScheme::Https);
    assert_eq!(https.host(), "example.com");
    assert_eq!(https.port(), 443);
    assert_eq!(https.to_string(), "https://example.com");

    assert_eq!(origin("http://example.com").port(), 80);
    assert_eq!(origin("ws://example.com").port(), 80);
    assert_eq!(origin("wss://example.com").port(), 443);
    assert_eq!(origin("https://example.com:8443").port(), 8443);
    assert_eq!(
        origin("https://example.com:8443").to_string(),
        "https://example.com:8443"
    );

    let ipv4 = origin("http://127.0.0.1:8080");
    assert_eq!(ipv4.host(), "127.0.0.1");
    assert_eq!(ipv4.ipv4(), Some("127.0.0.1".parse().unwrap()));

    let ipv6 = origin("https://[2001:DB8::1]:443");
    assert_eq!(ipv6.host(), "[2001:db8::1]");
    assert_eq!(ipv6.port(), 443);
    assert_eq!(ipv6.ipv6(), Some("2001:db8::1".parse().unwrap()));
    assert_eq!(ipv6.origin_string(), "https://[2001:db8::1]:443");
}

#[test]
fn endpoints_reject_paths_queries_fragments_userinfo_and_invalid_parts() {
    for value in [
        "https://example.com/",
        "https://example.com/foo",
        "https://example.com?token=secret",
        "https://example.com#frag",
        "https://user:pass@example.com",
        "https://example.com:0",
        "https://example.com:",
        "https://",
        "https:///path",
        "https://example.com\\path",
        "https://exa mple.com",
        "https://example.com\n",
        "https://-bad.example.com",
        "https://bad-.example.com",
        "https://bad..example.com",
        "https://example.com.",
        "https://2001:db8::1",
        "ftp://example.com",
        "example.com",
    ] {
        let error = EndpointOrigin::parse(value).expect_err(value);
        assert_eq!(error.code, ErrorCode::InvalidInput, "{value}");
    }

    let mut long_label = "a".repeat(64);
    long_label.push_str(".example");
    assert!(EndpointOrigin::parse(&format!("https://{long_label}")).is_err());

    let mut long_host = String::new();
    for _ in 0..5 {
        long_host.push_str("abcdefghij.");
    }
    assert!(EndpointOrigin::parse(&format!("https://{long_host}")).is_err());
}

#[test]
fn endpoint_constructors_validate_and_normalize() {
    assert!(matches!(
        InterfaceReachability::routable(TransportKind::WiFi, false, false, false),
        Err(error) if error.code == ErrorCode::InvalidInput
    ));
    let reachability =
        InterfaceReachability::routable(TransportKind::Cellular, true, true, false).unwrap();
    assert!(reachability.is_routable());
    assert!(reachability.is_vpn());
    assert!(reachability.ipv4());
    assert!(!reachability.ipv6());

    let invalid = InterfaceReachability::Routable {
        primary_transport: TransportKind::Other,
        is_vpn: false,
        ipv4: false,
        ipv6: false,
    };
    assert!(invalid.validate().is_err());
    assert!(
        NetworkSnapshot::new(
            invalid,
            CaptivePortalStatus::Clear,
            ConnectionCost::Unmetered,
            DataSaverMode::Off,
            SignalSource::Observed,
        )
        .is_err()
    );

    let snapshot = NetworkSnapshot::routable(TransportKind::Ethernet, false, true, true)
        .unwrap()
        .with_source(SignalSource::Inferred);
    assert_eq!(snapshot.source, SignalSource::Inferred);
    assert!(snapshot.is_routable());
}

#[test]
fn captive_portal_helpers_never_retain_query_tokens_or_userinfo() {
    let detected =
        CaptivePortalStatus::detect_from_url("https://portal.example/login?token=canary#wifi")
            .expect("sanitized portal");
    let portal = detected.portal_origin().expect("origin");
    assert_eq!(portal.to_string(), "https://portal.example");
    assert_eq!(portal.port(), 443);
    assert!(!format!("{detected:?}").contains("canary"));
    assert!(!format!("{detected:?}").contains("login"));
    assert!(!format!("{detected:?}").contains("wifi"));

    let error =
        CaptivePortalStatus::detect_from_url("https://user:pass@portal.example/login").unwrap_err();
    assert_eq!(error.code, ErrorCode::InvalidInput);
    let path_only = CaptivePortalStatus::detect_from_url("/login?token=canary").unwrap_err();
    assert_eq!(path_only.code, ErrorCode::InvalidInput);

    assert!(CaptivePortalStatus::Clear.is_clear());
    assert!(CaptivePortalStatus::Suspected.is_suspected());
    assert!(CaptivePortalStatus::detected(None).is_detected());
    assert!(CaptivePortalStatus::detected(None).blocks_workloads());
}

#[test]
fn workload_budget_matrix_is_exact_and_deterministic() {
    use NetworkWorkloadClass::*;
    use WorkloadBudgetDecision::*;

    let offline = NetworkSnapshot::unknown(SignalSource::Observed);
    for workload in NetworkWorkloadClass::ALL {
        assert_eq!(
            offline.evaluate_workload_budget(workload),
            BlockWhileOfflineOrCaptive
        );
    }

    let local_link =
        routable(TransportKind::WiFi).with_reachability(InterfaceReachability::LocalLinkOnly);
    for workload in NetworkWorkloadClass::ALL {
        assert_eq!(
            local_link.evaluate_workload_budget(workload),
            BlockWhileOfflineOrCaptive
        );
    }

    let detected = routable(TransportKind::WiFi).with_captive_portal(
        CaptivePortalStatus::detect_from_url("https://portal.example/login").unwrap(),
    );
    for workload in NetworkWorkloadClass::ALL {
        assert_eq!(
            detected.evaluate_workload_budget(workload),
            BlockWhileOfflineOrCaptive
        );
    }

    let suspected =
        routable(TransportKind::WiFi).with_captive_portal(CaptivePortalStatus::Suspected);
    assert_eq!(suspected.evaluate_workload_budget(Interactive), Allow);
    for workload in [Revalidation, Prefetch, BackgroundBulk] {
        assert_eq!(
            suspected.evaluate_workload_budget(workload),
            BlockWhileOfflineOrCaptive
        );
    }

    let constrained = routable(TransportKind::WiFi).with_data_saver(DataSaverMode::Constrained);
    assert_eq!(constrained.evaluate_workload_budget(Interactive), Allow);
    for workload in [Revalidation, Prefetch, BackgroundBulk] {
        assert_eq!(
            constrained.evaluate_workload_budget(workload),
            DeferUntilUnmetered
        );
    }

    let roaming = routable(TransportKind::Cellular).with_cost(ConnectionCost::CellularRoaming);
    assert_eq!(roaming.evaluate_workload_budget(Interactive), Allow);
    for workload in [Revalidation, Prefetch, BackgroundBulk] {
        assert_eq!(
            roaming.evaluate_workload_budget(workload),
            DeferUntilUnmetered
        );
    }

    let metered = routable(TransportKind::Cellular).with_cost(ConnectionCost::Metered);
    assert_eq!(metered.evaluate_workload_budget(Interactive), Allow);
    assert_eq!(metered.evaluate_workload_budget(Revalidation), Allow);
    assert_eq!(
        metered.evaluate_workload_budget(Prefetch),
        DeferUntilUnmetered
    );
    assert_eq!(
        metered.evaluate_workload_budget(BackgroundBulk),
        DeferUntilUnmetered
    );

    let unmetered = routable(TransportKind::Ethernet);
    for workload in NetworkWorkloadClass::ALL {
        assert_eq!(unmetered.evaluate_workload_budget(workload), Allow);
    }

    let unknown_portal =
        routable(TransportKind::Ethernet).with_captive_portal(CaptivePortalStatus::Unknown);
    for workload in NetworkWorkloadClass::ALL {
        assert_eq!(unknown_portal.evaluate_workload_budget(workload), Allow);
    }

    let invalid_routable =
        routable(TransportKind::Ethernet).with_reachability(InterfaceReachability::Routable {
            primary_transport: TransportKind::Ethernet,
            is_vpn: false,
            ipv4: false,
            ipv6: false,
        });
    assert_eq!(
        invalid_routable.evaluate_workload_budget(Interactive),
        BlockWhileOfflineOrCaptive
    );
}

#[test]
fn snapshots_compare_semantically_and_round_trip_through_serde() {
    let first = routable(TransportKind::Ethernet);
    let same = routable(TransportKind::Ethernet)
        .with_source(SignalSource::Inferred)
        .with_revision(99);
    assert!(first.semantic_eq(&same));
    assert_ne!(first, same);

    let changed = same.with_cost(ConnectionCost::Metered);
    assert!(!first.semantic_eq(&changed));

    let json = serde_json::to_string(&changed).expect("snapshot json");
    let decoded: NetworkSnapshot = serde_json::from_str(&json).expect("snapshot decode");
    assert_eq!(decoded, changed);

    let origin_json = serde_json::to_string(&origin("https://example.com:8443")).unwrap();
    assert_eq!(origin_json, "\"https://example.com:8443\"");
    let decoded_origin: EndpointOrigin = serde_json::from_str(&origin_json).unwrap();
    assert_eq!(decoded_origin.port(), 8443);

    assert!(serde_json::from_str::<EndpointOrigin>("\"https://example.com/path\"").is_err());
}

#[test]
fn diagnostic_realm_is_bounded_and_control_free() {
    assert_eq!(
        EndpointDiagnostic::proxy_auth_required(None).unwrap(),
        EndpointDiagnostic::ProxyAuthRequired { realm: None }
    );
    assert_eq!(
        EndpointDiagnostic::proxy_auth_required(Some("  corporate proxy  ")).unwrap(),
        EndpointDiagnostic::ProxyAuthRequired {
            realm: Some("corporate proxy".to_owned())
        }
    );
    assert_eq!(
        EndpointDiagnostic::proxy_auth_required(Some("   ")).unwrap(),
        EndpointDiagnostic::ProxyAuthRequired { realm: None }
    );
    assert_eq!(
        EndpointDiagnostic::proxy_auth_required(Some("bad\nrealm"))
            .unwrap_err()
            .code,
        ErrorCode::InvalidInput
    );
    assert_eq!(
        EndpointDiagnostic::proxy_auth_required(Some(&"a".repeat(129)))
            .unwrap_err()
            .code,
        ErrorCode::InvalidInput
    );

    assert!(
        EndpointDiagnostic::DnsFailure {
            kind: DnsFailureKind::Timeout
        }
        .is_retriable()
    );
    assert!(EndpointDiagnostic::EndpointHttpUnavailable { status_code: 429 }.is_retriable());
    assert!(!EndpointDiagnostic::EndpointHttpUnavailable { status_code: 404 }.is_retriable());
    assert!(
        !EndpointDiagnostic::TlsVerificationFailed {
            reason: TlsFailureReason::PinMismatch
        }
        .is_retriable()
    );
}
