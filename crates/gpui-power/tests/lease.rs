mod support;

use gpui_power::*;
use gpui_system::{Completion, ErrorCode, SurfaceId};
use support::*;

#[test]
fn wake_reason_is_bounded_public_safe_and_validated() {
    assert_eq!(
        WakeReason::new("index rebuild").expect("valid").as_str(),
        "index rebuild"
    );
    for invalid in [
        String::new(),
        "   ".to_owned(),
        "line\nbreak".to_owned(),
        "tab\tseparated".to_owned(),
        "null\0byte".to_owned(),
        "a".repeat(MAX_WAKE_REASON_BYTES + 1),
    ] {
        let error = WakeReason::new(invalid).expect_err("invalid reason");
        assert_eq!(error.code, ErrorCode::InvalidInput);
    }
    let boundary = "a".repeat(MAX_WAKE_REASON_BYTES);
    assert_eq!(
        WakeReason::new(boundary).expect("boundary length").len(),
        MAX_WAKE_REASON_BYTES
    );
}

#[test]
fn wake_reason_deserialization_reuses_validation() {
    assert!(serde_json::from_str::<WakeReason>("\"\"").is_err());
    assert!(serde_json::from_str::<WakeReason>("\"   \"").is_err());
    assert!(serde_json::from_str::<WakeReason>("\"bad\\nreason\"").is_err());
    assert!(serde_json::from_str::<WakeReason>(&format!("\"{}\"", "a".repeat(129))).is_err());
    let reason: WakeReason = serde_json::from_str("\"download shelf\"").expect("valid reason");
    assert_eq!(reason.as_str(), "download shelf");
    assert_eq!(
        serde_json::to_string(&reason).expect("serialize"),
        "\"download shelf\""
    );
}

#[test]
fn release_policy_defaults_match_the_lease_kind() {
    let system = ReleasePolicy::for_kind(WakeInhibitionKind::SystemIdleSleep);
    assert!(!system.revoke_on_background);
    assert!(!system.revoke_on_session_lock);
    assert!(system.revoke_on_system_sleep);
    assert!(!system.revoke_on_low_power_mode);

    let display = ReleasePolicy::for_kind(WakeInhibitionKind::DisplayAndSystem);
    assert!(display.revoke_on_background);
    assert!(display.revoke_on_session_lock);
    assert!(display.revoke_on_system_sleep);
    assert!(!display.revoke_on_low_power_mode);

    let customized = system
        .with_revoke_on_background(true)
        .with_revoke_on_session_lock(true)
        .with_revoke_on_system_sleep(false)
        .with_revoke_on_low_power_mode(true);
    assert!(customized.revoke_on_background);
    assert!(customized.revoke_on_session_lock);
    assert!(!customized.revoke_on_system_sleep);
    assert!(customized.revoke_on_low_power_mode);
    assert!(customized.revokes_on_background_for(WakeInhibitionKind::SystemIdleSleep));
    assert!(display.revokes_on_session_lock_for(WakeInhibitionKind::SystemIdleSleep));
    assert_eq!(ReleasePolicy::default(), system);
}

#[test]
fn request_constructors_keep_scope_and_policy_explicit() {
    let system = WakeLeaseRequest::system_idle(reason("system work"));
    assert_eq!(system.kind, WakeInhibitionKind::SystemIdleSleep);
    assert!(matches!(system.scope, WakeLeaseScope::Application { .. }));
    assert_eq!(system.policy(), ReleasePolicy::for_kind(system.kind));

    let display = WakeLeaseRequest::display_and_system(reason("visible playback")).with_scope(
        WakeLeaseScope::surface(
            SurfaceId(4),
            2,
            ReleasePolicy::for_kind(WakeInhibitionKind::DisplayAndSystem),
        ),
    );
    assert_eq!(display.scope.surface_id(), Some(SurfaceId(4)));
    assert_eq!(display.scope.generation(), Some(2));
    assert!(display.scope.is_surface_scoped());

    let policy = ReleasePolicy::for_kind(WakeInhibitionKind::SystemIdleSleep)
        .with_revoke_on_background(true);
    let customized = display.clone().with_policy(policy);
    assert_eq!(customized.policy(), policy);
    assert_eq!(customized.scope.surface_id(), Some(SurfaceId(4)));
    assert_eq!(
        display.scope,
        WakeLeaseScope::surface(
            SurfaceId(4),
            2,
            ReleasePolicy::for_kind(WakeInhibitionKind::DisplayAndSystem)
        )
    );
}

#[test]
fn effective_status_aggregates_and_drops_immediately() {
    let (fake, service) = fake_service();
    let first = completed(
        &service,
        WakeLeaseRequest::system_idle(reason("first system lease")),
    )
    .expect("first");
    assert!(first.is_effective());
    assert_eq!(first.state(), WakeLeaseState::Effective);
    assert_eq!(first.revocation_reason(), None);
    let status = service.effective_wake_status();
    assert!(status.inhibits_system_sleep);
    assert!(!status.inhibits_display_sleep);
    assert_eq!(status.active_lease_count, 1);
    assert_eq!(status.system_lease_count, 1);
    assert_eq!(status.display_lease_count, 0);

    let second = completed(
        &service,
        WakeLeaseRequest::system_idle(reason("second system lease")),
    )
    .expect("second");
    let display = completed(
        &service,
        WakeLeaseRequest::display_and_system(reason("display lease")),
    )
    .expect("display");
    let status = service.effective_wake_status();
    assert_eq!(status.active_lease_count, 3);
    assert_eq!(status.system_lease_count, 2);
    assert_eq!(status.display_lease_count, 1);
    assert!(status.inhibits_display_sleep);

    assert!(first.release());
    assert_eq!(first.state(), WakeLeaseState::Released);
    let status = service.effective_wake_status();
    assert_eq!(status.active_lease_count, 2);
    assert_eq!(status.system_lease_count, 1);
    assert_eq!(status.display_lease_count, 1);

    drop(second);
    drop(display);
    let status = service.effective_wake_status();
    assert!(!status.inhibits_system_sleep);
    assert!(!status.inhibits_display_sleep);
    assert_eq!(status.active_lease_count, 0);
    assert_eq!(status.system_lease_count, 0);
    assert_eq!(status.display_lease_count, 0);
    assert!(fake.active_native_tokens().is_empty());
    assert_eq!(fake.release_call_count(), 3);
}

#[test]
fn repeated_release_and_drop_release_the_native_token_at_most_once() {
    let (fake, service) = fake_service();
    let lease = completed(
        &service,
        WakeLeaseRequest::system_idle(reason("idempotent lease")),
    )
    .expect("lease");
    let token = fake.active_native_tokens()[0];
    assert!(lease.release());
    assert!(!lease.release());
    assert!(!lease.release());
    assert_eq!(lease.state(), WakeLeaseState::Released);
    assert_eq!(fake.released_native_tokens(), vec![token]);
    assert_eq!(fake.release_call_count(), 1);
    assert!(fake.active_native_tokens().is_empty());
    assert_eq!(service.active_lease_count(), 0);
    drop(lease);
    assert_eq!(fake.release_call_count(), 1);
    assert_eq!(fake.released_native_tokens(), vec![token]);
}

#[test]
fn drop_releases_an_effective_lease_exactly_once() {
    let (fake, service) = fake_service();
    let token = {
        let lease = completed(
            &service,
            WakeLeaseRequest::display_and_system(reason("drop lease")),
        )
        .expect("lease");
        assert!(lease.is_effective());
        fake.active_native_tokens()[0]
    };
    assert_eq!(service.active_lease_count(), 0);
    assert_eq!(fake.released_native_tokens(), vec![token]);
    assert_eq!(fake.release_call_count(), 1);
}

#[test]
fn lease_ids_are_unique_and_provider_tokens_are_monotonic() {
    let (fake, service) = fake_service();
    let mut ids = Vec::new();
    let mut tokens = Vec::new();
    let mut leases = Vec::new();
    for index in 0..4 {
        let lease = completed(
            &service,
            WakeLeaseRequest::system_idle(reason(format!("lease {index}"))),
        )
        .expect("lease");
        ids.push(lease.id());
        tokens.push(fake.active_native_tokens()[index]);
        leases.push(lease);
    }
    let mut unique = ids.clone();
    unique.sort_unstable();
    unique.dedup();
    assert_eq!(unique.len(), ids.len());
    assert!(tokens.windows(2).all(|pair| pair[0] < pair[1]));
    assert_eq!(fake.acquire_call_count(), 4);
    assert_eq!(service.active_lease_count(), 4);
    drop(leases);
    assert_eq!(fake.release_call_count(), 4);
    assert!(fake.active_native_tokens().is_empty());
}

#[test]
fn lease_debug_reports_identity_without_paths() {
    let (_fake, service) = fake_service();
    let lease = completed(
        &service,
        WakeLeaseRequest::system_idle(reason("debug lease")),
    )
    .expect("lease");
    let debug = format!("{lease:?}");
    assert!(debug.contains("WakePreventionLease"));
    assert!(debug.contains("SystemIdleSleep"));
    assert!(!debug.contains('/'));
    assert_eq!(service.effective_wake_status().active_lease_count, 1);
}

#[test]
fn cancelled_acquisition_returns_completion_and_never_touches_the_provider() {
    let (fake, service) = fake_service();
    let context = context();
    context.cancellation.cancel(gpui_system::CancelReason::User);
    let result = service
        .acquire_wake_lease(WakeLeaseRequest::system_idle(reason("cancelled")), &context)
        .expect("cancellation is not an error");
    assert!(matches!(
        result,
        Completion::Cancelled(gpui_system::CancelReason::User)
    ));
    assert_eq!(fake.acquire_call_count(), 0);
    assert_eq!(service.active_lease_count(), 0);
}
