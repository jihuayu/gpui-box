mod support;

use gpui_power::*;
use gpui_system::{Evidence, Support};
use std::collections::BTreeSet;

#[test]
fn platform_matrix_is_complete_and_unique() {
    let matrix = platform_support_matrix();
    assert_eq!(matrix.len(), 30);
    assert_eq!(PlatformTarget::ALL.len(), 6);
    assert_eq!(PowerOperation::ALL.len(), 5);
    let mut pairs = BTreeSet::new();
    for row in &matrix {
        assert!(pairs.insert((row.platform, row.operation)));
        assert_eq!(row.evidence, Evidence::ContractOnly);
    }
    assert_eq!(pairs.len(), 30);
    for platform in PlatformTarget::ALL {
        let rows = platform_operations(platform);
        assert_eq!(rows.len(), 5);
        for operation in PowerOperation::ALL {
            let row = operation_support(platform, operation);
            assert_eq!(row.platform, platform);
            assert_eq!(row.operation, operation);
            assert!(!row.required_host_configuration.is_empty());
            assert!(!row.notes.is_empty());
            assert!(row.has_link("#19"));
        }
    }
}

#[test]
fn support_rows_match_the_documented_platform_contract() {
    assert_eq!(
        operation_support(PlatformTarget::Windows, PowerOperation::ObservePower).support,
        Support::Native
    );
    assert_eq!(
        operation_support(
            PlatformTarget::Windows,
            PowerOperation::ObserveThermalMemory
        )
        .support,
        Support::Conditional
    );
    assert_eq!(
        operation_support(PlatformTarget::MacOs, PowerOperation::ObserveThermalMemory).support,
        Support::Native
    );
    for operation in PowerOperation::ALL {
        assert_eq!(
            operation_support(PlatformTarget::Linux, operation).support,
            Support::Conditional
        );
    }
    assert_eq!(
        operation_support(PlatformTarget::Android, PowerOperation::InhibitSystemSleep).support,
        Support::Conditional
    );
    assert_eq!(
        operation_support(PlatformTarget::Ios, PowerOperation::InhibitSystemSleep).support,
        Support::Unsupported
    );
    assert_eq!(
        operation_support(PlatformTarget::Ios, PowerOperation::InhibitDisplaySleep).support,
        Support::Conditional
    );
    assert_eq!(
        operation_support(PlatformTarget::Web, PowerOperation::ObservePower).support,
        Support::Conditional
    );
    assert_eq!(
        operation_support(PlatformTarget::Web, PowerOperation::ObserveThermalMemory).support,
        Support::Unsupported
    );
    assert_eq!(
        operation_support(PlatformTarget::Web, PowerOperation::InhibitSystemSleep).support,
        Support::Unsupported
    );
    assert_eq!(
        operation_support(PlatformTarget::Web, PowerOperation::InhibitDisplaySleep).support,
        Support::Conditional
    );
}

#[test]
fn ios_and_web_have_no_system_wake_lock_contract() {
    for platform in [PlatformTarget::Ios, PlatformTarget::Web] {
        let row = operation_support(platform, PowerOperation::InhibitSystemSleep);
        assert_eq!(row.support, Support::Unsupported);
        assert!(!row.is_supported());
    }
    let ios = operation_support(PlatformTarget::Ios, PowerOperation::InhibitSystemSleep);
    assert!(ios.required_host_configuration.contains("BGTaskScheduler"));
    let web = operation_support(PlatformTarget::Web, PowerOperation::InhibitSystemSleep);
    assert!(
        web.required_host_configuration
            .contains("no system wake lock")
    );
}

#[test]
fn native_api_names_are_recorded_in_host_configuration() {
    assert!(
        operation_support(PlatformTarget::Windows, PowerOperation::InhibitSystemSleep)
            .required_host_configuration
            .contains("PowerSetRequest")
    );
    assert!(
        operation_support(PlatformTarget::Windows, PowerOperation::ObservePower)
            .required_host_configuration
            .contains("GetSystemPowerStatus")
    );
    assert!(
        operation_support(PlatformTarget::MacOs, PowerOperation::InhibitDisplaySleep)
            .required_host_configuration
            .contains("kIOPMAssertionTypePreventUserIdleDisplaySleep")
    );
    assert!(
        operation_support(PlatformTarget::Linux, PowerOperation::InhibitSystemSleep)
            .required_host_configuration
            .contains("logind")
    );
    assert!(
        operation_support(PlatformTarget::Android, PowerOperation::InhibitSystemSleep)
            .required_host_configuration
            .contains("WAKE_LOCK")
    );
    assert!(
        operation_support(PlatformTarget::MacOs, PowerOperation::ObserveSession)
            .required_host_configuration
            .contains("NSWorkspace")
    );
}

#[test]
fn qualification_links_name_every_owner_issue() {
    let all_links = platform_support_matrix()
        .into_iter()
        .flat_map(|row| {
            row.qualification_links
                .iter()
                .map(|link| (*link).to_owned())
                .collect::<Vec<_>>()
        })
        .collect::<BTreeSet<_>>();
    for expected in ["#2", "#8", "#9", "#18", "#19", "#20", "#21", "#43"] {
        assert!(all_links.contains(expected), "missing link {expected}");
    }
}

#[test]
fn web_capability_policy_is_experimental_and_visible_document_only() {
    let policy = web_capability_policy();
    assert_eq!(policy.maturity, WebBackendMaturity::Experimental);
    assert_eq!(policy.backend_name, "navigator.wakeLock");
    assert!(policy.secure_context_required);
    assert_eq!(policy.permissions_policy, "screen-wake-lock");
    assert!(policy.visible_document_required);
    assert!(!policy.system_wake_lock_available);
    assert_eq!(policy.screen_wake_lock_type, "screen");
    assert!(policy.releases_on_visibility_hidden);
    assert!(policy.rejects_hidden_requests);
    assert!(policy.low_power_mode_overrides);
    assert_eq!(policy.observe_power, Support::Conditional);
    assert_eq!(policy.observe_session, Support::Conditional);
    assert_eq!(policy.observe_thermal_memory, Support::Unsupported);
    assert_eq!(policy.inhibit_system_sleep, Support::Unsupported);
    assert_eq!(policy.inhibit_display_sleep, Support::Conditional);
    assert!(policy.supports(PowerOperation::InhibitDisplaySleep));
    assert!(!policy.supports(PowerOperation::InhibitSystemSleep));
    assert!(policy.display_lease_effective(true, true));
    assert!(!policy.display_lease_effective(false, true));
    assert!(!policy.display_lease_effective(true, false));
}

#[test]
fn evidence_never_claims_runtime_verification() {
    assert!(
        platform_support_matrix()
            .iter()
            .all(|row| row.evidence != Evidence::RuntimeVerified)
    );
    assert!(
        platform_support_matrix()
            .iter()
            .all(|row| { matches!(row.evidence, Evidence::ContractOnly) })
    );
    assert!(
        PROVIDER_MANIFEST
            .iter()
            .all(|manifest| !manifest.native_provider_installed)
    );
    assert!(
        PROVIDER_MANIFEST
            .iter()
            .all(|manifest| manifest.evidence == Evidence::ContractOnly)
    );
}

#[test]
fn current_target_is_a_matrix_member_when_known() {
    if let Some(current) = PlatformTarget::current() {
        assert!(PlatformTarget::ALL.contains(&current));
        assert_eq!(platform_operations(current).len(), 5);
    }
}
