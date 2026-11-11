//! Public diagnostic vocabulary: correlation, redacted values, crash and
//! termination evidence, retention policy and recovery reports.
//!
//! Every free-form string is sanitized by [`crate::redact::redact_text`] at
//! construction or deserialization time. Public `Debug` implementations report
//! structured summaries rather than potentially private payload text.

use crate::redact::{RedactionKind, normalize_key, redact_text, sensitive_key_kind};
use gpui_system::SurfaceId;
use serde::{Deserialize, Deserializer, Serialize, Serializer, de::Error as _};
use std::{
    fmt,
    time::{SystemTime, UNIX_EPOCH},
};

/// Package version aligned with the workspace `0.1.0-dev` identity.
pub const PACKAGE_VERSION: &str = env!("CARGO_PKG_VERSION");
/// Upstream GPUI revision checked into this distribution.
pub const UPSTREAM_GPUI_REVISION: &str = "f8cfc7ed9e57503def9479138894f3139f5400cc";
/// On-disk and export format version.
pub const DIAGNOSTICS_FORMAT_VERSION: u32 = 1;
/// Version of the exported diagnostic report schema.
pub const DIAGNOSTIC_EXPORT_SCHEMA_VERSION: u32 = 1;
/// Hard upper bound for one staged export bundle.
pub const MAX_EXPORT_BYTES: usize = 8 * 1024 * 1024;

/// Milliseconds since the Unix epoch, saturating instead of panicking.
pub fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis().min(u128::from(u64::MAX)) as u64)
        .unwrap_or(0)
}

/// A conservative target triple for metadata when Cargo did not expose one.
pub fn current_target_triple() -> String {
    if let Some(target) = option_env!("TARGET") {
        return redact_text(target);
    }
    let architecture = std::env::consts::ARCH;
    let suffix = match std::env::consts::OS {
        "linux" => "unknown-linux-gnu",
        "windows" => "pc-windows-msvc",
        "macos" => "apple-darwin",
        "android" => "linux-android",
        "ios" => "apple-ios",
        "freebsd" => "unknown-freebsd",
        other => other,
    };
    format!("{architecture}-{suffix}")
}

/// Exact build metadata attached to events, crash records and exports.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BuildIdentity {
    #[serde(deserialize_with = "de_redacted_string")]
    pub package_version: String,
    #[serde(deserialize_with = "de_redacted_string")]
    pub upstream_gpui_revision: String,
    #[serde(deserialize_with = "de_redacted_string")]
    pub target_triple: String,
    #[serde(deserialize_with = "de_redacted_string")]
    pub profile: String,
    #[serde(deserialize_with = "de_redacted_string")]
    pub channel: String,
    #[serde(deserialize_with = "de_redacted_string")]
    pub build_id: String,
    #[serde(deserialize_with = "de_redacted_string")]
    pub symbol_id: String,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "de_redacted_optional_string"
    )]
    pub source_revision: Option<String>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "de_redacted_optional_string"
    )]
    pub toolchain: Option<String>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "de_redacted_optional_string"
    )]
    pub os: Option<String>,
}

impl BuildIdentity {
    /// Construct an identity and sanitize every free-form component.
    pub fn new(
        package_version: impl Into<String>,
        upstream_gpui_revision: impl Into<String>,
        target_triple: impl Into<String>,
        profile: impl Into<String>,
        channel: impl Into<String>,
        build_id: impl Into<String>,
        symbol_id: impl Into<String>,
    ) -> Self {
        Self {
            package_version: redact_text(&package_version.into()),
            upstream_gpui_revision: redact_text(&upstream_gpui_revision.into()),
            target_triple: redact_text(&target_triple.into()),
            profile: redact_text(&profile.into()),
            channel: redact_text(&channel.into()),
            build_id: redact_text(&build_id.into()),
            symbol_id: redact_text(&symbol_id.into()),
            source_revision: None,
            toolchain: None,
            os: None,
        }
    }

    /// The distribution version and upstream GPUI revision with test/build metadata.
    pub fn for_current_build(
        profile: impl Into<String>,
        channel: impl Into<String>,
        build_id: impl Into<String>,
        symbol_id: impl Into<String>,
    ) -> Self {
        let mut identity = Self::new(
            PACKAGE_VERSION,
            UPSTREAM_GPUI_REVISION,
            current_target_triple(),
            profile,
            channel,
            build_id,
            symbol_id,
        );
        identity.os = Some(redact_text(std::env::consts::OS));
        identity.toolchain = Some(redact_text(&format!(
            "rustc-{}",
            option_env!("CARGO_PKG_RUST_VERSION").unwrap_or("unknown")
        )));
        identity
    }

    /// Default metadata for the current distribution build.
    pub fn current() -> Self {
        Self::for_current_build(
            if cfg!(debug_assertions) {
                "dev"
            } else {
                "release"
            },
            "dev",
            "local",
            "none",
        )
    }

    pub fn with_source_revision(mut self, revision: impl Into<String>) -> Self {
        self.source_revision = Some(redact_text(&revision.into()));
        self
    }

    pub fn with_toolchain(mut self, toolchain: impl Into<String>) -> Self {
        self.toolchain = Some(redact_text(&toolchain.into()));
        self
    }

    pub fn with_os(mut self, os: impl Into<String>) -> Self {
        self.os = Some(redact_text(&os.into()));
        self
    }

    pub fn sanitized(mut self) -> Self {
        self.package_version = redact_text(&self.package_version);
        self.upstream_gpui_revision = redact_text(&self.upstream_gpui_revision);
        self.target_triple = redact_text(&self.target_triple);
        self.profile = redact_text(&self.profile);
        self.channel = redact_text(&self.channel);
        self.build_id = redact_text(&self.build_id);
        self.symbol_id = redact_text(&self.symbol_id);
        self.source_revision = self.source_revision.map(|value| redact_text(&value));
        self.toolchain = self.toolchain.map(|value| redact_text(&value));
        self.os = self.os.map(|value| redact_text(&value));
        self
    }
}

/// Which subsystem emitted an event.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiagnosticDomain {
    Command,
    NativeRequest,
    HostGeneration,
    UiCommit,
    Renderer,
    Background,
}

impl DiagnosticDomain {
    pub const ALL: [Self; 6] = [
        Self::Command,
        Self::NativeRequest,
        Self::HostGeneration,
        Self::UiCommit,
        Self::Renderer,
        Self::Background,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Command => "command",
            Self::NativeRequest => "native_request",
            Self::HostGeneration => "host_generation",
            Self::UiCommit => "ui_commit",
            Self::Renderer => "renderer",
            Self::Background => "background",
        }
    }
}

impl fmt::Display for DiagnosticDomain {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Event severity. `Critical` is reserved for process-termination evidence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiagnosticSeverity {
    Trace,
    Debug,
    Info,
    Warning,
    Error,
    Critical,
}

impl DiagnosticSeverity {
    pub const ALL: [Self; 6] = [
        Self::Trace,
        Self::Debug,
        Self::Info,
        Self::Warning,
        Self::Error,
        Self::Critical,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Trace => "trace",
            Self::Debug => "debug",
            Self::Info => "info",
            Self::Warning => "warning",
            Self::Error => "error",
            Self::Critical => "critical",
        }
    }
}

impl fmt::Display for DiagnosticSeverity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

fn serialize_surface<S>(surface: &Option<SurfaceId>, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    surface.map(|surface| surface.0).serialize(serializer)
}

fn deserialize_surface<'de, D>(deserializer: D) -> Result<Option<SurfaceId>, D::Error>
where
    D: Deserializer<'de>,
{
    Option::<u64>::deserialize(deserializer).map(|value| value.map(SurfaceId))
}

/// Correlation identifiers shared by commands, native requests, host
/// generations, UI commits, renderer work and background jobs.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CorrelationContext {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command_id: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request_id: Option<u64>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        serialize_with = "serialize_surface",
        deserialize_with = "deserialize_surface"
    )]
    pub surface: Option<SurfaceId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub surface_generation: Option<u64>,
    /// Scene or UI commit revision observed by the emitter.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revision: Option<u64>,
    /// Worker or background job identity.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub job_id: Option<u64>,
}

impl CorrelationContext {
    pub const fn new() -> Self {
        Self {
            command_id: None,
            request_id: None,
            surface: None,
            surface_generation: None,
            revision: None,
            job_id: None,
        }
    }

    pub const fn with_command_id(mut self, command_id: u64) -> Self {
        self.command_id = Some(command_id);
        self
    }

    pub const fn with_request_id(mut self, request_id: u64) -> Self {
        self.request_id = Some(request_id);
        self
    }

    pub const fn with_surface(mut self, surface: SurfaceId) -> Self {
        self.surface = Some(surface);
        self
    }

    pub const fn with_surface_generation(mut self, generation: u64) -> Self {
        self.surface_generation = Some(generation);
        self
    }

    pub const fn with_revision(mut self, revision: u64) -> Self {
        self.revision = Some(revision);
        self
    }

    pub const fn with_job_id(mut self, job_id: u64) -> Self {
        self.job_id = Some(job_id);
        self
    }

    pub const fn is_correlated(self) -> bool {
        self.command_id.is_some()
            || self.request_id.is_some()
            || self.surface.is_some()
            || self.job_id.is_some()
    }
}

/// Alias for the correlation vocabulary used by native and background code.
pub type Correlation = CorrelationContext;

/// A string that has already passed deterministic redaction.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct RedactedText(String);

impl RedactedText {
    pub fn new(value: impl Into<String>) -> Self {
        Self(redact_text(&value.into()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn into_string(self) -> String {
        self.0
    }
}

impl fmt::Debug for RedactedText {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("RedactedText").field(&self.0).finish()
    }
}

impl fmt::Display for RedactedText {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl From<&str> for RedactedText {
    fn from(value: &str) -> Self {
        Self::new(value)
    }
}

impl From<String> for RedactedText {
    fn from(value: String) -> Self {
        Self::new(value)
    }
}

impl<'de> Deserialize<'de> for RedactedText {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        String::deserialize(deserializer).map(Self::new)
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum RawRedactedValue {
    Text(String),
    Integer(i64),
    Unsigned(u64),
    Float(f64),
    Boolean(bool),
    Null,
    List(Vec<RawRedactedValue>),
    Object(Vec<RawDiagnosticField>),
}

#[derive(Deserialize)]
struct RawDiagnosticField {
    key: String,
    value: RawRedactedValue,
}

/// A structured, already-redacted diagnostic value.
#[derive(Clone, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RedactedValue {
    Text(RedactedText),
    Integer(i64),
    Unsigned(u64),
    Float(f64),
    Boolean(bool),
    Null,
    List(Vec<RedactedValue>),
    Object(Vec<DiagnosticField>),
}

impl RedactedValue {
    pub fn text(value: impl Into<String>) -> Self {
        Self::Text(RedactedText::new(value))
    }

    pub fn list(values: Vec<RedactedValue>) -> Self {
        Self::List(
            values
                .into_iter()
                .map(RedactedValue::sanitize_deep)
                .collect(),
        )
    }

    pub fn object(fields: Vec<DiagnosticField>) -> Self {
        Self::Object(
            fields
                .into_iter()
                .map(DiagnosticField::sanitize_deep)
                .collect(),
        )
    }

    pub fn as_text(&self) -> Option<&str> {
        match self {
            Self::Text(value) => Some(value.as_str()),
            _ => None,
        }
    }

    pub fn is_redacted_marker(&self) -> bool {
        self.as_text()
            .is_some_and(|text| text.starts_with(crate::redact::REDACTED_PREFIX))
    }

    pub(crate) fn sanitize_deep(self) -> Self {
        match self {
            Self::Text(value) => Self::Text(RedactedText::new(value.as_str())),
            Self::List(values) => Self::List(
                values
                    .into_iter()
                    .map(RedactedValue::sanitize_deep)
                    .collect(),
            ),
            Self::Object(fields) => Self::Object(
                fields
                    .into_iter()
                    .map(DiagnosticField::sanitize_deep)
                    .collect(),
            ),
            other => other,
        }
    }

    fn from_raw(raw: RawRedactedValue) -> Self {
        match raw {
            RawRedactedValue::Text(value) => Self::text(value),
            RawRedactedValue::Integer(value) => Self::Integer(value),
            RawRedactedValue::Unsigned(value) => Self::Unsigned(value),
            RawRedactedValue::Float(value) => Self::Float(value),
            RawRedactedValue::Boolean(value) => Self::Boolean(value),
            RawRedactedValue::Null => Self::Null,
            RawRedactedValue::List(values) => {
                Self::list(values.into_iter().map(Self::from_raw).collect())
            }
            RawRedactedValue::Object(fields) => Self::object(
                fields
                    .into_iter()
                    .map(|field| DiagnosticField::new(field.key, Self::from_raw(field.value)))
                    .collect(),
            ),
        }
    }
}

impl fmt::Debug for RedactedValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Text(value) => f.debug_tuple("Text").field(value).finish(),
            Self::Integer(value) => f.debug_tuple("Integer").field(value).finish(),
            Self::Unsigned(value) => f.debug_tuple("Unsigned").field(value).finish(),
            Self::Float(value) => f.debug_tuple("Float").field(value).finish(),
            Self::Boolean(value) => f.debug_tuple("Boolean").field(value).finish(),
            Self::Null => f.write_str("Null"),
            Self::List(values) => f.debug_struct("List").field("len", &values.len()).finish(),
            Self::Object(fields) => f
                .debug_struct("Object")
                .field("len", &fields.len())
                .finish(),
        }
    }
}

impl<'de> Deserialize<'de> for RedactedValue {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        RawRedactedValue::deserialize(deserializer).map(Self::from_raw)
    }
}

impl From<&str> for RedactedValue {
    fn from(value: &str) -> Self {
        Self::text(value)
    }
}

impl From<String> for RedactedValue {
    fn from(value: String) -> Self {
        Self::text(value)
    }
}

impl From<&String> for RedactedValue {
    fn from(value: &String) -> Self {
        Self::text(value)
    }
}

impl From<RedactedText> for RedactedValue {
    fn from(value: RedactedText) -> Self {
        Self::Text(value)
    }
}

impl From<bool> for RedactedValue {
    fn from(value: bool) -> Self {
        Self::Boolean(value)
    }
}

impl From<i64> for RedactedValue {
    fn from(value: i64) -> Self {
        Self::Integer(value)
    }
}

impl From<i32> for RedactedValue {
    fn from(value: i32) -> Self {
        Self::Integer(i64::from(value))
    }
}

impl From<u64> for RedactedValue {
    fn from(value: u64) -> Self {
        Self::Unsigned(value)
    }
}

impl From<u32> for RedactedValue {
    fn from(value: u32) -> Self {
        Self::Unsigned(u64::from(value))
    }
}

impl From<usize> for RedactedValue {
    fn from(value: usize) -> Self {
        Self::Unsigned(value as u64)
    }
}

impl From<f64> for RedactedValue {
    fn from(value: f64) -> Self {
        Self::Float(value)
    }
}

impl From<Vec<RedactedValue>> for RedactedValue {
    fn from(value: Vec<RedactedValue>) -> Self {
        Self::list(value)
    }
}

impl From<Vec<DiagnosticField>> for RedactedValue {
    fn from(value: Vec<DiagnosticField>) -> Self {
        Self::object(value)
    }
}

impl From<()> for RedactedValue {
    fn from((): ()) -> Self {
        Self::Null
    }
}

/// One named diagnostic field. A sensitive key replaces the whole value with a
/// stable `[REDACTED:<kind>]` marker; non-sensitive values are redacted deeply.
#[derive(Clone, PartialEq, Serialize)]
pub struct DiagnosticField {
    key: String,
    value: RedactedValue,
    redaction: Option<RedactionKind>,
}

impl DiagnosticField {
    pub fn new(key: impl Into<String>, value: impl Into<RedactedValue>) -> Self {
        let raw_key = key.into();
        let redaction = sensitive_key_kind(&raw_key);
        let key = redact_text(&raw_key);
        let value = match redaction {
            Some(kind) => RedactedValue::text(kind.marker()),
            None => value.into().sanitize_deep(),
        };
        Self {
            key,
            value,
            redaction,
        }
    }

    pub fn text(key: impl Into<String>, value: impl Into<String>) -> Self {
        Self::new(key, RedactedValue::text(value))
    }

    pub fn key(&self) -> &str {
        &self.key
    }

    pub fn value(&self) -> &RedactedValue {
        &self.value
    }

    pub fn redaction(&self) -> Option<RedactionKind> {
        self.redaction
    }

    pub fn normalized_key(&self) -> String {
        normalize_key(&self.key)
    }

    pub(crate) fn sanitize_deep(self) -> Self {
        Self::new(self.key, self.value)
    }
}

impl fmt::Debug for DiagnosticField {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DiagnosticField")
            .field("key", &self.key)
            .field("value", &self.value)
            .field("redaction", &self.redaction)
            .finish()
    }
}

impl<'de> Deserialize<'de> for DiagnosticField {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let raw = RawDiagnosticField::deserialize(deserializer)?;
        Ok(Self::new(raw.key, RedactedValue::from_raw(raw.value)))
    }
}

/// One correlated structured diagnostic event.
#[derive(Clone, PartialEq, Serialize)]
pub struct DiagnosticEvent {
    pub domain: DiagnosticDomain,
    pub severity: DiagnosticSeverity,
    pub correlation: CorrelationContext,
    pub message: RedactedValue,
    pub fields: Vec<DiagnosticField>,
}

impl DiagnosticEvent {
    pub fn new(
        domain: DiagnosticDomain,
        severity: DiagnosticSeverity,
        message: impl Into<RedactedValue>,
    ) -> Self {
        Self {
            domain,
            severity,
            correlation: CorrelationContext::default(),
            message: message.into().sanitize_deep(),
            fields: Vec::new(),
        }
    }

    pub fn with_correlation(mut self, correlation: CorrelationContext) -> Self {
        self.correlation = correlation;
        self
    }

    pub fn with_field(mut self, key: impl Into<String>, value: impl Into<RedactedValue>) -> Self {
        self.fields.push(DiagnosticField::new(key, value));
        self
    }

    pub fn with_text(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.fields.push(DiagnosticField::text(key, value));
        self
    }

    pub fn with_fields(mut self, fields: Vec<DiagnosticField>) -> Self {
        self.fields
            .extend(fields.into_iter().map(DiagnosticField::sanitize_deep));
        self
    }

    pub fn field(&self, key: &str) -> Option<&DiagnosticField> {
        self.fields.iter().find(|field| field.key() == key)
    }
}

impl fmt::Debug for DiagnosticEvent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DiagnosticEvent")
            .field("domain", &self.domain)
            .field("severity", &self.severity)
            .field("correlation", &self.correlation)
            .field("message", &self.message)
            .field("fields", &self.fields.len())
            .finish()
    }
}

#[derive(Deserialize)]
struct RawDiagnosticEvent {
    domain: DiagnosticDomain,
    severity: DiagnosticSeverity,
    #[serde(default)]
    correlation: CorrelationContext,
    message: RawRedactedValue,
    #[serde(default)]
    fields: Vec<RawDiagnosticField>,
}

impl<'de> Deserialize<'de> for DiagnosticEvent {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let raw = RawDiagnosticEvent::deserialize(deserializer)?;
        Ok(Self {
            domain: raw.domain,
            severity: raw.severity,
            correlation: raw.correlation,
            message: RedactedValue::from_raw(raw.message).sanitize_deep(),
            fields: raw
                .fields
                .into_iter()
                .map(|field| DiagnosticField::new(field.key, RedactedValue::from_raw(field.value)))
                .collect(),
        })
    }
}

/// Evidence attached to a Rust panic or panic hook.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PanicEvidence {
    #[serde(deserialize_with = "de_redacted_string")]
    message: String,
    #[serde(
        default,
        deserialize_with = "de_redacted_optional_string",
        skip_serializing_if = "Option::is_none"
    )]
    location: Option<String>,
    #[serde(default, deserialize_with = "de_redacted_strings")]
    frames: Vec<String>,
}

impl PanicEvidence {
    pub fn new(message: impl Into<String>, location: Option<String>, frames: Vec<String>) -> Self {
        Self {
            message: redact_text(&message.into()),
            location: location.map(|value| redact_text(&value)),
            frames: redact_strings(frames),
        }
    }

    pub fn message(&self) -> &str {
        &self.message
    }

    pub fn location(&self) -> Option<&str> {
        self.location.as_deref()
    }

    pub fn frames(&self) -> &[String] {
        &self.frames
    }
}

impl fmt::Debug for PanicEvidence {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PanicEvidence")
            .field("message_bytes", &self.message.len())
            .field("has_location", &self.location.is_some())
            .field("frames", &self.frames.len())
            .finish()
    }
}

/// Evidence from an Android ANR, tombstone or `ApplicationExitInfo` bridge.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AndroidAnrEvidence {
    #[serde(deserialize_with = "de_redacted_string")]
    reason: String,
    #[serde(default, deserialize_with = "de_redacted_strings")]
    frames: Vec<String>,
    #[serde(
        default,
        deserialize_with = "de_redacted_optional_string",
        skip_serializing_if = "Option::is_none"
    )]
    tombstone: Option<String>,
}

impl AndroidAnrEvidence {
    pub fn new(reason: impl Into<String>, frames: Vec<String>) -> Self {
        Self {
            reason: redact_text(&reason.into()),
            frames: redact_strings(frames),
            tombstone: None,
        }
    }

    pub fn with_tombstone(mut self, tombstone: impl Into<String>) -> Self {
        self.tombstone = Some(redact_text(&tombstone.into()));
        self
    }

    pub fn reason(&self) -> &str {
        &self.reason
    }

    pub fn frames(&self) -> &[String] {
        &self.frames
    }

    pub fn tombstone(&self) -> Option<&str> {
        self.tombstone.as_deref()
    }
}

impl fmt::Debug for AndroidAnrEvidence {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AndroidAnrEvidence")
            .field("reason_bytes", &self.reason.len())
            .field("frames", &self.frames.len())
            .field("has_tombstone", &self.tombstone.is_some())
            .finish()
    }
}

/// Evidence from a native signal, SEH exception or platform crash reporter.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NativeCrashEvidence {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    signal: Option<i32>,
    #[serde(
        default,
        deserialize_with = "de_redacted_optional_string",
        skip_serializing_if = "Option::is_none"
    )]
    exception_type: Option<String>,
    #[serde(default, deserialize_with = "de_redacted_strings")]
    frames: Vec<String>,
    #[serde(
        default,
        deserialize_with = "de_redacted_optional_string",
        skip_serializing_if = "Option::is_none"
    )]
    detail: Option<String>,
}

impl NativeCrashEvidence {
    pub fn new(signal: Option<i32>, exception_type: Option<String>, frames: Vec<String>) -> Self {
        Self {
            signal,
            exception_type: exception_type.map(|value| redact_text(&value)),
            frames: redact_strings(frames),
            detail: None,
        }
    }

    pub fn with_detail(mut self, detail: impl Into<String>) -> Self {
        self.detail = Some(redact_text(&detail.into()));
        self
    }

    pub fn signal(&self) -> Option<i32> {
        self.signal
    }

    pub fn exception_type(&self) -> Option<&str> {
        self.exception_type.as_deref()
    }

    pub fn frames(&self) -> &[String] {
        &self.frames
    }

    pub fn detail(&self) -> Option<&str> {
        self.detail.as_deref()
    }
}

impl fmt::Debug for NativeCrashEvidence {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("NativeCrashEvidence")
            .field("signal", &self.signal)
            .field("has_exception_type", &self.exception_type.is_some())
            .field("frames", &self.frames.len())
            .field("has_detail", &self.detail.is_some())
            .finish()
    }
}

/// Evidence from an operating-system termination or exit-reason bridge.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OsTerminationEvidence {
    #[serde(deserialize_with = "de_redacted_string")]
    reason: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    exit_code: Option<i32>,
    #[serde(default, deserialize_with = "de_redacted_strings")]
    exit_reasons: Vec<String>,
}

impl OsTerminationEvidence {
    pub fn new(reason: impl Into<String>, exit_code: Option<i32>) -> Self {
        Self {
            reason: redact_text(&reason.into()),
            exit_code,
            exit_reasons: Vec::new(),
        }
    }

    pub fn with_exit_reasons(mut self, reasons: Vec<String>) -> Self {
        self.exit_reasons = redact_strings(reasons);
        self
    }

    pub fn reason(&self) -> &str {
        &self.reason
    }

    pub fn exit_code(&self) -> Option<i32> {
        self.exit_code
    }

    pub fn exit_reasons(&self) -> &[String] {
        &self.exit_reasons
    }
}

impl fmt::Debug for OsTerminationEvidence {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("OsTerminationEvidence")
            .field("reason_bytes", &self.reason.len())
            .field("exit_code", &self.exit_code)
            .field("exit_reasons", &self.exit_reasons.len())
            .finish()
    }
}

/// Evidence synthesized from an unclean prior session.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UncleanShutdownEvidence {
    pub prior_session_id: u64,
    pub build_identity: BuildIdentity,
    #[serde(default, deserialize_with = "de_redacted_optional_string")]
    detail: Option<String>,
}

impl UncleanShutdownEvidence {
    pub fn new(prior_session_id: u64, build_identity: BuildIdentity) -> Self {
        Self {
            prior_session_id,
            build_identity: build_identity.sanitized(),
            detail: None,
        }
    }

    pub fn with_detail(mut self, detail: impl Into<String>) -> Self {
        self.detail = Some(redact_text(&detail.into()));
        self
    }

    pub fn detail(&self) -> Option<&str> {
        self.detail.as_deref()
    }
}

impl fmt::Debug for UncleanShutdownEvidence {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("UncleanShutdownEvidence")
            .field("prior_session_id", &self.prior_session_id)
            .field("build_identity", &self.build_identity)
            .finish()
    }
}

/// Why the previous process/session ended.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TerminationKind {
    CleanExit,
    RustPanic(PanicEvidence),
    AndroidAnr(AndroidAnrEvidence),
    NativeCrash(NativeCrashEvidence),
    OsTermination(OsTerminationEvidence),
    UncleanShutdown(UncleanShutdownEvidence),
}

impl TerminationKind {
    pub fn rust_panic(
        message: impl Into<String>,
        location: Option<String>,
        frames: Vec<String>,
    ) -> Self {
        Self::RustPanic(PanicEvidence::new(message, location, frames))
    }

    pub fn android_anr(reason: impl Into<String>, frames: Vec<String>) -> Self {
        Self::AndroidAnr(AndroidAnrEvidence::new(reason, frames))
    }

    pub fn native_crash(
        signal: Option<i32>,
        exception_type: Option<String>,
        frames: Vec<String>,
    ) -> Self {
        Self::NativeCrash(NativeCrashEvidence::new(signal, exception_type, frames))
    }

    pub fn os_termination(reason: impl Into<String>, exit_code: Option<i32>) -> Self {
        Self::OsTermination(OsTerminationEvidence::new(reason, exit_code))
    }

    pub fn unclean_shutdown(prior_session_id: u64, build_identity: BuildIdentity) -> Self {
        Self::UncleanShutdown(UncleanShutdownEvidence::new(
            prior_session_id,
            build_identity,
        ))
    }

    pub const fn label(&self) -> &'static str {
        match self {
            Self::CleanExit => "clean_exit",
            Self::RustPanic(_) => "rust_panic",
            Self::AndroidAnr(_) => "android_anr",
            Self::NativeCrash(_) => "native_crash",
            Self::OsTermination(_) => "os_termination",
            Self::UncleanShutdown(_) => "unclean_shutdown",
        }
    }

    /// Crash markers are only the four externally observable crash evidence kinds.
    pub const fn is_crash_marker(&self) -> bool {
        matches!(
            self,
            Self::RustPanic(_)
                | Self::AndroidAnr(_)
                | Self::NativeCrash(_)
                | Self::OsTermination(_)
        )
    }

    pub const fn is_clean(&self) -> bool {
        matches!(self, Self::CleanExit)
    }
}

impl fmt::Debug for TerminationKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::CleanExit => f.write_str("CleanExit"),
            Self::RustPanic(evidence) => f
                .debug_struct("RustPanic")
                .field("message_bytes", &evidence.message().len())
                .field("frames", &evidence.frames().len())
                .finish(),
            Self::AndroidAnr(evidence) => f
                .debug_struct("AndroidAnr")
                .field("reason_bytes", &evidence.reason().len())
                .field("frames", &evidence.frames().len())
                .finish(),
            Self::NativeCrash(evidence) => f
                .debug_struct("NativeCrash")
                .field("signal", &evidence.signal())
                .field("frames", &evidence.frames().len())
                .finish(),
            Self::OsTermination(evidence) => f
                .debug_struct("OsTermination")
                .field("reason_bytes", &evidence.reason().len())
                .field("exit_code", &evidence.exit_code())
                .finish(),
            Self::UncleanShutdown(evidence) => f
                .debug_struct("UncleanShutdown")
                .field("prior_session_id", &evidence.prior_session_id)
                .finish(),
        }
    }
}

/// Where a retained crash record came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CrashRecordSource {
    CrashMarker,
    UncleanShutdown,
    OrphanMarker,
}

/// A durable, bounded crash record reconstructed on a later launch.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CrashRecord {
    pub sequence: u64,
    pub session_id: u64,
    pub build_identity: BuildIdentity,
    pub recorded_at_millis: u64,
    pub termination: TerminationKind,
    pub source: CrashRecordSource,
}

impl CrashRecord {
    pub fn new(
        sequence: u64,
        session_id: u64,
        build_identity: BuildIdentity,
        recorded_at_millis: u64,
        termination: TerminationKind,
        source: CrashRecordSource,
    ) -> Self {
        Self {
            sequence,
            session_id,
            build_identity: build_identity.sanitized(),
            recorded_at_millis,
            termination,
            source,
        }
    }

    pub fn label(&self) -> &'static str {
        self.termination.label()
    }
}

impl fmt::Debug for CrashRecord {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CrashRecord")
            .field("sequence", &self.sequence)
            .field("session_id", &self.session_id)
            .field("build_identity", &self.build_identity)
            .field("recorded_at_millis", &self.recorded_at_millis)
            .field("termination", &self.termination)
            .field("source", &self.source)
            .finish()
    }
}

/// Active-session marker. `clean` is set by `mark_clean_exit`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionMarker {
    pub session_id: u64,
    pub build_identity: BuildIdentity,
    pub started_at_millis: u64,
    pub clean: bool,
}

impl SessionMarker {
    pub fn new(session_id: u64, build_identity: BuildIdentity, started_at_millis: u64) -> Self {
        Self {
            session_id,
            build_identity: build_identity.sanitized(),
            started_at_millis,
            clean: false,
        }
    }
}

/// A host-written crash marker that outlives the process.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CrashMarker {
    pub session_id: u64,
    pub build_identity: BuildIdentity,
    pub recorded_at_millis: u64,
    pub termination: TerminationKind,
}

impl CrashMarker {
    pub fn new(
        session_id: u64,
        build_identity: BuildIdentity,
        recorded_at_millis: u64,
        termination: TerminationKind,
    ) -> Self {
        Self {
            session_id,
            build_identity: build_identity.sanitized(),
            recorded_at_millis,
            termination,
        }
    }
}

/// Bounded retention for retained events and crash records.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct RetentionPolicy {
    pub max_events: usize,
    /// Serialized byte budget for retained events. A single event larger than
    /// the budget is deterministically evicted rather than corrupting the log.
    pub max_byte_budget: usize,
    pub max_crash_records: usize,
}

impl RetentionPolicy {
    pub const fn new(max_events: usize, max_byte_budget: usize, max_crash_records: usize) -> Self {
        Self {
            max_events,
            max_byte_budget,
            max_crash_records,
        }
    }
}

impl Default for RetentionPolicy {
    fn default() -> Self {
        Self {
            max_events: 512,
            max_byte_budget: 256 * 1024,
            max_crash_records: 32,
        }
    }
}

/// A corrupt record file moved intact into the `diagnostics/quarantine` area.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QuarantineEntry {
    #[serde(deserialize_with = "de_redacted_string")]
    pub original_name: String,
    #[serde(deserialize_with = "de_redacted_string")]
    pub stored_name: String,
    pub bytes: u64,
}

impl QuarantineEntry {
    pub fn new(
        original_name: impl Into<String>,
        stored_name: impl Into<String>,
        bytes: u64,
    ) -> Self {
        Self {
            original_name: redact_text(&original_name.into()),
            stored_name: redact_text(&stored_name.into()),
            bytes,
        }
    }
}

/// Evidence that a collector or signal is unavailable on this platform/runtime.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UnavailableEvidence {
    #[serde(deserialize_with = "de_redacted_string")]
    pub collector: String,
    #[serde(deserialize_with = "de_redacted_string")]
    pub reason: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub platform: Option<crate::matrix::PlatformTarget>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub operation: Option<DiagnosticsOperation>,
}

impl UnavailableEvidence {
    pub fn new(collector: impl Into<String>, reason: impl Into<String>) -> Self {
        Self {
            collector: redact_text(&collector.into()),
            reason: redact_text(&reason.into()),
            platform: None,
            operation: None,
        }
    }

    pub fn new_for_platform(
        collector: impl Into<String>,
        reason: impl Into<String>,
        platform: crate::matrix::PlatformTarget,
    ) -> Self {
        Self {
            platform: Some(platform),
            ..Self::new(collector, reason)
        }
    }

    pub fn with_platform(mut self, platform: crate::matrix::PlatformTarget) -> Self {
        self.platform = Some(platform);
        self
    }

    pub fn for_operation(mut self, operation: DiagnosticsOperation) -> Self {
        self.operation = Some(operation);
        self
    }
}

/// Prior-session termination summary included in recovery and export.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PriorSessionSummary {
    pub session_id: u64,
    pub build_identity: BuildIdentity,
    pub clean: bool,
    pub termination: TerminationKind,
    pub crash_marker_promoted: bool,
}

/// Result of recovery at start-up: what was adopted, retained, evicted or
/// quarantined, plus the current build/session identity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecoveryReport {
    pub format_version: u32,
    pub current_session_id: u64,
    pub build_identity: BuildIdentity,
    pub prior_session: Option<PriorSessionSummary>,
    pub recovered_crash_records: Vec<CrashRecord>,
    pub retained_events: usize,
    pub evicted_events: u64,
    pub retained_crash_records: usize,
    pub evicted_crash_records: u64,
    pub quarantined_entries: Vec<QuarantineEntry>,
    pub quarantined_count: usize,
    pub unavailable_evidence: Vec<UnavailableEvidence>,
}

impl RecoveryReport {
    pub fn quarantined_count(&self) -> usize {
        self.quarantined_count
    }

    pub fn unavailable_collectors(&self) -> &[UnavailableEvidence] {
        &self.unavailable_evidence
    }

    pub fn had_unclean_prior_session(&self) -> bool {
        self.prior_session
            .as_ref()
            .is_some_and(|prior| !prior.clean)
    }
}

/// Result of an accepted event append.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RecordOutcome {
    pub event: DiagnosticEvent,
    pub stored: bool,
    pub evicted_events: u64,
}

impl std::ops::Deref for RecordOutcome {
    type Target = DiagnosticEvent;

    fn deref(&self) -> &Self::Target {
        &self.event
    }
}

impl std::ops::DerefMut for RecordOutcome {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.event
    }
}

/// Result of an explicit retention prune.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct PruneReport {
    pub evicted_events: u64,
    pub evicted_crash_records: u64,
    pub retained_events: usize,
    pub retained_crash_records: usize,
}

/// Independently gated diagnostic operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiagnosticsOperation {
    RecordEvent,
    WriteCrashMarker,
    InspectRecovery,
    ExportBundle,
    PruneRetained,
}

impl DiagnosticsOperation {
    pub const ALL: [Self; 5] = [
        Self::RecordEvent,
        Self::WriteCrashMarker,
        Self::InspectRecovery,
        Self::ExportBundle,
        Self::PruneRetained,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::RecordEvent => "record_event",
            Self::WriteCrashMarker => "write_crash_marker",
            Self::InspectRecovery => "inspect_recovery",
            Self::ExportBundle => "export_bundle",
            Self::PruneRetained => "prune_retained",
        }
    }
}

impl fmt::Display for DiagnosticsOperation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

pub(crate) fn de_redacted_string<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: Deserializer<'de>,
{
    let value = String::deserialize(deserializer)?;
    Ok(redact_text(&value))
}

pub(crate) fn de_redacted_optional_string<'de, D>(
    deserializer: D,
) -> Result<Option<String>, D::Error>
where
    D: Deserializer<'de>,
{
    let value = Option::<String>::deserialize(deserializer)?;
    Ok(value.map(|value| redact_text(&value)))
}

pub(crate) fn de_redacted_strings<'de, D>(deserializer: D) -> Result<Vec<String>, D::Error>
where
    D: Deserializer<'de>,
{
    let values = Vec::<String>::deserialize(deserializer)?;
    Ok(redact_strings(values))
}

pub(crate) fn redact_strings(values: Vec<String>) -> Vec<String> {
    values
        .into_iter()
        .map(|value| redact_text(&value))
        .collect()
}

impl Serialize for RedactionKind {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for RedactionKind {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        match value.as_str() {
            "token" => Ok(Self::Token),
            "secret" => Ok(Self::Secret),
            "password" => Ok(Self::Password),
            "authorization" => Ok(Self::Authorization),
            "cookie" => Ok(Self::Cookie),
            "credential" => Ok(Self::Credential),
            "private_key" => Ok(Self::PrivateKey),
            "api_key" => Ok(Self::ApiKey),
            "session" => Ok(Self::Session),
            "path" => Ok(Self::Path),
            "bearer" => Ok(Self::Bearer),
            "basic" => Ok(Self::Basic),
            "jwt" => Ok(Self::Jwt),
            "url_userinfo" => Ok(Self::UrlUserInfo),
            "url_query" => Ok(Self::UrlQuery),
            other => Err(D::Error::custom(format!("unknown redaction kind {other}"))),
        }
    }
}
