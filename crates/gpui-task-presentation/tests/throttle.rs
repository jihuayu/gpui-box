//! Zero-sleep deterministic `Instant` throttling tests.

use gpui_permissions::{
    FakePermissionProvider, PermissionKind, PermissionService, PermissionStatus,
};
use gpui_task_presentation::notifications::{FakeNotificationProvider, NotificationService};
use gpui_task_presentation::{
    AcknowledgmentPrecision, FakeTaskPresentationProvider, SurfaceId, TaskKey,
    TaskPresentationService, TaskPriority, TaskProgressState, TaskScope, ThrottleConfig,
};
use std::{
    num::NonZeroU64,
    rc::Rc,
    time::{Duration, Instant},
};

const SURFACE: SurfaceId = SurfaceId(4);

fn total(value: u64) -> NonZeroU64 {
    NonZeroU64::new(value).expect("non-zero")
}

fn determinate(completed: u64, all: u64) -> TaskProgressState {
    TaskProgressState::determinate(completed, total(all)).expect("determinate")
}

fn notification_service() -> (Rc<FakeNotificationProvider>, NotificationService) {
    let permissions = PermissionService::from_provider(
        FakePermissionProvider::new()
            .with_status(PermissionKind::Notifications, PermissionStatus::Granted),
    );
    let fake = Rc::new(FakeNotificationProvider::new());
    let service = NotificationService::new(fake.clone(), permissions);
    (fake, service)
}

fn service() -> (Rc<FakeTaskPresentationProvider>, TaskPresentationService) {
    let fake = Rc::new(FakeTaskPresentationProvider::new());
    let service = TaskPresentationService::new(fake.clone()).with_throttle(ThrottleConfig::new(
        Duration::from_millis(100),
        Duration::from_millis(500),
    ));
    service.attach_surface(SURFACE, 1, false);
    (fake, service)
}

fn key(value: &str) -> TaskKey {
    TaskKey::new(value).expect("valid key")
}

fn scope() -> TaskScope {
    TaskScope::Surface {
        surface: SURFACE,
        generation: 1,
    }
}

#[test]
fn continuous_determinate_ticks_coalesce_without_provider_calls() {
    let (fake, service) = service();
    let start = Instant::now();
    let task = service
        .register_task_at(
            key("throttle"),
            TaskPriority::Normal,
            scope(),
            determinate(10, 100),
            start,
        )
        .expect("register");
    assert_eq!(fake.progress_calls(SURFACE), 1);

    let outcome = service
        .update_task_at(
            &task.id().clone(),
            determinate(20, 100),
            start + Duration::from_millis(10),
        )
        .expect("update");
    assert!(outcome.coalesced);
    assert!(outcome.receipts.iter().any(|receipt| matches!(
        receipt.precision,
        AcknowledgmentPrecision::CoalescedInMemory
    )));
    assert_eq!(fake.progress_calls(SURFACE), 1);

    let outcome = service
        .update_task_at(
            &task.id().clone(),
            determinate(30, 100),
            start + Duration::from_millis(20),
        )
        .expect("update");
    assert!(outcome.coalesced);
    assert_eq!(fake.progress_calls(SURFACE), 1);

    let outcome = service.flush_coalesced_at(start + Duration::from_millis(50));
    assert!(outcome.coalesced);
    assert_eq!(fake.progress_calls(SURFACE), 1);

    let outcome = service.flush_coalesced_at(start + Duration::from_millis(120));
    assert!(!outcome.coalesced);
    assert_eq!(fake.progress_calls(SURFACE), 2);
    assert_eq!(
        fake.last_progress(SURFACE)
            .expect("progress")
            .progress
            .as_ref()
            .and_then(TaskProgressState::per_mille),
        Some(300)
    );
}

#[test]
fn discrete_transitions_bypass_progress_throttle_immediately() {
    let (fake, service) = service();
    let start = Instant::now();
    let task = service
        .register_task_at(
            key("discrete"),
            TaskPriority::Normal,
            scope(),
            determinate(10, 100),
            start,
        )
        .expect("register");
    assert_eq!(fake.progress_calls(SURFACE), 1);

    service
        .update_task_at(
            &task.id().clone(),
            TaskProgressState::error(Some(100), Some("failed")).expect("error"),
            start,
        )
        .expect("error update");
    assert_eq!(fake.progress_calls(SURFACE), 2);

    service
        .update_task_at(
            &task.id().clone(),
            TaskProgressState::paused(Some(200)).expect("paused"),
            start,
        )
        .expect("pause update");
    assert_eq!(fake.progress_calls(SURFACE), 3);

    service
        .update_task_at(&task.id().clone(), TaskProgressState::Indeterminate, start)
        .expect("indeterminate update");
    assert_eq!(fake.progress_calls(SURFACE), 4);

    service
        .update_task_at(&task.id().clone(), determinate(50, 100), start)
        .expect("determinate update");
    assert_eq!(fake.progress_calls(SURFACE), 5);

    service
        .complete_task_at(&task.id().clone(), start)
        .expect("complete");
    assert_eq!(fake.progress_calls(SURFACE), 6);
    assert!(
        fake.last_progress(SURFACE)
            .expect("clear")
            .progress
            .is_none()
    );
}

#[test]
fn cancellation_and_lease_drop_clear_without_waiting() {
    let (fake, service) = service();
    let start = Instant::now();
    let first = service
        .register_task_at(
            key("cancelled"),
            TaskPriority::Normal,
            scope(),
            determinate(10, 100),
            start,
        )
        .expect("register");
    let second = service
        .register_task_at(
            key("dropped"),
            TaskPriority::High,
            scope(),
            determinate(20, 100),
            start + Duration::from_millis(1),
        )
        .expect("register");
    let calls = fake.progress_calls(SURFACE);
    service
        .cancel_task_at(second.id(), start + Duration::from_millis(2))
        .expect("cancel");
    assert_eq!(fake.progress_calls(SURFACE), calls + 1);
    let calls = fake.progress_calls(SURFACE);
    drop(first);
    assert_eq!(fake.progress_calls(SURFACE), calls + 1);
    assert!(
        fake.last_progress(SURFACE)
            .expect("clear")
            .progress
            .is_none()
    );
}

#[test]
fn notification_mirror_respects_its_own_interval() {
    let (notifications, notification_service) = notification_service();
    let (fake, service) = service();
    let service = service.with_notifications(notification_service);
    let start = Instant::now();
    let task = service
        .register_task_at(
            key("notify"),
            TaskPriority::Normal,
            scope(),
            determinate(10, 100),
            start,
        )
        .expect("register");
    assert_eq!(notifications.submission_count(), 1);

    let outcome = service
        .update_task_at(
            &task.id().clone(),
            determinate(20, 100),
            start + Duration::from_millis(10),
        )
        .expect("update");
    assert!(
        outcome
            .receipts
            .iter()
            .any(|receipt| receipt.surface == SURFACE
                && matches!(
                    receipt.precision,
                    AcknowledgmentPrecision::CoalescedInMemory
                ))
    );
    assert_eq!(notifications.submission_count(), 1);
    assert_eq!(fake.progress_calls(SURFACE), 1);

    service.flush_coalesced_at(start + Duration::from_millis(200));
    assert_eq!(notifications.submission_count(), 1);
    assert_eq!(fake.progress_calls(SURFACE), 2);

    service.flush_coalesced_at(start + Duration::from_millis(650));
    assert_eq!(notifications.submission_count(), 2);
    assert_eq!(
        notifications.delivered_progress(
            &gpui_task_presentation::notification_id_for_task(task.id()).expect("id")
        ),
        Some(Some(
            gpui_task_presentation::notifications::NotificationProgress::new(20, 100)
                .expect("progress"),
        ))
    );
}

#[test]
fn discrete_notification_states_are_immediate_and_terminal_withdraws() {
    let (notifications, notification_service) = notification_service();
    let (_fake, service) = service();
    let service = service.with_notifications(notification_service);
    let start = Instant::now();
    let task = service
        .register_task_at(
            key("notify-discrete"),
            TaskPriority::Normal,
            scope(),
            determinate(10, 100),
            start,
        )
        .expect("register");
    assert_eq!(notifications.submission_count(), 1);

    service
        .update_task_at(
            &task.id().clone(),
            TaskProgressState::error(Some(100), Some("failed")).expect("error"),
            start,
        )
        .expect("error update");
    assert_eq!(notifications.submission_count(), 2);

    service
        .update_task_at(
            &task.id().clone(),
            TaskProgressState::paused(Some(100)).expect("paused"),
            start,
        )
        .expect("pause update");
    assert_eq!(notifications.submission_count(), 3);

    service
        .complete_task_at(&task.id().clone(), start)
        .expect("complete");
    let notification_id = gpui_task_presentation::notification_id_for_task(task.id()).expect("id");
    assert_eq!(notifications.withdrawal_ids(), vec![notification_id]);
}

#[test]
fn terminal_with_summary_policy_replaces_the_ongoing_notification() {
    let (notifications, notification_service) = notification_service();
    let (_fake, service) = service();
    let service = service
        .with_notifications(notification_service)
        .with_terminal_notification_policy(
            gpui_task_presentation::TerminalNotificationPolicy::ReplaceWithSummary,
        );
    let start = Instant::now();
    let task = service
        .register_task_at(
            key("summary"),
            TaskPriority::Normal,
            scope(),
            determinate(10, 100),
            start,
        )
        .expect("register");
    service
        .complete_task_at(task.id(), start)
        .expect("complete");
    assert_eq!(notifications.submission_count(), 2);
    let notification_id = gpui_task_presentation::notification_id_for_task(task.id()).expect("id");
    assert_eq!(notifications.replacement_ids(), vec![notification_id]);
    assert!(notifications.withdrawal_ids().is_empty());
}

#[test]
fn unavailable_surfaces_report_skips_without_failing_updates() {
    let (fake, service) = service();
    fake.simulate_surface_loss(
        SURFACE,
        gpui_task_presentation::TaskPresentationOperation::PresentTaskProgress,
    );
    let start = Instant::now();
    let task = service
        .register_task_at(
            key("unavailable"),
            TaskPriority::Normal,
            scope(),
            determinate(10, 100),
            start,
        )
        .expect("register");
    assert!(
        task.initial_outcome()
            .receipts
            .iter()
            .any(|receipt| { receipt.precision == AcknowledgmentPrecision::SkippedUnavailable })
    );
    assert_eq!(fake.progress_calls(SURFACE), 0);
    service
        .update_task_at(task.id(), determinate(20, 100), start)
        .expect("business work continues without a surface");
}
