//! Deterministic bounded retention and eviction accounting.

mod support;

use gpui_diagnostics::{
    DiagnosticDomain, DiagnosticEvent, DiagnosticSeverity, DiagnosticsService, RetentionPolicy,
    TerminationKind,
};
use support::*;

#[test]
fn event_count_and_byte_budgets_evict_oldest_and_persist_counters() {
    let (_root, app) = app();
    let policy = RetentionPolicy::new(3, 1024 * 1024, 8);
    let store = gpui_diagnostics::DiagnosticStore::open_with_retention(&app, identity(), policy)
        .expect("store");
    let context = context();
    completed(store.recover(&context));
    for index in 0..5u64 {
        let event = DiagnosticEvent::new(
            DiagnosticDomain::Background,
            DiagnosticSeverity::Info,
            format!("retained-event-{index}"),
        )
        .with_field("index", index);
        completed(store.record_event(event, &context));
    }
    assert_eq!(store.retained_events().len(), 3);
    assert_eq!(store.evicted_events(), 2);
    completed(store.mark_clean_exit(&context));
    drop(store);

    let reopened = gpui_diagnostics::DiagnosticStore::open_with_retention(&app, identity(), policy)
        .expect("reopened store");
    let report = completed(reopened.recover(&context));
    assert_eq!(report.retained_events, 3);
    assert_eq!(report.evicted_events, 2);
    assert_eq!(
        reopened
            .retained_events()
            .iter()
            .filter_map(|event| event.message.as_text().map(str::to_owned))
            .collect::<Vec<_>>(),
        vec!["retained-event-2", "retained-event-3", "retained-event-4"]
    );
}

#[test]
fn serialized_byte_budget_bounds_retained_events() {
    let (_root, app) = app();
    let policy = RetentionPolicy::new(100, 512, 8);
    let store = gpui_diagnostics::DiagnosticStore::open_with_retention(&app, identity(), policy)
        .expect("store");
    let context = context();
    completed(store.recover(&context));
    for index in 0..20u64 {
        let event = DiagnosticEvent::new(
            DiagnosticDomain::Renderer,
            DiagnosticSeverity::Debug,
            "x".repeat(120),
        )
        .with_field("index", index)
        .with_text("detail", "y".repeat(120));
        completed(store.record_event(event, &context));
    }
    let retained = store.retained_events();
    let total: usize = retained
        .iter()
        .map(|event| serde_json::to_vec(event).expect("event encoding").len())
        .sum();
    assert!(total <= 512, "byte budget exceeded: {total}");
    assert!(store.evicted_events() > 0);
    assert!(retained.len() < 20);
}

#[test]
fn a_single_oversized_event_is_evicted_without_panicking() {
    let (_root, app) = app();
    let policy = RetentionPolicy::new(4, 64, 4);
    let store = gpui_diagnostics::DiagnosticStore::open_with_retention(&app, identity(), policy)
        .expect("store");
    let context = context();
    completed(store.recover(&context));
    let oversized = DiagnosticEvent::new(
        DiagnosticDomain::Renderer,
        DiagnosticSeverity::Error,
        "z".repeat(20_000),
    )
    .with_text("detail", "q".repeat(20_000));
    let outcome = completed(store.record_event(oversized, &context));
    assert!(!outcome.stored);
    assert_eq!(store.retained_events().len(), 0);
    assert!(store.evicted_events() >= 1);
    // The store remains usable and the oversized event did not corrupt the log.
    let follow_up = DiagnosticEvent::new(
        DiagnosticDomain::Background,
        DiagnosticSeverity::Info,
        "small",
    );
    completed(store.record_event(follow_up, &context));
    assert!(store.evicted_events() >= 1);
}

#[test]
fn crash_record_retention_is_bounded_and_deterministic() {
    let (_root, app) = app();
    let policy = RetentionPolicy::new(100, 1024 * 1024, 2);
    let context = context();
    let terminations = [
        TerminationKind::rust_panic(
            "panic at /home/alice/private/CANARY_PANIC_MESSAGE",
            Some("/home/alice/CANARY_PANIC_FRAME.rs:10".to_owned()),
            vec!["/home/alice/CANARY_PANIC_FRAME".to_owned()],
        ),
        TerminationKind::android_anr(
            "ANR on /data/user/0/app/CANARY_ANR_REASON",
            vec!["/data/user/0/app/CANARY_ANR_FRAME".to_owned()],
        ),
        TerminationKind::native_crash(
            Some(11),
            Some("SIGSEGV".to_owned()),
            vec!["/opt/app/CANARY_CRASH_DETAIL".to_owned()],
        ),
    ];
    for termination in terminations.iter().cloned() {
        let store =
            gpui_diagnostics::DiagnosticStore::open_with_retention(&app, identity(), policy)
                .expect("store");
        completed(store.recover(&context));
        completed(store.write_crash_marker(termination, &context));
    }

    let final_store =
        gpui_diagnostics::DiagnosticStore::open_with_retention(&app, identity(), policy)
            .expect("final store");
    let report = completed(final_store.recover(&context));
    assert_eq!(report.retained_crash_records, 2);
    assert_eq!(report.evicted_crash_records, 1);
    assert_eq!(report.recovered_crash_records.len(), 1);
    let retained = final_store.retained_crash_records();
    assert_eq!(
        retained
            .iter()
            .map(|record| record.label())
            .collect::<Vec<_>>(),
        vec!["android_anr", "native_crash"]
    );
    for record in &retained {
        let serialized = serde_json::to_string(record).expect("crash record JSON");
        assert_no_canaries(&serialized, "retained crash record JSON");
        assert_no_canaries(&format!("{record:?}"), "CrashRecord Debug");
    }
}

#[test]
fn lifecycle_kinds_cannot_be_written_as_crash_markers() {
    let (_root, app) = app();
    let store = gpui_diagnostics::DiagnosticStore::open(&app, identity()).expect("store");
    let context = context();
    completed(store.recover(&context));
    assert_code(
        store.write_crash_marker(TerminationKind::CleanExit, &context),
        gpui_diagnostics::ErrorCode::InvalidInput,
    );
    assert_code(
        store.write_crash_marker(TerminationKind::unclean_shutdown(1, identity()), &context),
        gpui_diagnostics::ErrorCode::InvalidInput,
    );
    assert!(store.retained_crash_records().is_empty());
}

#[test]
fn service_capability_gate_prevents_side_effects() {
    let fake = gpui_diagnostics::FakeDiagnosticsProvider::new();
    fake.set_operation_support(
        gpui_diagnostics::DiagnosticsOperation::RecordEvent,
        gpui_diagnostics::Support::Unsupported,
    );
    let service = DiagnosticsService::from_fake(fake.clone());
    let context = context();
    let event = DiagnosticEvent::new(
        DiagnosticDomain::Command,
        DiagnosticSeverity::Info,
        "must not reach the provider",
    );
    assert_code(
        service.record_event(event, &context),
        gpui_diagnostics::ErrorCode::Unsupported,
    );
    assert!(fake.events().is_empty());
    fake.clear_capability_overrides();
    completed(service.record_event(
        DiagnosticEvent::new(
            DiagnosticDomain::Command,
            DiagnosticSeverity::Info,
            "now accepted",
        ),
        &context,
    ));
    assert_eq!(fake.events().len(), 1);
}
