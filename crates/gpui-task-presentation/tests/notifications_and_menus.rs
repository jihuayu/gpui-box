//! End-to-end integration with real `gpui-notifications` and `gpui-menus`.

use gpui_menus::{
    CommandDescriptor, CommandId, CommandPayload, FakeMenuProvider, InvocationContext,
    MenuCommandService,
};
use gpui_permissions::{
    FakePermissionProvider, PermissionKind, PermissionService, PermissionStatus,
};
use gpui_task_presentation::notifications::{
    FakeNotificationProvider, NotificationService, PostOutcome,
};
use gpui_task_presentation::{
    AcknowledgmentPrecision, ActionRejectionReason, BusinessTaskCompletion,
    FakeTaskPresentationProvider, SurfaceId, TaskActionButton, TaskActionDispatchOutcome, TaskKey,
    TaskPresentationOperation, TaskPresentationService, TaskPriority, TaskProgressState,
    TaskRegistration, TaskScope, TaskStateKind, ThrottleConfig, UiOwner,
    notification_action_id_for_task, notification_id_for_task,
};
use std::{
    cell::Cell,
    future::Future,
    num::NonZeroU64,
    pin::Pin,
    rc::Rc,
    task::{Context, Poll, Waker},
};

const SURFACE: SurfaceId = SurfaceId(41);

fn total(value: u64) -> NonZeroU64 {
    NonZeroU64::new(value).expect("non-zero")
}

fn determinate(completed: u64, all: u64) -> TaskProgressState {
    TaskProgressState::determinate(completed, total(all)).expect("determinate")
}

fn key(value: &str) -> TaskKey {
    TaskKey::new(value).expect("valid key")
}

fn tasks() -> (Rc<FakeTaskPresentationProvider>, TaskPresentationService) {
    let fake = Rc::new(FakeTaskPresentationProvider::new());
    let service =
        TaskPresentationService::new(fake.clone()).with_throttle(ThrottleConfig::immediate());
    service.attach_surface(SURFACE, 1, false);
    (fake, service)
}

fn notifications(status: PermissionStatus) -> (Rc<FakeNotificationProvider>, NotificationService) {
    let permissions = PermissionService::from_provider(
        FakePermissionProvider::new().with_status(PermissionKind::Notifications, status),
    );
    let fake = Rc::new(FakeNotificationProvider::new().with_auto_display(true));
    let service = NotificationService::new(fake.clone(), permissions);
    (fake, service)
}

fn command_id(value: &str) -> CommandId {
    CommandId::new(value).expect("command id")
}

fn action(action_id: &str, command: &str, enabled: Vec<TaskStateKind>) -> TaskActionButton {
    TaskActionButton::new(
        action_id,
        "Run action",
        command_id(command),
        CommandPayload::json("payload"),
        enabled,
    )
    .expect("action")
}

fn ui_context(owner: &UiOwner) -> (InvocationContext, gpui_task_presentation::OperationContext) {
    let ui = owner.context(None).expect("ui context");
    let operation = gpui_task_presentation::OperationContext {
        ui: Some(ui),
        ..Default::default()
    };
    (
        InvocationContext::new(operation.clone()).with_surface(SURFACE),
        operation,
    )
}

fn resolve(
    mut request: gpui_task_presentation::Request<CommandPayload>,
) -> gpui_task_presentation::Completion<CommandPayload> {
    let waker = Waker::noop();
    let mut context = Context::from_waker(waker);
    for _ in 0..16 {
        match Pin::new(&mut request).poll(&mut context) {
            Poll::Ready(result) => return result.expect("command result"),
            Poll::Pending => std::hint::spin_loop(),
        }
    }
    panic!("UI command did not complete synchronously");
}

#[test]
fn valid_action_click_invokes_the_linked_menu_command_once() {
    let menu = MenuCommandService::from_provider(FakeMenuProvider::new());
    menu.register_surface(SURFACE, 1).expect("surface");
    let calls = Rc::new(Cell::new(0));
    let observed = calls.clone();
    let _registration = menu
        .register_ui::<String, String>(
            CommandDescriptor::ui(command_id("task.retry"), "Retry").expect("descriptor"),
            move |payload, context| {
                assert_eq!(payload, "payload");
                assert_eq!(context.trigger, gpui_menus::TriggerSource::Button);
                assert_eq!(context.surface, Some(SURFACE));
                observed.set(observed.get() + 1);
                Ok(payload)
            },
        )
        .expect("register command");
    let (_fake, service) = tasks();
    let service = service.with_menu_service(menu);
    let owner = UiOwner::new(SURFACE);
    let (context, _operation) = ui_context(&owner);
    let task = service
        .register_task_with(
            TaskRegistration::new(key("action.task"), determinate(10, 100))
                .with_scope(TaskScope::Surface {
                    surface: SURFACE,
                    generation: 1,
                })
                .with_action(action("retry", "task.retry", Vec::new())),
        )
        .expect("task");

    let outcome = service.dispatch_task_action(task.id(), "retry", context);
    match outcome {
        TaskActionDispatchOutcome::Invoked {
            request,
            task: invoked,
            ..
        } => {
            assert_eq!(&invoked, task.id());
            let completion = resolve(request);
            assert!(
                matches!(completion, gpui_task_presentation::Completion::Completed(payload) if payload == CommandPayload::json("payload"))
            );
        }
        TaskActionDispatchOutcome::Rejected { rejection, .. } => {
            panic!("unexpected rejection: {rejection}")
        }
    }
    assert_eq!(calls.get(), 1);
}

#[test]
fn disabled_and_unknown_actions_are_rejected_without_invocation() {
    let menu = MenuCommandService::from_provider(FakeMenuProvider::new());
    menu.register_surface(SURFACE, 1).expect("surface");
    let calls = Rc::new(Cell::new(0));
    let observed = calls.clone();
    let _registration = menu
        .register_ui::<String, String>(
            CommandDescriptor::ui(command_id("task.retry"), "Retry").expect("descriptor"),
            move |payload, _| {
                observed.set(observed.get() + 1);
                Ok(payload)
            },
        )
        .expect("register");
    let (_fake, service) = tasks();
    let service = service.with_menu_service(menu);
    let owner = UiOwner::new(SURFACE);
    let (context, _) = ui_context(&owner);
    let task = service
        .register_task_with(
            TaskRegistration::new(key("disabled"), determinate(10, 100))
                .with_scope(TaskScope::Surface {
                    surface: SURFACE,
                    generation: 1,
                })
                .with_action(action(
                    "retry",
                    "task.retry",
                    vec![TaskStateKind::Determinate],
                )),
        )
        .expect("task");
    service
        .update_task(
            task.id(),
            TaskProgressState::error(Some(100), Some("failed")).expect("error"),
        )
        .expect("update");
    let outcome = service.dispatch_task_action(task.id(), "retry", context.clone());
    assert_eq!(
        outcome.rejection().expect("rejection").reason,
        ActionRejectionReason::ActionDisabled
    );
    let outcome = service.dispatch_task_action(task.id(), "missing", context);
    assert_eq!(
        outcome.rejection().expect("rejection").reason,
        ActionRejectionReason::UnknownAction
    );
    assert_eq!(calls.get(), 0);
}

#[test]
fn stale_generation_and_terminal_clicks_are_rejected() {
    let menu = MenuCommandService::from_provider(FakeMenuProvider::new());
    menu.register_surface(SURFACE, 1).expect("surface");
    let calls = Rc::new(Cell::new(0));
    let observed = calls.clone();
    let _registration = menu
        .register_ui::<(), ()>(
            CommandDescriptor::ui(command_id("task.cleanup"), "Cleanup").expect("descriptor"),
            move |(), _| {
                observed.set(observed.get() + 1);
                Ok(())
            },
        )
        .expect("register");
    let (_fake, service) = tasks();
    let service = service.with_menu_service(menu);
    let owner = UiOwner::new(SURFACE);
    let (good_context, _) = ui_context(&owner);
    let task = service
        .register_task_with(
            TaskRegistration::new(key("stale"), determinate(10, 100))
                .with_scope(TaskScope::Surface {
                    surface: SURFACE,
                    generation: 1,
                })
                .with_action(
                    TaskActionButton::new(
                        "cleanup",
                        "Cleanup",
                        command_id("task.cleanup"),
                        CommandPayload::Unit,
                        Vec::new(),
                    )
                    .expect("action"),
                ),
        )
        .expect("task");

    let advanced = UiOwner::new(SURFACE);
    advanced.advance_generation().expect("advance");
    let (wrong_generation, _) = ui_context(&advanced);
    let outcome = service.dispatch_task_action(task.id(), "cleanup", wrong_generation);
    assert_eq!(
        outcome.rejection().expect("rejection").reason,
        ActionRejectionReason::GenerationMismatch
    );

    service.complete_task(task.id()).expect("complete");
    let outcome = service.dispatch_task_action(task.id(), "cleanup", good_context);
    assert_eq!(
        outcome.rejection().expect("rejection").reason,
        ActionRejectionReason::TaskTerminal
    );
    assert_eq!(calls.get(), 0);
}

#[test]
fn notification_mirror_posts_stable_ids_progress_and_actions() {
    let (notifications_fake, notification_service) = notifications(PermissionStatus::Granted);
    let (_fake, service) = tasks();
    let service = service.with_notifications(notification_service);
    let task = service
        .register_task_with(
            TaskRegistration::new(key("notified"), determinate(25, 100))
                .with_scope(TaskScope::Surface {
                    surface: SURFACE,
                    generation: 1,
                })
                .with_title("A long task")
                .with_body("Working through items")
                .with_action(action("retry", "task.retry", Vec::new())),
        )
        .expect("task");
    let notification_id = notification_id_for_task(task.id()).expect("notification id");
    assert_eq!(
        notifications_fake.delivered_ids(),
        vec![notification_id.clone()]
    );
    assert_eq!(
        notifications_fake.delivered_progress(&notification_id),
        Some(Some(
            gpui_task_presentation::notifications::NotificationProgress::new(25, 100)
                .expect("progress")
        ))
    );
    assert_eq!(
        notifications_fake.delivered_actions(&notification_id),
        Some(vec![
            notification_action_id_for_task(
                task.id(),
                &gpui_task_presentation::TaskActionId::new("retry").expect("action id")
            )
            .expect("notification action")
        ])
    );
    assert_eq!(
        notifications_fake
            .delivered_title(&notification_id)
            .as_deref(),
        Some("A long task")
    );

    service
        .update_task(task.id(), determinate(75, 100))
        .expect("update");
    assert_eq!(
        notifications_fake.replacement_ids(),
        vec![notification_id.clone()]
    );
    assert_eq!(notifications_fake.delivered_ids().len(), 1);
    assert_eq!(
        notifications_fake.delivered_progress(&notification_id),
        Some(Some(
            gpui_task_presentation::notifications::NotificationProgress::new(75, 100)
                .expect("progress")
        ))
    );
}

#[test]
fn permission_denied_is_a_presentation_skip_not_a_business_failure() {
    let (notifications_fake, notification_service) = notifications(PermissionStatus::Denied {
        can_request_again: false,
    });
    let (_fake, service) = tasks();
    let service = service.with_notifications(notification_service);
    let task = service
        .register_task(
            key("denied-notification"),
            TaskPriority::Normal,
            TaskScope::Surface {
                surface: SURFACE,
                generation: 1,
            },
            determinate(10, 100),
        )
        .expect("task");
    assert!(
        task.initial_outcome().receipts.iter().any(|receipt| {
            receipt.precision == AcknowledgmentPrecision::SkippedPermissionDenied
        })
    );
    assert_eq!(notifications_fake.submission_count(), 0);
    let record = service
        .complete_task_with_business_result(
            task.id(),
            BusinessTaskCompletion::Succeeded("business".to_owned()),
        )
        .expect("complete");
    assert_eq!(
        record.business,
        BusinessTaskCompletion::Succeeded("business".to_owned())
    );
}

#[test]
fn notification_action_activation_dispatches_and_stale_activation_is_dropped() {
    let menu = MenuCommandService::from_provider(FakeMenuProvider::new());
    menu.register_surface(SURFACE, 1).expect("surface");
    let calls = Rc::new(Cell::new(0));
    let observed = calls.clone();
    let _registration = menu
        .register_ui::<String, String>(
            CommandDescriptor::ui(command_id("task.retry"), "Retry").expect("descriptor"),
            move |payload, context| {
                assert_eq!(context.trigger, gpui_menus::TriggerSource::Button);
                observed.set(observed.get() + 1);
                Ok(payload)
            },
        )
        .expect("register");
    let (notifications_fake, notification_service) = notifications(PermissionStatus::Granted);
    let (_fake, service) = tasks();
    let service = service
        .with_menu_service(menu)
        .with_notifications(notification_service);
    let owner = UiOwner::new(SURFACE);
    let (context, _) = ui_context(&owner);
    let task = service
        .register_task_with(
            TaskRegistration::new(key("activated"), determinate(10, 100))
                .with_scope(TaskScope::Surface {
                    surface: SURFACE,
                    generation: 1,
                })
                .with_action(action("retry", "task.retry", Vec::new())),
        )
        .expect("task");
    let notification_id = notification_id_for_task(task.id()).expect("notification id");
    let notification_action = notification_action_id_for_task(
        task.id(),
        &gpui_task_presentation::TaskActionId::new("retry").expect("action"),
    )
    .expect("action id");
    let notification_service = service
        .notification_mirror()
        .expect("mirror")
        .service()
        .clone();
    notification_service
        .record_activated(&notification_id, Some(notification_action.clone()))
        .expect("record activation");
    let outcomes = service.pump_notification_activations(context.clone());
    assert_eq!(outcomes.len(), 1);
    match &outcomes[0] {
        TaskActionDispatchOutcome::Invoked { request, .. } => {
            // The request is consumed below; clone is impossible, so verify the
            // dispatched effect through the handler counter.
            let _ = request;
        }
        TaskActionDispatchOutcome::Rejected { rejection, .. } => {
            panic!("unexpected rejection: {rejection}")
        }
    }
    assert_eq!(calls.get(), 1);

    service.complete_task(task.id()).expect("complete");
    notification_service
        .record_activated(&notification_id, Some(notification_action))
        .expect("late activation");
    let outcomes = service.pump_notification_activations(context);
    assert_eq!(outcomes.len(), 1);
    assert!(outcomes[0].is_rejected());
    assert_eq!(
        outcomes[0].rejection().expect("rejection").reason,
        ActionRejectionReason::TaskTerminal
    );
    assert_eq!(calls.get(), 1);
    assert_eq!(notifications_fake.withdrawal_ids(), vec![notification_id]);
}

#[test]
fn notification_terminal_summary_replacement_policy() {
    let (notifications_fake, notification_service) = notifications(PermissionStatus::Granted);
    let (_fake, service) = tasks();
    let service = service
        .with_notifications(notification_service)
        .with_terminal_notification_policy(
            gpui_task_presentation::TerminalNotificationPolicy::ReplaceWithSummary,
        );
    let task = service
        .register_task(
            key("summary-notification"),
            TaskPriority::Normal,
            TaskScope::Surface {
                surface: SURFACE,
                generation: 1,
            },
            determinate(10, 100),
        )
        .expect("task");
    let notification_id = notification_id_for_task(task.id()).expect("id");
    service.cancel_task(task.id()).expect("cancel");
    assert_eq!(
        notifications_fake.replacement_ids(),
        vec![notification_id.clone()]
    );
    assert!(notifications_fake.withdrawal_ids().is_empty());
    assert!(
        notifications_fake
            .delivered_body(&notification_id)
            .is_some_and(|body| body.contains("Cancelled"))
    );
}

#[test]
fn notification_provider_failure_is_captured_in_the_receipt() {
    let (notifications_fake, notification_service) = notifications(PermissionStatus::Granted);
    notifications_fake.set_support(
        gpui_task_presentation::notifications::NotificationOperation::PostImmediate,
        gpui_task_presentation::Support::Unsupported,
    );
    notifications_fake.set_availability(
        gpui_task_presentation::notifications::NotificationOperation::PostImmediate,
        gpui_task_presentation::Availability::Unavailable,
    );
    let (_fake, service) = tasks();
    let service = service.with_notifications(notification_service);
    let task = service
        .register_task(
            key("notification-failure"),
            TaskPriority::Normal,
            TaskScope::Surface {
                surface: SURFACE,
                generation: 1,
            },
            determinate(10, 100),
        )
        .expect("task");
    assert!(task.initial_outcome().receipts.iter().any(|receipt| {
        receipt.operation == TaskPresentationOperation::MirrorProgressNotification
            && receipt.precision == AcknowledgmentPrecision::SkippedUnavailable
    }));
    let record = service
        .complete_task_with_business_result(task.id(), BusinessTaskCompletion::Succeeded(7u32))
        .expect("complete");
    assert_eq!(record.business, BusinessTaskCompletion::Succeeded(7u32));
}

#[test]
fn notification_post_outcome_translation_is_explicit() {
    // A posted submission is accepted host evidence, never a display claim.
    let (fake, service) = notifications(PermissionStatus::Granted);
    let content = gpui_task_presentation::notifications::NotificationContent::new("Title", "Body")
        .expect("content");
    let id = gpui_task_presentation::notifications::NotificationId::new("translation").expect("id");
    let submission =
        gpui_task_presentation::notifications::NotificationSubmission::new(id.clone(), content);
    let outcome = service.post(submission).expect("post");
    assert!(matches!(outcome, PostOutcome::Posted(_)));
    assert_eq!(fake.submission_count(), 1);
}
