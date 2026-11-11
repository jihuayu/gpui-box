//! Deterministic concurrent aggregation across priorities, severity and drops.

use gpui_task_presentation::{
    AggregateMode, FakeTaskPresentationProvider, SurfaceId, TaskKey, TaskPresentationService,
    TaskPriority, TaskProgressState, TaskScope,
};
use std::num::NonZeroU64;

const SURFACE: SurfaceId = SurfaceId(7);

fn total(value: u64) -> NonZeroU64 {
    NonZeroU64::new(value).expect("non-zero")
}

fn key(value: &str) -> TaskKey {
    TaskKey::new(value).expect("valid key")
}

fn service(mode: AggregateMode) -> TaskPresentationService {
    let service = TaskPresentationService::from_fake(FakeTaskPresentationProvider::new())
        .with_aggregate_mode(mode);
    service.attach_surface(SURFACE, 1, false);
    service
}

fn surface_scope() -> TaskScope {
    TaskScope::Surface {
        surface: SURFACE,
        generation: 1,
    }
}

#[test]
fn highest_priority_lead_wins_and_priority_falls_back() {
    let service = service(AggregateMode::HighestPriorityLead);
    let low = service
        .register_task(
            key("low"),
            TaskPriority::Background,
            surface_scope(),
            TaskProgressState::determinate(10, total(100)).expect("low"),
        )
        .expect("low");
    let normal = service
        .register_task(
            key("normal"),
            TaskPriority::Normal,
            surface_scope(),
            TaskProgressState::determinate(20, total(100)).expect("normal"),
        )
        .expect("normal");
    let high = service
        .register_task(
            key("high"),
            TaskPriority::High,
            surface_scope(),
            TaskProgressState::determinate(30, total(100)).expect("high"),
        )
        .expect("high");

    let aggregate = service
        .aggregated_surface_presentation(SURFACE)
        .expect("surface");
    assert_eq!(aggregate.lead_task.as_ref(), Some(high.id()));
    assert_eq!(
        aggregate
            .progress
            .as_ref()
            .and_then(TaskProgressState::per_mille),
        Some(300)
    );
    assert_eq!(aggregate.active_task_count, 3);

    service.complete_task(high.id()).expect("complete high");
    let aggregate = service
        .aggregated_surface_presentation(SURFACE)
        .expect("surface");
    assert_eq!(aggregate.lead_task.as_ref(), Some(normal.id()));
    assert_eq!(
        aggregate
            .progress
            .as_ref()
            .and_then(TaskProgressState::per_mille),
        Some(200)
    );

    service.cancel_task(normal.id()).expect("cancel normal");
    let aggregate = service
        .aggregated_surface_presentation(SURFACE)
        .expect("surface");
    assert_eq!(aggregate.lead_task.as_ref(), Some(low.id()));
    assert_eq!(
        aggregate
            .progress
            .as_ref()
            .and_then(TaskProgressState::per_mille),
        Some(100)
    );

    service.complete_task(low.id()).expect("complete low");
    let aggregate = service
        .aggregated_surface_presentation(SURFACE)
        .expect("surface");
    assert!(aggregate.progress.is_none());
    assert_eq!(aggregate.active_task_count, 0);
    assert!(aggregate.is_empty());
}

#[test]
fn severity_precedence_is_error_then_paused_then_determinate_then_indeterminate() {
    let service = service(AggregateMode::HighestPriorityLead);
    let determinate = service
        .register_task(
            key("determinate"),
            TaskPriority::Normal,
            surface_scope(),
            TaskProgressState::determinate(50, total(100)).expect("determinate"),
        )
        .expect("determinate");
    let indeterminate = service
        .register_task(
            key("indeterminate"),
            TaskPriority::Normal,
            surface_scope(),
            TaskProgressState::Indeterminate,
        )
        .expect("indeterminate");
    let paused = service
        .register_task(
            key("paused"),
            TaskPriority::Normal,
            surface_scope(),
            TaskProgressState::paused(Some(500)).expect("paused"),
        )
        .expect("paused");
    let error = service
        .register_task(
            key("error"),
            TaskPriority::Normal,
            surface_scope(),
            TaskProgressState::error(Some(500), Some("failed")).expect("error"),
        )
        .expect("error");

    let aggregate = service
        .aggregated_surface_presentation(SURFACE)
        .expect("surface");
    assert_eq!(aggregate.lead_task.as_ref(), Some(error.id()));
    assert_eq!(
        aggregate.progress.as_ref().map(TaskProgressState::kind),
        Some(gpui_task_presentation::TaskStateKind::Error)
    );

    service.complete_task(error.id()).expect("complete error");
    assert_eq!(
        service
            .aggregated_surface_presentation(SURFACE)
            .expect("surface")
            .lead_task
            .as_ref(),
        Some(paused.id())
    );
    service.complete_task(paused.id()).expect("complete paused");
    assert_eq!(
        service
            .aggregated_surface_presentation(SURFACE)
            .expect("surface")
            .lead_task
            .as_ref(),
        Some(determinate.id())
    );
    service
        .complete_task(determinate.id())
        .expect("complete determinate");
    assert_eq!(
        service
            .aggregated_surface_presentation(SURFACE)
            .expect("surface")
            .lead_task
            .as_ref(),
        Some(indeterminate.id())
    );
}

#[test]
fn priority_beats_severity_and_registration_order_breaks_ties() {
    let service = service(AggregateMode::HighestPriorityLead);
    let first = service
        .register_task(
            key("first"),
            TaskPriority::High,
            surface_scope(),
            TaskProgressState::Indeterminate,
        )
        .expect("first");
    let second = service
        .register_task(
            key("second"),
            TaskPriority::High,
            surface_scope(),
            TaskProgressState::Indeterminate,
        )
        .expect("second");
    let critical = service
        .register_task(
            key("critical"),
            TaskPriority::Critical,
            surface_scope(),
            TaskProgressState::Indeterminate,
        )
        .expect("critical");

    let aggregate = service
        .aggregated_surface_presentation(SURFACE)
        .expect("surface");
    assert_eq!(aggregate.lead_task.as_ref(), Some(critical.id()));

    service.complete_task(critical.id()).expect("complete");
    let aggregate = service
        .aggregated_surface_presentation(SURFACE)
        .expect("surface");
    assert_eq!(aggregate.lead_task.as_ref(), Some(first.id()));
    assert_ne!(aggregate.lead_task.as_ref(), Some(second.id()));
}

#[test]
fn weighted_determinate_sum_uses_integer_units() {
    let service = service(AggregateMode::WeightedDeterminateSum);
    let first = service
        .register_task(
            key("first"),
            TaskPriority::Normal,
            surface_scope(),
            TaskProgressState::determinate(50, total(100)).expect("first"),
        )
        .expect("first");
    let second = service
        .register_task(
            key("second"),
            TaskPriority::Normal,
            surface_scope(),
            TaskProgressState::determinate(50, total(100)).expect("second"),
        )
        .expect("second");
    let aggregate = service
        .aggregated_surface_presentation(SURFACE)
        .expect("surface");
    assert_eq!(aggregate.lead_task.as_ref(), Some(first.id()));
    match aggregate.progress.expect("progress") {
        TaskProgressState::Determinate {
            completed_units,
            total_units,
            per_mille,
        } => {
            assert_eq!(completed_units, 100);
            assert_eq!(total_units.get(), 200);
            assert_eq!(per_mille, 500);
        }
        other => panic!("expected determinate, got {other:?}"),
    }

    service
        .update_task(
            second.id(),
            TaskProgressState::determinate(100, total(100)).expect("full"),
        )
        .expect("update");
    match service
        .aggregated_surface_presentation(SURFACE)
        .expect("surface")
        .progress
        .expect("progress")
    {
        TaskProgressState::Determinate {
            completed_units,
            total_units,
            per_mille,
        } => {
            assert_eq!(completed_units, 150);
            assert_eq!(total_units.get(), 200);
            assert_eq!(per_mille, 750);
        }
        other => panic!("expected determinate, got {other:?}"),
    }
}

#[test]
fn weighted_mode_keeps_error_and_paused_over_determinate() {
    let service = service(AggregateMode::WeightedDeterminateSum);
    let _determinate = service
        .register_task(
            key("determinate"),
            TaskPriority::Normal,
            surface_scope(),
            TaskProgressState::determinate(50, total(100)).expect("determinate"),
        )
        .expect("determinate");
    let error = service
        .register_task(
            key("error"),
            TaskPriority::Normal,
            surface_scope(),
            TaskProgressState::error(Some(250), Some("boom")).expect("error"),
        )
        .expect("error");
    assert_eq!(
        service
            .aggregated_surface_presentation(SURFACE)
            .expect("surface")
            .progress
            .as_ref()
            .map(TaskProgressState::kind),
        Some(gpui_task_presentation::TaskStateKind::Error)
    );
    service.complete_task(error.id()).expect("complete error");
    let _paused = service
        .register_task(
            key("paused"),
            TaskPriority::Normal,
            surface_scope(),
            TaskProgressState::paused(Some(700)).expect("paused"),
        )
        .expect("paused");
    assert_eq!(
        service
            .aggregated_surface_presentation(SURFACE)
            .expect("surface")
            .progress
            .as_ref()
            .map(TaskProgressState::kind),
        Some(gpui_task_presentation::TaskStateKind::Paused)
    );
}

#[test]
fn application_and_surface_tasks_contribute_deterministically() {
    let service = service(AggregateMode::HighestPriorityLead);
    let app = service
        .register_task(
            key("app"),
            TaskPriority::Normal,
            TaskScope::Application,
            TaskProgressState::determinate(25, total(100)).expect("app"),
        )
        .expect("app");
    let surface_task = service
        .register_task(
            key("surface"),
            TaskPriority::High,
            surface_scope(),
            TaskProgressState::determinate(75, total(100)).expect("surface"),
        )
        .expect("surface");

    let aggregate = service
        .aggregated_surface_presentation(SURFACE)
        .expect("surface");
    assert_eq!(aggregate.lead_task.as_ref(), Some(surface_task.id()));
    assert_eq!(aggregate.active_task_count, 2);

    let app_aggregate = service.aggregated_app_presentation();
    assert_eq!(app_aggregate.lead_task.as_ref(), Some(surface_task.id()));
    assert_eq!(app_aggregate.surfaces.len(), 1);
    assert_eq!(app_aggregate.surfaces[0].surface, SURFACE);
    assert_eq!(
        app_aggregate.surfaces[0].lead_task.as_ref(),
        Some(surface_task.id())
    );
    assert_eq!(app_aggregate.active_task_count, 2);
    assert!(service.is_task_active(app.id()));
    assert!(service.is_task_active(surface_task.id()));
}

#[test]
fn dropping_the_lead_lease_falls_back_and_clears_terminal_state() {
    let service = service(AggregateMode::HighestPriorityLead);
    let low = service
        .register_task(
            key("low"),
            TaskPriority::Background,
            surface_scope(),
            TaskProgressState::determinate(10, total(100)).expect("low"),
        )
        .expect("low");
    let high = service
        .register_task(
            key("high"),
            TaskPriority::Critical,
            surface_scope(),
            TaskProgressState::determinate(90, total(100)).expect("high"),
        )
        .expect("high");
    assert_eq!(
        service
            .aggregated_surface_presentation(SURFACE)
            .expect("surface")
            .lead_task
            .as_ref(),
        Some(high.id())
    );
    drop(high);
    let aggregate = service
        .aggregated_surface_presentation(SURFACE)
        .expect("surface");
    assert_eq!(aggregate.lead_task.as_ref(), Some(low.id()));
    assert_eq!(aggregate.active_task_count, 1);
    drop(low);
    let aggregate = service
        .aggregated_surface_presentation(SURFACE)
        .expect("surface");
    assert!(aggregate.progress.is_none());
    assert_eq!(aggregate.active_task_count, 0);
}
