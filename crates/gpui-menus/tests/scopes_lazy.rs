mod common;

use common::*;
use gpui_menus::{
    ButtonCommandBinding, CancelReason, CommandDescriptor, CommandId, CommandPayload, CommandScope,
    CommandTaskState, Completion, ContributionBundle, ErrorCode, FakeMenuProvider,
    InvocationContext, KeychordBinding, KeystrokeSpec, LazyContribution, LazyMaterializationState,
    MenuBar, MenuCommandEvent, MenuCommandService, MenuNode, OperationContext, PaletteQuery,
    RegistrationLease, RouteRegistry, SurfaceId, SystemActionBinding, TopLevelMenu,
};
use std::{
    collections::BTreeMap,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
        mpsc,
    },
    time::Duration,
};

fn command(value: &str) -> CommandId {
    CommandId::new(value).expect("id")
}

#[test]
fn duplicate_ids_are_rejected_and_reported_without_overwriting() {
    let (_owner, context) = ui_context(30);
    let service = MenuCommandService::from_provider(FakeMenuProvider::new());
    let descriptor = CommandDescriptor::ui(command("file.save"), "Save").expect("descriptor");
    let first = service
        .register_ui(descriptor.clone(), |_: (), _| Ok(1_u32))
        .expect("first");
    let subscription = service.subscribe();
    let error = service
        .register_ui(descriptor, |_: (), _| Ok(2_u32))
        .expect_err("duplicate");
    assert_eq!(error.code, ErrorCode::InvalidInput);
    assert_eq!(service.duplicate_diagnostics().len(), 1);
    let diagnostic = &service.duplicate_diagnostics()[0];
    assert_eq!(diagnostic.command_id, command("file.save"));
    let MenuCommandEvent::DuplicateCommandRejected(event) = next_event(&subscription) else {
        panic!("duplicate event");
    };
    assert_eq!(event.command_id, command("file.save"));
    assert_ne!(event.existing_scope, event.conflicting_scope);
    assert_eq!(
        block_on(service.invoke_typed(first.handle(), (), context))
            .expect("original owner remains"),
        Completion::Completed(1_u32)
    );
}

#[test]
fn lazy_bundle_materializes_once_and_rejects_duplicate_commands() {
    let (_owner, context) = ui_context(31);
    let service = MenuCommandService::from_provider(FakeMenuProvider::new());
    let scope = service.create_scope(CommandScope::Global).expect("scope");
    let existing = service
        .register_ui_in(
            &scope,
            CommandDescriptor::ui(command("file.save"), "Save").expect("descriptor"),
            |_: (), _| Ok(1_u32),
        )
        .expect("existing");
    let factory_runs = Arc::new(AtomicUsize::new(0));
    let counter = factory_runs.clone();
    let (lazy, lazy_lease) = service
        .register_lazy(&scope, move || {
            counter.fetch_add(1, Ordering::SeqCst);
            ContributionBundle::new()
                .with_command_ui(
                    CommandDescriptor::ui(command("file.save"), "Save Again").expect("descriptor"),
                    |_: (), _| Ok(99_u32),
                )
                .with_command_ui(
                    CommandDescriptor::ui(command("lazy.greet"), "Greet").expect("descriptor"),
                    |_: (), _| Ok("hello".to_owned()),
                )
                .with_menu_node(MenuNode::command(command("lazy.greet")))
                .with_keybinding(KeychordBinding::new(
                    KeystrokeSpec::plain("g").expect("chord"),
                    command("lazy.greet"),
                ))
                .with_button(ButtonCommandBinding::new(command("lazy.greet")))
                .with_system_action(
                    SystemActionBinding::action("lazy.action", command("lazy.greet"))
                        .expect("action"),
                )
        })
        .expect("lazy registration");
    assert_eq!(lazy.state(), Some(LazyMaterializationState::Unmaterialized));
    assert_eq!(factory_runs.load(Ordering::SeqCst), 0);
    let subscription = service.subscribe();
    let palette =
        expect_completed(service.query_palette(PaletteQuery::new("greet"), context.clone()));
    assert_eq!(palette.len(), 1);
    assert_eq!(factory_runs.load(Ordering::SeqCst), 1);
    assert!(lazy.is_materialized());
    let MenuCommandEvent::DuplicateCommandRejected(diagnostic) = next_event(&subscription) else {
        panic!("lazy duplicate diagnostic");
    };
    assert_eq!(diagnostic.command_id, command("file.save"));
    assert_eq!(diagnostic.domain, gpui_menus::ExecutionDomain::Ui);
    assert_eq!(service.duplicate_diagnostics().len(), 1);

    let palette =
        expect_completed(service.query_palette(PaletteQuery::new("greet"), context.clone()));
    assert_eq!(palette.len(), 1);
    assert_eq!(factory_runs.load(Ordering::SeqCst), 1, "factory runs once");
    assert_eq!(
        block_on(service.invoke_typed(existing.handle(), (), context.clone()))
            .expect("existing owner"),
        Completion::Completed(1_u32)
    );
    assert_eq!(
        block_on(service.invoke_command(
            &command("lazy.greet"),
            CommandPayload::Unit,
            context.clone(),
        ))
        .expect("lazy command"),
        Completion::Completed(CommandPayload::json("hello"))
    );
    assert_eq!(
        block_on(service.dispatch_system_action("lazy.action", &BTreeMap::new(), context.clone(),))
            .expect("lazy system action"),
        Completion::Completed(CommandPayload::json("hello"))
    );
    assert!(service.button_states(&context).is_ok());
    assert!(
        service
            .resolve_keychord(&KeystrokeSpec::plain("g").expect("chord"), &context)
            .is_some()
    );
    assert!(lazy_lease.is_active());
}

#[test]
fn lazy_factory_never_runs_after_revocation() {
    let (_owner, context) = ui_context(32);
    let service = MenuCommandService::from_provider(FakeMenuProvider::new());
    let scope = service.create_scope(CommandScope::Global).expect("scope");
    let runs = Arc::new(AtomicUsize::new(0));
    let counter = runs.clone();
    let (lazy, _lease) = service
        .register_lazy(&scope, move || {
            counter.fetch_add(1, Ordering::SeqCst);
            ContributionBundle::new().with_command_ui(
                CommandDescriptor::ui(command("lazy.revoked"), "Revoked").expect("descriptor"),
                |_: (), _| Ok(()),
            )
        })
        .expect("lazy");
    assert!(scope.revoke());
    assert!(!scope.revoke(), "scope revocation is idempotent");
    assert_eq!(lazy.state(), None);
    let palette = expect_completed(service.query_palette(PaletteQuery::new("revoked"), context));
    assert!(palette.is_empty());
    assert_eq!(runs.load(Ordering::SeqCst), 0);
}

#[test]
fn scope_revocation_removes_every_surface_and_cancels_in_flight_work() {
    let owner = gpui_menus::UiOwner::new(SurfaceId(33));
    let context = InvocationContext::new(OperationContext {
        ui: Some(owner.context(None).expect("ui")),
        ..OperationContext::default()
    });
    let service = MenuCommandService::from_provider(FakeMenuProvider::new());
    let scope = service.create_scope(CommandScope::Global).expect("scope");
    let (started_tx, started_rx) = mpsc::channel::<()>();
    let (release_tx, release_rx) = mpsc::channel::<()>();
    let release_rx = Arc::new(Mutex::new(release_rx));
    let registration = service
        .register_background_in(
            &scope,
            CommandDescriptor::background(command("worker.scope"), "Scope Work")
                .expect("descriptor"),
            move |_: (), _| {
                let _ = started_tx.send(());
                release_rx.lock().expect("gate").recv().expect("release");
                Ok(())
            },
        )
        .expect("command");
    let menu_lease = service
        .register_menu_bar(
            &scope,
            MenuBar::new().with_menu(
                TopLevelMenu::new("File")
                    .with_nodes(vec![MenuNode::command(command("worker.scope"))]),
            ),
        )
        .expect("menu");
    let key_lease = service
        .register_keybinding(
            &scope,
            KeychordBinding::new(
                KeystrokeSpec::plain("k").expect("chord"),
                command("worker.scope"),
            ),
        )
        .expect("keybinding");
    let button = ButtonCommandBinding::new(command("worker.scope"));
    let button_lease = service
        .register_button(&scope, button.clone())
        .expect("button");
    let action_lease = service
        .register_system_action(
            &scope,
            SystemActionBinding::action("scope.action", command("worker.scope")).expect("action"),
            &RouteRegistry::default(),
        )
        .expect("system action");
    let lazy_runs = Arc::new(AtomicUsize::new(0));
    let counter = lazy_runs.clone();
    let (lazy, _lazy_lease) = service
        .register_lazy(&scope, move || {
            counter.fetch_add(1, Ordering::SeqCst);
            ContributionBundle::new().with_command_ui(
                CommandDescriptor::ui(command("lazy.scope"), "Lazy Scope").expect("descriptor"),
                |_: (), _| Ok(()),
            )
        })
        .expect("lazy");

    let subscription = service.subscribe();
    let request = service.invoke_typed(registration.handle(), (), context.clone());
    let MenuCommandEvent::CommandStarted(started) = next_event(&subscription) else {
        panic!("started");
    };
    started_rx
        .recv_timeout(Duration::from_secs(10))
        .expect("handler started");
    assert!(scope.revoke());
    assert_eq!(
        service.command_task(started.id).expect("task").state,
        CommandTaskState::Cancelled(CancelReason::OwnerDisposed)
    );
    assert_eq!(
        block_on(request).expect("cancelled result"),
        Completion::Cancelled(CancelReason::OwnerDisposed)
    );
    assert!(!service.is_registered(&command("worker.scope")));
    assert!(service.registered_menu_bars().is_empty());
    assert!(service.registered_context_menus().is_empty());
    assert!(
        expect_completed(service.query_palette(PaletteQuery::new("scope"), context.clone()))
            .is_empty()
    );
    assert!(
        service
            .resolve_keychord(&KeystrokeSpec::plain("k").expect("chord"), &context)
            .is_none()
    );
    assert!(service.button_states(&context).expect("buttons").is_empty());
    assert_eq!(
        block_on(service.dispatch_system_action("scope.action", &BTreeMap::new(), context.clone()))
            .expect_err("removed system action")
            .code,
        ErrorCode::Unavailable
    );
    assert_eq!(lazy.state(), None);
    assert_eq!(lazy_runs.load(Ordering::SeqCst), 0);
    assert!(!menu_lease.is_active());
    assert!(!key_lease.is_active());
    assert!(!button_lease.is_active());
    assert!(!action_lease.is_active());
    assert!(!registration.lease().is_active());
    release_tx.send(()).expect("release worker");
}

#[test]
fn lease_drop_revokes_only_its_own_command_and_cancels_work() {
    let service = MenuCommandService::from_provider(FakeMenuProvider::new());
    let (started_tx, started_rx) = mpsc::channel::<()>();
    let (release_tx, release_rx) = mpsc::channel::<()>();
    let release_rx = Arc::new(Mutex::new(release_rx));
    let registration = service
        .register_background(
            CommandDescriptor::background(command("worker.lease"), "Lease Work")
                .expect("descriptor"),
            move |_: (), _| {
                let _ = started_tx.send(());
                release_rx.lock().expect("gate").recv().expect("release");
                Ok(())
            },
        )
        .expect("command");
    let handle = registration.handle().clone();
    let subscription = service.subscribe();
    let request = service.invoke_typed(&handle, (), InvocationContext::default());
    let MenuCommandEvent::CommandStarted(started) = next_event(&subscription) else {
        panic!("started");
    };
    started_rx
        .recv_timeout(Duration::from_secs(10))
        .expect("handler started");
    let (_, lease) = registration.into_parts();
    assert!(lease.is_active());
    drop(lease);
    assert!(!service.is_registered(&command("worker.lease")));
    assert_eq!(
        service.command_task(started.id).expect("task").state,
        CommandTaskState::Cancelled(CancelReason::OwnerDisposed)
    );
    assert_eq!(
        block_on(request).expect("cancelled"),
        Completion::Cancelled(CancelReason::OwnerDisposed)
    );
    release_tx.send(()).expect("release worker");
}

#[test]
fn surface_generation_bump_revokes_scoped_contributions_and_in_flight_tasks() {
    let surface = SurfaceId(40);
    let owner = gpui_menus::UiOwner::new(surface);
    let context = InvocationContext::new(OperationContext {
        ui: Some(owner.context(None).expect("ui")),
        ..OperationContext::default()
    });
    let service = MenuCommandService::from_provider(FakeMenuProvider::new());
    service.register_surface(surface, 1).expect("surface");
    let scope = service
        .create_scope(CommandScope::surface(surface, 1))
        .expect("surface scope");
    let (started_tx, started_rx) = mpsc::channel::<()>();
    let (release_tx, release_rx) = mpsc::channel::<()>();
    let release_rx = Arc::new(Mutex::new(release_rx));
    let registration = service
        .register_background_in(
            &scope,
            CommandDescriptor::background(command("surface.work"), "Surface Work")
                .expect("descriptor"),
            move |_: (), _| {
                let _ = started_tx.send(());
                release_rx.lock().expect("gate").recv().expect("release");
                Ok(())
            },
        )
        .expect("command");
    let _menu_lease = service
        .register_menu_bar(
            &scope,
            MenuBar::new().with_menu(
                TopLevelMenu::new("File")
                    .with_nodes(vec![MenuNode::command(command("surface.work"))]),
            ),
        )
        .expect("menu");
    let _key_lease = service
        .register_keybinding(
            &scope,
            KeychordBinding::new(
                KeystrokeSpec::plain("s").expect("chord"),
                command("surface.work"),
            ),
        )
        .expect("keybinding");
    let _button_lease = service
        .register_button(&scope, ButtonCommandBinding::new(command("surface.work")))
        .expect("button");
    let _action_lease = service
        .register_system_action(
            &scope,
            SystemActionBinding::action("surface.action", command("surface.work")).expect("action"),
            &RouteRegistry::default(),
        )
        .expect("action");
    let (lazy, _lazy_lease) = service
        .register_lazy(&scope, || {
            ContributionBundle::new().with_command_ui(
                CommandDescriptor::ui(command("lazy.surface"), "Lazy Surface").expect("descriptor"),
                |_: (), _| Ok(()),
            )
        })
        .expect("lazy");

    let subscription = service.subscribe();
    let request = service.invoke_typed(registration.handle(), (), context.clone());
    let MenuCommandEvent::CommandStarted(started) = next_event(&subscription) else {
        panic!("started");
    };
    started_rx
        .recv_timeout(Duration::from_secs(10))
        .expect("handler started");
    assert_eq!(service.advance_generation(surface).expect("advance"), 2);
    assert_eq!(
        service.command_task(started.id).expect("task").state,
        CommandTaskState::Cancelled(CancelReason::OwnerDisposed)
    );
    assert_eq!(
        block_on(request).expect("cancelled"),
        Completion::Cancelled(CancelReason::OwnerDisposed)
    );
    assert!(!service.is_registered(&command("surface.work")));
    assert!(service.registered_menu_bars().is_empty());
    assert!(
        service
            .resolve_keychord(&KeystrokeSpec::plain("s").expect("chord"), &context)
            .is_none()
    );
    assert!(service.button_states(&context).expect("buttons").is_empty());
    assert_eq!(
        block_on(service.dispatch_system_action(
            "surface.action",
            &BTreeMap::new(),
            context.clone()
        ))
        .expect_err("revoked")
        .code,
        ErrorCode::Unavailable
    );
    assert_eq!(lazy.state(), None);
    assert!(!scope.is_active());
    assert!(
        service
            .create_scope(CommandScope::surface(surface, 1))
            .is_err(),
        "old generation scope is rejected"
    );
    assert_eq!(
        service
            .install_menu_bar(
                &service
                    .create_scope(CommandScope::Global)
                    .expect("new scope"),
                MenuBar::new(),
                context,
            )
            .expect_err("stale context")
            .code,
        ErrorCode::ContextExpired
    );
    release_tx.send(()).expect("release worker");
}

#[test]
fn dispose_surface_revokes_and_marks_the_surface_dead() {
    let surface = SurfaceId(41);
    let service = MenuCommandService::from_provider(FakeMenuProvider::new());
    service.register_surface(surface, 3).expect("surface");
    let scope = service
        .create_scope(CommandScope::surface(surface, 3))
        .expect("scope");
    let _command_registration = service
        .register_ui_in(
            &scope,
            CommandDescriptor::ui(command("surface.command"), "Surface").expect("descriptor"),
            |_: (), _| Ok(()),
        )
        .expect("command");
    assert!(service.is_registered(&command("surface.command")));
    service.dispose_surface(surface);
    assert_eq!(service.surface_generation(surface), None);
    assert!(!service.is_registered(&command("surface.command")));
    assert!(!scope.is_active());
    assert!(
        service
            .create_scope(CommandScope::surface(surface, 3))
            .is_err()
    );
}

#[test]
fn lazy_revocation_after_materialization_removes_every_bundle_contribution() {
    let (_owner, context) = ui_context(42);
    let service = MenuCommandService::from_provider(FakeMenuProvider::new());
    let scope = service.create_scope(CommandScope::Global).expect("scope");
    let (lazy, _lease) = service
        .register_lazy(&scope, || {
            ContributionBundle::new()
                .with_command_ui(
                    CommandDescriptor::ui(command("lazy.after"), "After").expect("descriptor"),
                    |_: (), _| Ok("after".to_owned()),
                )
                .with_keybinding(KeychordBinding::new(
                    KeystrokeSpec::plain("a").expect("chord"),
                    command("lazy.after"),
                ))
                .with_button(ButtonCommandBinding::new(command("lazy.after")))
        })
        .expect("lazy");
    assert_eq!(service.materialize_all(), 1);
    assert!(lazy.is_materialized());
    assert!(service.is_registered(&command("lazy.after")));
    assert!(
        service
            .resolve_keychord(&KeystrokeSpec::plain("a").expect("chord"), &context)
            .is_some()
    );
    assert!(lazy.revoke());
    assert!(!lazy.revoke(), "lazy revocation is idempotent");
    assert!(!service.is_registered(&command("lazy.after")));
    assert!(
        service
            .resolve_keychord(&KeystrokeSpec::plain("a").expect("chord"), &context)
            .is_none()
    );
    assert!(service.button_states(&context).expect("buttons").is_empty());
    assert!(
        expect_completed(service.query_palette(PaletteQuery::new("after"), context)).is_empty()
    );
}

#[test]
fn registration_lease_revoke_is_idempotent_and_isolated() {
    let service = MenuCommandService::from_provider(FakeMenuProvider::new());
    let scope = service.create_scope(CommandScope::Global).expect("scope");
    let first = service
        .register_ui_in(
            &scope,
            CommandDescriptor::ui(command("lease.first"), "First").expect("descriptor"),
            |_: (), _| Ok(()),
        )
        .expect("first");
    let second = service
        .register_ui_in(
            &scope,
            CommandDescriptor::ui(command("lease.second"), "Second").expect("descriptor"),
            |_: (), _| Ok(()),
        )
        .expect("second");
    let (_, first_lease) = first.into_parts();
    assert!(first_lease.is_active());
    assert!(first_lease.revoke(), "the first revoke wins");
    assert!(!first_lease.revoke(), "the second revoke is a no-op");
    assert!(!first_lease.is_active());
    assert!(!service.is_registered(&command("lease.first")));
    assert!(
        service.is_registered(&command("lease.second")),
        "sibling registrations survive"
    );
    drop(first_lease);
    assert!(service.is_registered(&command("lease.second")));
    assert!(second.lease().is_active());
    assert!(second.lease().revoke());
    assert!(!service.is_registered(&command("lease.second")));
}

#[test]
fn revoking_single_lease_does_not_cancel_sibling_task_in_same_scope() {
    let (_owner, context) = ui_context(43);
    let service = MenuCommandService::from_provider(FakeMenuProvider::new());
    let scope = service.create_scope(CommandScope::Global).expect("scope");
    let (a_started_tx, a_started_rx) = mpsc::channel::<()>();
    let (a_release_tx, a_release_rx) = mpsc::channel::<()>();
    let a_release_rx = Arc::new(Mutex::new(a_release_rx));
    let (b_started_tx, b_started_rx) = mpsc::channel::<()>();
    let (b_release_tx, b_release_rx) = mpsc::channel::<()>();
    let b_release_rx = Arc::new(Mutex::new(b_release_rx));
    let first = service
        .register_background_in(
            &scope,
            CommandDescriptor::background(command("worker.sibling.a"), "Sibling A")
                .expect("descriptor"),
            move |_: (), _| {
                let _ = a_started_tx.send(());
                a_release_rx.lock().expect("gate").recv().expect("release");
                Ok(())
            },
        )
        .expect("first command");
    let second = service
        .register_background_in(
            &scope,
            CommandDescriptor::background(command("worker.sibling.b"), "Sibling B")
                .expect("descriptor"),
            move |_: (), _| {
                let _ = b_started_tx.send(());
                b_release_rx.lock().expect("gate").recv().expect("release");
                Ok(())
            },
        )
        .expect("second command");

    let subscription = service.subscribe();
    let first_request = service.invoke_typed(first.handle(), (), context.clone());
    let second_request = service.invoke_typed(second.handle(), (), context.clone());
    let mut tasks = BTreeMap::new();
    while tasks.len() < 2 {
        match next_event(&subscription) {
            MenuCommandEvent::CommandStarted(snapshot) => {
                tasks.insert(snapshot.command_id.clone(), snapshot.id);
            }
            other => panic!("unexpected event before both tasks started: {other:?}"),
        }
    }
    a_started_rx
        .recv_timeout(Duration::from_secs(10))
        .expect("first handler started");
    b_started_rx
        .recv_timeout(Duration::from_secs(10))
        .expect("second handler started");
    let first_id = tasks[&command("worker.sibling.a")];
    let second_id = tasks[&command("worker.sibling.b")];

    let (_, first_lease) = first.into_parts();
    assert!(first_lease.revoke());
    assert_eq!(
        service.command_task(first_id).expect("first task").state,
        CommandTaskState::Cancelled(CancelReason::OwnerDisposed)
    );
    assert_eq!(
        service.command_task(second_id).expect("second task").state,
        CommandTaskState::Running,
        "a sibling task in the same persistent scope must keep running"
    );
    assert_eq!(
        block_on(first_request).expect("first result"),
        Completion::Cancelled(CancelReason::OwnerDisposed)
    );
    assert!(service.is_registered(&command("worker.sibling.b")));
    assert!(second.lease().is_active());

    b_release_tx.send(()).expect("release second worker");
    assert_eq!(
        block_on(second_request).expect("second result"),
        Completion::Completed(())
    );
    assert_eq!(
        service.command_task(second_id).expect("second task").state,
        CommandTaskState::Completed
    );
    a_release_tx.send(()).expect("release first worker");
}

#[allow(dead_code)]
fn _types(_: LazyContribution, _: RegistrationLease) {}
