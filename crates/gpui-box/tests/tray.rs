#![cfg(feature = "tray")]

use gpui_box::menus::{
    CommandDescriptor, CommandId, CommandScope, ContextMenu, FakeMenuProvider, MenuCommandService,
    MenuNode, RouteRegistry, SystemActionBinding,
};
use gpui_box::tray::*;
use std::sync::mpsc;
use std::{
    cell::Cell,
    future::Future,
    pin::Pin,
    rc::Rc,
    task::{Context, Poll, Waker},
};

fn command(value: &str) -> CommandId {
    CommandId::new(value).expect("valid command")
}

fn key(value: &str) -> StatusItemKey {
    StatusItemKey::new(value).expect("valid key")
}

fn icon_set() -> StatusIconSet {
    StatusIconSet::single(StatusIconVariant::new(
        IconPayload::asset_key("tray/status.png").expect("asset"),
        ColorSchemeMatch::Any,
        ScaleBucket::X1,
    ))
    .expect("icon set")
}

fn spec(value: &str) -> StatusItemSpec {
    StatusItemSpec::new(key(value), icon_set())
        .with_title("Status")
        .with_accessibility_label("Application status")
}

fn ui_context(surface: u64) -> (UiOwner, OperationContext) {
    let owner = UiOwner::new(SurfaceId(surface));
    let context = OperationContext {
        ui: Some(owner.context(None).expect("ui context")),
        ..OperationContext::default()
    };
    (owner, context)
}

fn completed<T>(result: ServiceResult<T>) -> T {
    match result.expect("operation succeeded") {
        Completion::Completed(value) => value,
        Completion::Cancelled(reason) => panic!("unexpected cancellation: {reason:?}"),
    }
}

fn next_payload<T>(subscription: &EventSubscription<T>) -> Option<Event<T>> {
    let mut next = subscription.try_next().ok()?;
    let waker = Waker::noop();
    let mut context = Context::from_waker(waker);
    match Pin::new(&mut next).poll(&mut context) {
        Poll::Ready(Some(EventEnvelope { payload, .. })) => Some(payload),
        Poll::Ready(None) | Poll::Pending => None,
    }
}

#[test]
fn facade_exports_the_coordinated_tray_contracts_and_thirty_cell_matrix() {
    assert_eq!(
        gpui_box::tray::UPSTREAM_GPUI_REVISION,
        gpui_box::UPSTREAM_REVISION
    );
    assert_eq!(gpui_box::tray::PACKAGE_VERSION, gpui_box::VERSION);
    assert_eq!(TrayOperation::ALL.len(), 5);
    assert_eq!(PlatformTarget::ALL.len(), 6);
    assert_eq!(DEFAULT_TRAY_EVENT_CAPACITY, 64);
    let matrix = platform_support_matrix();
    assert_eq!(matrix.len(), 30);
    let cells = matrix
        .iter()
        .map(|row| (row.platform, row.operation))
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(cells.len(), 30);
    for platform in PlatformTarget::ALL {
        assert_eq!(platform_operations(platform).len(), 5);
        for operation in TrayOperation::ALL {
            let support = platform.support(operation);
            if platform.is_mobile_or_web() {
                assert_eq!(support, Support::Unsupported);
            } else {
                assert!(matches!(support, Support::Native | Support::Conditional));
            }
        }
    }
}

#[test]
fn status_item_theme_dpi_lease_update_and_idempotent_removal() {
    let fake = FakeTrayProvider::new();
    let service = TrayService::from_fake(fake.clone());
    let (_owner, context) = ui_context(1);
    let icons = StatusIconSet::new([
        StatusIconVariant::new(
            IconPayload::asset_key("tray/light.png").expect("asset"),
            ColorSchemeMatch::Light,
            ScaleBucket::X1,
        ),
        StatusIconVariant::new(
            IconPayload::asset_key("tray/dark2.png").expect("asset"),
            ColorSchemeMatch::Dark,
            ScaleBucket::X2,
        ),
    ])
    .expect("icons");
    let item = StatusItemSpec::new(key("app.status"), icons)
        .with_title("Status")
        .with_accessibility_label("Application status");
    let lease = completed(service.create_application_status_item(item, &context));
    assert_eq!(fake.create_call_count(), 1);
    service.notify_preferences(SystemPreferences {
        appearance: PreferenceValue::Known(Appearance::Dark),
        ..SystemPreferences::default()
    });
    service.notify_scale_factor_milli(2000);
    assert_eq!(
        fake.hosted_items()[0].projection.icon.payload,
        IconPayload::AssetKey("tray/dark2.png".to_owned())
    );
    let revision =
        completed(lease.update(spec("app.status").with_title("Updated"), &context)).revision;
    assert!(revision >= 2);
    assert!(lease.remove());
    assert!(!lease.remove());
    drop(lease);
    assert_eq!(fake.remove_call_count(), 1);
    assert_eq!(service.active_item_count(), 0);
}

#[test]
fn live_menus_and_click_actions_invoke_commands_and_system_actions() {
    let fake = FakeTrayProvider::new();
    let menu_provider = Rc::new(FakeMenuProvider::new());
    let menus = MenuCommandService::new(menu_provider);
    let service = TrayService::from_fake_with_menus(fake.clone(), menus.clone());
    let runs: Rc<Cell<u32>> = Rc::new(Cell::new(0));
    let captured = runs.clone();
    let command_id = command("app.quit");
    let registration = menus
        .register_ui(
            CommandDescriptor::ui(command_id.clone(), "Quit").expect("descriptor"),
            move |_: (), _| {
                captured.set(captured.get().saturating_add(1));
                Ok(())
            },
        )
        .expect("registration");
    let scope = menus.create_scope(CommandScope::Global).expect("scope");
    let _system_lease = menus
        .register_system_action(
            &scope,
            SystemActionBinding::action("tray.quit", command_id.clone()).expect("binding"),
            &RouteRegistry::default(),
        )
        .expect("system action");
    let (_owner, context) = ui_context(2);
    service.attach_surface(SurfaceId(2), 1, true);
    let item = spec("app.status").with_menu(ContextMenu::new(vec![MenuNode::command(
        command_id.clone(),
    )]));
    let lease = completed(service.create_surface_status_item(item, SurfaceId(2), 1, &context));
    let resolved = completed(service.present_status_menu(lease.id(), &context));
    assert_eq!(resolved.all_items().len(), 1);
    assert_eq!(fake.menu_calls().len(), 1);
    assert!(fake.simulate_menu_item_click(lease.id(), command_id.clone()));
    assert_eq!(runs.get(), 1);

    let item = spec("app.status").with_click_action(
        TrayClickTrigger::DoubleClick,
        TrayClickAction::dispatch_system_action("tray.quit").expect("action"),
    );
    completed(service.update_status_item(lease.id(), item, &context));
    assert!(fake.simulate_click(lease.id(), TrayClickTrigger::DoubleClick));
    assert_eq!(runs.get(), 2);
    assert_eq!(menus.command_ids().len(), 1);
    drop(registration);
}

#[test]
fn residency_breaks_on_last_lease_loss_and_recovers_after_a_shell_restart() {
    let fake = FakeTrayProvider::new();
    let menu_provider = Rc::new(FakeMenuProvider::new());
    let menus = MenuCommandService::new(menu_provider);
    let service = TrayService::from_fake_with_menus(fake.clone(), menus.clone());
    let runs: Rc<Cell<u32>> = Rc::new(Cell::new(0));
    let captured = runs;
    let quit_id = command("app.quit");
    let _registration = menus
        .register_ui(
            CommandDescriptor::ui(quit_id.clone(), "Quit").expect("descriptor"),
            move |_: (), _| {
                captured.set(captured.get().saturating_add(1));
                Ok(())
            },
        )
        .expect("registration");
    service.set_residency_policy(
        TrayResidencyPolicy::new()
            .with_window_close_policy(WindowClosePolicy::MinimizeToTrayWhenAvailable)
            .with_quit_command(quit_id),
    );
    let (_owner, context) = ui_context(3);
    service.attach_surface(SurfaceId(3), 1, true);
    let item = spec("app.status").with_click_action(
        TrayClickTrigger::PrimaryClick,
        TrayClickAction::invoke_command(command("app.quit")),
    );
    let first = completed(service.create_application_status_item(item, &context));
    let subscription = service.subscribe();
    assert!(matches!(
        service.evaluate_close(SurfaceId(3), 1),
        CloseInterceptionOutcome::HideWindowToTray { .. }
    ));
    assert!(service.residing_in_tray());
    let old_token = first.native_item_token();
    drop(first);
    assert!(!service.residing_in_tray());
    let events = {
        let mut events = Vec::new();
        while let Some(payload) = next_payload(&subscription) {
            events.push(payload);
        }
        events
    };
    assert!(events.contains(&Event::Value(TrayEvent::ResidencyBroken {
        reason: ResidencyBrokenReason::LastStatusItemRemoved,
        recovery: ResidencyRecoveryAction::RestorePrimarySurface,
    })));

    // Recreate and verify Explorer/StatusNotifierWatcher-style restarting.
    let item = spec("app.status").with_click_action(
        TrayClickTrigger::PrimaryClick,
        TrayClickAction::InvokeCommand {
            command_id: command("app.quit"),
            payload: CommandPayload::Unit,
        },
    );
    let second = completed(service.create_application_status_item(item, &context));
    service.attach_surface(SurfaceId(3), 1, true);
    assert!(matches!(
        service.evaluate_close(SurfaceId(3), 1),
        CloseInterceptionOutcome::HideWindowToTray { .. }
    ));
    let epoch = fake.simulate_shell_restarted();
    assert_eq!(epoch, 2);
    assert_eq!(service.shell_epoch(), Some(2));
    assert_ne!(second.native_item_token(), old_token);
    assert!(service.residing_in_tray());
    assert_eq!(service.active_item_count(), 1);

    service.request_quit();
    assert!(service.is_quitting());
    assert_eq!(
        service.evaluate_close(SurfaceId(3), 1),
        CloseInterceptionOutcome::AllowWindowCloseAndQuit
    );
    assert!(fake.active_item_tokens().is_empty());
}

#[test]
fn explicit_quit_removes_all_leases_and_mobile_uses_explicit_alternatives() {
    let fake = FakeTrayProvider::new();
    let service = TrayService::from_fake(fake.clone());
    let (_owner, context) = ui_context(4);
    let _first = completed(service.create_application_status_item(
        spec("app.one").with_click_action(
            TrayClickTrigger::PrimaryClick,
            TrayClickAction::RestoreOrFocusApp,
        ),
        &context,
    ));
    let second = completed(service.create_application_status_item(
        spec("app.two").with_click_action(
            TrayClickTrigger::PrimaryClick,
            TrayClickAction::RestoreOrFocusApp,
        ),
        &context,
    ));
    service.request_quit();
    assert!(!second.is_active());
    assert!(fake.active_item_tokens().is_empty());
    assert_eq!(fake.remove_call_count(), 2);

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

    let web = TrayService::from_fake(FakeTrayProvider::with_platform(PlatformTarget::Web));
    web.set_residency_policy(TrayResidencyPolicy::new().with_alternative_surface(
        MobileAlternativeSurface::shortcut("tray.shortcut").expect("shortcut"),
    ));
    assert_eq!(
        web.resolve_surface_strategy(),
        EffectiveStatusSurface::UnsupportedNone
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
fn shell_absence_prevents_close_to_tray_and_quit_fallback_is_deterministic() {
    let fake = FakeTrayProvider::new();
    let service = TrayService::from_fake(fake.clone());
    let (_owner, context) = ui_context(5);
    service.attach_surface(SurfaceId(5), 1, true);
    service.set_residency_policy(
        TrayResidencyPolicy::new()
            .with_window_close_policy(WindowClosePolicy::MinimizeToTrayWhenAvailable)
            .with_shell_absence_fallback(ShellAbsenceFallback::QuitCleanly),
    );
    let _lease = completed(service.create_application_status_item(
        spec("app.status").with_click_action(
            TrayClickTrigger::PrimaryClick,
            TrayClickAction::RestoreOrFocusApp,
        ),
        &context,
    ));
    fake.set_shell_status(ShellTrayStatus::Absent);
    assert_eq!(
        service.evaluate_close(SurfaceId(5), 1),
        CloseInterceptionOutcome::PreventHideFallbackQuit {
            reason: ResidencyFallbackReason::ShellAbsent
        }
    );
    assert!(!service.residing_in_tray());
}

#[test]
fn bounded_event_stream_and_facade_reexport_surface_resync() {
    let fake = FakeTrayProvider::new();
    let service = TrayService::with_event_capacity(Rc::new(fake), 1);
    let subscription = service.subscribe();
    let (_owner, context) = ui_context(6);
    let lease = completed(service.create_application_status_item(spec("app.status"), &context));
    let id = lease.id();
    completed(service.update_status_item(id, spec("app.status").with_title("two"), &context));
    let first = next_payload(&subscription);
    assert_eq!(first, Some(Event::ResyncRequired));
}

#[test]
fn local_ui_thread_commands_are_dispatched_through_the_public_facade() {
    let fake = FakeTrayProvider::new();
    let menu_provider = Rc::new(FakeMenuProvider::new());
    let menus = MenuCommandService::new(menu_provider);
    let service = TrayService::from_fake_with_menus(fake.clone(), menus.clone());
    let runs: Rc<Cell<u32>> = Rc::new(Cell::new(0));
    let captured = runs.clone();
    let id = command("app.restore");
    let _registration = menus
        .register_ui(
            CommandDescriptor::ui(id.clone(), "Restore").expect("descriptor"),
            move |_: (), _| {
                captured.set(captured.get().saturating_add(1));
                Ok(())
            },
        )
        .expect("registration");
    let (_owner, context) = ui_context(7);
    service.attach_surface(SurfaceId(7), 1, true);
    let lease = completed(service.create_surface_status_item(
        spec("app.status").with_click_action(
            TrayClickTrigger::PrimaryClick,
            TrayClickAction::invoke_command(id),
        ),
        SurfaceId(7),
        1,
        &context,
    ));
    assert!(fake.simulate_click(lease.id(), TrayClickTrigger::PrimaryClick));
    assert_eq!(runs.get(), 1);

    let (sender, receiver) = mpsc::channel();
    std::thread::spawn(move || {
        let owner = UiOwner::new(SurfaceId(70));
        let foreign = owner.context(None).expect("foreign context");
        sender.send((owner, foreign)).expect("send context");
    });
    let (_foreign, foreign) = receiver.recv().expect("foreign context");
    let foreign_context = OperationContext {
        ui: Some(foreign),
        ..OperationContext::default()
    };
    let error = service
        .present_status_menu(lease.id(), &foreign_context)
        .expect_err("foreign thread");
    assert_eq!(error.code, ErrorCode::ContextExpired);
}
