//! Validated durable job identity, constraints, receipts, progress, retry
//! policy and durable record models.
//!
//! Every public numeric model is integral. There are no floating-point fields,
//! and every timestamp is a host-supplied monotonic millisecond value.

use gpui_system::{ErrorCode, SystemError};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fmt;

/// Largest accepted job namespace or job name in bytes.
pub const MAX_JOB_IDENTIFIER_BYTES: usize = 64;
/// Largest accepted ASCII progress stage tag in bytes.
pub const MAX_STAGE_BYTES: usize = 64;
/// Largest accepted idempotency token in bytes.
pub const MAX_IDEMPOTENCY_TOKEN_BYTES: usize = 128;
/// Largest accepted checkpoint payload in bytes.
pub const MAX_CHECKPOINT_BYTES: usize = 64 * 1024;
/// Largest accepted durable job record file in bytes.
pub const MAX_JOB_RECORD_BYTES: u64 = 256 * 1024;
/// Documented floor for a periodic cadence interval.
pub const MIN_PERIODIC_INTERVAL_MS: u64 = 60_000;
/// Upper bound for a periodic cadence interval (seven days).
pub const MAX_PERIODIC_INTERVAL_MS: u64 = 7 * 24 * 60 * 60 * 1_000;
/// Upper bound for an initial delay (seven days).
pub const MAX_INITIAL_DELAY_MS: u64 = 7 * 24 * 60 * 60 * 1_000;
/// Upper bound for one attempt execution budget (twenty-four hours).
pub const MAX_EXECUTION_BUDGET_MS: u64 = 24 * 60 * 60 * 1_000;
/// Upper bound for an overall job deadline (thirty days).
pub const MAX_DEADLINE_MS: u64 = 30 * 24 * 60 * 60 * 1_000;
/// Upper bound on attempts accepted by a retry policy.
pub const MAX_RETRY_ATTEMPTS: u32 = 1_000;
/// Default bounded capacity of one job event stream.
pub const DEFAULT_JOBS_EVENT_CAPACITY: usize = 64;
/// Cooperative grace period before an unacknowledged preemption is finalized.
pub const PREEMPTION_GRACE_MS: u64 = 5_000;

fn invalid() -> SystemError {
    SystemError::new(
        ErrorCode::InvalidInput,
        "Job identifiers must be bounded lowercase ASCII names",
    )
}

fn invalid_constraints() -> SystemError {
    SystemError::new(
        ErrorCode::InvalidInput,
        "Job constraints are outside their supported bounds",
    )
}

/// The typed error returned for a superseded, cancelled or preempted attempt.
///
/// `gpui-system` intentionally has a small, stable error vocabulary and does
/// not define a separate `Canceled` code, so a stale fence fails closed with
/// [`ErrorCode::ContextExpired`].
pub fn stale_fencing_error() -> SystemError {
    SystemError::new(
        ErrorCode::ContextExpired,
        "The attempt fencing token is stale or superseded",
    )
}

fn validate_identifier(value: &str) -> Result<(), SystemError> {
    if value.is_empty()
        || value.len() > MAX_JOB_IDENTIFIER_BYTES
        || !value.is_ascii()
        || value == "."
        || value == ".."
        || value.contains("..")
        || value.starts_with('.')
        || value.ends_with('.')
        || value.starts_with('-')
        || value.ends_with('-')
        || !value.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || b"._-".contains(&byte)
        })
    {
        return Err(invalid());
    }
    Ok(())
}

/// A validated job namespace.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct JobNamespace(String);

impl JobNamespace {
    /// Validate and construct a namespace.
    pub fn new(value: impl Into<String>) -> Result<Self, SystemError> {
        let value = value.into();
        validate_identifier(&value)?;
        Ok(Self(value))
    }

    /// The default application namespace.
    pub fn application() -> Self {
        Self(String::from("application"))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for JobNamespace {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

/// A validated durable job identity (`namespace/name`).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct JobKey {
    namespace: JobNamespace,
    name: String,
}

impl JobKey {
    pub fn new(namespace: JobNamespace, name: impl Into<String>) -> Result<Self, SystemError> {
        let name = name.into();
        validate_identifier(&name)?;
        Ok(Self { namespace, name })
    }

    /// Parse the canonical `namespace/name` spelling.
    pub fn parse(value: &str) -> Result<Self, SystemError> {
        let (namespace, name) = value.split_once('/').ok_or_else(invalid)?;
        if name.contains('/') {
            return Err(invalid());
        }
        Self::new(JobNamespace::new(namespace)?, name)
    }

    pub fn namespace(&self) -> &JobNamespace {
        &self.namespace
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    /// Stable, filesystem-safe encoding used by the durable store.
    pub fn storage_id(&self) -> String {
        hex_encode(format!("{}/{}", self.namespace.0, self.name).as_bytes())
    }
}

impl fmt::Display for JobKey {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}/{}", self.namespace.0, self.name)
    }
}

/// Deterministic registration conflict policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExistingJobPolicy {
    /// Preserve the existing registration and return it unchanged.
    KeepExisting,
    /// Replace the existing definition and fence its active epoch.
    Replace,
    /// Change only the constraints of the existing registration.
    UpdateConstraints,
    /// Reject the registration with a typed conflict error.
    RejectConflict,
}

/// How a registration resolved an existing job.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RegistrationDisposition {
    Created,
    KeptExisting,
    Replaced,
    UpdatedConstraints,
}

/// An explicit acknowledgement that OS scheduling is opportunistic.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SchedulingGuarantee {
    /// The OS may delay, coalesce or skip the run. Never an exact start time.
    AdvisoryOpportunistic,
    /// A bounded best-effort request with a higher platform priority. Still not
    /// an exact wall-clock promise.
    ExpeditedBestEffort,
}

impl SchedulingGuarantee {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::AdvisoryOpportunistic => "advisory_opportunistic",
            Self::ExpeditedBestEffort => "expedited_best_effort",
        }
    }

    /// Both guarantees are advisory. The method exists so consumers cannot
    /// accidentally treat either value as a hard reservation.
    pub const fn is_advisory(self) -> bool {
        true
    }
}

impl fmt::Display for SchedulingGuarantee {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// When a successfully dispatched job may next be considered.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JobCadence {
    /// A single successful run completes the job.
    OneShot,
    /// A bounded periodic cadence. The interval floor is
    /// [`MIN_PERIODIC_INTERVAL_MS`]; `flex_ms` must not exceed the interval.
    Periodic { interval_ms: u64, flex_ms: u64 },
}

impl JobCadence {
    /// Build a validated periodic cadence.
    pub fn periodic(interval_ms: u64, flex_ms: u64) -> Result<Self, SystemError> {
        let cadence = Self::Periodic {
            interval_ms,
            flex_ms,
        };
        cadence.validate()?;
        Ok(cadence)
    }

    pub fn validate(self) -> Result<(), SystemError> {
        if let Self::Periodic {
            interval_ms,
            flex_ms,
        } = self
            && (!(MIN_PERIODIC_INTERVAL_MS..=MAX_PERIODIC_INTERVAL_MS).contains(&interval_ms)
                || flex_ms > interval_ms)
        {
            return Err(invalid_constraints());
        }
        Ok(())
    }

    pub const fn interval_ms(self) -> Option<u64> {
        match self {
            Self::OneShot => None,
            Self::Periodic { interval_ms, .. } => Some(interval_ms),
        }
    }

    pub const fn is_periodic(self) -> bool {
        matches!(self, Self::Periodic { .. })
    }
}

/// A permission requirement persisted with a job definition.
///
/// `PermissionKind` itself is intentionally not retained: authorization is a
/// fresh observation, never a durable grant. `SpecialAccess` keeps the
/// host-supplied stable contract label.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JobPermissionRequirement {
    Notifications,
    Camera,
    Microphone,
    Photos,
    LocationWhenInUse,
    Contacts,
    Biometrics,
    SpecialAccess(String),
}

impl JobPermissionRequirement {
    pub fn label(&self) -> &str {
        match self {
            Self::Notifications => "Notifications",
            Self::Camera => "Camera",
            Self::Microphone => "Microphone",
            Self::Photos => "Photos",
            Self::LocationWhenInUse => "Location",
            Self::Contacts => "Contacts",
            Self::Biometrics => "Biometrics",
            Self::SpecialAccess(label) => label,
        }
    }

    pub fn matches_kind(&self, kind: gpui_permissions::PermissionKind) -> bool {
        self.label() == kind.label()
    }

    pub fn to_kind(&self) -> Option<gpui_permissions::PermissionKind> {
        Some(match self {
            Self::Notifications => gpui_permissions::PermissionKind::Notifications,
            Self::Camera => gpui_permissions::PermissionKind::Camera,
            Self::Microphone => gpui_permissions::PermissionKind::Microphone,
            Self::Photos => gpui_permissions::PermissionKind::Photos,
            Self::LocationWhenInUse => gpui_permissions::PermissionKind::LocationWhenInUse,
            Self::Contacts => gpui_permissions::PermissionKind::Contacts,
            Self::Biometrics => gpui_permissions::PermissionKind::Biometrics,
            Self::SpecialAccess(_) => return None,
        })
    }

    fn validate(&self) -> Result<(), SystemError> {
        if let Self::SpecialAccess(label) = self
            && (label.is_empty()
                || label.len() > MAX_JOB_IDENTIFIER_BYTES
                || !label.is_ascii()
                || label.bytes().any(|byte| byte.is_ascii_control()))
        {
            return Err(invalid_constraints());
        }
        Ok(())
    }
}

impl From<gpui_permissions::PermissionKind> for JobPermissionRequirement {
    fn from(kind: gpui_permissions::PermissionKind) -> Self {
        use gpui_permissions::PermissionKind;
        match kind {
            PermissionKind::Notifications => Self::Notifications,
            PermissionKind::Camera => Self::Camera,
            PermissionKind::Microphone => Self::Microphone,
            PermissionKind::Photos => Self::Photos,
            PermissionKind::LocationWhenInUse => Self::LocationWhenInUse,
            PermissionKind::Contacts => Self::Contacts,
            PermissionKind::Biometrics => Self::Biometrics,
            PermissionKind::SpecialAccess(label) => Self::SpecialAccess(label.to_owned()),
        }
    }
}

/// Network access required before a job may start.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum JobNetworkRequirement {
    #[default]
    None,
    Connected,
    Unmetered,
    NotRoaming,
}

impl JobNetworkRequirement {
    pub const fn requires_network(self) -> bool {
        !matches!(self, Self::None)
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Connected => "connected",
            Self::Unmetered => "unmetered",
            Self::NotRoaming => "not_roaming",
        }
    }
}

/// Integer-only linear or exponential retry backoff.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RetryBackoffPolicy {
    Linear {
        base_delay_ms: u64,
        max_delay_ms: u64,
        max_attempts: u32,
    },
    Exponential {
        initial_delay_ms: u64,
        max_delay_ms: u64,
        max_attempts: u32,
    },
}

impl RetryBackoffPolicy {
    pub const fn linear(base_delay_ms: u64, max_delay_ms: u64, max_attempts: u32) -> Self {
        Self::Linear {
            base_delay_ms,
            max_delay_ms,
            max_attempts,
        }
    }

    pub const fn exponential(initial_delay_ms: u64, max_delay_ms: u64, max_attempts: u32) -> Self {
        Self::Exponential {
            initial_delay_ms,
            max_delay_ms,
            max_attempts,
        }
    }

    /// A policy that permits exactly one attempt.
    pub const fn once() -> Self {
        Self::Linear {
            base_delay_ms: 1,
            max_delay_ms: 1,
            max_attempts: 1,
        }
    }

    pub const fn max_attempts(self) -> u32 {
        match self {
            Self::Linear { max_attempts, .. } | Self::Exponential { max_attempts, .. } => {
                max_attempts
            }
        }
    }

    pub fn validate(self) -> Result<(), SystemError> {
        let (base, max, attempts) = match self {
            Self::Linear {
                base_delay_ms,
                max_delay_ms,
                max_attempts,
            } => (base_delay_ms, max_delay_ms, max_attempts),
            Self::Exponential {
                initial_delay_ms,
                max_delay_ms,
                max_attempts,
            } => (initial_delay_ms, max_delay_ms, max_attempts),
        };
        if base == 0
            || max < base
            || attempts == 0
            || attempts > MAX_RETRY_ATTEMPTS
            || max > MAX_INITIAL_DELAY_MS
        {
            return Err(invalid_constraints());
        }
        Ok(())
    }

    /// The saturating delay before `attempt` (1-based) may be retried.
    pub fn delay_for_attempt(self, attempt: u32) -> u64 {
        let attempt = attempt.max(1);
        match self {
            Self::Linear {
                base_delay_ms,
                max_delay_ms,
                ..
            } => base_delay_ms
                .saturating_mul(u64::from(attempt))
                .min(max_delay_ms),
            Self::Exponential {
                initial_delay_ms,
                max_delay_ms,
                ..
            } => {
                let exponent = attempt - 1;
                let factor = if exponent >= 63 {
                    u64::MAX
                } else {
                    1u64 << exponent
                };
                initial_delay_ms.saturating_mul(factor).min(max_delay_ms)
            }
        }
    }

    /// True when another attempt may follow `completed_attempts`.
    pub fn allows_another_attempt(self, completed_attempts: u32) -> bool {
        completed_attempts < self.max_attempts()
    }
}

impl Default for RetryBackoffPolicy {
    fn default() -> Self {
        Self::exponential(30_000, 3_600_000, 3)
    }
}

/// All eligibility constraints for one durable job.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct JobConstraints {
    pub network: JobNetworkRequirement,
    /// A detected captive portal blocks a network-dependent job.
    pub deny_captive_portal: bool,
    /// Data-saver mode blocks a network-dependent job.
    pub respect_data_saver: bool,
    pub requires_charging: bool,
    pub disallow_battery_saver: bool,
    pub disallow_critical_thermal: bool,
    pub requires_device_idle: bool,
    pub requires_storage_not_low: bool,
    pub initial_delay_ms: u64,
    pub cadence: JobCadence,
    pub execution_budget_ms: u64,
    /// Overall deadline relative to registration, in milliseconds.
    pub deadline_ms: Option<u64>,
    pub required_permission: Option<JobPermissionRequirement>,
    pub retry: RetryBackoffPolicy,
}

impl Default for JobConstraints {
    fn default() -> Self {
        Self {
            network: JobNetworkRequirement::None,
            deny_captive_portal: true,
            respect_data_saver: true,
            requires_charging: false,
            disallow_battery_saver: false,
            disallow_critical_thermal: false,
            requires_device_idle: false,
            requires_storage_not_low: false,
            initial_delay_ms: 0,
            cadence: JobCadence::OneShot,
            execution_budget_ms: 60_000,
            deadline_ms: None,
            required_permission: None,
            retry: RetryBackoffPolicy::default(),
        }
    }
}

impl JobConstraints {
    pub fn with_network(mut self, network: JobNetworkRequirement) -> Self {
        self.network = network;
        self
    }

    pub fn requiring_charging(mut self, required: bool) -> Self {
        self.requires_charging = required;
        self
    }

    pub fn disallowing_battery_saver(mut self, disallow: bool) -> Self {
        self.disallow_battery_saver = disallow;
        self
    }

    pub fn disallowing_critical_thermal(mut self, disallow: bool) -> Self {
        self.disallow_critical_thermal = disallow;
        self
    }

    pub fn requiring_device_idle(mut self, required: bool) -> Self {
        self.requires_device_idle = required;
        self
    }

    pub fn requiring_storage_not_low(mut self, required: bool) -> Self {
        self.requires_storage_not_low = required;
        self
    }

    pub fn with_initial_delay_ms(mut self, delay_ms: u64) -> Self {
        self.initial_delay_ms = delay_ms;
        self
    }

    pub fn with_cadence(mut self, cadence: JobCadence) -> Self {
        self.cadence = cadence;
        self
    }

    pub fn with_execution_budget_ms(mut self, budget_ms: u64) -> Self {
        self.execution_budget_ms = budget_ms;
        self
    }

    pub fn with_deadline_ms(mut self, deadline_ms: u64) -> Self {
        self.deadline_ms = Some(deadline_ms);
        self
    }

    pub fn requiring_permission(mut self, permission: gpui_permissions::PermissionKind) -> Self {
        self.required_permission = Some(permission.into());
        self
    }

    pub fn with_retry(mut self, retry: RetryBackoffPolicy) -> Self {
        self.retry = retry;
        self
    }

    pub fn validate(&self) -> Result<(), SystemError> {
        self.cadence.validate()?;
        self.retry.validate()?;
        if self.initial_delay_ms > MAX_INITIAL_DELAY_MS
            || self.execution_budget_ms == 0
            || self.execution_budget_ms > MAX_EXECUTION_BUDGET_MS
            || self
                .deadline_ms
                .is_some_and(|deadline| deadline == 0 || deadline > MAX_DEADLINE_MS)
        {
            return Err(invalid_constraints());
        }
        if let Some(permission) = &self.required_permission {
            permission.validate()?;
        }
        Ok(())
    }
}

/// A validated bounded ASCII progress stage.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct JobStage(String);

impl JobStage {
    pub fn new(value: impl Into<String>) -> Result<Self, SystemError> {
        let value = value.into();
        if value.is_empty()
            || value.len() > MAX_STAGE_BYTES
            || !value.is_ascii()
            || !value
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"._:-".contains(&byte))
        {
            return Err(SystemError::new(
                ErrorCode::InvalidInput,
                "Progress stages must be bounded ASCII tags",
            ));
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for JobStage {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

/// Integer basis-point progress (`0..=10_000`) with optional unit counts.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct JobProgress {
    completed_units: u64,
    total_units: Option<u64>,
    stage: Option<JobStage>,
}

impl JobProgress {
    /// Construct progress from integer units. `total_units` of zero is
    /// rejected; `completed_units` is clamped to the total.
    pub fn units(completed_units: u64, total_units: Option<u64>) -> Result<Self, SystemError> {
        if total_units == Some(0) {
            return Err(SystemError::new(
                ErrorCode::InvalidInput,
                "A progress total must be positive",
            ));
        }
        let completed_units =
            total_units.map_or(completed_units, |total| completed_units.min(total));
        Ok(Self {
            completed_units,
            total_units,
            stage: None,
        })
    }

    /// Construct exact basis-point progress in `0..=10_000`.
    pub fn from_basis_points(basis_points: u16) -> Result<Self, SystemError> {
        if basis_points > 10_000 {
            return Err(SystemError::new(
                ErrorCode::InvalidInput,
                "Progress basis points must be within 0..=10000",
            ));
        }
        Ok(Self {
            completed_units: u64::from(basis_points),
            total_units: Some(10_000),
            stage: None,
        })
    }

    /// Attach a validated stage tag.
    pub fn with_stage(mut self, stage: impl Into<String>) -> Result<Self, SystemError> {
        self.stage = Some(JobStage::new(stage)?);
        Ok(self)
    }

    /// Integer basis points. Unknown totals report zero; clamped to `10_000`
    /// using `u128` arithmetic so no multiplication can overflow.
    pub fn basis_points(&self) -> u16 {
        let Some(total) = self.total_units else {
            return 0;
        };
        if total == 0 {
            return 0;
        }
        let ratio = (u128::from(self.completed_units) * 10_000) / u128::from(total);
        ratio.min(10_000) as u16
    }

    pub const fn completed_units(&self) -> u64 {
        self.completed_units
    }

    pub const fn total_units(&self) -> Option<u64> {
        self.total_units
    }

    pub fn stage(&self) -> Option<&str> {
        self.stage.as_ref().map(JobStage::as_str)
    }

    pub fn is_complete(&self) -> bool {
        self.basis_points() == 10_000
    }

    /// Validate a progress value received from deserialization or a peer.
    pub fn validate(&self) -> Result<(), SystemError> {
        if self.total_units == Some(0)
            || self
                .total_units
                .is_some_and(|total| self.completed_units > total)
            || self.basis_points() > 10_000
        {
            return Err(SystemError::new(
                ErrorCode::InvalidInput,
                "Progress units or basis points are out of bounds",
            ));
        }
        if let Some(stage) = &self.stage {
            JobStage::new(stage.as_str())?;
        }
        Ok(())
    }
}

/// The lifecycle status of one durable job.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JobStatus {
    /// At least one constraint is unmet, or the initial delay has not elapsed.
    PendingConstraints,
    /// Admitted for opportunistic provider scheduling.
    Scheduled,
    /// An attempt holds the current fencing epoch.
    Running,
    /// A retryable failure is waiting for its deterministic backoff.
    RetryBackoff,
    /// A one-shot job completed successfully.
    Succeeded,
    /// The job was cancelled. Stale completions cannot revive it.
    Cancelled,
    /// A non-retryable failure ended the job.
    Failed,
    /// The overall deadline or attempt budget was exhausted.
    ExpiredBudget,
}

impl JobStatus {
    pub const fn is_terminal(self) -> bool {
        matches!(
            self,
            Self::Succeeded | Self::Cancelled | Self::Failed | Self::ExpiredBudget
        )
    }

    pub const fn is_active(self) -> bool {
        matches!(self, Self::Running)
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::PendingConstraints => "pending_constraints",
            Self::Scheduled => "scheduled",
            Self::Running => "running",
            Self::RetryBackoff => "retry_backoff",
            Self::Succeeded => "succeeded",
            Self::Cancelled => "cancelled",
            Self::Failed => "failed",
            Self::ExpiredBudget => "expired_budget",
        }
    }
}

impl fmt::Display for JobStatus {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// A public-safe failure classification that remains serializable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JobFailureReason {
    Unsupported,
    Unavailable,
    PermissionDenied,
    InvalidConfiguration,
    InvalidInput,
    ContextExpired,
    Busy,
    TimedOut,
    ResourceExpired,
    PlatformFailure,
    IntegrityFailure,
    StaleFencing,
    RetryExhausted,
}

impl JobFailureReason {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Unsupported => "unsupported",
            Self::Unavailable => "unavailable",
            Self::PermissionDenied => "permission_denied",
            Self::InvalidConfiguration => "invalid_configuration",
            Self::InvalidInput => "invalid_input",
            Self::ContextExpired => "context_expired",
            Self::Busy => "busy",
            Self::TimedOut => "timed_out",
            Self::ResourceExpired => "resource_expired",
            Self::PlatformFailure => "platform_failure",
            Self::IntegrityFailure => "integrity_failure",
            Self::StaleFencing => "stale_fencing",
            Self::RetryExhausted => "retry_exhausted",
        }
    }
}

impl From<ErrorCode> for JobFailureReason {
    fn from(code: ErrorCode) -> Self {
        match code {
            ErrorCode::Unsupported => Self::Unsupported,
            ErrorCode::Unavailable => Self::Unavailable,
            ErrorCode::PermissionDenied => Self::PermissionDenied,
            ErrorCode::InvalidConfiguration => Self::InvalidConfiguration,
            ErrorCode::InvalidInput => Self::InvalidInput,
            ErrorCode::ContextExpired => Self::ContextExpired,
            ErrorCode::Busy => Self::Busy,
            ErrorCode::TimedOut => Self::TimedOut,
            ErrorCode::ResourceExpired => Self::ResourceExpired,
            ErrorCode::PlatformFailure => Self::PlatformFailure,
            ErrorCode::IntegrityFailure => Self::IntegrityFailure,
        }
    }
}

/// Why a job cannot be dispatched right now.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UnmetConstraint {
    NetworkUnknown,
    NetworkUnavailable,
    MeteredConnection,
    RoamingConnection,
    CaptivePortal,
    DataSaverEnabled,
    PowerUnknown,
    RequiresCharging,
    BatterySaverEnabled,
    CriticalThermal,
    PowerPacingPaused,
    DeviceNotIdle,
    StorageLow,
    PermissionDenied {
        permission: JobPermissionRequirement,
    },
    PermissionNotDetermined {
        permission: JobPermissionRequirement,
    },
    ProviderUnavailable,
    InitialDelay {
        remaining_ms: u64,
    },
    RetryBackoff {
        remaining_ms: u64,
    },
    LifecycleSuspended,
}

impl UnmetConstraint {
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::NetworkUnknown => "network_unknown",
            Self::NetworkUnavailable => "network_unavailable",
            Self::MeteredConnection => "metered_connection",
            Self::RoamingConnection => "roaming_connection",
            Self::CaptivePortal => "captive_portal",
            Self::DataSaverEnabled => "data_saver_enabled",
            Self::PowerUnknown => "power_unknown",
            Self::RequiresCharging => "requires_charging",
            Self::BatterySaverEnabled => "battery_saver_enabled",
            Self::CriticalThermal => "critical_thermal",
            Self::PowerPacingPaused => "power_pacing_paused",
            Self::DeviceNotIdle => "device_not_idle",
            Self::StorageLow => "storage_low",
            Self::PermissionDenied { .. } => "permission_denied",
            Self::PermissionNotDetermined { .. } => "permission_not_determined",
            Self::ProviderUnavailable => "provider_unavailable",
            Self::InitialDelay { .. } => "initial_delay",
            Self::RetryBackoff { .. } => "retry_backoff",
            Self::LifecycleSuspended => "lifecycle_suspended",
        }
    }
}

/// A validated idempotency token supplied to every attempt of one job.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct IdempotencyToken(String);

impl IdempotencyToken {
    pub fn new(value: impl Into<String>) -> Result<Self, SystemError> {
        let value = value.into();
        if value.is_empty()
            || value.len() > MAX_IDEMPOTENCY_TOKEN_BYTES
            || !value.is_ascii()
            || !value
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"._:-".contains(&byte))
        {
            return Err(SystemError::new(
                ErrorCode::InvalidInput,
                "Idempotency tokens must be bounded ASCII identifiers",
            ));
        }
        Ok(Self(value))
    }

    /// Deterministic token generation. The namespace prefix is bounded so the
    /// result always fits [`MAX_IDEMPOTENCY_TOKEN_BYTES`].
    pub fn generated(namespace: &JobNamespace, name: &str, now_ms: u64) -> Self {
        let prefix: String = namespace
            .as_str()
            .chars()
            .take(24)
            .chain(std::iter::once('-'))
            .collect();
        Self(format!("{prefix}{name}-{now_ms}"))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for IdempotencyToken {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

/// The fence for one dispatched attempt.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FencingToken {
    pub job_key: JobKey,
    pub run_epoch: u64,
    pub attempt: u32,
    pub idempotency_token: IdempotencyToken,
}

impl FencingToken {
    pub fn new(
        job_key: JobKey,
        run_epoch: u64,
        attempt: u32,
        idempotency_token: IdempotencyToken,
    ) -> Self {
        Self {
            job_key,
            run_epoch,
            attempt,
            idempotency_token,
        }
    }
}

/// Public summary of the latest committed checkpoint.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JobCheckpointSummary {
    pub run_epoch: u64,
    pub attempt: u32,
    pub sequence: u64,
    pub created_at_ms: u64,
    pub payload_len: u64,
    pub checksum: String,
}

impl JobCheckpointSummary {
    pub fn is_valid(&self) -> bool {
        self.checksum.len() == 64
            && self
                .checksum
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    }
}

/// A bounded, checksummed durable checkpoint.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JobCheckpoint {
    pub job_key: JobKey,
    pub run_epoch: u64,
    pub attempt: u32,
    pub sequence: u64,
    pub progress: JobProgress,
    pub payload: Vec<u8>,
    pub created_at_ms: u64,
    pub checksum: String,
}

impl JobCheckpoint {
    pub fn new(
        job_key: JobKey,
        run_epoch: u64,
        attempt: u32,
        sequence: u64,
        progress: JobProgress,
        payload: Vec<u8>,
        created_at_ms: u64,
    ) -> Result<Self, SystemError> {
        if payload.len() > MAX_CHECKPOINT_BYTES || sequence == 0 {
            return Err(SystemError::new(
                ErrorCode::InvalidInput,
                "Checkpoint payload or sequence is out of bounds",
            ));
        }
        progress.validate()?;
        let checksum = checkpoint_digest(
            &job_key,
            run_epoch,
            attempt,
            sequence,
            &progress,
            &payload,
            created_at_ms,
        );
        Ok(Self {
            job_key,
            run_epoch,
            attempt,
            sequence,
            progress,
            payload,
            created_at_ms,
            checksum,
        })
    }

    pub fn verify(&self) -> bool {
        JobNamespace::new(self.job_key.namespace().as_str()).is_ok()
            && JobKey::new(self.job_key.namespace().clone(), self.job_key.name()).is_ok()
            && validate_progress(&self.progress).is_ok()
            && self.payload.len() <= MAX_CHECKPOINT_BYTES
            && self.sequence != 0
            && self.checksum
                == checkpoint_digest(
                    &self.job_key,
                    self.run_epoch,
                    self.attempt,
                    self.sequence,
                    &self.progress,
                    &self.payload,
                    self.created_at_ms,
                )
    }

    pub fn summary(&self) -> JobCheckpointSummary {
        JobCheckpointSummary {
            run_epoch: self.run_epoch,
            attempt: self.attempt,
            sequence: self.sequence,
            created_at_ms: self.created_at_ms,
            payload_len: self.payload.len() as u64,
            checksum: self.checksum.clone(),
        }
    }
}

fn checkpoint_digest(
    job_key: &JobKey,
    run_epoch: u64,
    attempt: u32,
    sequence: u64,
    progress: &JobProgress,
    payload: &[u8],
    created_at_ms: u64,
) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"gpui-jobs-checkpoint\0");
    let storage_id = job_key.storage_id();
    hasher.update((storage_id.len() as u64).to_le_bytes());
    hasher.update(storage_id.as_bytes());
    hasher.update(run_epoch.to_le_bytes());
    hasher.update(attempt.to_le_bytes());
    hasher.update(sequence.to_le_bytes());
    hasher.update(progress.completed_units().to_le_bytes());
    hasher.update(progress.total_units().unwrap_or(u64::MAX).to_le_bytes());
    hasher.update(progress.basis_points().to_le_bytes());
    match progress.stage() {
        Some(stage) => {
            hasher.update([1u8]);
            hasher.update((stage.len() as u64).to_le_bytes());
            hasher.update(stage.as_bytes());
        }
        None => hasher.update([0u8]),
    }
    hasher.update(created_at_ms.to_le_bytes());
    hasher.update((payload.len() as u64).to_le_bytes());
    hasher.update(payload);
    hex_encode(&hasher.finalize())
}

/// The durable state of one job registration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JobRecord {
    pub key: JobKey,
    pub status: JobStatus,
    pub constraints: JobConstraints,
    pub run_epoch: u64,
    pub attempt: u32,
    pub completed_runs: u64,
    pub guarantee: SchedulingGuarantee,
    pub registered_at_ms: u64,
    pub updated_at_ms: u64,
    pub earliest_eligible_at_ms: u64,
    pub next_attempt_at_ms: u64,
    pub deadline_at_ms: Option<u64>,
    pub attempt_deadline_at_ms: Option<u64>,
    pub checkpoint: Option<JobCheckpointSummary>,
    pub progress: JobProgress,
    pub last_failure: Option<JobFailureReason>,
    pub unmet_constraints: Vec<UnmetConstraint>,
    pub idempotency_token: IdempotencyToken,
    pub provider_token: Option<u64>,
}

impl JobRecord {
    pub fn is_terminal(&self) -> bool {
        self.status.is_terminal()
    }

    pub fn is_active(&self) -> bool {
        self.status.is_active()
    }

    /// Validate a record loaded from disk before it is admitted to service
    /// state. A valid envelope digest is not sufficient: every semantic field
    /// must satisfy the same invariants as a locally constructed record.
    pub fn validate(&self) -> Result<(), SystemError> {
        let namespace = JobNamespace::new(self.key.namespace().as_str())?;
        let key = JobKey::new(namespace, self.key.name())?;
        if key != self.key {
            return Err(invalid_record());
        }
        IdempotencyToken::new(self.idempotency_token.as_str())?;
        self.constraints.validate()?;
        validate_progress(&self.progress)?;
        if let Some(summary) = &self.checkpoint
            && (summary.sequence == 0
                || summary.payload_len > MAX_CHECKPOINT_BYTES as u64
                || !summary.is_valid())
        {
            return Err(invalid_record());
        }
        Ok(())
    }
}

fn invalid_record() -> SystemError {
    SystemError::new(
        ErrorCode::IntegrityFailure,
        "Persisted job record failed semantic validation",
    )
}

fn validate_progress(progress: &JobProgress) -> Result<(), SystemError> {
    progress.validate().map_err(|_| invalid_record())
}

/// The advisory receipt returned by registration.
///
/// `earliest_eligible_after_ms` is a lower bound in host monotonic milliseconds.
/// Neither guarantee promises an exact start, completion or wall-clock time.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScheduleReceipt {
    pub job_key: JobKey,
    pub registration_disposition: RegistrationDisposition,
    pub status: JobStatus,
    pub run_epoch: u64,
    pub guarantee: SchedulingGuarantee,
    pub earliest_eligible_after_ms: u64,
    pub unmet_constraints: Vec<UnmetConstraint>,
}

impl ScheduleReceipt {
    pub fn is_pending(&self) -> bool {
        matches!(
            self.status,
            JobStatus::PendingConstraints | JobStatus::RetryBackoff
        )
    }

    pub const fn is_advisory(&self) -> bool {
        true
    }
}

/// Lowercase hexadecimal encoding without an external dependency.
pub(crate) fn hex_encode(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(HEX[(byte >> 4) as usize] as char);
        output.push(HEX[(byte & 0x0f) as usize] as char);
    }
    output
}
