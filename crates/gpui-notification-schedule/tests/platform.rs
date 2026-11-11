//! Six-platform × six-operation contract matrix and provider fail-closed policy.

use gpui_notification_schedule::*;
use std::collections::BTreeSet;

#[test]
fn matrix_has_exactly_thirty_six_contract_only_cells_with_issue_links() {
    assert_eq!(PlatformTarget::ALL.len(), 6);
    assert_eq!(ScheduleOperation::ALL.len(), 6);
    let matrix = platform_support_matrix();
    assert_eq!(matrix.len(), 36);
    let mut pairs = BTreeSet::new();
    for row in &matrix {
        assert!(pairs.insert((row.platform, row.operation)));
        assert_eq!(row.evidence, Evidence::ContractOnly);
        assert!(!row.required_host_configuration.is_empty());
        assert!(!row.notes.is_empty());
        for link in [
            "#19", "#20", "#21", "#22", "#24", "#26", "#43", "#44", "#54", "#70",
        ] {
            assert!(row.has_link(link), "missing {link} on {row:?}");
        }
        let platform_link = match row.platform {
            PlatformTarget::Windows | PlatformTarget::MacOs | PlatformTarget::Linux => "#8",
            PlatformTarget::Android => "#2",
            PlatformTarget::Ios => "#9",
            PlatformTarget::Web => "",
        };
        if !platform_link.is_empty() {
            assert!(row.has_link(platform_link), "missing {platform_link}");
        }
    }
    assert_eq!(pairs.len(), 36);
    for platform in PlatformTarget::ALL {
        let rows = platform_operations(platform);
        assert_eq!(rows.len(), 6);
        assert!(rows.iter().all(|row| row.platform == platform));
    }
}

#[test]
fn platform_specific_support_matches_native_reality() {
    assert_eq!(
        operation_support(
            PlatformTarget::Windows,
            ScheduleOperation::ScheduleNotification
        )
        .support,
        Support::Native
    );
    assert_eq!(
        operation_support(
            PlatformTarget::Windows,
            ScheduleOperation::ScheduleExactAlarm
        )
        .support,
        Support::Unsupported
    );
    assert_eq!(
        operation_support(
            PlatformTarget::Android,
            ScheduleOperation::ScheduleExactAlarm
        )
        .support,
        Support::Conditional
    );
    assert_eq!(
        operation_support(
            PlatformTarget::Linux,
            ScheduleOperation::ObserveScheduledDelivery
        )
        .support,
        Support::Unsupported
    );
    assert_eq!(
        operation_support(PlatformTarget::Web, ScheduleOperation::ScheduleNotification).support,
        Support::Conditional
    );
    assert_eq!(
        operation_support(PlatformTarget::Web, ScheduleOperation::ScheduleExactAlarm).support,
        Support::Unsupported
    );
    assert_eq!(PlatformTarget::Web.native_platform(), None);
    assert_eq!(PlatformTarget::Ios.native_platform(), Some(Platform::Ios));
}

#[test]
fn web_policy_requires_a_service_worker_and_never_claims_baseline_triggers() {
    for maturity in [
        WebBackendMaturity::Experimental,
        WebBackendMaturity::Preview,
        WebBackendMaturity::Stable,
    ] {
        let policy = web_capability_policy(maturity);
        assert!(policy.service_worker_required);
        assert!(policy.notification_triggers_optional);
        assert!(!policy.notification_triggers_baseline);
        assert!(!policy.scheduled_delivery_observable);
        assert!(!policy.exact_alarm_supported);
        assert!(policy.supports(ScheduleOperation::ScheduleNotification));
        assert!(!policy.supports(ScheduleOperation::ScheduleExactAlarm));
        assert_eq!(
            policy.support_for(ScheduleOperation::ObserveScheduledDelivery),
            Support::Unsupported
        );
    }
}

#[test]
fn unavailable_provider_and_manifest_fail_closed_everywhere() {
    assert_eq!(PROVIDER_MANIFEST.len(), 5);
    for manifest in PROVIDER_MANIFEST {
        assert!(!manifest.native_provider_installed);
        assert_eq!(manifest.evidence, Evidence::ContractOnly);
    }
    for platform in PlatformTarget::ALL {
        assert!(!durable_scheduling_available(platform));
    }
    let provider = UnavailableNotificationScheduleProvider::new();
    for operation in ScheduleOperation::ALL {
        let capability = provider.capability(operation);
        assert_eq!(capability.evidence, Evidence::ContractOnly);
        assert_eq!(capability.support, Support::Unsupported);
        assert_eq!(
            capability.check().expect_err("unavailable").code,
            ErrorCode::Unsupported
        );
    }
    assert_eq!(
        provider.pending_ids().expect_err("unavailable").code,
        ErrorCode::Unavailable
    );
    assert_eq!(
        provider
            .channel_status("any")
            .expect_err("unavailable")
            .code,
        ErrorCode::Unavailable
    );
}

#[test]
fn fake_provider_capability_changes_gate_check() {
    let provider = FakeNotificationScheduleProvider::new();
    provider.set_support(
        ScheduleOperation::RepeatAndCalendarSchedule,
        Support::Unsupported,
    );
    assert_eq!(
        provider
            .capability(ScheduleOperation::RepeatAndCalendarSchedule)
            .check()
            .expect_err("unsupported")
            .code,
        ErrorCode::Unsupported
    );
    provider.set_support(
        ScheduleOperation::RepeatAndCalendarSchedule,
        Support::Native,
    );
    provider.set_availability(
        ScheduleOperation::ReconcilePendingSchedules,
        Availability::Unavailable,
    );
    assert_eq!(
        provider
            .capability(ScheduleOperation::ReconcilePendingSchedules)
            .check()
            .expect_err("unavailable")
            .code,
        ErrorCode::Unavailable
    );
    provider.set_availability(
        ScheduleOperation::ReconcilePendingSchedules,
        Availability::Available,
    );
    provider.set_configuration(
        ScheduleOperation::ReconcilePendingSchedules,
        Configuration::Required,
    );
    assert_eq!(
        provider
            .capability(ScheduleOperation::ReconcilePendingSchedules)
            .check()
            .expect_err("configuration")
            .code,
        ErrorCode::InvalidConfiguration
    );
}

#[test]
fn operation_labels_and_platform_current_are_stable() {
    assert_eq!(
        ScheduleOperation::ScheduleNotification.as_str(),
        "schedule_notification"
    );
    assert_eq!(
        ScheduleOperation::ReconcilePendingSchedules.label(),
        "ReconcilePendingSchedules"
    );
    assert!(PlatformTarget::current().is_some());
    assert_eq!(PROVIDER_MANIFEST.len(), 5);
}
