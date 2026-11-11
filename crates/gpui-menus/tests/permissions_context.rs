mod common;

use common::*;
use gpui_menus::{
    ActivationEvent, ActivationId, ActivationPayload, ActivationSource, ActivationTarget,
    ButtonCommandBinding, CommandDescriptor, CommandId, CommandPayload, CommandScope, Completion,
    ContextExpr, ContextKey, ContextValue, FakeMenuProvider, FakePermissionProvider,
    InvocationContext, KeychordBinding, KeystrokeSpec, MenuBar, MenuCommandEvent,
    MenuCommandService, MenuNode, PermissionKind, PermissionStatus, ResolvedMenuItem,
    RouteRegistry, SystemActionBinding, TopLevelMenu,
};
use serde::{Deserialize, Serialize};
use std::{cell::RefCell, collections::BTreeMap as Map, rc::Rc, time::SystemTime};

fn command(value: &str) -> CommandId {
    CommandId::new(value).expect("id")
}

fn key(value: &str) -> ContextKey {
    ContextKey::new(value).expect("key")
}

fn all_surface_requests(
    service: &MenuCommandService,
    item: &ResolvedMenuItem,
    button: &ButtonCommandBinding,
    chord: &KeystrokeSpec,
    context: &InvocationContext,
) -> Vec<gpui_system::Request<CommandPayload>> {
    vec![
        service.activate_menu_item(item, context.clone()),
        service.dispatch_keychord(chord, context.clone()),
        service.invoke_palette_entry(
            &expect_completed(
                service.query_palette(gpui_menus::PaletteQuery::new("notify"), context.clone()),
            )
            .into_iter()
            .next()
            .expect("palette entry"),
            context.clone(),
        ),
        service.invoke_button(button, context.clone()),
        service.dispatch_system_action("app.notify", &Map::new(), context.clone()),
    ]
}

#[test]
fn live_permissions_and_context_are_revalidated_across_all_five_surfaces() {
    let owner = gpui_menus::UiOwner::new(gpui_menus::SurfaceId(51));
    let operation = gpui_menus::OperationContext {
        ui: Some(owner.context(None).expect("ui")),
        ..gpui_menus::OperationContext::default()
    };
    let context = InvocationContext::new(operation);
    let focused = key("editor.focused");
    let permissions = Rc::new(
        FakePermissionProvider::new()
            .with_status(PermissionKind::Notifications, PermissionStatus::Granted),
    );
    let provider = Rc::new(FakeMenuProvider::new());
    let service = MenuCommandService::with_permissions(provider, permissions.clone());
    service.set_global_context(focused.clone(), ContextValue::Bool(true));
    let runs = Rc::new(RefCell::new(0_u32));
    let captured = runs.clone();
    let descriptor = CommandDescriptor::ui(command("app.notify"), "Notify")
        .expect("descriptor")
        .require_permission(PermissionKind::Notifications)
        .enabled_when(ContextExpr::equals(
            focused.clone(),
            ContextValue::Bool(true),
        ));
    let scope = service.create_scope(CommandScope::Global).expect("scope");
    let registration = service
        .register_ui_in(&scope, descriptor, move |_: (), _| {
            *captured.borrow_mut() += 1;
            Ok("ok".to_owned())
        })
        .expect("registration");

    let bar = MenuBar::new().with_menu(
        TopLevelMenu::new("File").with_nodes(vec![MenuNode::command(command("app.notify"))]),
    );
    let Completion::Completed(menu) = service
        .install_menu_bar(&scope, bar, context.clone())
        .expect("install")
    else {
        panic!("menu bar must resolve");
    };
    let item = menu
        .find_item(&command("app.notify"))
        .expect("menu item")
        .clone();
    let chord = KeystrokeSpec::plain("n").expect("chord");
    let _keybinding_lease = service
        .register_keybinding(
            &scope,
            KeychordBinding::new(chord.clone(), command("app.notify")),
        )
        .expect("keybinding");
    let button = ButtonCommandBinding::new(command("app.notify"));
    let _button_lease = service
        .register_button(&scope, button.clone())
        .expect("button");
    let _system_action_lease = service
        .register_system_action(
            &scope,
            SystemActionBinding::action("app.notify", command("app.notify")).expect("action"),
            &RouteRegistry::default(),
        )
        .expect("system action");

    assert!(item.enabled, "menu item starts enabled");
    assert!(
        service
            .project_button(&button, &context)
            .expect("button state")
            .enabled
    );

    permissions.set_status(
        PermissionKind::Notifications,
        PermissionStatus::Denied {
            can_request_again: false,
        },
    );
    for request in all_surface_requests(&service, &item, &button, &chord, &context) {
        assert_eq!(
            block_on(request).expect_err("revoked permission").code,
            gpui_menus::ErrorCode::PermissionDenied
        );
    }
    assert_eq!(*runs.borrow(), 0);
    assert!(
        !service
            .project_button(&button, &context)
            .expect("button state")
            .enabled
    );

    permissions.set_status(PermissionKind::Notifications, PermissionStatus::Granted);
    let before = service.context_revision();
    service.set_global_context(focused.clone(), ContextValue::Bool(false));
    assert!(service.context_revision() > before);
    for request in all_surface_requests(&service, &item, &button, &chord, &context) {
        assert_eq!(
            block_on(request).expect_err("reactive disabled").code,
            gpui_menus::ErrorCode::InvalidInput
        );
    }
    assert_eq!(*runs.borrow(), 0);
    let palette = expect_completed(
        service.query_palette(gpui_menus::PaletteQuery::new("notify"), context.clone()),
    );
    assert_eq!(palette.len(), 1);
    assert!(!palette[0].enabled);
    assert!(palette[0].disabled_reason.is_some());

    service.set_global_context(focused, ContextValue::Bool(true));
    for request in all_surface_requests(&service, &item, &button, &chord, &context) {
        assert_eq!(
            block_on(request).expect("enabled invocation"),
            Completion::Completed(CommandPayload::json("ok"))
        );
    }
    assert_eq!(*runs.borrow(), 5);
    assert!(registration.lease().is_active());
    drop(owner);
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct OpenRequest {
    name: String,
}

#[test]
fn activation_routes_are_validated_and_map_parameters_to_typed_payloads() {
    let (_owner, context) = ui_context(52);
    let service = MenuCommandService::from_provider(FakeMenuProvider::new());
    let scope = service.create_scope(CommandScope::Global).expect("scope");
    let descriptor = CommandDescriptor::ui(command("file.open"), "Open").expect("descriptor");
    let registration = service
        .register_ui_in(&scope, descriptor, |input: OpenRequest, _| {
            Ok(format!("opened:{}", input.name))
        })
        .expect("registration");
    let mut routes = RouteRegistry::default();
    routes.register("open.file", 1).expect("registered route");
    let target = ActivationTarget::new(
        "open.file",
        1,
        Map::from([("name".to_owned(), "document".to_owned())]),
    )
    .expect("target");
    let _action_lease = service
        .register_system_action(
            &scope,
            SystemActionBinding::activation(target.clone(), command("file.open"))
                .parameter("name", "name"),
            &routes,
        )
        .expect("binding");
    let event = ActivationEvent::new(
        ActivationId::new("activation-1").expect("activation id"),
        ActivationSource::WarmLaunch,
        ActivationPayload::Route(target),
        SystemTime::now(),
    )
    .expect("event");
    assert_eq!(
        block_on(service.dispatch_activation(&event, &routes, context.clone()))
            .expect("activation"),
        Completion::Completed(CommandPayload::json("opened:document"))
    );

    let missing = ActivationTarget::new("missing.route", 1, Map::new()).expect("shape");
    let invalid_event = ActivationEvent::new(
        ActivationId::new("activation-2").expect("id"),
        ActivationSource::WarmLaunch,
        ActivationPayload::Route(missing),
        SystemTime::now(),
    )
    .expect("event");
    assert_eq!(
        block_on(service.dispatch_activation(&invalid_event, &routes, context))
            .expect_err("unregistered route")
            .code,
        gpui_menus::ErrorCode::InvalidConfiguration
    );
    assert!(registration.lease().is_active());
}

#[test]
fn activation_invocation_rechecks_a_revoked_permission() {
    let (_owner, context) = ui_context(53);
    let permissions = Rc::new(
        FakePermissionProvider::new()
            .with_status(PermissionKind::Notifications, PermissionStatus::Granted),
    );
    let service =
        MenuCommandService::with_permissions(Rc::new(FakeMenuProvider::new()), permissions.clone());
    let scope = service.create_scope(CommandScope::Global).expect("scope");
    let descriptor = CommandDescriptor::ui(command("app.notify"), "Notify")
        .expect("descriptor")
        .require_permission(PermissionKind::Notifications);
    let _registration = service
        .register_ui_in(&scope, descriptor, |_: (), _| Ok("sent".to_owned()))
        .expect("registration");
    let mut routes = RouteRegistry::default();
    routes.register("notify.now", 2).expect("route");
    let target = ActivationTarget::new("notify.now", 2, Map::new()).expect("target");
    let _action_lease = service
        .register_system_action(
            &scope,
            SystemActionBinding::activation(target.clone(), command("app.notify")),
            &routes,
        )
        .expect("binding");
    let event = ActivationEvent::new(
        ActivationId::new("activation-3").expect("id"),
        ActivationSource::Notification,
        ActivationPayload::Notification {
            notification_id: ActivationId::new("notification-1").expect("id"),
            target,
        },
        SystemTime::now(),
    )
    .expect("event");
    assert_eq!(
        block_on(service.dispatch_activation(&event, &routes, context.clone())).expect("granted"),
        Completion::Completed(CommandPayload::json("sent"))
    );
    permissions.set_status(
        PermissionKind::Notifications,
        PermissionStatus::Denied {
            can_request_again: false,
        },
    );
    assert_eq!(
        block_on(service.dispatch_activation(&event, &routes, context))
            .expect_err("revoked")
            .code,
        gpui_menus::ErrorCode::PermissionDenied
    );
}

#[test]
fn capability_gating_rejects_disabled_menu_operations_before_side_effects() {
    let (_owner, context) = ui_context(54);
    let provider = Rc::new(FakeMenuProvider::new());
    provider.set_operation_unsupported(gpui_menus::MenusOperation::InstallMenuBar);
    let service = MenuCommandService::new(provider.clone());
    let scope = service.create_scope(CommandScope::Global).expect("scope");
    let bar = MenuBar::new().with_menu(
        TopLevelMenu::new("File").with_nodes(vec![MenuNode::command(command("file.save"))]),
    );
    let error = service
        .install_menu_bar(&scope, bar, context)
        .expect_err("unsupported");
    assert_eq!(error.code, gpui_menus::ErrorCode::Unsupported);
    assert!(provider.installed_menu_bars().is_empty());
    assert!(service.registered_menu_bars().is_empty());
}

#[test]
fn unregistered_menu_commands_are_projected_as_disabled() {
    let (_owner, context) = ui_context(55);
    let service = MenuCommandService::from_provider(FakeMenuProvider::new());
    let scope = service.create_scope(CommandScope::Global).expect("scope");
    let bar = MenuBar::new().with_menu(
        TopLevelMenu::new("File").with_nodes(vec![MenuNode::command(command("file.missing"))]),
    );
    let Completion::Completed(resolved) = service
        .install_menu_bar(&scope, bar, context.clone())
        .expect("install")
    else {
        panic!("resolved");
    };
    let item = resolved.find_item(&command("file.missing")).expect("item");
    assert!(!item.enabled);
    assert!(!item.visible || item.disabled_reason.is_some());
    assert_eq!(
        block_on(service.activate_menu_item(item, context))
            .expect_err("missing command")
            .code,
        gpui_menus::ErrorCode::InvalidInput
    );
}

#[test]
fn menu_events_are_bounded_and_ordered_for_sync_invocations() {
    let (_owner, context) = ui_context(56);
    let service = MenuCommandService::from_provider(FakeMenuProvider::new());
    let descriptor = CommandDescriptor::ui(command("app.ping"), "Ping").expect("descriptor");
    let registration = service
        .register_ui(descriptor, |_: (), _| Ok(()))
        .expect("registration");
    let subscription = service.subscribe();
    let result = block_on(service.invoke_typed(registration.handle(), (), context));
    assert_eq!(result.expect("invoke"), Completion::Completed(()));
    let MenuCommandEvent::CommandStarted(started) = next_event(&subscription) else {
        panic!("started");
    };
    let MenuCommandEvent::CommandFinished(finished) = next_event(&subscription) else {
        panic!("finished");
    };
    assert_eq!(started.id, finished.id);
    assert_eq!(started.state, gpui_menus::CommandTaskState::Running);
    assert_eq!(finished.state, gpui_menus::CommandTaskState::Completed);
}
