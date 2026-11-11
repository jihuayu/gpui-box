//! Facade and integration tests for the network feature.
//!
//! These tests are deterministic, use no sleeps and run without a native
//! provider. The QueryClient integration proves that scoped revalidation
//! directives drive real cache invalidation.

#![cfg(feature = "network")]

use gpui_box::network::*;
use std::sync::Arc;

fn context() -> OperationContext {
    OperationContext::default()
}

fn completed<T>(result: ServiceResult<T>) -> T {
    match result.expect("operation succeeded") {
        Completion::Completed(value) => value,
        Completion::Cancelled(reason) => panic!("unexpected cancellation: {reason:?}"),
    }
}

fn origin(value: &str) -> EndpointOrigin {
    EndpointOrigin::parse(value).expect("valid origin")
}

fn routable(transport: TransportKind) -> NetworkSnapshot {
    NetworkSnapshot::routable(transport, false, true, true).expect("routable snapshot")
}

fn linux_service(fake: FakeNetworkProvider) -> NetworkService {
    NetworkService::with_platform(Arc::new(fake), PlatformTarget::Linux)
}

#[test]
fn network_reexport_exposes_coordinated_contracts_and_matrix() {
    assert_eq!(
        gpui_box::network::UPSTREAM_GPUI_REVISION,
        gpui_box::UPSTREAM_REVISION
    );
    assert_eq!(gpui_box::network::PACKAGE_VERSION, gpui_box::VERSION);
    assert_eq!(NetworkOperation::ALL.len(), 6);
    assert_eq!(PlatformTarget::ALL.len(), 6);
    assert_eq!(platform_support_matrix().len(), 36);
    assert_eq!(DEFAULT_NETWORK_EVENT_CAPACITY, 64);
    assert_eq!(MAX_PROXY_AUTH_ATTEMPTS, 2);
    assert_eq!(
        operation_support(PlatformTarget::Web, NetworkOperation::ResolveSystemProxy).support,
        Support::Unsupported
    );
    let policy = web_capability_policy(WebBackendMaturity::Experimental);
    assert_eq!(policy.observe_connectivity, Support::Conditional);
    assert!(policy.browser_managed_tls_only);
}

#[test]
fn network_reexport_resolves_proxy_and_workload_policy() {
    let fake = FakeNetworkProvider::with_snapshot(routable(TransportKind::Ethernet));
    let service = linux_service(fake);
    let config = ProxyConfig::static_rules(
        vec![ProxyRule::new(
            ProxyScheme::Http,
            origin("http://proxy.example:8080"),
        )],
        vec![BypassPattern::loopback()],
    );
    service.set_proxy_config(config).expect("proxy config");
    let candidates = completed(service.resolve_proxies(&origin("https://api.example"), &context()));
    assert_eq!(
        candidates,
        vec![ProxyCandidate::Http {
            proxy: origin("http://proxy.example:8080")
        }]
    );
    assert_eq!(
        service.evaluate_workload_budget(NetworkWorkloadClass::Prefetch),
        WorkloadBudgetDecision::Allow
    );
    assert_eq!(
        service.evaluate_workload_budget(NetworkWorkloadClass::Interactive),
        WorkloadBudgetDecision::Allow
    );

    service
        .apply_snapshot(routable(TransportKind::Cellular).with_cost(ConnectionCost::Metered))
        .expect("metered");
    assert_eq!(
        service.evaluate_workload_budget(NetworkWorkloadClass::Prefetch),
        WorkloadBudgetDecision::DeferUntilUnmetered
    );
    assert_eq!(
        service.evaluate_workload_budget(NetworkWorkloadClass::Revalidation),
        WorkloadBudgetDecision::Allow
    );
}

#[test]
fn network_reexport_evaluates_fail_closed_trust_policy() {
    let service = linux_service(FakeNetworkProvider::new());
    let chain = CertificateChainObservation::from_root(ObservedRootKind::Enterprise);
    assert_eq!(
        service.evaluate_tls_trust(&chain),
        TrustEvaluation::Failed(TlsFailureReason::UntrustedRoot)
    );
    let pin = SpkiPinSha256::from_bytes([5; 32]);
    service
        .set_trust_policy(TrustStorePolicy::strict_pins([pin], true, true).unwrap())
        .expect("pin policy");
    assert_eq!(
        service.evaluate_tls_trust(&chain),
        TrustEvaluation::Verified(VerifiedAnchorKind::EnterpriseAnchor)
    );
    assert_eq!(
        service
            .set_trust_policy(TrustStorePolicy::BrowserManagedTls)
            .unwrap_err()
            .code,
        ErrorCode::Unsupported
    );
}

#[cfg(any(target_os = "windows", target_os = "macos", target_os = "linux"))]
#[test]
fn staged_revalidation_drives_query_client_invalidation_and_account_teardown() {
    use gpui_box_components::query::QueryClient;

    let fake = FakeNetworkProvider::with_snapshot(routable(TransportKind::Ethernet));
    let service = linux_service(fake);
    service.set_app_phase(AppPhase::ForegroundActive);
    let client = completed(service.register_client(ClientOwnerScope::Application, &context()));

    let mut queries = QueryClient::<&'static str, i32>::default();
    let request = queries.start("profile");
    assert!(queries.complete(&"profile", &request, Ok(7)));
    assert_eq!(queries.state(&"profile").expect("state").value, Some(7));
    drop(request);

    // Observe the same scoped stream independently of the lease queue. The
    // callback cannot capture QueryClient (it is Rc-backed and not Send), so it
    // records directives and the test applies them to QueryClient below.
    let observed = Arc::new(std::sync::Mutex::new(Vec::<ScopedRevalidation>::new()));
    let observer = {
        let observed = Arc::clone(&observed);
        service.on_revalidation(RevalidationScopeFilter::Application, move |scoped| {
            observed.lock().expect("observer lock").push(*scoped);
        })
    };
    assert!(observer.is_active());
    let take_observed = || {
        let mut guard = observed.lock().expect("observer lock");
        std::mem::take(&mut *guard)
    };

    // A background transition defers stale-query revalidation but delivers pool
    // hygiene immediately through both the lease queue and the observer.
    service.set_app_phase(AppPhase::Background);
    service
        .apply_snapshot(routable(TransportKind::WiFi))
        .expect("background transition");
    let directives = client.drain_directives();
    let observed_directives = take_observed();
    assert!(directives.iter().any(|directive| {
        directive.directive == RevalidationDirective::DropAllPoolsAndReconnect
    }));
    assert!(observed_directives.iter().any(|directive| {
        directive.directive == RevalidationDirective::DropAllPoolsAndReconnect
    }));
    assert!(
        directives.iter().all(|directive| {
            directive.directive != RevalidationDirective::RevalidateStaleQueries
        })
    );
    assert!(
        observed_directives.iter().all(|directive| {
            directive.directive != RevalidationDirective::RevalidateStaleQueries
        })
    );
    for directive in &observed_directives {
        if directive.directive == RevalidationDirective::DropAllPoolsAndReconnect {
            queries.clear();
        }
    }
    assert!(queries.state(&"profile").is_none());

    let request = queries.start("profile");
    assert!(queries.complete(&"profile", &request, Ok(9)));
    drop(request);
    let before = queries.state(&"profile").expect("state").revision();

    // Resume flushes exactly one coalesced stale-query directive to the lease
    // queue and to the observer; apply the observed directive to the cache.
    service.set_app_phase(AppPhase::ForegroundActive);
    let directives = client.drain_directives();
    let observed_directives = take_observed();
    assert_eq!(
        directives
            .iter()
            .filter(|directive| {
                directive.directive == RevalidationDirective::RevalidateStaleQueries
            })
            .count(),
        1
    );
    assert_eq!(
        observed_directives
            .iter()
            .filter(|directive| {
                directive.directive == RevalidationDirective::RevalidateStaleQueries
            })
            .count(),
        1
    );
    for directive in &observed_directives {
        if directive.directive == RevalidationDirective::RevalidateStaleQueries {
            queries.invalidate(&"profile");
        }
    }
    assert!(queries.state(&"profile").expect("state").revision() > before);

    // Account teardown revokes the lease and clears the cache explicitly.
    let account_key = AccountScopeKey::new("account@example.com").expect("account");
    let account_client =
        completed(service.register_client(ClientOwnerScope::account(account_key), &context()));
    assert_eq!(service.revoke_account_clients(&account_key), 1);
    assert_eq!(
        account_client.revocation_reason(),
        Some(ClientRevocationReason::AccountCleared)
    );
    queries.clear();
    assert!(queries.state(&"profile").is_none());
}

#[test]
fn lease_drop_and_generation_disposal_are_safe_through_the_facade() {
    let fake = FakeNetworkProvider::with_snapshot(routable(TransportKind::Ethernet));
    let service = linux_service(fake);
    let surface = SurfaceId(31);
    let first =
        completed(service.register_client(ClientOwnerScope::surface(surface, 1), &context()));
    let second =
        completed(service.register_client(ClientOwnerScope::surface(surface, 2), &context()));
    assert_eq!(service.dispose_surface_generation(surface, 1), 1);
    assert_eq!(
        first.revocation_reason(),
        Some(ClientRevocationReason::SurfaceDisposed)
    );
    assert!(second.is_active());
    drop(second);
    assert_eq!(service.active_client_count(), 0);
}

#[cfg(feature = "network-gpui")]
#[test]
fn network_gpui_feature_exposes_adapter_and_entity_model_symbols() {
    fn assert_adapter_symbols() {
        let _init = gpui_box::network::gpui_adapter::init_network;
        let _lookup = gpui_box::network::gpui_adapter::network_service;
        let _install = gpui_box::network::gpui_adapter::install_network_service;
        let _scope = gpui_box::network::gpui_adapter::NetworkSurfaceScope::new;
        let _entity = gpui_box::network::gpui_adapter::NetworkEntityModel::new;
    }
    assert_adapter_symbols();
    assert_eq!(
        gpui_box::network::gpui_adapter::NetworkEntityModel::MAX_RETAINED_EVENTS,
        64
    );

    let service = linux_service(FakeNetworkProvider::new());
    let mut services = gpui_box::system::SystemServices::new();
    gpui_box::network::gpui_adapter::install_network_in_services(&mut services, service.clone())
        .expect("registration");
    let registered = services
        .service::<NetworkService>()
        .expect("registered service");
    assert!(service.shares_state_with(&registered));
}
