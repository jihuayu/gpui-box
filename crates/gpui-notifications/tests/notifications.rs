use gpui_activation::{
    ActivationId, ActivationInbox, ActivationPayload, ActivationSource, ActivationTarget,
    EnqueueStatus, InboxConfig, RouteRegistry, UrlPolicy,
};
use gpui_app_data::{AppData, DataLocation, DataScope, DirectoryRoots};
use gpui_notifications::{
    ActionId, ActionPolicy, ActivationRoute, ApplicationIdentity, Completion, ErrorCode, Event,
    EventSubscription, FakeNotificationProvider, NotificationAction, NotificationActivationBinding,
    NotificationActivationCallback, NotificationActivationConfig, NotificationActivationStore,
    NotificationContent, NotificationEvent, NotificationId, NotificationOperation,
    NotificationProgress, NotificationService, NotificationSubmission, PermissionKind,
    PermissionService, PermissionStatus, PostRejectReason, ProgressPolicy, ReceiptPolicy,
    RecoveryAction, RecoveryGuidance, SettingsTarget, SubmissionState, SuppressionReason,
    UnavailableNotificationProvider, WithdrawOutcome,
};
use gpui_permissions::FakePermissionProvider;
use gpui_system::{OperationContext, ServiceResult, SystemServices};
use std::{
    cell::{Cell, RefCell},
    collections::BTreeMap,
    fs,
    future::Future,
    pin::Pin,
    rc::Rc,
    sync::Arc,
    task::{Context, Poll, Wake, Waker},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

fn value<T>(result: ServiceResult<T>) -> T {
    match result.unwrap() {
        Completion::Completed(value) => value,
        Completion::Cancelled(_) => panic!("operation was cancelled"),
    }
}

fn at(seconds: u64) -> SystemTime {
    UNIX_EPOCH + Duration::from_secs(seconds)
}

fn routes() -> RouteRegistry {
    let mut routes = RouteRegistry::default();
    routes.register("document", 1).unwrap();
    routes.register("settings", 1).unwrap();
    routes
}

fn permission_service(status: PermissionStatus) -> (Rc<FakePermissionProvider>, PermissionService) {
    let fake =
        Rc::new(FakePermissionProvider::new().with_status(PermissionKind::Notifications, status));
    let service = PermissionService::new(fake.clone());
    (fake, service)
}

fn service(
    provider: &Rc<FakeNotificationProvider>,
    status: PermissionStatus,
) -> NotificationService {
    let (_, permissions) = permission_service(status);
    NotificationService::new(provider.clone(), permissions)
}

fn target(document: &str) -> ActivationTarget {
    ActivationTarget::new(
        "document",
        1,
        BTreeMap::from([("document".to_owned(), document.to_owned())]),
    )
    .unwrap()
}

fn action_target(document: &str, action: &str) -> ActivationTarget {
    ActivationTarget::new(
        "document",
        1,
        BTreeMap::from([
            ("document".to_owned(), document.to_owned()),
            ("action".to_owned(), action.to_owned()),
        ]),
    )
    .unwrap()
}

fn poll_with<T: Future + Unpin>(future: &mut T, waker: &Waker) -> Poll<T::Output> {
    Pin::new(future).poll(&mut Context::from_waker(waker))
}

fn poll<T: Future + Unpin>(future: &mut T) -> Poll<T::Output> {
    poll_with(future, Waker::noop())
}

fn drain(subscription: &EventSubscription<NotificationEvent>) -> Vec<NotificationEvent> {
    let mut events = Vec::new();
    loop {
        let mut next = subscription.try_next().expect("single reader");
        match poll(&mut next) {
            Poll::Ready(Some(envelope)) => match envelope.payload {
                Event::Value(event) => events.push(event),
                Event::ResyncRequired => panic!("unexpected resync"),
            },
            Poll::Ready(None) | Poll::Pending => return events,
        }
    }
}

fn document_opened(target: &ActivationTarget) -> Option<String> {
    target.parameters().get("document").cloned()
}

#[test]
fn full_workflow_export_post_replace_withdraw_and_warm_cold_activation() {
    let root = tempfile::tempdir().unwrap();
    let app = AppData::open(DirectoryRoots::under(root.path().join("路径 with spaces"))).unwrap();
    let context = OperationContext::default();

    // #22 staging is a separate resource handoff; the notification carries only
    // the validated route that reconstructs the document.
    let lease = value(app.temporary_export(b"staged document bytes", 4096, &context));
    assert!(lease.path().exists());

    let provider = Rc::new(FakeNotificationProvider::new());
    let service = service(&provider, PermissionStatus::Granted);
    let events = service.subscribe();

    let id = NotificationId::new("invoice-42").unwrap();
    let transient = NotificationId::new("sync-progress").unwrap();
    let action_id = ActionId::new("mark-read").unwrap();
    let binding = NotificationActivationBinding::new(target("invoice-42"))
        .bind(action_id.clone(), action_target("invoice-42", "mark-read"))
        .unwrap();
    let content = NotificationContent::new("Invoice ready", "Open invoice 42")
        .unwrap()
        .with_actions(vec![
            NotificationAction::new(action_id.clone(), "Mark read").unwrap(),
        ])
        .unwrap()
        .with_activation(binding.clone());

    let store = value(NotificationActivationStore::open(
        &app,
        "workflow",
        NotificationActivationConfig {
            capacity: 8,
            retention: Duration::from_secs(3600),
        },
        routes(),
        &context,
    ));
    let inbox = value(ActivationInbox::open(
        &app,
        "workflow",
        InboxConfig::default(),
        UrlPolicy::new(["https"]).unwrap(),
        routes(),
        &context,
    ));
    value(store.remember(&id, binding.clone(), at(100), &context));

    let receipt = service
        .post(NotificationSubmission::new(id.clone(), content))
        .unwrap()
        .into_result()
        .unwrap();
    assert_eq!(receipt.notification_id, id);
    assert_eq!(receipt.submission, SubmissionState::PostedToCenter);
    assert_eq!(receipt.replaced_existing, Some(false));
    assert!(!receipt.omitted_actions);
    assert!(!receipt.omitted_progress);
    assert_eq!(
        provider.delivered_title(&id).as_deref(),
        Some("Invoice ready")
    );
    let submitted = drain(&events);
    assert_eq!(submitted.len(), 1);
    assert!(matches!(
        &submitted[0],
        NotificationEvent::Submitted {
            submission: SubmissionState::PostedToCenter,
            ..
        }
    ));

    let updated = NotificationContent::new("Invoice updated", "Open invoice 42 again")
        .unwrap()
        .with_actions(vec![
            NotificationAction::new(action_id.clone(), "Mark read").unwrap(),
        ])
        .unwrap()
        .with_activation(binding.clone());
    let receipt = service
        .replace(NotificationSubmission::new(id.clone(), updated))
        .unwrap()
        .into_result()
        .unwrap();
    assert_eq!(receipt.replaced_existing, Some(true));
    assert_eq!(
        provider.delivered_title(&id).as_deref(),
        Some("Invoice updated")
    );
    assert!(provider.replacement_ids().contains(&id));

    let progress = NotificationContent::new("Syncing", "")
        .unwrap()
        .with_progress(NotificationProgress::new(1, 4).unwrap());
    let receipt = service
        .post(NotificationSubmission::new(transient.clone(), progress))
        .unwrap()
        .into_result()
        .unwrap();
    assert!(!receipt.omitted_progress);
    assert_eq!(
        service.withdraw(&transient).unwrap(),
        WithdrawOutcome::Withdrawn
    );
    assert!(!provider.delivered_ids().contains(&transient));
    assert!(provider.withdrawal_ids().contains(&transient));

    // Warm activation uses the live store and ready inbox.
    inbox.set_ready(true);
    let warm = NotificationActivationCallback::new(
        ActivationId::new("tap-warm").unwrap(),
        id.clone(),
        None,
        at(110),
    );
    let route = value(store.ingest_activation(warm.clone(), &inbox, at(110), &context));
    assert!(matches!(
        &route,
        ActivationRoute::Enqueued { receipt, .. } if receipt.status == EnqueueStatus::Queued
    ));
    let pending = value(inbox.pending(at(111), &context));
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].source(), ActivationSource::Notification);
    let mut opens = BTreeMap::<String, usize>::new();
    match pending[0].payload() {
        ActivationPayload::Notification {
            notification_id,
            target,
        } => {
            assert_eq!(notification_id.as_str(), id.as_str());
            let document = document_opened(target).expect("document parameter");
            *opens.entry(document).or_default() += 1;
        }
        _ => panic!("unexpected activation payload"),
    }
    value(inbox.acknowledge(pending[0].id(), at(111), &context));

    let duplicate = value(store.ingest_activation(warm, &inbox, at(112), &context));
    assert!(duplicate.is_duplicate());
    assert!(value(inbox.pending(at(112), &context)).is_empty());

    let action = NotificationActivationCallback::new(
        ActivationId::new("tap-action").unwrap(),
        id.clone(),
        Some(action_id.clone()),
        at(113),
    );
    let route = value(store.ingest_activation(action, &inbox, at(113), &context));
    let ActivationRoute::Enqueued { target, .. } = route else {
        panic!("action activation did not route");
    };
    assert_eq!(
        target.parameters().get("action").map(String::as_str),
        Some("mark-read")
    );
    let pending = value(inbox.pending(at(114), &context));
    assert_eq!(pending.len(), 1);
    let document = match pending[0].payload() {
        ActivationPayload::Notification { target, .. } => {
            document_opened(target).expect("document parameter")
        }
        _ => panic!("unexpected activation payload"),
    };
    *opens.entry(document).or_default() += 1;
    value(inbox.acknowledge(pending[0].id(), at(114), &context));
    assert_eq!(opens.values().sum::<usize>(), 2);

    // Cold relaunch: no content survives, but reconstruction metadata does.
    drop(inbox);
    drop(store);
    drop(events);
    drop(service);
    drop(provider);
    let store = value(NotificationActivationStore::open(
        &app,
        "workflow",
        NotificationActivationConfig {
            capacity: 8,
            retention: Duration::from_secs(3600),
        },
        routes(),
        &context,
    ));
    let inbox = value(ActivationInbox::open(
        &app,
        "workflow",
        InboxConfig::default(),
        UrlPolicy::new(["https"]).unwrap(),
        routes(),
        &context,
    ));
    assert!(!inbox.is_ready());
    let remembered = value(store.lookup(&id, at(120), &context)).expect("binding survives");
    assert_eq!(remembered.default_target().route(), "document");
    let cold = NotificationActivationCallback::new(
        ActivationId::new("tap-cold").unwrap(),
        id.clone(),
        None,
        at(120),
    );
    assert!(matches!(
        value(store.ingest_activation(cold.clone(), &inbox, at(120), &context)),
        ActivationRoute::Enqueued { receipt, .. } if receipt.status == EnqueueStatus::Queued
    ));
    inbox.set_ready(true);
    let pending = value(inbox.pending(at(121), &context));
    assert_eq!(pending.len(), 1);
    let document = match pending[0].payload() {
        ActivationPayload::Notification { target, .. } => {
            document_opened(target).expect("document parameter")
        }
        _ => panic!("unexpected activation payload"),
    };
    *opens.entry(document).or_default() += 1;
    value(inbox.acknowledge(pending[0].id(), at(121), &context));
    assert!(value(store.ingest_activation(cold, &inbox, at(122), &context)).is_duplicate());
    assert_eq!(opens.values().sum::<usize>(), 3);

    // The durable file holds reconstruction targets, never title, body or labels.
    let raw = fs::read_to_string(
        app.location(DataLocation::Config)
            .path()
            .join("application/settings-notification-activation-workflow.json"),
    )
    .unwrap();
    assert!(!raw.contains("Invoice ready"));
    assert!(!raw.contains("Invoice updated"));
    assert!(!raw.contains("Open invoice"));
    assert!(!raw.contains("Mark read"));
    assert!(raw.contains("\"document\""));

    // The staged export is independent of the notification and remains pinned.
    assert!(lease.path().exists());
}

#[test]
fn actionable_rejections_distinguish_permission_channel_identity_and_unavailable() {
    let provider = Rc::new(FakeNotificationProvider::new());
    let (permissions, permissions_service) = permission_service(PermissionStatus::Denied {
        can_request_again: false,
    });
    let service = NotificationService::new(provider.clone(), permissions_service);
    let submission = NotificationSubmission::new(
        NotificationId::new("denied").unwrap(),
        NotificationContent::new("Title", "Body").unwrap(),
    );

    let outcome = service.post(submission.clone()).unwrap();
    match outcome.rejection().expect("permission rejection") {
        PostRejectReason::PermissionDenied {
            can_request_again,
            recovery,
        } => {
            assert!(!can_request_again);
            assert_eq!(*recovery, RecoveryGuidance::notification_settings());
            assert_eq!(recovery.action, RecoveryAction::OpenSettings);
            assert_eq!(
                recovery.settings,
                Some(SettingsTarget::NotificationSettings)
            );
        }
        other => panic!("unexpected rejection: {other:?}"),
    }
    let error = outcome.clone().into_result().unwrap_err();
    assert_eq!(error.code, ErrorCode::PermissionDenied);
    assert_eq!(error.recovery, Some(RecoveryAction::OpenSettings));

    permissions.set_status(
        PermissionKind::Notifications,
        PermissionStatus::Denied {
            can_request_again: true,
        },
    );
    let outcome = service.post(submission.clone()).unwrap();
    let reason = outcome.rejection().unwrap();
    assert_eq!(
        reason.retry_advice(),
        gpui_notifications::RetryAdvice::Retry
    );

    permissions.set_status(PermissionKind::Notifications, PermissionStatus::Granted);
    let channel = NotificationContent::new("Title", "Body")
        .unwrap()
        .with_channel_id("alerts")
        .unwrap();
    provider.set_channel_enabled("alerts", false);
    let outcome = service
        .post(NotificationSubmission::new(
            NotificationId::new("channel").unwrap(),
            channel,
        ))
        .unwrap();
    assert_eq!(
        outcome.rejection(),
        Some(&PostRejectReason::ChannelOrServerDisabled)
    );

    provider.set_channel_enabled("alerts", true);
    provider.set_server_enabled(false);
    let outcome = service.post(submission.clone()).unwrap();
    assert_eq!(
        outcome.rejection(),
        Some(&PostRejectReason::ChannelOrServerDisabled)
    );
    let error = outcome.into_result().unwrap_err();
    assert_eq!(error.code, ErrorCode::Unavailable);
    assert_eq!(error.recovery, Some(RecoveryAction::OpenSettings));

    provider.set_server_enabled(true);
    provider.set_application_identity(ApplicationIdentity::Missing);
    let outcome = service.post(submission.clone()).unwrap();
    assert_eq!(
        outcome.rejection(),
        Some(&PostRejectReason::MissingApplicationIdentity)
    );
    let error = outcome.into_result().unwrap_err();
    assert_eq!(error.code, ErrorCode::InvalidConfiguration);
    assert_eq!(error.recovery, Some(RecoveryAction::RepairConfiguration));

    provider.set_application_identity(ApplicationIdentity::Ready);
    provider.set_availability(
        NotificationOperation::PostImmediate,
        gpui_notifications::Availability::Unavailable,
    );
    let outcome = service.post(submission.clone()).unwrap();
    assert_eq!(
        outcome.rejection(),
        Some(&PostRejectReason::UnavailableProvider)
    );
    let error = outcome.into_result().unwrap_err();
    assert_eq!(error.code, ErrorCode::Unavailable);
    assert_eq!(error.recovery, Some(RecoveryAction::ProbeAgain));

    let (_, granted_permissions) = permission_service(PermissionStatus::Granted);
    let unavailable = NotificationService::new(
        Rc::new(UnavailableNotificationProvider),
        granted_permissions,
    );
    let outcome = unavailable.post(submission.clone()).unwrap();
    assert_eq!(
        outcome.rejection(),
        Some(&PostRejectReason::UnavailableProvider)
    );
    assert_eq!(
        outcome.into_result().unwrap_err().code,
        ErrorCode::Unavailable
    );

    let mut services = SystemServices::new();
    service.clone().install(&mut services).unwrap();
    let installed = services.service::<NotificationService>().unwrap();
    assert!(installed.is_ui_thread());
    assert!(matches!(
        services.register(service),
        Err(gpui_notifications::SystemError {
            code: ErrorCode::InvalidConfiguration,
            ..
        })
    ));
}

#[test]
fn receipt_display_and_activation_are_separate_and_capabilities_gate_features() {
    let provider = Rc::new(FakeNotificationProvider::new());
    provider.set_submission_state(SubmissionState::AcceptedByProvider);
    let service = service(&provider, PermissionStatus::Granted);
    let events = service.subscribe();

    let id = NotificationId::new("evidence").unwrap();
    let content = NotificationContent::new("Title", "Body").unwrap();
    let receipt = service
        .post(NotificationSubmission::new(id.clone(), content))
        .unwrap()
        .into_result()
        .unwrap();
    assert_eq!(receipt.submission, SubmissionState::AcceptedByProvider);
    assert_eq!(provider.observation_count(), 0);
    let observed = drain(&events);
    assert_eq!(observed.len(), 1);
    assert!(matches!(observed[0], NotificationEvent::Submitted { .. }));

    assert!(provider.mark_displayed(&id));
    assert_eq!(service.pump_observations().unwrap(), 1);
    let observed = drain(&events);
    assert_eq!(observed.len(), 1);
    assert!(matches!(
        observed[0],
        NotificationEvent::ObservedDisplayed { .. }
    ));

    service.record_activated(&id, None).unwrap();
    let activated = drain(&events);
    assert_eq!(activated.len(), 1);
    assert!(matches!(
        activated[0],
        NotificationEvent::Activated {
            action_id: None,
            ..
        }
    ));

    let action = NotificationAction::new(ActionId::new("open").unwrap(), "Open").unwrap();
    let action_content = NotificationContent::new("Actions", "Body")
        .unwrap()
        .with_actions(vec![action.clone()])
        .unwrap();
    provider.set_support(
        NotificationOperation::ActionButtons,
        gpui_notifications::Support::Unsupported,
    );
    let omitted = service
        .post(
            NotificationSubmission::new(
                NotificationId::new("actions-omit").unwrap(),
                action_content.clone(),
            )
            .with_action_policy(ActionPolicy::OmitIfUnsupported),
        )
        .unwrap()
        .into_result()
        .unwrap();
    assert!(omitted.omitted_actions);
    assert_eq!(
        provider.delivered_actions(&NotificationId::new("actions-omit").unwrap()),
        Some(Vec::new())
    );
    let rejected = service
        .post(
            NotificationSubmission::new(
                NotificationId::new("actions-require").unwrap(),
                action_content,
            )
            .with_action_policy(ActionPolicy::RequireSupported),
        )
        .unwrap();
    assert_eq!(
        rejected.rejection(),
        Some(&PostRejectReason::Unsupported(
            NotificationOperation::ActionButtons
        ))
    );

    let progress = NotificationContent::new("Progress", "Body")
        .unwrap()
        .with_progress(NotificationProgress::new(1, 2).unwrap());
    provider.set_support(
        NotificationOperation::ProgressUpdates,
        gpui_notifications::Support::Unsupported,
    );
    let omitted = service
        .post(
            NotificationSubmission::new(
                NotificationId::new("progress-omit").unwrap(),
                progress.clone(),
            )
            .with_progress_policy(ProgressPolicy::OmitIfUnsupported),
        )
        .unwrap()
        .into_result()
        .unwrap();
    assert!(omitted.omitted_progress);
    assert_eq!(
        provider.delivered_progress(&NotificationId::new("progress-omit").unwrap()),
        Some(None)
    );
    let rejected = service
        .post(
            NotificationSubmission::new(NotificationId::new("progress-require").unwrap(), progress)
                .with_progress_policy(ProgressPolicy::RequireSupported),
        )
        .unwrap();
    assert_eq!(
        rejected.rejection(),
        Some(&PostRejectReason::Unsupported(
            NotificationOperation::ProgressUpdates
        ))
    );

    provider.set_support(
        NotificationOperation::ReplaceById,
        gpui_notifications::Support::Unsupported,
    );
    let replace = service
        .replace(NotificationSubmission::new(
            id.clone(),
            NotificationContent::new("Replacement", "Body").unwrap(),
        ))
        .unwrap();
    assert_eq!(
        replace.rejection(),
        Some(&PostRejectReason::Unsupported(
            NotificationOperation::ReplaceById
        ))
    );

    provider.set_support(
        NotificationOperation::WithdrawById,
        gpui_notifications::Support::Unsupported,
    );
    assert_eq!(service.withdraw(&id).unwrap(), WithdrawOutcome::Unsupported);

    provider.set_support(
        NotificationOperation::DeliveryReceipt,
        gpui_notifications::Support::Unsupported,
    );
    let downgraded = service
        .post(
            NotificationSubmission::new(
                NotificationId::new("receipt-omit").unwrap(),
                NotificationContent::new("Receipt", "Body").unwrap(),
            )
            .with_receipt_policy(ReceiptPolicy::OmitIfUnsupported),
        )
        .unwrap()
        .into_result()
        .unwrap();
    assert_eq!(downgraded.provider_id, None);
    assert_eq!(downgraded.submission, SubmissionState::AcceptedByProvider);
    let rejected = service
        .post(
            NotificationSubmission::new(
                NotificationId::new("receipt-require").unwrap(),
                NotificationContent::new("Receipt", "Body").unwrap(),
            )
            .with_receipt_policy(ReceiptPolicy::RequireSupported),
        )
        .unwrap();
    assert_eq!(
        rejected.rejection(),
        Some(&PostRejectReason::Unsupported(
            NotificationOperation::DeliveryReceipt
        ))
    );

    let suppressed = drain(&events);
    assert!(suppressed.iter().any(|event| matches!(
        event,
        NotificationEvent::Suppressed {
            reason: SuppressionReason::Unsupported,
            ..
        }
    )));
}

#[test]
fn durable_store_survives_recreation_deduplicates_replays_and_expires() {
    let root = tempfile::tempdir().unwrap();
    let app = AppData::open(DirectoryRoots::under(root.path())).unwrap();
    let context = OperationContext::default();
    let id = NotificationId::new("durable").unwrap();
    let secret_target = ActivationTarget::new(
        "document",
        1,
        BTreeMap::from([("document".to_owned(), "route-secret-value".to_owned())]),
    )
    .unwrap();
    let binding = NotificationActivationBinding::new(secret_target);
    let config = NotificationActivationConfig {
        capacity: 2,
        retention: Duration::from_secs(60),
    };
    let store = value(NotificationActivationStore::open(
        &app,
        "durable",
        config.clone(),
        routes(),
        &context,
    ));
    value(store.remember(&id, binding.clone(), at(100), &context));
    assert!(matches!(
        NotificationActivationStore::open(&app, "durable", config.clone(), routes(), &context),
        Err(gpui_notifications::SystemError {
            code: ErrorCode::Busy,
            ..
        })
    ));
    drop(store);

    // Host recreation keeps the target but never any content.
    let store = value(NotificationActivationStore::open(
        &app,
        "durable",
        config.clone(),
        routes(),
        &context,
    ));
    assert!(value(store.lookup(&id, at(101), &context)).is_some());
    let inbox = value(ActivationInbox::open(
        &app,
        "durable",
        InboxConfig::default(),
        UrlPolicy::new(["https"]).unwrap(),
        routes(),
        &context,
    ));
    inbox.set_ready(true);
    let callback = NotificationActivationCallback::new(
        ActivationId::new("replayed").unwrap(),
        id.clone(),
        None,
        at(101),
    );
    assert_eq!(callback.source(), ActivationSource::Notification);
    let route = value(store.ingest_activation(callback.clone(), &inbox, at(101), &context));
    assert!(matches!(
        &route,
        ActivationRoute::Enqueued { receipt, .. } if receipt.status == EnqueueStatus::Queued
    ));
    assert!(!format!("{route:?}").contains("route-secret-value"));
    let pending = value(inbox.pending(at(101), &context));
    assert_eq!(pending.len(), 1);
    value(inbox.acknowledge(pending[0].id(), at(101), &context));
    assert!(
        value(store.ingest_activation(callback.clone(), &inbox, at(102), &context)).is_duplicate()
    );
    drop(store);
    drop(inbox);

    let store = value(NotificationActivationStore::open(
        &app,
        "durable",
        config.clone(),
        routes(),
        &context,
    ));
    let inbox = value(ActivationInbox::open(
        &app,
        "durable",
        InboxConfig::default(),
        UrlPolicy::new(["https"]).unwrap(),
        routes(),
        &context,
    ));
    assert!(value(store.ingest_activation(callback, &inbox, at(103), &context)).is_duplicate());
    assert!(value(inbox.pending(at(103), &context)).is_empty());
    drop(store);
    drop(inbox);

    let first = NotificationId::new("first").unwrap();
    let second = NotificationId::new("second").unwrap();
    let third = NotificationId::new("third").unwrap();
    let store = value(NotificationActivationStore::open(
        &app,
        "expiry",
        config.clone(),
        routes(),
        &context,
    ));
    value(store.remember(&first, binding.clone(), at(100), &context));
    value(store.remember(&second, binding.clone(), at(101), &context));
    value(store.remember(&third, binding.clone(), at(102), &context));
    assert!(value(store.lookup(&first, at(103), &context)).is_none());
    assert!(value(store.lookup(&second, at(103), &context)).is_some());
    assert!(value(store.lookup(&third, at(103), &context)).is_some());
    assert_eq!(store.len(), 2);
    assert_eq!(value(store.expire(at(1000), &context)), 2);
    assert!(value(store.lookup(&second, at(1000), &context)).is_none());
    assert!(store.is_empty());

    // Redaction covers content, bindings, events and typed errors.
    let secret_content = NotificationContent::new("SECRET TITLE", "SECRET BODY").unwrap();
    assert!(!format!("{secret_content:?}").contains("SECRET"));
    assert!(!format!("{binding:?}").contains("route-secret-value"));
    assert!(!format!("{store:?}").contains("route-secret-value"));
    assert!(!format!("{route:?}").contains("route-secret-value"));
    let error = PostRejectReason::PermissionDenied {
        can_request_again: false,
        recovery: RecoveryGuidance::notification_settings(),
    }
    .into_error();
    assert!(!error.to_string().contains("route-secret-value"));
    assert!(!error.to_string().contains("SECRET"));
}

#[test]
fn multi_subscriber_events_survive_synchronous_waker_reentrancy() {
    thread_local! {
        static SERVICE: RefCell<Option<NotificationService>> = const { RefCell::new(None) };
        static IN_WAKE: Cell<bool> = const { Cell::new(false) };
        static REENTERED: Cell<bool> = const { Cell::new(false) };
        static THIRD: RefCell<Option<EventSubscription<NotificationEvent>>> =
            const { RefCell::new(None) };
    }

    struct ReentrantWake;

    impl Wake for ReentrantWake {
        fn wake(self: Arc<Self>) {
            if IN_WAKE.with(|flag| flag.replace(true)) {
                return;
            }
            let service = SERVICE.with(|slot| slot.borrow().clone());
            if let Some(service) = service {
                REENTERED.with(|flag| flag.set(true));
                let _ = service.subscriber_count();
                let nested = NotificationSubmission::new(
                    NotificationId::new("nested").unwrap(),
                    NotificationContent::new("Nested", "Posted from a waker").unwrap(),
                );
                let _ = service.post(nested);
                let _ = service.withdraw(&NotificationId::new("first").unwrap());
                THIRD.with(|slot| drop(slot.borrow_mut().take()));
            }
            IN_WAKE.with(|flag| flag.set(false));
        }
    }

    let provider = Rc::new(FakeNotificationProvider::new());
    let service = service(&provider, PermissionStatus::Granted);
    let first = service.subscribe();
    let second = service.subscribe();
    let third = service.subscribe();
    THIRD.with(|slot| *slot.borrow_mut() = Some(third));
    assert_eq!(service.subscriber_count(), 3);
    SERVICE.with(|slot| *slot.borrow_mut() = Some(service.clone()));

    let waker = Waker::from(Arc::new(ReentrantWake));
    assert!(poll_with(&mut first.try_next().unwrap(), &waker).is_pending());

    let original = service
        .post(NotificationSubmission::new(
            NotificationId::new("first").unwrap(),
            NotificationContent::new("First", "Initial").unwrap(),
        ))
        .unwrap();
    assert!(original.is_posted());
    assert!(REENTERED.with(Cell::get));
    assert_eq!(service.subscriber_count(), 2);

    let first_events = drain(&first);
    assert_eq!(first_events.len(), 3);
    assert!(first_events.iter().any(|event| matches!(
        event,
        NotificationEvent::Submitted { notification_id, .. }
            if notification_id.as_str() == "first"
    )));
    assert!(first_events.iter().any(|event| matches!(
        event,
        NotificationEvent::Submitted { notification_id, .. }
            if notification_id.as_str() == "nested"
    )));
    assert!(first_events.iter().any(|event| matches!(
        event,
        NotificationEvent::Withdrawn { notification_id }
            if notification_id.as_str() == "first"
    )));
    let second_events = drain(&second);
    assert_eq!(second_events.len(), 3);
    assert!(second_events.iter().any(|event| matches!(
        event,
        NotificationEvent::Submitted { notification_id, .. }
            if notification_id.as_str() == "nested"
    )));

    SERVICE.with(|slot| drop(slot.borrow_mut().take()));
}

#[test]
fn corrupt_metadata_is_preserved_until_explicit_quarantine() {
    let root = tempfile::tempdir().unwrap();
    let app = AppData::open(DirectoryRoots::under(root.path())).unwrap();
    let context = OperationContext::default();
    let path = app
        .location(DataLocation::Config)
        .path()
        .join("application/settings-notification-activation-corrupt.json");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, b"{broken").unwrap();
    assert!(matches!(
        NotificationActivationStore::open(
            &app,
            "corrupt",
            NotificationActivationConfig::default(),
            routes(),
            &context
        ),
        Err(gpui_notifications::SystemError {
            code: ErrorCode::IntegrityFailure,
            ..
        })
    ));
    assert_eq!(fs::read(&path).unwrap(), b"{broken");
    value(
        app.settings(
            DataScope::Application,
            "notification-activation-corrupt",
            1,
            2 * 1024 * 1024,
        )
        .unwrap()
        .quarantine(&context),
    );
    value(NotificationActivationStore::open(
        &app,
        "corrupt",
        NotificationActivationConfig::default(),
        routes(),
        &context,
    ));
}

#[test]
fn omitted_actions_keep_a_consistent_activation_binding() {
    let provider = Rc::new(FakeNotificationProvider::new().with_support(
        NotificationOperation::ActionButtons,
        gpui_notifications::Support::Unsupported,
    ));
    let service = service(&provider, PermissionStatus::Granted);
    let notification_id = NotificationId::new("omit-actions-with-binding").unwrap();
    let action_id = ActionId::new("mark-read").unwrap();
    let binding = NotificationActivationBinding::new(target("invoice-7"))
        .bind(action_id.clone(), action_target("invoice-7", "mark-read"))
        .unwrap();
    let content = NotificationContent::new("Invoice", "Open invoice 7")
        .unwrap()
        .with_actions(vec![
            NotificationAction::new(action_id, "Mark read").unwrap(),
        ])
        .unwrap()
        .with_activation(binding);

    let receipt = service
        .post(
            NotificationSubmission::new(notification_id, content)
                .with_action_policy(ActionPolicy::OmitIfUnsupported),
        )
        .unwrap()
        .into_result()
        .unwrap();
    assert!(receipt.omitted_actions);

    let observed = provider
        .last_submission()
        .expect("provider observed the normalized submission");
    observed.validate().unwrap();
    assert!(observed.content.actions().is_empty());
    let activation = observed
        .content
        .activation()
        .expect("body-tap target survives action omission");
    assert!(activation.action_targets().is_empty());
    assert_eq!(activation.default_target().route(), "document");
    assert_eq!(
        activation
            .default_target()
            .parameters()
            .get("document")
            .map(String::as_str),
        Some("invoice-7")
    );
}

#[test]
fn stale_action_after_replacement_is_unbound_and_body_tap_still_routes() {
    let root = tempfile::tempdir().unwrap();
    let app = AppData::open(DirectoryRoots::under(root.path())).unwrap();
    let context = OperationContext::default();
    let notification_id = NotificationId::new("replaced-notification").unwrap();
    let old_action = ActionId::new("old-action").unwrap();
    let store = value(NotificationActivationStore::open(
        &app,
        "stale-action",
        NotificationActivationConfig {
            capacity: 4,
            retention: Duration::from_secs(3600),
        },
        routes(),
        &context,
    ));
    let inbox = value(ActivationInbox::open(
        &app,
        "stale-action",
        InboxConfig::default(),
        UrlPolicy::new(["https"]).unwrap(),
        routes(),
        &context,
    ));
    let before = NotificationActivationBinding::new(target("before-replacement"))
        .bind(
            old_action.clone(),
            action_target("before-replacement", "old-secret-action"),
        )
        .unwrap();
    value(store.remember(&notification_id, before, at(100), &context));
    // Replacement drops the old action target while preserving a new default.
    let after = NotificationActivationBinding::new(target("after-replacement"));
    value(store.remember(&notification_id, after, at(101), &context));
    inbox.set_ready(true);

    let late = NotificationActivationCallback::new(
        ActivationId::new("late-old-action").unwrap(),
        notification_id.clone(),
        Some(old_action.clone()),
        at(102),
    );
    let route = value(store.ingest_activation(late, &inbox, at(102), &context));
    assert!(matches!(
        &route,
        ActivationRoute::UnboundAction {
            notification_id: routed_id,
            action_id,
        } if routed_id == &notification_id && action_id == &old_action
    ));
    assert!(value(inbox.pending(at(102), &context)).is_empty());
    let debug = format!("{route:?}");
    assert!(!debug.contains("before-replacement"));
    assert!(!debug.contains("after-replacement"));
    assert!(!debug.contains("old-secret-action"));

    let body_tap = NotificationActivationCallback::new(
        ActivationId::new("updated-body-tap").unwrap(),
        notification_id.clone(),
        None,
        at(103),
    );
    let route = value(store.ingest_activation(body_tap, &inbox, at(103), &context));
    let ActivationRoute::Enqueued {
        target, receipt, ..
    } = route
    else {
        panic!("updated body tap did not route");
    };
    assert_eq!(receipt.status, EnqueueStatus::Queued);
    assert_eq!(
        target.parameters().get("document").map(String::as_str),
        Some("after-replacement")
    );
    let pending = value(inbox.pending(at(103), &context));
    assert_eq!(pending.len(), 1);
    value(inbox.acknowledge(pending[0].id(), at(103), &context));
}

#[test]
fn fake_provider_debug_reports_counts_without_content() {
    let provider = Rc::new(FakeNotificationProvider::new());
    let service = service(&provider, PermissionStatus::Granted);
    let notification_id = NotificationId::new("debug-redaction").unwrap();
    let binding = NotificationActivationBinding::new(target("secret-route-value"));
    let content = NotificationContent::new("confidential-title", "confidential-body")
        .unwrap()
        .with_activation(binding);
    service
        .post(NotificationSubmission::new(notification_id, content))
        .unwrap()
        .into_result()
        .unwrap();

    let debug = format!("{provider:?}");
    assert!(debug.contains("FakeNotificationProvider"));
    assert!(debug.contains("delivered: 1"));
    assert!(!debug.contains("confidential-title"));
    assert!(!debug.contains("confidential-body"));
    assert!(!debug.contains("secret-route-value"));
}

#[test]
fn unbound_action_route_lookup_is_a_non_error_miss() {
    let root = tempfile::tempdir().unwrap();
    let app = AppData::open(DirectoryRoots::under(root.path())).unwrap();
    let context = OperationContext::default();
    let notification_id = NotificationId::new("direct-unbound-route").unwrap();
    let action_id = ActionId::new("removed-action").unwrap();
    let store = value(NotificationActivationStore::open(
        &app,
        "direct-unbound",
        NotificationActivationConfig::default(),
        routes(),
        &context,
    ));
    let binding = NotificationActivationBinding::new(target("document"));
    value(store.remember(&notification_id, binding, at(100), &context));
    let callback = NotificationActivationCallback::new(
        ActivationId::new("direct-unbound-callback").unwrap(),
        notification_id,
        Some(action_id),
        at(101),
    );
    assert!(value(store.route_activation(&callback, at(101), &context)).is_none());
}
