//! Structured diagnostics, crash records, bounded retention and recoverable
//! export for `gpui-box`.
//!
//! The default build has no GPUI dependency. Every free-form string is
//! deterministically redacted at construction or ingestion time. Blocking
//! storage requires a background [`gpui_system::OperationContext`]; a
//! UI-attached context is rejected with `ErrorCode::InvalidInput` before any
//! side effect. The optional `gpui` feature adds the `App` global adapter.
//!
//! See the repository's `docs/S_DIAGNOSTICS.md` for the persistence,
//! quarantine, export, FFI containment and six-target qualification contracts.

mod export;
mod matrix;
mod model;
mod provider;
mod redact;
mod service;
mod store;

pub use export::{
    DIAGNOSTIC_EXPORT_FORMAT, DiagnosticExportBundle, OperationCapabilitySummary, PlatformMetadata,
};
pub use matrix::{
    PlatformOperationSupport, PlatformSupport, PlatformTarget, QualificationEvidence,
    UnresolvedHostAdapter, default_unavailable_evidence, operation_support, platform_support,
    platform_support_matrix, unresolved_host_adapters,
};
pub use model::{
    AndroidAnrEvidence, BuildIdentity, Correlation, CorrelationContext, CrashMarker, CrashRecord,
    CrashRecordSource, DIAGNOSTIC_EXPORT_SCHEMA_VERSION, DIAGNOSTICS_FORMAT_VERSION,
    DiagnosticDomain, DiagnosticEvent, DiagnosticField, DiagnosticSeverity, DiagnosticsOperation,
    MAX_EXPORT_BYTES, NativeCrashEvidence, OsTerminationEvidence, PACKAGE_VERSION, PanicEvidence,
    PriorSessionSummary, PruneReport, QuarantineEntry, RecordOutcome, RecoveryReport, RedactedText,
    RedactedValue, RetentionPolicy, SessionMarker, TerminationKind, UPSTREAM_GPUI_REVISION,
    UnavailableEvidence, UncleanShutdownEvidence, current_target_triple, now_millis,
};
pub use provider::{
    DiagnosticsProvider, FakeDiagnosticsProvider, InProcessDiagnosticsProvider, PROVIDER_MANIFEST,
};
pub use redact::{
    REDACTED_PREFIX, RedactionKind, normalize_key, redact_text, redaction_marker,
    sensitive_key_kind,
};
pub use service::{
    DiagnosticServiceEvent, DiagnosticsService, fake_service, in_process_service,
    unavailable_service,
};
pub use store::{
    DiagnosticStore, IntoAppData, NoopStorageHook, StorageFault, StorageFaultInjection,
    StorageHook, cleanup_staging_files, in_process_capabilities,
};

pub use gpui_app_data::AppData;
pub use gpui_files::ExportResourceLease;
pub use gpui_system::{
    Availability, CancelReason, Capability, Completion, Configuration, Constraint, ErrorCode,
    Event, EventEnvelope, EventSubscription, Evidence, OperationContext, PermissionState, Platform,
    ProviderManifest, RecoveryAction, Request, RetryAdvice, ServiceResult, Support, SurfaceId,
    SystemError, SystemServices, UiContext, UiOwner,
};

#[cfg(feature = "gpui")]
pub mod gpui_adapter;
