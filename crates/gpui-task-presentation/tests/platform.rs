//! Support-matrix, capability and badge-downgrade policy coverage.

use gpui_system::{Evidence, Support};
use gpui_task_presentation::{
    PlatformTarget, TaskPresentationOperation, current_platform_badge_downgrades_to_dot,
    operation_support, platform_operations, platform_support_matrix,
};

#[test]
fn all_thirty_cells_have_capability_and_evidence_invariants() {
    let rows = platform_support_matrix();
    assert_eq!(
        rows.len(),
        PlatformTarget::ALL.len() * TaskPresentationOperation::ALL.len()
    );
    for platform in PlatformTarget::ALL {
        for operation in TaskPresentationOperation::ALL {
            let row = operation_support(platform, operation);
            assert_eq!(row.platform, platform);
            assert_eq!(row.operation, operation);
            assert_eq!(row.evidence, Evidence::ContractOnly);
            assert!(row.required_host_configuration.len() > 16);
            assert!(row.notes.len() > 8);
            assert!(!row.qualification_links.is_empty());
            assert!(row.requires_string_configuration());
            assert!(row.has_link("#53"));
            match row.support {
                Support::Native | Support::Conditional => assert!(row.is_supported()),
                Support::Unsupported => assert!(!row.is_supported()),
                Support::FallbackAvailable | Support::Unknown => {
                    panic!("the policy matrix must not claim fallback or unknown support")
                }
            }
        }
        assert_eq!(platform.operations().len(), 5);
        assert_eq!(platform_operations(platform).len(), 5);
    }
}

#[test]
fn platform_specific_constraints_are_honest() {
    // Windows/macOS have native taskbar/Dock progress.
    assert_eq!(
        operation_support(
            PlatformTarget::Windows,
            TaskPresentationOperation::PresentTaskProgress
        )
        .support,
        Support::Native
    );
    assert_eq!(
        operation_support(
            PlatformTarget::MacOs,
            TaskPresentationOperation::PresentTaskProgress
        )
        .support,
        Support::Native
    );
    assert_eq!(
        operation_support(
            PlatformTarget::Linux,
            TaskPresentationOperation::PresentTaskProgress
        )
        .support,
        Support::Conditional
    );

    // Mobile platforms have no Dock/taskbar progress or attention surface.
    assert_eq!(
        operation_support(
            PlatformTarget::Android,
            TaskPresentationOperation::PresentTaskProgress
        )
        .support,
        Support::Unsupported
    );
    assert_eq!(
        operation_support(
            PlatformTarget::Ios,
            TaskPresentationOperation::PresentTaskProgress
        )
        .support,
        Support::Unsupported
    );
    assert_eq!(
        operation_support(
            PlatformTarget::Ios,
            TaskPresentationOperation::RequestUserAttention
        )
        .support,
        Support::Unsupported
    );
    assert_eq!(
        operation_support(
            PlatformTarget::Android,
            TaskPresentationOperation::MirrorProgressNotification
        )
        .support,
        Support::Native
    );
    assert_eq!(
        operation_support(
            PlatformTarget::Ios,
            TaskPresentationOperation::PresentAppBadge
        )
        .support,
        Support::Native
    );

    // The Web has no taskbar progress or attention contract.
    assert_eq!(
        operation_support(
            PlatformTarget::Web,
            TaskPresentationOperation::PresentTaskProgress
        )
        .support,
        Support::Unsupported
    );
    assert_eq!(
        operation_support(
            PlatformTarget::Web,
            TaskPresentationOperation::RequestUserAttention
        )
        .support,
        Support::Unsupported
    );
    assert_eq!(
        operation_support(
            PlatformTarget::Web,
            TaskPresentationOperation::PresentAppBadge
        )
        .support,
        Support::Conditional
    );
}

#[test]
fn badge_downgrade_policy_is_declared_per_platform() {
    let windows = operation_support(
        PlatformTarget::Windows,
        TaskPresentationOperation::PresentAppBadge,
    );
    assert!(windows.badge_downgrades_to_dot);
    assert!(windows.notes.contains("downgrade"));

    let macos = operation_support(
        PlatformTarget::MacOs,
        TaskPresentationOperation::PresentAppBadge,
    );
    assert!(!macos.badge_downgrades_to_dot);

    let linux = operation_support(
        PlatformTarget::Linux,
        TaskPresentationOperation::PresentAppBadge,
    );
    assert!(linux.badge_downgrades_to_dot);

    let android = operation_support(
        PlatformTarget::Android,
        TaskPresentationOperation::PresentAppBadge,
    );
    assert!(!android.badge_downgrades_to_dot);

    let web = operation_support(
        PlatformTarget::Web,
        TaskPresentationOperation::PresentAppBadge,
    );
    assert!(!web.badge_downgrades_to_dot);

    let current = PlatformTarget::current();
    assert_eq!(
        current_platform_badge_downgrades_to_dot(),
        current.is_some_and(
            |platform| platform == PlatformTarget::Windows || platform == PlatformTarget::Linux
        )
    );
}

#[test]
fn every_operation_has_a_distinct_policy_family() {
    let mut descriptions = Vec::new();
    for operation in TaskPresentationOperation::ALL {
        descriptions.push(operation.as_str());
        assert!(!operation.label().is_empty());
        assert_eq!(operation.to_string(), operation.as_str());
    }
    descriptions.sort_unstable();
    descriptions.dedup();
    assert_eq!(descriptions.len(), 5);
    assert!(TaskPresentationOperation::PresentTaskProgress.is_progress());
    assert!(TaskPresentationOperation::PresentAppBadge.is_badge());
    assert!(TaskPresentationOperation::RequestUserAttention.is_attention());
    assert!(!TaskPresentationOperation::InvokeTaskAction.is_progress());
}
