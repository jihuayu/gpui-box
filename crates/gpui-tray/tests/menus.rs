mod support;

use gpui_menus::{
    CommandDescriptor, CommandScope, ContextExpr, ContextKey, ContextValue, MenuNode,
    RegistrationLease, RouteRegistry, SystemActionBinding,
};
use gpui_tray::*;
use std::{cell::Cell, rc::Rc};
use support::*;

type Setup = (
    FakeTrayProvider,
    TrayService,
    MenuCommandService,
    Rc<Cell<u32>>,
    StatusItemLease,
    UiOwner,
    RegistrationLease,
);

fn setup(menu_nodes: Vec<MenuNode>) -> Setup {
    let (fake, service, _provider, menus) = fake_service_with_menus();
    let runs = Rc::new(Cell::new(0));
    let registration = register_command(&menus, "app.quit", runs.clone());
    let (owner, context) = ui_context(3);
    service.attach_surface(SurfaceId(3), 1, true);
    let spec = StatusItemSpec::new(key("app.status"), icon_set())
        .with_title("Status")
        .with_accessibility_label("Application status")
        .with_menu(ContextMenu::new(menu_nodes));
    let lease = completed(service.create_surface_status_item(spec, SurfaceId(3), 1, &context));
    (fake, service, menus, runs, lease, owner, registration)
}

#[test]
fn attached_menu_is_resolved_live_and_menu_clicks_dispatch_through_menus() {
    let command = command_id("app.quit");
    let (fake, service, _menus, runs, lease, _owner, _registration) =
        setup(vec![MenuNode::command(command.clone())]);
    let (_owner, context) = ui_context(3);
    let resolved = completed(service.present_status_menu(lease.id(), &context));
    assert_eq!(resolved.all_items().len(), 1);
    assert!(resolved.all_items()[0].enabled);
    assert_eq!(fake.menu_calls().len(), 1);
    assert!(fake.simulate_menu_item_click(lease.id(), command));
    assert_eq!(runs.get(), 1);
}

#[test]
fn menu_disabled_state_is_reevaluated_at_click_time() {
    let command = command_id("app.quit");
    let enabled_key = ContextKey::new("tray.enabled").expect("key");
    let node = MenuNode::command(command.clone()).enabled_when(ContextExpr::equals(
        enabled_key.clone(),
        ContextValue::Bool(true),
    ));
    let (fake, service, menus, runs, lease, _owner, _registration) = setup(vec![node]);
    let (_owner, context) = ui_context(3);
    menus.set_global_context(enabled_key, ContextValue::Bool(false));
    let resolved = completed(service.present_status_menu(lease.id(), &context));
    assert!(!resolved.all_items()[0].enabled, "disabled live menu item");
    let outcome = completed(service.handle_menu_item_click_event(&TrayMenuItemClick {
        item_id: lease.id(),
        native_item_token: lease.native_item_token(),
        shell_epoch: lease.shell_epoch(),
        command_id: command,
    }));
    assert_eq!(
        outcome,
        TrayClickOutcome::Ignored(StaleCallbackReason::DisabledOrHidden)
    );
    assert_eq!(runs.get(), 0);
    assert!(!fake.menu_calls().is_empty());
}

#[test]
fn menu_hidden_state_and_checkmarks_reflect_live_context() {
    let command = command_id("app.quit");
    let checked_key = ContextKey::new("tray.checked").expect("key");
    let hidden_key = ContextKey::new("tray.hidden").expect("key");
    let node = MenuNode::check(command.clone(), checked_key.clone()).visible_when(
        ContextExpr::equals(hidden_key.clone(), ContextValue::Bool(false)),
    );
    let (_fake, service, menus, _runs, lease, _owner, _registration) = setup(vec![node]);
    let (_owner, context) = ui_context(3);
    menus.set_global_context(checked_key, ContextValue::Bool(true));
    menus.set_global_context(hidden_key.clone(), ContextValue::Bool(true));
    let resolved = completed(service.present_status_menu(lease.id(), &context));
    let item = resolved.all_items()[0];
    assert!(!item.visible);
    let outcome = completed(service.handle_menu_item_click_event(&TrayMenuItemClick {
        item_id: lease.id(),
        native_item_token: lease.native_item_token(),
        shell_epoch: lease.shell_epoch(),
        command_id: command,
    }));
    assert_eq!(
        outcome,
        TrayClickOutcome::Ignored(StaleCallbackReason::DisabledOrHidden)
    );

    menus.set_global_context(hidden_key, ContextValue::Bool(false));
    let resolved = completed(service.present_status_menu(lease.id(), &context));
    let item = resolved.all_items()[0];
    assert!(item.visible);
    assert_eq!(item.checked, Some(true));
}

#[test]
fn click_triggers_invoke_registered_commands_and_system_actions() {
    let (fake, service, _provider, menus) = fake_service_with_menus();
    let runs = Rc::new(Cell::new(0));
    let _registration = register_command(&menus, "app.restore", runs.clone());
    let (owner, context) = ui_context(4);
    service.attach_surface(SurfaceId(4), 1, true);
    let spec = StatusItemSpec::new(key("app.status"), icon_set())
        .with_title("Status")
        .with_accessibility_label("Application status")
        .with_click_action(
            TrayClickTrigger::PrimaryClick,
            TrayClickAction::invoke_command(command_id("app.restore")),
        );
    let lease = completed(service.create_surface_status_item(spec, SurfaceId(4), 1, &context));
    let command = command_id("app.restore");
    let scope = menus.create_scope(CommandScope::Global).expect("scope");
    let binding = SystemActionBinding::action("tray.restore", command.clone()).expect("binding");
    let _system_lease = menus
        .register_system_action(&scope, binding, &RouteRegistry::default())
        .expect("register action");
    assert!(fake.simulate_click(lease.id(), TrayClickTrigger::PrimaryClick));
    assert_eq!(runs.get(), 1);

    let spec = StatusItemSpec::new(key("app.status"), icon_set())
        .with_title("Status")
        .with_accessibility_label("Application status")
        .with_click_action(
            TrayClickTrigger::DoubleClick,
            TrayClickAction::dispatch_system_action("tray.restore").expect("action"),
        );
    completed(service.update_status_item(lease.id(), spec, &context));
    assert!(fake.simulate_click(lease.id(), TrayClickTrigger::DoubleClick));
    assert_eq!(runs.get(), 2);
    drop(owner);
}

#[test]
fn non_ui_thread_contexts_are_rejected_before_menu_presentation() {
    let command = command_id("app.quit");
    let (fake, service, _menus, _runs, lease, _owner, _registration) =
        setup(vec![MenuNode::command(command)]);
    let (sender, receiver) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let owner = UiOwner::new(SurfaceId(99));
        let context = owner.context(None).expect("foreign context");
        sender.send((owner, context)).expect("send context");
    });
    let (_foreign_owner, foreign) = receiver.recv().expect("foreign context received");
    let context = OperationContext {
        ui: Some(foreign),
        ..OperationContext::default()
    };
    let error = service
        .present_status_menu(lease.id(), &context)
        .expect_err("foreign thread context");
    assert_eq!(error.code, ErrorCode::ContextExpired);
    assert!(fake.menu_calls().is_empty());
}

#[test]
fn disabled_click_action_is_rejected_by_the_live_command_service() {
    let (fake, service, _provider, menus) = fake_service_with_menus();
    let runs: Rc<Cell<u32>> = Rc::new(Cell::new(0));
    let enabled_key = ContextKey::new("tray.command.enabled").expect("key");
    let descriptor = CommandDescriptor::ui(command_id("app.restore"), "Restore")
        .expect("descriptor")
        .enabled_when(ContextExpr::equals(
            enabled_key.clone(),
            ContextValue::Bool(true),
        ));
    let captured = runs.clone();
    let _registration = menus
        .register_ui(descriptor, move |_: (), _| {
            captured.set(captured.get().saturating_add(1));
            Ok(())
        })
        .expect("registration");
    let (_owner, context) = ui_context(5);
    service.attach_surface(SurfaceId(5), 1, true);
    let spec = StatusItemSpec::new(key("app.status"), icon_set())
        .with_title("Status")
        .with_accessibility_label("Application status")
        .with_click_action(
            TrayClickTrigger::PrimaryClick,
            TrayClickAction::invoke_command(command_id("app.restore")),
        );
    let lease = completed(service.create_surface_status_item(spec, SurfaceId(5), 1, &context));
    menus.set_global_context(enabled_key, ContextValue::Bool(false));
    let hosted = fake.find_hosted_by_item(lease.id()).expect("hosted");
    let event = TrayClickEvent {
        item_id: lease.id(),
        native_item_token: hosted.native_item_token,
        shell_epoch: hosted.shell_epoch,
        trigger: TrayClickTrigger::PrimaryClick,
    };
    let error = service
        .handle_click_event(&event)
        .expect_err("disabled command");
    assert_eq!(error.code, ErrorCode::InvalidInput);
    assert_eq!(runs.get(), 0);
}

fn application_spec(name: &str, quit: &gpui_menus::CommandId, action_id: &str) -> StatusItemSpec {
    StatusItemSpec::new(key(name), icon_set())
        .with_title("Status")
        .with_accessibility_label("Application status")
        .with_menu(ContextMenu::new(vec![MenuNode::command(quit.clone())]))
        .with_click_action(
            TrayClickTrigger::PrimaryClick,
            TrayClickAction::invoke_command(quit.clone()),
        )
        .with_click_action(
            TrayClickTrigger::DoubleClick,
            TrayClickAction::dispatch_system_action(action_id).expect("valid system action"),
        )
}

#[test]
fn application_items_fall_back_to_the_internal_ui_owner_after_creator_surface_disposal() {
    let (fake, service, _provider, menus) = fake_service_with_menus();
    let runs: Rc<Cell<u32>> = Rc::new(Cell::new(0));
    let quit = command_id("app.quit");
    let _registration = register_command(&menus, "app.quit", runs.clone());
    let scope = menus.create_scope(CommandScope::Global).expect("scope");
    let _system = menus
        .register_system_action(
            &scope,
            SystemActionBinding::action("tray.quit", quit.clone()).expect("binding"),
            &RouteRegistry::default(),
        )
        .expect("system action");
    service.set_residency_policy(
        TrayResidencyPolicy::new()
            .with_window_close_policy(WindowClosePolicy::MinimizeToTrayWhenAvailable)
            .with_quit_command(quit.clone())
            .with_quit_system_action("tray.quit")
            .expect("policy action"),
    );

    let default_item = completed(service.create_application_status_item(
        application_spec("app.default", &quit, "tray.quit"),
        &OperationContext::default(),
    ));
    let (owner, window_context) = ui_context(21);
    let disposed_item = completed(service.create_application_status_item(
        application_spec("app.disposed", &quit, "tray.quit"),
        &window_context,
    ));
    service.dispose_surface(SurfaceId(21));
    owner.dispose();
    assert!(default_item.is_active());
    assert!(disposed_item.is_active());

    service.attach_surface(SurfaceId(22), 1, true);
    assert_eq!(
        service.evaluate_close(SurfaceId(22), 1),
        CloseInterceptionOutcome::HideWindowToTray {
            item_id: default_item.id()
        }
    );
    assert!(service.residing_in_tray());

    let default_context = OperationContext::default();
    assert_eq!(
        completed(service.present_status_menu(default_item.id(), &default_context))
            .all_items()
            .len(),
        1
    );
    assert_eq!(
        completed(service.present_status_menu(disposed_item.id(), &default_context))
            .all_items()
            .len(),
        1
    );
    assert_eq!(fake.menu_calls().len(), 2);

    assert!(fake.simulate_menu_item_click(disposed_item.id(), quit.clone()));
    assert_eq!(runs.get(), 1);
    assert!(fake.simulate_click(default_item.id(), TrayClickTrigger::PrimaryClick));
    assert_eq!(runs.get(), 2);
    assert!(fake.simulate_click(disposed_item.id(), TrayClickTrigger::DoubleClick));
    assert_eq!(runs.get(), 3);
}

#[test]
fn application_items_reject_foreign_and_disposed_explicit_ui_contexts() {
    let (fake, service, _provider, menus) = fake_service_with_menus();
    let quit = command_id("app.quit");
    let _registration = register_command(&menus, "app.quit", Rc::new(Cell::new(0)));

    let (owner, disposed_context) = ui_context(23);
    owner.dispose();
    let error = match service.create_application_status_item(
        application_spec("app.disposed", &quit, "tray.quit"),
        &disposed_context,
    ) {
        Ok(_) => panic!("disposed UI context must be rejected"),
        Err(error) => error,
    };
    assert_eq!(error.code, ErrorCode::ContextExpired);
    assert_eq!(fake.create_call_count(), 0);

    let (sender, receiver) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let owner = UiOwner::new(SurfaceId(24));
        let context = owner.context(None).expect("foreign context");
        sender.send((owner, context)).expect("send context");
    });
    let (_foreign_owner, foreign) = receiver.recv().expect("foreign context received");
    let foreign_context = OperationContext {
        ui: Some(foreign),
        ..OperationContext::default()
    };
    let error = match service.create_application_status_item(
        application_spec("app.foreign", &quit, "tray.quit"),
        &foreign_context,
    ) {
        Ok(_) => panic!("foreign UI context must be rejected"),
        Err(error) => error,
    };
    assert_eq!(error.code, ErrorCode::ContextExpired);
    assert_eq!(fake.create_call_count(), 0);

    let item = completed(service.create_application_status_item(
        application_spec("app.status", &quit, "tray.quit"),
        &OperationContext::default(),
    ));
    let error = match service.present_status_menu(item.id(), &foreign_context) {
        Ok(_) => panic!("foreign presentation context must be rejected"),
        Err(error) => error,
    };
    assert_eq!(error.code, ErrorCode::ContextExpired);
    assert!(fake.menu_calls().is_empty());
}
