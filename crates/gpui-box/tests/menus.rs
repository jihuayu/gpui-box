//! Facade feature smoke tests for the menus re-export.
//!
//! These tests use no OS-specific paths or native UI. They compile on Linux and
//! Windows with the same deterministic provider.

#![cfg(feature = "menus")]

use gpui_box::menus::{
    CommandDescriptor, CommandId, CommandScope, ExecutionDomain, FakeMenuProvider,
    MenuCommandService, MenuIssueLinks, MenuPlatform, MenusOperation, Support,
};
use std::rc::Rc;

#[test]
fn menus_reexport_exposes_typed_commands_and_matrix() {
    let provider = Rc::new(FakeMenuProvider::new());
    let service = MenuCommandService::new(provider);
    assert_eq!(service.capabilities().len(), 5);
    let descriptor = CommandDescriptor::ui(CommandId::new("file.save").expect("id"), "Save")
        .expect("descriptor");
    assert_eq!(descriptor.domain, ExecutionDomain::Ui);
    let scope = service.create_scope(CommandScope::Global).expect("scope");
    let registration = service
        .register_ui_in(&scope, descriptor, |_: (), _| Ok(()))
        .expect("registration");
    assert!(service.is_registered(&CommandId::new("file.save").expect("id")));
    assert!(registration.lease().is_active());
    assert_eq!(gpui_box::menus::platform_support_matrix().len(), 30);
    assert_eq!(MenuPlatform::ALL.len(), 6);
    assert_eq!(MenusOperation::ALL.len(), 5);
    assert_eq!(MenuIssueLinks::default().components, "#3");
    assert_eq!(
        gpui_box::menus::UPSTREAM_GPUI_REVISION,
        gpui_box::UPSTREAM_REVISION
    );
    assert_eq!(gpui_box::menus::PACKAGE_VERSION, gpui_box::VERSION);
    assert_eq!(
        gpui_box::menus::operation_support(MenuPlatform::Android, MenusOperation::InstallMenuBar)
            .support,
        Support::Unsupported
    );
}

#[cfg(feature = "menus-gpui")]
#[test]
fn menus_gpui_feature_exposes_adapter_symbols() {
    fn assert_symbols() {
        let _init = gpui_box::menus::gpui_adapter::init_menus;
        let _lookup = gpui_box::menus::gpui_adapter::menus_service;
        let _scope = gpui_box::menus::gpui_adapter::MenuContributionScope::new;
        let _run = gpui_box::menus::gpui_adapter::run_command_request::<()>;
    }
    assert_symbols();
}
