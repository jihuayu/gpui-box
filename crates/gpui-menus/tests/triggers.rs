//! End-to-end coverage for the five command trigger surfaces.
//!
//! Every assertion is deterministic: workers are released through explicit
//! channels and provider state is inspected directly. No test relies on a
//! platform-native menu, a real keyboard or elapsed time.

mod common;

use common::*;
use gpui_menus::{
    ButtonCommandBinding, CommandDescriptor, CommandId, CommandPayload, CommandScope, Completion,
    ContextExpr, ContextKey, ContextMenu, ContextValue, ContributionBundle, ErrorCode,
    FakeMenuProvider, KeychordBinding, KeystrokeSpec, LazyMaterializationState, LogicalRect,
    MenuBar, MenuCommandService, MenuNode, MenuSection, Modifiers, ResolvedMenuNode, RouteRegistry,
    SystemActionBinding, SystemMenuRole, TopLevelMenu, TriggerSource, UiOwner,
};
use serde::{Deserialize, Serialize};
use std::{cell::RefCell, collections::BTreeMap, rc::Rc, time::SystemTime};

fn command(value: &str) -> CommandId {
    CommandId::new(value).expect("valid command identity")
}

fn key(value: &str) -> ContextKey {
    ContextKey::new(value).expect("valid context key")
}

fn chord(key: &str) -> KeystrokeSpec {
    KeystrokeSpec::plain(key).expect("valid chord")
}

fn completed<T>(result: Completion<T>) -> T {
    match result {
        Completion::Completed(value) => value,
        Completion::Cancelled(reason) => panic!("unexpected cancellation: {reason:?}"),
    }
}

fn latest_task(service: &MenuCommandService) -> gpui_menus::CommandTaskSnapshot {
    service.tasks().pop().expect("task history")
}

#[test]
fn native_menu_bar_projects_checks_submenus_roles_and_installs() {
    let (owner, context) = ui_context(60);
    let provider = Rc::new(FakeMenuProvider::new());
    let service = MenuCommandService::new(provider.clone());
    let scope = service.create_scope(CommandScope::Global).expect("scope");
    let sidebar = key("view.sidebar.visible");
    service.set_global_context(sidebar.clone(), ContextValue::Bool(true));

    let _echo = service
        .register_ui_in(
            &scope,
            CommandDescriptor::ui(command("view.echo"), "Echo")
                .expect("descriptor")
                .with_category("View"),
            |value: String, _| Ok(value),
        )
        .expect("echo command");
    let _toggle = service
        .register_ui_in(
            &scope,
            CommandDescriptor::ui(command("view.toggle"), "Toggle Sidebar").expect("descriptor"),
            |_: (), _| Ok(()),
        )
        .expect("toggle command");

    let bar = MenuBar::new().with_menu(TopLevelMenu::new("View").with_section(MenuSection::new(
        vec![
            MenuNode::check(command("view.toggle"), sidebar.clone()),
            MenuNode::submenu(
                "Echo",
                vec![MenuNode::command(command("view.echo")).with_payload(payload(&"nested"))],
            ),
            MenuNode::separator(),
            MenuNode::system_role(SystemMenuRole::Preferences),
        ],
    )));
    let resolved = completed(
        service
            .install_menu_bar(&scope, bar.clone(), context.clone())
            .expect("install menu bar"),
    );
    assert_eq!(resolved.surface, Some(gpui_menus::SurfaceId(60)));
    assert_eq!(resolved.generation, Some(1));
    assert_eq!(provider.installed_menu_bars(), vec![resolved.clone()]);

    let check = resolved
        .find_item(&command("view.toggle"))
        .expect("check item");
    assert!(check.visible && check.enabled);
    assert_eq!(check.checked, Some(true));
    let echo = resolved
        .find_item(&command("view.echo"))
        .expect("nested echo item");
    assert_eq!(echo.payload, payload(&"nested"));
    assert_eq!(echo.shortcut_hint, None);

    let nodes = &resolved.menus[0].sections[0].nodes;
    assert!(matches!(nodes[0], ResolvedMenuNode::CheckItem(_)));
    assert!(matches!(&nodes[1], ResolvedMenuNode::Submenu(submenu) if submenu.title == "Echo"));
    assert!(matches!(&nodes[1], ResolvedMenuNode::Submenu(submenu) if submenu.nodes.len() == 1));
    assert!(matches!(nodes[2], ResolvedMenuNode::Separator));
    assert!(matches!(
        nodes[3],
        ResolvedMenuNode::SystemRole(SystemMenuRole::Preferences)
    ));

    assert_eq!(
        block_on(service.activate_menu_item(echo, context.clone())).expect("activate nested item"),
        Completion::Completed(CommandPayload::json("nested"))
    );
    let check = resolved
        .find_item(&command("view.toggle"))
        .expect("check item")
        .clone();
    assert_eq!(
        block_on(service.activate_menu_item(&check, context.clone())).expect("activate check"),
        Completion::Completed(CommandPayload::Unit)
    );

    service.set_global_context(sidebar, ContextValue::Bool(false));
    let republished = completed(
        service
            .install_menu_bar(&scope, bar, context)
            .expect("reinstall menu bar"),
    );
    assert_eq!(
        republished
            .find_item(&command("view.toggle"))
            .expect("check item")
            .checked,
        Some(false)
    );
    assert_eq!(service.registered_menu_bars().len(), 2);
    drop(owner);
}

#[test]
fn context_menu_stale_generation_and_provider_overrides_are_typed() {
    let owner = UiOwner::new(gpui_menus::SurfaceId(61));
    let surface = gpui_menus::SurfaceId(61);
    let provider = Rc::new(FakeMenuProvider::new());
    let service = MenuCommandService::new(provider.clone());
    service.register_surface(surface, 1).expect("surface");
    let anchor = LogicalRect {
        x: 4.0,
        y: 8.0,
        width: 120.0,
        height: 32.0,
    };
    let context = gpui_menus::context_menu_context(&owner, anchor).expect("menu context");
    let scope = service.create_scope(CommandScope::Global).expect("scope");
    let _registration = service
        .register_ui_in(
            &scope,
            CommandDescriptor::ui(command("file.rename"), "Rename").expect("descriptor"),
            |_: (), _| Ok(()),
        )
        .expect("command");
    let menu = ContextMenu::new(vec![
        MenuNode::command(command("file.rename")),
        MenuNode::system_role(SystemMenuRole::Close),
    ])
    .with_title("File");

    let resolved = completed(
        service
            .present_context_menu(&scope, menu, context.clone())
            .expect("present context menu"),
    );
    assert_eq!(resolved.title.as_deref(), Some("File"));
    assert_eq!(resolved.surface, Some(surface));
    assert_eq!(resolved.generation, Some(1));
    let presented = provider.presented_context_menus();
    assert_eq!(presented.len(), 1);
    assert_eq!(presented[0].anchor, Some(anchor));
    assert_eq!(presented[0].generation, 1);
    let item = resolved
        .all_items()
        .into_iter()
        .find(|item| item.command_id == command("file.rename"))
        .expect("resolved item")
        .clone();
    assert_eq!(
        block_on(service.activate_context_menu_item(&item, context.clone())).expect("activate"),
        Completion::Completed(CommandPayload::Unit)
    );

    assert_eq!(
        service
            .advance_surface_generation(surface)
            .expect("advance"),
        2
    );
    assert_eq!(
        block_on(service.activate_context_menu_item(&item, context)).expect_err("stale item"),
        gpui_menus::SystemError::new(
            ErrorCode::ContextExpired,
            "Menu item belongs to an expired surface generation"
        )
    );

    provider.set_surface_generation(surface, 2);
    assert_eq!(
        provider
            .record_callback(surface, 1)
            .expect_err("stale callback")
            .code,
        ErrorCode::ContextExpired
    );
    assert!(provider.record_callback(surface, 2).is_ok());
    assert_eq!(provider.stale_callback_rejections(), vec![(surface, 1)]);

    let fresh_owner = UiOwner::new(gpui_menus::SurfaceId(62));
    let fresh = gpui_menus::context_menu_context(&fresh_owner, anchor).expect("fresh context");
    provider.set_operation_unsupported(gpui_menus::MenusOperation::PresentContextMenu);
    assert_eq!(
        service
            .present_context_menu(&scope, ContextMenu::new(Vec::new()), fresh.clone())
            .expect_err("unsupported")
            .code,
        ErrorCode::Unsupported
    );
    provider.set_support(
        gpui_menus::MenusOperation::PresentContextMenu,
        gpui_menus::Support::Native,
    );
    provider.set_availability(
        gpui_menus::MenusOperation::PresentContextMenu,
        gpui_menus::Availability::Available,
    );
    provider.fail_operation(
        gpui_menus::MenusOperation::PresentContextMenu,
        gpui_menus::SystemError::new(ErrorCode::PlatformFailure, "present failed"),
    );
    assert_eq!(
        service
            .present_context_menu(&scope, ContextMenu::new(Vec::new()), fresh)
            .expect_err("provider failure")
            .code,
        ErrorCode::PlatformFailure
    );
    assert_eq!(service.registered_context_menus().len(), 1);
}

#[test]
fn keybinding_when_scope_specificity_and_priority_choose_the_winner() {
    let surface = gpui_menus::SurfaceId(63);
    let owner = UiOwner::new(surface);
    let context = gpui_menus::invocation_context(&owner, TriggerSource::Menu).expect("context");
    let service = MenuCommandService::from_provider(FakeMenuProvider::new());
    service.register_surface(surface, 1).expect("surface");
    let scope = service.create_scope(CommandScope::Global).expect("scope");
    let gate = key("editor.gate");
    service.set_global_context(gate.clone(), ContextValue::Bool(true));

    let runs: Rc<RefCell<Vec<&'static str>>> = Rc::new(RefCell::new(Vec::new()));
    let mut registrations = Vec::new();
    for (id, label) in [
        ("editor.low", "low"),
        ("editor.high", "high"),
        ("editor.surface", "surface"),
        ("editor.j_low", "j_low"),
        ("editor.j_high", "j_high"),
    ] {
        let captured = runs.clone();
        registrations.push(
            service
                .register_ui_in(
                    &scope,
                    CommandDescriptor::ui(command(id), id).expect("descriptor"),
                    move |_: (), _| {
                        captured.borrow_mut().push(label);
                        Ok(())
                    },
                )
                .expect("command"),
        );
    }

    let chord_k = chord("k");
    let chord_j = chord("j");
    let _low = service
        .register_keybinding(
            &scope,
            KeychordBinding::new(chord_k.clone(), command("editor.low")),
        )
        .expect("low binding");
    let _high = service
        .register_keybinding(
            &scope,
            KeychordBinding::new(chord_k.clone(), command("editor.high")).with_priority(10),
        )
        .expect("high binding");
    let _scoped = service
        .register_keybinding(
            &scope,
            KeychordBinding::new(chord_k.clone(), command("editor.surface"))
                .with_scope(CommandScope::surface(surface, 1))
                .when(ContextExpr::equals(gate.clone(), ContextValue::Bool(true))),
        )
        .expect("scoped binding");
    let _j_low = service
        .register_keybinding(
            &scope,
            KeychordBinding::new(chord_j.clone(), command("editor.j_low")),
        )
        .expect("j low binding");
    let _j_high = service
        .register_keybinding(
            &scope,
            KeychordBinding::new(chord_j.clone(), command("editor.j_high"))
                .with_priority(4)
                .when(ContextExpr::equals(gate.clone(), ContextValue::Bool(true))),
        )
        .expect("j high binding");

    let sync = completed(
        service
            .sync_keybindings(context.clone())
            .expect("sync keybindings"),
    );
    assert_eq!(sync.len(), 2);
    let winner_k = sync
        .iter()
        .find(|binding| binding.chord == chord_k)
        .expect("k winner");
    assert_eq!(winner_k.command_id, command("editor.surface"));
    assert_eq!(winner_k.scope_specificity, 2);
    assert_eq!(winner_k.priority, 0);
    assert_eq!(winner_k.shortcut_hint(), "K");
    let winner_j = sync
        .iter()
        .find(|binding| binding.chord == chord_j)
        .expect("j winner");
    assert_eq!(winner_j.command_id, command("editor.j_high"));
    assert_eq!(winner_j.scope_specificity, 1);
    assert_eq!(winner_j.priority, 4);
    assert_eq!(winner_j.shortcut_hint(), "J");

    assert_eq!(
        block_on(service.dispatch_keychord(&chord_k, context.clone())).expect("surface"),
        Completion::Completed(CommandPayload::Unit)
    );
    assert_eq!(
        block_on(service.dispatch_keychord(&chord_j, context.clone())).expect("j high"),
        Completion::Completed(CommandPayload::Unit)
    );

    service.set_global_context(gate, ContextValue::Bool(false));
    let sync = completed(
        service
            .sync_keybindings(context.clone())
            .expect("sync after gate"),
    );
    assert_eq!(
        sync.iter()
            .find(|binding| binding.chord == chord_k)
            .expect("k winner")
            .command_id,
        command("editor.high")
    );
    assert_eq!(
        sync.iter()
            .find(|binding| binding.chord == chord_j)
            .expect("j winner")
            .command_id,
        command("editor.j_low")
    );
    assert_eq!(
        block_on(service.dispatch_keychord(&chord_k, context.clone())).expect("high"),
        Completion::Completed(CommandPayload::Unit)
    );
    assert_eq!(
        block_on(service.dispatch_keychord(&chord_j, context)).expect("j low"),
        Completion::Completed(CommandPayload::Unit)
    );
    assert_eq!(*runs.borrow(), vec!["surface", "j_high", "high", "j_low"]);

    assert_eq!(
        KeystrokeSpec::new("S", Modifiers::command().with_shift())
            .expect("chord")
            .to_string(),
        "Shift+Cmd+S"
    );
}

#[test]
fn palette_filters_ranks_shortcuts_disabled_state_and_invokes() {
    let (_owner, context) = ui_context(64);
    let service = MenuCommandService::from_provider(FakeMenuProvider::new());
    let scope = service.create_scope(CommandScope::Global).expect("scope");
    let visible = key("palette.visible");
    let enabled = key("palette.enabled");
    service.set_global_context(visible.clone(), ContextValue::Bool(false));
    service.set_global_context(enabled.clone(), ContextValue::Bool(false));

    let runs: Rc<RefCell<Vec<String>>> = Rc::new(RefCell::new(Vec::new()));
    let register = |id: &str, title: &str, category: &str| {
        let captured = runs.clone();
        let command_id = command(id);
        let title = title.to_owned();
        let descriptor = CommandDescriptor::ui(command_id.clone(), title.clone())
            .expect("descriptor")
            .with_category(category)
            .with_summary(format!("{title} summary"));
        service
            .register_ui_in(&scope, descriptor, move |_: (), _| {
                captured.borrow_mut().push(command_id.as_str().to_owned());
                Ok(())
            })
            .expect("command")
    };
    let _save = register("file.save", "Save", "File");
    let _save_as = register("file.save_as", "Save As", "File");
    let _save_locked = {
        let captured = runs.clone();
        service
            .register_ui_in(
                &scope,
                CommandDescriptor::ui(command("file.save_locked"), "Save Locked")
                    .expect("descriptor")
                    .with_category("File")
                    .enabled_when(ContextExpr::equals(
                        enabled.clone(),
                        ContextValue::Bool(true),
                    )),
                move |_: (), _| {
                    captured.borrow_mut().push("file.save_locked".to_owned());
                    Ok(())
                },
            )
            .expect("locked command")
    };
    let _open = register("file.open", "Open", "File");
    let _conditional = {
        let captured = runs.clone();
        service
            .register_ui_in(
                &scope,
                CommandDescriptor::ui(command("palette.conditional"), "Conditional")
                    .expect("descriptor")
                    .with_category("Navigate")
                    .visible_when(ContextExpr::equals(
                        visible.clone(),
                        ContextValue::Bool(true),
                    )),
                move |_: (), _| {
                    captured.borrow_mut().push("palette.conditional".to_owned());
                    Ok(())
                },
            )
            .expect("conditional command")
    };

    let _shortcut = service
        .register_keybinding(
            &scope,
            KeychordBinding::new(
                KeystrokeSpec::new("s", Modifiers::command()).expect("chord"),
                command("file.save"),
            ),
        )
        .expect("shortcut");

    let entries = completed(
        service
            .query_palette(gpui_menus::PaletteQuery::new("save"), context.clone())
            .expect("palette"),
    );
    assert_eq!(entries.len(), 3);
    assert_eq!(entries[0].command_id, command("file.save"));
    assert_eq!(entries[0].score, 1000);
    assert_eq!(entries[0].shortcut_hint.as_deref(), Some("Cmd+S"));
    assert!(entries[0].enabled);
    assert!(
        entries
            .windows(2)
            .all(|pair| pair[0].score >= pair[1].score)
    );
    let locked = entries
        .iter()
        .find(|entry| entry.command_id == command("file.save_locked"))
        .expect("disabled entry");
    assert!(!locked.enabled);
    assert!(locked.disabled_reason.is_some());
    assert!(
        !entries
            .iter()
            .any(|entry| entry.command_id == command("file.open")),
        "visible_when=false entries are filtered out"
    );
    assert!(
        !entries
            .iter()
            .any(|entry| entry.command_id == command("palette.conditional")),
        "unpublished conditional command is filtered out"
    );

    let file_entries = completed(
        service
            .query_palette(
                gpui_menus::PaletteQuery::new("").with_category("File"),
                context.clone(),
            )
            .expect("category palette"),
    );
    assert_eq!(file_entries.len(), 4);
    assert!(
        file_entries
            .iter()
            .all(|entry| entry.category.as_deref() == Some("File"))
    );
    assert_eq!(
        completed(
            service
                .query_palette(
                    gpui_menus::PaletteQuery::new("save").with_limit(1),
                    context.clone(),
                )
                .expect("limited palette"),
        )
        .len(),
        1
    );

    service.set_global_context(visible, ContextValue::Bool(true));
    service.set_global_context(enabled.clone(), ContextValue::Bool(true));
    let entries = completed(
        service
            .query_palette(
                gpui_menus::PaletteQuery::new("conditional"),
                context.clone(),
            )
            .expect("conditional palette"),
    );
    assert_eq!(entries.len(), 1);
    assert!(entries[0].enabled);
    assert_eq!(
        block_on(service.invoke_palette_entry(&entries[0], context.clone())).expect("invoke"),
        Completion::Completed(CommandPayload::Unit)
    );
    assert_eq!(
        runs.borrow().last().map(String::as_str),
        Some("palette.conditional")
    );

    let locked = completed(
        service
            .query_palette(gpui_menus::PaletteQuery::new("locked"), context.clone())
            .expect("locked palette"),
    )
    .into_iter()
    .next()
    .expect("locked entry");
    assert!(locked.enabled);
    assert_eq!(
        block_on(service.invoke_palette_entry(&locked, context.clone())).expect("invoke locked"),
        Completion::Completed(CommandPayload::Unit)
    );

    service.set_global_context(enabled, ContextValue::Bool(false));
    assert_eq!(
        block_on(service.invoke_palette_entry(&locked, context))
            .expect_err("disabled invocation")
            .code,
        ErrorCode::InvalidInput
    );
    assert_eq!(
        *runs.borrow(),
        vec!["palette.conditional", "file.save_locked"]
    );
}

#[test]
fn component_button_binding_projects_and_invokes() {
    let (_owner, context) = ui_context(65);
    let service = MenuCommandService::from_provider(FakeMenuProvider::new());
    let scope = service.create_scope(CommandScope::Global).expect("scope");
    let visible = key("button.visible");
    let enabled = key("button.enabled");
    service.set_global_context(visible.clone(), ContextValue::Bool(true));
    service.set_global_context(enabled.clone(), ContextValue::Bool(true));
    let runs = Rc::new(RefCell::new(0_u32));
    let captured = runs.clone();
    let _echo = service
        .register_ui_in(
            &scope,
            CommandDescriptor::ui(command("button.echo"), "Echo")
                .expect("descriptor")
                .with_summary("Echo the payload"),
            move |value: String, _| {
                *captured.borrow_mut() += 1;
                Ok(value)
            },
        )
        .expect("command");
    let _shortcut = service
        .register_keybinding(
            &scope,
            KeychordBinding::new(chord("e"), command("button.echo")),
        )
        .expect("shortcut");
    let binding = ButtonCommandBinding::new(command("button.echo"))
        .with_label("Echo Now")
        .with_payload(payload(&"hello"))
        .visible_when(ContextExpr::equals(
            visible.clone(),
            ContextValue::Bool(true),
        ))
        .enabled_when(ContextExpr::equals(
            enabled.clone(),
            ContextValue::Bool(true),
        ));
    let _button = service
        .register_button(&scope, binding.clone())
        .expect("button");
    let state = service.project_button(&binding, &context).expect("state");
    assert_eq!(state.label, "Echo Now");
    assert_eq!(state.accessible_name, "Echo Now");
    assert_eq!(state.summary, "Echo the payload");
    assert!(state.visible && state.enabled);
    assert_eq!(state.shortcut_hint.as_deref(), Some("E"));
    assert_eq!(service.button_states(&context).expect("states").len(), 1);

    service.set_global_context(visible, ContextValue::Bool(false));
    let hidden = service.project_button(&binding, &context).expect("hidden");
    assert!(!hidden.visible);
    assert_eq!(
        block_on(service.invoke_button(&binding, context.clone()))
            .expect_err("hidden button")
            .code,
        ErrorCode::InvalidInput
    );
    service.set_global_context(key("button.visible"), ContextValue::Bool(true));
    service.set_global_context(enabled, ContextValue::Bool(false));
    let disabled = service
        .project_button(&binding, &context)
        .expect("disabled");
    assert!(!disabled.enabled);
    assert!(disabled.disabled_reason.is_some());
    assert_eq!(
        block_on(service.invoke_button(&binding, context.clone()))
            .expect_err("disabled button")
            .code,
        ErrorCode::InvalidInput
    );

    service.set_global_context(key("button.enabled"), ContextValue::Bool(true));
    assert_eq!(
        block_on(service.invoke_button(&binding, context.clone())).expect("enabled"),
        Completion::Completed(CommandPayload::json("hello"))
    );
    assert_eq!(*runs.borrow(), 1);

    let missing = ButtonCommandBinding::new(command("button.missing"));
    assert_eq!(
        service
            .project_button(&missing, &context)
            .expect_err("missing command")
            .code,
        ErrorCode::Unavailable
    );
    assert_eq!(
        block_on(service.invoke_button(&missing, context))
            .expect_err("missing invocation")
            .code,
        ErrorCode::Unavailable
    );
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct OpenRequest {
    name: String,
}

#[test]
fn lazy_buttons_materialize_and_invoke_button_uses_scope_context() {
    let (_owner, context) = ui_context(68);
    let service = MenuCommandService::from_provider(FakeMenuProvider::new());
    let scope = service.create_scope(CommandScope::Global).expect("scope");
    let enabled = key("button.lazy.enabled");
    service.set_scope_context(scope.id(), enabled.clone(), ContextValue::Bool(true));
    let binding = ButtonCommandBinding::new(command("button.lazy"))
        .with_label("Lazy Action")
        .enabled_when(ContextExpr::equals(
            enabled.clone(),
            ContextValue::Bool(true),
        ));
    let (lazy, _lease) = service
        .register_lazy(&scope, {
            let binding = binding.clone();
            move || {
                ContributionBundle::new()
                    .with_command_ui(
                        CommandDescriptor::ui(command("button.lazy"), "Lazy Action")
                            .expect("descriptor"),
                        |_: (), _| Ok("lazy"),
                    )
                    .with_button(binding)
            }
        })
        .expect("lazy");
    assert_eq!(lazy.state(), Some(LazyMaterializationState::Unmaterialized));

    let states = service
        .button_states(&context)
        .expect("materialized states");
    assert_eq!(states.len(), 1, "lazy bundle button is projected");
    assert_eq!(states[0].label, "Lazy Action");
    assert!(states[0].visible && states[0].enabled);
    assert!(lazy.is_materialized());

    service.set_scope_context(scope.id(), enabled.clone(), ContextValue::Bool(false));
    let disabled = service
        .project_button(&binding, &context)
        .expect("disabled projection");
    assert!(!disabled.enabled);
    assert!(disabled.disabled_reason.is_some());
    assert_eq!(
        block_on(service.invoke_button(&binding, context.clone()))
            .expect_err("scope context disables the button")
            .code,
        ErrorCode::InvalidInput
    );

    service.set_scope_context(scope.id(), enabled, ContextValue::Bool(true));
    assert_eq!(
        block_on(service.invoke_button(&binding, context)).expect("scope fallback enables"),
        Completion::Completed(CommandPayload::json("lazy"))
    );
}

#[test]
fn system_actions_and_activation_routes_dispatch_to_typed_payloads() {
    let (_owner, context) = ui_context(66);
    let service = MenuCommandService::from_provider(FakeMenuProvider::new());
    let scope = service.create_scope(CommandScope::Global).expect("scope");
    let descriptor = CommandDescriptor::ui(command("file.open"), "Open").expect("descriptor");
    let _registration = service
        .register_ui_in(&scope, descriptor, move |input: OpenRequest, _| {
            Ok(format!("opened:{}", input.name))
        })
        .expect("command");
    let _activate_command = service
        .register_ui_in(
            &scope,
            CommandDescriptor::ui(command("app.activate"), "Activate").expect("descriptor"),
            |_: (), _| Ok(()),
        )
        .expect("activate command");
    let mut routes = RouteRegistry::default();
    routes.register("open.file", 1).expect("route");
    let target = gpui_menus::ActivationTarget::new(
        "open.file",
        1,
        BTreeMap::from([("name".to_owned(), "document".to_owned())]),
    )
    .expect("target");
    let _action = service
        .register_system_action(
            &scope,
            SystemActionBinding::action("open.file", command("file.open"))
                .expect("action target")
                .parameter("name", "name"),
            &routes,
        )
        .expect("action binding");
    let _activation = service
        .register_system_action(
            &scope,
            SystemActionBinding::activation(target.clone(), command("file.open"))
                .parameter("name", "name"),
            &routes,
        )
        .expect("activation binding");
    let _activate_action = service
        .register_system_action(
            &scope,
            SystemActionBinding::action("activate", command("app.activate"))
                .expect("activate action"),
            &routes,
        )
        .expect("activate action binding");

    let parameters = BTreeMap::from([("name".to_owned(), "document".to_owned())]);
    assert_eq!(
        block_on(service.dispatch_system_action("open.file", &parameters, context.clone()))
            .expect("action id"),
        Completion::Completed(CommandPayload::json("opened:document"))
    );
    assert_eq!(
        latest_task(&service).trigger,
        TriggerSource::SystemAction,
        "system action dispatch records its own trigger"
    );
    assert_eq!(
        block_on(service.dispatch_system_action(
            "open.file",
            &BTreeMap::from([("other".to_owned(), "value".to_owned())]),
            context.clone(),
        ))
        .expect_err("missing parameter")
        .code,
        ErrorCode::InvalidInput
    );
    assert_eq!(
        block_on(service.dispatch_system_action("missing.action", &parameters, context.clone()))
            .expect_err("unknown action")
            .code,
        ErrorCode::Unavailable
    );

    let event = gpui_menus::ActivationEvent::new(
        gpui_menus::ActivationId::new("activation-1").expect("id"),
        gpui_menus::ActivationSource::WarmLaunch,
        gpui_menus::ActivationPayload::Route(target.clone()),
        SystemTime::now(),
    )
    .expect("event");
    assert_eq!(
        block_on(service.dispatch_activation(&event, &routes, context.clone()))
            .expect("activation"),
        Completion::Completed(CommandPayload::json("opened:document"))
    );
    assert_eq!(
        latest_task(&service).trigger,
        TriggerSource::Activation,
        "route activation preserves TriggerSource::Activation"
    );

    let invalid = gpui_menus::ActivationEvent::new(
        gpui_menus::ActivationId::new("activation-2").expect("id"),
        gpui_menus::ActivationSource::WarmLaunch,
        gpui_menus::ActivationPayload::Route(
            gpui_menus::ActivationTarget::new("open.file", 2, BTreeMap::new()).expect("target"),
        ),
        SystemTime::now(),
    )
    .expect("event");
    assert_eq!(
        block_on(service.dispatch_activation(&invalid, &routes, context.clone()))
            .expect_err("version")
            .code,
        ErrorCode::InvalidConfiguration
    );

    let notification = gpui_menus::ActivationEvent::new(
        gpui_menus::ActivationId::new("activation-3").expect("id"),
        gpui_menus::ActivationSource::Notification,
        gpui_menus::ActivationPayload::Notification {
            notification_id: gpui_menus::ActivationId::new("notification-1").expect("id"),
            target,
        },
        SystemTime::now(),
    )
    .expect("event");
    assert_eq!(
        block_on(service.dispatch_activation(&notification, &routes, context.clone()))
            .expect("notification activation"),
        Completion::Completed(CommandPayload::json("opened:document"))
    );
    assert_eq!(
        latest_task(&service).trigger,
        TriggerSource::Activation,
        "notification activation preserves TriggerSource::Activation"
    );

    let activate = gpui_menus::ActivationEvent::new(
        gpui_menus::ActivationId::new("activation-4").expect("id"),
        gpui_menus::ActivationSource::WarmLaunch,
        gpui_menus::ActivationPayload::Activate,
        SystemTime::now(),
    )
    .expect("event");
    assert_eq!(
        block_on(service.dispatch_activation(&activate, &routes, context.clone()))
            .expect("activate activation"),
        Completion::Completed(CommandPayload::Unit)
    );
    assert_eq!(
        latest_task(&service).trigger,
        TriggerSource::Activation,
        "Activate activation preserves TriggerSource::Activation"
    );
    assert_eq!(latest_task(&service).command_id, command("app.activate"));
    assert_eq!(
        block_on(service.dispatch_system_action("activate", &BTreeMap::new(), context))
            .expect("activate system action"),
        Completion::Completed(CommandPayload::Unit)
    );
    assert_eq!(
        latest_task(&service).trigger,
        TriggerSource::SystemAction,
        "the same action dispatched directly is a system action"
    );
}

#[test]
fn unregistered_menu_items_are_disabled_and_activation_is_rejected() {
    let (_owner, context) = ui_context(67);
    let service = MenuCommandService::from_provider(FakeMenuProvider::new());
    let scope = service.create_scope(CommandScope::Global).expect("scope");
    let bar = MenuBar::new().with_menu(
        TopLevelMenu::new("File").with_nodes(vec![MenuNode::command(command("file.absent"))]),
    );
    let resolved = completed(
        service
            .install_menu_bar(&scope, bar, context.clone())
            .expect("install"),
    );
    let item = resolved
        .find_item(&command("file.absent"))
        .expect("placeholder");
    assert!(!item.enabled);
    assert!(item.disabled_reason.is_some());
    assert_eq!(
        block_on(service.activate_menu_item(item, context))
            .expect_err("disabled placeholder")
            .code,
        ErrorCode::InvalidInput
    );
}
