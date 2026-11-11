//! Versioned, recoverable diagnostic export bundle.

use crate::matrix::{
    PlatformOperationSupport, PlatformTarget, UnresolvedHostAdapter, platform_support_matrix,
    unresolved_host_adapters,
};
use crate::model::{
    BuildIdentity, CrashRecord, DIAGNOSTIC_EXPORT_SCHEMA_VERSION, DIAGNOSTICS_FORMAT_VERSION,
    DiagnosticEvent, DiagnosticsOperation, RecoveryReport, RetentionPolicy, UnavailableEvidence,
    now_millis,
};
use serde::{Deserialize, Serialize};

/// Stable export format identifier.
pub const DIAGNOSTIC_EXPORT_FORMAT: &str = "gpui-box.diagnostics";

/// OS/architecture metadata for the exporting session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlatformMetadata {
    pub target: Option<PlatformTarget>,
    #[serde(deserialize_with = "crate::model::de_redacted_string")]
    pub os: String,
    #[serde(deserialize_with = "crate::model::de_redacted_string")]
    pub arch: String,
    #[serde(deserialize_with = "crate::model::de_redacted_string")]
    pub family: String,
}

impl PlatformMetadata {
    pub fn current() -> Self {
        Self {
            target: PlatformTarget::current(),
            os: std::env::consts::OS.to_owned(),
            arch: std::env::consts::ARCH.to_owned(),
            family: std::env::consts::FAMILY.to_owned(),
        }
    }
}

/// Serialization-friendly capability summary for every diagnostics operation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OperationCapabilitySummary {
    pub operation: DiagnosticsOperation,
    #[serde(deserialize_with = "crate::model::de_redacted_string")]
    pub support: String,
    #[serde(deserialize_with = "crate::model::de_redacted_string")]
    pub availability: String,
    #[serde(deserialize_with = "crate::model::de_redacted_string")]
    pub configuration: String,
    #[serde(deserialize_with = "crate::model::de_redacted_string")]
    pub permission: String,
    #[serde(deserialize_with = "crate::model::de_redacted_string")]
    pub evidence: String,
    #[serde(deserialize_with = "crate::model::de_redacted_strings")]
    pub constraints: Vec<String>,
    pub observed_revision: u64,
}

impl OperationCapabilitySummary {
    pub fn from_capability(capability: &gpui_system::Capability<DiagnosticsOperation>) -> Self {
        Self {
            operation: capability.operation,
            support: format!("{:?}", capability.support),
            availability: format!("{:?}", capability.availability),
            configuration: format!("{:?}", capability.configuration),
            permission: format!("{:?}", capability.permission),
            evidence: format!("{:?}", capability.evidence),
            constraints: capability
                .constraints
                .iter()
                .map(|constraint| format!("{constraint:?}"))
                .collect(),
            observed_revision: capability.observed_revision,
        }
    }
}

/// Fully self-describing, versioned diagnostic report exported through
/// `gpui_files::ExportResourceLease`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DiagnosticExportBundle {
    pub format: String,
    pub format_version: u32,
    pub schema_version: u32,
    pub generated_at_millis: u64,
    pub build_identity: BuildIdentity,
    pub session_id: u64,
    pub platform: PlatformMetadata,
    pub capabilities: Vec<OperationCapabilitySummary>,
    pub platform_matrix: Vec<PlatformOperationSupport>,
    pub unresolved_host_adapters: Vec<UnresolvedHostAdapter>,
    pub retention: RetentionPolicy,
    pub recovery: RecoveryReport,
    pub events: Vec<DiagnosticEvent>,
    pub crash_records: Vec<CrashRecord>,
    pub unavailable_evidence: Vec<UnavailableEvidence>,
}

impl DiagnosticExportBundle {
    pub fn new(
        build_identity: BuildIdentity,
        session_id: u64,
        capabilities: Vec<OperationCapabilitySummary>,
        retention: RetentionPolicy,
        recovery: RecoveryReport,
        events: Vec<DiagnosticEvent>,
        crash_records: Vec<CrashRecord>,
    ) -> Self {
        let unavailable_evidence = recovery.unavailable_evidence.clone();
        Self {
            format: DIAGNOSTIC_EXPORT_FORMAT.to_owned(),
            format_version: DIAGNOSTICS_FORMAT_VERSION,
            schema_version: DIAGNOSTIC_EXPORT_SCHEMA_VERSION,
            generated_at_millis: now_millis(),
            build_identity: build_identity.sanitized(),
            session_id,
            platform: PlatformMetadata::current(),
            capabilities,
            platform_matrix: platform_support_matrix(),
            unresolved_host_adapters: unresolved_host_adapters(),
            retention,
            recovery,
            events,
            crash_records,
            unavailable_evidence,
        }
    }

    pub fn is_current_schema(&self) -> bool {
        self.format == DIAGNOSTIC_EXPORT_FORMAT
            && self.format_version == DIAGNOSTICS_FORMAT_VERSION
            && self.schema_version == DIAGNOSTIC_EXPORT_SCHEMA_VERSION
    }

    pub fn platform_rows(&self) -> usize {
        self.platform_matrix.len()
    }

    pub fn has_all_platforms(&self) -> bool {
        PlatformTarget::ALL.iter().all(|platform| {
            DiagnosticsOperation::ALL.iter().all(|operation| {
                self.platform_matrix
                    .iter()
                    .any(|row| row.platform == *platform && row.operation == *operation)
            })
        })
    }

    pub fn unsupported_matrix_rows(&self) -> Vec<&PlatformOperationSupport> {
        self.platform_matrix
            .iter()
            .filter(|row| !row.is_supported())
            .collect()
    }

    pub fn evidence_for(&self, operation: DiagnosticsOperation) -> Option<gpui_system::Evidence> {
        self.platform_matrix
            .iter()
            .find(|row| row.operation == operation)
            .map(|row| row.evidence)
    }
}
