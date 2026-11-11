mod support;

use gpui_lifecycle::{AppPhase, LifecycleState, MemoryPressure, SessionLockState as LockState};
use gpui_menus::{ContextExpr, ContextKey, ContextValue, MenuNode, RegistrationLease};
use gpui_tray::*;
use std::{cell::Cell, rc::Rc};
use support::*;

type RestoreSetup = (
    FakeTrayProvider,
    TrayService,
    Rc<Cell<u32>>,
    StatusItemLease,
    UiOwner,
    RegistrationLease,
);

fn restore_setup() -> RestoreSetup {
    let (fake, service, _provider, menus) = fake_service_with_menus();
    let runs = Rc::new(Cell::new(0));
    let registration = register_command(&menus, "app.restore", runs.clone());
    service.set_residency_policy(
        TrayResidencyPolicy::new()
            .with_window_close_policy(WindowClosePolicy::MinimizeToTrayWhenAvailable)
            .with_restore_command(command_id("app.restore")),
    );
    let (owner, context) = ui_context(2);
    service.attach_surface(SurfaceId(2), 1, true);
    let spec = StatusItemSpec::new(key("app.status"), icon_set())
        .with_title("Status")
        .with_accessibility_label("Application status")
        .with_role(StatusItemRole::Restore)
        .with_click_action(
            TrayClickTrigger::PrimaryClick,
            TrayClickAction::RestoreOrFocusApp,
        );
    let lease = completed(service.create_application_status_item(spec, &context));
    (fake, service, runs, lease, owner, registration)
}

#[test]
fn close_normally_allows_quit_and_multiple_surfaces_keep_running() {
    let (fake, service) = fake_service();
    service.attach_surface(SurfaceId(1), 1, true);
    service.attach_surface(SurfaceId(2), 1, true);
    assert_eq!(service.visible_surface_count(), 2);
    assert_eq!(
        service.evaluate_close(SurfaceId(1), 1),
        CloseInterceptionOutcome::AllowWindowCloseKeepRunning
    );
    assert_eq!(
        service.evaluate_close(SurfaceId(2), 1),
        CloseInterceptionOutcome::AllowWindowCloseAndQuit
    );
    assert!(!service.is_quitting());
    assert!(fake.active_item_tokens().is_empty());
}

#[test]
fn close_to_tray_refuses_to_hide_without_shell_capability_or_item() {
    let (fake, service, _provider, _menus) = fake_service_with_menus();
    service.set_residency_policy(
        TrayResidencyPolicy::new()
            .with_window_close_policy(WindowClosePolicy::MinimizeToTrayWhenAvailable),
    );
    service.attach_surface(SurfaceId(2), 1, true);
    assert_eq!(
        service.evaluate_close(SurfaceId(2), 1),
        CloseInterceptionOutcome::PreventHideFallbackRestore {
            reason: ResidencyFallbackReason::NoHostedStatusItem
        }
    );
    assert!(!service.residing_in_tray());

    let (_owner, context) = ui_context(2);
    let lease = completed(service.create_surface_status_item(
        spec("app.status").with_click_action(
            TrayClickTrigger::PrimaryClick,
            TrayClickAction::RestoreOrFocusApp,
        ),
        SurfaceId(2),
        1,
        &context,
    ));
    fake.set_shell_status(ShellTrayStatus::Absent);
    assert_eq!(
        service.evaluate_close(SurfaceId(2), 1),
        CloseInterceptionOutcome::PreventHideFallbackRestore {
            reason: ResidencyFallbackReason::ShellAbsent
        }
    );
    fake.set_shell_status(ShellTrayStatus::Available { shell_epoch: 1 });
    fake.set_support(
        TrayOperation::ManageCloseToTrayResidency,
        Support::Unsupported,
    );
    assert_eq!(
        service.evaluate_close(SurfaceId(2), 1),
        CloseInterceptionOutcome::PreventHideFallbackRestore {
            reason: ResidencyFallbackReason::CapabilityUnavailable
        }
    );
    assert!(lease.is_active());
}

#[test]
fn close_to_tray_hides_only_with_a_reachable_application_lease() {
    let (fake, service, _runs, lease, _owner, _registration) = restore_setup();
    let outcome = service.evaluate_close(SurfaceId(2), 1);
    assert_eq!(
        outcome,
        CloseInterceptionOutcome::HideWindowToTray {
            item_id: lease.id()
        }
    );
    assert!(service.residing_in_tray());
    assert_eq!(service.visible_surface_count(), 0);
    assert!(lease.is_active());
    assert_eq!(fake.active_item_tokens(), vec![lease.native_item_token()]);
}

#[test]
fn surface_scoped_item_on_the_closing_surface_never_authorizes_hiding() {
    let (_fake, service, _provider, menus) = fake_service_with_menus();
    let runs = Rc::new(Cell::new(0));
    let _registration = register_command(&menus, "app.restore", runs);
    service.set_residency_policy(
        TrayResidencyPolicy::new()
            .with_window_close_policy(WindowClosePolicy::MinimizeToTrayWhenAvailable)
            .with_restore_command(command_id("app.restore")),
    );
    let (_owner, context) = ui_context(2);
    service.attach_surface(SurfaceId(2), 1, true);
    let _lease = completed(service.create_surface_status_item(
        spec("app.status").with_click_action(
            TrayClickTrigger::PrimaryClick,
            TrayClickAction::invoke_command(command_id("app.restore")),
        ),
        SurfaceId(2),
        1,
        &context,
    ));
    assert_eq!(
        service.evaluate_close(SurfaceId(2), 1),
        CloseInterceptionOutcome::PreventHideFallbackRestore {
            reason: ResidencyFallbackReason::NoEligibleStatusItem
        }
    );
    assert_eq!(service.active_item_count(), 1);
    assert!(!service.residing_in_tray());
}

#[test]
fn disabled_menu_exit_path_does_not_authorize_hiding() {
    let (_fake, service, _provider, menus) = fake_service_with_menus();
    let runs = Rc::new(Cell::new(0));
    let _registration = register_command(&menus, "app.quit", runs);
    let enabled_key = ContextKey::new("tray.menu.enabled").expect("key");
    menus.set_global_context(enabled_key.clone(), ContextValue::Bool(false));
    service.set_residency_policy(
        TrayResidencyPolicy::new()
            .with_window_close_policy(WindowClosePolicy::MinimizeToTrayWhenAvailable)
            .with_quit_command(command_id("app.quit")),
    );
    let (_owner, context) = ui_context(2);
    service.attach_surface(SurfaceId(2), 1, true);
    let spec = StatusItemSpec::new(key("app.status"), icon_set())
        .with_title("Status")
        .with_accessibility_label("Application status")
        .with_menu(ContextMenu::new(vec![
            MenuNode::command(command_id("app.quit")).enabled_when(ContextExpr::equals(
                enabled_key.clone(),
                ContextValue::Bool(true),
            )),
        ]));
    let _lease = completed(service.create_application_status_item(spec, &context));
    assert_eq!(
        service.evaluate_close(SurfaceId(2), 1),
        CloseInterceptionOutcome::PreventHideFallbackRestore {
            reason: ResidencyFallbackReason::NoReachableRestoreOrQuit
        }
    );
    menus.set_global_context(enabled_key, ContextValue::Bool(true));
    assert_eq!(
        service.evaluate_close(SurfaceId(2), 1),
        CloseInterceptionOutcome::HideWindowToTray {
            item_id: StatusItemId(1)
        }
    );
}

#[test]
fn registered_click_and_menu_quit_paths_authorize_hiding() {
    let (fake, service, _provider, menus) = fake_service_with_menus();
    let runs = Rc::new(Cell::new(0));
    let _registration = register_command(&menus, "app.quit", runs);
    service.set_residency_policy(
        TrayResidencyPolicy::new()
            .with_window_close_policy(WindowClosePolicy::MinimizeToTrayWhenAvailable)
            .with_quit_command(command_id("app.quit")),
    );
    let (_owner, context) = ui_context(2);
    service.attach_surface(SurfaceId(2), 1, true);
    let click_spec = StatusItemSpec::new(key("app.status"), icon_set())
        .with_title("Status")
        .with_accessibility_label("Application status")
        .with_click_action(
            TrayClickTrigger::PrimaryClick,
            TrayClickAction::invoke_command(command_id("app.quit")),
        );
    let _lease = completed(service.create_application_status_item(click_spec, &context));
    assert_eq!(
        service.evaluate_close(SurfaceId(2), 1),
        CloseInterceptionOutcome::HideWindowToTray {
            item_id: StatusItemId(1)
        }
    );
    assert!(service.residing_in_tray());
    assert_eq!(fake.active_item_tokens().len(), 1);
}

#[test]
fn mid_residency_last_lease_drop_breaks_residency_with_recovery() {
    let (fake, service, _runs, lease, _owner, _registration) = restore_setup();
    let subscription = service.subscribe();
    assert_eq!(
        service.evaluate_close(SurfaceId(2), 1),
        CloseInterceptionOutcome::HideWindowToTray {
            item_id: lease.id()
        }
    );
    assert!(service.residing_in_tray());
    drop(lease);
    assert!(!service.residing_in_tray());
    assert!(fake.active_item_tokens().is_empty());
    let events = drain(&subscription);
    assert!(events.contains(&Event::Value(TrayEvent::ResidencyBroken {
        reason: ResidencyBrokenReason::LastStatusItemRemoved,
        recovery: ResidencyRecoveryAction::RestorePrimarySurface,
    })));
    assert!(events.contains(&Event::Value(TrayEvent::ResidencyChanged {
        residing_in_tray: false,
    })));
}

#[test]
fn mid_residency_shell_loss_breaks_and_requests_recovery() {
    let (fake, service, _runs, _lease, _owner, _registration) = restore_setup();
    let subscription = service.subscribe();
    assert!(matches!(
        service.evaluate_close(SurfaceId(2), 1),
        CloseInterceptionOutcome::HideWindowToTray { .. }
    ));
    fake.simulate_shell_lost();
    assert!(!service.residing_in_tray());
    let events = drain(&subscription);
    assert!(events.contains(&Event::Value(TrayEvent::ShellLost)));
    assert!(events.contains(&Event::Value(TrayEvent::ResidencyBroken {
        reason: ResidencyBrokenReason::ShellLost,
        recovery: ResidencyRecoveryAction::RestorePrimarySurface,
    })));
}

#[test]
fn updating_away_the_only_reachable_path_breaks_residency() {
    let (fake, service, _runs, lease, _owner, _registration) = restore_setup();
    let subscription = service.subscribe();
    assert!(matches!(
        service.evaluate_close(SurfaceId(2), 1),
        CloseInterceptionOutcome::HideWindowToTray { .. }
    ));
    let (_owner, context) = ui_context(2);
    let spec = StatusItemSpec::new(key("app.status"), icon_set())
        .with_title("Status")
        .with_accessibility_label("Application status");
    completed(lease.update(spec, &context));
    assert!(!service.residing_in_tray());
    let events = drain(&subscription);
    assert!(events.contains(&Event::Value(TrayEvent::ResidencyBroken {
        reason: ResidencyBrokenReason::ReachabilityChanged,
        recovery: ResidencyRecoveryAction::RestorePrimarySurface,
    })));
    assert!(fake.active_item_tokens().len() == 1);
}

#[test]
fn external_menu_context_change_breaks_residency_on_refresh() {
    let (fake, service, _provider, menus) = fake_service_with_menus();
    let runs = Rc::new(Cell::new(0));
    let _registration = register_command(&menus, "app.quit", runs);
    let enabled_key = ContextKey::new("tray.menu.enabled").expect("key");
    let mut policy = TrayResidencyPolicy::new()
        .with_window_close_policy(WindowClosePolicy::MinimizeToTrayWhenAvailable)
        .with_quit_command(command_id("app.quit"));
    policy.shell_absence_fallback = ShellAbsenceFallback::RestorePrimarySurface;
    service.set_residency_policy(policy);
    menus.set_global_context(enabled_key.clone(), ContextValue::Bool(true));
    let (_owner, context) = ui_context(2);
    service.attach_surface(SurfaceId(2), 1, true);
    let spec = StatusItemSpec::new(key("app.status"), icon_set())
        .with_title("Status")
        .with_accessibility_label("Application status")
        .with_menu(ContextMenu::new(vec![
            MenuNode::command(command_id("app.quit")).enabled_when(ContextExpr::equals(
                enabled_key.clone(),
                ContextValue::Bool(true),
            )),
        ]));
    let _lease = completed(service.create_application_status_item(spec, &context));
    assert!(matches!(
        service.evaluate_close(SurfaceId(2), 1),
        CloseInterceptionOutcome::HideWindowToTray { .. }
    ));
    menus.set_global_context(enabled_key, ContextValue::Bool(false));
    service.refresh_residency();
    assert!(!service.residing_in_tray());
    assert_eq!(fake.active_item_tokens().len(), 1);
}

#[test]
fn shell_restart_recreates_items_in_creation_order_with_fresh_tokens_and_icons() {
    let (fake, service, _provider, menus) = fake_service_with_menus();
    let runs = Rc::new(Cell::new(0));
    let _registration = register_command(&menus, "app.restore", runs);
    service.set_residency_policy(
        TrayResidencyPolicy::new()
            .with_window_close_policy(WindowClosePolicy::MinimizeToTrayWhenAvailable)
            .with_restore_command(command_id("app.restore")),
    );
    let (_owner, context) = ui_context(2);
    service.attach_surface(SurfaceId(2), 1, true);
    let icons = StatusIconSet::new([
        icon_variant("light", ColorSchemeMatch::Light, ScaleBucket::X1),
        icon_variant("dark", ColorSchemeMatch::Dark, ScaleBucket::X2),
    ])
    .expect("icons");
    let mut leases = Vec::new();
    for name in ["one", "two"] {
        let spec = StatusItemSpec::new(key(name), icons.clone())
            .with_title("Status")
            .with_accessibility_label("Application status")
            .with_click_action(
                TrayClickTrigger::PrimaryClick,
                TrayClickAction::RestoreOrFocusApp,
            );
        leases.push(completed(
            service.create_application_status_item(spec, &context),
        ));
    }
    let close_outcome = service.evaluate_close(SurfaceId(2), 1);
    assert!(
        matches!(
            close_outcome,
            CloseInterceptionOutcome::HideWindowToTray { .. }
        ),
        "{close_outcome:?}"
    );
    let old_tokens = fake.active_item_tokens();
    service.notify_preferences(SystemPreferences {
        appearance: PreferenceValue::Known(Appearance::Dark),
        ..SystemPreferences::default()
    });
    service.notify_scale_factor_milli(2000);
    let subscription = service.subscribe();
    let epoch = fake.simulate_shell_restarted();
    assert_eq!(epoch, 2);
    let calls = fake.create_calls();
    assert_eq!(calls.len(), 4, "two initial creates plus two recoveries");
    assert_eq!(calls[2].item_id, StatusItemId(1));
    assert_eq!(calls[3].item_id, StatusItemId(2));
    assert_eq!(
        calls[2].projection.icon.payload,
        IconPayload::AssetKey("tray/dark.png".to_owned())
    );
    let new_tokens = fake.active_item_tokens();
    assert_eq!(new_tokens.len(), 2);
    assert!(new_tokens.iter().all(|token| !old_tokens.contains(token)));
    assert!(service.residing_in_tray());
    assert_eq!(leases.len(), 2);
    let events = drain(&subscription);
    assert!(
        events.contains(&Event::Value(TrayEvent::ShellRestartRecovered {
            shell_epoch: 2,
            restored_count: 2,
        }))
    );
}

#[test]
fn explicit_quit_overrides_close_to_tray_and_removes_every_lease() {
    let (fake, service, _runs, _lease, _owner, _registration) = restore_setup();
    let (_owner2, context) = ui_context(2);
    let second = completed(service.create_application_status_item(
        spec("app.second").with_click_action(
            TrayClickTrigger::PrimaryClick,
            TrayClickAction::RestoreOrFocusApp,
        ),
        &context,
    ));
    assert!(matches!(
        service.evaluate_close(SurfaceId(2), 1),
        CloseInterceptionOutcome::HideWindowToTray { .. }
    ));
    service.request_quit();
    assert!(service.is_quitting());
    assert!(!service.residing_in_tray());
    assert_eq!(
        service.evaluate_close(SurfaceId(2), 1),
        CloseInterceptionOutcome::AllowWindowCloseAndQuit
    );
    assert!(fake.active_item_tokens().is_empty());
    assert_eq!(fake.remove_call_count(), 2);
    assert!(!second.is_active());
}

#[test]
fn terminating_lifecycle_and_explicit_quit_cleanly_remove_items() {
    let (fake, service, _runs, lease, _owner, _registration) = restore_setup();
    let lifecycle = LifecycleState::from_parts(
        AppPhase::Terminating,
        [(SurfaceId(2), visible_snapshot(1))],
        LockState::Unlocked,
        MemoryPressure::Normal,
    );
    service.notify_lifecycle(&lifecycle);
    assert!(service.is_quitting());
    assert!(!lease.is_active());
    assert!(fake.active_item_tokens().is_empty());
    assert_eq!(
        service.evaluate_close(SurfaceId(2), 1),
        CloseInterceptionOutcome::AllowWindowCloseAndQuit
    );
}

#[test]
fn shell_absence_fallback_quit_is_used_when_configured() {
    let (_fake, service) = fake_service();
    service.set_residency_policy(
        TrayResidencyPolicy::new()
            .with_window_close_policy(WindowClosePolicy::MinimizeToTrayWhenAvailable)
            .with_shell_absence_fallback(ShellAbsenceFallback::QuitCleanly),
    );
    service.attach_surface(SurfaceId(2), 1, true);
    assert_eq!(
        service.evaluate_close(SurfaceId(2), 1),
        CloseInterceptionOutcome::PreventHideFallbackQuit {
            reason: ResidencyFallbackReason::NoHostedStatusItem
        }
    );
}

#[test]
fn restore_click_and_surface_visibility_clear_residency() {
    let (fake, service, _runs, lease, _owner, _registration) = restore_setup();
    let subscription = service.subscribe();
    assert!(matches!(
        service.evaluate_close(SurfaceId(2), 1),
        CloseInterceptionOutcome::HideWindowToTray { .. }
    ));
    assert!(fake.simulate_click(lease.id(), TrayClickTrigger::PrimaryClick));
    assert!(!service.residing_in_tray());
    let events = drain(&subscription);
    assert!(events.contains(&Event::Value(TrayEvent::ClickDispatched {
        id: lease.id(),
        trigger: TrayClickTrigger::PrimaryClick,
        action: TrayClickAction::RestoreOrFocusApp,
    })));

    service.attach_surface(SurfaceId(2), 1, true);
    service.set_residency_policy(
        TrayResidencyPolicy::new()
            .with_window_close_policy(WindowClosePolicy::MinimizeToTrayWhenAvailable)
            .with_restore_command(command_id("app.restore")),
    );
    assert!(matches!(
        service.evaluate_close(SurfaceId(2), 1),
        CloseInterceptionOutcome::HideWindowToTray { .. }
    ));
    service.notify_surface_visible(SurfaceId(2), 1);
    assert!(!service.residing_in_tray());
}

#[test]
fn hidden_surface_items_recover_across_shell_restarts_and_remain_hosted_when_visible() {
    let (fake, service) = fake_service();
    let (_owner, context) = ui_context(31);
    service.attach_surface(SurfaceId(31), 1, true);
    let lease = completed(service.create_surface_status_item(
        spec("app.status").with_click_action(
            TrayClickTrigger::PrimaryClick,
            TrayClickAction::RestoreOrFocusApp,
        ),
        SurfaceId(31),
        1,
        &context,
    ));
    let old_token = lease.native_item_token();
    service.attach_surface(SurfaceId(31), 1, false);
    assert!(lease.is_active());
    assert_eq!(service.active_item_count(), 1);

    let epoch = fake.simulate_shell_restarted();
    assert_eq!(epoch, 2);
    assert_eq!(fake.create_call_count(), 2);
    let new_token = lease.native_item_token();
    assert_ne!(new_token, old_token);
    assert!(lease.is_active());
    assert!(lease.snapshot().expect("snapshot").hosted);
    assert_eq!(fake.active_item_tokens(), vec![new_token]);
    assert_eq!(
        fake.create_calls()[1].item_id,
        lease.id(),
        "the hidden surface item is recovered in the new shell epoch"
    );

    service.notify_surface_visible(SurfaceId(31), 1);
    assert!(lease.is_active());
    assert!(lease.snapshot().expect("snapshot").hosted);
    assert_eq!(lease.native_item_token(), new_token);
    assert_eq!(fake.active_item_tokens(), vec![new_token]);
}
