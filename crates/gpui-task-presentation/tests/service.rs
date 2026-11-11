//! Leases, clearing ownership, surface recreation, receipts and bounded events.

use gpui_task_presentation::{
    AcknowledgmentPrecision, AttentionClearPolicy, AttentionRequest, AttentionUrgency,
    BadgeClearPolicy, BadgeRequest, BadgeValue, BusinessTaskCompletion, ClearReason, ErrorCode,
    Event, EventEnvelope, EventSubscription, FakeTaskPresentationProvider, MAX_TASK_TOMBSTONES,
    SurfaceId, SystemError, TaskActionButton, TaskKey, TaskPresentationEvent,
    TaskPresentationOperation, TaskPresentationService, TaskPriority, TaskProgressState,
    TaskRegistration, TaskScope, TaskStateKind,
};
use std::{
    future::Future,
    num::NonZeroU64,
    pin::Pin,
    rc::Rc,
    task::{Context, Poll, Waker},
};

const SURFACE: SurfaceId = SurfaceId(21);

fn total(value: u64) -> NonZeroU64 {
    NonZeroU64::new(value).expect("non-zero")
}

fn determinate(completed: u64, all: u64) -> TaskProgressState {
    TaskProgressState::determinate(completed, total(all)).expect("determinate")
}

fn key(value: &str) -> TaskKey {
    TaskKey::new(value).expect("valid key")
}

fn service() -> (Rc<FakeTaskPresentationProvider>, TaskPresentationService) {
    let fake = Rc::new(FakeTaskPresentationProvider::new());
    let service = TaskPresentationService::new(fake.clone());
    service.attach_surface(SURFACE, 1, false);
    (fake, service)
}

fn surface_scope() -> TaskScope {
    TaskScope::Surface {
        surface: SURFACE,
        generation: 1,
    }
}

fn next_event(
    subscription: &EventSubscription<TaskPresentationEvent>,
) -> Option<Event<TaskPresentationEvent>> {
    let mut next = subscription.try_next().ok()?;
    let waker = Waker::noop();
    let mut context = Context::from_waker(waker);
    match Pin::new(&mut next).poll(&mut context) {
        Poll::Ready(Some(EventEnvelope { payload, .. })) => Some(payload),
        Poll::Ready(None) | Poll::Pending => None,
    }
}

#[test]
fn task_lease_release_is_idempotent_and_drop_cleans_up() {
    let (fake, service) = service();
    let mut lease = service
        .register_task(
            key("lease"),
            TaskPriority::Normal,
            surface_scope(),
            determinate(10, 100),
        )
        .expect("register");
    assert_eq!(service.active_task_count(), 1);
    assert!(lease.is_active());
    assert!(lease.release());
    assert!(!lease.release());
    assert!(!lease.is_active());
    assert_eq!(service.active_task_count(), 0);
    assert_eq!(fake.last_progress(SURFACE).expect("clear").progress, None);

    let dropped = service
        .register_task(
            key("dropped"),
            TaskPriority::High,
            surface_scope(),
            determinate(20, 100),
        )
        .expect("register");
    assert!(service.is_task_active(dropped.id()));
    drop(dropped);
    assert!(!service.is_task_active(&key("dropped")));
    assert_eq!(service.active_task_count(), 0);
    assert_eq!(fake.last_progress(SURFACE).expect("clear").progress, None);
}

#[test]
fn badge_clear_on_surface_focus_preserves_other_policies() {
    let (_fake, service) = service();
    let focus = service
        .apply_badge(BadgeRequest::surface(
            BadgeValue::Count(3),
            SURFACE,
            1,
            BadgeClearPolicy::ClearOnSurfaceFocus,
        ))
        .expect("focus badge");
    let explicit = service
        .apply_badge(BadgeRequest::surface(
            BadgeValue::Count(4),
            SURFACE,
            1,
            BadgeClearPolicy::ExplicitCallerOnly,
        ))
        .expect("explicit badge");
    let lease_scoped = service
        .apply_badge(BadgeRequest::surface(
            BadgeValue::Dot,
            SURFACE,
            1,
            BadgeClearPolicy::LeaseScoped,
        ))
        .expect("lease badge");
    assert_eq!(
        service
            .aggregated_surface_presentation(SURFACE)
            .expect("surface")
            .badge,
        Some(BadgeValue::Dot)
    );

    service.notify_surface_focused(SURFACE);
    assert_eq!(
        service
            .aggregated_surface_presentation(SURFACE)
            .expect("surface")
            .badge,
        Some(BadgeValue::Dot)
    );
    drop(focus);
    assert_eq!(
        service
            .aggregated_surface_presentation(SURFACE)
            .expect("surface")
            .badge,
        Some(BadgeValue::Dot),
        "dropping a non-lease-scoped badge must preserve the newer lease badge"
    );
    drop(lease_scoped);
    assert_eq!(
        service
            .aggregated_surface_presentation(SURFACE)
            .expect("surface")
            .badge,
        Some(BadgeValue::Count(4)),
        "dropping focus-owned or caller-owned badges must not clear them"
    );
    let mut explicit = explicit;
    assert!(explicit.release());
    assert!(
        service
            .aggregated_surface_presentation(SURFACE)
            .expect("surface")
            .badge
            .is_none()
    );
}

#[test]
fn badge_lease_drop_is_policy_aware_and_reapplies_the_winning_fallback() {
    let (fake, service) = service();
    let explicit = service
        .apply_badge(BadgeRequest::surface(
            BadgeValue::Count(4),
            SURFACE,
            1,
            BadgeClearPolicy::ExplicitCallerOnly,
        ))
        .expect("explicit badge");
    drop(explicit);
    assert_eq!(
        service
            .aggregated_surface_presentation(SURFACE)
            .expect("surface")
            .badge,
        Some(BadgeValue::Count(4))
    );

    let focus = service
        .apply_badge(BadgeRequest::surface(
            BadgeValue::Count(5),
            SURFACE,
            1,
            BadgeClearPolicy::ClearOnSurfaceFocus,
        ))
        .expect("focus badge");
    drop(focus);
    assert_eq!(
        service
            .aggregated_surface_presentation(SURFACE)
            .expect("surface")
            .badge,
        Some(BadgeValue::Count(5))
    );

    let all_tasks = service
        .apply_badge(BadgeRequest::surface(
            BadgeValue::Count(6),
            SURFACE,
            1,
            BadgeClearPolicy::ClearOnAllTasksTerminal,
        ))
        .expect("all-tasks badge");
    drop(all_tasks);
    assert_eq!(
        service
            .aggregated_surface_presentation(SURFACE)
            .expect("surface")
            .badge,
        Some(BadgeValue::Count(6))
    );

    let lease = service
        .apply_badge(BadgeRequest::surface(
            BadgeValue::Count(9),
            SURFACE,
            1,
            BadgeClearPolicy::LeaseScoped,
        ))
        .expect("lease badge");
    assert_eq!(
        service
            .aggregated_surface_presentation(SURFACE)
            .expect("surface")
            .badge,
        Some(BadgeValue::Count(9))
    );
    drop(lease);
    assert_eq!(
        service
            .aggregated_surface_presentation(SURFACE)
            .expect("surface")
            .badge,
        Some(BadgeValue::Count(6)),
        "removing the top badge must re-apply the remaining winner"
    );
    assert_eq!(
        fake.badge_history()
            .last()
            .and_then(|record| record.badge.clone()),
        Some(BadgeValue::Count(6))
    );

    let mut caller_owned = service
        .apply_badge(BadgeRequest::surface(
            BadgeValue::Count(7),
            SURFACE,
            1,
            BadgeClearPolicy::ExplicitCallerOnly,
        ))
        .expect("caller badge");
    assert!(caller_owned.release_at(std::time::Instant::now()));
    assert_eq!(
        service
            .aggregated_surface_presentation(SURFACE)
            .expect("surface")
            .badge,
        Some(BadgeValue::Count(6))
    );
}

#[test]
fn attention_lease_drop_is_policy_aware_and_prefers_urgency_fallback() {
    let (fake, first_service) = service();
    let focus = first_service
        .request_attention(
            AttentionRequest::new(AttentionUrgency::Informational, SURFACE, 1),
            AttentionClearPolicy::ClearOnSurfaceFocus,
        )
        .expect("focus attention");
    let focus_id = focus.id();
    drop(focus);
    assert_eq!(
        first_service
            .aggregated_surface_presentation(SURFACE)
            .expect("surface")
            .attention,
        vec![AttentionRequest::new(
            AttentionUrgency::Informational,
            SURFACE,
            1
        )]
    );

    let critical = first_service
        .request_attention(
            AttentionRequest::new(AttentionUrgency::Critical, SURFACE, 1),
            AttentionClearPolicy::LeaseScoped,
        )
        .expect("critical attention");
    drop(critical);
    assert_eq!(
        first_service
            .aggregated_surface_presentation(SURFACE)
            .expect("surface")
            .attention,
        vec![AttentionRequest::new(
            AttentionUrgency::Informational,
            SURFACE,
            1
        )]
    );
    assert_eq!(
        fake.attention_history()
            .last()
            .and_then(|record| record.request.clone())
            .map(|request| request.urgency),
        Some(AttentionUrgency::Informational),
        "dropping critical attention must re-apply the remaining informational request"
    );

    let informational = first_service
        .request_attention(
            AttentionRequest::new(AttentionUrgency::Informational, SURFACE, 1),
            AttentionClearPolicy::LeaseScoped,
        )
        .expect("informational attention");
    drop(informational);
    assert_eq!(
        first_service
            .aggregated_surface_presentation(SURFACE)
            .expect("surface")
            .attention
            .len(),
        1
    );

    let mut releasable = first_service
        .request_attention(
            AttentionRequest::new(AttentionUrgency::Informational, SURFACE, 1),
            AttentionClearPolicy::ClearOnSurfaceFocus,
        )
        .expect("releasable attention");
    assert!(releasable.release_at(std::time::Instant::now()));
    let _ = first_service.clear_attention(focus_id);
    assert!(
        first_service
            .aggregated_surface_presentation(SURFACE)
            .expect("surface")
            .attention
            .is_empty()
    );

    let (clear_fake, clear_service) = service();
    let only = clear_service
        .request_attention(
            AttentionRequest::new(AttentionUrgency::Informational, SURFACE, 1),
            AttentionClearPolicy::LeaseScoped,
        )
        .expect("only attention");
    drop(only);
    assert!(
        clear_service
            .aggregated_surface_presentation(SURFACE)
            .expect("surface")
            .attention
            .is_empty()
    );
    assert!(
        clear_fake
            .attention_history()
            .last()
            .is_some_and(|record| record.request.is_none())
    );
}

#[test]
fn tombstones_are_bounded_with_deterministic_fifo_eviction() {
    let (_fake, service) = service();
    let total = MAX_TASK_TOMBSTONES + 44;
    for index in 0..total {
        let task = service
            .register_task(
                key(&format!("tomb-{index}")),
                TaskPriority::Normal,
                TaskScope::Application,
                TaskProgressState::Indeterminate,
            )
            .expect("register");
        service.complete_task(task.id()).expect("complete");
    }
    assert_eq!(service.tombstone_count(), MAX_TASK_TOMBSTONES);
    assert!(service.tombstone_state(&key("tomb-0")).is_none());
    assert!(service.tombstone_state(&key("tomb-43")).is_none());
    assert_eq!(
        service.tombstone_state(&key("tomb-44")),
        Some(TaskStateKind::Completed)
    );
    assert_eq!(
        service.tombstone_state(&key(&format!("tomb-{}", total - 1))),
        Some(TaskStateKind::Completed)
    );

    let reused = service
        .register_task(
            key(&format!("tomb-{}", total - 1)),
            TaskPriority::Normal,
            TaskScope::Application,
            TaskProgressState::Indeterminate,
        )
        .expect("reuse identity");
    assert!(service.tombstone_state(reused.id()).is_none());
}

#[test]
fn stale_task_lease_cannot_mutate_a_reused_identity() {
    let (_fake, service) = service();
    let mut stale = service
        .register_task(
            key("reused"),
            TaskPriority::Normal,
            TaskScope::Application,
            determinate(10, 100),
        )
        .expect("stale task");
    service.complete_task(stale.id()).expect("complete stale");

    let mut fresh = service
        .register_task(
            key("reused"),
            TaskPriority::Normal,
            TaskScope::Application,
            determinate(20, 100),
        )
        .expect("fresh task");
    assert_eq!(
        service
            .task_state(fresh.id())
            .and_then(|state| state.per_mille()),
        Some(200)
    );

    for error in [
        stale.update(TaskProgressState::Indeterminate).unwrap_err(),
        stale.rebind(TaskScope::Application).unwrap_err(),
        stale.complete().unwrap_err(),
        stale.cancel().unwrap_err(),
        stale
            .complete_with_business_result(BusinessTaskCompletion::<()>::Succeeded(()))
            .unwrap_err(),
    ] {
        assert_eq!(error.code, ErrorCode::ContextExpired);
    }
    assert_eq!(service.active_task_count(), 1);
    assert_eq!(
        service
            .task_state(fresh.id())
            .and_then(|state| state.per_mille()),
        Some(200)
    );
    assert!(!stale.release());
    drop(stale);
    assert!(service.is_task_active(fresh.id()));
    assert!(
        fresh
            .update(TaskProgressState::Paused {
                per_mille: Some(10)
            })
            .is_ok()
    );
    assert!(fresh.complete().is_ok());
    assert!(!service.is_task_active(fresh.id()));

    for (index, terminal) in [TaskProgressState::Completed, TaskProgressState::Cancelled]
        .into_iter()
        .enumerate()
    {
        let error = service
            .register_task(
                key(&format!("terminal-{index}")),
                TaskPriority::Normal,
                TaskScope::Application,
                terminal,
            )
            .expect_err("terminal registration");
        assert_eq!(error.code, ErrorCode::InvalidInput);
    }
}

#[test]
fn pause_and_resume_actions_swap_on_surface_thumbbars() {
    use gpui_task_presentation::menus::{CommandId, CommandPayload};

    let (fake, service) = service();
    let pause = TaskActionButton::new(
        "pause",
        "Pause",
        CommandId::new("task.pause").expect("command"),
        CommandPayload::Unit,
        [TaskStateKind::Determinate],
    )
    .expect("pause button");
    let resume = TaskActionButton::new(
        "resume",
        "Resume",
        CommandId::new("task.resume").expect("command"),
        CommandPayload::Unit,
        [TaskStateKind::Paused],
    )
    .expect("resume button");
    let registration = TaskRegistration::new(key("action-swap"), determinate(10, 100))
        .with_scope(surface_scope())
        .with_actions([pause, resume]);
    let lease = service
        .register_task_with(registration)
        .expect("register with actions");

    let action_ids = |service: &TaskPresentationService| {
        service
            .aggregated_surface_presentation(SURFACE)
            .expect("surface")
            .action_buttons
            .iter()
            .map(|button| button.action_id.as_str().to_owned())
            .collect::<Vec<_>>()
    };
    assert_eq!(action_ids(&service), vec!["pause"]);
    assert_eq!(
        fake.action_history()
            .last()
            .expect("initial actions")
            .actions
            .iter()
            .map(|button| button.action_id.as_str())
            .collect::<Vec<_>>(),
        vec!["pause"]
    );

    lease
        .update(TaskProgressState::Paused {
            per_mille: Some(10),
        })
        .expect("pause");
    assert_eq!(action_ids(&service), vec!["resume"]);
    assert_eq!(
        fake.action_history()
            .last()
            .expect("paused actions")
            .actions
            .iter()
            .map(|button| button.action_id.as_str())
            .collect::<Vec<_>>(),
        vec!["resume"]
    );

    lease.update(determinate(20, 100)).expect("resume");
    assert_eq!(action_ids(&service), vec!["pause"]);
    assert_eq!(
        fake.action_history()
            .last()
            .expect("resumed actions")
            .actions
            .iter()
            .map(|button| button.action_id.as_str())
            .collect::<Vec<_>>(),
        vec!["pause"]
    );
}

#[test]
fn clear_on_all_tasks_terminal_waits_for_the_last_task() {
    let (_fake, service) = service();
    let _badge = service
        .apply_badge(BadgeRequest::surface(
            BadgeValue::Count(2),
            SURFACE,
            1,
            BadgeClearPolicy::ClearOnAllTasksTerminal,
        ))
        .expect("badge");
    let first = service
        .register_task(
            key("first"),
            TaskPriority::Normal,
            surface_scope(),
            TaskProgressState::Indeterminate,
        )
        .expect("first");
    let second = service
        .register_task(
            key("second"),
            TaskPriority::Normal,
            surface_scope(),
            TaskProgressState::Indeterminate,
        )
        .expect("second");
    service.complete_task(first.id()).expect("complete first");
    assert_eq!(
        service
            .aggregated_surface_presentation(SURFACE)
            .expect("surface")
            .badge,
        Some(BadgeValue::Count(2))
    );
    drop(second);
    assert!(
        service
            .aggregated_surface_presentation(SURFACE)
            .expect("surface")
            .badge
            .is_none()
    );
}

#[test]
fn app_badge_and_attention_are_cleared_on_focus() {
    let (_fake, service) = service();
    let badge = service
        .apply_badge(BadgeRequest::application(
            BadgeValue::Dot,
            BadgeClearPolicy::ClearOnSurfaceFocus,
        ))
        .expect("app badge");
    let attention = service
        .request_attention(
            AttentionRequest::new(AttentionUrgency::Critical, SURFACE, 1),
            AttentionClearPolicy::ClearOnSurfaceFocus,
        )
        .expect("attention");
    assert_eq!(
        service.aggregated_app_presentation().badge,
        Some(BadgeValue::Dot)
    );
    service.notify_surface_focused(SURFACE);
    assert!(service.aggregated_app_presentation().badge.is_none());
    let mut badge = badge;
    let mut attention = attention;
    assert!(!badge.release());
    assert!(!attention.release());
}

#[test]
fn recreate_disposes_old_generation_and_replays_app_tasks() {
    let (fake, service) = service();
    let app = service
        .register_task(
            key("app"),
            TaskPriority::Normal,
            TaskScope::Application,
            determinate(40, 100),
        )
        .expect("app");
    let surface_task = service
        .register_task(
            key("surface"),
            TaskPriority::High,
            surface_scope(),
            determinate(80, 100),
        )
        .expect("surface");
    let badge = service
        .apply_badge(BadgeRequest::surface(
            BadgeValue::Count(2),
            SURFACE,
            1,
            BadgeClearPolicy::LeaseScoped,
        ))
        .expect("badge");
    let attention = service
        .request_attention(
            AttentionRequest::new(AttentionUrgency::Informational, SURFACE, 1),
            AttentionClearPolicy::LeaseScoped,
        )
        .expect("attention");

    let outcome = service.recreate_surface(SURFACE, 2).expect("recreate");
    assert!(!outcome.receipts.is_empty());
    assert!(!service.is_task_active(surface_task.id()));
    assert!(service.is_task_active(app.id()));
    let aggregate = service
        .aggregated_surface_presentation(SURFACE)
        .expect("surface");
    assert_eq!(aggregate.generation, 2);
    assert_eq!(aggregate.lead_task.as_ref(), Some(app.id()));
    assert!(aggregate.badge.is_none());
    assert!(aggregate.attention.is_empty());
    let last = fake.last_progress(SURFACE).expect("replay");
    assert_eq!(last.generation, 2);
    assert_eq!(
        last.progress
            .as_ref()
            .and_then(TaskProgressState::per_mille),
        Some(400)
    );

    assert_eq!(
        service
            .recreate_surface(SURFACE, 1)
            .expect_err("stale generation")
            .code,
        ErrorCode::InvalidInput
    );
    let mut badge = badge;
    let mut attention = attention;
    assert!(!badge.release());
    assert!(!attention.release());
    drop(surface_task);
    assert!(service.is_task_active(app.id()));
}

#[test]
fn unavailable_denied_and_failing_surfaces_preserve_business_results() {
    let (fake, first_service) = service();
    fake.simulate_surface_loss(SURFACE, TaskPresentationOperation::PresentTaskProgress);
    let task = first_service
        .register_task(
            key("unavailable"),
            TaskPriority::Normal,
            surface_scope(),
            determinate(10, 100),
        )
        .expect("register");
    assert!(
        task.initial_outcome()
            .receipts
            .iter()
            .any(|receipt| { receipt.precision == AcknowledgmentPrecision::SkippedUnavailable })
    );
    let record = first_service
        .complete_task_with_business_result(
            task.id(),
            BusinessTaskCompletion::Succeeded("payload".to_owned()),
        )
        .expect("complete");
    assert_eq!(
        record.business,
        BusinessTaskCompletion::Succeeded("payload".to_owned())
    );
    assert!(
        record
            .presentation
            .receipts
            .iter()
            .all(|receipt| receipt.precision.is_skipped()
                || matches!(receipt.precision, AcknowledgmentPrecision::Failed { .. }))
    );

    let (denied_fake, denied_service) = service();
    denied_fake.deny_permission(SURFACE, TaskPresentationOperation::PresentTaskProgress);
    let denied = denied_service
        .register_task(
            key("denied"),
            TaskPriority::Normal,
            surface_scope(),
            determinate(10, 100),
        )
        .expect("register");
    assert!(
        denied.initial_outcome().receipts.iter().any(|receipt| {
            receipt.precision == AcknowledgmentPrecision::SkippedPermissionDenied
        })
    );
    assert_eq!(denied_fake.progress_calls(SURFACE), 0);
    let record = denied_service
        .complete_task_with_business_result(
            denied.id(),
            BusinessTaskCompletion::<()>::Cancelled(gpui_system::CancelReason::User),
        )
        .expect("complete");
    assert!(record.business.is_cancelled());

    let (failing_fake, failing_service) = service();
    failing_fake.set_failure(
        SURFACE,
        Some(SystemError::new(
            ErrorCode::PlatformFailure,
            "The taskbar disappeared",
        )),
    );
    let failing = failing_service
        .register_task(
            key("failing"),
            TaskPriority::Normal,
            surface_scope(),
            determinate(10, 100),
        )
        .expect("register");
    assert!(failing.initial_outcome().receipts.iter().any(|receipt| {
        matches!(
            receipt.precision,
            AcknowledgmentPrecision::Failed {
                code: ErrorCode::PlatformFailure
            }
        ) && receipt
            .provider_error
            .as_ref()
            .is_some_and(|error| error.code == ErrorCode::PlatformFailure)
    }));
    let record = failing_service
        .complete_task_with_business_result(
            failing.id(),
            BusinessTaskCompletion::<()>::Failed("business failure".to_owned()),
        )
        .expect("business completion survives presentation failure");
    assert_eq!(
        record.business,
        BusinessTaskCompletion::Failed("business failure".to_owned())
    );
}

#[test]
fn unsupported_provider_reports_skips_and_never_confirms() {
    let service = TaskPresentationService::unsupported();
    service.attach_surface(SURFACE, 1, false);
    let task = service
        .register_task(
            key("unsupported"),
            TaskPriority::Normal,
            surface_scope(),
            TaskProgressState::Indeterminate,
        )
        .expect("register");
    assert!(!task.initial_outcome().receipts.is_empty());
    assert!(
        task.initial_outcome()
            .receipts
            .iter()
            .all(|receipt| { receipt.precision == AcknowledgmentPrecision::SkippedUnavailable })
    );
}

#[test]
fn bounded_event_ring_emits_resync_required_after_overflow() {
    let (_fake, service) = service();
    let subscription = service.subscribe();
    for index in 0..40 {
        let _task = service
            .register_task(
                key(&format!("event-{index}")),
                TaskPriority::Normal,
                TaskScope::Application,
                determinate(index as u64, 100),
            )
            .expect("register");
    }
    let mut saw_resync = false;
    for _ in 0..200 {
        match next_event(&subscription) {
            Some(Event::ResyncRequired) => {
                saw_resync = true;
                break;
            }
            Some(Event::Value(_)) => {}
            None => break,
        }
    }
    assert!(saw_resync, "ring overflow must surface ResyncRequired");
}

#[test]
fn lifecycle_sync_attaches_recreates_and_clears_focus_owned_indicators() {
    use gpui_lifecycle::{
        AppPhase, LifecycleState, MemoryPressure, SessionLockState, SurfaceSnapshot,
        SurfaceVisibility,
    };

    let (_fake, service) = service();
    let _badge = service
        .apply_badge(BadgeRequest::surface(
            BadgeValue::Count(1),
            SURFACE,
            1,
            BadgeClearPolicy::ClearOnSurfaceFocus,
        ))
        .expect("badge");
    let _attention = service
        .request_attention(
            AttentionRequest::new(AttentionUrgency::Informational, SURFACE, 1),
            AttentionClearPolicy::ClearOnSurfaceFocus,
        )
        .expect("attention");
    let focused = LifecycleState::from_host(
        AppPhase::ForegroundActive,
        [(
            SURFACE,
            SurfaceSnapshot {
                visibility: SurfaceVisibility::Visible { focused: true },
                generation: 1,
            },
        )],
        SessionLockState::Unlocked,
        MemoryPressure::Normal,
    );
    service.sync_lifecycle(&focused);
    assert!(
        service
            .aggregated_surface_presentation(SURFACE)
            .expect("surface")
            .badge
            .is_none()
    );
    assert!(
        service
            .aggregated_surface_presentation(SURFACE)
            .expect("surface")
            .attention
            .is_empty()
    );

    let app = service
        .register_task(
            key("lifecycle-app"),
            TaskPriority::Normal,
            TaskScope::Application,
            determinate(30, 100),
        )
        .expect("app task");
    service.attach_surface(SURFACE, 1, false);
    let recreated = LifecycleState::from_host(
        AppPhase::ForegroundActive,
        [(
            SURFACE,
            SurfaceSnapshot {
                visibility: SurfaceVisibility::Visible { focused: false },
                generation: 2,
            },
        )],
        SessionLockState::Unlocked,
        MemoryPressure::Normal,
    );
    service.sync_lifecycle(&recreated);
    let aggregate = service
        .aggregated_surface_presentation(SURFACE)
        .expect("surface");
    assert_eq!(aggregate.generation, 2);
    assert_eq!(aggregate.lead_task.as_ref(), Some(app.id()));
    assert_eq!(
        aggregate.progress.and_then(|state| state.per_mille()),
        Some(300)
    );
}

#[test]
fn clear_reason_labels_are_stable() {
    assert_eq!(ClearReason::Focus.as_str(), "focus");
    assert_eq!(ClearReason::LeaseReleased.as_str(), "lease_released");
    assert_eq!(ClearReason::AllTasksTerminal.as_str(), "all_tasks_terminal");
    assert_eq!(
        ClearReason::GenerationExpired.as_str(),
        "generation_expired"
    );
}

#[cfg(feature = "gpui")]
mod gpui_adapter {
    use super::*;
    use gpui::{AppContext, TestAppContext};
    use gpui_task_presentation::gpui_adapter::{
        TaskPresentationAppExt, TaskPresentationEntityModel, init_task_presentation,
        task_presentation_service,
    };

    #[gpui::test]
    fn installs_global_registry_and_disposes_entity_surface(cx: &mut TestAppContext) {
        let provider = Rc::new(FakeTaskPresentationProvider::new());
        let service = cx.update(|cx| init_task_presentation(cx, provider));
        assert!(
            service.shares_state_with(
                &cx.update(|cx| task_presentation_service(cx))
                    .expect("global")
            )
        );
        let owner = gpui_task_presentation::UiOwner::new(SurfaceId(31));
        let model = cx.update(|cx| {
            cx.new(|cx| TaskPresentationEntityModel::for_entity(owner, service.clone(), cx))
        });
        let task = cx.update(|cx| {
            model.update(cx, |model, _| {
                service
                    .register_task(
                        key("entity"),
                        TaskPriority::Normal,
                        TaskScope::Surface {
                            surface: model.surface(),
                            generation: model.generation(),
                        },
                        TaskProgressState::Indeterminate,
                    )
                    .expect("task")
            })
        });
        assert!(service.is_task_active(task.id()));
        drop(model);
        cx.update(|_| {});
        assert!(!service.is_task_active(task.id()));
    }

    #[gpui::test]
    fn missing_service_is_typed(cx: &mut TestAppContext) {
        cx.update(|cx| {
            assert_eq!(
                task_presentation_service(cx).expect_err("missing").code,
                ErrorCode::Unavailable
            );
            let _ = cx.task_presentation_service();
        });
    }
}
