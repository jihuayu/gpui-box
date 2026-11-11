//! Facade feature smoke tests for the accessibility re-export.
//!
//! These tests contain no OS-specific paths or native UI assumptions. They
//! compile on Linux and Windows with the same deterministic provider.

#![cfg(feature = "accessibility")]

use gpui_box::accessibility::{
    AcceptanceStatus, AccessibilityAcceptanceReport, AccessibilityPlatform, AccessibilityService,
    FakeAccessibilityProvider, SceneRevision, SemanticIdentity,
};
use std::rc::Rc;

#[test]
fn accessibility_reexport_exposes_semantic_service_types() {
    let provider = Rc::new(FakeAccessibilityProvider::new(
        AccessibilityPlatform::Windows,
    ));
    let service = AccessibilityService::new(provider);
    assert_eq!(service.platform(), AccessibilityPlatform::Windows);
    assert_eq!(
        service
            .capability(gpui_box::accessibility::AccessibilityOperation::ObserveTree)
            .operation,
        gpui_box::accessibility::AccessibilityOperation::ObserveTree
    );
    let report = AccessibilityAcceptanceReport::new(AccessibilityPlatform::Windows);
    assert_eq!(report.status(), AcceptanceStatus::PendingAutomation);
    assert_eq!(SceneRevision(4).next(), Some(SceneRevision(5)));
    assert_eq!(SemanticIdentity::at(1u64).node_id, 1);
    assert_eq!(gpui_box::accessibility::ACCESSKIT_VERSION, "0.24.0");
}

#[cfg(feature = "accessibility-gpui")]
#[test]
fn accessibility_gpui_feature_exposes_adapter_and_system_registration() {
    fn assert_adapter_symbols() {
        let _init = gpui_box::accessibility::gpui_adapter::init_accessibility;
        let _lookup = gpui_box::accessibility::gpui_adapter::accessibility_service;
        let _scope = gpui_box::accessibility::gpui_adapter::AccessibilitySurfaceScope::new;
    }
    assert_adapter_symbols();

    let provider = Rc::new(FakeAccessibilityProvider::new(AccessibilityPlatform::Linux));
    let service = AccessibilityService::new(provider);
    let mut services = gpui_box::system::SystemServices::new();
    service
        .clone()
        .install(&mut services)
        .expect("registration");
    let registered = services
        .service::<AccessibilityService>()
        .expect("registered service");
    assert!(service.shares_state_with(&registered));
}
