//! Optional GPUI integration for [`DiagnosticsService`].
//!
//! Installation and lookup must run on the owning GPUI UI thread. Storage,
//! recovery and export are blocking background operations; the spawn helpers
//! move only owned `Send` data to GPUI's background executor.

use crate::{
    DiagnosticServiceEvent, DiagnosticsService, EventSubscription, OperationContext, ServiceResult,
    SystemError, SystemServices, UiOwner,
};
use gpui::{App, Context, Task};
use gpui_app_data::AppData;
use gpui_files::ExportResourceLease;
use gpui_system::ErrorCode;

impl gpui::Global for DiagnosticsService {}

/// Install the diagnostics service into GPUI's `App` global.
pub fn install_diagnostics_service(service: DiagnosticsService, cx: &mut App) {
    cx.set_global(service);
}

/// Access the installed service from GPUI's `App` global.
pub fn diagnostics_service(cx: &App) -> Result<DiagnosticsService, SystemError> {
    let missing = SystemError::new(
        ErrorCode::Unavailable,
        "DiagnosticsService is not installed",
    );
    cx.try_global::<DiagnosticsService>()
        .cloned()
        .ok_or(missing)
}

/// Register the service in the typed `SystemServices` registry instead.
pub fn install_diagnostics_in_services(
    services: &mut SystemServices,
    service: DiagnosticsService,
) -> Result<(), SystemError> {
    services.register(service)
}

fn background_spawn<T: Send + 'static>(
    cx: &App,
    work: impl FnOnce() -> T + Send + 'static,
) -> Task<T> {
    cx.background_executor().spawn(async move { work() })
}

/// Run one blocking diagnostics operation on the background executor.
pub fn spawn_diagnostics_work<T: Send + 'static>(
    cx: &App,
    work: impl FnOnce() -> T + Send + 'static,
) -> Task<T> {
    background_spawn(cx, work)
}

/// Record an event on the background executor.
pub fn spawn_record_event(
    cx: &App,
    service: DiagnosticsService,
    event: crate::DiagnosticEvent,
    context: OperationContext,
) -> Task<ServiceResult<crate::RecordOutcome>> {
    background_spawn(cx, move || service.record_event(event, &context))
}

/// Recover persisted state on the background executor.
pub fn spawn_recovery(
    cx: &App,
    service: DiagnosticsService,
    context: OperationContext,
) -> Task<ServiceResult<crate::RecoveryReport>> {
    background_spawn(cx, move || service.recover(&context))
}

/// Stage an export bundle on the background executor. Retain the returned lease
/// until the native consumer ends.
pub fn spawn_export_bundle(
    cx: &App,
    service: DiagnosticsService,
    app_data: AppData,
    context: OperationContext,
) -> Task<ServiceResult<ExportResourceLease>> {
    background_spawn(cx, move || service.export_bundle(&app_data, &context))
}

/// Future form for callers already inside a foreground task.
pub fn spawn_export_bundle_async(
    cx: &gpui::AsyncApp,
    service: DiagnosticsService,
    app_data: AppData,
    context: OperationContext,
) -> Task<ServiceResult<ExportResourceLease>> {
    let executor = cx.background_executor();
    executor.spawn(async move { service.export_bundle(&app_data, &context) })
}

/// Entity-owned scope retaining diagnostics observations and the UI release
/// guard. Dropping the scope drops streams first, then disposes the owner.
pub struct DiagnosticsEntityScope {
    subscriptions: Vec<EventSubscription<DiagnosticServiceEvent>>,
    scope: gpui_system::gpui_adapter::EntityScope,
}

impl DiagnosticsEntityScope {
    pub fn new<T: 'static>(owner: UiOwner, cx: &Context<T>) -> Self {
        Self {
            subscriptions: Vec::new(),
            scope: gpui_system::gpui_adapter::EntityScope::new(owner, cx),
        }
    }

    pub fn owner(&self) -> &UiOwner {
        self.scope.owner()
    }

    pub fn retain(&mut self, subscription: EventSubscription<DiagnosticServiceEvent>) {
        self.subscriptions.push(subscription);
    }

    pub fn subscribe(&mut self, service: &DiagnosticsService) {
        self.retain(service.subscribe());
    }

    pub fn subscription_count(&self) -> usize {
        self.subscriptions.len()
    }
}

/// Retain a GPUI task without requiring the caller to name the future.
pub fn detach<T: 'static>(_: Task<T>) {}

#[cfg(all(
    test,
    feature = "gpui",
    any(target_os = "windows", target_os = "macos", target_os = "linux")
))]
mod tests {
    use super::*;
    use crate::{DiagnosticsOperation, FakeDiagnosticsProvider};
    use gpui::TestAppContext;
    use std::sync::Arc;

    #[gpui::test]
    fn installs_and_resolves_the_global_service(cx: &TestAppContext) {
        cx.update(|cx| {
            let fake = FakeDiagnosticsProvider::new();
            let service = DiagnosticsService::new(Arc::new(fake));
            gpui_system::gpui_adapter::install_system_services(SystemServices::new(), cx);
            install_diagnostics_service(service.clone(), cx);
            let resolved = diagnostics_service(cx).expect("installed service");
            assert_eq!(
                resolved.capabilities().len(),
                DiagnosticsOperation::ALL.len()
            );
            assert_eq!(resolved.revision(), 0);
            let mut services = SystemServices::new();
            install_diagnostics_in_services(&mut services, service).expect("registry");
            assert!(services.service::<DiagnosticsService>().is_ok());
            detach(spawn_diagnostics_work(cx, || 1));
            detach(gpui_system::gpui_adapter::spawn_local(cx, async {}));
        });
    }

    #[gpui::test]
    fn missing_global_is_typed(cx: &TestAppContext) {
        cx.update(|cx| {
            assert_eq!(
                diagnostics_service(cx).unwrap_err().code,
                ErrorCode::Unavailable
            );
        });
    }
}
