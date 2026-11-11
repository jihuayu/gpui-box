//! Six-target, five-operation platform contract and qualification metadata.

use gpui_diagnostics::{
    DiagnosticsOperation, PlatformTarget, operation_support, platform_support,
    platform_support_matrix, unresolved_host_adapters,
};

#[test]
fn matrix_covers_every_platform_operation_pair_exactly_once() {
    let matrix = platform_support_matrix();
    assert_eq!(
        matrix.len(),
        PlatformTarget::ALL.len() * DiagnosticsOperation::ALL.len()
    );
    assert_eq!(matrix.len(), 30);
    for platform in PlatformTarget::ALL {
        let rows = platform_support(platform);
        assert_eq!(rows.len(), DiagnosticsOperation::ALL.len());
        for operation in DiagnosticsOperation::ALL {
            let matching: Vec<_> = matrix
                .iter()
                .filter(|row| row.platform == platform && row.operation == operation)
                .collect();
            assert_eq!(matching.len(), 1, "{platform}/{operation} must be unique");
            assert_eq!(operation_support(platform, operation), *matching[0]);
            assert!(!matching[0].required_host_configuration.is_empty());
            assert!(!matching[0].notes.is_empty());
            assert!(!matching[0].qualification_links.is_empty());
        }
    }
}

#[test]
fn desktop_android_ios_and_web_support_shape_is_honest() {
    for platform in [
        PlatformTarget::Windows,
        PlatformTarget::MacOs,
        PlatformTarget::Linux,
    ] {
        assert_eq!(
            operation_support(platform, DiagnosticsOperation::RecordEvent).support,
            gpui_diagnostics::Support::Native
        );
        assert_eq!(
            operation_support(platform, DiagnosticsOperation::WriteCrashMarker).support,
            gpui_diagnostics::Support::Conditional
        );
        assert_eq!(
            operation_support(platform, DiagnosticsOperation::InspectRecovery).support,
            gpui_diagnostics::Support::Native
        );
        assert_eq!(
            operation_support(platform, DiagnosticsOperation::ExportBundle).support,
            gpui_diagnostics::Support::Native
        );
        assert_eq!(
            operation_support(platform, DiagnosticsOperation::PruneRetained).support,
            gpui_diagnostics::Support::Native
        );
    }

    for platform in [PlatformTarget::Android, PlatformTarget::Ios] {
        assert_eq!(
            operation_support(platform, DiagnosticsOperation::RecordEvent).support,
            gpui_diagnostics::Support::Native
        );
        assert_eq!(
            operation_support(platform, DiagnosticsOperation::WriteCrashMarker).support,
            gpui_diagnostics::Support::Conditional
        );
        assert_eq!(
            operation_support(platform, DiagnosticsOperation::InspectRecovery).support,
            gpui_diagnostics::Support::Native
        );
        assert_eq!(
            operation_support(platform, DiagnosticsOperation::ExportBundle).support,
            gpui_diagnostics::Support::Conditional
        );
        assert_eq!(
            operation_support(platform, DiagnosticsOperation::PruneRetained).support,
            gpui_diagnostics::Support::Native
        );
    }

    assert_eq!(
        operation_support(PlatformTarget::Web, DiagnosticsOperation::RecordEvent).support,
        gpui_diagnostics::Support::Conditional
    );
    assert_eq!(
        operation_support(PlatformTarget::Web, DiagnosticsOperation::WriteCrashMarker).support,
        gpui_diagnostics::Support::Unsupported
    );
    assert_eq!(
        operation_support(PlatformTarget::Web, DiagnosticsOperation::InspectRecovery).support,
        gpui_diagnostics::Support::Conditional
    );
    assert_eq!(
        operation_support(PlatformTarget::Web, DiagnosticsOperation::ExportBundle).support,
        gpui_diagnostics::Support::Unsupported
    );
    assert_eq!(
        operation_support(PlatformTarget::Web, DiagnosticsOperation::PruneRetained).support,
        gpui_diagnostics::Support::Conditional
    );
}

#[test]
fn host_configuration_and_qualification_links_cover_the_required_evidence() {
    let android = operation_support(
        PlatformTarget::Android,
        DiagnosticsOperation::WriteCrashMarker,
    );
    let config = android.required_host_configuration.to_ascii_lowercase();
    assert!(config.contains("jni"));
    assert!(config.contains("anr") || config.contains("tombstone"));

    let ios = operation_support(PlatformTarget::Ios, DiagnosticsOperation::WriteCrashMarker);
    let config = ios.required_host_configuration.to_ascii_lowercase();
    assert!(config.contains("metrickit"));
    assert!(config.contains("swift") || config.contains("abi"));

    let windows = operation_support(
        PlatformTarget::Windows,
        DiagnosticsOperation::WriteCrashMarker,
    );
    let config = windows.required_host_configuration.to_ascii_lowercase();
    assert!(config.contains("seh") || config.contains("signal"));
    assert!(config.contains("unwind"));

    let web = operation_support(PlatformTarget::Web, DiagnosticsOperation::ExportBundle);
    assert!(web.notes.to_ascii_lowercase().contains("host"));
    assert!(
        web.required_host_configuration
            .to_ascii_lowercase()
            .contains("host")
    );

    let links: Vec<String> = unresolved_host_adapters()
        .into_iter()
        .map(|adapter| adapter.link)
        .collect();
    for required in ["#2", "#8", "#9", "#19", "#22", "#23", "#43", "#73"] {
        assert!(
            links.iter().any(|link| link == required),
            "missing {required}"
        );
    }

    let unavailable = gpui_diagnostics::default_unavailable_evidence();
    assert!(!unavailable.is_empty());
    for platform in [
        PlatformTarget::Android,
        PlatformTarget::Ios,
        PlatformTarget::Web,
    ] {
        assert!(
            unavailable
                .iter()
                .any(|evidence| evidence.platform == Some(platform)),
            "missing unavailable evidence for {platform}"
        );
    }
    for evidence in unavailable {
        assert!(!evidence.collector.is_empty());
        assert!(!evidence.reason.is_empty());
    }
}
