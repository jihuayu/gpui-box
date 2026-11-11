//! Facade feature smoke tests for the power re-export.
//!
//! These tests are deterministic and contain no OS-specific paths or native UI
//! assumptions. The same fake provider runs on Linux and Windows.

#![cfg(feature = "power")]

use gpui_box::power::{
    BatteryStatus, ChargingState, Completion, FakePowerProvider, IneffectiveInhibitionReason,
    LowPowerMode, PowerOperation, PowerService, ReleasePolicy, SignalSource, SleepResumeState,
    Support, SurfaceId, WakeInhibitionKind, WakeLeaseRequest, WakeLeaseScope, WakeReason,
    WakeRevocationReason, operation_support, platform_support_matrix,
};

fn reason(text: &str) -> WakeReason {
    WakeReason::new(text).expect("valid reason")
}

fn acquire(
    service: &PowerService,
    request: WakeLeaseRequest,
) -> gpui_box::power::ServiceResult<gpui_box::power::WakePreventionLease> {
    service.acquire_wake_lease(request, &gpui_box::power::OperationContext::default())
}

#[test]
fn power_reexport_exposes_coordinated_contracts() {
    assert_eq!(
        gpui_box::power::UPSTREAM_GPUI_REVISION,
        gpui_box::UPSTREAM_REVISION
    );
    assert_eq!(gpui_box::power::PACKAGE_VERSION, gpui_box::VERSION);
    assert_eq!(PowerOperation::ALL.len(), 5);
    assert_eq!(platform_support_matrix().len(), 30);
    assert_eq!(gpui_box::power::DEFAULT_POWER_EVENT_CAPACITY, 64);
    assert_eq!(gpui_box::power::WAKE_LOCK_PERMISSION.label(), "wake_lock");
    assert_eq!(
        operation_support(
            gpui_box::power::PlatformTarget::Ios,
            PowerOperation::InhibitSystemSleep
        )
        .support,
        Support::Unsupported
    );
    let battery = BatteryStatus::new(
        Some(50),
        ChargingState::Discharging,
        LowPowerMode::Disabled,
        None,
        SignalSource::Observed,
    )
    .expect("battery");
    assert_eq!(battery.level_percent(), Some(50));
    assert!(
        BatteryStatus::new(
            Some(101),
            ChargingState::Unknown,
            LowPowerMode::Unknown,
            None,
            SignalSource::Observed
        )
        .is_err()
    );
}

#[test]
fn power_reexport_drives_wake_lease_lifecycle() {
    let fake = FakePowerProvider::new();
    let service = PowerService::from_fake(fake.clone());
    let lease = match acquire(
        &service,
        WakeLeaseRequest::system_idle(reason("facade lease")),
    )
    .expect("acquire")
    {
        Completion::Completed(lease) => lease,
        Completion::Cancelled(reason) => panic!("cancelled: {reason:?}"),
    };
    assert!(lease.is_effective());
    assert_eq!(lease.kind(), WakeInhibitionKind::SystemIdleSleep);
    assert_eq!(service.effective_wake_status().active_lease_count, 1);
    assert_eq!(fake.active_native_tokens().len(), 1);
    assert!(lease.release());
    assert!(!lease.release());
    assert_eq!(fake.release_call_count(), 1);
    assert_eq!(service.effective_wake_status().active_lease_count, 0);
}

#[test]
fn ineffective_inhibition_never_reports_effective_success() {
    let fake = FakePowerProvider::new();
    let service = PowerService::from_fake(fake.clone());
    fake.set_inhibition_mode(
        WakeInhibitionKind::DisplayAndSystem,
        IneffectiveInhibitionReason::HiddenSurfaceOrDocument,
    );
    let error = acquire(
        &service,
        WakeLeaseRequest::display_and_system(reason("hidden facade")),
    )
    .expect_err("refused");
    assert_eq!(error.code, gpui_box::power::ErrorCode::Unavailable);
    let status = service.effective_wake_status();
    assert!(!status.inhibits_system_sleep);
    assert!(!status.inhibits_display_sleep);
    assert_eq!(status.active_lease_count, 0);
    assert!(fake.active_native_tokens().is_empty());
}

#[test]
fn surface_scoped_lease_is_revoked_when_disposed() {
    let fake = FakePowerProvider::new();
    let service = PowerService::from_fake(fake.clone());
    service.attach_surface(SurfaceId(2), 1, true);
    let request = WakeLeaseRequest::display_and_system(reason("surface facade")).with_scope(
        WakeLeaseScope::surface(
            SurfaceId(2),
            1,
            ReleasePolicy::for_kind(WakeInhibitionKind::DisplayAndSystem),
        ),
    );
    let lease = match acquire(&service, request).expect("acquire") {
        Completion::Completed(lease) => lease,
        Completion::Cancelled(reason) => panic!("cancelled: {reason:?}"),
    };
    service.dispose_surface(SurfaceId(2));
    assert_eq!(
        lease.revocation_reason(),
        Some(WakeRevocationReason::SurfaceDisposed)
    );
    assert_eq!(fake.release_call_count(), 1);
    assert!(!lease.release());
}

#[test]
fn fake_provider_transitions_are_visible_through_the_facade() {
    let fake = FakePowerProvider::new();
    let service = PowerService::from_fake(fake.clone());
    fake.set_sleep_resume_state(SleepResumeState::Awake);
    fake.set_sleep_resume_state(SleepResumeState::PreparingSleep);
    fake.set_sleep_resume_state(SleepResumeState::Sleeping);
    fake.set_sleep_resume_state(SleepResumeState::Resumed);
    let snapshot = service.current_snapshot();
    assert_eq!(snapshot.sleep_resume.state, SleepResumeState::Resumed);
    assert_eq!(snapshot.sleep_resume.resume_epoch, 1);
    assert_eq!(snapshot.sleep_resume.source, SignalSource::Observed);
}

#[cfg(feature = "power-gpui")]
#[test]
fn power_gpui_feature_exposes_adapter_and_system_registration() {
    fn assert_adapter_symbols() {
        let _init = gpui_box::power::gpui_adapter::init_power;
        let _lookup = gpui_box::power::gpui_adapter::power_service;
        let _install = gpui_box::power::gpui_adapter::install_power_service;
        let _scope = gpui_box::power::gpui_adapter::PowerSurfaceScope::new;
    }
    assert_adapter_symbols();

    let fake = FakePowerProvider::new();
    let service = PowerService::from_fake(fake);
    let mut services = gpui_box::system::SystemServices::new();
    gpui_box::power::gpui_adapter::install_power_in_services(&mut services, service.clone())
        .expect("registration");
    let registered = services
        .service::<PowerService>()
        .expect("registered service");
    assert!(service.shares_state_with(&registered));
}
