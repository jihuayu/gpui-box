//! Crash/termination markers, relaunch recovery, quarantine, fault safety and
//! capability/context ordering.

mod support;

use gpui_diagnostics::{
    CancelReason, CrashRecordSource, DiagnosticDomain, DiagnosticEvent, DiagnosticServiceEvent,
    DiagnosticSeverity, DiagnosticsOperation, DiagnosticsProvider, DiagnosticsService, ErrorCode,
    Event, FakeDiagnosticsProvider, OperationContext, PlatformTarget, StorageFaultInjection,
    SurfaceId, TerminationKind, UiOwner,
};
use std::{
    fs,
    sync::Arc,
    time::{Duration, Instant},
};
use support::*;

fn panic_termination() -> TerminationKind {
    TerminationKind::rust_panic(
        "panic at /home/alice/private/CANARY_PANIC_MESSAGE",
        Some("/home/alice/CANARY_PANIC_FRAME.rs:91".to_owned()),
        vec!["/home/alice/CANARY_PANIC_FRAME".to_owned()],
    )
}

fn anr_termination() -> TerminationKind {
    TerminationKind::android_anr(
        "ANR on /data/user/0/app/CANARY_ANR_REASON",
        vec!["/data/user/0/app/CANARY_ANR_FRAME".to_owned()],
    )
}

fn native_termination() -> TerminationKind {
    TerminationKind::native_crash(
        Some(11),
        Some("SIGSEGV".to_owned()),
        vec!["/opt/app/CANARY_CRASH_DETAIL".to_owned()],
    )
}

fn os_termination() -> TerminationKind {
    TerminationKind::os_termination("terminated by /var/tmp/CANARY_OS_REASON", Some(9))
}

fn assert_termination_sanitized(termination: &TerminationKind) {
    let serialized = serde_json::to_string(termination).expect("termination JSON");
    assert_no_canaries(&serialized, "termination JSON");
    assert_no_canaries(&format!("{termination:?}"), "TerminationKind Debug");
}

#[test]
fn every_crash_marker_kind_is_promoted_with_its_exact_evidence() {
    for termination in [
        panic_termination(),
        anr_termination(),
        native_termination(),
        os_termination(),
    ] {
        let (_root, app) = app();
        let build = identity();
        let context = context();
        let first = gpui_diagnostics::DiagnosticStore::open(&app, build.clone()).expect("store");
        let initial = completed(first.recover(&context));
        let prior_session = initial.current_session_id;
        completed(first.write_crash_marker(termination.clone(), &context));
        drop(first);

        let second = gpui_diagnostics::DiagnosticStore::open(&app, build.clone()).expect("store");
        let report = completed(second.recover(&context));
        assert_eq!(report.recovered_crash_records.len(), 1);
        let record = &report.recovered_crash_records[0];
        assert_eq!(record.session_id, prior_session);
        assert_eq!(record.termination, termination);
        assert_eq!(record.source, CrashRecordSource::CrashMarker);
        assert_eq!(record.build_identity, build);
        let prior = report.prior_session.as_ref().expect("prior session");
        assert!(!prior.clean);
        assert!(prior.crash_marker_promoted);
        assert_eq!(prior.session_id, prior_session);
        assert_eq!(prior.termination, termination);
        assert_eq!(report.retained_crash_records, 1);
        assert_termination_sanitized(&record.termination);
        assert_no_canaries(
            &serde_json::to_string(record).expect("crash record JSON"),
            "CrashRecord JSON",
        );
        assert_no_canaries(&format!("{record:?}"), "CrashRecord Debug");
        assert_eq!(second.retained_crash_records().len(), 1);

        completed(second.mark_clean_exit(&context));
        drop(second);

        let third = gpui_diagnostics::DiagnosticStore::open(&app, build).expect("store");
        let clean_report = completed(third.recover(&context));
        let clean_prior = clean_report.prior_session.as_ref().expect("clean prior");
        assert!(clean_prior.clean);
        assert_eq!(clean_prior.termination, TerminationKind::CleanExit);
        assert!(clean_report.recovered_crash_records.is_empty());
        assert_eq!(clean_report.retained_crash_records, 1);
    }
}

#[test]
fn unclosed_session_synthesizes_unclean_shutdown_and_clean_exit_does_not() {
    let (_root, app) = app();
    let build = identity();
    let context = context();
    let first = gpui_diagnostics::DiagnosticStore::open(&app, build.clone()).expect("store");
    let initial = completed(first.recover(&context));
    let prior_session = initial.current_session_id;
    completed(first.record_event(
        DiagnosticEvent::new(
            DiagnosticDomain::UiCommit,
            DiagnosticSeverity::Info,
            "before unclean exit",
        ),
        &context,
    ));
    drop(first);

    let second = gpui_diagnostics::DiagnosticStore::open(&app, build.clone()).expect("store");
    let report = completed(second.recover(&context));
    assert_eq!(report.recovered_crash_records.len(), 1);
    let record = &report.recovered_crash_records[0];
    assert_eq!(record.session_id, prior_session);
    assert_eq!(record.source, CrashRecordSource::UncleanShutdown);
    match &record.termination {
        TerminationKind::UncleanShutdown(evidence) => {
            assert_eq!(evidence.prior_session_id, prior_session);
            assert_eq!(evidence.build_identity, build);
        }
        other => panic!("expected UncleanShutdown, got {other:?}"),
    }
    assert_eq!(report.retained_events, 1);
    assert_eq!(report.evicted_events, 0);
    let prior = report.prior_session.as_ref().expect("prior");
    assert!(!prior.clean);
    assert!(!prior.crash_marker_promoted);
    completed(second.mark_clean_exit(&context));
    drop(second);

    let third = gpui_diagnostics::DiagnosticStore::open(&app, build).expect("store");
    let clean = completed(third.recover(&context));
    assert!(clean.prior_session.as_ref().expect("clean prior").clean);
    assert!(clean.recovered_crash_records.is_empty());
}

#[test]
fn corrupt_files_are_quarantined_intact_and_reported() {
    let (_root, app) = app();
    let context = context();
    let first = gpui_diagnostics::DiagnosticStore::open(&app, identity()).expect("store");
    completed(first.recover(&context));
    completed(first.record_event(
        DiagnosticEvent::new(
            DiagnosticDomain::Background,
            DiagnosticSeverity::Warning,
            "retained before corruption",
        ),
        &context,
    ));
    drop(first);

    let directory = diagnostics_dir(&app);
    let corrupt = b"CORRUPT_FORENSIC_CANARY { this is not JSON".to_vec();
    for name in [
        "events.json",
        "crashes.json",
        "session.json",
        "crash-marker.json",
    ] {
        fs::write(directory.join(name), &corrupt).expect("corrupt record");
    }

    let second = gpui_diagnostics::DiagnosticStore::open(&app, identity()).expect("store");
    let report = completed(second.recover(&context));
    assert_eq!(report.quarantined_count, 4);
    assert_eq!(report.quarantined_entries.len(), 4);
    assert!(report.recovered_crash_records.is_empty());
    let quarantine = directory.join("quarantine");
    let quarantined = regular_files(&quarantine);
    assert_eq!(quarantined.len(), 4);
    for entry in &quarantined {
        let bytes = fs::read(entry).expect("quarantined bytes");
        assert_eq!(bytes, corrupt);
    }
    for name in [
        "events.json",
        "crashes.json",
        "session.json",
        "crash-marker.json",
    ] {
        assert!(
            report
                .quarantined_entries
                .iter()
                .any(|entry| entry.original_name == name),
            "missing quarantine report for {name}"
        );
    }
    // Corruption does not poison the new session.
    completed(second.record_event(
        DiagnosticEvent::new(
            DiagnosticDomain::Background,
            DiagnosticSeverity::Info,
            "after quarantine",
        ),
        &context,
    ));
    assert_eq!(second.retained_events().len(), 1);
    assert!(!report.unavailable_evidence.is_empty());
}

#[test]
fn simulated_disk_full_preserves_previously_committed_records() {
    let (_root, app) = app();
    let fault = Arc::new(StorageFaultInjection::new());
    let policy = gpui_diagnostics::RetentionPolicy::default();
    let store = gpui_diagnostics::DiagnosticStore::open_with_options(
        &app,
        identity(),
        policy,
        fault.clone(),
    )
    .expect("store");
    let context = context();
    completed(store.recover(&context));
    for index in 0..2u64 {
        completed(
            store.record_event(
                DiagnosticEvent::new(
                    DiagnosticDomain::Command,
                    DiagnosticSeverity::Info,
                    format!("committed-{index}"),
                )
                .with_field("index", index),
                &context,
            ),
        );
    }
    let events_path = diagnostics_dir(&app).join("events.json");
    let committed = fs::read(&events_path).expect("committed events");

    fault.fail_next_write();
    assert_code(
        store.record_event(
            DiagnosticEvent::new(
                DiagnosticDomain::Command,
                DiagnosticSeverity::Error,
                "write failure",
            ),
            &context,
        ),
        ErrorCode::PlatformFailure,
    );
    assert_eq!(
        fs::read(&events_path).expect("events after write failure"),
        committed
    );

    fault.fail_next_rename();
    assert_code(
        store.record_event(
            DiagnosticEvent::new(
                DiagnosticDomain::Command,
                DiagnosticSeverity::Error,
                "rename failure",
            ),
            &context,
        ),
        ErrorCode::PlatformFailure,
    );
    assert_eq!(
        fs::read(&events_path).expect("events after rename failure"),
        committed
    );
    drop(store);

    let reopened = gpui_diagnostics::DiagnosticStore::open(&app, identity()).expect("reopen");
    let report = completed(reopened.recover(&context));
    assert_eq!(report.retained_events, 2);
    assert_eq!(report.quarantined_count, 0);
    assert_eq!(reopened.retained_events().len(), 2);
}

#[test]
fn disk_full_during_recovery_is_retryable() {
    let (_root, app) = app();
    let fault = Arc::new(StorageFaultInjection::new());
    let store = gpui_diagnostics::DiagnosticStore::open_with_options(
        &app,
        identity(),
        gpui_diagnostics::RetentionPolicy::default(),
        fault.clone(),
    )
    .expect("store");
    let context = context();
    fault.fail_next_rename();
    assert_code(store.recover(&context), ErrorCode::PlatformFailure);
    assert!(!store.is_recovered());
    fault.clear();
    let report = completed(store.recover(&context));
    assert!(report.current_session_id != 0);
    assert!(store.is_recovered());
    assert_eq!(report.quarantined_count, 0);
}

#[test]
fn ui_deadline_and_cancellation_are_checked_before_side_effects() {
    let (_root, app) = app();
    let store = gpui_diagnostics::DiagnosticStore::open(&app, identity()).expect("store");
    let owner = UiOwner::new(SurfaceId(7));
    let ui = OperationContext {
        ui: Some(owner.context(None).expect("UI context")),
        ..Default::default()
    };
    assert_code(store.recover(&ui), ErrorCode::InvalidInput);
    assert!(!store.is_recovered());
    assert!(!diagnostics_dir(&app).exists());

    let expired = OperationContext {
        deadline: Some(Instant::now() - Duration::from_secs(1)),
        ..Default::default()
    };
    assert_code(store.recover(&expired), ErrorCode::TimedOut);
    assert!(!store.is_recovered());

    let cancelled_context = OperationContext::default();
    cancelled_context.cancellation.cancel(CancelReason::Caller);
    assert_eq!(
        cancelled(store.recover(&cancelled_context)),
        CancelReason::Caller
    );
    assert!(!store.is_recovered());
    assert!(!diagnostics_dir(&app).exists());

    // The service applies the same order and rejects UI contexts first.
    let service = DiagnosticsService::in_process(&app, identity()).expect("service");
    assert_code(service.recover(&ui), ErrorCode::InvalidInput);
    assert_code(
        service.record_event(
            DiagnosticEvent::new(
                DiagnosticDomain::Command,
                DiagnosticSeverity::Info,
                "ui must not write",
            ),
            &ui,
        ),
        ErrorCode::InvalidInput,
    );
    assert!(!service.store().expect("store").is_recovered());
}

#[test]
fn cancelled_export_never_leaves_a_staged_file() {
    let (_root, app) = app();
    let context = context();

    let before = FakeDiagnosticsProvider::new();
    let service = DiagnosticsService::from_fake(before.clone());
    let cancelled_before = OperationContext::default();
    cancelled_before.cancellation.cancel(CancelReason::User);
    assert_eq!(
        cancelled(service.export_bundle(&app, &cancelled_before)),
        CancelReason::User
    );
    assert!(export_files(&app).is_empty());

    let during = FakeDiagnosticsProvider::new();
    during.set_cancel_during_export(true);
    let service = DiagnosticsService::from_fake(during.clone());
    assert_eq!(
        cancelled(service.export_bundle(&app, &context)),
        CancelReason::Caller
    );
    assert!(export_files(&app).is_empty());

    let after = FakeDiagnosticsProvider::new();
    after.set_cancel_after_bundle(true);
    let service = DiagnosticsService::from_fake(after.clone());
    assert_eq!(
        cancelled(service.export_bundle(&app, &context)),
        CancelReason::Caller
    );
    assert!(export_files(&app).is_empty());
    assert!(after.events().is_empty());
}

#[test]
fn export_capability_and_disk_full_fail_before_staging() {
    let (_root, app) = app();
    let context = context();
    let fake = FakeDiagnosticsProvider::new();
    fake.set_operation_support(
        DiagnosticsOperation::ExportBundle,
        gpui_diagnostics::Support::Unsupported,
    );
    let service = DiagnosticsService::from_fake(fake);
    assert_code(
        service.export_bundle(&app, &context),
        ErrorCode::Unsupported,
    );
    assert!(export_files(&app).is_empty());

    let fake = FakeDiagnosticsProvider::new();
    fake.set_disk_full(DiagnosticsOperation::ExportBundle);
    let service = DiagnosticsService::from_fake(fake);
    assert_code(
        service.export_bundle(&app, &context),
        ErrorCode::PlatformFailure,
    );
    assert!(export_files(&app).is_empty());
}

#[test]
fn fake_provider_supports_capability_and_failure_overrides() {
    let fake = FakeDiagnosticsProvider::new();
    let service = DiagnosticsService::from_fake(fake.clone());
    for operation in DiagnosticsOperation::ALL {
        let capability = service.capability(operation);
        assert_eq!(capability.operation, operation);
        assert_eq!(capability.support, gpui_diagnostics::Support::Native);
        assert_eq!(capability.evidence, gpui_diagnostics::Evidence::Compiled);
    }
    fake.set_operation_configuration(
        DiagnosticsOperation::WriteCrashMarker,
        gpui_diagnostics::Configuration::Required,
    );
    assert_code(
        service.write_crash_marker(panic_termination(), &context()),
        ErrorCode::InvalidConfiguration,
    );
    fake.clear_capability_overrides();
    fake.set_disk_full(DiagnosticsOperation::WriteCrashMarker);
    assert_code(
        service.write_crash_marker(panic_termination(), &context()),
        ErrorCode::PlatformFailure,
    );
    fake.clear_failures();
    completed(service.write_crash_marker(panic_termination(), &context()));
    assert_eq!(fake.crash_markers().len(), 1);
    assert_eq!(fake.crash_records().len(), 1);
    let report = completed(service.recover(&context()));
    assert_eq!(report.retained_crash_records, 1);
    assert!(report.unavailable_evidence.iter().any(|evidence| {
        evidence.platform == Some(PlatformTarget::Android)
            || evidence.platform == Some(PlatformTarget::Ios)
            || evidence.platform == Some(PlatformTarget::Web)
    }));
    let unavailable: Arc<dyn DiagnosticsProvider> = service.provider().clone();
    assert!(unavailable.as_any().is::<FakeDiagnosticsProvider>());
}

#[test]
fn service_is_clone_send_sync_and_registerable() {
    fn assert_send_sync<T: Send + Sync + 'static>() {}
    assert_send_sync::<DiagnosticsService>();
    let fake = FakeDiagnosticsProvider::new();
    let service = DiagnosticsService::from_fake(fake);
    let mut services = gpui_diagnostics::SystemServices::new();
    service.install_cloned(&mut services).expect("install");
    assert!(services.service::<DiagnosticsService>().is_ok());
    let cloned = service.clone();
    assert_eq!(cloned.revision(), 0);
}

#[test]
fn service_events_report_operation_failures_but_not_cancellation() {
    let (_root, app) = app();
    let fake = FakeDiagnosticsProvider::new();
    let service = DiagnosticsService::from_fake(fake.clone());
    let subscription = service.subscribe();
    let context = context();
    let event = || {
        DiagnosticEvent::new(
            DiagnosticDomain::Command,
            DiagnosticSeverity::Info,
            "event for failure observations",
        )
    };

    fake.set_operation_support(
        DiagnosticsOperation::RecordEvent,
        gpui_diagnostics::Support::Unsupported,
    );
    assert_code(
        service.record_event(event(), &context),
        ErrorCode::Unsupported,
    );
    match next_event(&subscription).payload {
        Event::Value(DiagnosticServiceEvent::OperationFailed { operation, code }) => {
            assert_eq!(operation, DiagnosticsOperation::RecordEvent);
            assert_eq!(code, ErrorCode::Unsupported);
        }
        other => panic!("unexpected capability failure event: {other:?}"),
    }

    fake.clear_capability_overrides();
    let owner = UiOwner::new(SurfaceId(9));
    let ui = OperationContext {
        ui: Some(owner.context(None).expect("UI context")),
        ..Default::default()
    };
    assert_code(service.record_event(event(), &ui), ErrorCode::InvalidInput);
    match next_event(&subscription).payload {
        Event::Value(DiagnosticServiceEvent::OperationFailed { operation, code }) => {
            assert_eq!(operation, DiagnosticsOperation::RecordEvent);
            assert_eq!(code, ErrorCode::InvalidInput);
        }
        other => panic!("unexpected UI gate failure event: {other:?}"),
    }

    fake.set_disk_full(DiagnosticsOperation::ExportBundle);
    assert_code(
        service.export_bundle(&app, &context),
        ErrorCode::PlatformFailure,
    );
    match next_event(&subscription).payload {
        Event::Value(DiagnosticServiceEvent::OperationFailed { operation, code }) => {
            assert_eq!(operation, DiagnosticsOperation::ExportBundle);
            assert_eq!(code, ErrorCode::PlatformFailure);
        }
        other => panic!("unexpected disk-full failure event: {other:?}"),
    }
    assert!(export_files(&app).is_empty());

    fake.clear_failures();
    let cancelled_context = OperationContext::default();
    cancelled_context.cancellation.cancel(CancelReason::Caller);
    assert_eq!(
        cancelled(service.record_event(event(), &cancelled_context)),
        CancelReason::Caller
    );
    assert_no_event(&subscription);
}
