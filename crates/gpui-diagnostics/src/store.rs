//! Atomic, bounded diagnostics persistence under the private `diagnostics`
//! `AppData` namespace.
//!
//! All methods are blocking background primitives. A `UI`-attached
//! [`OperationContext`] is rejected with `ErrorCode::InvalidInput` before any
//! side effect. Records are staged in the same directory, synced, then renamed
//! into place; a write or rename failure leaves the previously committed file
//! readable.

use crate::export::{DiagnosticExportBundle, OperationCapabilitySummary};
use crate::matrix::default_unavailable_evidence;
use crate::model::{
    BuildIdentity, CrashMarker, CrashRecord, CrashRecordSource, DIAGNOSTICS_FORMAT_VERSION,
    DiagnosticEvent, DiagnosticsOperation, PriorSessionSummary, PruneReport, QuarantineEntry,
    RecordOutcome, RecoveryReport, RetentionPolicy, SessionMarker, TerminationKind,
    UnavailableEvidence, now_millis,
};
use gpui_app_data::{AppData, DataLocation};
use gpui_system::{
    Availability, Capability, Completion, Configuration, ErrorCode, Evidence, OperationContext,
    PermissionState, RetryAdvice, ServiceResult, Support, SystemError,
};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::{
    fs,
    io::{self, Write},
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::Instant,
};

const NAMESPACE: &str = "diagnostics";
const EVENTS_FILE: &str = "events.json";
const CRASHES_FILE: &str = "crashes.json";
const SESSION_FILE: &str = "session.json";
const CRASH_MARKER_FILE: &str = "crash-marker.json";
const QUARANTINE_DIR: &str = "quarantine";
const MAX_PERSISTED_BYTES: u64 = 64 * 1024 * 1024;

static NEXT_SESSION: AtomicU64 = AtomicU64::new(1);

fn error(code: ErrorCode, message: &'static str) -> SystemError {
    SystemError::new(code, message)
}

fn io_error(cause: io::Error) -> SystemError {
    let code = match cause.kind() {
        io::ErrorKind::PermissionDenied => ErrorCode::PermissionDenied,
        io::ErrorKind::WouldBlock => ErrorCode::Busy,
        _ => ErrorCode::PlatformFailure,
    };
    let message = match cause.kind() {
        io::ErrorKind::StorageFull => "Diagnostics storage has insufficient space",
        io::ErrorKind::PermissionDenied => "Diagnostics storage permission was denied",
        io::ErrorKind::WouldBlock => "Diagnostics storage is busy",
        _ => "Diagnostics storage I/O failed",
    };
    let mut error = error(code, message);
    error.retry = RetryAdvice::AfterConditionsChange;
    error
}

fn completed<T>(value: T) -> ServiceResult<T> {
    Ok(Completion::Completed(value))
}

pub(crate) fn check_background<T>(
    context: &OperationContext,
) -> Result<Option<Completion<T>>, SystemError> {
    if context.ui.is_some() {
        return Err(error(
            ErrorCode::InvalidInput,
            "Diagnostics storage requires a background context without a UI attachment",
        ));
    }
    context.check_at(Instant::now())?;
    Ok(context.cancellation.reason().map(Completion::Cancelled))
}

fn lock<T>(value: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    value
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Deterministic id for the next app session. It is opaque, non-zero and never
/// derived from private data.
fn next_session_id() -> u64 {
    let sequence = NEXT_SESSION.fetch_add(1, Ordering::Relaxed) & 0x0fff;
    let time = now_millis().saturating_mul(4096);
    let id = time | sequence;
    if id == 0 {
        sequence.saturating_add(1)
    } else {
        id
    }
}

/// Injection point used by tests and hosts to simulate a write/rename failure
/// without touching real storage.
pub trait StorageHook: Send + Sync {
    /// Called after the staging file exists and before bytes are written.
    fn before_write(&self, file: &str) -> io::Result<()> {
        let _ = file;
        Ok(())
    }

    /// Called after the staging file is synced and immediately before rename.
    fn before_rename(&self, file: &str) -> io::Result<()> {
        let _ = file;
        Ok(())
    }
}

/// Default no-op storage hook.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoopStorageHook;

impl StorageHook for NoopStorageHook {}

/// One-shot fault injection for deterministic disk-full and rename tests.
#[derive(Debug, Default)]
pub struct StorageFaultInjection {
    fail_write: AtomicBool,
    fail_rename: AtomicBool,
}

impl StorageFaultInjection {
    pub fn new() -> Self {
        Self::default()
    }

    /// The next staging write fails with `ErrorKind::StorageFull`.
    pub fn fail_next_write(&self) {
        self.fail_write.store(true, Ordering::SeqCst);
    }

    /// The next staging-file rename fails with `ErrorKind::StorageFull`.
    pub fn fail_next_rename(&self) {
        self.fail_rename.store(true, Ordering::SeqCst);
    }

    pub fn clear(&self) {
        self.fail_write.store(false, Ordering::SeqCst);
        self.fail_rename.store(false, Ordering::SeqCst);
    }

    pub fn write_failure_pending(&self) -> bool {
        self.fail_write.load(Ordering::SeqCst)
    }

    pub fn rename_failure_pending(&self) -> bool {
        self.fail_rename.load(Ordering::SeqCst)
    }
}

/// Compatibility alias for a one-shot storage fault injector.
pub type StorageFault = StorageFaultInjection;

impl StorageHook for StorageFaultInjection {
    fn before_write(&self, _file: &str) -> io::Result<()> {
        if self.fail_write.swap(false, Ordering::SeqCst) {
            Err(io::Error::from(io::ErrorKind::StorageFull))
        } else {
            Ok(())
        }
    }

    fn before_rename(&self, _file: &str) -> io::Result<()> {
        if self.fail_rename.swap(false, Ordering::SeqCst) {
            Err(io::Error::from(io::ErrorKind::StorageFull))
        } else {
            Ok(())
        }
    }
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct EventsEnvelope {
    format: u32,
    schema: u32,
    evicted_events: u64,
    events: Vec<DiagnosticEvent>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct CrashesEnvelope {
    format: u32,
    schema: u32,
    evicted_crash_records: u64,
    crash_records: Vec<CrashRecord>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SessionEnvelope {
    format: u32,
    schema: u32,
    marker: SessionMarker,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct CrashMarkerEnvelope {
    format: u32,
    schema: u32,
    marker: CrashMarker,
}

trait StoredEnvelope {
    fn is_valid(&self) -> bool;
}

impl StoredEnvelope for EventsEnvelope {
    fn is_valid(&self) -> bool {
        self.format == DIAGNOSTICS_FORMAT_VERSION && self.schema == DIAGNOSTICS_FORMAT_VERSION
    }
}

impl StoredEnvelope for CrashesEnvelope {
    fn is_valid(&self) -> bool {
        self.format == DIAGNOSTICS_FORMAT_VERSION && self.schema == DIAGNOSTICS_FORMAT_VERSION
    }
}

impl StoredEnvelope for SessionEnvelope {
    fn is_valid(&self) -> bool {
        self.format == DIAGNOSTICS_FORMAT_VERSION
            && self.schema == DIAGNOSTICS_FORMAT_VERSION
            && self.marker.session_id != 0
    }
}

impl StoredEnvelope for CrashMarkerEnvelope {
    fn is_valid(&self) -> bool {
        self.format == DIAGNOSTICS_FORMAT_VERSION
            && self.schema == DIAGNOSTICS_FORMAT_VERSION
            && self.marker.session_id != 0
            && self.marker.termination.is_crash_marker()
    }
}

enum LoadState<T> {
    Missing,
    Loaded(T),
    Corrupt { bytes: u64 },
}

#[derive(Clone)]
struct StoreState {
    recovered: bool,
    events: Vec<DiagnosticEvent>,
    evicted_events: u64,
    crash_records: Vec<CrashRecord>,
    evicted_crash_records: u64,
    next_crash_sequence: u64,
    current_session: Option<SessionMarker>,
    prior_session: Option<PriorSessionSummary>,
    recovered_crash_records: Vec<CrashRecord>,
    quarantined: Vec<QuarantineEntry>,
}

impl Default for StoreState {
    fn default() -> Self {
        Self {
            recovered: false,
            events: Vec::new(),
            evicted_events: 0,
            crash_records: Vec::new(),
            evicted_crash_records: 0,
            next_crash_sequence: 1,
            current_session: None,
            prior_session: None,
            recovered_crash_records: Vec::new(),
            quarantined: Vec::new(),
        }
    }
}

impl StoreState {
    fn report(
        &self,
        identity: &BuildIdentity,
        unavailable: &[UnavailableEvidence],
    ) -> RecoveryReport {
        RecoveryReport {
            format_version: DIAGNOSTICS_FORMAT_VERSION,
            current_session_id: self
                .current_session
                .as_ref()
                .map_or(0, |marker| marker.session_id),
            build_identity: identity.clone(),
            prior_session: self.prior_session.clone(),
            recovered_crash_records: self.recovered_crash_records.clone(),
            retained_events: self.events.len(),
            evicted_events: self.evicted_events,
            retained_crash_records: self.crash_records.len(),
            evicted_crash_records: self.evicted_crash_records,
            quarantined_entries: self.quarantined.clone(),
            quarantined_count: self.quarantined.len(),
            unavailable_evidence: unavailable.to_vec(),
        }
    }
}

/// Blocking diagnostics persistence, retention and recovery store.
///
/// Clones share the same serialized state. All public operations require a
/// background [`OperationContext`].
/// Accept either an owned `AppData` clone or a borrowed service.
pub trait IntoAppData {
    fn into_app_data(self) -> AppData;
}

impl IntoAppData for AppData {
    fn into_app_data(self) -> AppData {
        self
    }
}

impl IntoAppData for &AppData {
    fn into_app_data(self) -> AppData {
        self.clone()
    }
}

impl IntoAppData for std::sync::Arc<AppData> {
    fn into_app_data(self) -> AppData {
        (*self).clone()
    }
}

#[derive(Clone)]
pub struct DiagnosticStore {
    app: AppData,
    directory: PathBuf,
    identity: BuildIdentity,
    retention: RetentionPolicy,
    hook: Arc<dyn StorageHook>,
    unavailable: Vec<UnavailableEvidence>,
    state: Arc<Mutex<StoreState>>,
    recovery_lock: Arc<Mutex<()>>,
}

impl DiagnosticStore {
    /// Open the diagnostics namespace without creating files or reading records.
    pub fn open(app: impl IntoAppData, identity: BuildIdentity) -> Result<Self, SystemError> {
        Self::open_with_options(
            app,
            identity,
            RetentionPolicy::default(),
            Arc::new(NoopStorageHook),
        )
    }

    /// Open with an explicit write/rename fault hook.
    pub fn open_with_hook(
        app: impl IntoAppData,
        identity: BuildIdentity,
        hook: Arc<dyn StorageHook>,
    ) -> Result<Self, SystemError> {
        Self::open_with_options(app, identity, RetentionPolicy::default(), hook)
    }

    /// Open with an explicit retention policy.
    pub fn open_with_retention(
        app: impl IntoAppData,
        identity: BuildIdentity,
        retention: RetentionPolicy,
    ) -> Result<Self, SystemError> {
        Self::open_with_options(app, identity, retention, Arc::new(NoopStorageHook))
    }

    /// Open with explicit retention and a storage fault-injection hook.
    pub fn open_with_options(
        app: impl IntoAppData,
        identity: BuildIdentity,
        retention: RetentionPolicy,
        hook: Arc<dyn StorageHook>,
    ) -> Result<Self, SystemError> {
        let app = app.into_app_data();
        let directory = app
            .location(DataLocation::Data)
            .child(NAMESPACE)
            .map_err(|_| {
                error(
                    ErrorCode::InvalidConfiguration,
                    "Diagnostics namespace is not a private storage path",
                )
            })?;
        Ok(Self {
            app,
            directory,
            identity: identity.sanitized(),
            retention,
            hook,
            unavailable: default_unavailable_evidence(),
            state: Arc::new(Mutex::new(StoreState::default())),
            recovery_lock: Arc::new(Mutex::new(())),
        })
    }

    /// Replace the unavailable-collector evidence reported by recovery/export.
    pub fn with_unavailable_evidence(
        mut self,
        evidence: impl IntoIterator<Item = UnavailableEvidence>,
    ) -> Self {
        self.unavailable = evidence.into_iter().collect();
        self
    }

    pub fn identity(&self) -> &BuildIdentity {
        &self.identity
    }

    pub fn unavailable_evidence(&self) -> &[UnavailableEvidence] {
        &self.unavailable
    }

    pub fn retention(&self) -> RetentionPolicy {
        self.retention
    }

    pub fn app_data(&self) -> &AppData {
        &self.app
    }

    /// The dedicated `diagnostics` namespace path. It is not part of public
    /// `Debug` output; tests and hosts may use it for qualification.
    pub fn namespace_path(&self) -> &Path {
        &self.directory
    }

    pub fn namespace(&self) -> &Path {
        &self.directory
    }

    pub fn current_session_id(&self) -> Option<u64> {
        lock(&self.state)
            .current_session
            .as_ref()
            .map(|marker| marker.session_id)
    }

    pub fn retained_events(&self) -> Vec<DiagnosticEvent> {
        lock(&self.state).events.clone()
    }

    pub fn retained_crash_records(&self) -> Vec<CrashRecord> {
        lock(&self.state).crash_records.clone()
    }

    pub fn evicted_events(&self) -> u64 {
        lock(&self.state).evicted_events
    }

    pub fn evicted_crash_records(&self) -> u64 {
        lock(&self.state).evicted_crash_records
    }

    pub fn is_recovered(&self) -> bool {
        lock(&self.state).recovered
    }

    /// Stable capability descriptions for a store-backed provider.
    pub fn capabilities(&self) -> Vec<Capability<DiagnosticsOperation>> {
        in_process_capabilities()
    }

    /// Recover persisted state and open a new active session marker.
    ///
    /// Idempotent: after the first successful call the same session remains
    /// active and the report reflects the current retained counts.
    pub fn recover(&self, context: &OperationContext) -> ServiceResult<RecoveryReport> {
        if let Some(cancelled) = check_background(context)? {
            return Ok(cancelled);
        }
        let _recovery_guard = lock(&self.recovery_lock);
        {
            let state = lock(&self.state);
            if state.recovered {
                return completed(state.report(&self.identity, &self.unavailable));
            }
        }
        if let Some(cancelled) = check_background(context)? {
            return Ok(cancelled);
        }

        let mut quarantined = Vec::new();
        let events = match self.load_envelope::<EventsEnvelope>(EVENTS_FILE)? {
            LoadState::Missing => EventsEnvelope {
                format: DIAGNOSTICS_FORMAT_VERSION,
                schema: DIAGNOSTICS_FORMAT_VERSION,
                evicted_events: 0,
                events: Vec::new(),
            },
            LoadState::Loaded(value) => value,
            LoadState::Corrupt { bytes } => {
                quarantined.push(self.quarantine(EVENTS_FILE, bytes)?);
                EventsEnvelope {
                    format: DIAGNOSTICS_FORMAT_VERSION,
                    schema: DIAGNOSTICS_FORMAT_VERSION,
                    evicted_events: 0,
                    events: Vec::new(),
                }
            }
        };
        let crashes = match self.load_envelope::<CrashesEnvelope>(CRASHES_FILE)? {
            LoadState::Missing => CrashesEnvelope {
                format: DIAGNOSTICS_FORMAT_VERSION,
                schema: DIAGNOSTICS_FORMAT_VERSION,
                evicted_crash_records: 0,
                crash_records: Vec::new(),
            },
            LoadState::Loaded(value) => value,
            LoadState::Corrupt { bytes } => {
                quarantined.push(self.quarantine(CRASHES_FILE, bytes)?);
                CrashesEnvelope {
                    format: DIAGNOSTICS_FORMAT_VERSION,
                    schema: DIAGNOSTICS_FORMAT_VERSION,
                    evicted_crash_records: 0,
                    crash_records: Vec::new(),
                }
            }
        };
        let session = match self.load_envelope::<SessionEnvelope>(SESSION_FILE)? {
            LoadState::Missing => None,
            LoadState::Loaded(value) => Some(value.marker),
            LoadState::Corrupt { bytes } => {
                quarantined.push(self.quarantine(SESSION_FILE, bytes)?);
                None
            }
        };
        let crash_marker = match self.load_envelope::<CrashMarkerEnvelope>(CRASH_MARKER_FILE)? {
            LoadState::Missing => None,
            LoadState::Loaded(value) => Some(value.marker),
            LoadState::Corrupt { bytes } => {
                quarantined.push(self.quarantine(CRASH_MARKER_FILE, bytes)?);
                None
            }
        };

        if let Some(cancelled) = check_background(context)? {
            return Ok(cancelled);
        }

        let mut event_list = events.events;
        let mut evicted_events = events.evicted_events;
        let events_changed =
            apply_event_retention(self.retention, &mut event_list, &mut evicted_events);

        let mut crash_list = crashes.crash_records;
        let mut evicted_crashes = crashes.evicted_crash_records;
        let mut next_sequence = crash_list
            .iter()
            .map(|record| record.sequence)
            .max()
            .unwrap_or(0)
            .saturating_add(1);

        let mut prior_session = None;
        let mut recovered_crash_records = Vec::new();
        let mut crashes_changed = false;
        let mut remove_crash_marker = false;
        let mut marker_consumed = false;

        if let Some(marker) = &session {
            let matching_marker = crash_marker
                .as_ref()
                .filter(|crash| crash.session_id == marker.session_id);
            if marker.clean {
                prior_session = Some(PriorSessionSummary {
                    session_id: marker.session_id,
                    build_identity: marker.build_identity.clone(),
                    clean: true,
                    termination: TerminationKind::CleanExit,
                    crash_marker_promoted: false,
                });
                if matching_marker.is_some() {
                    marker_consumed = true;
                    remove_crash_marker = true;
                }
            } else if let Some(crash) = matching_marker {
                marker_consumed = true;
                remove_crash_marker = true;
                let already_recorded = crash_list.iter().any(|record| {
                    record.session_id == crash.session_id
                        && record.recorded_at_millis == crash.recorded_at_millis
                        && record.source == CrashRecordSource::CrashMarker
                });
                if !already_recorded {
                    let record = CrashRecord::new(
                        next_sequence,
                        crash.session_id,
                        crash.build_identity.clone(),
                        crash.recorded_at_millis,
                        crash.termination.clone(),
                        CrashRecordSource::CrashMarker,
                    );
                    next_sequence = next_sequence.saturating_add(1);
                    recovered_crash_records.push(record.clone());
                    crash_list.push(record);
                    crashes_changed = true;
                }
                prior_session = Some(PriorSessionSummary {
                    session_id: marker.session_id,
                    build_identity: marker.build_identity.clone(),
                    clean: false,
                    termination: crash.termination.clone(),
                    crash_marker_promoted: !already_recorded,
                });
            } else {
                let existing = crash_list
                    .iter()
                    .find(|record| {
                        record.session_id == marker.session_id
                            && (matches!(record.termination, TerminationKind::UncleanShutdown(_))
                                || matches!(
                                    record.source,
                                    CrashRecordSource::CrashMarker
                                        | CrashRecordSource::OrphanMarker
                                ))
                    })
                    .cloned();
                let termination = existing.as_ref().map_or_else(
                    || {
                        TerminationKind::unclean_shutdown(
                            marker.session_id,
                            marker.build_identity.clone(),
                        )
                    },
                    |record| record.termination.clone(),
                );
                if existing.is_none() {
                    let record = CrashRecord::new(
                        next_sequence,
                        marker.session_id,
                        marker.build_identity.clone(),
                        now_millis(),
                        termination.clone(),
                        CrashRecordSource::UncleanShutdown,
                    );
                    next_sequence = next_sequence.saturating_add(1);
                    recovered_crash_records.push(record.clone());
                    crash_list.push(record);
                    crashes_changed = true;
                }
                prior_session = Some(PriorSessionSummary {
                    session_id: marker.session_id,
                    build_identity: marker.build_identity.clone(),
                    clean: false,
                    termination,
                    crash_marker_promoted: existing.as_ref().is_some_and(|record| {
                        matches!(
                            record.source,
                            CrashRecordSource::CrashMarker | CrashRecordSource::OrphanMarker
                        )
                    }),
                });
            }
        }

        if !marker_consumed && let Some(crash) = &crash_marker {
            let already_recorded = crash_list.iter().any(|record| {
                record.session_id == crash.session_id
                    && record.recorded_at_millis == crash.recorded_at_millis
                    && matches!(
                        record.source,
                        CrashRecordSource::CrashMarker | CrashRecordSource::OrphanMarker
                    )
            });
            if !already_recorded {
                let record = CrashRecord::new(
                    next_sequence,
                    crash.session_id,
                    crash.build_identity.clone(),
                    crash.recorded_at_millis,
                    crash.termination.clone(),
                    CrashRecordSource::OrphanMarker,
                );
                next_sequence = next_sequence.saturating_add(1);
                recovered_crash_records.push(record.clone());
                crash_list.push(record);
                crashes_changed = true;
            }
            remove_crash_marker = true;
        }

        if !recovered_crash_records.is_empty() {
            crashes_changed = true;
        }
        if apply_crash_retention(self.retention, &mut crash_list, &mut evicted_crashes) {
            crashes_changed = true;
        }

        if let Some(cancelled) = check_background(context)? {
            return Ok(cancelled);
        }

        if crashes_changed {
            self.write_envelope(
                CRASHES_FILE,
                &CrashesEnvelope {
                    format: DIAGNOSTICS_FORMAT_VERSION,
                    schema: DIAGNOSTICS_FORMAT_VERSION,
                    evicted_crash_records: evicted_crashes,
                    crash_records: crash_list.clone(),
                },
            )?;
        }
        if remove_crash_marker {
            self.remove_file(CRASH_MARKER_FILE)?;
        }
        if events_changed {
            self.write_envelope(
                EVENTS_FILE,
                &EventsEnvelope {
                    format: DIAGNOSTICS_FORMAT_VERSION,
                    schema: DIAGNOSTICS_FORMAT_VERSION,
                    evicted_events,
                    events: event_list.clone(),
                },
            )?;
        }

        let session_marker =
            SessionMarker::new(next_session_id(), self.identity.clone(), now_millis());
        self.write_envelope(
            SESSION_FILE,
            &SessionEnvelope {
                format: DIAGNOSTICS_FORMAT_VERSION,
                schema: DIAGNOSTICS_FORMAT_VERSION,
                marker: session_marker.clone(),
            },
        )?;

        let report = {
            let mut state = lock(&self.state);
            state.recovered = true;
            state.events = event_list;
            state.evicted_events = evicted_events;
            state.crash_records = crash_list;
            state.evicted_crash_records = evicted_crashes;
            state.next_crash_sequence = next_sequence;
            state.current_session = Some(session_marker);
            state.prior_session = prior_session;
            state.recovered_crash_records = recovered_crash_records;
            state.quarantined = quarantined;
            state.report(&self.identity, &self.unavailable)
        };
        completed(report)
    }

    /// Append one event under bounded retention. A failed persistence write
    /// rolls back the in-memory append and leaves the committed log intact.
    pub fn record_event(
        &self,
        event: DiagnosticEvent,
        context: &OperationContext,
    ) -> ServiceResult<RecordOutcome> {
        if let Some(cancelled) = check_background(context)? {
            return Ok(cancelled);
        }
        match self.ensure_recovered(context)? {
            Completion::Completed(()) => {}
            Completion::Cancelled(reason) => return Ok(Completion::Cancelled(reason)),
        }
        if let Some(cancelled) = check_background(context)? {
            return Ok(cancelled);
        }

        let mut state = lock(&self.state);
        if state.current_session.is_none() {
            return Err(error(
                ErrorCode::InvalidConfiguration,
                "Diagnostics session has not been recovered",
            ));
        }
        let mut events = state.events.clone();
        let mut evicted = state.evicted_events;
        events.push(event.clone());
        apply_event_retention(self.retention, &mut events, &mut evicted);
        let stored = events.last().is_some_and(|candidate| candidate == &event);
        if let Some(cancelled) = check_background(context)? {
            return Ok(cancelled);
        }
        self.write_envelope(
            EVENTS_FILE,
            &EventsEnvelope {
                format: DIAGNOSTICS_FORMAT_VERSION,
                schema: DIAGNOSTICS_FORMAT_VERSION,
                evicted_events: evicted,
                events: events.clone(),
            },
        )?;
        state.events = events;
        state.evicted_events = evicted;
        completed(RecordOutcome {
            event,
            stored,
            evicted_events: evicted,
        })
    }

    /// Persist a crash marker for the active session. Only the four external
    /// crash evidence kinds are accepted; lifecycle conclusions are rejected.
    pub fn write_crash_marker(
        &self,
        termination: TerminationKind,
        context: &OperationContext,
    ) -> ServiceResult<()> {
        if let Some(cancelled) = check_background(context)? {
            return Ok(cancelled);
        }
        if !termination.is_crash_marker() {
            return Err(error(
                ErrorCode::InvalidInput,
                "Crash markers require RustPanic, AndroidAnr, NativeCrash or OsTermination evidence",
            ));
        }
        match self.ensure_recovered(context)? {
            Completion::Completed(()) => {}
            Completion::Cancelled(reason) => return Ok(Completion::Cancelled(reason)),
        }
        if let Some(cancelled) = check_background(context)? {
            return Ok(cancelled);
        }
        let session_id = {
            let state = lock(&self.state);
            state
                .current_session
                .as_ref()
                .map(|marker| marker.session_id)
                .ok_or_else(|| {
                    error(
                        ErrorCode::InvalidConfiguration,
                        "Diagnostics session has not been recovered",
                    )
                })?
        };
        let marker = CrashMarker::new(session_id, self.identity.clone(), now_millis(), termination);
        if let Some(cancelled) = check_background(context)? {
            return Ok(cancelled);
        }
        self.write_envelope(
            CRASH_MARKER_FILE,
            &CrashMarkerEnvelope {
                format: DIAGNOSTICS_FORMAT_VERSION,
                schema: DIAGNOSTICS_FORMAT_VERSION,
                marker,
            },
        )?;
        completed(())
    }

    /// Mark the active session clean so a later launch does not synthesize an
    /// `UncleanShutdown` record.
    pub fn mark_clean_exit(&self, context: &OperationContext) -> ServiceResult<()> {
        if let Some(cancelled) = check_background(context)? {
            return Ok(cancelled);
        }
        match self.ensure_recovered(context)? {
            Completion::Completed(()) => {}
            Completion::Cancelled(reason) => return Ok(Completion::Cancelled(reason)),
        }
        if let Some(cancelled) = check_background(context)? {
            return Ok(cancelled);
        }
        let mut state = lock(&self.state);
        let Some(current) = state.current_session.clone() else {
            return Err(error(
                ErrorCode::InvalidConfiguration,
                "Diagnostics session has not been recovered",
            ));
        };
        if current.clean {
            return completed(());
        }
        let marker = SessionMarker {
            clean: true,
            ..current
        };
        if let Some(cancelled) = check_background(context)? {
            return Ok(cancelled);
        }
        self.write_envelope(
            SESSION_FILE,
            &SessionEnvelope {
                format: DIAGNOSTICS_FORMAT_VERSION,
                schema: DIAGNOSTICS_FORMAT_VERSION,
                marker: marker.clone(),
            },
        )?;
        state.current_session = Some(marker);
        drop(state);
        // The session marker takes precedence; removing a stale crash marker is
        // best effort. A later recovery ignores it when the session is clean.
        let _ = self.remove_file(CRASH_MARKER_FILE);
        completed(())
    }

    /// Apply the configured retention policy and report cumulative evictions.
    pub fn prune_retained(&self, context: &OperationContext) -> ServiceResult<PruneReport> {
        if let Some(cancelled) = check_background(context)? {
            return Ok(cancelled);
        }
        match self.ensure_recovered(context)? {
            Completion::Completed(()) => {}
            Completion::Cancelled(reason) => return Ok(Completion::Cancelled(reason)),
        }
        if let Some(cancelled) = check_background(context)? {
            return Ok(cancelled);
        }
        let mut state = lock(&self.state);
        let mut events = state.events.clone();
        let mut evicted_events = state.evicted_events;
        let events_changed =
            apply_event_retention(self.retention, &mut events, &mut evicted_events);
        let mut crash_records = state.crash_records.clone();
        let mut evicted_crashes = state.evicted_crash_records;
        let crashes_changed =
            apply_crash_retention(self.retention, &mut crash_records, &mut evicted_crashes);
        if let Some(cancelled) = check_background(context)? {
            return Ok(cancelled);
        }
        if events_changed {
            self.write_envelope(
                EVENTS_FILE,
                &EventsEnvelope {
                    format: DIAGNOSTICS_FORMAT_VERSION,
                    schema: DIAGNOSTICS_FORMAT_VERSION,
                    evicted_events,
                    events: events.clone(),
                },
            )?;
        }
        if crashes_changed {
            self.write_envelope(
                CRASHES_FILE,
                &CrashesEnvelope {
                    format: DIAGNOSTICS_FORMAT_VERSION,
                    schema: DIAGNOSTICS_FORMAT_VERSION,
                    evicted_crash_records: evicted_crashes,
                    crash_records: crash_records.clone(),
                },
            )?;
        }
        state.events = events;
        state.evicted_events = evicted_events;
        state.crash_records = crash_records;
        state.evicted_crash_records = evicted_crashes;
        completed(PruneReport {
            evicted_events,
            evicted_crash_records: evicted_crashes,
            retained_events: state.events.len(),
            retained_crash_records: state.crash_records.len(),
        })
    }

    /// Build the versioned export bundle. Staging is the caller's responsibility.
    pub fn export_bundle(
        &self,
        context: &OperationContext,
    ) -> ServiceResult<DiagnosticExportBundle> {
        if let Some(cancelled) = check_background(context)? {
            return Ok(cancelled);
        }
        match self.ensure_recovered(context)? {
            Completion::Completed(()) => {}
            Completion::Cancelled(reason) => return Ok(Completion::Cancelled(reason)),
        }
        if let Some(cancelled) = check_background(context)? {
            return Ok(cancelled);
        }
        let (report, events, crash_records) = {
            let state = lock(&self.state);
            (
                state.report(&self.identity, &self.unavailable),
                state.events.clone(),
                state.crash_records.clone(),
            )
        };
        if let Some(cancelled) = check_background(context)? {
            return Ok(cancelled);
        }
        let capabilities = self
            .capabilities()
            .iter()
            .map(OperationCapabilitySummary::from_capability)
            .collect();
        completed(DiagnosticExportBundle::new(
            self.identity.clone(),
            report.current_session_id,
            capabilities,
            self.retention,
            report,
            events,
            crash_records,
        ))
    }

    fn ensure_recovered(&self, context: &OperationContext) -> Result<Completion<()>, SystemError> {
        if lock(&self.state).recovered {
            return Ok(Completion::Completed(()));
        }
        match self.recover(context)? {
            Completion::Completed(_) => Ok(Completion::Completed(())),
            Completion::Cancelled(reason) => Ok(Completion::Cancelled(reason)),
        }
    }

    fn load_envelope<T>(&self, name: &str) -> Result<LoadState<T>, SystemError>
    where
        T: DeserializeOwned + StoredEnvelope,
    {
        let path = self.directory.join(name);
        let metadata = match fs::symlink_metadata(&path) {
            Ok(metadata) => metadata,
            Err(cause) if cause.kind() == io::ErrorKind::NotFound => {
                return Ok(LoadState::Missing);
            }
            Err(cause) => return Err(io_error(cause)),
        };
        if !metadata.is_file() || metadata.file_type().is_symlink() {
            return Ok(LoadState::Corrupt {
                bytes: metadata.len(),
            });
        }
        if metadata.len() > MAX_PERSISTED_BYTES {
            return Ok(LoadState::Corrupt {
                bytes: metadata.len(),
            });
        }
        let bytes = match fs::read(&path) {
            Ok(bytes) => bytes,
            Err(cause) if cause.kind() == io::ErrorKind::NotFound => {
                return Ok(LoadState::Missing);
            }
            Err(cause) => return Err(io_error(cause)),
        };
        match serde_json::from_slice::<T>(&bytes) {
            Ok(value) if value.is_valid() => Ok(LoadState::Loaded(value)),
            Ok(_) | Err(_) => Ok(LoadState::Corrupt {
                bytes: bytes.len() as u64,
            }),
        }
    }

    fn quarantine(&self, file_name: &str, bytes: u64) -> Result<QuarantineEntry, SystemError> {
        let directory = self.directory.join(QUARANTINE_DIR);
        fs::create_dir_all(&directory).map_err(io_error)?;
        let source = self.directory.join(file_name);
        let base = format!("{file_name}.{}", now_millis());
        let mut stored_name = base.clone();
        let mut suffix = 0u64;
        while directory.join(&stored_name).exists() {
            suffix = suffix.saturating_add(1);
            stored_name = format!("{base}.{suffix}");
        }
        fs::rename(&source, directory.join(&stored_name)).map_err(io_error)?;
        Ok(QuarantineEntry::new(file_name, stored_name, bytes))
    }

    fn write_envelope<T: Serialize>(&self, name: &str, value: &T) -> Result<(), SystemError> {
        let bytes = serde_json::to_vec(value).map_err(|_| {
            error(
                ErrorCode::IntegrityFailure,
                "Diagnostics record cannot be encoded",
            )
        })?;
        fs::create_dir_all(&self.directory).map_err(io_error)?;
        let mut staging = tempfile::Builder::new()
            .prefix(".diagnostics-")
            .tempfile_in(&self.directory)
            .map_err(io_error)?;
        self.hook.before_write(name).map_err(io_error)?;
        staging.write_all(&bytes).map_err(io_error)?;
        staging.as_file().sync_all().map_err(io_error)?;
        self.hook.before_rename(name).map_err(io_error)?;
        staging
            .persist(self.directory.join(name))
            .map_err(|failure| io_error(failure.error))?;
        Ok(())
    }

    fn remove_file(&self, name: &str) -> Result<(), SystemError> {
        match fs::remove_file(self.directory.join(name)) {
            Ok(()) => Ok(()),
            Err(cause) if cause.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(cause) => Err(io_error(cause)),
        }
    }
}

impl std::fmt::Debug for DiagnosticStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let state = lock(&self.state);
        f.debug_struct("DiagnosticStore")
            .field("build_identity", &self.identity)
            .field("retention", &self.retention)
            .field("recovered", &state.recovered)
            .field("events", &state.events.len())
            .field("crash_records", &state.crash_records.len())
            .finish()
    }
}

/// Capability snapshots for the in-process provider.
pub fn in_process_capabilities() -> Vec<Capability<DiagnosticsOperation>> {
    DiagnosticsOperation::ALL
        .into_iter()
        .map(|operation| Capability {
            operation,
            support: Support::Native,
            availability: Availability::Available,
            configuration: Configuration::Ready,
            permission: PermissionState::NotRequired,
            evidence: Evidence::Compiled,
            constraints: Vec::new(),
            observed_revision: 1,
        })
        .collect()
}

fn apply_event_retention(
    retention: RetentionPolicy,
    events: &mut Vec<DiagnosticEvent>,
    evicted: &mut u64,
) -> bool {
    let mut changed = false;
    while events.len() > retention.max_events {
        events.remove(0);
        *evicted = evicted.saturating_add(1);
        changed = true;
    }
    let mut sizes: Vec<usize> = events
        .iter()
        .map(|event| serde_json::to_vec(event).map_or(0, |bytes| bytes.len()))
        .collect();
    let mut total: usize = sizes.iter().sum();
    while total > retention.max_byte_budget && !events.is_empty() {
        events.remove(0);
        total = total.saturating_sub(sizes.remove(0));
        *evicted = evicted.saturating_add(1);
        changed = true;
    }
    changed
}

fn apply_crash_retention(
    retention: RetentionPolicy,
    records: &mut Vec<CrashRecord>,
    evicted: &mut u64,
) -> bool {
    let mut changed = false;
    while records.len() > retention.max_crash_records {
        records.remove(0);
        *evicted = evicted.saturating_add(1);
        changed = true;
    }
    changed
}

/// Remove abandoned `.diagnostics-*` staging files inside the namespace. Only
/// regular files are touched; committed records and quarantine entries remain.
pub fn cleanup_staging_files(store: &DiagnosticStore) -> Result<usize, SystemError> {
    let directory = store.namespace_path();
    let entries = match fs::read_dir(directory) {
        Ok(entries) => entries,
        Err(cause) if cause.kind() == io::ErrorKind::NotFound => return Ok(0),
        Err(cause) => return Err(io_error(cause)),
    };
    let mut removed = 0usize;
    for entry in entries {
        let entry = entry.map_err(io_error)?;
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if !name.starts_with(".diagnostics-") {
            continue;
        }
        let metadata = fs::symlink_metadata(entry.path()).map_err(io_error)?;
        if metadata.is_file() && !metadata.file_type().is_symlink() {
            fs::remove_file(entry.path()).map_err(io_error)?;
            removed += 1;
        }
    }
    Ok(removed)
}
