//! Capability-gated diagnostics service and bounded observation stream.

use crate::export::DiagnosticExportBundle;
use crate::model::{
    BuildIdentity, DiagnosticDomain, DiagnosticEvent, DiagnosticSeverity, DiagnosticsOperation,
    MAX_EXPORT_BYTES, PruneReport, RecordOutcome, RecoveryReport, RetentionPolicy, TerminationKind,
    UnavailableEvidence,
};
use crate::provider::{DiagnosticsProvider, FakeDiagnosticsProvider, InProcessDiagnosticsProvider};
use crate::store::{DiagnosticStore, IntoAppData, StorageHook, check_background};
use gpui_app_data::AppData;
use gpui_files::ExportResourceLease;
use gpui_system::{
    Capability, Completion, ErrorCode, EventSender, EventSubscription, OperationContext,
    RetryAdvice, ServiceResult, SystemError, SystemServices, event_channel,
};
use std::{
    fmt,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};

const EVENT_CAPACITY: usize = 16;

type Subscriber = (Arc<AtomicBool>, EventSender<DiagnosticServiceEvent>);

/// Safe, payload-free service observations. Raw diagnostic values are never
/// copied into the event stream.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiagnosticServiceEvent {
    EventRecorded {
        domain: DiagnosticDomain,
        severity: DiagnosticSeverity,
        stored: bool,
    },
    CrashMarkerWritten {
        kind: &'static str,
    },
    RecoveryInspected {
        recovered_crash_records: usize,
        retained_events: usize,
        quarantined: usize,
    },
    BundleStaged {
        bytes: u64,
    },
    Pruned {
        evicted_events: u64,
        evicted_crash_records: u64,
    },
    CleanExitMarked,
    /// Emitted exactly once whenever a gated service operation returns `Err`.
    ///
    /// This covers context/capability gate failures as well as provider,
    /// encoding, staging and simulated disk-full failures. Cooperative
    /// `Ok(Completion::Cancelled(_))` is not an error and never emits this
    /// event.
    OperationFailed {
        operation: DiagnosticsOperation,
        code: ErrorCode,
    },
}

#[derive(Default)]
struct ServiceState {
    subscribers: Vec<Subscriber>,
}

struct ServiceInner {
    provider: Arc<dyn DiagnosticsProvider>,
    state: Mutex<ServiceState>,
}

fn lock<T>(value: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    value
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn error<T>(code: ErrorCode, message: &'static str) -> Result<T, SystemError> {
    Err(SystemError::new(code, message))
}

impl ServiceInner {
    fn emit(&self, event: DiagnosticServiceEvent) {
        let deliveries = {
            let mut state = lock(&self.state);
            state
                .subscribers
                .retain(|(live, _)| live.load(Ordering::Acquire));
            state
                .subscribers
                .iter()
                .map(|(_, sender)| sender.clone())
                .collect::<Vec<_>>()
        };
        for sender in deliveries {
            sender.send(event.clone());
        }
    }

    fn gate<T>(
        &self,
        operation: DiagnosticsOperation,
        context: &OperationContext,
    ) -> Result<Option<Completion<T>>, SystemError> {
        if let Some(cancelled) = check_background(context)? {
            return Ok(Some(cancelled));
        }
        self.provider.capability(operation).check()?;
        Ok(None)
    }

    fn emit_failure(&self, operation: DiagnosticsOperation, error: &SystemError) {
        self.emit(DiagnosticServiceEvent::OperationFailed {
            operation,
            code: error.code,
        });
    }
}

/// Cloneable, `Send + Sync` diagnostics service suitable for
/// `gpui_system::SystemServices` and background execution.
#[derive(Clone)]
pub struct DiagnosticsService(Arc<ServiceInner>);

impl DiagnosticsService {
    pub fn new(provider: Arc<dyn DiagnosticsProvider>) -> Self {
        Self(Arc::new(ServiceInner {
            provider,
            state: Mutex::new(ServiceState::default()),
        }))
    }

    pub fn from_provider(provider: impl DiagnosticsProvider + 'static) -> Self {
        Self::new(Arc::new(provider))
    }

    /// Construct a service backed by an `AppData` diagnostics namespace.
    pub fn in_process(app: impl IntoAppData, identity: BuildIdentity) -> Result<Self, SystemError> {
        let store = DiagnosticStore::open(app, identity)?;
        Ok(Self::from_store(store))
    }

    pub fn in_process_with_retention(
        app: impl IntoAppData,
        identity: BuildIdentity,
        retention: RetentionPolicy,
    ) -> Result<Self, SystemError> {
        let store = DiagnosticStore::open_with_retention(app, identity, retention)?;
        Ok(Self::from_store(store))
    }

    pub fn in_process_with_options(
        app: impl IntoAppData,
        identity: BuildIdentity,
        retention: RetentionPolicy,
        hook: Arc<dyn StorageHook>,
    ) -> Result<Self, SystemError> {
        let store = DiagnosticStore::open_with_options(app, identity, retention, hook)?;
        Ok(Self::from_store(store))
    }

    pub fn from_store(store: DiagnosticStore) -> Self {
        Self::from_provider(InProcessDiagnosticsProvider::new(store))
    }

    pub fn from_fake(fake: FakeDiagnosticsProvider) -> Self {
        Self::from_provider(fake)
    }

    pub fn provider(&self) -> &Arc<dyn DiagnosticsProvider> {
        &self.0.provider
    }

    /// Access the store behind an in-process provider, when present.
    pub fn store(&self) -> Option<DiagnosticStore> {
        self.0
            .provider
            .as_any()
            .downcast_ref::<InProcessDiagnosticsProvider>()
            .map(|provider| provider.store().clone())
    }

    /// Access the fake provider, when present. Intended for tests.
    pub fn fake(&self) -> Option<&FakeDiagnosticsProvider> {
        self.0
            .provider
            .as_any()
            .downcast_ref::<FakeDiagnosticsProvider>()
    }

    pub fn install(self, services: &mut SystemServices) -> Result<(), SystemError> {
        services.register(self)
    }

    pub fn install_cloned(&self, services: &mut SystemServices) -> Result<(), SystemError> {
        services.register(self.clone())
    }

    /// Describe one operation without performing it.
    pub fn capability(&self, operation: DiagnosticsOperation) -> Capability<DiagnosticsOperation> {
        self.0.provider.capability(operation)
    }

    pub fn capabilities(&self) -> Vec<Capability<DiagnosticsOperation>> {
        self.0.provider.capabilities()
    }

    fn gate_or_fail<T>(
        &self,
        operation: DiagnosticsOperation,
        context: &OperationContext,
    ) -> Result<Option<Completion<T>>, SystemError> {
        match self.0.gate(operation, context) {
            Ok(value) => Ok(value),
            Err(error) => {
                self.0.emit_failure(operation, &error);
                Err(error)
            }
        }
    }

    fn fail<T>(&self, operation: DiagnosticsOperation, error: SystemError) -> ServiceResult<T> {
        self.0.emit_failure(operation, &error);
        Err(error)
    }

    /// Create an independent bounded observation stream.
    pub fn subscribe(&self) -> EventSubscription<DiagnosticServiceEvent> {
        let live = Arc::new(AtomicBool::new(true));
        let cleanup = Arc::clone(&live);
        let (sender, subscription) = event_channel(EVENT_CAPACITY, move || {
            cleanup.store(false, Ordering::Release);
        });
        let mut state = lock(&self.0.state);
        state
            .subscribers
            .retain(|(live, _)| live.load(Ordering::Acquire));
        state.subscribers.push((live, sender));
        subscription
    }

    /// Highest observation sequence across live subscribers.
    pub fn revision(&self) -> u64 {
        let mut state = lock(&self.0.state);
        state
            .subscribers
            .retain(|(live, _)| live.load(Ordering::Acquire));
        state
            .subscribers
            .iter()
            .map(|(_, sender)| sender.revision())
            .max()
            .unwrap_or(0)
    }

    pub fn unavailable_evidence(&self) -> Vec<UnavailableEvidence> {
        self.0.provider.unavailable_evidence()
    }

    /// Record a correlated, already-redacted structured event.
    pub fn record_event(
        &self,
        event: DiagnosticEvent,
        context: &OperationContext,
    ) -> ServiceResult<RecordOutcome> {
        let operation = DiagnosticsOperation::RecordEvent;
        if let Some(cancelled) = self.gate_or_fail::<RecordOutcome>(operation, context)? {
            return Ok(cancelled);
        }
        let domain = event.domain;
        let severity = event.severity;
        match self.0.provider.record_event(event, context) {
            Ok(Completion::Completed(outcome)) => {
                self.0.emit(DiagnosticServiceEvent::EventRecorded {
                    domain,
                    severity,
                    stored: outcome.stored,
                });
                Ok(Completion::Completed(outcome))
            }
            Ok(Completion::Cancelled(reason)) => Ok(Completion::Cancelled(reason)),
            Err(error) => self.fail(operation, error),
        }
    }

    /// Persist a host crash marker for the active session.
    pub fn write_crash_marker(
        &self,
        termination: TerminationKind,
        context: &OperationContext,
    ) -> ServiceResult<()> {
        let operation = DiagnosticsOperation::WriteCrashMarker;
        if let Some(cancelled) = self.gate_or_fail::<()>(operation, context)? {
            return Ok(cancelled);
        }
        let kind = termination.label();
        match self.0.provider.write_crash_marker(termination, context) {
            Ok(Completion::Completed(())) => {
                self.0
                    .emit(DiagnosticServiceEvent::CrashMarkerWritten { kind });
                Ok(Completion::Completed(()))
            }
            Ok(Completion::Cancelled(reason)) => Ok(Completion::Cancelled(reason)),
            Err(error) => self.fail(operation, error),
        }
    }

    /// Inspect recovery state. The first call opens the active session; later
    /// calls report the current retained counts without creating another session.
    pub fn inspect_recovery(&self, context: &OperationContext) -> ServiceResult<RecoveryReport> {
        let operation = DiagnosticsOperation::InspectRecovery;
        if let Some(cancelled) = self.gate_or_fail::<RecoveryReport>(operation, context)? {
            return Ok(cancelled);
        }
        match self.0.provider.inspect_recovery(context) {
            Ok(Completion::Completed(report)) => {
                self.0.emit(DiagnosticServiceEvent::RecoveryInspected {
                    recovered_crash_records: report.recovered_crash_records.len(),
                    retained_events: report.retained_events,
                    quarantined: report.quarantined_count,
                });
                Ok(Completion::Completed(report))
            }
            Ok(Completion::Cancelled(reason)) => Ok(Completion::Cancelled(reason)),
            Err(error) => self.fail(operation, error),
        }
    }

    /// Alias for [`Self::inspect_recovery`] used by host entrypoints.
    pub fn recover(&self, context: &OperationContext) -> ServiceResult<RecoveryReport> {
        self.inspect_recovery(context)
    }

    /// Stage a versioned diagnostic JSON bundle as a recoverable
    /// [`ExportResourceLease`]. Cancellation before or during export returns
    /// `Completion::Cancelled` without leaving a staged file.
    pub fn export_bundle(
        &self,
        app_data: &AppData,
        context: &OperationContext,
    ) -> ServiceResult<ExportResourceLease> {
        match self.export_bundle_inner(app_data, context) {
            Ok(value) => value,
            Err(error) => self.fail(DiagnosticsOperation::ExportBundle, error),
        }
    }

    fn export_bundle_inner(
        &self,
        app_data: &AppData,
        context: &OperationContext,
    ) -> Result<ServiceResult<ExportResourceLease>, SystemError> {
        if let Some(cancelled) = self.0.gate(DiagnosticsOperation::ExportBundle, context)? {
            return Ok(Ok(cancelled));
        }
        let bundle = match self.0.provider.export_bundle(app_data, context) {
            Ok(Completion::Completed(bundle)) => bundle,
            Ok(Completion::Cancelled(reason)) => return Ok(Ok(Completion::Cancelled(reason))),
            Err(error) => return Err(error),
        };
        if !bundle.is_current_schema() {
            return error(
                ErrorCode::IntegrityFailure,
                "Diagnostic export has an unsupported schema",
            );
        }
        if let Some(cancelled) = check_background::<ExportResourceLease>(context)? {
            return Ok(Ok(cancelled));
        }
        let bytes = serde_json::to_vec(&bundle).map_err(|_| {
            SystemError::new(
                ErrorCode::IntegrityFailure,
                "Diagnostic export cannot be encoded",
            )
        })?;
        if bytes.len() > MAX_EXPORT_BYTES {
            return error(
                ErrorCode::InvalidInput,
                "Diagnostic export exceeds the size bound",
            );
        }
        if let Some(cancelled) = check_background::<ExportResourceLease>(context)? {
            return Ok(Ok(cancelled));
        }
        let lease =
            ExportResourceLease::stage_export_bytes(app_data, &bytes, MAX_EXPORT_BYTES, context)?;
        if let Completion::Completed(lease) = &lease {
            self.0.emit(DiagnosticServiceEvent::BundleStaged {
                bytes: lease.bytes(),
            });
        }
        Ok(Ok(lease))
    }

    /// Apply retention policy explicitly.
    pub fn prune_retained(&self, context: &OperationContext) -> ServiceResult<PruneReport> {
        let operation = DiagnosticsOperation::PruneRetained;
        if let Some(cancelled) = self.gate_or_fail::<PruneReport>(operation, context)? {
            return Ok(cancelled);
        }
        match self.0.provider.prune_retained(context) {
            Ok(Completion::Completed(report)) => {
                self.0.emit(DiagnosticServiceEvent::Pruned {
                    evicted_events: report.evicted_events,
                    evicted_crash_records: report.evicted_crash_records,
                });
                Ok(Completion::Completed(report))
            }
            Ok(Completion::Cancelled(reason)) => Ok(Completion::Cancelled(reason)),
            Err(error) => self.fail(operation, error),
        }
    }

    /// Mark the active session clean. Gated under `PruneRetained` because it is
    /// lifecycle maintenance rather than a report or export operation.
    pub fn mark_clean_exit(&self, context: &OperationContext) -> ServiceResult<()> {
        let operation = DiagnosticsOperation::PruneRetained;
        if let Some(cancelled) = self.gate_or_fail::<()>(operation, context)? {
            return Ok(cancelled);
        }
        match self.0.provider.mark_clean_exit(context) {
            Ok(Completion::Completed(())) => {
                self.0.emit(DiagnosticServiceEvent::CleanExitMarked);
                Ok(Completion::Completed(()))
            }
            Ok(Completion::Cancelled(reason)) => Ok(Completion::Cancelled(reason)),
            Err(error) => self.fail(operation, error),
        }
    }

    /// Inspect the versioned bundle without staging it. Useful for host bridges
    /// that provide their own export transport.
    pub fn diagnostic_bundle(
        &self,
        app_data: &AppData,
        context: &OperationContext,
    ) -> ServiceResult<DiagnosticExportBundle> {
        match self.diagnostic_bundle_inner(app_data, context) {
            Ok(value) => value,
            Err(error) => self.fail(DiagnosticsOperation::ExportBundle, error),
        }
    }

    fn diagnostic_bundle_inner(
        &self,
        app_data: &AppData,
        context: &OperationContext,
    ) -> Result<ServiceResult<DiagnosticExportBundle>, SystemError> {
        if let Some(cancelled) = self.0.gate(DiagnosticsOperation::ExportBundle, context)? {
            return Ok(Ok(cancelled));
        }
        match self.0.provider.export_bundle(app_data, context) {
            Ok(value) => Ok(Ok(value)),
            Err(error) => Err(error),
        }
    }
}

impl fmt::Debug for DiagnosticsService {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DiagnosticsService")
            .field("revision", &self.revision())
            .field(
                "unavailable_evidence",
                &self.0.provider.unavailable_evidence().len(),
            )
            .finish()
    }
}

/// Convenience constructor for a fake-backed service.
pub fn fake_service(fake: FakeDiagnosticsProvider) -> DiagnosticsService {
    DiagnosticsService::from_fake(fake)
}

/// Convenience constructor for a store-backed service.
pub fn in_process_service(
    app: impl IntoAppData,
    identity: BuildIdentity,
) -> Result<DiagnosticsService, SystemError> {
    DiagnosticsService::in_process(app, identity)
}

/// Build a retryable platform failure for a host that cannot initialize storage.
pub fn unavailable_service(reason: &'static str) -> SystemError {
    let mut error = SystemError::new(ErrorCode::Unavailable, reason);
    error.retry = RetryAdvice::AfterConditionsChange;
    error
}
