//! Per-operation platform matrix and host-adapter qualification links.
//!
//! The matrix is a contract, not runtime proof. Native collectors, signal
//! handoffs, Android JNI bridges and Apple/Web host integrations remain separate
//! platform work; each row names the configuration the host must supply.

use crate::model::DiagnosticsOperation;
use serde::{Deserialize, Serialize};
use std::fmt;

/// The six targets covered by the diagnostics support matrix.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlatformTarget {
    Windows,
    MacOs,
    Linux,
    Android,
    Ios,
    Web,
}

impl PlatformTarget {
    pub const ALL: [Self; 6] = [
        Self::Windows,
        Self::MacOs,
        Self::Linux,
        Self::Android,
        Self::Ios,
        Self::Web,
    ];

    /// The compile-time target running this code, if it is one of the matrix rows.
    pub const fn current() -> Option<Self> {
        if cfg!(target_arch = "wasm32") {
            Some(Self::Web)
        } else if cfg!(target_os = "windows") {
            Some(Self::Windows)
        } else if cfg!(target_os = "macos") {
            Some(Self::MacOs)
        } else if cfg!(target_os = "linux") {
            Some(Self::Linux)
        } else if cfg!(target_os = "android") {
            Some(Self::Android)
        } else if cfg!(target_os = "ios") {
            Some(Self::Ios)
        } else {
            None
        }
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Windows => "windows",
            Self::MacOs => "macos",
            Self::Linux => "linux",
            Self::Android => "android",
            Self::Ios => "ios",
            Self::Web => "web",
        }
    }
}

impl fmt::Display for PlatformTarget {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Serialization-friendly support level for the platform matrix.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlatformSupport {
    Native,
    Conditional,
    FallbackAvailable,
    Unsupported,
    Unknown,
}

impl PlatformSupport {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Native => "native",
            Self::Conditional => "conditional",
            Self::FallbackAvailable => "fallback_available",
            Self::Unsupported => "unsupported",
            Self::Unknown => "unknown",
        }
    }

    pub const fn is_supported(self) -> bool {
        matches!(self, Self::Native | Self::Conditional)
    }

    pub const fn from_system(support: gpui_system::Support) -> Self {
        match support {
            gpui_system::Support::Native => Self::Native,
            gpui_system::Support::Conditional => Self::Conditional,
            gpui_system::Support::FallbackAvailable => Self::FallbackAvailable,
            gpui_system::Support::Unsupported => Self::Unsupported,
            gpui_system::Support::Unknown => Self::Unknown,
        }
    }

    pub const fn to_system(self) -> gpui_system::Support {
        match self {
            Self::Native => gpui_system::Support::Native,
            Self::Conditional => gpui_system::Support::Conditional,
            Self::FallbackAvailable => gpui_system::Support::FallbackAvailable,
            Self::Unsupported => gpui_system::Support::Unsupported,
            Self::Unknown => gpui_system::Support::Unknown,
        }
    }
}

impl From<PlatformSupport> for gpui_system::Support {
    fn from(value: PlatformSupport) -> Self {
        value.to_system()
    }
}

impl fmt::Display for PlatformSupport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Qualification evidence for one matrix row. A compile is not runtime proof.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QualificationEvidence {
    ContractOnly,
    Compiled,
    RuntimeVerified,
}

impl QualificationEvidence {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ContractOnly => "contract_only",
            Self::Compiled => "compiled",
            Self::RuntimeVerified => "runtime_verified",
        }
    }

    pub const fn from_system(evidence: gpui_system::Evidence) -> Self {
        match evidence {
            gpui_system::Evidence::ContractOnly => Self::ContractOnly,
            gpui_system::Evidence::Compiled => Self::Compiled,
            gpui_system::Evidence::RuntimeVerified => Self::RuntimeVerified,
        }
    }

    pub const fn to_system(self) -> gpui_system::Evidence {
        match self {
            Self::ContractOnly => gpui_system::Evidence::ContractOnly,
            Self::Compiled => gpui_system::Evidence::Compiled,
            Self::RuntimeVerified => gpui_system::Evidence::RuntimeVerified,
        }
    }
}

impl From<QualificationEvidence> for gpui_system::Evidence {
    fn from(value: QualificationEvidence) -> Self {
        value.to_system()
    }
}

impl fmt::Display for QualificationEvidence {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// One `(platform, operation)` contract row.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlatformOperationSupport {
    pub platform: PlatformTarget,
    pub operation: DiagnosticsOperation,
    #[serde(with = "system_support")]
    pub support: gpui_system::Support,
    #[serde(deserialize_with = "crate::model::de_redacted_string")]
    pub required_host_configuration: String,
    #[serde(with = "system_evidence")]
    pub evidence: gpui_system::Evidence,
    #[serde(deserialize_with = "crate::model::de_redacted_string")]
    pub notes: String,
    #[serde(deserialize_with = "crate::model::de_redacted_strings")]
    pub qualification_links: Vec<String>,
}

impl PlatformOperationSupport {
    pub fn new(
        platform: PlatformTarget,
        operation: DiagnosticsOperation,
        support: impl Into<gpui_system::Support>,
        required_host_configuration: &str,
        notes: &str,
        evidence: impl Into<gpui_system::Evidence>,
        qualification_links: &[&str],
    ) -> Self {
        Self {
            platform,
            operation,
            support: support.into(),
            required_host_configuration: required_host_configuration.to_owned(),
            evidence: evidence.into(),
            notes: notes.to_owned(),
            qualification_links: qualification_links
                .iter()
                .map(|link| (*link).to_owned())
                .collect(),
        }
    }

    pub const fn is_supported(&self) -> bool {
        matches!(
            self.support,
            gpui_system::Support::Native | gpui_system::Support::Conditional
        )
    }

    pub const fn support_level(&self) -> PlatformSupport {
        PlatformSupport::from_system(self.support)
    }

    pub const fn evidence_level(&self) -> QualificationEvidence {
        QualificationEvidence::from_system(self.evidence)
    }
}

/// A host adapter or qualification gap that remains unresolved.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UnresolvedHostAdapter {
    #[serde(deserialize_with = "crate::model::de_redacted_string")]
    pub link: String,
    #[serde(deserialize_with = "crate::model::de_redacted_string")]
    pub description: String,
}

impl UnresolvedHostAdapter {
    pub fn new(link: &str, description: &str) -> Self {
        Self {
            link: link.to_owned(),
            description: description.to_owned(),
        }
    }
}

mod system_support {
    use gpui_system::Support;
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S>(value: &Support, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let text = match value {
            Support::Native => "native",
            Support::Conditional => "conditional",
            Support::FallbackAvailable => "fallback_available",
            Support::Unsupported => "unsupported",
            Support::Unknown => "unknown",
        };
        serializer.serialize_str(text)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Support, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        match value.as_str() {
            "native" => Ok(Support::Native),
            "conditional" => Ok(Support::Conditional),
            "fallback_available" => Ok(Support::FallbackAvailable),
            "unsupported" => Ok(Support::Unsupported),
            "unknown" => Ok(Support::Unknown),
            other => Err(serde::de::Error::custom(format!(
                "unknown support level {other}"
            ))),
        }
    }
}

mod system_evidence {
    use gpui_system::Evidence;
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S>(value: &Evidence, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let text = match value {
            Evidence::ContractOnly => "contract_only",
            Evidence::Compiled => "compiled",
            Evidence::RuntimeVerified => "runtime_verified",
        };
        serializer.serialize_str(text)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Evidence, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        match value.as_str() {
            "contract_only" => Ok(Evidence::ContractOnly),
            "compiled" => Ok(Evidence::Compiled),
            "runtime_verified" => Ok(Evidence::RuntimeVerified),
            other => Err(serde::de::Error::custom(format!(
                "unknown evidence level {other}"
            ))),
        }
    }
}

const LINKS_DESKTOP: &[&str] = &["#8", "#19", "#43"];
const LINKS_WINDOWS: &[&str] = &["#2", "#8", "#19", "#43"];
const LINKS_ANDROID: &[&str] = &["#2", "#19", "#22", "#43"];
const LINKS_IOS: &[&str] = &["#9", "#19", "#43"];
const LINKS_WEB: &[&str] = &["#19", "#22", "#43", "#73"];

const CONFIG_DESKTOP: &str = "Open gpui-app-data with an explicit private root and run every diagnostics operation on a background OperationContext.";
const CONFIG_CRASH_DESKTOP: &str = "Install a panic hook and an SEH/signal-safe boundary at Rust entrypoints; capture the marker without unwinding across a foreign ABI.";
const CONFIG_ANDROID: &str = "Inject an app-private Android container root through the host; never infer a desktop path. Diagnostics storage is blocking background work.";
const CONFIG_CRASH_ANDROID: &str = "Forward ANR, tombstone and ApplicationExitInfo evidence through the JNI bridge; capture panics at Rust entrypoints and never unwind across JNI.";
const CONFIG_IOS: &str = "Inject an app-private iOS container root through the host. Diagnostics storage is blocking background work.";
const CONFIG_CRASH_IOS: &str = "Hand off MetricKit diagnostics and signal-safe markers from the Swift/C entrypoint; never unwind across the Swift or C ABI.";
const CONFIG_WEB: &str = "The web target has no filesystem AppData backend; a host bridge must provide ephemeral or IndexedDB-backed storage and explicit flush semantics.";
const CONFIG_EXPORT_WEB: &str = "ExportBundle requires a host download handoff. This crate cannot stage a filesystem ExportResourceLease in a browser or WebWorker.";

fn row(
    platform: PlatformTarget,
    operation: DiagnosticsOperation,
    support: PlatformSupport,
    configuration: &str,
    evidence: QualificationEvidence,
    notes: &str,
    links: &[&str],
) -> PlatformOperationSupport {
    PlatformOperationSupport::new(
        platform,
        operation,
        support,
        configuration,
        notes,
        evidence,
        links,
    )
}

/// The complete six-target, five-operation support matrix (30 rows).
pub fn platform_support_matrix() -> Vec<PlatformOperationSupport> {
    let mut rows = Vec::with_capacity(30);
    for (platform, configuration, crash_configuration, evidence, links) in [
        (
            PlatformTarget::Windows,
            CONFIG_DESKTOP,
            CONFIG_CRASH_DESKTOP,
            QualificationEvidence::Compiled,
            LINKS_WINDOWS,
        ),
        (
            PlatformTarget::MacOs,
            CONFIG_DESKTOP,
            CONFIG_CRASH_DESKTOP,
            QualificationEvidence::Compiled,
            LINKS_DESKTOP,
        ),
        (
            PlatformTarget::Linux,
            CONFIG_DESKTOP,
            CONFIG_CRASH_DESKTOP,
            QualificationEvidence::Compiled,
            LINKS_DESKTOP,
        ),
        (
            PlatformTarget::Android,
            CONFIG_ANDROID,
            CONFIG_CRASH_ANDROID,
            QualificationEvidence::Compiled,
            LINKS_ANDROID,
        ),
        (
            PlatformTarget::Ios,
            CONFIG_IOS,
            CONFIG_CRASH_IOS,
            QualificationEvidence::Compiled,
            LINKS_IOS,
        ),
        (
            PlatformTarget::Web,
            CONFIG_WEB,
            CONFIG_WEB,
            QualificationEvidence::ContractOnly,
            LINKS_WEB,
        ),
    ] {
        rows.push(row(
            platform,
            DiagnosticsOperation::RecordEvent,
            if platform == PlatformTarget::Web {
                PlatformSupport::Conditional
            } else {
                PlatformSupport::Native
            },
            configuration,
            evidence,
            "Events are structured, correlated and deterministically redacted at ingestion.",
            links,
        ));
        rows.push(row(
            platform,
            DiagnosticsOperation::WriteCrashMarker,
            if platform == PlatformTarget::Web {
                PlatformSupport::Unsupported
            } else {
                PlatformSupport::Conditional
            },
            crash_configuration,
            evidence,
            "Writes only host-supplied RustPanic, AndroidAnr, NativeCrash or OsTermination evidence; CleanExit and UncleanShutdown are lifecycle results, not crash markers.",
            links,
        ));
        rows.push(row(
            platform,
            DiagnosticsOperation::InspectRecovery,
            if platform == PlatformTarget::Web {
                PlatformSupport::Conditional
            } else {
                PlatformSupport::Native
            },
            configuration,
            evidence,
            "Recovery adopts a prior crash marker or synthesizes UncleanShutdown, quarantines corrupt files and reports unavailable collectors.",
            links,
        ));
        rows.push(row(
            platform,
            DiagnosticsOperation::ExportBundle,
            if platform == PlatformTarget::Web {
                PlatformSupport::Unsupported
            } else if matches!(platform, PlatformTarget::Android | PlatformTarget::Ios) {
                PlatformSupport::Conditional
            } else {
                PlatformSupport::Native
            },
            if platform == PlatformTarget::Web {
                CONFIG_EXPORT_WEB
            } else if matches!(platform, PlatformTarget::Android | PlatformTarget::Ios) {
                "Stage a versioned bundle through gpui-files and hand the ExportResourceLease to the host share sheet or document picker."
            } else {
                CONFIG_DESKTOP
            },
            evidence,
            if platform == PlatformTarget::Web {
                "A browser/WebWorker has no filesystem ExportResourceLease; ExportBundle requires a host download handoff and explicit flush semantics."
            } else if matches!(platform, PlatformTarget::Android | PlatformTarget::Ios) {
                "The versioned JSON bundle is staged through gpui-files and handed to the host document picker or share sheet; the lease remains pinned until release."
            } else {
                "The versioned JSON bundle contains build identity, recovery/retention summary, redacted events and crash records, plus unavailable-evidence markers."
            },
            &["#23", "#19", "#43"],
        ));
        rows.push(row(
            platform,
            DiagnosticsOperation::PruneRetained,
            if platform == PlatformTarget::Web {
                PlatformSupport::Conditional
            } else {
                PlatformSupport::Native
            },
            configuration,
            evidence,
            "Applies max_events, max_byte_budget and max_crash_records deterministically and increments cumulative eviction counters.",
            links,
        ));
    }
    rows
}

/// Look up one exact `(platform, operation)` row.
pub fn operation_support(
    platform: PlatformTarget,
    operation: DiagnosticsOperation,
) -> PlatformOperationSupport {
    platform_support_matrix()
        .into_iter()
        .find(|row| row.platform == platform && row.operation == operation)
        .expect("the support matrix contains every platform/operation pair")
}

/// All five operations for one platform in declaration order.
pub fn platform_support(platform: PlatformTarget) -> Vec<PlatformOperationSupport> {
    DiagnosticsOperation::ALL
        .into_iter()
        .map(|operation| operation_support(platform, operation))
        .collect()
}

/// Host adapters and qualification evidence that are not resolved by this crate.
pub fn unresolved_host_adapters() -> Vec<UnresolvedHostAdapter> {
    vec![
        UnresolvedHostAdapter::new(
            "#2",
            "Windows/Android first executable path and packaged-host acceptance.",
        ),
        UnresolvedHostAdapter::new(
            "#8",
            "macOS/Linux desktop hardware qualification for renderer and process boundaries.",
        ),
        UnresolvedHostAdapter::new("#9", "iOS device and MetricKit qualification."),
        UnresolvedHostAdapter::new(
            "#19",
            "Shared system-service contracts, capability snapshots and error vocabulary.",
        ),
        UnresolvedHostAdapter::new(
            "#22",
            "Android JNI host bridge for ANR, tombstone and ApplicationExitInfo evidence.",
        ),
        UnresolvedHostAdapter::new(
            "#23",
            "Authorized export leases and native share-sheet/document-picker handoff.",
        ),
        UnresolvedHostAdapter::new(
            "#43",
            "Background execution, durable retention and cancellation policy.",
        ),
        UnresolvedHostAdapter::new(
            "#73",
            "Cross-platform diagnostics qualification and follow-up collector coordination.",
        ),
    ]
}

/// Evidence for collectors/signals that this revision cannot observe.
pub fn default_unavailable_evidence() -> Vec<crate::model::UnavailableEvidence> {
    use crate::model::UnavailableEvidence;
    let mut evidence = vec![
        UnavailableEvidence::new(
            "native-signal-collector",
            "No in-process SEH/signal/Mach exception collector is bundled; the host entrypoint must capture crash markers without unwinding across a foreign ABI.",
        )
        .for_operation(DiagnosticsOperation::WriteCrashMarker),
        UnavailableEvidence::new(
            "jvm-anr-bridge",
            "ANR, tombstone and ApplicationExitInfo evidence is only available through the Android JNI host bridge (#22).",
        )
        .with_platform(PlatformTarget::Android)
        .for_operation(DiagnosticsOperation::WriteCrashMarker),
        UnavailableEvidence::new(
            "metrickit-crash-diagnostics",
            "MetricKit crash diagnostics require an iOS host handoff (#9); they are not synthesized by this crate.",
        )
        .with_platform(PlatformTarget::Ios)
        .for_operation(DiagnosticsOperation::WriteCrashMarker),
        UnavailableEvidence::new(
            "web-filesystem-export",
            "A browser/WebWorker has no filesystem AppData or ExportResourceLease; export requires a host download bridge.",
        )
        .with_platform(PlatformTarget::Web)
        .for_operation(DiagnosticsOperation::ExportBundle),
    ];
    if let Some(current) = PlatformTarget::current() {
        evidence.push(
            UnavailableEvidence::new(
                "runtime-renderer-trace",
                "Renderer frame-timing and GPU trace collectors are host/backend-specific and are not installed by the diagnostics contract crate.",
            )
            .with_platform(current)
            .for_operation(DiagnosticsOperation::RecordEvent),
        );
    }
    evidence
}
