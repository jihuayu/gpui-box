//! Integration coverage for the optional GPUI `App` global adapter.
//!
//! The whole test crate is excluded when the `gpui` feature is disabled or the
//! host is not one of the desktop targets whose GPUI test-support graph applies.

#![cfg(all(
    feature = "gpui",
    any(target_os = "windows", target_os = "macos", target_os = "linux")
))]

use gpui::TestAppContext;
use gpui_diagnostics::gpui_adapter::{
    diagnostics_service, install_diagnostics_in_services, install_diagnostics_service,
};
use gpui_diagnostics::{
    DiagnosticDomain, DiagnosticEvent, DiagnosticSeverity, DiagnosticsOperation,
    DiagnosticsService, FakeDiagnosticsProvider, OperationContext, SystemError, SystemServices,
};
use std::{future::Future, sync::Arc};

#[gpui::test]
fn global_adapter_installs_resolves_and_records(cx: &TestAppContext) {
    cx.update(|cx| {
        let fake = FakeDiagnosticsProvider::new();
        let service = DiagnosticsService::new(Arc::new(fake.clone()));
        gpui_system::gpui_adapter::install_system_services(SystemServices::new(), cx);
        install_diagnostics_service(service.clone(), cx);

        let resolved = diagnostics_service(cx).expect("installed diagnostics global");
        assert_eq!(
            resolved.capabilities().len(),
            DiagnosticsOperation::ALL.len()
        );
        assert_eq!(resolved.revision(), 0);

        let subscription = resolved.subscribe();
        let outcome = match resolved.record_event(
            DiagnosticEvent::new(
                DiagnosticDomain::UiCommit,
                DiagnosticSeverity::Info,
                "global adapter event",
            ),
            &OperationContext::default(),
        ) {
            Ok(gpui_diagnostics::Completion::Completed(outcome)) => outcome,
            other => panic!("adapter event did not complete: {other:?}"),
        };
        assert!(outcome.stored);
        assert_eq!(fake.events().len(), 1);
        let mut next = subscription.try_next().expect("single reader");
        match std::pin::Pin::new(&mut next)
            .poll(&mut std::task::Context::from_waker(std::task::Waker::noop()))
        {
            std::task::Poll::Ready(Some(envelope)) => {
                assert!(matches!(
                    envelope.payload,
                    gpui_diagnostics::Event::Value(
                        gpui_diagnostics::DiagnosticServiceEvent::EventRecorded { .. }
                    )
                ));
            }
            other => panic!("adapter event stream did not resolve: {other:?}"),
        }

        let mut services = SystemServices::new();
        install_diagnostics_in_services(&mut services, service).expect("typed registry");
        assert!(services.service::<DiagnosticsService>().is_ok());
    });
}

#[gpui::test]
fn missing_global_reports_unavailable(cx: &TestAppContext) {
    cx.update(|cx| {
        let error: SystemError = diagnostics_service(cx).expect_err("not installed");
        assert_eq!(error.code, gpui_diagnostics::ErrorCode::Unavailable);
    });
}
