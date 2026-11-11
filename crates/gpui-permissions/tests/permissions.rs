use gpui_permissions::*;
use std::{
    cell::RefCell,
    future::Future,
    pin::Pin,
    rc::Rc,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    task::{Context, Poll, Wake, Waker},
};

std::thread_local! {
    static REENTRANT_SERVICE: RefCell<Option<PermissionService>> = const { RefCell::new(None) };
}

struct ReentrantWake {
    woken: Arc<AtomicBool>,
}

impl Wake for ReentrantWake {
    fn wake(self: Arc<Self>) {
        self.wake_by_ref();
    }

    fn wake_by_ref(self: &Arc<Self>) {
        REENTRANT_SERVICE.with(|slot| {
            if let Some(service) = slot.borrow().as_ref().cloned() {
                let _ = service.active_prompt();
                let _ = service.revision();
            }
        });
        self.woken.store(true, Ordering::SeqCst);
    }
}

fn poll_ready<T: Future + Unpin>(future: &mut T) -> Poll<T::Output> {
    Pin::new(future).poll(&mut Context::from_waker(Waker::noop()))
}

fn outcome(request: &mut Request<PermissionOutcome>) -> PermissionOutcome {
    match poll_ready(request) {
        Poll::Ready(Ok(Completion::Completed(outcome))) => outcome,
        Poll::Ready(Ok(Completion::Cancelled(reason))) => {
            panic!("permission request was cancelled: {reason:?}")
        }
        Poll::Ready(Err(error)) => panic!("permission request failed: {error}"),
        Poll::Pending => panic!("permission request did not complete"),
    }
}

fn snapshot(request: &mut Request<PermissionSnapshot>) -> PermissionSnapshot {
    match poll_ready(request) {
        Poll::Ready(Ok(Completion::Completed(snapshot))) => snapshot,
        Poll::Ready(Ok(Completion::Cancelled(reason))) => {
            panic!("permission query was cancelled: {reason:?}")
        }
        Poll::Ready(Err(error)) => panic!("permission query failed: {error}"),
        Poll::Pending => panic!("permission query did not complete"),
    }
}

fn handoff(request: &mut Request<SettingsHandoff>) -> SettingsHandoff {
    match poll_ready(request) {
        Poll::Ready(Ok(Completion::Completed(handoff))) => handoff,
        Poll::Ready(Ok(Completion::Cancelled(reason))) => {
            panic!("settings handoff was cancelled: {reason:?}")
        }
        Poll::Ready(Err(error)) => panic!("settings handoff failed: {error}"),
        Poll::Pending => panic!("settings handoff did not complete"),
    }
}

fn expect_error<T>(request: &mut Request<T>) -> SystemError {
    match poll_ready(request) {
        Poll::Ready(Err(error)) => error,
        Poll::Ready(Ok(Completion::Completed(_))) => {
            panic!("request completed instead of returning an error")
        }
        Poll::Ready(Ok(Completion::Cancelled(reason))) => {
            panic!("request was cancelled: {reason:?}")
        }
        Poll::Pending => panic!("request did not complete"),
    }
}

fn expect_cancelled<T>(request: &mut Request<T>) -> CancelReason {
    match poll_ready(request) {
        Poll::Ready(Ok(Completion::Cancelled(reason))) => reason,
        Poll::Ready(Ok(Completion::Completed(_))) => {
            panic!("request completed before cancellation")
        }
        Poll::Ready(Err(error)) => panic!("request failed before cancellation: {error}"),
        Poll::Pending => panic!("cancelled request did not complete"),
    }
}

fn next_event(
    subscription: &EventSubscription<PermissionChange>,
) -> Option<EventEnvelope<PermissionChange>> {
    let mut next = subscription.try_next().unwrap();
    match poll_ready(&mut next) {
        Poll::Ready(event) => event,
        Poll::Pending => panic!("permission event did not arrive"),
    }
}

fn next_change(subscription: &EventSubscription<PermissionChange>) -> PermissionChange {
    match next_event(subscription) {
        Some(EventEnvelope {
            payload: Event::Value(change),
            ..
        }) => change,
        Some(EventEnvelope {
            payload: Event::ResyncRequired,
            ..
        }) => panic!("permission stream unexpectedly required resync"),
        None => panic!("permission stream closed unexpectedly"),
    }
}

fn ui_context(owner: &UiOwner) -> OperationContext {
    OperationContext {
        ui: Some(owner.context(None).unwrap()),
        ..Default::default()
    }
}

#[test]
fn notification_flow_reconciles_after_settings_change() {
    let owner = UiOwner::new(SurfaceId(20));
    let kind = PermissionKind::Notifications;
    let fake =
        Rc::new(FakePermissionProvider::new().with_status(kind, PermissionStatus::NotDetermined));
    let service = PermissionService::new(fake.clone());
    let changes = service.subscribe();

    let mut first = service.request_permission(PermissionRequest::new(kind), ui_context(&owner));
    assert!(poll_ready(&mut first).is_pending());
    assert_eq!(fake.pending_prompt_count(), 1);
    assert!(fake.deny_next(true));
    let first = outcome(&mut first);
    assert_eq!(
        first.status,
        PermissionStatus::Denied {
            can_request_again: true,
        }
    );
    assert!(first.prompted);
    assert!(first.changed);
    assert_eq!(first.recovery(), Some(RecoveryAction::OpenSettings));
    assert_eq!(next_change(&changes).current, first.status);

    let mut repeat = service.request_permission(PermissionRequest::new(kind), ui_context(&owner));
    assert!(poll_ready(&mut repeat).is_pending());
    assert!(fake.deny_next(false));
    let repeat = outcome(&mut repeat);
    assert_eq!(
        repeat.status,
        PermissionStatus::Denied {
            can_request_again: false,
        }
    );
    assert!(repeat.prompted);
    assert!(repeat.requires_settings());
    assert_eq!(next_change(&changes).current, repeat.status);

    // A permanently denied state resolves immediately with settings recovery;
    // it neither prompts nor waits for a native callback.
    let mut immediate =
        service.request_permission(PermissionRequest::new(kind), ui_context(&owner));
    let immediate = outcome(&mut immediate);
    assert!(!immediate.prompted);
    assert!(!immediate.changed);
    assert_eq!(immediate.status, repeat.status);
    assert_eq!(fake.pending_prompt_count(), 0);
    let error = immediate.into_result().unwrap_err();
    assert_eq!(error.code, ErrorCode::PermissionDenied);
    assert_eq!(error.recovery, Some(RecoveryAction::OpenSettings));

    let target = SettingsTarget::NotificationSettings;
    let mut settings = service.open_settings(target, ui_context(&owner));
    assert_eq!(handoff(&mut settings), SettingsHandoff::Opened);
    assert_eq!(fake.opened_targets(), vec![target]);

    // The settings receipt does not claim a toggle. Only a fresh probe does.
    fake.set_status(kind, PermissionStatus::Granted);
    let reconciliation = service.reconcile_on_resume().unwrap();
    assert_eq!(reconciliation.changes.len(), 1);
    assert_eq!(reconciliation.changes[0].current, PermissionStatus::Granted);
    assert!(reconciliation.revision > 0);
    let granted = next_change(&changes);
    assert_eq!(granted.current, PermissionStatus::Granted);
    assert!(granted.authorization_gained());

    let mut query = service.query(kind, ui_context(&owner));
    assert_eq!(snapshot(&mut query).status, PermissionStatus::Granted);
}

#[test]
fn missing_native_configuration_is_actionable() {
    let kind = PermissionKind::Camera;
    let fake = Rc::new(FakePermissionProvider::new().with_configuration_error(kind));
    let service = PermissionService::new(fake);

    let mut query = service.query(kind, OperationContext::default());
    let error = expect_error(&mut query);
    assert_eq!(error.code, ErrorCode::InvalidConfiguration);
    assert_eq!(error.recovery, Some(RecoveryAction::RepairConfiguration));

    let mut request =
        service.request_permission(PermissionRequest::new(kind), OperationContext::default());
    let error = expect_error(&mut request);
    assert_eq!(error.code, ErrorCode::InvalidConfiguration);
    assert_eq!(error.recovery, Some(RecoveryAction::RepairConfiguration));

    let capability = service.capability(PermissionOperation::Request(kind));
    assert_eq!(capability.configuration, Configuration::Required);
    assert_eq!(capability.support, Support::Conditional);
    assert_eq!(capability.evidence, Evidence::ContractOnly);

    let owner = UiOwner::new(SurfaceId(21));
    let target = SettingsTarget::AppDetails;
    let fake = Rc::new(FakePermissionProvider::new().with_settings_error(target));
    let service = PermissionService::new(fake.clone());
    let mut settings = service.open_settings(target, ui_context(&owner));
    let error = expect_error(&mut settings);
    assert_eq!(error.code, ErrorCode::InvalidConfiguration);
    assert_eq!(error.recovery, Some(RecoveryAction::RepairConfiguration));
    assert_eq!(fake.opened_targets(), vec![target]);

    let target = SettingsTarget::SpecialAccess("background-location");
    let fake = Rc::new(
        FakePermissionProvider::new().with_settings_handoff(target, SettingsHandoff::Unavailable),
    );
    let service = PermissionService::new(fake);
    let mut settings = service.open_settings(target, ui_context(&owner));
    assert_eq!(handoff(&mut settings), SettingsHandoff::Unavailable);
    let capability = service.capability(PermissionOperation::OpenSettings(target));
    assert_eq!(capability.availability, Availability::Unavailable);
}

#[test]
fn desktop_no_prompt_status_has_no_synthetic_ui() {
    let kind = PermissionKind::Notifications;
    let fake = Rc::new(
        FakePermissionProvider::new()
            .with_status(kind, PermissionStatus::NotRequired)
            .with_status(PermissionKind::Camera, PermissionStatus::Granted),
    );
    let service = PermissionService::new(fake.clone());

    let capabilities = service.capabilities();
    assert_eq!(capabilities.len(), PermissionKind::ALL.len() * 2 + 2);
    assert!(fake.pending_prompts().is_empty());
    assert!(fake.requested_kinds().is_empty());
    let request_capability = service.capability(PermissionOperation::Request(kind));
    assert_eq!(
        request_capability.operation,
        PermissionOperation::Request(kind)
    );
    assert_eq!(request_capability.permission, PermissionState::NotRequired);
    assert_eq!(request_capability.evidence, Evidence::ContractOnly);

    // No UI context is supplied: NotRequired must not require one and must not
    // create a synthetic prompt.
    let mut request =
        service.request_permission(PermissionRequest::new(kind), OperationContext::default());
    let not_required = outcome(&mut request);
    assert_eq!(not_required.status, PermissionStatus::NotRequired);
    assert!(!not_required.prompted);
    assert!(!not_required.changed);
    assert!(not_required.status.allows_use());
    assert!(fake.pending_prompts().is_empty());
    assert!(fake.requested_kinds().is_empty());

    let mut granted = service.request_permission(
        PermissionRequest::new(PermissionKind::Camera),
        OperationContext::default(),
    );
    let granted = outcome(&mut granted);
    assert_eq!(granted.status, PermissionStatus::Granted);
    assert!(!granted.prompted);
    assert!(fake.requested_kinds().is_empty());

    let mut query = service.query(kind, OperationContext::default());
    assert_eq!(snapshot(&mut query).status, PermissionStatus::NotRequired);
}

#[test]
fn concurrent_interactive_prompts_are_serialized() {
    let owner = UiOwner::new(SurfaceId(22));
    let kind = PermissionKind::Camera;
    let fake =
        Rc::new(FakePermissionProvider::new().with_status(kind, PermissionStatus::NotDetermined));
    let service = PermissionService::new(fake.clone());

    let mut first = service.request_permission(PermissionRequest::new(kind), ui_context(&owner));
    assert!(poll_ready(&mut first).is_pending());
    assert_eq!(fake.pending_prompt_count(), 1);

    let mut second = service.request_permission(PermissionRequest::new(kind), ui_context(&owner));
    let error = expect_error(&mut second);
    assert_eq!(error.code, ErrorCode::Busy);
    assert_eq!(error.retry, RetryAdvice::AfterConditionsChange);
    assert_eq!(fake.pending_prompt_count(), 1);
    assert!(fake.deny_next(true));
    assert_eq!(
        outcome(&mut first).status,
        PermissionStatus::Denied {
            can_request_again: true,
        }
    );
    assert!(service.active_prompt().is_none());
    // Cancellation releases the native prompt lane.
    fake.set_status(kind, PermissionStatus::NotDetermined);
    let context = ui_context(&owner);
    let cancellation = context.cancellation.clone();
    let mut cancelled = service.request_permission(PermissionRequest::new(kind), context);
    assert!(poll_ready(&mut cancelled).is_pending());
    cancellation.cancel(CancelReason::User);
    assert!(!fake.deny_next(true));
    assert_eq!(expect_cancelled(&mut cancelled), CancelReason::User);
    assert!(service.active_prompt().is_none());

    // A later prompt can start and releases the lane on completion.
    fake.set_status(kind, PermissionStatus::NotDetermined);
    let mut third = service.request_permission(PermissionRequest::new(kind), ui_context(&owner));
    assert!(poll_ready(&mut third).is_pending());
    assert!(fake.deny_next(false));
    assert_eq!(
        outcome(&mut third).status,
        PermissionStatus::Denied {
            can_request_again: false,
        }
    );
    assert!(service.active_prompt().is_none());
}

#[test]
fn worker_thread_prompt_and_settings_are_rejected() {
    let owner = UiOwner::new(SurfaceId(30));
    let worker_context = ui_context(&owner);
    let worker_owner = owner.clone();
    std::thread::spawn(move || {
        assert!(worker_owner.context(None).is_err());
        let fake = Rc::new(
            FakePermissionProvider::new()
                .with_status(PermissionKind::Camera, PermissionStatus::NotDetermined),
        );
        let service = PermissionService::new(fake.clone());
        let mut request = service.request_permission(
            PermissionRequest::new(PermissionKind::Camera),
            worker_context.clone(),
        );
        assert_eq!(expect_error(&mut request).code, ErrorCode::ContextExpired);
        assert!(fake.pending_prompts().is_empty());

        let mut settings = service.open_settings(SettingsTarget::AppDetails, worker_context);
        assert_eq!(expect_error(&mut settings).code, ErrorCode::ContextExpired);
        assert!(fake.opened_targets().is_empty());
    })
    .join()
    .unwrap();
}

#[test]
fn generation_advance_rejects_late_prompt_callback() {
    let owner = UiOwner::new(SurfaceId(31));
    let kind = PermissionKind::Camera;
    let fake =
        Rc::new(FakePermissionProvider::new().with_status(kind, PermissionStatus::NotDetermined));
    let service = PermissionService::new(fake.clone());
    let mut request = service.request_permission(PermissionRequest::new(kind), ui_context(&owner));
    assert!(poll_ready(&mut request).is_pending());

    owner.advance_generation().unwrap();
    assert!(service.active_prompt().is_none());
    assert!(!fake.complete_next(PermissionStatus::Granted));
    assert_eq!(expect_error(&mut request).code, ErrorCode::ContextExpired);
    assert!(service.active_prompt().is_none());

    // The prompt lane is reusable under the new generation.
    fake.set_status(kind, PermissionStatus::NotDetermined);
    let mut fresh = service.request_permission(PermissionRequest::new(kind), ui_context(&owner));
    assert!(poll_ready(&mut fresh).is_pending());
    assert!(fake.deny_next(false));
    assert_eq!(
        outcome(&mut fresh).status,
        PermissionStatus::Denied {
            can_request_again: false,
        }
    );

    // Duplicate native callbacks are rejected without a second delivery.
    fake.set_status(kind, PermissionStatus::NotDetermined);
    let mut duplicate =
        service.request_permission(PermissionRequest::new(kind), ui_context(&owner));
    assert!(poll_ready(&mut duplicate).is_pending());
    let completion = fake.next_completion().unwrap();
    let snapshot = PermissionSnapshot::new(kind, PermissionStatus::Granted, 91);
    assert!(completion.complete(snapshot.clone()));
    assert!(!completion.complete(snapshot));
    assert_eq!(outcome(&mut duplicate).status, PermissionStatus::Granted);
}

#[test]
fn owner_disposal_rejects_late_prompt_callback() {
    let owner = UiOwner::new(SurfaceId(32));
    let kind = PermissionKind::Microphone;
    let fake =
        Rc::new(FakePermissionProvider::new().with_status(kind, PermissionStatus::NotDetermined));
    let service = PermissionService::new(fake.clone());
    let mut request = service.request_permission(PermissionRequest::new(kind), ui_context(&owner));
    assert!(poll_ready(&mut request).is_pending());

    owner.dispose();
    assert!(service.active_prompt().is_none());
    let completion = fake.next_completion().unwrap();
    assert!(!completion.complete(PermissionSnapshot::new(kind, PermissionStatus::Granted, 92)));
    assert_eq!(expect_error(&mut request).code, ErrorCode::ContextExpired);
    assert!(service.active_prompt().is_none());
}

#[test]
fn bounded_event_overflow_requires_reconciled_acknowledgment() {
    const KINDS: [PermissionKind; 17] = [
        PermissionKind::SpecialAccess("special.00"),
        PermissionKind::SpecialAccess("special.01"),
        PermissionKind::SpecialAccess("special.02"),
        PermissionKind::SpecialAccess("special.03"),
        PermissionKind::SpecialAccess("special.04"),
        PermissionKind::SpecialAccess("special.05"),
        PermissionKind::SpecialAccess("special.06"),
        PermissionKind::SpecialAccess("special.07"),
        PermissionKind::SpecialAccess("special.08"),
        PermissionKind::SpecialAccess("special.09"),
        PermissionKind::SpecialAccess("special.10"),
        PermissionKind::SpecialAccess("special.11"),
        PermissionKind::SpecialAccess("special.12"),
        PermissionKind::SpecialAccess("special.13"),
        PermissionKind::SpecialAccess("special.14"),
        PermissionKind::SpecialAccess("special.15"),
        PermissionKind::SpecialAccess("special.16"),
    ];
    let fake = Rc::new(FakePermissionProvider::new());
    let service = PermissionService::new(fake.clone());
    let changes = service.subscribe();
    assert!(!changes.snapshot_required());

    for kind in KINDS {
        let mut initial = service.query(kind, OperationContext::default());
        assert!(matches!(
            poll_ready(&mut initial),
            Poll::Ready(Ok(Completion::Completed(_)))
        ));
        fake.set_status(kind, PermissionStatus::Granted);
        let mut changed = service.query(kind, OperationContext::default());
        assert!(matches!(
            poll_ready(&mut changed),
            Poll::Ready(Ok(Completion::Completed(_)))
        ));
    }
    assert!(changes.snapshot_required());
    assert!(matches!(
        next_event(&changes).map(|event| event.payload),
        Some(Event::ResyncRequired)
    ));

    let reconciliation = service.reconcile_on_resume().unwrap();
    assert!(reconciliation.changes.is_empty());
    changes.acknowledge_snapshot(reconciliation.revision);
    assert!(!changes.snapshot_required());
}

#[test]
fn service_registration_and_explicit_override_are_typed() {
    assert_eq!(PROVIDER_MANIFEST.len(), 5);
    assert!(
        PROVIDER_MANIFEST
            .iter()
            .all(|manifest| !manifest.native_provider_installed)
    );
    assert!(
        PROVIDER_MANIFEST
            .iter()
            .all(|manifest| manifest.evidence == Evidence::ContractOnly)
    );

    let mut services = SystemServices::new();
    let kind = PermissionKind::Notifications;
    let first = PermissionService::new(Rc::new(
        FakePermissionProvider::new().with_status(kind, PermissionStatus::NotDetermined),
    ));
    services.register(first.clone()).unwrap();
    assert!(services.register(first).is_err());
    let registered = services.service::<PermissionService>().unwrap();
    let mut query = registered.query(kind, OperationContext::default());
    assert_eq!(snapshot(&mut query).status, PermissionStatus::NotDetermined);

    services.override_provider(PermissionService::new(Rc::new(
        FakePermissionProvider::new().with_status(kind, PermissionStatus::NotRequired),
    )));
    let replaced = services.service::<PermissionService>().unwrap();
    let mut query = replaced.query(kind, OperationContext::default());
    assert_eq!(snapshot(&mut query).status, PermissionStatus::NotRequired);
}

#[test]
fn concurrent_subscribers_receive_independent_deltas() {
    let kind = PermissionKind::Notifications;
    let fake =
        Rc::new(FakePermissionProvider::new().with_status(kind, PermissionStatus::NotDetermined));
    let service = PermissionService::new(fake.clone());
    let first = service.subscribe();
    let second = service.subscribe();

    let mut initial = service.query(kind, OperationContext::default());
    assert_eq!(
        snapshot(&mut initial).status,
        PermissionStatus::NotDetermined
    );

    fake.set_status(kind, PermissionStatus::Granted);
    let mut granted = service.query(kind, OperationContext::default());
    assert_eq!(snapshot(&mut granted).status, PermissionStatus::Granted);
    let first_change = next_change(&first);
    let second_change = next_change(&second);
    assert_eq!(first_change.current, PermissionStatus::Granted);
    assert_eq!(second_change.current, PermissionStatus::Granted);
    assert_eq!(first_change.revision, 1);
    assert_eq!(second_change.revision, 1);
    assert_eq!(service.revision(), 1);

    drop(first);
    fake.set_status(
        kind,
        PermissionStatus::Denied {
            can_request_again: false,
        },
    );
    let mut denied = service.query(kind, OperationContext::default());
    assert_eq!(
        snapshot(&mut denied).status,
        PermissionStatus::Denied {
            can_request_again: false,
        }
    );
    let second_change = next_change(&second);
    assert_eq!(second_change.previous, PermissionStatus::Granted);
    assert_eq!(
        second_change.current,
        PermissionStatus::Denied {
            can_request_again: false,
        }
    );
    assert_eq!(second_change.revision, 2);
    assert_eq!(service.revision(), 2);
}

#[test]
fn waker_can_reenter_service_without_refcell_conflict() {
    let kind = PermissionKind::Camera;
    let fake =
        Rc::new(FakePermissionProvider::new().with_status(kind, PermissionStatus::NotDetermined));
    let service = PermissionService::new(fake.clone());
    let subscription = service.subscribe();

    REENTRANT_SERVICE.with(|slot| *slot.borrow_mut() = Some(service.clone()));
    let woken = Arc::new(AtomicBool::new(false));
    let waker = Waker::from(Arc::new(ReentrantWake {
        woken: Arc::clone(&woken),
    }));
    let mut next = subscription.try_next().unwrap();
    let mut context = Context::from_waker(&waker);
    assert!(Pin::new(&mut next).poll(&mut context).is_pending());

    let mut initial = service.query(kind, OperationContext::default());
    assert_eq!(
        snapshot(&mut initial).status,
        PermissionStatus::NotDetermined
    );

    fake.set_status(kind, PermissionStatus::Granted);
    let mut query = service.query(kind, OperationContext::default());
    assert_eq!(snapshot(&mut query).status, PermissionStatus::Granted);
    assert!(woken.load(Ordering::SeqCst));
    assert!(matches!(
        Pin::new(&mut next).poll(&mut context),
        Poll::Ready(Some(_))
    ));

    // Exercise the same delivery path from `finish_prompt`, which also emits
    // changes while transitioning an accepted native prompt.
    drop(next);
    woken.store(false, Ordering::SeqCst);
    let mut next = subscription.try_next().unwrap();
    assert!(Pin::new(&mut next).poll(&mut context).is_pending());
    fake.set_status(kind, PermissionStatus::NotDetermined);
    let owner = UiOwner::new(SurfaceId(40));
    let mut request = service.request_permission(PermissionRequest::new(kind), ui_context(&owner));
    assert!(poll_ready(&mut request).is_pending());
    assert!(fake.complete_next(PermissionStatus::Granted));
    assert!(woken.load(Ordering::SeqCst));

    REENTRANT_SERVICE.with(|slot| *slot.borrow_mut() = None);
}
