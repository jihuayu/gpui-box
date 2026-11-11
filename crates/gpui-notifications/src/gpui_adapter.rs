//! Optional GPUI integration for [`NotificationService`].
//!
//! All functions run on the owning GPUI UI thread. The adapter never installs a
//! native provider implicitly. Durable metadata lookup and inbox enqueueing are
//! moved to GPUI's background executor; the response callback itself only parses
//! the platform response and publishes the local activation observation.

use crate::{
    ActionId, ActionPolicy, NotificationActivationCallback, NotificationActivationStore,
    NotificationContent, NotificationEvent, NotificationId, NotificationOperation,
    NotificationProvider, NotificationService, NotificationSubmission, PostOutcome, PostReceipt,
    PostRejectReason, ProgressPolicy, ReceiptPolicy, SubmissionState, Support, WithdrawOutcome,
};
use gpui::{
    App, Context, SystemNotification, SystemNotificationAction, SystemNotificationResponse, Task,
};
use gpui_activation::{ActivationId, ActivationInbox};
use gpui_permissions::PermissionService;
use gpui_system::{
    EventSubscription, OperationContext, ServiceResult, SystemError, SystemServices, UiOwner,
    gpui_adapter,
};
use std::{
    rc::Rc,
    sync::{Arc, Mutex, PoisonError},
    time::SystemTime,
};

const MAX_ROUTE_FAILURES: usize = 16;

/// Convert validated notification content into GPUI's platform notification.
///
/// The notification identity becomes GPUI's stable tag, so a later
/// `show_system_notification` with the same tag replaces the earlier notification
/// where the platform supports replacement.
pub fn to_gpui_system_notification(
    notification_id: &NotificationId,
    content: &NotificationContent,
) -> SystemNotification {
    SystemNotification {
        tag: notification_id.as_str().into(),
        title: content.title().into(),
        body: content.body().into(),
        actions: content
            .actions()
            .iter()
            .map(to_gpui_system_notification_action)
            .collect(),
    }
}

/// Convert one validated action into GPUI's action-button type.
pub fn to_gpui_system_notification_action(
    action: &crate::NotificationAction,
) -> SystemNotificationAction {
    SystemNotificationAction {
        id: action.id().as_str().into(),
        label: action.label().into(),
    }
}

/// Convert a GPUI response into a durable activation callback.
///
/// GPUI's response carries only the tag and optional action ID; it does not carry
/// a stable native event identity. The host must supply the same [`ActivationId`]
/// for a redelivered callback so the activation inbox can deduplicate it.
pub fn from_gpui_notification_response(
    activation_event_id: ActivationId,
    response: SystemNotificationResponse,
) -> Result<NotificationActivationCallback, SystemError> {
    from_gpui_notification_response_at(activation_event_id, response, SystemTime::now())
}

/// Deterministic variant of [`from_gpui_notification_response`] for hosts that
/// already captured the native callback timestamp.
pub fn from_gpui_notification_response_at(
    activation_event_id: ActivationId,
    response: SystemNotificationResponse,
    received_at: SystemTime,
) -> Result<NotificationActivationCallback, SystemError> {
    let notification_id = NotificationId::new(response.tag.as_ref())?;
    let action_id = response
        .action_id
        .as_deref()
        .map(ActionId::new)
        .transpose()?;
    Ok(NotificationActivationCallback::new(
        activation_event_id,
        notification_id,
        action_id,
        received_at,
    ))
}

/// Register one explicit notification service. Call before starting operations.
pub fn install_notification_service(
    services: &mut SystemServices,
    provider: Rc<dyn NotificationProvider>,
    permissions: PermissionService,
) -> Result<(), SystemError> {
    services.register(NotificationService::new(provider, permissions))
}

/// Access the installed service from GPUI's typed `SystemServices` global.
pub fn notification_service(cx: &App) -> Result<Rc<NotificationService>, SystemError> {
    cx.global::<SystemServices>()
        .service::<NotificationService>()
}

/// Direct bridge over `App::show_system_notification`,
/// `App::dismiss_system_notification` and `App::on_system_notification_response`.
///
/// This bypasses [`NotificationProvider::post`] because GPUI's void desktop API
/// exposes no pre-submission capability query. The bridge still publishes
/// receipts and events through the shared service and routes responses through
/// the durable store and activation inbox on the background executor.
#[derive(Clone)]
pub struct GpuiDesktopNotificationBridge {
    service: NotificationService,
    store: NotificationActivationStore,
    inbox: ActivationInbox,
    route_failures: Arc<Mutex<Vec<SystemError>>>,
}

impl GpuiDesktopNotificationBridge {
    pub fn new(
        service: NotificationService,
        store: NotificationActivationStore,
        inbox: ActivationInbox,
    ) -> Self {
        Self {
            service,
            store,
            inbox,
            route_failures: Arc::new(Mutex::new(Vec::new())),
        }
    }

    pub fn service(&self) -> &NotificationService {
        &self.service
    }

    pub fn store(&self) -> &NotificationActivationStore {
        &self.store
    }

    pub fn inbox(&self) -> &ActivationInbox {
        &self.inbox
    }

    /// Drain callback failures retained by the background router.
    pub fn drain_route_failures(&self) -> Vec<SystemError> {
        let mut failures = self
            .route_failures
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        std::mem::take(&mut *failures)
    }

    pub fn route_failure_count(&self) -> usize {
        self.route_failures
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .len()
    }

    /// Post a notification through GPUI's void platform call.
    ///
    /// GPUI returns no acceptance or display evidence, so the receipt is always
    /// `AcceptedByProvider` with no provider identity and an unknown replacement
    /// result. Progress is not representable by `SystemNotification` and is
    /// omitted or rejected according to the submission policy.
    pub fn post(
        &self,
        cx: &App,
        submission: NotificationSubmission,
    ) -> Result<PostOutcome, SystemError> {
        self.submit(cx, submission, false)
    }

    /// Replace the notification with the same stable tag through GPUI.
    ///
    /// This mirrors [`Self::post`] but reflects the intent to update an existing
    /// notification. An explicit provider `Support::Unsupported` declaration for
    /// `ReplaceById` is rejected; GPUI itself exposes no capability query, so
    /// unavailable or unknown provider evidence does not block the tag attempt.
    /// The void API also exposes no replacement observation, so the receipt reports
    /// `replaced_existing: None` and never asserts that a previous banner existed.
    pub fn replace(
        &self,
        cx: &App,
        submission: NotificationSubmission,
    ) -> Result<PostOutcome, SystemError> {
        self.submit(cx, submission, true)
    }

    fn submit(
        &self,
        cx: &App,
        submission: NotificationSubmission,
        replace: bool,
    ) -> Result<PostOutcome, SystemError> {
        self.service.require_ui_thread()?;
        submission.validate()?;
        if let Some(reason) = self.permission_rejection()? {
            let _ = self
                .service
                .record_suppressed(&submission.notification_id, &reason);
            return Ok(PostOutcome::Rejected(reason));
        }
        if replace
            && matches!(
                self.service
                    .capability(NotificationOperation::ReplaceById)
                    .support,
                Support::Unsupported
            )
        {
            let reason = PostRejectReason::Unsupported(NotificationOperation::ReplaceById);
            let _ = self
                .service
                .record_suppressed(&submission.notification_id, &reason);
            return Ok(PostOutcome::Rejected(reason));
        }
        if !submission.content.actions().is_empty()
            && submission.action_policy == ActionPolicy::RequireSupported
        {
            let reason = PostRejectReason::Unsupported(NotificationOperation::ActionButtons);
            let _ = self
                .service
                .record_suppressed(&submission.notification_id, &reason);
            return Ok(PostOutcome::Rejected(reason));
        }
        let mut content = submission.content.clone();
        let mut omitted_progress = false;
        if content.progress().is_some() {
            if submission.progress_policy == ProgressPolicy::RequireSupported {
                let reason = PostRejectReason::Unsupported(NotificationOperation::ProgressUpdates);
                let _ = self
                    .service
                    .record_suppressed(&submission.notification_id, &reason);
                return Ok(PostOutcome::Rejected(reason));
            }
            content = content.without_progress();
            omitted_progress = true;
        }
        if submission.receipt_policy == ReceiptPolicy::RequireSupported {
            let reason = PostRejectReason::Unsupported(NotificationOperation::DeliveryReceipt);
            let _ = self
                .service
                .record_suppressed(&submission.notification_id, &reason);
            return Ok(PostOutcome::Rejected(reason));
        }
        // GPUI uses the stable tag for both post and replacement. The void call
        // gives no capability or replacement evidence; `replace` is an explicit
        // intent and the receipt remains deliberately unobservable.
        cx.show_system_notification(to_gpui_system_notification(
            &submission.notification_id,
            &content,
        ));
        let receipt = PostReceipt {
            notification_id: submission.notification_id,
            provider_id: None,
            submission: SubmissionState::AcceptedByProvider,
            replaced_existing: None,
            omitted_actions: false,
            omitted_progress,
        };
        self.service.record_submitted(&receipt)?;
        Ok(PostOutcome::Posted(receipt))
    }

    /// Dismiss through GPUI's best-effort platform call. GPUI cannot report
    /// whether a delivered notification was actually retracted.
    pub fn withdraw(
        &self,
        cx: &App,
        notification_id: &NotificationId,
    ) -> Result<WithdrawOutcome, SystemError> {
        self.service.require_ui_thread()?;
        NotificationId::new(notification_id.as_str())?;
        cx.dismiss_system_notification(notification_id.as_str());
        self.service.record_withdrawn(notification_id)?;
        Ok(WithdrawOutcome::Withdrawn)
    }

    /// Install the process-local GPUI response handler.
    ///
    /// `event_id` must return a stable identity for one native callback replay.
    /// The handler publishes `Activated` immediately and sends the durable lookup
    /// plus inbox enqueue to the background executor. A cold launch must forward
    /// its native launch payload through the same store and inbox; GPUI's response
    /// callback only exists for the current process.
    pub fn install_response_handler<F>(&self, cx: &App, mut event_id: F)
    where
        F: FnMut(&SystemNotificationResponse) -> ActivationId + 'static,
    {
        let service = self.service.clone();
        let store = self.store.clone();
        let inbox = self.inbox.clone();
        let failures = Arc::clone(&self.route_failures);
        cx.on_system_notification_response(move |response, cx| {
            let received_at = SystemTime::now();
            let Ok(callback) =
                from_gpui_notification_response_at(event_id(&response), response, received_at)
            else {
                return;
            };
            let _ =
                service.record_activated(callback.notification_id(), callback.action_id().cloned());
            let store = store.clone();
            let inbox = inbox.clone();
            let failures = Arc::clone(&failures);
            cx.background_executor()
                .spawn(async move {
                    let context = OperationContext::default();
                    if let Err(error) =
                        store.ingest_activation(callback, &inbox, received_at, &context)
                    {
                        let mut failures = failures.lock().unwrap_or_else(PoisonError::into_inner);
                        if failures.len() == MAX_ROUTE_FAILURES {
                            failures.remove(0);
                        }
                        failures.push(error);
                    }
                })
                .detach();
        });
    }

    fn permission_rejection(&self) -> Result<Option<PostRejectReason>, SystemError> {
        let snapshot = self.service.authorization_status()?;
        if snapshot.status.allows_use() {
            return Ok(None);
        }
        Ok(Some(crate::service::permission_status_rejection(
            snapshot.status,
        )))
    }
}

/// Entity-owned scope which retains a notification observation stream and
/// invalidates the UI owner on entity release.
pub struct NotificationEntityScope {
    subscriptions: Vec<EventSubscription<NotificationEvent>>,
    scope: gpui_system::gpui_adapter::EntityScope,
}

impl NotificationEntityScope {
    pub fn new<T: 'static>(owner: UiOwner, cx: &Context<T>) -> Self {
        Self {
            subscriptions: Vec::new(),
            scope: gpui_adapter::EntityScope::new(owner, cx),
        }
    }

    pub fn owner(&self) -> &UiOwner {
        self.scope.owner()
    }

    pub fn retain(&mut self, subscription: EventSubscription<NotificationEvent>) {
        self.subscriptions.push(subscription);
    }

    pub fn subscribe(&mut self, service: &NotificationService) {
        self.retain(service.subscribe());
    }

    pub fn subscription_count(&self) -> usize {
        self.subscriptions.len()
    }
}

/// Run a delegated authorization request on GPUI's local foreground executor
/// with the runtime deadline timer installed.
pub fn run_notification_request<T: 'static>(
    request: gpui_system::Request<T>,
    cx: &App,
) -> Task<ServiceResult<T>> {
    gpui_adapter::run_request(request, cx)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        FakeNotificationProvider, NotificationAction, NotificationActivationBinding,
        NotificationActivationConfig, NotificationActivationStore, NotificationProgress,
        PermissionKind, PermissionStatus, PostOutcome,
    };
    use gpui::{AppContext, SystemNotificationResponse, TestAppContext};
    use gpui_activation::{
        ActivationId, ActivationInbox, ActivationPayload, ActivationSource, ActivationTarget,
        InboxConfig, RouteRegistry, UrlPolicy,
    };
    use gpui_app_data::{AppData, DirectoryRoots};
    use gpui_permissions::{FakePermissionProvider, PermissionService};
    use gpui_system::{OperationContext, SurfaceId, SystemServices, UiOwner};
    use std::{
        collections::BTreeMap,
        time::{Duration, SystemTime, UNIX_EPOCH},
    };

    fn content() -> NotificationContent {
        NotificationContent::new("Release ready", "Build 42 is available")
            .unwrap()
            .with_progress(NotificationProgress::new(1, 2).unwrap())
            .with_actions(vec![
                NotificationAction::new(ActionId::new("open").unwrap(), "Open").unwrap(),
            ])
            .unwrap()
    }

    #[test]
    fn conversions_preserve_tag_actions_and_response_identity() {
        let id = NotificationId::new("thread-1").unwrap();
        let notification = to_gpui_system_notification(&id, &content());
        assert_eq!(notification.tag.as_ref(), "thread-1");
        assert_eq!(notification.title.as_ref(), "Release ready");
        assert_eq!(notification.actions.len(), 1);
        assert_eq!(notification.actions[0].id.as_ref(), "open");
        assert_eq!(notification.actions[0].label.as_ref(), "Open");

        let callback = from_gpui_notification_response_at(
            ActivationId::new("event-1").unwrap(),
            SystemNotificationResponse {
                tag: "thread-1".into(),
                action_id: Some("open".into()),
            },
            UNIX_EPOCH + Duration::from_secs(7),
        )
        .unwrap();
        assert_eq!(callback.notification_id().as_str(), "thread-1");
        assert_eq!(callback.action_id().unwrap().as_str(), "open");
        assert_eq!(callback.received_at(), UNIX_EPOCH + Duration::from_secs(7));

        let body_tap = from_gpui_notification_response(
            ActivationId::new("event-2").unwrap(),
            SystemNotificationResponse {
                tag: "thread-1".into(),
                action_id: None,
            },
        )
        .unwrap();
        assert!(body_tap.action_id().is_none());
    }

    #[test]
    fn response_rejects_invalid_tags_and_actions() {
        for (tag, action) in [
            ("../bad", None),
            ("ok", Some("../bad")),
            ("", None),
            ("bad tag", None),
        ] {
            let response = SystemNotificationResponse {
                tag: tag.into(),
                action_id: action.map(Into::into),
            };
            assert!(
                from_gpui_notification_response(ActivationId::new("event-1").unwrap(), response)
                    .is_err()
            );
        }
    }

    #[test]
    fn debug_output_redacts_content() {
        let content = content();
        let text = format!("{content:?}");
        assert!(!text.contains("Release ready"));
        assert!(!text.contains("Build 42"));
        assert!(!text.contains("Open"));
    }

    struct ScopeEntity {
        scope: NotificationEntityScope,
    }

    fn bridge_fixture() -> (
        tempfile::TempDir,
        AppData,
        NotificationActivationStore,
        ActivationInbox,
        NotificationId,
        GpuiDesktopNotificationBridge,
        Rc<FakeNotificationProvider>,
    ) {
        let root = tempfile::tempdir().unwrap();
        let app = AppData::open(DirectoryRoots::under(root.path())).unwrap();
        let context = OperationContext::default();
        let mut routes = RouteRegistry::default();
        routes.register("document", 1).unwrap();
        let store = match NotificationActivationStore::open(
            &app,
            "gpui",
            NotificationActivationConfig::default(),
            routes.clone(),
            &context,
        )
        .unwrap()
        {
            crate::Completion::Completed(store) => store,
            crate::Completion::Cancelled(_) => panic!("store open cancelled"),
        };
        let inbox = match ActivationInbox::open(
            &app,
            "gpui",
            InboxConfig::default(),
            UrlPolicy::new(["https"]).unwrap(),
            routes,
            &context,
        )
        .unwrap()
        {
            crate::Completion::Completed(inbox) => inbox,
            crate::Completion::Cancelled(_) => panic!("inbox open cancelled"),
        };
        inbox.set_ready(true);
        let notification_id = NotificationId::new("gpui-thread-1").unwrap();
        let binding = NotificationActivationBinding::new(
            ActivationTarget::new(
                "document",
                1,
                BTreeMap::from([("document".to_owned(), "gpui-document".to_owned())]),
            )
            .unwrap(),
        );
        match store
            .remember(&notification_id, binding, SystemTime::now(), &context)
            .unwrap()
        {
            crate::Completion::Completed(_) => {}
            crate::Completion::Cancelled(_) => panic!("remember cancelled"),
        }
        let provider = Rc::new(FakeNotificationProvider::new());
        let permissions = PermissionService::new(Rc::new(
            FakePermissionProvider::new()
                .with_status(PermissionKind::Notifications, PermissionStatus::Granted),
        ));
        let service = NotificationService::new(provider.clone(), permissions);
        let bridge = GpuiDesktopNotificationBridge::new(service, store.clone(), inbox.clone());
        (root, app, store, inbox, notification_id, bridge, provider)
    }

    #[gpui::test]
    fn gpui_service_install_scope_and_owner_release(cx: &mut TestAppContext) {
        let provider = Rc::new(FakeNotificationProvider::new());
        let permissions = PermissionService::new(Rc::new(
            FakePermissionProvider::new()
                .with_status(PermissionKind::Notifications, PermissionStatus::Granted),
        ));
        let mut services = SystemServices::new();
        install_notification_service(&mut services, provider, permissions).unwrap();
        let service = cx.update(|cx| {
            gpui_system::gpui_adapter::install_system_services(services, cx);
            notification_service(cx).unwrap()
        });
        assert!(service.is_ui_thread());

        let owner = UiOwner::new(SurfaceId(7));
        let probe = owner.clone();
        let entity = cx.update(|cx| {
            cx.new(|cx| ScopeEntity {
                scope: NotificationEntityScope::new(owner, cx),
            })
        });
        cx.update(|cx| {
            entity.update(cx, |entity, _cx| {
                entity.scope.subscribe(&service);
                assert_eq!(entity.scope.subscription_count(), 1);
            });
        });
        assert_eq!(service.subscriber_count(), 1);
        drop(entity);
        // App::update releases dropped entities and invokes their release listeners.
        cx.update(|_| {});
        assert!(probe.context(None).is_err());
        assert_eq!(service.subscriber_count(), 0);
    }

    #[gpui::test]
    fn gpui_bridge_posts_replaces_withdraws_and_routes_responses(cx: &mut TestAppContext) {
        let (_root, _app, store, inbox, notification_id, bridge, provider) = bridge_fixture();
        let initial = NotificationContent::new("Initial title", "Initial body").unwrap();
        let updated = NotificationContent::new("Updated title", "Updated body").unwrap();
        let (post, replace, withdraw) = cx.update(|cx| {
            cx.set_app_identity("com.example.notifications", "Notifications");
            let post = bridge
                .post(
                    cx,
                    NotificationSubmission::new(notification_id.clone(), initial),
                )
                .unwrap();
            let replace = bridge
                .replace(
                    cx,
                    NotificationSubmission::new(notification_id.clone(), updated),
                )
                .unwrap();
            let withdraw = bridge.withdraw(cx, &notification_id).unwrap();
            (post, replace, withdraw)
        });
        assert!(matches!(
            post,
            PostOutcome::Posted(PostReceipt {
                submission: SubmissionState::AcceptedByProvider,
                provider_id: None,
                replaced_existing: None,
                ..
            })
        ));
        assert!(matches!(
            replace,
            PostOutcome::Posted(PostReceipt {
                submission: SubmissionState::AcceptedByProvider,
                provider_id: None,
                replaced_existing: None,
                ..
            })
        ));
        assert_eq!(withdraw, WithdrawOutcome::Withdrawn);
        assert_eq!(cx.shown_system_notifications().len(), 2);
        assert_eq!(
            cx.shown_system_notifications()[1].title.as_ref(),
            "Updated title"
        );
        assert!(cx.delivered_system_notifications().is_empty());
        assert_eq!(cx.dismissed_system_notifications(), ["gpui-thread-1"]);

        let unsupported_provider = Rc::new(
            FakeNotificationProvider::new()
                .with_support(NotificationOperation::ReplaceById, Support::Unsupported),
        );
        let unsupported_service = NotificationService::new(
            unsupported_provider,
            PermissionService::new(Rc::new(
                FakePermissionProvider::new()
                    .with_status(PermissionKind::Notifications, PermissionStatus::Granted),
            )),
        );
        let unsupported_bridge =
            GpuiDesktopNotificationBridge::new(unsupported_service, store.clone(), inbox.clone());
        let unsupported = cx.update(|cx| {
            unsupported_bridge
                .replace(
                    cx,
                    NotificationSubmission::new(
                        notification_id.clone(),
                        NotificationContent::new("Retry title", "Retry body").unwrap(),
                    ),
                )
                .unwrap()
        });
        assert!(matches!(
            unsupported,
            PostOutcome::Rejected(PostRejectReason::Unsupported(
                NotificationOperation::ReplaceById
            ))
        ));
        assert_eq!(cx.shown_system_notifications().len(), 2);

        cx.update(|cx| {
            bridge.install_response_handler(cx, |_response| {
                ActivationId::new("gpui-response-1").unwrap()
            });
        });
        cx.simulate_system_notification_response(SystemNotificationResponse {
            tag: notification_id.as_str().into(),
            action_id: None,
        });
        cx.run_until_parked();
        let pending = match inbox.pending(SystemTime::now(), &OperationContext::default()) {
            Ok(crate::Completion::Completed(pending)) => pending,
            other => panic!("pending failed: {other:?}"),
        };
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].source(), ActivationSource::Notification);
        match pending[0].payload() {
            ActivationPayload::Notification {
                notification_id: routed,
                target,
            } => {
                assert_eq!(routed.as_str(), notification_id.as_str());
                assert_eq!(
                    target.parameters().get("document").map(String::as_str),
                    Some("gpui-document")
                );
            }
            _ => panic!("unexpected payload"),
        }
        match inbox
            .acknowledge(
                pending[0].id(),
                SystemTime::now(),
                &OperationContext::default(),
            )
            .unwrap()
        {
            crate::Completion::Completed(_) => {}
            crate::Completion::Cancelled(_) => panic!("ack cancelled"),
        }
        assert_eq!(provider.submission_count(), 0);
        assert_eq!(store.len(), 1);
    }
}
