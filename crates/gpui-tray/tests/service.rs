mod support;

use gpui_tray::*;
use std::{cell::RefCell, rc::Rc};
use support::*;

fn assert_active_zero(service: &TrayService, fake: &FakeTrayProvider) {
    assert_eq!(service.active_item_count(), 0);
    assert!(fake.active_item_tokens().is_empty());
}

#[test]
fn create_update_remove_lifecycle_and_idempotent_raii_drop() {
    let (fake, service) = fake_service();
    let (_owner, context) = ui_context(1);
    let lease = completed(service.create_application_status_item(spec("app.status"), &context));
    assert!(lease.is_active());
    assert_eq!(lease.id(), StatusItemId(1));
    assert_eq!(lease.revision(), 1);
    assert_eq!(service.active_item_count(), 1);
    assert_eq!(fake.create_call_count(), 1);
    assert_eq!(
        fake.create_calls()[0].projection.accessibility_label,
        "Application status"
    );
    assert_eq!(
        fake.create_calls()[0].projection.title.as_deref(),
        Some("Status")
    );
    assert_eq!(fake.active_item_tokens(), vec![lease.native_item_token()]);

    let updated = completed(
        lease.update(
            spec("app.status")
                .with_title("Updated")
                .with_accessibility_label("Updated status"),
            &context,
        ),
    );
    assert_eq!(updated.revision, 2);
    assert_eq!(fake.update_call_count(), 1);
    assert_eq!(
        fake.update_calls()[0].1.projection.title.as_deref(),
        Some("Updated")
    );
    assert_eq!(
        service
            .status_item_snapshot(lease.id())
            .expect("snapshot")
            .revision,
        2
    );

    assert!(lease.remove());
    assert!(!lease.remove());
    drop(lease);
    assert_eq!(fake.remove_call_count(), 1);
    assert_active_zero(&service, &fake);
}

#[test]
fn duplicate_active_or_reserved_keys_are_rejected_without_provider_calls() {
    let (fake, service) = fake_service();
    let (_owner, context) = ui_context(1);
    let _first = completed(service.create_application_status_item(spec("app.status"), &context));
    let error = service
        .create_application_status_item(spec("app.status"), &context)
        .expect_err("duplicate key");
    assert_eq!(error.code, ErrorCode::Busy);
    assert_eq!(fake.create_call_count(), 1);
}

#[test]
fn failed_creation_cleans_the_key_reservation() {
    let (fake, service) = fake_service();
    let (_owner, context) = ui_context(1);
    fake.set_create_failure(Some(SystemError::new(
        ErrorCode::PlatformFailure,
        "injected create failure",
    )));
    let error = service
        .create_application_status_item(spec("app.status"), &context)
        .expect_err("create failure");
    assert_eq!(error.code, ErrorCode::PlatformFailure);
    assert_eq!(service.active_item_count(), 0);
    fake.set_create_failure(None);
    let _lease = completed(service.create_application_status_item(spec("app.status"), &context));
    assert_eq!(service.active_item_count(), 1);
}

#[test]
fn surface_scoped_leases_require_a_live_matching_generation() {
    let (fake, service) = fake_service();
    let (_owner, context) = ui_context(7);
    let unmatched = service
        .create_surface_status_item(spec("app.status"), SurfaceId(7), 1, &context)
        .expect_err("surface is not attached");
    assert_eq!(unmatched.code, ErrorCode::ContextExpired);

    service.attach_surface(SurfaceId(7), 2, true);
    let stale = service
        .create_surface_status_item(spec("app.status"), SurfaceId(7), 1, &context)
        .expect_err("generation mismatch");
    assert_eq!(stale.code, ErrorCode::ContextExpired);
    assert_eq!(fake.create_call_count(), 0);

    let lease = completed(service.create_surface_status_item(
        spec("app.status"),
        SurfaceId(7),
        2,
        &OperationContext::default(),
    ));
    service.advance_surface_generation(SurfaceId(7), 3);
    assert!(!lease.is_active());
    assert_eq!(fake.remove_call_count(), 1);
    assert_active_zero(&service, &fake);
}

#[test]
fn disposing_a_surface_removes_only_its_scoped_leases() {
    let (fake, service) = fake_service();
    let (_owner, context) = ui_context(8);
    service.attach_surface(SurfaceId(8), 1, true);
    let surface_lease = completed(service.create_surface_status_item(
        spec("surface.status"),
        SurfaceId(8),
        1,
        &context,
    ));
    let app_lease = completed(service.create_application_status_item(spec("app.status"), &context));
    service.dispose_surface(SurfaceId(8));
    assert!(!surface_lease.is_active());
    assert!(app_lease.is_active());
    assert_eq!(service.active_item_count(), 1);
    assert_eq!(fake.remove_call_count(), 1);
}

#[test]
fn duplicate_status_item_identity_is_never_reused() {
    let (_fake, service) = fake_service();
    let (_owner, context) = ui_context(1);
    let first = completed(service.create_application_status_item(spec("first"), &context));
    let first_id = first.id();
    drop(first);
    let second = completed(service.create_application_status_item(spec("second"), &context));
    assert!(second.id() > first_id);
}

#[test]
fn mid_create_cancellation_removes_the_native_token_and_registers_nothing() {
    let fake = FakeTrayProvider::new();
    let token = CancellationToken::default();
    let hook_token = token.clone();
    fake.set_on_create_item(Some(Rc::new(move |_| {
        hook_token.cancel(CancelReason::User);
    })));
    let service = TrayService::from_fake(fake.clone());
    let context = OperationContext {
        cancellation: token,
        ..OperationContext::default()
    };
    let result = service.create_application_status_item(spec("app.status"), &context);
    assert!(matches!(
        result,
        Ok(Completion::Cancelled(CancelReason::User))
    ));
    assert_active_zero(&service, &fake);
    assert_eq!(fake.create_call_count(), 1);
    assert_eq!(fake.remove_calls(), vec![1]);
}

#[test]
fn mid_create_shell_loss_removes_the_orphan_without_a_lease() {
    let fake = FakeTrayProvider::new();
    let hook_fake = fake.clone();
    fake.set_on_create_item(Some(Rc::new(move |_| {
        hook_fake.simulate_shell_lost();
    })));
    let service = TrayService::from_fake(fake.clone());
    let error = service
        .create_application_status_item(spec("app.status"), &context())
        .expect_err("shell lost mid-create");
    assert_eq!(error.code, ErrorCode::ContextExpired);
    assert_active_zero(&service, &fake);
    assert_eq!(fake.remove_calls(), vec![1]);
}

#[test]
fn mid_create_shell_restart_removes_the_old_epoch_orphan() {
    let fake = FakeTrayProvider::new();
    let hook_fake = fake.clone();
    fake.set_on_create_item(Some(Rc::new(move |_| {
        hook_fake.simulate_shell_restarted();
    })));
    let service = TrayService::from_fake(fake.clone());
    let error = service
        .create_application_status_item(spec("app.status"), &context())
        .expect_err("shell restart mid-create");
    assert_eq!(error.code, ErrorCode::ContextExpired);
    assert_active_zero(&service, &fake);
    assert_eq!(fake.remove_calls(), vec![1]);
    assert_eq!(service.shell_epoch(), Some(2));
}

#[test]
fn mid_create_surface_disposal_removes_the_orphan() {
    let fake = FakeTrayProvider::new();
    let holder: Rc<RefCell<Option<TrayService>>> = Rc::new(RefCell::new(None));
    let hook_holder = holder.clone();
    fake.set_on_create_item(Some(Rc::new(move |_| {
        if let Some(service) = hook_holder.borrow().as_ref() {
            service.dispose_surface(SurfaceId(9));
        }
    })));
    let service = TrayService::from_fake(fake.clone());
    *holder.borrow_mut() = Some(service.clone());
    let (_owner, context) = ui_context(9);
    service.attach_surface(SurfaceId(9), 1, true);
    let error = service
        .create_surface_status_item(spec("app.status"), SurfaceId(9), 1, &context)
        .expect_err("surface disposed mid-create");
    assert_eq!(error.code, ErrorCode::ContextExpired);
    assert_active_zero(&service, &fake);
    assert_eq!(fake.remove_calls(), vec![1]);
}

#[test]
fn mid_create_quit_removes_the_orphan_and_stays_quitting() {
    let fake = FakeTrayProvider::new();
    let holder: Rc<RefCell<Option<TrayService>>> = Rc::new(RefCell::new(None));
    let hook_holder = holder.clone();
    fake.set_on_create_item(Some(Rc::new(move |_| {
        if let Some(service) = hook_holder.borrow().as_ref() {
            service.request_quit();
        }
    })));
    let service = TrayService::from_fake(fake.clone());
    *holder.borrow_mut() = Some(service.clone());
    let error = service
        .create_application_status_item(spec("app.status"), &context())
        .expect_err("quit mid-create");
    assert_eq!(error.code, ErrorCode::Unavailable);
    assert!(service.is_quitting());
    assert_active_zero(&service, &fake);
    assert_eq!(fake.remove_calls(), vec![1]);
}

#[test]
fn update_rejects_a_removed_lease_and_a_key_change() {
    let (_fake, service) = fake_service();
    let (_owner, context) = ui_context(1);
    let lease = completed(service.create_application_status_item(spec("app.status"), &context));
    let wrong_key = lease
        .update(spec("other.status"), &context)
        .expect_err("key change");
    assert_eq!(wrong_key.code, ErrorCode::InvalidInput);
    let id = lease.id();
    assert!(lease.remove());
    let removed = service
        .update_status_item(id, spec("app.status"), &context)
        .expect_err("removed lease");
    assert_eq!(removed.code, ErrorCode::ContextExpired);
}

#[test]
fn stale_callbacks_are_ignored_without_dispatch_or_native_mutation() {
    let (fake, service) = fake_service();
    let (_owner, context) = ui_context(1);
    let lease = completed(service.create_application_status_item(
        spec("app.status").with_click_action(
            TrayClickTrigger::PrimaryClick,
            TrayClickAction::RestoreOrFocusApp,
        ),
        &context,
    ));
    let snapshot = service.status_item_snapshot(lease.id()).expect("snapshot");
    let event = TrayClickEvent {
        item_id: lease.id(),
        native_item_token: snapshot.native_item_token + 9,
        shell_epoch: snapshot.shell_epoch,
        trigger: TrayClickTrigger::PrimaryClick,
    };
    let outcome = completed(service.handle_click_event(&event));
    assert_eq!(
        outcome,
        TrayClickOutcome::Ignored(StaleCallbackReason::NativeTokenMismatch)
    );
    assert_eq!(service.stale_callback_count(), 1);

    let stale_epoch = TrayClickEvent {
        shell_epoch: snapshot.shell_epoch + 1,
        ..event
    };
    let outcome = completed(service.handle_click_event(&stale_epoch));
    assert_eq!(
        outcome,
        TrayClickOutcome::Ignored(StaleCallbackReason::ShellEpochMismatch)
    );

    let unknown = TrayClickEvent {
        item_id: StatusItemId(999),
        ..stale_epoch.clone()
    };
    let outcome = completed(service.handle_click_event(&unknown));
    assert_eq!(
        outcome,
        TrayClickOutcome::Ignored(StaleCallbackReason::UnknownStatusItem)
    );

    let good = TrayClickEvent {
        item_id: lease.id(),
        native_item_token: snapshot.native_item_token,
        shell_epoch: snapshot.shell_epoch,
        trigger: TrayClickTrigger::PrimaryClick,
    };
    fake.simulate_click_event(good.clone());
    assert_eq!(service.stale_callback_count(), 3);
    assert!(lease.is_active());

    service.request_quit();
    let outcome = completed(service.handle_click_event(&good));
    assert_eq!(
        outcome,
        TrayClickOutcome::Ignored(StaleCallbackReason::AppQuitting)
    );
}

#[test]
fn preference_and_scale_changes_reproject_icons_deterministically() {
    let fake = FakeTrayProvider::new();
    let service = TrayService::from_fake(fake.clone());
    let (_owner, context) = ui_context(1);
    let icons = StatusIconSet::new([
        icon_variant("light", ColorSchemeMatch::Light, ScaleBucket::X1),
        icon_variant("dark1", ColorSchemeMatch::Dark, ScaleBucket::X1),
        icon_variant("dark2", ColorSchemeMatch::Dark, ScaleBucket::X2),
    ])
    .expect("icons");
    let spec = StatusItemSpec::new(key("app.status"), icons)
        .with_title("Status")
        .with_accessibility_label("Status");
    let lease = completed(service.create_application_status_item(spec, &context));
    assert_eq!(
        fake.hosted_items()[0].projection.icon.payload,
        IconPayload::AssetKey("tray/light.png".to_owned())
    );

    let preferences = SystemPreferences {
        appearance: PreferenceValue::Known(Appearance::Dark),
        ..SystemPreferences::default()
    };
    service.notify_preferences(preferences);
    assert_eq!(
        fake.hosted_items()[0].projection.icon.payload,
        IconPayload::AssetKey("tray/dark1.png".to_owned())
    );
    service.notify_scale_factor_milli(2000);
    assert_eq!(
        fake.hosted_items()[0].projection.icon.payload,
        IconPayload::AssetKey("tray/dark2.png".to_owned())
    );
    assert_eq!(fake.update_call_count(), 2);
    assert_eq!(
        service
            .status_item_snapshot(lease.id())
            .expect("snapshot")
            .revision,
        3
    );
}

#[test]
fn bounded_event_ring_surfaces_resync_without_blocking_producers() {
    let fake = FakeTrayProvider::new();
    let service = TrayService::with_event_capacity(Rc::new(fake), 2);
    let subscription = service.subscribe();
    let (_owner, context) = ui_context(1);
    let lease = completed(service.create_application_status_item(spec("app.status"), &context));
    let id = lease.id();
    completed(service.update_status_item(id, spec("app.status").with_title("two"), &context));
    completed(service.update_status_item(id, spec("app.status").with_title("three"), &context));
    let payload = next_payload(&subscription);
    assert_eq!(payload, Some(Event::ResyncRequired));
    assert!(subscription.snapshot_required());
}

#[cfg(all(
    feature = "gpui",
    any(target_os = "windows", target_os = "macos", target_os = "linux")
))]
mod gpui_adapter_tests {
    use super::*;
    use gpui::TestAppContext;
    use gpui_tray::gpui_adapter::{
        TraySurfaceScope, init_tray, install_tray_service, tray_service,
    };

    #[test]
    fn generation_checked_disposal_and_stale_scope_drop_preserve_the_new_surface() {
        let fake = FakeTrayProvider::new();
        let service = TrayService::from_fake(fake.clone());
        let owner = UiOwner::new(SurfaceId(42));
        let old_scope = TraySurfaceScope::new(owner.clone(), service.clone());
        let old_lease = completed(old_scope.create_status_item(spec("surface.old")));
        assert!(old_lease.is_active());

        owner
            .advance_generation()
            .expect("advance owner generation");
        let new_scope = TraySurfaceScope::new(owner.clone(), service.clone());
        let new_lease = completed(new_scope.create_status_item(spec("surface.new")));
        assert!(!old_lease.is_active());
        assert!(new_lease.is_active());
        assert_eq!(service.active_item_count(), 1);
        let token = new_lease.native_item_token();

        service.dispose_surface_generation(SurfaceId(42), 1);
        assert!(new_lease.is_active());
        assert!(new_lease.snapshot().expect("snapshot").hosted);
        assert_eq!(new_lease.native_item_token(), token);
        assert_eq!(fake.active_item_tokens(), vec![token]);
        assert_eq!(service.visible_surface_count(), 1);

        drop(old_scope);
        assert!(new_scope.context().is_ok(), "new generation owner survives");
        assert!(new_lease.is_active());
        assert!(new_lease.snapshot().expect("snapshot").hosted);
        assert_eq!(new_lease.native_item_token(), token);
        assert_eq!(fake.active_item_tokens(), vec![token]);

        drop(new_scope);
        assert!(!new_lease.is_active());
        assert!(fake.active_item_tokens().is_empty());
        assert_eq!(service.visible_surface_count(), 0);
    }

    #[gpui::test]
    fn installs_global_and_scope_removes_surface_items_on_drop(cx: &mut TestAppContext) {
        cx.update(|cx| {
            let fake = FakeTrayProvider::new();
            let service = init_tray(cx, Rc::new(fake.clone()));
            assert!(
                tray_service(cx)
                    .expect("global")
                    .shares_state_with(&service)
            );
            let owner = UiOwner::new(SurfaceId(41));
            let mut scope = TraySurfaceScope::new(owner.clone(), service.clone());
            scope.subscribe();
            assert_eq!(scope.subscription_count(), 1);
            let lease = completed(scope.create_status_item(spec("surface.status")));
            assert!(lease.is_active());
            assert_eq!(service.visible_surface_count(), 1);
            drop(scope);
            assert!(!lease.is_active());
            assert_eq!(fake.remove_call_count(), 1);
            assert!(owner.context(None).is_err());
            install_tray_service(service.clone(), cx);
            assert!(
                tray_service(cx)
                    .expect("global")
                    .shares_state_with(&service)
            );
        });
    }
}
