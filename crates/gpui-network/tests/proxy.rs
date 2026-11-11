mod support;

use gpui_network::*;
use gpui_system::ErrorCode;
use std::sync::Arc;
use support::*;

#[test]
fn loopback_bypass_covers_names_and_loopback_addresses() {
    let pattern = BypassPattern::loopback();
    for value in [
        "http://localhost",
        "http://api.localhost:8080",
        "http://127.0.0.1",
        "http://127.255.255.254:9000",
        "http://[::1]:443",
    ] {
        assert!(pattern.matches(&origin(value)), "{value}");
    }
    for value in [
        "https://example.com",
        "https://notlocalhost.example",
        "http://10.0.0.1",
        "http://[2001:db8::1]",
    ] {
        assert!(!pattern.matches(&origin(value)), "{value}");
    }
}

#[test]
fn cidr_bypass_handles_prefix_zero_and_boundaries_without_panicking() {
    let any_v4 = BypassPattern::ipv4_cidr("0.0.0.0".parse().unwrap(), 0).unwrap();
    assert!(any_v4.matches(&origin("http://203.0.113.10")));
    assert!(!any_v4.matches(&origin("http://[2001:db8::1]")));

    let ten_eight = BypassPattern::ipv4_cidr("10.0.0.0".parse().unwrap(), 8).unwrap();
    assert!(ten_eight.matches(&origin("http://10.1.2.3")));
    assert!(ten_eight.matches(&origin("http://10.255.255.255")));
    assert!(!ten_eight.matches(&origin("http://11.0.0.1")));

    let exact = BypassPattern::ipv4_cidr("192.168.1.7".parse().unwrap(), 32).unwrap();
    assert!(exact.matches(&origin("http://192.168.1.7")));
    assert!(!exact.matches(&origin("http://192.168.1.8")));

    assert_eq!(
        BypassPattern::ipv4_cidr("0.0.0.0".parse().unwrap(), 33)
            .unwrap_err()
            .code,
        ErrorCode::InvalidInput
    );
    let malformed = BypassPattern::Ipv4Cidr {
        network: "10.0.0.0".parse().unwrap(),
        prefix_len: 33,
    };
    assert!(!malformed.matches(&origin("http://10.0.0.1")));

    let any_v6 = BypassPattern::ipv6_cidr("::".parse().unwrap(), 0).unwrap();
    assert!(any_v6.matches(&origin("http://[2001:db8::1]")));
    assert!(!any_v6.matches(&origin("http://10.0.0.1")));

    let doc = BypassPattern::ipv6_cidr("2001:db8::".parse().unwrap(), 32).unwrap();
    assert!(doc.matches(&origin("http://[2001:db8:ffff::1]")));
    assert!(!doc.matches(&origin("http://[2001:db9::1]")));

    let exact6 = BypassPattern::ipv6_cidr("2001:db8::1".parse().unwrap(), 128).unwrap();
    assert!(exact6.matches(&origin("http://[2001:db8::1]")));
    assert!(!exact6.matches(&origin("http://[2001:db8::2]")));

    assert_eq!(
        BypassPattern::ipv6_cidr("::".parse().unwrap(), 129)
            .unwrap_err()
            .code,
        ErrorCode::InvalidInput
    );
}

#[test]
fn domain_suffix_matching_never_matches_a_suffix_inside_another_label() {
    let wildcard = BypassPattern::domain_suffix("*.corp.example").unwrap();
    assert!(wildcard.matches(&origin("https://api.corp.example")));
    assert!(wildcard.matches(&origin("https://a.b.corp.example")));
    assert!(!wildcard.matches(&origin("https://badcorp.example")));
    assert!(!wildcard.matches(&origin("https://corp.example")));
    assert!(!wildcard.matches(&origin("https://corp.example.evil")));

    let bare = BypassPattern::domain_suffix("Corp.Example").unwrap();
    assert!(bare.matches(&origin("https://corp.example")));
    assert!(bare.matches(&origin("https://api.corp.example")));
    assert!(!bare.matches(&origin("https://badcorp.example")));
    assert!(!bare.matches(&origin("https://[2001:db8::1]")));

    for invalid in ["", "*.", "bad/suffix", "bad suffix", "bad?query"] {
        assert!(
            BypassPattern::domain_suffix(invalid).is_err(),
            "{invalid:?}"
        );
    }
}

#[test]
fn exact_host_and_host_port_are_normalized() {
    let host = BypassPattern::exact_host("API.Example.COM").unwrap();
    assert!(host.matches(&origin("https://api.example.com")));
    assert!(host.matches(&origin("https://api.example.com:8443")));
    assert!(!host.matches(&origin("https://other.example.com")));

    let with_port = BypassPattern::exact_host_port("api.example.com", 8443).unwrap();
    assert!(with_port.matches(&origin("https://api.example.com:8443")));
    assert!(!with_port.matches(&origin("https://api.example.com")));

    let ipv6 = BypassPattern::exact_host("2001:db8::1").unwrap();
    assert!(ipv6.matches(&origin("https://[2001:db8::1]")));
    assert!(!ipv6.matches(&origin("https://[2001:db8::2]")));

    assert!(BypassPattern::exact_host_port("example.com", 0).is_err());
}

#[test]
fn static_proxy_rules_preserve_order_and_target_scheme_filters() {
    let proxy_a = origin("http://proxy-a.example:8080");
    let proxy_b = origin("https://proxy-b.example:8443");
    let socks = origin("https://proxy-c.example:1080");
    let config = ProxyConfig::static_rules(
        vec![
            ProxyRule::new(ProxyScheme::Http, proxy_a.clone()),
            ProxyRule::new(ProxyScheme::Https, proxy_b.clone())
                .for_target_scheme(OriginScheme::Https),
            ProxyRule::new(ProxyScheme::Socks5, socks.clone()),
        ],
        Vec::new(),
    );

    let https = config.resolve_proxies(&origin("https://api.example"));
    assert_eq!(https.len(), 3);
    assert_eq!(
        https[0],
        ProxyCandidate::Http {
            proxy: proxy_a.clone()
        }
    );
    assert_eq!(
        https[1],
        ProxyCandidate::Https {
            proxy: proxy_b.clone()
        }
    );
    assert_eq!(
        https[2],
        ProxyCandidate::Socks5 {
            proxy: socks.clone()
        }
    );

    let http = config.resolve_proxies(&origin("http://api.example"));
    assert_eq!(http.len(), 2);
    assert_eq!(http[0], ProxyCandidate::Http { proxy: proxy_a });
    assert_eq!(http[1], ProxyCandidate::Socks5 { proxy: socks });

    let empty = ProxyConfig::static_rules(Vec::new(), Vec::new());
    assert_eq!(
        empty.resolve_proxies(&origin("https://example.com")),
        vec![ProxyCandidate::Direct]
    );
}

#[test]
fn bypass_rules_short_circuit_to_direct() {
    let config = ProxyConfig::static_rules(
        vec![ProxyRule::new(
            ProxyScheme::Http,
            origin("http://proxy.example:8080"),
        )],
        vec![
            BypassPattern::loopback(),
            BypassPattern::exact_host("localhost").unwrap(),
            BypassPattern::domain_suffix("*.internal.example").unwrap(),
        ],
    );
    for value in [
        "http://localhost:3000",
        "http://127.0.0.1",
        "https://api.internal.example",
    ] {
        assert_eq!(
            config.resolve_proxies(&origin(value)),
            vec![ProxyCandidate::Direct],
            "{value}"
        );
    }
    assert_eq!(
        config.resolve_proxies(&origin("https://api.example")),
        vec![ProxyCandidate::Http {
            proxy: origin("http://proxy.example:8080")
        }]
    );
}

#[test]
fn pac_script_ids_are_opaque_bounded_ascii_identifiers() {
    let pac =
        ProxyConfig::pac_url(origin("https://config.example"), "corp-proxy-v2").expect("valid pac");
    assert_eq!(
        pac,
        ProxyConfig::PacUrl {
            script_origin: origin("https://config.example"),
            script_id: "corp-proxy-v2".to_owned()
        }
    );
    for invalid in ["", "token?x=1", "a/b", "a b", "ümlaut", &"a".repeat(129)] {
        assert!(ProxyConfig::pac_url(origin("https://config.example"), invalid).is_err());
    }
    assert!(ProxyConfig::Direct.validate().is_ok());
    assert!(ProxyConfig::SystemDynamic.validate().is_ok());
    assert!(ProxyConfig::BrowserOpaque.validate().is_ok());
}

#[test]
fn service_resolves_static_rules_and_delegates_dynamic_policy() {
    let (fake, service) = fake_service();
    let config = ProxyConfig::static_rules(
        vec![ProxyRule::new(
            ProxyScheme::Socks5,
            origin("https://proxy.example:1080"),
        )],
        vec![BypassPattern::loopback()],
    );
    service.set_proxy_config(config).expect("install rules");

    let proxied = completed(service.resolve_proxies(&origin("https://api.example"), &context()));
    assert_eq!(
        proxied,
        vec![ProxyCandidate::Socks5 {
            proxy: origin("https://proxy.example:1080")
        }]
    );
    assert_eq!(fake.resolve_call_count(), 0);

    let direct = completed(service.resolve_proxies(&origin("http://localhost:3000"), &context()));
    assert_eq!(direct, vec![ProxyCandidate::Direct]);

    service
        .set_proxy_config(ProxyConfig::SystemDynamic)
        .expect("dynamic policy");
    let dynamic = completed(service.resolve_proxies(&origin("https://api.example"), &context()));
    assert_eq!(dynamic, vec![ProxyCandidate::Direct]);
    assert_eq!(fake.resolve_call_count(), 1);
}

#[test]
fn browser_owned_proxy_fails_closed_on_web() {
    let provider = Arc::new(FakeNetworkProvider::with_snapshot(routable(
        TransportKind::WiFi,
    )));
    let service = NetworkService::with_policies(
        provider.clone(),
        PlatformTarget::Web,
        TrustProfile::Release,
        ProxyConfig::BrowserOpaque,
        TrustStorePolicy::BrowserManagedTls,
    )
    .expect("browser policy");
    assert_eq!(service.platform(), PlatformTarget::Web);
    assert_eq!(
        service
            .resolve_proxies(&origin("https://example.com"), &context())
            .unwrap_err()
            .code,
        ErrorCode::Unsupported
    );
    assert_eq!(provider.resolve_call_count(), 0);

    let error = NetworkService::with_policies(
        provider.clone(),
        PlatformTarget::Web,
        TrustProfile::Release,
        ProxyConfig::Direct,
        TrustStorePolicy::BrowserManagedTls,
    )
    .expect_err("browser policy must be opaque");
    assert_eq!(error.code, ErrorCode::Unsupported);

    let native = NetworkService::with_platform(provider, PlatformTarget::Linux);
    let error = native
        .set_proxy_config(ProxyConfig::BrowserOpaque)
        .expect_err("browser opaque is web-only");
    assert_eq!(error.code, ErrorCode::Unsupported);
}

#[test]
fn cancelled_and_expired_contexts_never_call_the_provider() {
    let (fake, service) = fake_service();
    service
        .set_proxy_config(ProxyConfig::SystemDynamic)
        .expect("dynamic");
    let cancellation = CancellationToken::default();
    cancellation.cancel(CancelReason::Caller);
    let context = OperationContext {
        cancellation,
        ..OperationContext::default()
    };
    assert!(matches!(
        service
            .resolve_proxies(&origin("https://example.com"), &context)
            .expect("cancelled is not an error"),
        Completion::Cancelled(CancelReason::Caller)
    ));
    assert_eq!(fake.resolve_call_count(), 0);

    let context = OperationContext {
        deadline: Some(std::time::Instant::now()),
        ..OperationContext::default()
    };
    let error = service
        .resolve_proxies(&origin("https://example.com"), &context)
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::TimedOut);
}

#[test]
fn dynamic_proxy_invalidation_reconnects_while_static_policies_are_noops() {
    let (fake, service) = fake_service();
    service.set_app_phase(AppPhase::ForegroundActive);
    let client = completed(service.register_client(ClientOwnerScope::Application, &context()));
    let subscription = service.subscribe();

    let pac = ProxyConfig::pac_url(origin("https://pac.example"), "corp").expect("PAC config");
    for config in [ProxyConfig::SystemDynamic, pac] {
        service
            .set_proxy_config(config.clone())
            .expect("dynamic proxy config");
        let _ = client.drain_directives();
        let _ = drain(&subscription);

        let update = service.invalidate_proxy_configuration();
        assert!(update.changed);
        assert_eq!(update.previous_epoch.next(), update.epoch);
        assert_eq!(
            update.directives,
            vec![
                RevalidationDirective::DropAllPoolsAndReconnect,
                RevalidationDirective::RevalidateStaleQueries,
            ]
        );
        let scoped = client.drain_directives();
        assert_eq!(scoped.len(), 2);
        assert_eq!(
            scoped[0].directive,
            RevalidationDirective::DropAllPoolsAndReconnect
        );
        assert_eq!(
            scoped[1].directive,
            RevalidationDirective::RevalidateStaleQueries
        );
        assert!(
            scoped
                .iter()
                .all(|directive| directive.epoch == update.epoch)
        );
        let events = values(drain(&subscription));
        assert!(events.contains(&NetworkEvent::ProxyConfigChanged {
            config: config.clone()
        }));
        assert!(events.contains(&NetworkEvent::PolicyEpochAdvanced {
            epoch: update.epoch
        }));
        assert_eq!(
            events
                .iter()
                .filter(|event| matches!(
                    event,
                    NetworkEvent::RevalidationDirective { epoch, .. } if *epoch == update.epoch
                ))
                .count(),
            2
        );

        // Provider-originated invalidation takes the same path.
        let before = service.policy_epoch();
        fake.invalidate_proxy_configuration();
        let epoch = service.policy_epoch();
        assert_eq!(epoch, before.next());
        let scoped = client.drain_directives();
        assert_eq!(scoped.len(), 2);
        assert!(scoped.iter().all(|directive| directive.epoch == epoch));
        let events = values(drain(&subscription));
        assert!(events.contains(&NetworkEvent::ProxyConfigChanged {
            config: config.clone()
        }));
        assert!(events.contains(&NetworkEvent::PolicyEpochAdvanced { epoch }));
    }

    for config in [
        ProxyConfig::Direct,
        ProxyConfig::static_rules(Vec::new(), Vec::new()),
    ] {
        service
            .set_proxy_config(config)
            .expect("explicit proxy config");
        let _ = client.drain_directives();
        let _ = drain(&subscription);

        let before = service.policy_epoch();
        let update = service.invalidate_proxy_configuration();
        assert!(!update.changed);
        assert_eq!(update.previous_epoch, before);
        assert_eq!(update.epoch, before);
        assert!(update.directives.is_empty());

        fake.invalidate_proxy_configuration();
        assert_eq!(service.policy_epoch(), before);
        assert!(client.drain_directives().is_empty());
        assert!(drain(&subscription).is_empty());
    }

    // A provider credential rotation still uses the coordinated epoch path.
    service
        .set_proxy_config(ProxyConfig::SystemDynamic)
        .expect("dynamic proxy config");
    let _ = client.drain_directives();
    let _ = drain(&subscription);
    let before = service.policy_epoch();
    fake.rotate_proxy_credentials();
    let epoch = service.policy_epoch();
    assert_eq!(epoch, before.next());
    let scoped = client.drain_directives();
    assert!(
        scoped
            .iter()
            .any(|directive| { directive.directive == RevalidationDirective::ReauthenticateProxy })
    );
    assert!(
        scoped.iter().any(|directive| {
            directive.directive == RevalidationDirective::RecycleIdleConnections
        })
    );
    let events = values(drain(&subscription));
    assert!(events.contains(&NetworkEvent::ProxyCredentialsRotated { scope: None }));
    assert!(events.contains(&NetworkEvent::PolicyEpochAdvanced { epoch }));
}
