mod common;

use common::*;
use gpui_menus::{
    BrowserShortcutPolicy, CommandDescriptor, CommandId, CommandScope, Completion, Constraint,
    ContextMenu, ErrorCode, Evidence, FakeMenuProvider, KeychordBinding, KeystrokeSpec,
    LogicalRect, MenuBar, MenuCommandService, MenuIssueLinks, MenuNode, MenuPlatform,
    MenusOperation, Modifiers, Support, SystemError, browser_shortcut_allowed,
    browser_shortcut_policy, is_browser_reserved_shortcut,
};

fn command(value: &str) -> CommandId {
    CommandId::new(value).expect("id")
}

#[test]
fn six_platform_five_operation_matrix_has_thirty_rows_and_owner_links() {
    let matrix = gpui_menus::platform_support_matrix();
    assert_eq!(matrix.len(), 30);
    assert_eq!(MenuPlatform::ALL.len(), 6);
    assert_eq!(MenusOperation::ALL.len(), 5);
    for platform in MenuPlatform::ALL {
        assert_eq!(gpui_menus::platform_support(platform).len(), 5);
        for operation in MenusOperation::ALL {
            let row = gpui_menus::operation_support(platform, operation);
            assert_eq!(row.platform, platform);
            assert_eq!(row.operation, operation);
            assert_eq!(row.permission, gpui_menus::PermissionState::NotRequired);
            assert_eq!(row.links, MenuIssueLinks::default());
            assert_eq!(row.links.as_array(), MenuIssueLinks::ALL);
            assert!(!row.required_host_configuration.is_empty());
            assert!(!row.runtime_evidence.is_empty());
            assert!(!row.notes.is_empty());
            let capability = row.capability(7);
            assert_eq!(capability.operation, operation);
            assert_eq!(capability.observed_revision, 7);
        }
    }
    for platform in [MenuPlatform::Android, MenuPlatform::Ios, MenuPlatform::Web] {
        let install = gpui_menus::operation_support(platform, MenusOperation::InstallMenuBar);
        assert_eq!(install.support, Support::Unsupported);
        assert!(!install.is_supported());
        let sync = gpui_menus::operation_support(platform, MenusOperation::SyncKeybindings);
        assert!(sync.constraints.contains(&Constraint::Hardware));
        assert_eq!(install.evidence, Evidence::ContractOnly);
    }
    assert_eq!(
        gpui_menus::operation_support(MenuPlatform::Windows, MenusOperation::InstallMenuBar)
            .evidence,
        Evidence::Compiled
    );
    assert_eq!(
        gpui_menus::operation_support(MenuPlatform::Linux, MenusOperation::InstallMenuBar).support,
        Support::Conditional
    );
}

#[test]
fn browser_shortcut_policy_reserves_w_t_n_l_q() {
    let policy: BrowserShortcutPolicy = browser_shortcut_policy();
    assert_eq!(policy.reserved_shortcuts.len(), 5);
    assert!(policy.hardware_keyboard_supported);
    for key in ["w", "t", "n", "l", "q"] {
        assert!(is_browser_reserved_shortcut(
            &KeystrokeSpec::new(key, Modifiers::command()).expect("chord")
        ));
        assert!(is_browser_reserved_shortcut(
            &KeystrokeSpec::new(key, Modifiers::control()).expect("chord")
        ));
        assert!(!browser_shortcut_allowed(
            &KeystrokeSpec::new(key, Modifiers::command()).expect("chord")
        ));
    }
    assert!(browser_shortcut_allowed(
        &KeystrokeSpec::new("s", Modifiers::command()).expect("chord")
    ));
    assert!(gpui_menus::supports_browser_shortcut_policy(
        MenuPlatform::Web
    ));
    assert!(!gpui_menus::supports_browser_shortcut_policy(
        MenuPlatform::Linux
    ));
}

#[test]
fn fake_provider_rejects_stale_generations_and_records_attempts() {
    let provider = FakeMenuProvider::new();
    let surface = gpui_menus::SurfaceId(9);
    provider.set_surface_generation(surface, 2);
    assert_eq!(
        provider
            .record_callback(surface, 1)
            .expect_err("stale callback")
            .code,
        ErrorCode::ContextExpired
    );
    assert_eq!(provider.stale_callback_rejections(), vec![(surface, 1)]);
    assert_eq!(provider.callback_attempts().len(), 1);
    assert!(provider.record_callback(surface, 2).is_ok());
    assert_eq!(provider.callback_attempts().len(), 2);
    assert_eq!(provider.stale_callback_rejections().len(), 1);
}

#[test]
fn provider_records_menu_bar_context_menu_and_keybinding_sync() {
    let (owner, context) = ui_context(10);
    let provider = std::rc::Rc::new(FakeMenuProvider::new());
    let service = MenuCommandService::new(provider.clone());
    let scope = service.create_scope(CommandScope::Global).expect("scope");
    let _registration = service
        .register_ui_in(
            &scope,
            CommandDescriptor::ui(command("file.save"), "Save").expect("descriptor"),
            |_: (), _| Ok(()),
        )
        .expect("command");
    let _keybinding_lease = service
        .register_keybinding(
            &scope,
            KeychordBinding::new(
                KeystrokeSpec::new("s", Modifiers::command()).expect("chord"),
                command("file.save"),
            ),
        )
        .expect("keybinding");
    let bar = MenuBar::new().with_menu(
        gpui_menus::TopLevelMenu::new("File")
            .with_nodes(vec![MenuNode::command(command("file.save"))]),
    );
    let Completion::Completed(resolved) = service
        .install_menu_bar(&scope, bar, context.clone())
        .expect("install")
    else {
        panic!("resolved bar");
    };
    assert_eq!(provider.installed_menu_bars(), vec![resolved]);

    let anchor = LogicalRect {
        x: 4.0,
        y: 8.0,
        width: 120.0,
        height: 32.0,
    };
    let context_menu = ContextMenu::new(vec![MenuNode::command(command("file.save"))]);
    let menu_context =
        gpui_menus::context_menu_context(&owner, anchor).expect("context menu context");
    let presented_menu = match service
        .present_context_menu(&scope, context_menu, menu_context)
        .expect("present request")
    {
        Completion::Completed(menu) => menu,
        Completion::Cancelled(reason) => panic!("context menu cancelled: {reason:?}"),
    };
    assert_eq!(presented_menu.surface, Some(gpui_menus::SurfaceId(10)));
    assert_eq!(presented_menu.generation, Some(1));
    let presented = provider.presented_context_menus();
    assert_eq!(presented.len(), 1);
    assert_eq!(presented[0].surface, gpui_menus::SurfaceId(10));
    assert_eq!(presented[0].generation, 1);
    assert_eq!(presented[0].anchor, Some(anchor));

    let Completion::Completed(bindings) = service.sync_keybindings(context).expect("sync") else {
        panic!("sync cancelled");
    };
    assert_eq!(bindings.len(), 1);
    assert_eq!(bindings[0].command_id, command("file.save"));
    assert_eq!(provider.synced_keybindings().len(), 1);
}

#[test]
fn provider_failure_injection_revokes_the_failed_registration() {
    let (_owner, context) = ui_context(11);
    let provider = std::rc::Rc::new(FakeMenuProvider::new());
    provider.fail_operation(
        MenusOperation::InstallMenuBar,
        SystemError::new(ErrorCode::PlatformFailure, "install failed"),
    );
    let service = MenuCommandService::new(provider.clone());
    let scope = service.create_scope(CommandScope::Global).expect("scope");
    let error = service
        .install_menu_bar(&scope, MenuBar::new(), context.clone())
        .expect_err("install failure");
    assert_eq!(error.code, ErrorCode::PlatformFailure);
    assert!(service.registered_menu_bars().is_empty());
    provider.clear_failure(MenusOperation::InstallMenuBar);
    provider.fail_operation(
        MenusOperation::PresentContextMenu,
        SystemError::new(ErrorCode::PlatformFailure, "present failed"),
    );
    let error = service
        .present_context_menu(&scope, ContextMenu::new(Vec::new()), context)
        .expect_err("present failure");
    assert_eq!(error.code, ErrorCode::PlatformFailure);
    assert!(service.registered_context_menus().is_empty());
}

#[test]
fn capability_overrides_gate_every_menu_operation() {
    let (_owner, context) = ui_context(12);
    let provider = std::rc::Rc::new(FakeMenuProvider::new());
    let service = MenuCommandService::new(provider.clone());
    let capabilities = service.capabilities();
    assert_eq!(capabilities.len(), 5);
    for operation in MenusOperation::ALL {
        assert_eq!(
            capabilities
                .iter()
                .find(|capability| capability.operation == operation)
                .expect("operation capability")
                .operation,
            operation
        );
    }
    provider.set_operation_unsupported(MenusOperation::SyncKeybindings);
    let error = service
        .sync_keybindings(context.clone())
        .expect_err("sync unsupported");
    assert_eq!(error.code, ErrorCode::Unsupported);
    provider.set_support(MenusOperation::SyncKeybindings, Support::Native);
    provider.set_availability(
        MenusOperation::SyncKeybindings,
        gpui_menus::Availability::Available,
    );
    let Completion::Completed(bindings) = service.sync_keybindings(context).expect("sync restored")
    else {
        panic!("sync cancelled");
    };
    assert!(bindings.is_empty());
}
