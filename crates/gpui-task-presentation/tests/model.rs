//! Validated model, integer progress math and support-matrix invariants.

use gpui_task_presentation::{
    BadgeValue, ErrorCode, PlatformTarget, TaskActionId, TaskKey, TaskPresentationId,
    TaskPresentationOperation, TaskPresentationService, TaskPriority, TaskProgressState, TaskScope,
    platform_operations, platform_support_matrix,
};
use serde_json::json;
use std::num::NonZeroU64;

fn total(value: u64) -> NonZeroU64 {
    NonZeroU64::new(value).expect("non-zero")
}

#[test]
fn task_identifier_is_bounded_ascii_and_serde_validated() {
    let key = TaskKey::new("build:worker-1.alpha_beta").expect("valid key");
    assert_eq!(key.as_str(), "build:worker-1.alpha_beta");
    assert_eq!(
        TaskPresentationId::new("same").expect("alias").as_str(),
        "same"
    );

    for invalid in ["", "space key", "slash/key", "unicodé", "line\nbreak"] {
        let error = TaskKey::new(invalid).expect_err("invalid key");
        assert_eq!(error.code, ErrorCode::InvalidInput);
    }
    let too_long = "a".repeat(129);
    assert_eq!(
        TaskKey::new(too_long).expect_err("too long").code,
        ErrorCode::InvalidInput
    );
    assert!(TaskKey::new("a".repeat(128)).is_ok());
    assert!(!TaskKey::is_valid("not valid"));

    let parsed: TaskKey = serde_json::from_str("\"ok.key\"").expect("serde key");
    assert_eq!(parsed.as_str(), "ok.key");
    assert!(serde_json::from_str::<TaskKey>("\"bad key\"").is_err());
    assert_eq!(
        serde_json::to_string(&parsed).expect("serialize"),
        "\"ok.key\""
    );

    assert!(TaskActionId::new("retry").is_ok());
    assert!(TaskActionId::new("bad action").is_err());
    assert!(TaskActionId::new("a".repeat(65)).is_err());
}

#[test]
fn determinate_progress_uses_integer_per_mille_math() {
    let zero = TaskProgressState::determinate(0, total(100)).expect("zero");
    assert_eq!(
        zero.kind(),
        gpui_task_presentation::TaskStateKind::Determinate
    );
    assert_eq!(zero.per_mille(), Some(0));
    assert!(!zero.is_terminal());

    let half = TaskProgressState::determinate(50, total(100)).expect("half");
    assert_eq!(half.per_mille(), Some(500));

    let third = TaskProgressState::determinate(1, total(3)).expect("third");
    assert_eq!(third.per_mille(), Some(333));

    let full = TaskProgressState::determinate(u64::MAX, total(u64::MAX)).expect("max");
    assert_eq!(full.per_mille(), Some(1000));
    assert_eq!(
        TaskProgressState::determinate(u64::MAX - 1, total(u64::MAX))
            .expect("near max")
            .per_mille(),
        Some(999)
    );

    let error = TaskProgressState::determinate(101, total(100)).expect_err("over complete");
    assert_eq!(error.code, ErrorCode::InvalidInput);

    let clamped = TaskProgressState::determinate_clamped(200, total(100));
    assert_eq!(clamped.per_mille(), Some(1000));
    if let TaskProgressState::Determinate {
        completed_units,
        total_units,
        ..
    } = clamped
    {
        assert_eq!(completed_units, 100);
        assert_eq!(total_units.get(), 100);
    } else {
        panic!("expected determinate");
    }
}

#[test]
fn progress_serde_rejects_inconsistent_values() {
    let valid = TaskProgressState::determinate(1, total(4)).expect("valid");
    let encoded = serde_json::to_value(&valid).expect("encode");
    assert_eq!(
        serde_json::from_value::<TaskProgressState>(encoded.clone()).expect("round trip"),
        valid
    );

    let mut inconsistent = encoded.clone();
    inconsistent["per_mille"] = json!(999);
    assert!(serde_json::from_value::<TaskProgressState>(inconsistent).is_err());

    let mut out_of_range = encoded.clone();
    out_of_range["per_mille"] = json!(1001);
    assert!(serde_json::from_value::<TaskProgressState>(out_of_range).is_err());

    let mut over_complete = encoded;
    over_complete["completed_units"] = json!(5);
    assert!(serde_json::from_value::<TaskProgressState>(over_complete).is_err());

    let paused = TaskProgressState::paused(Some(250)).expect("paused");
    assert_eq!(paused.per_mille(), Some(250));
    assert!(TaskProgressState::paused(Some(1001)).is_err());
    assert!(
        serde_json::from_value::<TaskProgressState>(json!({
            "kind": "paused", "per_mille": 1001
        }))
        .is_err()
    );

    let error =
        TaskProgressState::error(Some(400), Some("  network unreachable  ")).expect("error");
    assert_eq!(error.per_mille(), Some(400));
    let debug = format!("{error:?}");
    assert!(debug.contains("[redacted]"));
    assert!(!debug.contains("network unreachable"));
    assert!(TaskProgressState::error(None::<u16>, Some("line\nbreak")).is_err());
    assert!(
        serde_json::from_value::<TaskProgressState>(json!({
            "kind": "error", "per_mille": 0, "message": " trailing"
        }))
        .is_err()
    );

    assert!(TaskProgressState::Completed.is_terminal());
    assert!(TaskProgressState::Cancelled.is_terminal());
}

#[test]
fn badge_short_status_is_bounded_and_serde_validated() {
    let badge = BadgeValue::short_status("✓").expect("glyph");
    assert_eq!(badge, BadgeValue::ShortStatus("✓".to_owned()));
    assert!(BadgeValue::short_status("12345678").is_ok());
    assert!(BadgeValue::short_status("123456789").is_err());
    assert!(BadgeValue::short_status("").is_err());
    assert!(BadgeValue::short_status(" badge ").is_err());
    assert!(BadgeValue::short_status("bad\nbadge").is_err());

    let encoded = serde_json::to_value(badge).expect("encode");
    assert_eq!(
        serde_json::from_value::<BadgeValue>(encoded).expect("round trip"),
        BadgeValue::ShortStatus("✓".to_owned())
    );
    assert!(
        serde_json::from_value::<BadgeValue>(json!({
            "kind": "short_status", "value": "123456789"
        }))
        .is_err()
    );
    assert!(
        serde_json::from_value::<BadgeValue>(json!({
            "kind": "short_status", "value": " "
        }))
        .is_err()
    );

    assert_eq!(
        BadgeValue::Count(4).downgraded_to_dot(),
        Some(BadgeValue::Dot)
    );
    assert_eq!(
        BadgeValue::Count(0).downgraded_to_dot(),
        Some(BadgeValue::Clear)
    );
    assert_eq!(BadgeValue::Dot.downgraded_to_dot(), None);
    assert!(BadgeValue::Count(4).requires_dot_surface());
}

#[test]
fn support_matrix_has_thirty_cells_and_operation_invariants() {
    let rows = platform_support_matrix();
    assert_eq!(rows.len(), 30);
    assert_eq!(TaskPresentationOperation::ALL.len(), 5);
    assert_eq!(PlatformTarget::ALL.len(), 6);
    for platform in PlatformTarget::ALL {
        assert_eq!(platform.operations().len(), 5);
        assert_eq!(platform_operations(platform).len(), 5);
        for operation in TaskPresentationOperation::ALL {
            let matches = rows
                .iter()
                .filter(|row| row.platform == platform && row.operation == operation)
                .count();
            assert_eq!(matches, 1, "{platform:?}/{operation:?}");
        }
    }
    for row in &rows {
        assert_eq!(row.evidence, gpui_system::Evidence::ContractOnly);
        assert!(!row.required_host_configuration.is_empty());
        assert!(!row.notes.is_empty());
        assert!(!row.qualification_links.is_empty());
        assert_eq!(
            row.is_supported(),
            !matches!(row.support, gpui_system::Support::Unsupported)
        );
    }
    assert_eq!(
        gpui_task_presentation::operation_support(
            PlatformTarget::Windows,
            TaskPresentationOperation::PresentTaskProgress
        )
        .support,
        gpui_system::Support::Native
    );
    assert_eq!(
        gpui_task_presentation::operation_support(
            PlatformTarget::Ios,
            TaskPresentationOperation::PresentTaskProgress
        )
        .support,
        gpui_system::Support::Unsupported
    );
    assert!(
        gpui_task_presentation::operation_support(
            PlatformTarget::Windows,
            TaskPresentationOperation::PresentAppBadge
        )
        .badge_downgrades_to_dot
    );
}

#[test]
fn service_exposes_matrix_and_coordinated_versions() {
    let fake = gpui_task_presentation::FakeTaskPresentationProvider::new();
    let service = TaskPresentationService::from_fake(fake);
    let task = service
        .register_task(
            TaskKey::new("model.task").expect("key"),
            TaskPriority::Normal,
            TaskScope::Application,
            TaskProgressState::Indeterminate,
        )
        .expect("register");
    assert!(service.is_task_active(task.id()));
    assert_eq!(service.active_task_count(), 1);
    assert_eq!(
        gpui_task_presentation::UPSTREAM_GPUI_REVISION,
        "f8cfc7ed9e57503def9479138894f3139f5400cc"
    );
    assert_eq!(
        gpui_task_presentation::DEFAULT_TASK_PRESENTATION_EVENT_CAPACITY,
        64
    );
    assert_eq!(
        gpui_task_presentation::PACKAGE_VERSION,
        env!("CARGO_PKG_VERSION")
    );
}
