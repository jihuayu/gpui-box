//! Correlated events across all six domains and strict redaction, including
//! `Debug` and serialized output.

mod support;

use gpui_diagnostics::{
    CorrelationContext, DiagnosticDomain, DiagnosticEvent, DiagnosticField, DiagnosticSeverity,
    DiagnosticsService, RedactedValue, SurfaceId, sensitive_key_kind,
};
use support::*;

fn event_for(domain: DiagnosticDomain) -> DiagnosticEvent {
    let correlation = CorrelationContext::new()
        .with_command_id(11)
        .with_request_id(22)
        .with_surface(SurfaceId(33))
        .with_surface_generation(4)
        .with_revision(55)
        .with_job_id(66);
    let message = format!(
        "domain {domain} Bearer CANARY_BEARER_VALUE \
         https://alice:CANARY_URL_PASSWORD@example.com/private?x=CANARY_URL_QUERY \
         https://example.com/callback?access_token=CANARY_QUERY_TOKEN&secret=CANARY_QUERY_SECRET \
         jwt eyJhbGciOiJIUzI1NiJ9.CANARY_JWT_PAYLOAD.CANARY_JWT_SIGNATURE \
         unix /home/alice/private/CANARY_UNIX_PATH.txt \
         var /var/tmp/CANARY_VAR_PATH/out.log \
         tmp /tmp/CANARY_TMP_PATH \
         windows C:\\Users\\alice\\CANARY_WIN_PATH.txt \
         drive D:/work/CANARY_DRIVE_PATH.json \
         unc \\\\server\\share\\CANARY_UNC_PATH.txt \
         extended \\\\?\\C:\\Users\\alice\\CANARY_EXTENDED_PATH.txt"
    );
    let mut event = DiagnosticEvent::new(domain, DiagnosticSeverity::Error, message)
        .with_correlation(correlation);
    for (key, value) in [
        ("token", "CANARY_TOKEN_VALUE"),
        ("client_secret", "CANARY_SECRET_VALUE"),
        ("password", "CANARY_PASSWORD_VALUE"),
        ("authorization", "Bearer CANARY_AUTH_VALUE"),
        ("cookie", "session=CANARY_COOKIE_VALUE"),
        ("credential", "CANARY_CREDENTIAL_VALUE"),
        ("private_key", "CANARY_PRIVATE_KEY_VALUE"),
        ("x-api-key", "CANARY_API_KEY_VALUE"),
        ("session_id", "CANARY_SESSION_VALUE"),
        ("file_path", "/home/alice/private/CANARY_UNIX_PATH.txt"),
        ("filepath", "C:\\Users\\alice\\CANARY_WIN_PATH.txt"),
    ] {
        assert!(
            sensitive_key_kind(key).is_some(),
            "{key} must be classified as sensitive"
        );
        event = event.with_text(key, value);
    }
    event
        .with_field(
            "nested",
            RedactedValue::object(vec![
                DiagnosticField::text("api_key", "CANARY_NESTED_API"),
                DiagnosticField::text("note", "Bearer CANARY_NESTED_BEARER"),
                DiagnosticField::text("location", "/home/alice/CANARY_NESTED_PATH/secret.txt"),
            ]),
        )
        .with_field(
            "list",
            RedactedValue::list(vec![
                RedactedValue::text("C:\\Users\\alice\\CANARY_LIST_PATH"),
                RedactedValue::text("Bearer CANARY_LIST_BEARER"),
            ]),
        )
}

#[test]
fn correlated_events_across_all_domains_are_redacted_in_storage_and_debug() {
    let (_root, app) = app();
    let service = DiagnosticsService::in_process(&app, identity()).expect("service");
    let context = context();
    let recovery = completed(service.recover(&context));
    assert_eq!(recovery.retained_events, 0);

    let subscription = service.subscribe();
    for (index, domain) in DiagnosticDomain::ALL.into_iter().enumerate() {
        let event = event_for(domain);
        let outcome = completed(service.record_event(event, &context));
        assert!(outcome.stored);
        assert_eq!(outcome.evicted_events, 0);

        let envelope = next_event(&subscription);
        match envelope.payload {
            gpui_diagnostics::Event::Value(
                gpui_diagnostics::DiagnosticServiceEvent::EventRecorded {
                    domain: emitted,
                    severity,
                    stored,
                },
            ) => {
                assert_eq!(emitted, domain);
                assert_eq!(severity, DiagnosticSeverity::Error);
                assert!(stored);
            }
            other => panic!("unexpected service event for {index}: {other:?}"),
        }
    }

    let store = service.store().expect("in-process store");
    let events = store.retained_events();
    assert_eq!(events.len(), DiagnosticDomain::ALL.len());
    for event in &events {
        assert_eq!(
            event.correlation,
            CorrelationContext::new()
                .with_command_id(11)
                .with_request_id(22)
                .with_surface(SurfaceId(33))
                .with_surface_generation(4)
                .with_revision(55)
                .with_job_id(66)
        );
        let serialized = serde_json::to_string(event).expect("serialize event");
        assert_no_canaries(&serialized, "retained event JSON");
        assert_no_canaries(&format!("{event:?}"), "DiagnosticEvent Debug");
        assert!(serialized.contains("[REDACTED:"));
        for field in &event.fields {
            assert_no_canaries(&format!("{field:?}"), "DiagnosticField Debug");
            assert_no_canaries(
                &serde_json::to_string(field).expect("serialize field"),
                "DiagnosticField JSON",
            );
        }
        if let Some(field) = event.field("token") {
            assert!(field.value().is_redacted_marker());
        }
        if let Some(field) = event.field("nested") {
            assert_no_canaries(&format!("{field:?}"), "nested field Debug");
        }
    }

    let stored = store.evicted_events();
    assert_eq!(stored, 0);
    let report = completed(service.inspect_recovery(&context));
    assert_eq!(report.retained_events, DiagnosticDomain::ALL.len());
    assert_eq!(report.evicted_events, 0);
}

#[test]
fn redacted_text_newtype_never_retains_raw_debug_payload() {
    let value = RedactedValue::text("Bearer CANARY_BEARER_VALUE");
    assert_no_canaries(&format!("{value:?}"), "RedactedValue Debug");
    assert_no_canaries(
        &serde_json::to_string(&value).expect("serialize value"),
        "RedactedValue JSON",
    );
    let field = DiagnosticField::new("authorization", "Bearer CANARY_AUTH_VALUE");
    assert_no_canaries(&format!("{field:?}"), "DiagnosticField Debug");
    assert!(field.redaction().is_some());
}
