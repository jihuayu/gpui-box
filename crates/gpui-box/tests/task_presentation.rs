//! Facade integration across `task_presentation`, `menus` and `notifications`.

#![cfg(feature = "task-presentation")]

use gpui_box::menus::{
    CommandDescriptor, CommandId, CommandPayload, FakeMenuProvider, InvocationContext,
    MenuCommandService,
};
use gpui_box::notifications::{FakeNotificationProvider, NotificationService};
use gpui_box::permissions::{
    FakePermissionProvider, PermissionKind, PermissionService, PermissionStatus,
};
use gpui_box::task_presentation::{
    AcknowledgmentPrecision, Completion, FakeTaskPresentationProvider, SurfaceId, TaskActionButton,
    TaskActionDispatchOutcome, TaskKey, TaskPresentationService, TaskPriority, TaskProgressState,
    TaskScope, TerminalNotificationPolicy, ThrottleConfig, UiOwner,
};
use std::{
    cell::Cell,
    future::Future,
    num::NonZeroU64,
    pin::Pin,
    rc::Rc,
    task::{Context, Poll, Waker},
};

const SURFACE: SurfaceId = SurfaceId(51);

fn total(value: u64) -> NonZeroU64 {
    NonZeroU64::new(value).expect("non-zero")
}

#[test]
fn facade_task_presentation_reexports_coordinated_contracts() {
    assert_eq!(
        gpui_box::task_presentation::UPSTREAM_GPUI_REVISION,
        gpui_box::UPSTREAM_REVISION
    );
    assert_eq!(
        gpui_box::task_presentation::PACKAGE_VERSION,
        gpui_box::VERSION
    );
    assert_eq!(
        gpui_box::task_presentation::TaskPresentationOperation::ALL.len(),
        5
    );
    assert_eq!(
        gpui_box::task_presentation::platform_support_matrix().len(),
        30
    );
    assert_eq!(
        gpui_box::task_presentation::DEFAULT_TASK_PRESENTATION_EVENT_CAPACITY,
        64
    );
    assert_eq!(
        gpui_box::task_presentation::TaskPresentationOperation::PresentAppBadge.as_str(),
        "present_app_badge"
    );
}

#[test]
fn facade_task_presentation_links_menus_and_notifications() {
    let permissions = PermissionService::from_provider(
        FakePermissionProvider::new()
            .with_status(PermissionKind::Notifications, PermissionStatus::Granted),
    );
    let notification_fake = Rc::new(FakeNotificationProvider::new());
    let notification_service = NotificationService::new(notification_fake.clone(), permissions);

    let menu = MenuCommandService::from_provider(FakeMenuProvider::new());
    menu.register_surface(SURFACE, 1).expect("menu surface");
    let calls = Rc::new(Cell::new(0));
    let observed = calls.clone();
    let _registration = menu
        .register_ui::<String, String>(
            CommandDescriptor::ui(CommandId::new("facade.retry").expect("command"), "Retry")
                .expect("descriptor"),
            move |payload, context| {
                assert_eq!(context.trigger, gpui_box::menus::TriggerSource::Button);
                observed.set(observed.get() + 1);
                Ok(payload)
            },
        )
        .expect("register");

    let presentation = TaskPresentationService::from_fake(FakeTaskPresentationProvider::new())
        .with_throttle(ThrottleConfig::immediate())
        .with_menu_service(menu)
        .with_notifications(notification_service);
    presentation.attach_surface(SURFACE, 1, false);

    let action = TaskActionButton::new(
        "retry",
        "Retry",
        CommandId::new("facade.retry").expect("command"),
        CommandPayload::json("facade"),
        Vec::new(),
    )
    .expect("action");
    let task = presentation
        .register_task_with_at(
            gpui_box::task_presentation::TaskRegistration::new(
                TaskKey::new("facade.task").expect("key"),
                TaskProgressState::determinate(40, total(100)).expect("progress"),
            )
            .with_priority(TaskPriority::High)
            .with_scope(TaskScope::Surface {
                surface: SURFACE,
                generation: 1,
            })
            .with_action(action),
            std::time::Instant::now(),
        )
        .expect("task");
    assert!(task.initial_outcome().receipts.len() >= 2);
    assert!(
        task.initial_outcome()
            .receipts
            .iter()
            .any(|receipt| { receipt.precision == AcknowledgmentPrecision::AppliedConfirmed })
    );

    let notification_id =
        gpui_box::task_presentation::notification_id_for_task(task.id()).expect("notification");
    assert_eq!(notification_fake.delivered_ids(), vec![notification_id]);

    let owner = UiOwner::new(SURFACE);
    let ui = owner.context(None).expect("ui");
    let context = InvocationContext::new(gpui_box::task_presentation::OperationContext {
        ui: Some(ui),
        ..Default::default()
    })
    .with_surface(SURFACE);
    let outcome = presentation.dispatch_task_action(task.id(), "retry", context);
    match outcome {
        TaskActionDispatchOutcome::Invoked { mut request, .. } => {
            let waker = Waker::noop();
            let mut task_context = Context::from_waker(waker);
            let completion = match Pin::new(&mut request).poll(&mut task_context) {
                Poll::Ready(Ok(completion)) => completion,
                other => panic!("unexpected request state: {other:?}"),
            };
            assert!(matches!(
                completion,
                Completion::Completed(payload) if payload == CommandPayload::json("facade")
            ));
        }
        TaskActionDispatchOutcome::Rejected { rejection, .. } => {
            panic!("unexpected rejection: {rejection}")
        }
    }
    assert_eq!(calls.get(), 1);

    let record = presentation
        .complete_task_with_business_result(
            task.id(),
            gpui_box::task_presentation::BusinessTaskCompletion::Succeeded("done".to_owned()),
        )
        .expect("complete");
    assert_eq!(
        record.business,
        gpui_box::task_presentation::BusinessTaskCompletion::Succeeded("done".to_owned())
    );
    assert_eq!(
        notification_fake.withdrawal_ids(),
        vec![gpui_box::task_presentation::notification_id_for_task(task.id()).expect("id")]
    );
    assert_eq!(
        gpui_box::task_presentation::TerminalNotificationPolicy::default(),
        TerminalNotificationPolicy::Withdraw
    );
}
