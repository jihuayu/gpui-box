//! Explicit diagnostics provider contract plus in-process and deterministic
//! fake providers.

use crate::export::{DiagnosticExportBundle, OperationCapabilitySummary};
use crate::model::{
    BuildIdentity, CrashMarker, CrashRecord, CrashRecordSource, DIAGNOSTICS_FORMAT_VERSION,
    DiagnosticEvent, DiagnosticsOperation, PruneReport, RecordOutcome, RecoveryReport,
    RetentionPolicy, TerminationKind, UnavailableEvidence, now_millis,
};
use crate::store::{
    DiagnosticStore, IntoAppData, StorageHook, check_background, in_process_capabilities,
};
use gpui_app_data::AppData;
use gpui_system::{
    Availability, CancelReason, Capability, Completion, Configuration, Constraint, ErrorCode,
    Evidence, OperationContext, PermissionState, ProviderManifest, RetryAdvice, ServiceResult,
    Support, SystemError,
};
use std::{
    any::Any,
    collections::{BTreeMap, BTreeSet},
    fmt,
    sync::{Arc, Mutex},
};

/// Conservative manifest: no native diagnostics collector is installed by this
/// crate. Platform collectors remain explicit host/provider work.
pub const PROVIDER_MANIFEST: [ProviderManifest; 5] = gpui_system::PROVIDER_MANIFEST;

fn error(code: ErrorCode, message: &'static str) -> SystemError {
    SystemError::new(code, message)
}

fn completed<T>(value: T) -> ServiceResult<T> {
    Ok(Completion::Completed(value))
}

fn default_capability(operation: DiagnosticsOperation) -> Capability<DiagnosticsOperation> {
    Capability {
        operation,
        support: Support::Native,
        availability: Availability::Available,
        configuration: Configuration::Ready,
        permission: PermissionState::NotRequired,
        evidence: Evidence::Compiled,
        constraints: Vec::new(),
        observed_revision: 1,
    }
}

/// Provider boundary for structured diagnostics.
///
/// Implementing types must be safe to call from a background executor. Every
/// method must reject a UI-attached [`OperationContext`] before touching
/// storage or returning a result.
pub trait DiagnosticsProvider: Send + Sync {
    fn capability(&self, operation: DiagnosticsOperation) -> Capability<DiagnosticsOperation>;

    fn capabilities(&self) -> Vec<Capability<DiagnosticsOperation>> {
        DiagnosticsOperation::ALL
            .into_iter()
            .map(|operation| self.capability(operation))
            .collect()
    }

    fn record_event(
        &self,
        event: DiagnosticEvent,
        context: &OperationContext,
    ) -> ServiceResult<RecordOutcome>;

    fn write_crash_marker(
        &self,
        termination: TerminationKind,
        context: &OperationContext,
    ) -> ServiceResult<()>;

    fn inspect_recovery(&self, context: &OperationContext) -> ServiceResult<RecoveryReport>;

    fn export_bundle(
        &self,
        app_data: &AppData,
        context: &OperationContext,
    ) -> ServiceResult<DiagnosticExportBundle>;

    fn prune_retained(&self, context: &OperationContext) -> ServiceResult<PruneReport>;

    /// Session lifecycle maintenance. Gated under `PruneRetained` by the service
    /// because it is a maintenance operation rather than a report generator.
    fn mark_clean_exit(&self, context: &OperationContext) -> ServiceResult<()>;

    fn unavailable_evidence(&self) -> Vec<UnavailableEvidence> {
        Vec::new()
    }

    fn as_any(&self) -> &dyn Any;
}

/// Store-backed provider used by desktop/headless hosts.
#[derive(Clone)]
pub struct InProcessDiagnosticsProvider {
    store: DiagnosticStore,
}

impl InProcessDiagnosticsProvider {
    pub fn new(store: DiagnosticStore) -> Self {
        Self { store }
    }

    pub fn store(&self) -> &DiagnosticStore {
        &self.store
    }

    pub fn from_store(store: DiagnosticStore) -> Self {
        Self::new(store)
    }

    /// Open a store-backed provider from `AppData`.
    pub fn open(app: impl IntoAppData, identity: BuildIdentity) -> Result<Self, SystemError> {
        DiagnosticStore::open(app, identity).map(Self::new)
    }

    /// Open a store-backed provider with explicit retention and fault hooks.
    pub fn open_with_options(
        app: impl IntoAppData,
        identity: BuildIdentity,
        retention: RetentionPolicy,
        hook: Arc<dyn StorageHook>,
    ) -> Result<Self, SystemError> {
        DiagnosticStore::open_with_options(app, identity, retention, hook).map(Self::new)
    }
}

impl fmt::Debug for InProcessDiagnosticsProvider {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("InProcessDiagnosticsProvider")
            .field("store", &self.store)
            .finish()
    }
}

impl DiagnosticsProvider for InProcessDiagnosticsProvider {
    fn capability(&self, operation: DiagnosticsOperation) -> Capability<DiagnosticsOperation> {
        in_process_capabilities()
            .into_iter()
            .find(|capability| capability.operation == operation)
            .unwrap_or_else(|| default_capability(operation))
    }

    fn record_event(
        &self,
        event: DiagnosticEvent,
        context: &OperationContext,
    ) -> ServiceResult<RecordOutcome> {
        self.capability(DiagnosticsOperation::RecordEvent).check()?;
        self.store.record_event(event, context)
    }

    fn write_crash_marker(
        &self,
        termination: TerminationKind,
        context: &OperationContext,
    ) -> ServiceResult<()> {
        self.capability(DiagnosticsOperation::WriteCrashMarker)
            .check()?;
        self.store.write_crash_marker(termination, context)
    }

    fn inspect_recovery(&self, context: &OperationContext) -> ServiceResult<RecoveryReport> {
        self.capability(DiagnosticsOperation::InspectRecovery)
            .check()?;
        self.store.recover(context)
    }

    fn export_bundle(
        &self,
        _app_data: &AppData,
        context: &OperationContext,
    ) -> ServiceResult<DiagnosticExportBundle> {
        self.capability(DiagnosticsOperation::ExportBundle)
            .check()?;
        self.store.export_bundle(context)
    }

    fn prune_retained(&self, context: &OperationContext) -> ServiceResult<PruneReport> {
        self.capability(DiagnosticsOperation::PruneRetained)
            .check()?;
        self.store.prune_retained(context)
    }

    fn mark_clean_exit(&self, context: &OperationContext) -> ServiceResult<()> {
        self.capability(DiagnosticsOperation::PruneRetained)
            .check()?;
        self.store.mark_clean_exit(context)
    }

    fn unavailable_evidence(&self) -> Vec<UnavailableEvidence> {
        self.store.unavailable_evidence().to_vec()
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

struct FakeState {
    identity: BuildIdentity,
    retention: RetentionPolicy,
    events: Vec<DiagnosticEvent>,
    crash_markers: Vec<CrashMarker>,
    crash_records: Vec<CrashRecord>,
    capabilities: BTreeMap<DiagnosticsOperation, Capability<DiagnosticsOperation>>,
    unavailable: Vec<UnavailableEvidence>,
    failures: BTreeMap<DiagnosticsOperation, SystemError>,
    disk_full: BTreeSet<DiagnosticsOperation>,
    cancel_during_export: bool,
    cancel_after_bundle: bool,
    clean_exit_marked: bool,
    session_id: u64,
    next_sequence: u64,
}

/// Deterministic diagnostics provider for capability, fault and cancellation
/// tests. It never touches the filesystem.
#[derive(Clone)]
pub struct FakeDiagnosticsProvider {
    state: Arc<Mutex<FakeState>>,
}

impl Default for FakeDiagnosticsProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl FakeDiagnosticsProvider {
    pub fn new() -> Self {
        let identity = BuildIdentity::current();
        Self {
            state: Arc::new(Mutex::new(FakeState {
                identity,
                retention: RetentionPolicy::default(),
                events: Vec::new(),
                crash_markers: Vec::new(),
                crash_records: Vec::new(),
                capabilities: DiagnosticsOperation::ALL
                    .into_iter()
                    .map(|operation| (operation, default_capability(operation)))
                    .collect(),
                unavailable: crate::matrix::default_unavailable_evidence(),
                failures: BTreeMap::new(),
                disk_full: BTreeSet::new(),
                cancel_during_export: false,
                cancel_after_bundle: false,
                clean_exit_marked: false,
                session_id: 1,
                next_sequence: 1,
            })),
        }
    }

    pub fn with_build_identity(self, identity: BuildIdentity) -> Self {
        self.with_state(|state| state.identity = identity.sanitized());
        self
    }

    pub fn set_retention(&self, retention: RetentionPolicy) {
        self.with_state(|state| state.retention = retention);
    }

    pub fn set_operation_support(&self, operation: DiagnosticsOperation, support: Support) {
        self.with_capability(operation, |capability| capability.support = support);
    }

    /// Short alias used by host and test harnesses.
    pub fn set_support(&self, operation: DiagnosticsOperation, support: Support) {
        self.set_operation_support(operation, support);
    }

    pub fn set_operation_availability(
        &self,
        operation: DiagnosticsOperation,
        availability: Availability,
    ) {
        self.with_capability(operation, |capability| {
            capability.availability = availability
        });
    }

    pub fn set_operation_configuration(
        &self,
        operation: DiagnosticsOperation,
        configuration: Configuration,
    ) {
        self.with_capability(operation, |capability| {
            capability.configuration = configuration
        });
    }

    pub fn set_operation_permission(
        &self,
        operation: DiagnosticsOperation,
        permission: PermissionState,
    ) {
        self.with_capability(operation, |capability| capability.permission = permission);
    }

    pub fn add_operation_constraint(
        &self,
        operation: DiagnosticsOperation,
        constraint: Constraint,
    ) {
        self.with_capability(operation, |capability| {
            capability.constraints.push(constraint);
        });
    }

    pub fn clear_capability_overrides(&self) {
        self.with_state(|state| {
            state.capabilities = DiagnosticsOperation::ALL
                .into_iter()
                .map(|operation| (operation, default_capability(operation)))
                .collect();
        });
    }

    pub fn set_unavailable_evidence(&self, evidence: Vec<UnavailableEvidence>) {
        self.with_state(|state| state.unavailable = evidence);
    }

    pub fn set_failure(&self, operation: DiagnosticsOperation, failure: SystemError) {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        state.disk_full.remove(&operation);
        state.failures.insert(operation, failure);
    }

    /// Simulate a full disk for the next operation of this kind.
    pub fn set_disk_full(&self, operation: DiagnosticsOperation) {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        state.failures.remove(&operation);
        state.disk_full.insert(operation);
    }

    pub fn clear_failures(&self) {
        self.with_state(|state| {
            state.failures.clear();
            state.disk_full.clear();
        });
    }

    /// Cancel the supplied operation context while export is in progress. This
    /// exercises service cleanup without staging a partial lease.
    pub fn set_cancel_during_export(&self, cancel: bool) {
        self.with_state(|state| state.cancel_during_export = cancel);
    }

    /// Cancel the context, then still return a completed bundle. The service
    /// must observe the cancellation and refuse to stage any export file.
    pub fn set_cancel_after_bundle(&self, cancel: bool) {
        self.with_state(|state| state.cancel_after_bundle = cancel);
    }

    pub fn events(&self) -> Vec<DiagnosticEvent> {
        self.with_state(|state| state.events.clone())
    }

    pub fn crash_markers(&self) -> Vec<CrashMarker> {
        self.with_state(|state| state.crash_markers.clone())
    }

    pub fn crash_records(&self) -> Vec<CrashRecord> {
        self.with_state(|state| state.crash_records.clone())
    }

    pub fn clean_exit_marked(&self) -> bool {
        self.with_state(|state| state.clean_exit_marked)
    }

    pub fn build_identity(&self) -> BuildIdentity {
        self.with_state(|state| state.identity.clone())
    }

    pub fn recovery_report(&self) -> RecoveryReport {
        self.with_state(|state| fake_recovery(state))
    }

    fn with_state<T>(&self, f: impl FnOnce(&mut FakeState) -> T) -> T {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        f(&mut state)
    }

    fn with_capability(
        &self,
        operation: DiagnosticsOperation,
        f: impl FnOnce(&mut Capability<DiagnosticsOperation>),
    ) {
        self.with_state(|state| {
            let capability = state
                .capabilities
                .entry(operation)
                .or_insert_with(|| default_capability(operation));
            f(capability);
        });
    }

    fn take_failure(&self, operation: DiagnosticsOperation) -> Option<SystemError> {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if state.disk_full.remove(&operation) {
            let mut error = error(
                ErrorCode::PlatformFailure,
                "Simulated diagnostics storage-full failure",
            );
            error.retry = RetryAdvice::AfterConditionsChange;
            return Some(error);
        }
        state.failures.get(&operation).cloned()
    }
}

impl fmt::Debug for FakeDiagnosticsProvider {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        f.debug_struct("FakeDiagnosticsProvider")
            .field("build_identity", &state.identity)
            .field("events", &state.events.len())
            .field("crash_markers", &state.crash_markers.len())
            .field("crash_records", &state.crash_records.len())
            .field("clean_exit_marked", &state.clean_exit_marked)
            .finish()
    }
}

impl DiagnosticsProvider for FakeDiagnosticsProvider {
    fn capability(&self, operation: DiagnosticsOperation) -> Capability<DiagnosticsOperation> {
        self.with_state(|state| {
            state
                .capabilities
                .get(&operation)
                .cloned()
                .unwrap_or_else(|| default_capability(operation))
        })
    }

    fn record_event(
        &self,
        event: DiagnosticEvent,
        context: &OperationContext,
    ) -> ServiceResult<RecordOutcome> {
        if let Some(cancelled) = check_background::<RecordOutcome>(context)? {
            return Ok(cancelled);
        }
        self.capability(DiagnosticsOperation::RecordEvent).check()?;
        if let Some(failure) = self.take_failure(DiagnosticsOperation::RecordEvent) {
            return Err(failure);
        }
        let evicted_events = self.with_state(|state| {
            state.events.push(event.clone());
            let retention = state.retention;
            let mut evicted = 0u64;
            while state.events.len() > retention.max_events {
                state.events.remove(0);
                evicted = evicted.saturating_add(1);
            }
            evicted
        });
        completed(RecordOutcome {
            event,
            stored: true,
            evicted_events,
        })
    }

    fn write_crash_marker(
        &self,
        termination: TerminationKind,
        context: &OperationContext,
    ) -> ServiceResult<()> {
        if let Some(cancelled) = check_background::<()>(context)? {
            return Ok(cancelled);
        }
        self.capability(DiagnosticsOperation::WriteCrashMarker)
            .check()?;
        if !termination.is_crash_marker() {
            return Err(error(
                ErrorCode::InvalidInput,
                "Crash markers require RustPanic, AndroidAnr, NativeCrash or OsTermination evidence",
            ));
        }
        if let Some(failure) = self.take_failure(DiagnosticsOperation::WriteCrashMarker) {
            return Err(failure);
        }
        self.with_state(|state| {
            let marker = CrashMarker::new(
                state.session_id,
                state.identity.clone(),
                now_millis(),
                termination.clone(),
            );
            state.crash_markers.push(marker.clone());
            let sequence = state.next_sequence;
            state.next_sequence = state.next_sequence.saturating_add(1);
            state.crash_records.push(CrashRecord::new(
                sequence,
                marker.session_id,
                marker.build_identity.clone(),
                marker.recorded_at_millis,
                marker.termination.clone(),
                CrashRecordSource::CrashMarker,
            ));
        });
        completed(())
    }

    fn inspect_recovery(&self, context: &OperationContext) -> ServiceResult<RecoveryReport> {
        if let Some(cancelled) = check_background::<RecoveryReport>(context)? {
            return Ok(cancelled);
        }
        self.capability(DiagnosticsOperation::InspectRecovery)
            .check()?;
        if let Some(failure) = self.take_failure(DiagnosticsOperation::InspectRecovery) {
            return Err(failure);
        }
        completed(self.recovery_report())
    }

    fn export_bundle(
        &self,
        _app_data: &AppData,
        context: &OperationContext,
    ) -> ServiceResult<DiagnosticExportBundle> {
        if let Some(cancelled) = check_background::<DiagnosticExportBundle>(context)? {
            return Ok(cancelled);
        }
        self.capability(DiagnosticsOperation::ExportBundle)
            .check()?;
        let cancel_during_export = self.with_state(|state| state.cancel_during_export);
        if cancel_during_export {
            context.cancellation.cancel(CancelReason::Caller);
            return Ok(Completion::Cancelled(CancelReason::Caller));
        }
        if let Some(failure) = self.take_failure(DiagnosticsOperation::ExportBundle) {
            return Err(failure);
        }
        let cancel_after_bundle = self.with_state(|state| state.cancel_after_bundle);
        if cancel_after_bundle {
            context.cancellation.cancel(CancelReason::Caller);
        }
        let (recovery, events, crash_records, capabilities, retention, identity) =
            self.with_state(|state| {
                let capabilities = state
                    .capabilities
                    .values()
                    .map(OperationCapabilitySummary::from_capability)
                    .collect();
                (
                    fake_recovery(state),
                    state.events.clone(),
                    state.crash_records.clone(),
                    capabilities,
                    state.retention,
                    state.identity.clone(),
                )
            });
        let session_id = recovery.current_session_id;
        completed(DiagnosticExportBundle::new(
            identity,
            session_id,
            capabilities,
            retention,
            recovery.clone(),
            events,
            crash_records,
        ))
    }

    fn prune_retained(&self, context: &OperationContext) -> ServiceResult<PruneReport> {
        if let Some(cancelled) = check_background::<PruneReport>(context)? {
            return Ok(cancelled);
        }
        self.capability(DiagnosticsOperation::PruneRetained)
            .check()?;
        if let Some(failure) = self.take_failure(DiagnosticsOperation::PruneRetained) {
            return Err(failure);
        }
        completed(self.with_state(|state| PruneReport {
            evicted_events: 0,
            evicted_crash_records: 0,
            retained_events: state.events.len(),
            retained_crash_records: state.crash_records.len(),
        }))
    }

    fn mark_clean_exit(&self, context: &OperationContext) -> ServiceResult<()> {
        if let Some(cancelled) = check_background::<()>(context)? {
            return Ok(cancelled);
        }
        self.capability(DiagnosticsOperation::PruneRetained)
            .check()?;
        if let Some(failure) = self.take_failure(DiagnosticsOperation::PruneRetained) {
            return Err(failure);
        }
        self.with_state(|state| state.clean_exit_marked = true);
        completed(())
    }

    fn unavailable_evidence(&self) -> Vec<UnavailableEvidence> {
        self.with_state(|state| state.unavailable.clone())
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

fn fake_recovery(state: &FakeState) -> RecoveryReport {
    RecoveryReport {
        format_version: DIAGNOSTICS_FORMAT_VERSION,
        current_session_id: state.session_id,
        build_identity: state.identity.clone(),
        prior_session: None,
        recovered_crash_records: state.crash_records.clone(),
        retained_events: state.events.len(),
        evicted_events: 0,
        retained_crash_records: state.crash_records.len(),
        evicted_crash_records: 0,
        quarantined_entries: Vec::new(),
        quarantined_count: 0,
        unavailable_evidence: state.unavailable.clone(),
    }
}
