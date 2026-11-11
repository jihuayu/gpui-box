mod support;

use gpui_tray::*;
use support::*;

#[test]
fn matrix_has_all_thirty_cells_and_desktop_is_supported() {
    let rows = platform_support_matrix();
    assert_eq!(rows.len(), 30);
    let mut seen = std::collections::BTreeSet::new();
    for row in &rows {
        assert!(
            seen.insert((row.platform, row.operation)),
            "duplicate matrix cell"
        );
        assert_eq!(row.evidence, Evidence::ContractOnly);
        if row.platform.is_mobile_or_web() {
            assert_eq!(row.support, Support::Unsupported, "{row:?}");
        } else {
            assert!(row.is_supported(), "{row:?}");
        }
        assert!(!row.required_host_configuration.is_empty());
        assert!(!row.notes.is_empty());
        assert!(row.has_link("#50"));
    }
    for platform in PlatformTarget::ALL {
        let operations = platform_operations(platform);
        assert_eq!(operations.len(), TrayOperation::ALL.len());
        assert_eq!(
            operations
                .iter()
                .map(|row| row.operation)
                .collect::<Vec<_>>(),
            TrayOperation::ALL.to_vec()
        );
        for operation in TrayOperation::ALL {
            assert_eq!(operation_support(platform, operation).operation, operation);
            assert_eq!(
                platform.support(operation),
                operation_support(platform, operation).support
            );
        }
    }
}

#[test]
fn mobile_and_web_never_report_native_support() {
    for platform in [
        PlatformTarget::Android,
        PlatformTarget::Ios,
        PlatformTarget::Web,
    ] {
        for operation in TrayOperation::ALL {
            assert_eq!(platform.support(operation), Support::Unsupported);
        }
        let provider = UnsupportedTrayProvider::new(platform);
        assert_eq!(provider.platform(), platform);
        for capability in TrayProvider::capabilities(&provider) {
            assert_eq!(capability.support, Support::Unsupported);
            assert!(capability.check().is_err());
        }
        assert_eq!(provider.shell_status(), ShellTrayStatus::Absent);
    }
}

#[test]
fn provider_manifest_is_explicitly_contract_only() {
    assert_eq!(PROVIDER_MANIFEST.len(), 5);
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
fn desktop_resolves_native_tray_when_shell_is_available() {
    let fake = FakeTrayProvider::with_platform(PlatformTarget::Linux);
    let service = TrayService::from_fake(fake.clone());
    assert_eq!(
        service.resolve_surface_strategy(),
        EffectiveStatusSurface::NativeTray
    );
    fake.set_shell_status(ShellTrayStatus::Absent);
    assert_eq!(
        service.resolve_surface_strategy(),
        EffectiveStatusSurface::UnsupportedNone
    );
    service.set_residency_policy(
        TrayResidencyPolicy::new()
            .with_alternative_surface(MobileAlternativeSurface::InAppStatusStrip),
    );
    assert_eq!(
        service.resolve_surface_strategy(),
        EffectiveStatusSurface::MobileAlternative(MobileAlternativeSurface::InAppStatusStrip)
    );
}

#[test]
fn desktop_unsupported_capability_falls_back_without_faking_native_tray() {
    let fake = FakeTrayProvider::with_platform(PlatformTarget::Windows);
    let service = TrayService::from_fake(fake.clone());
    fake.set_support(TrayOperation::CreateStatusItem, Support::Unsupported);
    assert_eq!(
        service.resolve_surface_strategy(),
        EffectiveStatusSurface::UnsupportedNone
    );
    service.set_residency_policy(TrayResidencyPolicy::new().with_alternative_surface(
        MobileAlternativeSurface::ongoing_notification("tray", "status").expect("notification"),
    ));
    assert!(matches!(
        service.resolve_surface_strategy(),
        EffectiveStatusSurface::MobileAlternative(
            MobileAlternativeSurface::OngoingNotification { .. }
        )
    ));
}

#[test]
fn mobile_resolution_uses_only_configured_applicable_alternatives() {
    let android = TrayService::from_fake(FakeTrayProvider::with_platform(PlatformTarget::Android));
    assert_eq!(
        android.resolve_surface_strategy(),
        EffectiveStatusSurface::UnsupportedNone
    );
    android.set_residency_policy(TrayResidencyPolicy::new().with_alternative_surface(
        MobileAlternativeSurface::ongoing_notification("tray", "status").expect("notification"),
    ));
    assert!(matches!(
        android.resolve_surface_strategy(),
        EffectiveStatusSurface::MobileAlternative(
            MobileAlternativeSurface::OngoingNotification { .. }
        )
    ));

    let ios = TrayService::from_fake(FakeTrayProvider::with_platform(PlatformTarget::Ios));
    ios.set_residency_policy(TrayResidencyPolicy::new().with_alternative_surface(
        MobileAlternativeSurface::ongoing_notification("tray", "status").expect("notification"),
    ));
    assert_eq!(
        ios.resolve_surface_strategy(),
        EffectiveStatusSurface::UnsupportedNone,
        "iOS has no ongoing-notification surface"
    );
    ios.set_residency_policy(
        TrayResidencyPolicy::new()
            .with_alternative_surface(MobileAlternativeSurface::InAppStatusStrip),
    );
    assert_eq!(
        ios.resolve_surface_strategy(),
        EffectiveStatusSurface::MobileAlternative(MobileAlternativeSurface::InAppStatusStrip)
    );

    let web = TrayService::from_fake(FakeTrayProvider::with_platform(PlatformTarget::Web));
    web.set_residency_policy(TrayResidencyPolicy::new().with_alternative_surface(
        MobileAlternativeSurface::shortcut("tray.shortcut").expect("shortcut"),
    ));
    assert_eq!(
        web.resolve_surface_strategy(),
        EffectiveStatusSurface::UnsupportedNone,
        "the Web has no OS quick-settings surface"
    );
    web.set_residency_policy(
        TrayResidencyPolicy::new()
            .with_alternative_surface(MobileAlternativeSurface::InAppStatusStrip),
    );
    assert_eq!(
        web.resolve_surface_strategy(),
        EffectiveStatusSurface::MobileAlternative(MobileAlternativeSurface::InAppStatusStrip)
    );
}

#[test]
fn mobile_native_creation_is_never_faked_even_if_a_capability_is_tampered_with() {
    let fake = FakeTrayProvider::with_platform(PlatformTarget::Android);
    fake.set_support(TrayOperation::CreateStatusItem, Support::Native);
    let service = TrayService::from_fake(fake.clone());
    assert_eq!(
        service.resolve_surface_strategy(),
        EffectiveStatusSurface::UnsupportedNone
    );
    let (_owner, context) = ui_context(1);
    service.attach_surface(SurfaceId(1), 1, true);
    let error = service
        .create_application_status_item(spec("app.status"), &context)
        .expect_err("mobile native creation must be rejected");
    assert_eq!(error.code, ErrorCode::Unsupported);
    assert_eq!(fake.create_call_count(), 0);
}
