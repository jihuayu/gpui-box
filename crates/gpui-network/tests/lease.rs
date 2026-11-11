mod support;

use gpui_network::*;
use gpui_system::{ErrorCode, SurfaceId};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use support::*;

fn lease(service: &NetworkService, scope: ClientOwnerScope) -> NetworkClientLease {
    completed(service.register_client(scope, &context()))
}

#[test]
fn clients_acknowledge_release_and_drop_idempotently() {
    let (_, service) = fake_service();
    let subscription = service.subscribe();
    let client = lease(&service, ClientOwnerScope::Application);
    let id = client.client_id();
    assert!(client.is_active());
    assert_eq!(client.observed_epoch(), service.policy_epoch());
    assert!(client.acknowledge_epoch(service.policy_epoch().next()));
    assert_eq!(client.observed_epoch(), service.policy_epoch().next());
    assert!(!client.acknowledge_epoch(service.policy_epoch()));

    assert_eq!(service.active_client_count(), 1);
    assert!(client.release());
    assert!(!client.release());
    assert!(!client.is_active());
    assert_eq!(
        client.revocation_reason(),
        Some(ClientRevocationReason::Released)
    );
    drop(client);
    assert_eq!(service.active_client_count(), 0);

    let events = values(drain(&subscription));
    assert!(events.iter().any(|event| matches!(
        event,
        NetworkEvent::ClientRegistered { client_id, .. } if *client_id == id
    )));
    assert_eq!(
        events
            .iter()
            .filter(|event| matches!(
                event,
                NetworkEvent::ClientRevoked {
                    client_id,
                    reason: ClientRevocationReason::Released,
                    ..
                } if *client_id == id
            ))
            .count(),
        1
    );
}

#[test]
fn dropping_a_lease_releases_it_automatically() {
    let (_, service) = fake_service();
    let client = lease(&service, ClientOwnerScope::surface(SurfaceId(1), 1));
    assert_eq!(service.active_client_count(), 1);
    drop(client);
    assert_eq!(service.active_client_count(), 0);
}

#[test]
fn surface_generation_disposal_is_exact() {
    let (_, service) = fake_service();
    let surface = SurfaceId(9);
    let first = lease(&service, ClientOwnerScope::surface(surface, 1));
    let second = lease(&service, ClientOwnerScope::surface(surface, 2));

    assert_eq!(service.dispose_surface_generation(surface, 1), 1);
    assert!(!first.is_active());
    assert_eq!(
        first.revocation_reason(),
        Some(ClientRevocationReason::SurfaceDisposed)
    );
    assert!(second.is_active());
    assert_eq!(service.dispose_surface_generation(surface, 1), 0);

    assert_eq!(service.dispose_surface(surface), 1);
    assert!(!second.is_active());
    assert_eq!(
        second.revocation_reason(),
        Some(ClientRevocationReason::SurfaceDisposed)
    );
}

#[test]
fn stale_generation_revocation_leaves_the_current_generation_live() {
    let (_, service) = fake_service();
    let surface = SurfaceId(10);
    let stale = lease(&service, ClientOwnerScope::surface(surface, 3));
    let current = lease(&service, ClientOwnerScope::surface(surface, 4));
    assert_eq!(service.revoke_stale_generations(surface, 4), 1);
    assert_eq!(
        stale.revocation_reason(),
        Some(ClientRevocationReason::StaleGeneration)
    );
    assert!(current.is_active());
}

#[test]
fn account_scope_keys_and_events_never_expose_the_raw_account() {
    let (_, service) = fake_service();
    let raw = "canary-account@example.com";
    let account = AccountScopeKey::new(raw).expect("account key");
    let scope = ClientOwnerScope::account(account);
    let client = lease(&service, scope);
    assert!(!format!("{account:?}").contains(raw));
    assert!(!format!("{account}").contains(raw));
    assert!(!format!("{scope:?}").contains(raw));
    assert!(!format!("{client:?}").contains(raw));
    let json = serde_json::to_string(&account).unwrap();
    assert!(!json.contains(raw));
    assert!(json.contains(&account.fingerprint().to_string()));

    let subscription = service.subscribe();
    service
        .apply_snapshot(routable(TransportKind::WiFi).with_data_saver(DataSaverMode::Constrained))
        .unwrap();
    let events = values(drain(&subscription));
    for event in events {
        assert!(!format!("{event:?}").contains(raw));
    }

    let other = AccountScopeKey::new("other@example.com").unwrap();
    assert_eq!(service.revoke_account_clients(&other), 0);
    assert!(client.is_active());
    assert_eq!(service.revoke_account_clients(&account), 1);
    assert!(!client.is_active());
    assert_eq!(
        client.revocation_reason(),
        Some(ClientRevocationReason::AccountCleared)
    );
    assert_eq!(
        AccountScopeKey::new("").unwrap_err().code,
        ErrorCode::InvalidInput
    );
    assert_eq!(
        AccountScopeKey::new("bad\naccount").unwrap_err().code,
        ErrorCode::InvalidInput
    );
}

#[test]
fn shutdown_revokes_every_live_client() {
    let (_, service) = fake_service();
    let application = lease(&service, ClientOwnerScope::Application);
    let account = lease(
        &service,
        ClientOwnerScope::account(AccountScopeKey::new("shutdown@example.com").unwrap()),
    );
    assert_eq!(service.shutdown(), 2);
    assert_eq!(
        application.revocation_reason(),
        Some(ClientRevocationReason::ServiceShutdown)
    );
    assert_eq!(
        account.revocation_reason(),
        Some(ClientRevocationReason::ServiceShutdown)
    );
    assert_eq!(service.shutdown(), 0);
    let error = service
        .register_client(ClientOwnerScope::Application, &context())
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::Unavailable);
}

#[test]
fn revalidation_callbacks_run_outside_internal_locks_and_can_reenter() {
    let (_, service) = fake_service();
    let callbacks = Arc::new(AtomicUsize::new(0));
    let counter = callbacks.clone();
    let reentrant = service.clone();
    let observer = service.on_revalidation(RevalidationScopeFilter::All, move |_| {
        counter.fetch_add(1, Ordering::AcqRel);
        let _ = reentrant.current_snapshot();
        let _ = reentrant.evaluate_workload_budget(NetworkWorkloadClass::Interactive);
        let _ = reentrant.resolve_proxies(&origin("https://example.com"), &context());
        let _ = reentrant.register_client(ClientOwnerScope::Application, &context());
    });
    let client = lease(&service, ClientOwnerScope::Application);
    service
        .apply_snapshot(routable(TransportKind::WiFi))
        .expect("transition");
    assert!(callbacks.load(Ordering::Acquire) >= 2);
    assert!(observer.is_active());
    assert!(client.is_active());
    let was_active = observer.is_active();
    drop(observer);
    assert!(was_active);
}

#[test]
fn background_phases_defer_and_coalesce_stale_query_revalidation() {
    let (_, service) = fake_service();
    service.set_app_phase(AppPhase::ForegroundActive);
    let client = lease(&service, ClientOwnerScope::Application);
    service.set_app_phase(AppPhase::Background);

    service
        .apply_snapshot(routable(TransportKind::WiFi))
        .expect("first transition");
    service
        .apply_snapshot(routable(TransportKind::Cellular))
        .expect("second transition");
    service
        .apply_snapshot(routable(TransportKind::Ethernet))
        .expect("third transition");
    let directives = client.drain_directives();
    assert!(
        directives
            .iter()
            .all(|directive| directive.directive != RevalidationDirective::RevalidateStaleQueries)
    );
    assert!(client.has_pending_stale_revalidation());
    assert!(service.pending_stale_revalidation());

    service.set_app_phase(AppPhase::ForegroundActive);
    let directives = client.drain_directives();
    assert_eq!(
        directives
            .iter()
            .filter(|directive| {
                directive.directive == RevalidationDirective::RevalidateStaleQueries
            })
            .count(),
        1
    );
    assert!(!service.pending_stale_revalidation());
    assert!(!client.has_pending_stale_revalidation());
}

#[test]
fn a_client_registered_during_a_background_transition_gets_one_flush_on_resume() {
    let (_, service) = fake_service();
    service.set_app_phase(AppPhase::ForegroundActive);
    service.set_app_phase(AppPhase::Suspended);
    service
        .apply_snapshot(routable(TransportKind::WiFi))
        .expect("background transition");
    assert!(service.pending_stale_revalidation());
    let client = lease(&service, ClientOwnerScope::Application);
    service.set_app_phase(AppPhase::ForegroundActive);
    let directives = client.drain_directives();
    assert_eq!(
        directives
            .iter()
            .filter(|directive| {
                directive.directive == RevalidationDirective::RevalidateStaleQueries
            })
            .count(),
        1
    );
}

#[test]
fn lifecycle_snapshots_and_terminating_phases_are_applied() {
    let (_, service) = fake_service();
    let lifecycle = LifecycleState::from_parts(
        AppPhase::ForegroundActive,
        std::iter::empty(),
        gpui_lifecycle::SessionLockState::Unlocked,
        gpui_lifecycle::MemoryPressure::Normal,
    );
    service.apply_lifecycle_state(&lifecycle);
    assert_eq!(service.app_phase(), AppPhase::ForegroundActive);
    let lifecycle = lifecycle.with_phase(AppPhase::Background);
    service.apply_lifecycle_state(&lifecycle);
    assert_eq!(service.app_phase(), AppPhase::Background);
}

#[test]
fn semantically_identical_policies_do_not_advance_the_epoch() {
    let (_, service) = fake_service();
    service.set_app_phase(AppPhase::ForegroundActive);
    let client = lease(&service, ClientOwnerScope::Application);
    let epoch = service.policy_epoch();

    let update = service
        .apply_snapshot(routable(TransportKind::Ethernet).with_source(SignalSource::Inferred))
        .expect("same semantics");
    assert!(!update.changed);
    assert_eq!(update.epoch, epoch);
    assert!(update.directives.is_empty());
    assert!(client.drain_directives().is_empty());

    let update = service
        .set_proxy_config(ProxyConfig::Direct)
        .expect("same proxy");
    assert!(!update.changed);
    assert_eq!(service.policy_epoch(), epoch);

    let update = service
        .set_trust_policy(TrustStorePolicy::OsSystemRoots)
        .expect("same trust");
    assert!(!update.changed);
    assert_eq!(service.policy_epoch(), epoch);
}

#[test]
fn policy_changes_advance_epochs_and_emit_typed_directives() {
    let (_, service) = fake_service();
    service.set_app_phase(AppPhase::ForegroundActive);
    let client = lease(&service, ClientOwnerScope::Application);

    let update = service
        .set_proxy_config(ProxyConfig::static_rules(
            vec![ProxyRule::new(
                ProxyScheme::Http,
                origin("http://proxy.example:8080"),
            )],
            Vec::new(),
        ))
        .expect("proxy change");
    assert!(update.changed);
    assert!(update.epoch > update.previous_epoch);
    assert!(
        update
            .directives
            .contains(&RevalidationDirective::DropAllPoolsAndReconnect)
    );
    assert!(client.drain_directives().iter().any(|directive| {
        directive.directive == RevalidationDirective::DropAllPoolsAndReconnect
    }));
    assert_eq!(service.policy_epoch(), update.epoch);

    let pin = SpkiPinSha256::from_bytes([3; 32]);
    let update = service
        .set_trust_policy(TrustStorePolicy::strict_pins([pin], true, false).unwrap())
        .expect("trust change");
    assert!(update.changed);
    assert!(update.epoch > update.previous_epoch);
    let _ = client.drain_directives();

    let detected = service
        .apply_snapshot(routable(TransportKind::WiFi).with_captive_portal(
            CaptivePortalStatus::detect_from_url("https://portal.example/login").unwrap(),
        ))
        .expect("captive");
    assert!(
        detected
            .directives
            .contains(&RevalidationDirective::AwaitCaptivePortalClear)
    );
    assert!(
        detected
            .directives
            .contains(&RevalidationDirective::ThrottleBackgroundTransfers)
    );
    let directives = client.drain_directives();
    assert!(directives.iter().any(|directive| {
        directive.directive == RevalidationDirective::AwaitCaptivePortalClear
    }));
    assert!(
        directives
            .iter()
            .all(|directive| directive.directive != RevalidationDirective::RevalidateStaleQueries)
    );

    let cleared = service
        .apply_snapshot(routable(TransportKind::WiFi))
        .expect("captive clear");
    assert!(
        cleared
            .directives
            .contains(&RevalidationDirective::RecycleIdleConnections)
    );
    assert!(
        cleared
            .directives
            .contains(&RevalidationDirective::RevalidateStaleQueries)
    );
}

#[test]
fn constrained_cost_and_data_saver_emit_throttle_and_relaxation_revalidates() {
    let (_, service) = fake_service();
    service.set_app_phase(AppPhase::ForegroundActive);
    let client = lease(&service, ClientOwnerScope::Application);

    let metered = service
        .apply_snapshot(routable(TransportKind::Cellular).with_cost(ConnectionCost::Metered))
        .expect("metered");
    assert!(
        metered
            .directives
            .contains(&RevalidationDirective::ThrottleBackgroundTransfers)
    );

    let constrained = service
        .apply_snapshot(
            routable(TransportKind::Cellular)
                .with_cost(ConnectionCost::Metered)
                .with_data_saver(DataSaverMode::Constrained),
        )
        .expect("data saver");
    assert!(
        constrained
            .directives
            .contains(&RevalidationDirective::ThrottleBackgroundTransfers)
    );

    let relaxed = service
        .apply_snapshot(routable(TransportKind::Cellular))
        .expect("relaxed");
    assert!(
        relaxed
            .directives
            .contains(&RevalidationDirective::RevalidateStaleQueries)
    );
    let _ = client.drain_directives();
}

#[test]
fn service_is_send_sync_and_trust_facade_is_available() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<NetworkService>();
    assert_send_sync::<NetworkClientLease>();
    assert_send_sync::<AccountScopeKey>();
}

#[test]
fn surface_client_scopes_require_non_zero_ids_and_generations() {
    let (_, service) = fake_service();
    let subscription = service.subscribe();

    for scope in [
        ClientOwnerScope::surface(SurfaceId(0), 1),
        ClientOwnerScope::surface(SurfaceId(1), 0),
    ] {
        let error = service
            .register_client(scope, &context())
            .expect_err("zero surface ids and generations must be rejected");
        assert_eq!(error.code, ErrorCode::InvalidInput);
    }
    assert_eq!(service.active_client_count(), 0);

    let events = values(drain(&subscription));
    assert_eq!(
        events
            .iter()
            .filter(|event| matches!(
                event,
                NetworkEvent::OperationFailed { operation, code }
                    if *operation == NetworkOperation::SubscribeRevalidation
                        && *code == ErrorCode::InvalidInput
            ))
            .count(),
        2
    );
    assert!(
        events
            .iter()
            .all(|event| !matches!(event, NetworkEvent::ClientRegistered { .. }))
    );

    let valid = lease(&service, ClientOwnerScope::surface(SurfaceId(1), 1));
    assert!(valid.is_active());
}
