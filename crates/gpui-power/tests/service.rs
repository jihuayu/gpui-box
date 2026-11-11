mod support;

use gpui_lifecycle::{AppPhase, LifecycleState, MemoryPressure, SessionLockState as LifecycleLock};
use gpui_permissions::{PermissionKind, PermissionStatus};
use gpui_power::*;
use gpui_system::{
    Availability, CancellationToken, Completion, Configuration, ErrorCode, Event, Support,
    SurfaceId, UiOwner,
};
use std::{
    sync::{Arc, Mutex, Weak},
    time::{Duration, Instant},
};
use support::*;

fn capability_override(
    fake: &FakePowerProvider,
    operation: PowerOperation,
    update: impl FnOnce(&mut Capability<PowerOperation>),
) {
    let mut capability = fake.capability(operation);
    update(&mut capability);
    fake.set_capability(operation, capability);
}

fn assert_no_effective_lease(service: &PowerService, fake: &FakePowerProvider) {
    let status = service.effective_wake_status();
    assert!(!status.inhibits_system_sleep);
    assert!(!status.inhibits_display_sleep);
    assert_eq!(status.active_lease_count, 0);
    assert_eq!(status.system_lease_count, 0);
    assert_eq!(status.display_lease_count, 0);
    assert!(fake.active_native_tokens().is_empty());
}

fn system_request() -> WakeLeaseRequest {
    WakeLeaseRequest::system_idle(reason("system work"))
}

fn display_request() -> WakeLeaseRequest {
    WakeLeaseRequest::display_and_system(reason("display work"))
}

fn surface_request(surface: SurfaceId, generation: u64) -> WakeLeaseRequest {
    WakeLeaseRequest::display_and_system(reason("surface work")).with_scope(
        WakeLeaseScope::surface(
            surface,
            generation,
            ReleasePolicy::for_kind(WakeInhibitionKind::DisplayAndSystem),
        ),
    )
}

#[test]
fn unsupported_capability_never_reports_success() {
    let (fake, service) = fake_service();
    capability_override(&fake, PowerOperation::InhibitSystemSleep, |capability| {
        capability.support = Support::Unsupported;
    });
    let error = service
        .acquire_wake_lease(system_request(), &context())
        .expect_err("unsupported");
    assert_eq!(error.code, ErrorCode::Unsupported);
    assert_eq!(fake.acquire_call_count(), 0);
    assert_no_effective_lease(&service, &fake);
}

#[test]
fn required_configuration_never_reports_success() {
    let (fake, service) = fake_service();
    capability_override(&fake, PowerOperation::InhibitDisplaySleep, |capability| {
        capability.configuration = Configuration::Required;
    });
    let error = service
        .acquire_wake_lease(display_request(), &context())
        .expect_err("configuration required");
    assert_eq!(error.code, ErrorCode::InvalidConfiguration);
    assert_eq!(
        error.recovery,
        Some(gpui_system::RecoveryAction::RepairConfiguration)
    );
    assert_eq!(fake.acquire_call_count(), 0);
    assert_no_effective_lease(&service, &fake);
}

#[test]
fn unavailable_provider_capability_never_reports_success() {
    let (fake, service) = fake_service();
    capability_override(&fake, PowerOperation::InhibitSystemSleep, |capability| {
        capability.availability = Availability::Unavailable;
    });
    let error = service
        .acquire_wake_lease(system_request(), &context())
        .expect_err("unavailable");
    assert_eq!(error.code, ErrorCode::Unavailable);
    assert_eq!(fake.acquire_call_count(), 0);
    assert_no_effective_lease(&service, &fake);
}

#[test]
fn denied_permission_never_reports_success() {
    let (fake, service) = fake_service();
    fake.set_permission_status(PermissionStatus::Denied {
        can_request_again: true,
    });
    let error = service
        .acquire_wake_lease(system_request(), &context())
        .expect_err("denied");
    assert_eq!(error.code, ErrorCode::PermissionDenied);
    assert_eq!(
        error.recovery,
        Some(gpui_system::RecoveryAction::OpenSettings)
    );
    assert_eq!(fake.acquire_call_count(), 0);
    assert_no_effective_lease(&service, &fake);
}

#[test]
fn ineffective_provider_grants_are_typed_and_never_register_a_lease() {
    let cases = [
        (
            WakeInhibitionKind::SystemIdleSleep,
            IneffectiveInhibitionReason::UnsupportedKind,
            ErrorCode::Unsupported,
        ),
        (
            WakeInhibitionKind::SystemIdleSleep,
            IneffectiveInhibitionReason::PermissionDenied,
            ErrorCode::PermissionDenied,
        ),
        (
            WakeInhibitionKind::SystemIdleSleep,
            IneffectiveInhibitionReason::HiddenSurfaceOrDocument,
            ErrorCode::Unavailable,
        ),
        (
            WakeInhibitionKind::DisplayAndSystem,
            IneffectiveInhibitionReason::BatterySaverOverride,
            ErrorCode::Unavailable,
        ),
        (
            WakeInhibitionKind::SystemIdleSleep,
            IneffectiveInhibitionReason::SessionLocked,
            ErrorCode::Unavailable,
        ),
        (
            WakeInhibitionKind::SystemIdleSleep,
            IneffectiveInhibitionReason::SystemSleeping,
            ErrorCode::Unavailable,
        ),
        (
            WakeInhibitionKind::SystemIdleSleep,
            IneffectiveInhibitionReason::AppBackgrounded,
            ErrorCode::Unavailable,
        ),
        (
            WakeInhibitionKind::SystemIdleSleep,
            IneffectiveInhibitionReason::ProviderUnavailable,
            ErrorCode::Unavailable,
        ),
        (
            WakeInhibitionKind::SystemIdleSleep,
            IneffectiveInhibitionReason::ConfigurationRequired,
            ErrorCode::InvalidConfiguration,
        ),
    ];
    for (kind, ineffective, expected) in cases {
        let (fake, service) = fake_service();
        fake.set_inhibition_mode(kind, ineffective);
        let request = WakeLeaseRequest::new(kind, reason("ineffective"));
        let error = service
            .acquire_wake_lease(request, &context())
            .expect_err("ineffective grant must fail");
        assert_eq!(error.code, expected, "reason {ineffective:?}");
        assert_eq!(fake.acquire_call_count(), 1);
        assert_no_effective_lease(&service, &fake);
    }
}

#[test]
fn hidden_browser_document_and_battery_saver_refuse_display_wake_lock() {
    let (fake, service) = fake_service();
    fake.set_hidden_document(true);
    let error = service
        .acquire_wake_lease(display_request(), &context())
        .expect_err("hidden document");
    assert_eq!(error.code, ErrorCode::Unavailable);
    assert_no_effective_lease(&service, &fake);

    fake.set_hidden_document(false);
    fake.set_battery_saver_override(true);
    let error = service
        .acquire_wake_lease(display_request(), &context())
        .expect_err("battery saver");
    assert_eq!(error.code, ErrorCode::Unavailable);
    assert_no_effective_lease(&service, &fake);
}

#[test]
fn system_sleep_session_lock_and_low_power_block_acquisition() {
    let (fake, service) = fake_service();
    fake.set_sleep_resume_state(SleepResumeState::PreparingSleep);
    let error = service
        .acquire_wake_lease(system_request(), &context())
        .expect_err("sleeping");
    assert_eq!(error.code, ErrorCode::Unavailable);
    assert_eq!(fake.acquire_call_count(), 0);
    assert_no_effective_lease(&service, &fake);

    let (fake, service) = fake_service();
    fake.set_session_lock(SessionLockState::Locked);
    let error = service
        .acquire_wake_lease(display_request(), &context())
        .expect_err("locked");
    assert_eq!(error.code, ErrorCode::Unavailable);
    assert_eq!(fake.acquire_call_count(), 0);
    assert_no_effective_lease(&service, &fake);
    // A system-only lease with the default policy may coexist with a lock.
    let lease = completed(&service, system_request()).expect("system lease");
    assert!(lease.is_effective());
    drop(lease);

    let (fake, service) = fake_service();
    fake.set_low_power_mode(LowPowerMode::Enabled);
    let request = system_request().with_policy(
        ReleasePolicy::for_kind(WakeInhibitionKind::SystemIdleSleep)
            .with_revoke_on_low_power_mode(true),
    );
    let error = service
        .acquire_wake_lease(request, &context())
        .expect_err("low power");
    assert_eq!(error.code, ErrorCode::Unavailable);
    assert_eq!(fake.acquire_call_count(), 0);
    assert_no_effective_lease(&service, &fake);
}

#[test]
fn expired_deadline_fails_before_any_provider_side_effect() {
    let (fake, service) = fake_service();
    let mut context = context();
    context.deadline = Some(Instant::now() - Duration::from_millis(1));
    let error = service
        .acquire_wake_lease(system_request(), &context)
        .expect_err("timed out");
    assert_eq!(error.code, ErrorCode::TimedOut);
    assert_eq!(fake.acquire_call_count(), 0);
    assert_no_effective_lease(&service, &fake);
}

#[test]
fn cancelled_context_is_a_completion_not_an_error_or_event() {
    let (fake, service) = fake_service();
    let subscription = service.subscribe();
    let context = context();
    context
        .cancellation
        .cancel(gpui_system::CancelReason::Caller);
    let result = service
        .acquire_wake_lease(system_request(), &context)
        .expect("cancellation");
    assert!(matches!(
        result,
        Completion::Cancelled(gpui_system::CancelReason::Caller)
    ));
    assert_eq!(fake.acquire_call_count(), 0);
    assert_no_effective_lease(&service, &fake);
    assert!(drain(&subscription).is_empty());
}

#[test]
fn failure_emits_operation_failed_with_the_attempted_operation() {
    let (fake, service) = fake_service();
    let subscription = service.subscribe();
    capability_override(&fake, PowerOperation::InhibitDisplaySleep, |capability| {
        capability.support = Support::Unsupported;
    });
    let error = service
        .acquire_wake_lease(display_request(), &context())
        .expect_err("unsupported");
    assert_eq!(error.code, ErrorCode::Unsupported);
    let events = drain(&subscription);
    assert!(events.iter().any(|event| matches!(
        event,
        Event::Value(PowerEvent::OperationFailed {
            operation: PowerOperation::InhibitDisplaySleep,
            code: ErrorCode::Unsupported
        })
    )));
}

#[test]
fn unavailable_contract_provider_is_honest_and_never_effective() {
    let service = PowerService::unavailable();
    let snapshot = service.current_snapshot();
    assert_eq!(snapshot.power_source.kind, PowerSource::Unknown);
    assert_eq!(
        snapshot.power_source.source,
        SignalSource::ProviderUnavailable
    );
    assert_eq!(snapshot.session_lock.state, SessionLockState::Unknown);
    assert_eq!(snapshot.thermal.condition, ThermalCondition::Unknown);
    let error = service
        .snapshot(&context())
        .expect_err("observe unavailable");
    assert_eq!(error.code, ErrorCode::Unavailable);
    let error = service
        .acquire_wake_lease(system_request(), &context())
        .expect_err("inhibit unavailable");
    assert_eq!(error.code, ErrorCode::Unavailable);
}

#[test]
fn accessible_surface_lease_lifecycle_and_generation_expiry() {
    let (fake, service) = fake_service();
    service.attach_surface(SurfaceId(3), 1, true);
    let lease = completed(&service, surface_request(SurfaceId(3), 1)).expect("surface lease");
    assert_eq!(service.active_lease_count(), 1);

    service.advance_surface_generation(SurfaceId(3), 2);
    assert!(!lease.is_effective());
    assert_eq!(
        lease.revocation_reason(),
        Some(WakeRevocationReason::GenerationExpired)
    );
    assert!(fake.active_native_tokens().is_empty());
    assert_eq!(fake.release_call_count(), 1);
    assert_eq!(service.active_lease_count(), 0);
    assert!(!lease.release());

    let error = service
        .acquire_wake_lease(surface_request(SurfaceId(3), 1), &context())
        .expect_err("stale generation");
    assert_eq!(error.code, ErrorCode::ContextExpired);
    let lease = completed(&service, surface_request(SurfaceId(3), 2)).expect("current generation");
    drop(lease);
}

#[test]
fn disposed_surface_revokes_all_of_its_leases_and_rejects_new_ones() {
    let (fake, service) = fake_service();
    service.attach_surface(SurfaceId(8), 1, true);
    let first = completed(&service, surface_request(SurfaceId(8), 1)).expect("first");
    let second = completed(&service, surface_request(SurfaceId(8), 1)).expect("second");
    assert_eq!(service.active_lease_count(), 2);
    service.dispose_surface(SurfaceId(8));
    assert_eq!(service.active_lease_count(), 0);
    assert_eq!(
        first.revocation_reason(),
        Some(WakeRevocationReason::SurfaceDisposed)
    );
    assert_eq!(
        second.revocation_reason(),
        Some(WakeRevocationReason::SurfaceDisposed)
    );
    assert_eq!(fake.release_call_count(), 2);
    assert!(fake.active_native_tokens().is_empty());
    assert!(!service.surface_is_tracked(SurfaceId(8)));

    let error = service
        .acquire_wake_lease(surface_request(SurfaceId(8), 1), &context())
        .expect_err("disposed surface");
    assert_eq!(error.code, ErrorCode::ContextExpired);
}

#[test]
fn attaching_an_invisible_surface_blocks_display_leases() {
    let (fake, service) = fake_service();
    service.attach_surface(SurfaceId(4), 1, false);
    let error = service
        .acquire_wake_lease(surface_request(SurfaceId(4), 1), &context())
        .expect_err("hidden surface");
    assert_eq!(error.code, ErrorCode::Unavailable);
    assert_eq!(fake.acquire_call_count(), 0);
    assert_no_effective_lease(&service, &fake);
}

#[test]
fn session_lock_and_background_lifecycle_revoke_at_the_right_boundary() {
    let (_fake, service) = fake_service();
    let display = completed(&service, display_request()).expect("display");
    let system = completed(&service, system_request()).expect("system");
    service.notify_app_phase(AppPhase::Background);
    assert_eq!(
        display.revocation_reason(),
        Some(WakeRevocationReason::AppBackgrounded)
    );
    assert!(!display.is_effective());
    assert!(
        system.is_effective(),
        "default system lease survives background"
    );
    assert_eq!(service.active_lease_count(), 1);
    system.release();

    let (fake, service) = fake_service();
    let locking = completed(&service, display_request()).expect("display");
    fake.set_session_lock(SessionLockState::Locking);
    assert_eq!(
        locking.revocation_reason(),
        Some(WakeRevocationReason::SessionLocked)
    );
    assert_eq!(service.active_lease_count(), 0);
    assert_eq!(fake.release_call_count(), 1);
    assert!(!locking.release());

    let (fake, service) = fake_service();
    let display = completed(&service, display_request()).expect("display");
    let locked = LifecycleState::from_parts(
        AppPhase::ForegroundActive,
        [],
        LifecycleLock::Locked,
        MemoryPressure::Normal,
    );
    service.notify_lifecycle(&locked);
    assert_eq!(
        display.revocation_reason(),
        Some(WakeRevocationReason::SessionLocked)
    );
    assert_eq!(
        service.current_snapshot().session_lock.state,
        SessionLockState::Locked
    );
    assert_eq!(
        service.current_snapshot().session_lock.source,
        SignalSource::Observed
    );
    assert_eq!(service.active_lease_count(), 0);
    assert_eq!(fake.release_call_count(), 1);
    let error = service
        .acquire_wake_lease(display_request(), &context())
        .expect_err("locked");
    assert_eq!(error.code, ErrorCode::Unavailable);
}

#[test]
fn lifecycle_surface_generation_and_disposal_are_authoritative() {
    let (fake, service) = fake_service();
    let initial = LifecycleState::from_parts(
        AppPhase::ForegroundActive,
        [(SurfaceId(12), visible_snapshot(1))],
        LifecycleLock::Unlocked,
        MemoryPressure::Normal,
    );
    service.notify_lifecycle(&initial);
    assert!(service.surface_is_tracked(SurfaceId(12)));
    let lease = completed(&service, surface_request(SurfaceId(12), 1)).expect("lease");

    let rotated = LifecycleState::from_parts(
        AppPhase::ForegroundActive,
        [(SurfaceId(12), visible_snapshot(2))],
        LifecycleLock::Unlocked,
        MemoryPressure::Normal,
    );
    service.notify_lifecycle(&rotated);
    assert_eq!(
        lease.revocation_reason(),
        Some(WakeRevocationReason::GenerationExpired)
    );
    assert_eq!(fake.release_call_count(), 1);

    let lease = completed(&service, surface_request(SurfaceId(12), 2)).expect("new lease");
    let hidden = LifecycleState::from_parts(
        AppPhase::ForegroundActive,
        [(SurfaceId(12), hidden_snapshot(2))],
        LifecycleLock::Unlocked,
        MemoryPressure::Normal,
    );
    service.notify_lifecycle(&hidden);
    assert_eq!(
        lease.revocation_reason(),
        Some(WakeRevocationReason::AppBackgrounded)
    );

    service.attach_surface(SurfaceId(12), 2, true);
    let lease = completed(&service, surface_request(SurfaceId(12), 2)).expect("third lease");
    let empty = LifecycleState::from_parts(
        AppPhase::ForegroundActive,
        [],
        LifecycleLock::Unlocked,
        MemoryPressure::Normal,
    );
    service.notify_lifecycle(&empty);
    assert_eq!(
        lease.revocation_reason(),
        Some(WakeRevocationReason::SurfaceDisposed)
    );
    assert!(!service.surface_is_tracked(SurfaceId(12)));
}

#[test]
fn lifecycle_memory_pressure_updates_snapshot_and_pacing() {
    let (_fake, service) = fake_service();
    let subscription = service.subscribe();
    let critical = LifecycleState::from_parts(
        AppPhase::ForegroundActive,
        [],
        LifecycleLock::Unlocked,
        MemoryPressure::Critical,
    );
    service.notify_lifecycle(&critical);
    let snapshot = service.current_snapshot();
    assert_eq!(snapshot.memory.condition, MemoryCondition::Critical);
    assert_eq!(snapshot.memory.source, SignalSource::Observed);
    assert_eq!(service.recommend_pacing().tier, PacingTier::Paused);
    assert!(drain(&subscription).iter().any(|event| matches!(
        event,
        Event::Value(PowerEvent::MemoryChanged {
            condition: MemoryCondition::Critical
        })
    )));
}

#[test]
fn os_token_revocation_revokes_exactly_one_lease_without_double_release() {
    let (fake, service) = fake_service();
    let first = completed(&service, system_request()).expect("first");
    let second = completed(&service, system_request()).expect("second");
    let token = fake.active_native_tokens()[0];
    assert!(fake.revoke_token(token, WakeRevocationReason::SystemRevoked));
    assert!(!first.is_effective());
    assert_eq!(
        first.revocation_reason(),
        Some(WakeRevocationReason::SystemRevoked)
    );
    assert!(second.is_effective());
    assert_eq!(service.active_lease_count(), 1);
    assert_eq!(fake.release_call_count(), 0);
    assert!(!first.release());
    drop(first);
    assert_eq!(fake.release_call_count(), 0);
    assert_eq!(service.active_lease_count(), 1);

    let remaining = fake.active_native_tokens()[0];
    assert_eq!(
        fake.revoke_all_tokens(WakeRevocationReason::SystemRevoked),
        1
    );
    assert!(!second.is_effective());
    assert_eq!(
        second.revocation_reason(),
        Some(WakeRevocationReason::SystemRevoked)
    );
    assert_eq!(service.active_lease_count(), 0);
    assert_eq!(fake.release_call_count(), 0);
    assert!(fake.active_native_tokens().is_empty());
    assert_eq!(fake.revoked_native_tokens().len(), 2);
    assert!(
        fake.revoked_native_tokens()
            .iter()
            .any(|(revoked, _)| *revoked == remaining)
    );
}

#[test]
fn permission_revocation_while_active_revokes_every_lease() {
    let (fake, service) = fake_service();
    let first = completed(&service, system_request()).expect("first");
    let second = completed(&service, display_request()).expect("second");
    assert_eq!(service.active_lease_count(), 2);
    fake.set_permission_status(PermissionStatus::Denied {
        can_request_again: false,
    });
    assert_eq!(service.active_lease_count(), 0);
    for lease in [&first, &second] {
        assert!(!lease.is_effective());
        assert_eq!(
            lease.revocation_reason(),
            Some(WakeRevocationReason::PermissionRevoked)
        );
        assert!(!lease.release());
    }
    assert_eq!(fake.release_call_count(), 2);
    assert!(fake.active_native_tokens().is_empty());
    let error = service
        .acquire_wake_lease(system_request(), &context())
        .expect_err("permission revoked");
    assert_eq!(error.code, ErrorCode::PermissionDenied);
}

#[test]
fn provider_snapshot_push_revokes_leases_on_sleep_and_low_power() {
    let (fake, service) = fake_service();
    let system = completed(&service, system_request()).expect("system");
    fake.set_sleep_resume_state(SleepResumeState::Sleeping);
    assert_eq!(
        system.revocation_reason(),
        Some(WakeRevocationReason::SystemSleeping)
    );
    assert_eq!(service.active_lease_count(), 0);
    assert_eq!(fake.release_call_count(), 1);
    assert_eq!(fake.acquire_call_count(), 1);

    let (fake, service) = fake_service();
    let request = system_request().with_policy(
        ReleasePolicy::for_kind(WakeInhibitionKind::SystemIdleSleep)
            .with_revoke_on_low_power_mode(true),
    );
    let lease = completed(&service, request).expect("lease");
    fake.set_low_power_mode(LowPowerMode::Enabled);
    assert_eq!(
        lease.revocation_reason(),
        Some(WakeRevocationReason::LowPowerSaver)
    );
    assert_eq!(service.active_lease_count(), 0);
}

#[test]
fn ui_owner_surface_and_generation_validation() {
    let (_fake, service) = fake_service();
    let owner = UiOwner::new(SurfaceId(5));
    let ui = owner.context(None).expect("ui context");
    service.attach_surface(SurfaceId(5), 1, true);

    let mut request_context = gpui_system::OperationContext {
        ui: Some(ui.clone()),
        ..gpui_system::OperationContext::default()
    };
    let lease = match service
        .acquire_wake_lease(surface_request(SurfaceId(5), 1), &request_context)
        .expect("matching surface")
    {
        Completion::Completed(lease) => lease,
        Completion::Cancelled(reason) => panic!("cancelled: {reason:?}"),
    };
    drop(lease);

    service.attach_surface(SurfaceId(6), 1, true);
    let error = service
        .acquire_wake_lease(surface_request(SurfaceId(6), 1), &request_context)
        .expect_err("surface mismatch");
    assert_eq!(error.code, ErrorCode::InvalidInput);

    owner.advance_generation().expect("advance");
    request_context.ui = Some(owner.context(None).expect("fresh ui"));
    let error = service
        .acquire_wake_lease(surface_request(SurfaceId(5), 1), &request_context)
        .expect_err("stale generation");
    assert_eq!(error.code, ErrorCode::ContextExpired);

    owner.dispose();
    let error = service
        .acquire_wake_lease(surface_request(SurfaceId(5), 1), &request_context)
        .expect_err("disposed owner");
    assert_eq!(error.code, ErrorCode::ContextExpired);
}

#[test]
fn bounded_event_stream_reports_resync_without_blocking_producers() {
    let fake = FakePowerProvider::new();
    fake.set_thermal(ThermalCondition::Nominal);
    fake.set_memory(MemoryCondition::Normal);
    let service = PowerService::with_event_capacity(std::sync::Arc::new(fake), 1);
    let subscription = service.subscribe();
    service.set_budget_policy(PowerBudgetPolicy::new());
    service.set_budget_policy(PowerBudgetPolicy::new().with_low_battery_threshold_percent(5));
    assert!(service.event_revision() >= 2);
    assert_eq!(next_payload(&subscription), Some(Event::ResyncRequired));
    match next_payload(&subscription) {
        Some(Event::Value(PowerEvent::PolicyUpdated { recommendation })) => {
            assert_eq!(recommendation.tier, PacingTier::Normal);
        }
        other => panic!("unexpected event: {other:?}"),
    }
    assert_eq!(next_payload(&subscription), None);
}

#[test]
fn acquire_release_and_revoke_events_are_ordered_and_complete() {
    let (_fake, service) = fake_service();
    let subscription = service.subscribe();
    let lease = completed(&service, system_request()).expect("lease");
    let id = lease.id();
    let events = drain(&subscription);
    assert!(events.iter().any(|event| matches!(
        event,
        Event::Value(PowerEvent::WakeLeaseAcquired {
            id: event_id,
            kind: WakeInhibitionKind::SystemIdleSleep
        }) if *event_id == id
    )));
    assert!(events.iter().any(|event| matches!(
        event,
        Event::Value(PowerEvent::EffectiveWakeStatusChanged { status })
            if status.inhibits_system_sleep && status.active_lease_count == 1
    )));

    assert!(lease.release());
    let events = drain(&subscription);
    assert!(events.iter().any(|event| matches!(
        event,
        Event::Value(PowerEvent::WakeLeaseReleased { id: event_id, .. }) if *event_id == id
    )));
    assert!(events.iter().any(|event| matches!(
        event,
        Event::Value(PowerEvent::EffectiveWakeStatusChanged { status })
            if !status.inhibits_system_sleep && status.active_lease_count == 0
    )));
}

#[test]
fn service_is_installable_and_shares_state() {
    let (_fake, service) = fake_service();
    let clone = service.clone();
    assert!(service.shares_state_with(&clone));
    let mut services = gpui_system::SystemServices::new();
    service.clone().install(&mut services).expect("install");
    let registered = services
        .service::<PowerService>()
        .expect("registered service");
    assert!(service.shares_state_with(&registered));
}

#[test]
fn snapshot_refresh_is_capability_gated_and_updates_the_cache() {
    let (fake, service) = fake_service();
    fake.set_power_source(PowerSource::Battery);
    let snapshot = match service.snapshot(&context()).expect("snapshot") {
        Completion::Completed(snapshot) => snapshot,
        Completion::Cancelled(reason) => panic!("cancelled: {reason:?}"),
    };
    assert_eq!(snapshot.power_source.kind, PowerSource::Battery);
    assert_eq!(service.current_snapshot(), snapshot);

    let (fake, service) = fake_service();
    capability_override(&fake, PowerOperation::ObservePower, |capability| {
        capability.support = Support::Unsupported;
    });
    let error = service
        .snapshot(&context())
        .expect_err("observe unsupported");
    assert_eq!(error.code, ErrorCode::Unsupported);
}

#[test]
fn service_and_lease_handles_are_send_and_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<PowerService>();
    assert_send_sync::<WakePreventionLease>();
    assert_send_sync::<PowerSnapshot>();
    assert_send_sync::<PowerEvent>();
}

#[test]
fn sync_from_provider_refreshes_the_cache_and_returns_the_snapshot() {
    let (fake, service) = fake_service();
    fake.set_power_source(PowerSource::Usb);
    let snapshot = service.sync_from_provider().expect("provider snapshot");
    assert_eq!(snapshot.power_source.kind, PowerSource::Usb);
    assert_eq!(snapshot.power_source.source, SignalSource::Observed);
    assert_eq!(service.current_snapshot(), snapshot);
}

#[test]
fn domain_transition_events_preserve_epoch_and_provenance() {
    let (fake, service) = fake_service();
    let subscription = service.subscribe();
    fake.set_sleep_resume_state(SleepResumeState::PreparingSleep);
    fake.set_sleep_resume_state(SleepResumeState::Sleeping);
    fake.set_sleep_resume_state(SleepResumeState::Resumed);
    fake.set_session_lock(SessionLockState::Locked);
    fake.set_thermal(ThermalCondition::Serious);
    fake.set_memory(MemoryCondition::Moderate);
    let events = drain(&subscription);
    assert!(events.iter().any(|event| matches!(
        event,
        Event::Value(PowerEvent::SleepResumeChanged {
            state: SleepResumeState::Resumed,
            resume_epoch: 1
        })
    )));
    assert!(events.iter().any(|event| matches!(
        event,
        Event::Value(PowerEvent::SessionLockChanged {
            state: SessionLockState::Locked
        })
    )));
    assert!(events.iter().any(|event| matches!(
        event,
        Event::Value(PowerEvent::ThermalChanged {
            condition: ThermalCondition::Serious
        })
    )));
    assert!(events.iter().any(|event| matches!(
        event,
        Event::Value(PowerEvent::MemoryChanged {
            condition: MemoryCondition::Moderate
        })
    )));
    assert!(events.iter().any(|event| matches!(
        event,
        Event::Value(PowerEvent::SnapshotUpdated { snapshot })
            if snapshot.sleep_resume.resume_epoch == 1
                && snapshot.sleep_resume.source == SignalSource::Observed
    )));
}

#[test]
fn hidden_document_and_battery_saver_revoke_active_tokens_from_the_provider() {
    let (fake, service) = fake_service();
    let hidden = completed(&service, display_request()).expect("display");
    fake.set_hidden_document(true);
    assert_eq!(
        hidden.revocation_reason(),
        Some(WakeRevocationReason::AppBackgrounded)
    );
    assert!(!hidden.is_effective());
    assert_eq!(service.active_lease_count(), 0);
    assert_eq!(fake.release_call_count(), 0);
    assert!(fake.active_native_tokens().is_empty());
    assert!(!hidden.release());

    fake.set_hidden_document(false);
    let saver = completed(&service, display_request()).expect("display");
    fake.set_battery_saver_override(true);
    assert_eq!(
        saver.revocation_reason(),
        Some(WakeRevocationReason::LowPowerSaver)
    );
    assert_eq!(service.active_lease_count(), 0);
    assert_eq!(fake.release_call_count(), 0);
    assert!(fake.active_native_tokens().is_empty());
    assert!(!saver.release());
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RaceMode {
    TokenRevoked,
    AllTokensRevoked,
    PermissionDenied,
    SurfaceDisposed,
    GenerationAdvanced,
    Sleeping,
    Background,
    Cancellation,
}

struct RaceProviderState {
    mode: RaceMode,
    surface: SurfaceId,
    generation: u64,
    next_token: u64,
    active_tokens: Vec<u64>,
    released_tokens: Vec<u64>,
    release_calls: u64,
    listener: Option<Weak<dyn PowerProviderListener>>,
    service: Weak<PowerService>,
    cancellation: CancellationToken,
    permission: PermissionStatus,
    snapshot: PowerSnapshot,
}

struct RaceProvider {
    state: Mutex<RaceProviderState>,
}

impl RaceProvider {
    fn new(
        mode: RaceMode,
        surface: SurfaceId,
        generation: u64,
        cancellation: CancellationToken,
    ) -> Self {
        Self {
            state: Mutex::new(RaceProviderState {
                mode,
                surface,
                generation,
                next_token: 1,
                active_tokens: Vec::new(),
                released_tokens: Vec::new(),
                release_calls: 0,
                listener: None,
                service: Weak::new(),
                cancellation,
                permission: PermissionStatus::NotRequired,
                snapshot: PowerSnapshot::unknown(SignalSource::Unknown),
            }),
        }
    }

    fn set_service(&self, service: Weak<PowerService>) {
        self.state.lock().expect("race provider state").service = service;
    }

    fn context(&self) -> OperationContext {
        let cancellation = self
            .state
            .lock()
            .expect("race provider state")
            .cancellation
            .clone();
        OperationContext {
            cancellation,
            ..OperationContext::default()
        }
    }

    fn active_tokens(&self) -> Vec<u64> {
        self.state
            .lock()
            .expect("race provider state")
            .active_tokens
            .clone()
    }

    fn released_tokens(&self) -> Vec<u64> {
        self.state
            .lock()
            .expect("race provider state")
            .released_tokens
            .clone()
    }

    fn release_calls(&self) -> u64 {
        self.state
            .lock()
            .expect("race provider state")
            .release_calls
    }

    fn notify(&self, notice: PowerProviderNotice) {
        let listener = self
            .state
            .lock()
            .expect("race provider state")
            .listener
            .clone()
            .and_then(|listener| listener.upgrade());
        if let Some(listener) = listener {
            listener.on_power_provider_notice(notice);
        }
    }

    fn upgrade_service(&self) -> Option<PowerService> {
        self.state
            .lock()
            .expect("race provider state")
            .service
            .upgrade()
            .map(|service| (*service).clone())
    }
}

impl PowerProvider for RaceProvider {
    fn capability(&self, operation: PowerOperation) -> Capability<PowerOperation> {
        Capability {
            operation,
            support: Support::Native,
            availability: Availability::Available,
            configuration: Configuration::Ready,
            permission: PermissionState::NotRequired,
            evidence: Evidence::ContractOnly,
            constraints: Vec::new(),
            observed_revision: self
                .state
                .lock()
                .expect("race provider state")
                .snapshot
                .revision,
        }
    }

    fn snapshot(&self) -> Result<PowerSnapshot, SystemError> {
        Ok(self
            .state
            .lock()
            .expect("race provider state")
            .snapshot
            .clone())
    }

    fn permission_status(&self, _kind: PermissionKind) -> PermissionStatus {
        self.state.lock().expect("race provider state").permission
    }

    fn acquire_inhibition(
        &self,
        _request: &PowerInhibitionRequest,
    ) -> Result<ProviderInhibitionGrant, SystemError> {
        let token = {
            let mut state = self.state.lock().expect("race provider state");
            let token = state.next_token;
            state.next_token = state.next_token.wrapping_add(1);
            state.active_tokens.push(token);
            token
        };
        let mode = self.state.lock().expect("race provider state").mode;
        match mode {
            RaceMode::TokenRevoked => self.notify(PowerProviderNotice::TokenRevoked {
                native_token: token,
                reason: WakeRevocationReason::SystemRevoked,
            }),
            RaceMode::AllTokensRevoked => self.notify(PowerProviderNotice::AllTokensRevoked {
                reason: WakeRevocationReason::SystemRevoked,
            }),
            RaceMode::PermissionDenied => {
                let denied = PermissionStatus::Denied {
                    can_request_again: false,
                };
                self.state.lock().expect("race provider state").permission = denied;
                self.notify(PowerProviderNotice::PermissionChanged {
                    kind: crate::WAKE_LOCK_PERMISSION,
                    status: denied,
                });
            }
            RaceMode::SurfaceDisposed => {
                if let Some(service) = self.upgrade_service() {
                    let surface = self.state.lock().expect("race provider state").surface;
                    service.dispose_surface(surface);
                }
            }
            RaceMode::GenerationAdvanced => {
                if let Some(service) = self.upgrade_service() {
                    let state = self.state.lock().expect("race provider state");
                    let (surface, generation) = (state.surface, state.generation);
                    drop(state);
                    service.advance_surface_generation(surface, generation.saturating_add(1));
                }
            }
            RaceMode::Sleeping => {
                let snapshot = PowerSnapshot {
                    sleep_resume: SleepResumeStatus::new(
                        SleepResumeState::PreparingSleep,
                        0,
                        SignalSource::Observed,
                    ),
                    revision: 2,
                    ..PowerSnapshot::unknown(SignalSource::Unknown)
                };
                self.notify(PowerProviderNotice::SnapshotUpdated(snapshot));
            }
            RaceMode::Background => {
                if let Some(service) = self.upgrade_service() {
                    service.notify_app_phase(AppPhase::Background);
                }
            }
            RaceMode::Cancellation => {
                let cancellation = self
                    .state
                    .lock()
                    .expect("race provider state")
                    .cancellation
                    .clone();
                cancellation.cancel(CancelReason::Caller);
            }
        }
        Ok(ProviderInhibitionGrant::Effective {
            native_token: token,
        })
    }

    fn release_inhibition(&self, native_token: u64) -> bool {
        let mut state = self.state.lock().expect("race provider state");
        state.release_calls += 1;
        let position = state
            .active_tokens
            .iter()
            .position(|token| *token == native_token);
        let Some(position) = position else {
            return false;
        };
        state.active_tokens.remove(position);
        state.released_tokens.push(native_token);
        true
    }

    fn attach_listener(&self, listener: Weak<dyn PowerProviderListener>) {
        self.state.lock().expect("race provider state").listener = Some(listener);
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

fn race_fixture(
    mode: RaceMode,
    surface: SurfaceId,
) -> (Arc<RaceProvider>, PowerService, Arc<PowerService>) {
    let cancellation = CancellationToken::default();
    let provider = Arc::new(RaceProvider::new(mode, surface, 1, cancellation));
    let service = PowerService::new(provider.clone());
    let anchor = Arc::new(service.clone());
    provider.set_service(Arc::downgrade(&anchor));
    (provider, service, anchor)
}

fn assert_no_effective_status(service: &PowerService) {
    let status = service.effective_wake_status();
    assert!(!status.inhibits_system_sleep);
    assert!(!status.inhibits_display_sleep);
    assert_eq!(status.active_lease_count, 0);
    assert_eq!(status.system_lease_count, 0);
    assert_eq!(status.display_lease_count, 0);
    assert_eq!(service.active_lease_count(), 0);
}

fn assert_no_acquired_event(events: &[Event<PowerEvent>]) {
    assert!(
        !events
            .iter()
            .any(|event| matches!(event, Event::Value(PowerEvent::WakeLeaseAcquired { .. }))),
        "a raced acquisition must not emit WakeLeaseAcquired"
    );
}

#[test]
fn toctou_revocations_during_acquire_never_register_effective_leases() {
    for (mode, expected) in [
        (RaceMode::TokenRevoked, ErrorCode::Unavailable),
        (RaceMode::AllTokensRevoked, ErrorCode::Unavailable),
        (RaceMode::PermissionDenied, ErrorCode::PermissionDenied),
    ] {
        let (provider, service, _anchor) = race_fixture(mode, SurfaceId(70));
        let subscription = service.subscribe();
        let error = service
            .acquire_wake_lease(display_request(), &provider.context())
            .expect_err("raced revocation must fail");
        assert_eq!(error.code, expected, "mode {mode:?}");
        assert_eq!(provider.release_calls(), 1, "mode {mode:?}");
        assert_eq!(provider.released_tokens(), vec![1], "mode {mode:?}");
        assert!(provider.active_tokens().is_empty(), "mode {mode:?}");
        assert_no_effective_status(&service);
        assert_no_acquired_event(&drain(&subscription));
    }
}

#[test]
fn toctou_lifecycle_and_cancellation_during_acquire_never_register_effective_leases() {
    for (mode, expected) in [
        (RaceMode::SurfaceDisposed, ErrorCode::ContextExpired),
        (RaceMode::GenerationAdvanced, ErrorCode::ContextExpired),
        (RaceMode::Sleeping, ErrorCode::Unavailable),
        (RaceMode::Background, ErrorCode::Unavailable),
    ] {
        let surface = SurfaceId(71);
        let (provider, service, _anchor) = race_fixture(mode, surface);
        service.attach_surface(surface, 1, true);
        let subscription = service.subscribe();
        let error = service
            .acquire_wake_lease(surface_request(surface, 1), &provider.context())
            .expect_err("raced lifecycle change must fail");
        assert_eq!(error.code, expected, "mode {mode:?}");
        assert_eq!(provider.release_calls(), 1, "mode {mode:?}");
        assert_eq!(provider.released_tokens(), vec![1], "mode {mode:?}");
        assert!(provider.active_tokens().is_empty(), "mode {mode:?}");
        assert_no_effective_status(&service);
        assert_no_acquired_event(&drain(&subscription));
    }

    let surface = SurfaceId(72);
    let (provider, service, _anchor) = race_fixture(RaceMode::Cancellation, surface);
    service.attach_surface(surface, 1, true);
    let subscription = service.subscribe();
    let result = service
        .acquire_wake_lease(surface_request(surface, 1), &provider.context())
        .expect("cancellation is not an error");
    match result {
        Completion::Cancelled(CancelReason::Caller) => {}
        _ => panic!("expected caller cancellation"),
    }
    assert_eq!(provider.release_calls(), 1);
    assert_eq!(provider.released_tokens(), vec![1]);
    assert!(provider.active_tokens().is_empty());
    assert_no_effective_status(&service);
    assert_no_acquired_event(&drain(&subscription));
}
