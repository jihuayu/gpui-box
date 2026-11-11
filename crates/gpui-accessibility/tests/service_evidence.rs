mod common;

use accesskit::Role;
use common::*;
use gpui_accessibility::{
    AcceptanceStatus, AccessibilityAcceptanceMatrix, AccessibilityAcceptanceReport,
    AccessibilityOperation, AccessibilityPlatform, AccessibilityProviderManifest,
    AccessibilityService, AssistiveBridgeKind, AssistiveReader, AssistiveReaderGate,
    AssistiveReaderScenario, AssistiveReaderSessionEvidence, AssistiveReaderSessionStatus,
    AutomationEvidence, EvidenceError, FakeAccessibilityProvider, HostPlatform, IssueLinks,
    LiveAnnouncement, LiveAnnouncementDisposition, LivePriority, SceneRevision, SemanticIdentity,
    TextSelectionRequest, TextSelectionSpan,
};
use gpui_system::{
    Availability, Completion, Configuration, Constraint, ErrorCode, Evidence, Support, SurfaceId,
    SystemServices, UiOwner,
};
use std::rc::Rc;

#[cfg(feature = "gpui")]
fn root_snapshot(
    displayed: gpui_accessibility::DisplayedScene,
) -> gpui_accessibility::SemanticTreeSnapshot {
    let mut root = base_node(1, Role::Window);
    root.set_children(Vec::<accesskit::NodeId>::new());
    gpui_accessibility::SemanticTreeSnapshot::from_accesskit_update(
        displayed,
        &update(1, 1, vec![(1, root)]),
    )
    .expect("root snapshot")
}

fn assert_request_error<T>(mut request: gpui_system::Request<T>, expected: ErrorCode) {
    match poll_once(&mut request) {
        std::task::Poll::Ready(Err(error)) => assert_eq!(error.code, expected),
        std::task::Poll::Ready(Ok(_)) => {
            panic!("expected {expected:?}, got a successful completion")
        }
        std::task::Poll::Pending => panic!("expected {expected:?}, got a pending request"),
    }
}

#[test]
fn capability_gating_covers_every_accessibility_operation() {
    let provider = Rc::new(FakeAccessibilityProvider::new(
        AccessibilityPlatform::Windows,
    ));
    let service = AccessibilityService::new(provider.clone());
    let scene = scene(1);
    let target = SemanticIdentity::at(1u64);
    let selection = TextSelectionSpan::collapsed_at("text", 0).expect("selection");
    let context = gpui_system::OperationContext::default();

    for operation in AccessibilityOperation::ALL {
        provider.set_operation_unsupported(operation);
        match operation {
            AccessibilityOperation::ObserveTree => assert_request_error(
                service.observe_tree(
                    scene,
                    update(1, 1, vec![(1, base_node(1, Role::Window))]),
                    context.clone(),
                ),
                ErrorCode::Unsupported,
            ),
            AccessibilityOperation::PerformAction => assert_request_error(
                service.perform_action(
                    gpui_accessibility::SemanticActionRequest::click(scene, target.clone()),
                    context.clone(),
                ),
                ErrorCode::Unsupported,
            ),
            AccessibilityOperation::EditTextSelection => assert_request_error(
                service.edit_text_selection(
                    TextSelectionRequest::new(scene, target.clone(), "text", selection),
                    context.clone(),
                ),
                ErrorCode::Unsupported,
            ),
            AccessibilityOperation::AnnounceLive => assert_request_error(
                service.announce(
                    LiveAnnouncement::new(scene, target.clone(), LivePriority::Polite, "Blocked"),
                    context.clone(),
                ),
                ErrorCode::Unsupported,
            ),
            AccessibilityOperation::AuditScene => assert_request_error(
                service.audit_scene(scene, context.clone()),
                ErrorCode::Unsupported,
            ),
        }
        provider.set_support(operation, Support::Native);
        provider.set_availability(operation, Availability::Available);
        provider.set_configuration(operation, Configuration::Ready);
    }

    for capability in service.capabilities() {
        capability.check().expect("restored capability");
    }
    provider.set_permission(
        AccessibilityOperation::PerformAction,
        gpui_system::PermissionState::Denied,
    );
    assert_eq!(
        service
            .capability(AccessibilityOperation::PerformAction)
            .check()
            .expect_err("denied permission")
            .code,
        ErrorCode::PermissionDenied
    );
}

#[test]
fn all_six_platform_policies_map_bridges_configuration_and_owner_links() {
    assert_eq!(AccessibilityPlatform::ALL.len(), 6);
    let expected = [
        (
            AccessibilityPlatform::Windows,
            AssistiveBridgeKind::UiAutomation,
            "#8",
        ),
        (
            AccessibilityPlatform::MacOs,
            AssistiveBridgeKind::NsAccessibility,
            "#8",
        ),
        (
            AccessibilityPlatform::Linux,
            AssistiveBridgeKind::AtSpi,
            "#8",
        ),
        (
            AccessibilityPlatform::Android,
            AssistiveBridgeKind::TalkBack,
            "#2",
        ),
        (
            AccessibilityPlatform::Ios,
            AssistiveBridgeKind::VoiceOver,
            "#9",
        ),
        (
            AccessibilityPlatform::Web,
            AssistiveBridgeKind::BrowserAria,
            "#70",
        ),
    ];
    for (platform, bridge, adapter) in expected {
        assert_eq!(platform.bridge(), bridge);
        for operation in AccessibilityOperation::ALL {
            let policy = gpui_accessibility::platform_operation_policy(platform, operation);
            assert_eq!(policy.platform, platform);
            assert_eq!(policy.operation, operation);
            assert_eq!(policy.bridge, bridge);
            assert_eq!(policy.links.adapter, adapter);
            assert_eq!(policy.links.components, "#3");
            assert_eq!(policy.links.phase_one, "#18");
            assert!(policy.links.coordination.contains(&"#43"));
            assert!(policy.links.coordination.contains(&"#68"));
            assert!(policy.links.coordination.contains(&"#70"));
            assert!(!policy.required_configuration.is_empty());
            assert_eq!(policy.evidence, Evidence::ContractOnly);
            assert!(policy.runtime_evidence.is_empty());

            let capability = policy.capability(11);
            assert_eq!(capability.operation, operation);
            assert_eq!(
                capability.permission,
                gpui_system::PermissionState::NotRequired
            );
            assert!(
                capability
                    .constraints
                    .contains(&Constraint::Provider(bridge.label()))
            );
            let probed = policy.runtime_capability(
                Availability::Available,
                Configuration::Ready,
                Evidence::RuntimeVerified,
                12,
            );
            probed.check().expect("runtime capability");
        }
        assert_eq!(gpui_accessibility::platform_policies(platform).len(), 5);
    }
    assert_eq!(gpui_accessibility::all_platform_policies().len(), 30);

    let links = gpui_accessibility::platform_operation_policy(
        AccessibilityPlatform::Android,
        AccessibilityOperation::ObserveTree,
    )
    .links;
    assert_eq!(
        links,
        IssueLinks {
            adapter: "#2",
            components: "#3",
            phase_one: "#18",
            coordination: &["#43", "#68", "#70"],
        }
    );
    assert_eq!(
        gpui_accessibility::platform_operation_policy(
            AccessibilityPlatform::Windows,
            AccessibilityOperation::ObserveTree
        )
        .support,
        Support::Native
    );
    assert_eq!(
        gpui_accessibility::platform_operation_policy(
            AccessibilityPlatform::Web,
            AccessibilityOperation::ObserveTree
        )
        .support,
        Support::Conditional
    );
    for platform in AccessibilityPlatform::ALL {
        assert_eq!(
            gpui_accessibility::platform_operation_policy(
                platform,
                AccessibilityOperation::AuditScene
            )
            .support,
            Support::Native
        );
    }

    for manifest in gpui_accessibility::ACCESSIBILITY_PROVIDER_MANIFEST {
        assert!(!manifest.native_provider_installed);
        assert_eq!(manifest.evidence, Evidence::ContractOnly);
        assert_eq!(manifest.bridge, manifest.platform.bridge());
    }
    let policy = gpui_accessibility::web_capability_policy();
    assert_eq!(
        policy.maturity,
        gpui_accessibility::WebBackendMaturity::Experimental
    );
    assert!(policy.requires_secure_context);
    assert!(policy.requires_dom_accessibility_tree);
    assert!(policy.requires_same_origin_frame);
    assert_eq!(policy.maximum_tree_nodes, 5_000);
    assert_eq!(policy.supported_operations.len(), 5);
    assert_eq!(gpui_accessibility::ACCESSKIT_VERSION, "0.24.0");
    assert_eq!(
        gpui_accessibility::GPUI_REVISION,
        "f8cfc7ed9e57503def9479138894f3139f5400cc"
    );
    let _: HostPlatform = AccessibilityPlatform::Web;
    let _ = AccessibilityProviderManifest {
        platform: AccessibilityPlatform::Web,
        bridge: AssistiveBridgeKind::BrowserAria,
        native_provider_installed: false,
        evidence: Evidence::ContractOnly,
    };
}

fn reader_for(platform: AccessibilityPlatform) -> AssistiveReader {
    match platform {
        AccessibilityPlatform::Windows => AssistiveReader::Narrator,
        AccessibilityPlatform::MacOs => AssistiveReader::VoiceOverMacOs,
        AccessibilityPlatform::Linux => AssistiveReader::Orca,
        AccessibilityPlatform::Android => AssistiveReader::TalkBack,
        AccessibilityPlatform::Ios => AssistiveReader::VoiceOverIos,
        AccessibilityPlatform::Web => AssistiveReader::Other("Browser screen reader"),
    }
}

fn reader_evidence(
    platform: AccessibilityPlatform,
    status: AssistiveReaderSessionStatus,
) -> AssistiveReaderSessionEvidence {
    AssistiveReaderSessionEvidence::new(reader_for(platform), "2026.11", platform, status)
        .with_environment("test OS", "test device", "local session")
        .with_scenarios(AssistiveReaderScenario::ALL)
        .with_reference("2026-11-11T12:00:00Z", "session-recording-1")
}

fn automation_evidence(platform: AccessibilityPlatform) -> AutomationEvidence {
    AutomationEvidence::new(
        platform,
        AccessibilityOperation::AuditScene,
        "gpui-accessibility",
        "six_platform_semantics",
        "cargo test --locked -p gpui-accessibility -p gpui-box",
        true,
    )
    .with_tool("rustc", "1.99.1")
}

#[test]
fn automation_never_satisfies_reader_qualification_on_any_platform() {
    for platform in AccessibilityPlatform::ALL {
        let mut report = AccessibilityAcceptanceReport::new(platform);
        assert_eq!(report.status(), AcceptanceStatus::PendingAutomation);
        report
            .record_automation(automation_evidence(platform))
            .expect("automation evidence");
        assert!(report.automation_passed());
        assert!(!report.reader_session_accepted());
        assert_eq!(
            report.assistive_reader_session,
            AssistiveReaderGate::PendingReaderSession
        );
        assert_eq!(report.status(), AcceptanceStatus::PendingReaderSession);
        assert!(!report.is_accepted());
        assert_eq!(
            report.require_accepted(),
            Err(EvidenceError::ReaderSessionFailed)
        );

        report
            .record_assistive_reader_session(reader_evidence(
                platform,
                AssistiveReaderSessionStatus::Passed,
            ))
            .expect("reader evidence");
        assert!(report.reader_session_accepted());
        assert_eq!(report.status(), AcceptanceStatus::Accepted);
        assert!(report.is_accepted());
        report.require_accepted().expect("fully accepted");
    }
}

#[test]
fn reader_evidence_validation_and_failed_runs_stay_distinct() {
    for platform in AccessibilityPlatform::ALL {
        let mut report = AccessibilityAcceptanceReport::new(platform);
        report
            .record_automation(automation_evidence(platform))
            .expect("automation evidence");

        let mut incomplete = reader_evidence(platform, AssistiveReaderSessionStatus::Passed);
        incomplete.scenarios = vec![AssistiveReaderScenario::KeyboardTraversal];
        assert!(!incomplete.covers_required_scenarios());
        assert!(!incomplete.is_accepted());
        report
            .record_assistive_reader_session(incomplete)
            .expect("incomplete reader evidence is still recorded");
        assert_eq!(report.status(), AcceptanceStatus::ReaderSessionFailed);

        let failed = reader_evidence(platform, AssistiveReaderSessionStatus::Failed);
        report
            .record_assistive_reader_session(failed)
            .expect("failed session is recorded");
        assert_eq!(report.status(), AcceptanceStatus::ReaderSessionFailed);

        let passed = reader_evidence(platform, AssistiveReaderSessionStatus::Passed);
        assert!(passed.covers_required_scenarios());
        assert!(passed.is_accepted());
        report
            .record_assistive_reader_session(passed)
            .expect("passing reader evidence");
        assert!(report.is_accepted());
    }

    let mut report = AccessibilityAcceptanceReport::new(AccessibilityPlatform::Windows);
    report
        .record_automation(AutomationEvidence::new(
            AccessibilityPlatform::Linux,
            AccessibilityOperation::AuditScene,
            "suite",
            "test",
            "cargo test",
            true,
        ))
        .expect_err("cross-platform automation must be rejected");
    let mut wrong_reader = reader_evidence(
        AccessibilityPlatform::Windows,
        AssistiveReaderSessionStatus::Passed,
    );
    wrong_reader.reader = AssistiveReader::TalkBack;
    assert_eq!(
        report.record_assistive_reader_session(wrong_reader),
        Err(EvidenceError::ReaderPlatformMismatch {
            reader: AssistiveReader::TalkBack,
            platform: AccessibilityPlatform::Windows,
        })
    );

    let mut failed_automation = AccessibilityAcceptanceReport::new(AccessibilityPlatform::Linux);
    failed_automation
        .record_automation(AutomationEvidence::new(
            AccessibilityPlatform::Linux,
            AccessibilityOperation::ObserveTree,
            "suite",
            "test",
            "cargo test",
            false,
        ))
        .expect("failed automation is recorded");
    assert_eq!(
        failed_automation.status(),
        AcceptanceStatus::AutomationFailed
    );
    assert_eq!(
        failed_automation.require_accepted(),
        Err(EvidenceError::AutomationFailed)
    );
}

#[test]
fn six_platform_matrix_keeps_reader_gates_pending_after_all_automation() {
    let mut matrix = AccessibilityAcceptanceMatrix::new();
    assert!(matrix.all_reader_sessions_pending());
    assert_eq!(matrix.statuses().len(), 6);
    for platform in AccessibilityPlatform::ALL {
        assert_eq!(
            matrix.report(platform).expect("report").status(),
            AcceptanceStatus::PendingAutomation
        );
        matrix
            .report_mut(platform)
            .record_automation(automation_evidence(platform))
            .expect("automation");
    }
    assert!(matrix.all_automation_passed());
    assert!(matrix.all_reader_sessions_pending());
    for (_, status) in matrix.statuses() {
        assert_eq!(status, AcceptanceStatus::PendingReaderSession);
    }
    for platform in AccessibilityPlatform::ALL {
        matrix
            .report_mut(platform)
            .record_assistive_reader_session(reader_evidence(
                platform,
                AssistiveReaderSessionStatus::Passed,
            ))
            .expect("reader session");
    }
    assert!(!matrix.all_reader_sessions_pending());
    for (_, status) in matrix.statuses() {
        assert_eq!(status, AcceptanceStatus::Accepted);
    }
}

#[test]
fn service_records_evidence_without_conflating_automation_and_reader_channels() {
    let provider = Rc::new(FakeAccessibilityProvider::new(
        AccessibilityPlatform::Windows,
    ));
    let service = AccessibilityService::new(provider.clone());
    service
        .record_automation_evidence(automation_evidence(AccessibilityPlatform::Windows))
        .expect("service automation evidence");
    assert_eq!(service.automation_evidence().len(), 1);
    assert!(service.assistive_reader_evidence().is_empty());
    let report = service.acceptance_report();
    assert_eq!(report.status(), AcceptanceStatus::PendingReaderSession);
    assert_eq!(
        report.assistive_reader_session,
        AssistiveReaderGate::PendingReaderSession
    );

    service
        .record_assistive_reader_evidence(reader_evidence(
            AccessibilityPlatform::Windows,
            AssistiveReaderSessionStatus::Passed,
        ))
        .expect("service reader evidence");
    assert_eq!(service.assistive_reader_evidence().len(), 1);
    assert_eq!(provider.automation_evidence().len(), 1);
    assert_eq!(provider.assistive_reader_evidence().len(), 1);
    assert!(service.acceptance_report().is_accepted());

    let wrong_platform = AutomationEvidence::new(
        AccessibilityPlatform::Linux,
        AccessibilityOperation::AuditScene,
        "suite",
        "test",
        "cargo test",
        true,
    );
    let error = service
        .record_automation_evidence(wrong_platform)
        .expect_err("platform mismatch");
    assert_eq!(error.code, ErrorCode::InvalidConfiguration);
}

#[test]
fn system_services_registration_is_typed_and_rejects_duplicates() {
    let provider = Rc::new(FakeAccessibilityProvider::new(AccessibilityPlatform::Linux));
    let service = AccessibilityService::new(provider);
    let mut services = SystemServices::new();
    service.clone().install(&mut services).expect("install");
    let registered: Rc<AccessibilityService> = services
        .service::<AccessibilityService>()
        .expect("registered service");
    assert!(service.shares_state_with(&registered));
    assert!(service.clone().install(&mut services).is_err());
    assert!(services.service::<FakeAccessibilityProvider>().is_err());
}

#[test]
fn announce_is_transactional_when_the_provider_fails() {
    let provider = Rc::new(FakeAccessibilityProvider::new(
        AccessibilityPlatform::Windows,
    ));
    let service = AccessibilityService::with_live_capacity(provider.clone(), 4);
    let owner = UiOwner::new(SURFACE);
    let mut root = base_node(1, Role::Window);
    root.set_children(Vec::<accesskit::NodeId>::new());
    let snapshot = gpui_accessibility::SemanticTreeSnapshot::from_accesskit_update(
        scene(1),
        &update(1, 1, vec![(1, root)]),
    )
    .expect("scene snapshot");
    let mut commit = service.commit_scene(snapshot, ui_context(&owner));
    match poll_once(&mut commit) {
        std::task::Poll::Ready(result) => {
            completed(result);
        }
        std::task::Poll::Pending => panic!("commit did not complete"),
    }

    let announcement = LiveAnnouncement::new(
        scene(1),
        SemanticIdentity::at(1u64),
        LivePriority::Polite,
        "Saved",
    );
    provider.set_announce_error(Some(gpui_system::SystemError::new(
        ErrorCode::Unavailable,
        "Announcement bridge is unavailable",
    )));
    let mut failed = service.announce(announcement.clone(), ui_context(&owner));
    match poll_once(&mut failed) {
        std::task::Poll::Ready(Err(error)) => assert_eq!(error.code, ErrorCode::Unavailable),
        other => panic!("failed announcement was not reported: {other:?}"),
    }
    assert_eq!(service.live_queue_len(), 0);
    assert!(provider.recorded_announcements().is_empty());

    provider.set_announce_error(None);
    let mut retried = service.announce(announcement, ui_context(&owner));
    match poll_once(&mut retried) {
        std::task::Poll::Ready(result) => {
            let receipt = completed(result);
            assert_eq!(receipt.disposition, LiveAnnouncementDisposition::Queued);
            assert_eq!(receipt.queue_len, 1);
            assert_eq!(receipt.dropped, 0);
        }
        std::task::Poll::Pending => panic!("retry did not complete"),
    }
    assert_eq!(service.live_queue_len(), 1);
    assert_eq!(provider.recorded_announcements().len(), 1);
}

#[test]
fn surface_generation_disposal_cancellation_and_mismatch_are_enforced() {
    let provider = Rc::new(FakeAccessibilityProvider::new(
        AccessibilityPlatform::Windows,
    ));
    let service = AccessibilityService::new(provider);
    let owner = UiOwner::new(SURFACE);

    let mut observe = service.observe_tree(
        scene(1),
        update(1, 1, vec![(1, base_node(1, Role::Window))]),
        ui_context(&owner),
    );
    match poll_once(&mut observe) {
        std::task::Poll::Ready(result) => {
            completed(result);
        }
        std::task::Poll::Pending => panic!("observe did not complete"),
    }
    let mut repeated = service.observe_tree(
        scene(1),
        update(1, 1, vec![(1, base_node(1, Role::Window))]),
        ui_context(&owner),
    );
    match poll_once(&mut repeated) {
        std::task::Poll::Ready(Err(error)) => assert_eq!(error.code, ErrorCode::ResourceExpired),
        other => panic!("duplicate revision was not rejected: {other:?}"),
    }

    let mut audit = service.audit_scene(scene(1), gpui_system::OperationContext::default());
    match poll_once(&mut audit) {
        std::task::Poll::Ready(result) => {
            let report = completed(result);
            assert!(report.passed());
            assert_eq!(report.revision, SceneRevision(1));
        }
        std::task::Poll::Pending => panic!("audit did not complete"),
    }

    let stale_context = ui_context(&owner);
    owner.advance_generation().expect("generation bump");
    let mut stale_action = service.perform_action(
        gpui_accessibility::SemanticActionRequest::click(scene(1), SemanticIdentity::at(1u64)),
        stale_context,
    );
    match poll_once(&mut stale_action) {
        std::task::Poll::Ready(Err(error)) => assert_eq!(error.code, ErrorCode::ContextExpired),
        other => panic!("stale UI owner was not rejected: {other:?}"),
    }

    let other_surface = gpui_accessibility::DisplayedScene::new(SurfaceId(99), 1, SceneRevision(1));
    let mut wrong_surface = service.observe_tree(
        other_surface,
        update(1, 1, vec![(1, base_node(1, Role::Window))]),
        ui_context(&owner),
    );
    match poll_once(&mut wrong_surface) {
        std::task::Poll::Ready(Err(error)) => assert_eq!(error.code, ErrorCode::InvalidInput),
        other => panic!("wrong surface was not rejected: {other:?}"),
    }

    service.dispose_surface(SURFACE);
    assert!(service.snapshot(SURFACE).is_none());
    let mut disposed = service.observe_tree(
        scene_generation(1, 2),
        update(1, 1, vec![(1, base_node(1, Role::Window))]),
        ui_context(&owner),
    );
    match poll_once(&mut disposed) {
        std::task::Poll::Ready(Err(error)) => assert_eq!(error.code, ErrorCode::ContextExpired),
        other => panic!("disposed surface was not rejected: {other:?}"),
    }

    let mut next_generation = service.observe_tree(
        scene_generation(2, 1),
        update(1, 1, vec![(1, base_node(1, Role::Window))]),
        ui_context(&owner),
    );
    match poll_once(&mut next_generation) {
        std::task::Poll::Ready(result) => {
            let snapshot = completed(result);
            assert_eq!(snapshot.generation, 2);
        }
        std::task::Poll::Pending => panic!("new generation did not complete"),
    }
    let announcement = LiveAnnouncement::new(
        scene_generation(2, 1),
        SemanticIdentity::at(1u64),
        LivePriority::Polite,
        "Generation two",
    );
    let mut announce = service.announce(announcement, ui_context(&owner));
    match poll_once(&mut announce) {
        std::task::Poll::Ready(result) => {
            completed(result);
        }
        std::task::Poll::Pending => panic!("announcement did not complete"),
    }
    assert_eq!(service.live_queue_len(), 1);
    service
        .advance_generation(scene_generation(3, 1))
        .expect("advance generation");
    assert_eq!(service.live_queue_len(), 0);
    let mut old_generation = service.observe_tree(
        scene_generation(2, 2),
        update(1, 1, vec![(1, base_node(1, Role::Window))]),
        ui_context(&owner),
    );
    match poll_once(&mut old_generation) {
        std::task::Poll::Ready(Err(error)) => assert_eq!(error.code, ErrorCode::ContextExpired),
        other => panic!("old generation was not rejected: {other:?}"),
    }

    let cancelled_context = ui_context(&owner);
    cancelled_context
        .cancellation
        .cancel(gpui_system::CancelReason::Caller);
    let mut cancelled = service.observe_tree(
        scene_generation(3, 1),
        update(1, 1, vec![(1, base_node(1, Role::Window))]),
        cancelled_context,
    );
    match poll_once(&mut cancelled) {
        std::task::Poll::Ready(Ok(Completion::Cancelled(reason))) => {
            assert_eq!(reason, gpui_system::CancelReason::Caller)
        }
        other => panic!("cancellation was not preserved: {other:?}"),
    }
}

#[cfg(feature = "gpui")]
mod gpui_adapter_tests {
    use super::*;
    use gpui::TestAppContext;

    #[gpui::test]
    fn gpui_global_and_trait_adapter_resolve(cx: &mut TestAppContext) {
        let provider = Rc::new(FakeAccessibilityProvider::new(
            AccessibilityPlatform::Windows,
        ));
        let installed =
            cx.update(|cx| gpui_accessibility::gpui_adapter::init_accessibility(cx, provider));
        let resolved = cx.update(|cx| {
            gpui_accessibility::gpui_adapter::accessibility_service(cx)
                .expect("installed accessibility global")
        });
        assert!(Rc::ptr_eq(&installed, &resolved));

        use gpui_accessibility::gpui_adapter::AccessibilityAppExt as _;
        let via_trait = cx.update(|cx| {
            let provider = Rc::new(FakeAccessibilityProvider::new(AccessibilityPlatform::Linux));
            let service = cx.init_accessibility(provider);
            let looked_up = cx.accessibility_service().expect("trait lookup");
            assert!(Rc::ptr_eq(&service, &looked_up));
            looked_up
        });
        assert_eq!(via_trait.platform(), AccessibilityPlatform::Linux);
    }

    #[gpui::test]
    fn gpui_system_services_adapter_resolves_without_a_dedicated_global(cx: &mut TestAppContext) {
        let from_system_services = cx.update(|cx| {
            let provider = Rc::new(FakeAccessibilityProvider::new(AccessibilityPlatform::MacOs));
            let mut services = SystemServices::new();
            gpui_accessibility::gpui_adapter::install_accessibility_service(
                &mut services,
                provider,
            )
            .expect("registration");
            gpui_system::gpui_adapter::install_system_services(services, cx);
            gpui_accessibility::gpui_adapter::accessibility_service(cx)
                .expect("system services fallback")
        });
        assert_eq!(
            from_system_services.platform(),
            AccessibilityPlatform::MacOs
        );
    }

    #[gpui::test]
    fn surface_scope_revokes_scene_and_live_state(_cx: &mut TestAppContext) {
        let provider = Rc::new(FakeAccessibilityProvider::new(
            AccessibilityPlatform::Windows,
        ));
        let service = AccessibilityService::new(provider);
        let owner = UiOwner::new(SURFACE);
        let mut commit = service.commit_scene(root_snapshot(scene(1)), ui_context(&owner));
        match poll_once(&mut commit) {
            std::task::Poll::Ready(result) => {
                completed(result);
            }
            std::task::Poll::Pending => panic!("commit did not complete"),
        }
        let scope = gpui_accessibility::gpui_adapter::AccessibilitySurfaceScope::new(
            owner,
            service.clone(),
        );
        assert_eq!(scope.surface(), SURFACE);
        assert!(service.snapshot(SURFACE).is_some());
        drop(scope);
        assert!(service.snapshot(SURFACE).is_none());
    }
}
