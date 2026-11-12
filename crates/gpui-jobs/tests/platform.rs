//! Honest platform capability matrix and browser policy.

use gpui_jobs::*;
use std::collections::BTreeSet;

#[test]
fn matrix_is_exactly_six_operations_by_six_targets() {
    let matrix = platform_support_matrix();
    assert_eq!(matrix.len(), 36);
    assert_eq!(JobsOperation::ALL.len(), 6);
    assert_eq!(PlatformTarget::ALL.len(), 6);
    let mut pairs = BTreeSet::new();
    for row in &matrix {
        assert!(pairs.insert((row.platform, row.operation)));
        assert_eq!(
            operation_support(row.platform, row.operation).evidence,
            Evidence::ContractOnly
        );
        assert!(!row.notes.is_empty());
        assert!(!row.required_host_configuration.is_empty());
        assert!(!row.qualification_links.is_empty());
        assert!(row.has_link("#70"));
        assert!(row.has_link("#22"));
        assert!(row.has_link("#55"));
        assert!(row.has_link("#69"));
        assert!(!row.has_link("#68"));
    }
    assert_eq!(pairs.len(), 36);
    assert!(
        operation_support(PlatformTarget::Android, JobsOperation::ScheduleDurableJob)
            .has_link("#2")
    );
    assert!(
        operation_support(PlatformTarget::Ios, JobsOperation::ScheduleDurableJob).has_link("#9")
    );
    assert!(
        operation_support(PlatformTarget::Windows, JobsOperation::ScheduleDurableJob)
            .has_link("#8")
    );
    for platform in PlatformTarget::ALL {
        let rows = platform_operations(platform);
        assert_eq!(rows.len(), 6);
        assert!(rows.iter().all(|row| row.platform == platform));
    }
}

#[test]
fn every_matrix_cell_is_contract_only_and_links_are_bounded() {
    for row in platform_support_matrix() {
        assert_eq!(row.evidence, Evidence::ContractOnly);
        assert!(matches!(
            row.support,
            Support::Native | Support::Conditional | Support::Unsupported
        ));
        assert!(
            row.qualification_links
                .iter()
                .all(|link| link.starts_with('#'))
        );
        assert!(row.qualification_links.len() <= 10);
        assert!(row.notes.len() <= 512);
        assert!(row.required_host_configuration.len() <= 1024);
        assert!(
            row.has_link(row.qualification_links[0]),
            "row links must be self-consistent"
        );
    }
    assert_eq!(
        operation_support(PlatformTarget::Windows, JobsOperation::ScheduleDurableJob).support,
        Support::Native
    );
    for operation in JobsOperation::ALL {
        assert_eq!(
            operation_support(PlatformTarget::Web, operation).support,
            Support::Conditional
        );
    }
}

#[test]
fn platform_names_and_operations_are_unique_and_stable() {
    let platforms = PlatformTarget::ALL
        .iter()
        .map(|platform| platform.as_str())
        .collect::<BTreeSet<_>>();
    assert_eq!(platforms.len(), 6);
    let operations = JobsOperation::ALL
        .iter()
        .map(|operation| operation.as_str())
        .collect::<BTreeSet<_>>();
    assert_eq!(operations.len(), 6);
    for operation in JobsOperation::ALL {
        assert_eq!(
            operation.is_in_process(),
            matches!(
                operation,
                JobsOperation::RunBoundedCpuTask | JobsOperation::RunBoundedIoTask
            )
        );
        assert!(!operation.label().is_empty());
    }
    let expected = match (std::env::consts::ARCH, std::env::consts::OS) {
        ("wasm32", _) => Some(PlatformTarget::Web),
        (_, "windows") => Some(PlatformTarget::Windows),
        (_, "macos") => Some(PlatformTarget::MacOs),
        (_, "linux") => Some(PlatformTarget::Linux),
        (_, "android") => Some(PlatformTarget::Android),
        (_, "ios") => Some(PlatformTarget::Ios),
        _ => None,
    };
    assert_eq!(PlatformTarget::current(), expected);
    assert_eq!(PlatformTarget::Windows.to_string(), "windows");
    assert_eq!(
        JobsOperation::RunBoundedCpuTask.to_string(),
        "run_bounded_cpu_task"
    );
}

#[test]
fn web_browser_policy_never_promises_durable_background_sync() {
    for maturity in [
        WebBackendMaturity::Experimental,
        WebBackendMaturity::Preview,
        WebBackendMaturity::Stable,
    ] {
        let policy = web_capability_policy(maturity);
        assert_eq!(policy.maturity, maturity);
        assert_eq!(policy.backend_name, "Service Worker Background Sync");
        assert!(policy.service_worker_required);
        assert!(policy.background_sync_optional);
        assert!(policy.durable_storage_available);
        assert!(policy.in_process_workers_available);
        assert_eq!(
            policy.schedule_durable_job,
            Support::Conditional,
            "Background Sync is optional, never a promise"
        );
        for operation in JobsOperation::ALL {
            assert!(policy.supports(operation));
            assert_eq!(policy.support_for(operation), Support::Conditional);
        }
        assert!(!durable_scheduling_available(PlatformTarget::Web));
    }
}

#[test]
fn provider_manifest_is_contract_only_and_covers_host_platforms() {
    assert_eq!(PROVIDER_MANIFEST.len(), 5);
    for manifest in PROVIDER_MANIFEST {
        assert!(!manifest.native_provider_installed);
        assert_eq!(manifest.evidence, Evidence::ContractOnly);
    }
    for platform in [
        Platform::Windows,
        Platform::MacOs,
        Platform::Linux,
        Platform::Android,
        Platform::Ios,
    ] {
        assert!(
            PROVIDER_MANIFEST
                .iter()
                .any(|manifest| manifest.platform == platform)
        );
    }
    for platform in PlatformTarget::ALL {
        assert!(!durable_scheduling_available(platform));
    }
}
