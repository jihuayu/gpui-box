//! End-to-end notification workflow without a native platform.
//!
//! The sample stages an export through private application storage, posts and
//! replaces a notification backed by fake providers, withdraws a transient
//! progress notification, then reconstructs warm and cold-launch activations
//! from durable metadata. No OS notification or native callback is used.

use gpui_activation::{
    ActivationId, ActivationInbox, ActivationPayload, ActivationSource, ActivationTarget,
    EnqueueStatus, InboxConfig, RouteRegistry, UrlPolicy,
};
use gpui_app_data::{AppData, DirectoryRoots};
use gpui_notifications::{
    ActionId, ApplicationIdentity, Completion, FakeNotificationProvider, NotificationAction,
    NotificationActivationBinding, NotificationActivationCallback, NotificationActivationConfig,
    NotificationActivationStore, NotificationContent, NotificationId, NotificationProgress,
    NotificationService, NotificationSubmission, PermissionKind, PermissionService,
    PermissionStatus, ServiceResult, SystemError, WithdrawOutcome,
};
use gpui_permissions::FakePermissionProvider;
use gpui_system::{ErrorCode, OperationContext};
use std::{collections::BTreeMap, error::Error, rc::Rc, time::SystemTime};

fn value<T>(result: ServiceResult<T>) -> Result<T, SystemError> {
    match result? {
        Completion::Completed(value) => Ok(value),
        Completion::Cancelled(_) => Err(SystemError::new(
            ErrorCode::Unavailable,
            "Sample operation was cancelled",
        )),
    }
}

fn routes() -> RouteRegistry {
    let mut routes = RouteRegistry::default();
    routes
        .register("document", 1)
        .expect("static route is valid");
    routes
}

fn target(document: &str) -> ActivationTarget {
    ActivationTarget::new(
        "document",
        1,
        BTreeMap::from([("document".to_owned(), document.to_owned())]),
    )
    .expect("sample target is valid")
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
    .expect("sample action target is valid")
}

fn main() -> Result<(), Box<dyn Error>> {
    let root = std::env::temp_dir().join(format!(
        "gpui-box-notifications-example-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    let app = AppData::open(DirectoryRoots::under(&root))?;
    let context = OperationContext::default();

    let lease = value(app.temporary_export(b"staged invoice bytes", 4096, &context))?;

    let permission = Rc::new(
        FakePermissionProvider::new()
            .with_status(PermissionKind::Notifications, PermissionStatus::Granted),
    );
    let permissions = PermissionService::new(permission);
    let provider = Rc::new(
        FakeNotificationProvider::new()
            .with_identity(ApplicationIdentity::Ready)
            .with_auto_display(true),
    );
    let service = NotificationService::new(provider.clone(), permissions);

    let id = NotificationId::new("invoice-42")?;
    let transient = NotificationId::new("sync-progress")?;
    let action_id = ActionId::new("mark-read")?;
    let binding = NotificationActivationBinding::new(target("invoice-42"))
        .bind(action_id.clone(), action_target("invoice-42", "mark-read"))?;
    let store = value(NotificationActivationStore::open(
        &app,
        "sample",
        NotificationActivationConfig::default(),
        routes(),
        &context,
    ))?;
    let inbox = value(ActivationInbox::open(
        &app,
        "sample",
        InboxConfig::default(),
        UrlPolicy::new(["https"])?,
        routes(),
        &context,
    ))?;
    value(store.remember(&id, binding.clone(), SystemTime::now(), &context))?;

    let content = NotificationContent::new("Invoice ready", "Open invoice 42")?
        .with_actions(vec![NotificationAction::new(action_id, "Mark read")?])?
        .with_activation(binding);
    let posted = service
        .post(NotificationSubmission::new(id.clone(), content))?
        .into_result()?;
    let replaced = service
        .replace(NotificationSubmission::new(
            id.clone(),
            NotificationContent::new("Invoice updated", "Open invoice 42 again")?
                .with_activation(NotificationActivationBinding::new(target("invoice-42"))),
        ))?
        .into_result()?;

    let progress =
        NotificationContent::new("Syncing", "")?.with_progress(NotificationProgress::new(1, 3)?);
    service
        .post(NotificationSubmission::new(transient.clone(), progress))?
        .into_result()?;
    let withdrawn = service.withdraw(&transient)?;
    assert_eq!(withdrawn, WithdrawOutcome::Withdrawn);

    inbox.set_ready(true);
    let warm = NotificationActivationCallback::new(
        ActivationId::new("warm-tap")?,
        id.clone(),
        None,
        SystemTime::now(),
    );
    let warm_route =
        value(store.ingest_activation(warm.clone(), &inbox, SystemTime::now(), &context))?;
    assert!(matches!(
        &warm_route,
        gpui_notifications::ActivationRoute::Enqueued { receipt, .. }
            if receipt.status == EnqueueStatus::Queued
    ));
    let warm_pending = value(inbox.pending(SystemTime::now(), &context))?;
    if let Some(event) = warm_pending.first() {
        assert_eq!(event.source(), ActivationSource::Notification);
        value(inbox.acknowledge(event.id(), SystemTime::now(), &context))?;
    }
    assert!(
        value(store.ingest_activation(warm, &inbox, SystemTime::now(), &context))?.is_duplicate()
    );

    // A brand-new process opens the same durable roots. Only reconstruction
    // metadata survived; the notification content is gone.
    drop(store);
    drop(inbox);
    let store = value(NotificationActivationStore::open(
        &app,
        "sample",
        NotificationActivationConfig::default(),
        routes(),
        &context,
    ))?;
    let inbox = value(ActivationInbox::open(
        &app,
        "sample",
        InboxConfig::default(),
        UrlPolicy::new(["https"])?,
        routes(),
        &context,
    ))?;
    let cold = NotificationActivationCallback::new(
        ActivationId::new("cold-tap")?,
        id.clone(),
        None,
        SystemTime::now(),
    );
    let cold_route = value(store.ingest_activation(cold, &inbox, SystemTime::now(), &context))?;
    assert!(matches!(
        &cold_route,
        gpui_notifications::ActivationRoute::Enqueued { receipt, .. }
            if receipt.status == EnqueueStatus::Queued
    ));
    inbox.set_ready(true);
    let cold_pending = value(inbox.pending(SystemTime::now(), &context))?;
    let routed_document = cold_pending
        .first()
        .and_then(|event| match event.payload() {
            ActivationPayload::Notification { target, .. } => target.parameters().get("document"),
            _ => None,
        });
    for event in &cold_pending {
        value(inbox.acknowledge(event.id(), SystemTime::now(), &context))?;
    }

    println!(
        "notification {} posted as {:?}, replacement observed {:?}, transient notification {:?}, warm/cold document {:?}, staged export {} bytes",
        id.as_str(),
        posted.submission,
        replaced.replaced_existing,
        withdrawn,
        routed_document,
        std::fs::metadata(lease.path())?.len()
    );
    drop(store);
    drop(inbox);
    drop(service);
    drop(provider);
    let _ = std::fs::remove_dir_all(root);
    Ok(())
}
