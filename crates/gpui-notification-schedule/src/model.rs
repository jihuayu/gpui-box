//! Validated schedule identity, integer triggers, durable records, evidence
//! receipts, activation handoffs and lifecycle events.
//!
//! Submission, observed presentation and user activation are separate models.
//! A submission receipt never asserts presentation, a delivery observation
//! never asserts activation, and an activation never fabricates a presentation.
//! All time fields are integer epoch or monotonic milliseconds.

use crate::calendar::{CalendarRule, CivilDateTime};
use crate::timezone::{
    DstFoldPolicy, DstGapPolicy, ResolvedTimeZone, TimeZoneSpec, next_calendar_occurrence,
};
use gpui_activation::{
    ActivationEvent, ActivationId, ActivationPayload, ActivationSource, ActivationTarget,
};
use gpui_notifications::{ActionId, NotificationContent};
use gpui_permissions::PermissionKind;
use gpui_system::{ErrorCode, SystemError};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fmt;

/// Largest accepted schedule namespace or name in bytes.
pub const MAX_SCHEDULE_IDENTIFIER_BYTES: usize = 64;
/// Documented and enforced floor for a repeating interval.
pub const MIN_REPEATING_INTERVAL_MS: u64 = 60_000;
/// Largest accepted repeating interval (366 days).
pub const MAX_REPEATING_INTERVAL_MS: u64 = 366 * 24 * 60 * 60 * 1_000;
/// Largest accepted initial delay (366 days).
pub const MAX_DELAY_MS: u64 = 366 * 24 * 60 * 60 * 1_000;
/// Largest accepted missed-fire grace (30 days).
pub const MAX_MISSED_FIRE_GRACE_MS: u64 = 30 * 24 * 60 * 60 * 1_000;
/// Maximum attachments on one schedule.
pub const MAX_ATTACHMENTS: usize = 4;
/// Maximum bytes for one attachment.
pub const MAX_ATTACHMENT_BYTES: u64 = 4 * 1024 * 1024;
/// Maximum bytes across all attachments of one schedule.
pub const MAX_TOTAL_ATTACHMENT_BYTES: u64 = 8 * 1024 * 1024;
/// Maximum attachment path or bundled-asset identifier in bytes.
pub const MAX_ATTACHMENT_PATH_BYTES: usize = 256;
/// Maximum MIME type spelling in bytes.
pub const MAX_MIME_TYPE_BYTES: usize = 128;
/// Maximum text-input response retained with one activation.
pub const MAX_TEXT_INPUT_BYTES: usize = 1024;
/// Maximum delivery-observation summaries retained per record.
pub const MAX_DELIVERY_OBSERVATIONS: usize = 32;
/// Maximum activation handoffs retained per record.
pub const MAX_ACTIVATION_HANDOFFS: usize = 16;
/// Default active pending schedule budget (Apple's pending-notification limit).
pub const DEFAULT_MAX_ACTIVE_SCHEDULES: usize = 64;
/// Default number of terminal records retained.
pub const DEFAULT_MAX_TERMINAL_RECORDS: usize = 128;
/// Default terminal-record time-to-live (seven days).
pub const DEFAULT_TERMINAL_TTL_MS: u64 = 7 * 24 * 60 * 60 * 1_000;
/// Default bounded event-stream capacity.
pub const DEFAULT_NOTIFICATION_SCHEDULE_EVENT_CAPACITY: usize = 64;
/// Stable special-access permission label required for exact alarms.
pub const SCHEDULE_EXACT_ALARM_PERMISSION: &str = "schedule_exact_alarm";
/// Reserved action id that is permitted to carry a text-input response.
pub const SCHEDULE_TEXT_INPUT_ACTION_ID: &str = "schedule-text-input";

fn invalid(message: &'static str) -> SystemError {
    SystemError::new(ErrorCode::InvalidInput, message)
}

fn invalid_record() -> SystemError {
    SystemError::new(
        ErrorCode::IntegrityFailure,
        "Persisted notification schedule failed semantic validation",
    )
}

/// The typed error for a superseded, replaced or cancelled schedule epoch.
pub fn stale_epoch_error() -> SystemError {
    SystemError::new(
        ErrorCode::ContextExpired,
        "The notification schedule epoch is stale or superseded",
    )
}

/// The typed conflict error for `RejectConflict`.
pub fn conflict_error() -> SystemError {
    let mut error = SystemError::new(
        ErrorCode::InvalidInput,
        "A schedule with this identity already exists",
    );
    error.retry = gpui_system::RetryAdvice::ModifyRequest;
    error
}

/// The typed error for exhausting the active pending schedule budget.
pub fn capacity_error() -> SystemError {
    let mut error = SystemError::new(
        ErrorCode::Busy,
        "Active notification schedule budget is exhausted",
    );
    error.retry = gpui_system::RetryAdvice::AfterConditionsChange;
    error
}

/// The permission kind required for exact alarms on this platform contract.
pub const fn exact_alarm_permission_kind() -> PermissionKind {
    PermissionKind::SpecialAccess(SCHEDULE_EXACT_ALARM_PERMISSION)
}

fn validate_identifier(value: &str) -> Result<(), SystemError> {
    if value.is_empty()
        || value.len() > MAX_SCHEDULE_IDENTIFIER_BYTES
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
        return Err(invalid(
            "Schedule identifiers must be bounded lowercase ASCII names",
        ));
    }
    Ok(())
}

/// A validated application-owned schedule namespace.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ScheduleNamespace(String);

impl ScheduleNamespace {
    pub fn new(value: impl Into<String>) -> Result<Self, SystemError> {
        let value = value.into();
        validate_identifier(&value)?;
        Ok(Self(value))
    }

    pub fn application() -> Self {
        Self(String::from("application"))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ScheduleNamespace {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

/// A validated durable notification schedule identity (`namespace/name`).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct ScheduledNotificationId {
    namespace: ScheduleNamespace,
    name: String,
}

impl ScheduledNotificationId {
    pub fn new(namespace: ScheduleNamespace, name: impl Into<String>) -> Result<Self, SystemError> {
        let name = name.into();
        validate_identifier(&name)?;
        Ok(Self { namespace, name })
    }

    /// Parse the canonical `namespace/name` spelling.
    pub fn parse(value: &str) -> Result<Self, SystemError> {
        let (namespace, name) = value
            .split_once('/')
            .ok_or_else(|| invalid("Schedule identity must be namespace/name"))?;
        if name.contains('/') {
            return Err(invalid("Schedule identity has too many segments"));
        }
        Self::new(ScheduleNamespace::new(namespace)?, name)
    }

    pub fn namespace(&self) -> &ScheduleNamespace {
        &self.namespace
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    /// Deterministic SHA-256 storage id. It contains only lowercase hex and
    /// cannot traverse a path.
    pub fn storage_id(&self) -> String {
        let mut hasher = Sha256::new();
        hasher.update(b"gpui-notification-schedule-id\0");
        hasher.update(self.namespace.0.as_bytes());
        hasher.update([0]);
        hasher.update(self.name.as_bytes());
        hex_encode(&hasher.finalize())
    }

    /// A valid durable activation identity for this schedule.
    pub fn activation_notification_id(&self) -> Result<ActivationId, SystemError> {
        let storage = self.storage_id();
        ActivationId::new(format!("schedule-{}", &storage[..32]))
    }
}

impl fmt::Display for ScheduledNotificationId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}/{}", self.namespace.0, self.name)
    }
}

/// How a submission resolves an existing active identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScheduleConflictPolicy {
    KeepExisting,
    Replace,
    RejectConflict,
}

/// Whether the host may fire at an exact alarm instant.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SchedulePrecision {
    /// Advisory scheduling. The OS may delay, coalesce or suppress delivery.
    InexactAdvisory,
    /// An exact-alarm request. It requires both provider support and the
    /// platform exact-alarm special-access authorization.
    ExactAlarm,
}

impl SchedulePrecision {
    pub const fn is_exact(self) -> bool {
        matches!(self, Self::ExactAlarm)
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::InexactAdvisory => "inexact_advisory",
            Self::ExactAlarm => "exact_alarm",
        }
    }
}

/// What to do when exact-alarm capability or permission is unavailable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExactAlarmFallback {
    /// Fail closed with a typed error.
    Reject,
    /// Submit explicitly as `InexactAdvisory` and record the downgrade.
    AllowInexact,
}

/// Why an exact-alarm schedule was downgraded.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InexactDowngradeReason {
    ExactAlarmCapabilityUnavailable,
    ExactAlarmPermissionDenied,
    ProviderExactAlarmRefused,
}

impl InexactDowngradeReason {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ExactAlarmCapabilityUnavailable => "exact_alarm_capability_unavailable",
            Self::ExactAlarmPermissionDenied => "exact_alarm_permission_denied",
            Self::ProviderExactAlarmRefused => "provider_exact_alarm_refused",
        }
    }
}

/// Durable record of an explicit exact-to-inexact downgrade.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InexactDowngrade {
    pub reason: InexactDowngradeReason,
    pub recorded_at_epoch_ms: u64,
}

/// Policy for occurrences missed while the process was suspended or offline.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MissedFirePolicy {
    /// Coalesce every missed occurrence into at most one immediate catch-up
    /// and advance the next occurrence strictly past the current time.
    FireOnceLatest,
    /// Drop missed occurrences without firing and resume at the next future
    /// occurrence, or complete/expire when none exists.
    SkipToNext,
    /// Fire one catch-up only when the due instant is within the grace window.
    ExpireAfterGrace { grace_ms: u64 },
}

impl MissedFirePolicy {
    pub fn validate(self) -> Result<(), SystemError> {
        if let Self::ExpireAfterGrace { grace_ms } = self
            && !(1..=MAX_MISSED_FIRE_GRACE_MS).contains(&grace_ms)
        {
            return Err(invalid("Missed-fire grace is outside supported bounds"));
        }
        Ok(())
    }

    pub const fn grace_ms(self) -> Option<u64> {
        match self {
            Self::ExpireAfterGrace { grace_ms } => Some(grace_ms),
            Self::FireOnceLatest | Self::SkipToNext => None,
        }
    }
}

/// Bounded attachment source. Only app-private files and bundled assets are
/// accepted; remote URLs and traversal are rejected by construction.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum AttachmentSource {
    PrivateFile(String),
    BundledAsset(String),
}

impl AttachmentSource {
    pub fn path(&self) -> &str {
        match self {
            Self::PrivateFile(path) | Self::BundledAsset(path) => path,
        }
    }

    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::PrivateFile(_) => "private_file",
            Self::BundledAsset(_) => "bundled_asset",
        }
    }

    fn validate(&self) -> Result<(), SystemError> {
        validate_attachment_path(self.path())
    }
}

/// One validated attachment descriptor. It describes bytes the host owns; this
/// contract crate never reads or uploads the file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScheduleAttachment {
    pub source: AttachmentSource,
    pub mime_type: String,
    pub byte_len: u64,
}

impl ScheduleAttachment {
    pub fn new(
        source: AttachmentSource,
        mime_type: impl Into<String>,
        byte_len: u64,
    ) -> Result<Self, SystemError> {
        let attachment = Self {
            source,
            mime_type: mime_type.into(),
            byte_len,
        };
        attachment.validate()?;
        Ok(attachment)
    }

    pub fn validate(&self) -> Result<(), SystemError> {
        self.source.validate()?;
        validate_mime_type(&self.mime_type)?;
        if self.byte_len == 0 || self.byte_len > MAX_ATTACHMENT_BYTES {
            return Err(invalid(
                "Attachment byte length is outside supported bounds",
            ));
        }
        Ok(())
    }
}

/// Validate the whole attachment list, including count and total-byte bounds.
pub fn validate_attachments(attachments: &[ScheduleAttachment]) -> Result<(), SystemError> {
    if attachments.len() > MAX_ATTACHMENTS {
        return Err(invalid("Notification schedule has too many attachments"));
    }
    let mut total = 0u64;
    for attachment in attachments {
        attachment.validate()?;
        total = total
            .checked_add(attachment.byte_len)
            .ok_or_else(|| invalid("Attachment byte total overflowed"))?;
    }
    if total > MAX_TOTAL_ATTACHMENT_BYTES {
        return Err(invalid("Attachment byte total exceeds the supported bound"));
    }
    Ok(())
}

fn validate_attachment_path(path: &str) -> Result<(), SystemError> {
    if path.is_empty()
        || path.len() > MAX_ATTACHMENT_PATH_BYTES
        || !path.is_ascii()
        || path.starts_with('/')
        || path.starts_with('\\')
        || path.contains('\\')
        || path.contains(':')
        || path.contains("//")
        || path.chars().any(char::is_control)
        || path.chars().any(char::is_whitespace)
    {
        return Err(invalid(
            "Attachment path must be a bounded relative private path",
        ));
    }
    let lower = path.to_ascii_lowercase();
    if ["http://", "https://", "file:", "data:", "javascript:"]
        .iter()
        .any(|prefix| lower.starts_with(prefix))
    {
        return Err(invalid(
            "Remote or executable attachment locations are rejected",
        ));
    }
    for segment in path.split('/') {
        if segment.is_empty()
            || segment == "."
            || segment == ".."
            || segment.len() > 64
            || !segment
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
        {
            return Err(invalid(
                "Attachment path contains a traversal or invalid segment",
            ));
        }
    }
    if path.ends_with('/') {
        return Err(invalid("Attachment path must name a file"));
    }
    Ok(())
}

fn validate_mime_type(mime_type: &str) -> Result<(), SystemError> {
    if mime_type.is_empty()
        || mime_type.len() > MAX_MIME_TYPE_BYTES
        || mime_type.trim() != mime_type
        || mime_type.chars().any(char::is_control)
        || mime_type.chars().any(char::is_whitespace)
        || mime_type.contains(';')
    {
        return Err(invalid("Attachment MIME type is malformed"));
    }
    let Some((top, sub)) = mime_type.split_once('/') else {
        return Err(invalid("Attachment MIME type requires a type/subtype pair"));
    };
    if sub.contains('/') || top.is_empty() || sub.is_empty() {
        return Err(invalid("Attachment MIME type requires a type/subtype pair"));
    }
    if !top.bytes().all(|byte| {
        byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'-' | b'+' | b'.')
    }) || !sub.bytes().all(|byte| {
        byte.is_ascii_lowercase()
            || byte.is_ascii_digit()
            || matches!(
                byte,
                b'!' | b'#' | b'$' | b'&' | b'^' | b'_' | b'.' | b'+' | b'-'
            )
    }) {
        return Err(invalid("Attachment MIME type contains invalid characters"));
    }
    Ok(())
}

/// A strictly integer schedule trigger.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScheduleTrigger {
    /// Fire once at an absolute wall-clock epoch instant.
    OneShotAt { fire_at_epoch_ms: u64 },
    /// Fire once after a positive delay measured from the accepted submission
    /// wall clock. A zero delay is rejected rather than treated as immediate.
    AfterDelay { delay_ms: u64 },
    /// Repeat at a fixed interval measured from the accepted submission wall
    /// clock. The interval floor is [`MIN_REPEATING_INTERVAL_MS`].
    RepeatingInterval {
        interval_ms: u64,
        max_occurrences: Option<u32>,
    },
    /// Recur in a validated timezone using a civil calendar rule.
    CalendarRule(CalendarRule),
}

impl ScheduleTrigger {
    pub fn validate(&self) -> Result<(), SystemError> {
        match self {
            Self::OneShotAt { fire_at_epoch_ms } => {
                if *fire_at_epoch_ms == 0 {
                    return Err(invalid("One-shot fire instant must be after the epoch"));
                }
            }
            Self::AfterDelay { delay_ms } => {
                if !(1..=MAX_DELAY_MS).contains(delay_ms) {
                    return Err(invalid("AfterDelay must be in 1..=366 days"));
                }
            }
            Self::RepeatingInterval {
                interval_ms,
                max_occurrences,
            } => {
                if !(MIN_REPEATING_INTERVAL_MS..=MAX_REPEATING_INTERVAL_MS).contains(interval_ms) {
                    return Err(invalid(
                        "Repeating interval must be at least 60000 ms and at most 366 days",
                    ));
                }
                if *max_occurrences == Some(0) {
                    return Err(invalid("max_occurrences must be at least 1"));
                }
            }
            Self::CalendarRule(rule) => rule.validate()?,
        }
        Ok(())
    }

    pub const fn max_occurrences(&self) -> Option<u32> {
        match self {
            Self::RepeatingInterval {
                max_occurrences, ..
            } => *max_occurrences,
            Self::CalendarRule(rule) => rule.max_occurrences,
            Self::OneShotAt { .. } | Self::AfterDelay { .. } => Some(1),
        }
    }

    pub const fn requires_repeat_capability(&self) -> bool {
        matches!(self, Self::RepeatingInterval { .. } | Self::CalendarRule(_))
    }

    pub const fn is_calendar(&self) -> bool {
        matches!(self, Self::CalendarRule(_))
    }

    pub const fn is_one_shot(&self) -> bool {
        matches!(self, Self::OneShotAt { .. } | Self::AfterDelay { .. })
    }

    pub const fn kind_label(&self) -> &'static str {
        match self {
            Self::OneShotAt { .. } => "one_shot_at",
            Self::AfterDelay { .. } => "after_delay",
            Self::RepeatingInterval { .. } => "repeating_interval",
            Self::CalendarRule(_) => "calendar_rule",
        }
    }

    /// First fire instant for an accepted submission. Calendar rules are
    /// resolved strictly after the submission wall clock.
    pub fn first_fire_at(
        &self,
        submitted_epoch_ms: u64,
        zone: &ResolvedTimeZone,
        gap: DstGapPolicy,
        fold: DstFoldPolicy,
    ) -> Result<Option<u64>, SystemError> {
        match self {
            Self::OneShotAt { fire_at_epoch_ms } => Ok(Some(*fire_at_epoch_ms)),
            Self::AfterDelay { delay_ms } => submitted_epoch_ms
                .checked_add(*delay_ms)
                .map(Some)
                .ok_or_else(|| invalid("Delayed fire instant overflowed")),
            Self::RepeatingInterval { interval_ms, .. } => submitted_epoch_ms
                .checked_add(*interval_ms)
                .map(Some)
                .ok_or_else(|| invalid("Repeating fire instant overflowed")),
            Self::CalendarRule(rule) => {
                next_calendar_occurrence(rule, zone, submitted_epoch_ms, gap, fold)
            }
        }
    }

    /// The next fire instant strictly after `after_epoch_ms`. `anchor` is the
    /// first accepted fire instant for interval triggers.
    pub fn next_fire_after(
        &self,
        anchor: Option<u64>,
        after_epoch_ms: u64,
        zone: &ResolvedTimeZone,
        gap: DstGapPolicy,
        fold: DstFoldPolicy,
    ) -> Result<Option<u64>, SystemError> {
        match self {
            Self::OneShotAt { fire_at_epoch_ms } => {
                Ok((*fire_at_epoch_ms > after_epoch_ms).then_some(*fire_at_epoch_ms))
            }
            Self::AfterDelay { .. } => Ok(anchor.filter(|anchor| *anchor > after_epoch_ms)),
            Self::RepeatingInterval { interval_ms, .. } => {
                let anchor = anchor.ok_or_else(|| {
                    invalid("A repeating schedule requires its accepted anchor instant")
                })?;
                if anchor > after_epoch_ms {
                    return Ok(Some(anchor));
                }
                let delta = after_epoch_ms - anchor;
                let steps = delta / interval_ms + 1;
                anchor
                    .checked_add(
                        steps
                            .checked_mul(*interval_ms)
                            .ok_or_else(|| invalid("Repeating fire instant overflowed"))?,
                    )
                    .map(Some)
                    .ok_or_else(|| invalid("Repeating fire instant overflowed"))
            }
            Self::CalendarRule(rule) => {
                next_calendar_occurrence(rule, zone, after_epoch_ms, gap, fold)
            }
        }
    }
}

impl fmt::Display for ScheduleTrigger {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.kind_label())
    }
}

/// One validated schedule submission.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScheduleSubmission {
    pub schedule_id: ScheduledNotificationId,
    pub trigger: ScheduleTrigger,
    pub content: NotificationContent,
    pub attachments: Vec<ScheduleAttachment>,
    pub time_zone: TimeZoneSpec,
    pub follows_active_time_zone: bool,
    pub dst_gap_policy: DstGapPolicy,
    pub dst_fold_policy: DstFoldPolicy,
    pub missed_fire_policy: MissedFirePolicy,
    pub precision: SchedulePrecision,
    pub exact_alarm_fallback: ExactAlarmFallback,
    pub conflict_policy: ScheduleConflictPolicy,
}

impl ScheduleSubmission {
    pub fn new(
        schedule_id: ScheduledNotificationId,
        trigger: ScheduleTrigger,
        content: NotificationContent,
    ) -> Self {
        Self {
            schedule_id,
            trigger,
            content,
            attachments: Vec::new(),
            time_zone: TimeZoneSpec::Utc,
            follows_active_time_zone: false,
            dst_gap_policy: DstGapPolicy::ShiftForward,
            dst_fold_policy: DstFoldPolicy::FirstOccurrence,
            missed_fire_policy: MissedFirePolicy::FireOnceLatest,
            precision: SchedulePrecision::InexactAdvisory,
            exact_alarm_fallback: ExactAlarmFallback::Reject,
            conflict_policy: ScheduleConflictPolicy::RejectConflict,
        }
    }

    pub fn with_attachments(mut self, attachments: Vec<ScheduleAttachment>) -> Self {
        self.attachments = attachments;
        self
    }

    pub fn with_time_zone(mut self, time_zone: TimeZoneSpec) -> Self {
        self.time_zone = time_zone;
        self
    }

    pub fn following_active_time_zone(mut self, follow: bool) -> Self {
        self.follows_active_time_zone = follow;
        self
    }

    pub fn with_dst_policies(mut self, gap: DstGapPolicy, fold: DstFoldPolicy) -> Self {
        self.dst_gap_policy = gap;
        self.dst_fold_policy = fold;
        self
    }

    pub fn with_missed_fire_policy(mut self, policy: MissedFirePolicy) -> Self {
        self.missed_fire_policy = policy;
        self
    }

    pub fn with_precision(
        mut self,
        precision: SchedulePrecision,
        fallback: ExactAlarmFallback,
    ) -> Self {
        self.precision = precision;
        self.exact_alarm_fallback = fallback;
        self
    }

    pub fn with_conflict_policy(mut self, policy: ScheduleConflictPolicy) -> Self {
        self.conflict_policy = policy;
        self
    }

    pub fn validate(&self) -> Result<(), SystemError> {
        self.trigger.validate()?;
        self.content.validate()?;
        validate_attachments(&self.attachments)?;
        self.time_zone.validate_shape()?;
        self.missed_fire_policy.validate()?;
        Ok(())
    }
}

/// Durable schedule lifecycle status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NotificationScheduleStatus {
    /// Journaled and awaiting provider confirmation after a restart or failure.
    Submitted,
    /// Accepted by the OS provider.
    Scheduled,
    /// Withdrawn because permission or channel policy no longer permits it.
    SuspendedByPolicy,
    /// The one-shot notification was observed as presented.
    Delivered,
    /// The configured occurrence budget completed.
    Completed,
    /// Explicitly cancelled by the application.
    Cancelled,
    /// Expired because no future occurrence exists after a clock jump.
    Expired,
}

impl NotificationScheduleStatus {
    pub const fn is_terminal(self) -> bool {
        matches!(
            self,
            Self::Delivered | Self::Completed | Self::Cancelled | Self::Expired
        )
    }

    pub const fn is_pending(self) -> bool {
        matches!(self, Self::Submitted | Self::Scheduled)
    }

    pub const fn is_active(self) -> bool {
        matches!(
            self,
            Self::Submitted | Self::Scheduled | Self::SuspendedByPolicy
        )
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Submitted => "submitted",
            Self::Scheduled => "scheduled",
            Self::SuspendedByPolicy => "suspended_by_policy",
            Self::Delivered => "delivered",
            Self::Completed => "completed",
            Self::Cancelled => "cancelled",
            Self::Expired => "expired",
        }
    }
}

impl fmt::Display for NotificationScheduleStatus {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// Why a schedule was withdrawn from the provider without becoming terminal.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SuspensionReason {
    NotificationsPermissionDenied,
    ExactAlarmPermissionRevoked,
    ChannelDisabled { channel_id: String },
    ProviderUnavailable,
}

impl SuspensionReason {
    pub fn label(&self) -> &'static str {
        match self {
            Self::NotificationsPermissionDenied => "notifications_permission_denied",
            Self::ExactAlarmPermissionRevoked => "exact_alarm_permission_revoked",
            Self::ChannelDisabled { .. } => "channel_disabled",
            Self::ProviderUnavailable => "provider_unavailable",
        }
    }
}

/// How a submission resolved its identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SubmissionDisposition {
    Created,
    KeptExisting,
    Replaced,
    DowngradedToInexact,
}

impl SubmissionDisposition {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Created => "created",
            Self::KeptExisting => "kept_existing",
            Self::Replaced => "replaced",
            Self::DowngradedToInexact => "downgraded_to_inexact",
        }
    }
}

/// Submission evidence only: validated, durable and accepted by the provider.
/// It never asserts presentation or user activation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScheduleSubmissionReceipt {
    pub schedule_id: ScheduledNotificationId,
    pub schedule_epoch: u64,
    pub disposition: SubmissionDisposition,
    pub status: NotificationScheduleStatus,
    pub precision: SchedulePrecision,
    pub downgrade: Option<InexactDowngrade>,
    pub next_fire_at_epoch_ms: Option<u64>,
    pub provider_token: Option<u64>,
    pub submitted_at_epoch_ms: u64,
}

impl ScheduleSubmissionReceipt {
    pub const fn is_scheduled(&self) -> bool {
        matches!(self.status, NotificationScheduleStatus::Scheduled)
    }

    pub const fn was_downgraded(&self) -> bool {
        self.downgrade.is_some()
    }
}

/// One observation that the platform presented a scheduled notification.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeliveryObservation {
    pub schedule_id: ScheduledNotificationId,
    pub schedule_epoch: u64,
    pub presented_at_epoch_ms: u64,
    pub provider_token: Option<u64>,
}

impl DeliveryObservation {
    pub fn new(
        schedule_id: ScheduledNotificationId,
        schedule_epoch: u64,
        presented_at_epoch_ms: u64,
    ) -> Self {
        Self {
            schedule_id,
            schedule_epoch,
            presented_at_epoch_ms,
            provider_token: None,
        }
    }

    pub fn with_provider_token(mut self, provider_token: u64) -> Self {
        self.provider_token = Some(provider_token);
        self
    }

    pub fn validate(&self) -> Result<(), SystemError> {
        if self.schedule_epoch == 0 || self.presented_at_epoch_ms == 0 {
            return Err(invalid(
                "Delivery observation identity or instant is invalid",
            ));
        }
        Ok(())
    }
}

/// Bounded retained delivery evidence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeliveryObservationSummary {
    pub presented_at_epoch_ms: u64,
    pub provider_token: Option<u64>,
}

/// Presentation evidence receipt. It never asserts user activation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeliveryObservationReceipt {
    pub schedule_id: ScheduledNotificationId,
    pub schedule_epoch: u64,
    pub presented_at_epoch_ms: u64,
    pub presented_count: u32,
    pub next_fire_at_epoch_ms: Option<u64>,
    pub status: NotificationScheduleStatus,
    pub provider_rescheduled: bool,
}

/// One platform-reported user tap or action invocation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ActivationObservation {
    pub schedule_id: ScheduledNotificationId,
    pub schedule_epoch: u64,
    pub action_id: Option<ActionId>,
    pub text_input: Option<String>,
    pub activated_at_epoch_ms: u64,
}

impl ActivationObservation {
    pub fn tap(
        schedule_id: ScheduledNotificationId,
        schedule_epoch: u64,
        activated_at_epoch_ms: u64,
    ) -> Self {
        Self {
            schedule_id,
            schedule_epoch,
            action_id: None,
            text_input: None,
            activated_at_epoch_ms,
        }
    }

    pub fn action(
        schedule_id: ScheduledNotificationId,
        schedule_epoch: u64,
        action_id: ActionId,
        activated_at_epoch_ms: u64,
    ) -> Self {
        Self {
            schedule_id,
            schedule_epoch,
            action_id: Some(action_id),
            text_input: None,
            activated_at_epoch_ms,
        }
    }

    pub fn validate(&self) -> Result<(), SystemError> {
        if self.schedule_epoch == 0 || self.activated_at_epoch_ms == 0 {
            return Err(invalid("Activation identity or instant is invalid"));
        }
        if let Some(text) = &self.text_input
            && (text.is_empty()
                || text.len() > MAX_TEXT_INPUT_BYTES
                || text.trim() != text
                || text.chars().any(char::is_control))
        {
            return Err(invalid("Activation text input is outside supported bounds"));
        }
        Ok(())
    }
}

/// Typed activation handoff compatible with `gpui-activation` and
/// `gpui-notifications`. The action id is retained separately because the
/// durable `ActivationPayload::Notification` shape intentionally carries only
/// the notification identity and route target.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScheduledActivationHandoff {
    pub schedule_id: ScheduledNotificationId,
    pub schedule_epoch: u64,
    pub notification_activation_id: ActivationId,
    pub activation_event_id: ActivationId,
    pub source: ActivationSource,
    pub action_id: Option<ActionId>,
    pub text_input: Option<String>,
    pub target: Option<ActivationTarget>,
    pub payload: Option<ActivationPayload>,
    pub received_at_epoch_ms: u64,
}

impl ScheduledActivationHandoff {
    /// Build the durable activation event expected by `gpui-activation`.
    pub fn into_activation_event(
        &self,
        received_at: std::time::SystemTime,
    ) -> Result<ActivationEvent, SystemError> {
        let payload = self
            .payload
            .clone()
            .ok_or_else(|| invalid("Activation handoff has no registered route target"))?;
        ActivationEvent::new(
            self.activation_event_id.clone(),
            ActivationSource::Notification,
            payload,
            received_at,
        )
    }

    pub fn action_label<'a>(&self, content: &'a NotificationContent) -> Option<&'a str> {
        let action_id = self.action_id.as_ref()?;
        content
            .actions()
            .iter()
            .find(|action| action.id() == action_id)
            .map(|action| action.label())
    }
}

impl fmt::Debug for ScheduledActivationHandoff {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ScheduledActivationHandoff")
            .field("schedule_id", &self.schedule_id)
            .field("schedule_epoch", &self.schedule_epoch)
            .field("action_id", &self.action_id)
            .field(
                "text_input",
                &self.text_input.as_ref().map(|_| "[redacted]"),
            )
            .field("has_target", &self.target.is_some())
            .field("received_at_epoch_ms", &self.received_at_epoch_ms)
            .finish()
    }
}

/// The durable state of one scheduled notification.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NotificationScheduleRecord {
    pub schedule_id: ScheduledNotificationId,
    pub schedule_epoch: u64,
    pub status: NotificationScheduleStatus,
    pub trigger: ScheduleTrigger,
    pub time_zone: TimeZoneSpec,
    pub follows_active_time_zone: bool,
    pub dst_gap_policy: DstGapPolicy,
    pub dst_fold_policy: DstFoldPolicy,
    pub missed_fire_policy: MissedFirePolicy,
    pub precision: SchedulePrecision,
    pub exact_alarm_fallback: ExactAlarmFallback,
    pub downgrade: Option<InexactDowngrade>,
    pub content: NotificationContent,
    pub attachments: Vec<ScheduleAttachment>,
    pub conflict_policy: ScheduleConflictPolicy,
    pub submitted_at_epoch_ms: u64,
    pub submitted_monotonic_ms: u64,
    pub updated_at_epoch_ms: u64,
    pub anchor_fire_at_epoch_ms: Option<u64>,
    pub next_fire_at_epoch_ms: Option<u64>,
    pub catch_up_at_epoch_ms: Option<u64>,
    pub provider_fire_at_epoch_ms: Option<u64>,
    pub provider_token: Option<u64>,
    pub presented_count: u32,
    pub last_presented_at_epoch_ms: Option<u64>,
    pub activation_count: u32,
    pub last_activated_at_epoch_ms: Option<u64>,
    pub delivery_observations: Vec<DeliveryObservationSummary>,
    pub activation_handoffs: Vec<ScheduledActivationHandoff>,
    pub suspension: Option<SuspensionReason>,
    pub terminal_at_epoch_ms: Option<u64>,
}

impl NotificationScheduleRecord {
    pub const fn is_terminal(&self) -> bool {
        self.status.is_terminal()
    }

    pub const fn is_active(&self) -> bool {
        self.status.is_active()
    }

    pub const fn is_pending(&self) -> bool {
        self.status.is_pending()
    }

    pub const fn expected_provider_fire_at(&self) -> Option<u64> {
        match self.catch_up_at_epoch_ms {
            Some(instant) => Some(instant),
            None => self.next_fire_at_epoch_ms,
        }
    }

    pub const fn max_occurrences(&self) -> Option<u32> {
        self.trigger.max_occurrences()
    }

    pub fn local_civil(&self, zone: &ResolvedTimeZone) -> Option<CivilDateTime> {
        let instant = self.next_fire_at_epoch_ms?;
        crate::timezone::local_civil(zone, instant).ok()
    }

    /// Validate a record loaded from disk before admitting it to service state.
    pub fn validate(&self) -> Result<(), SystemError> {
        validate_identifier(self.schedule_id.namespace().as_str()).map_err(|_| invalid_record())?;
        validate_identifier(self.schedule_id.name()).map_err(|_| invalid_record())?;
        if self.schedule_id.namespace().as_str().is_empty() {
            return Err(invalid_record());
        }
        if self.schedule_epoch == 0 {
            return Err(invalid_record());
        }
        self.trigger.validate().map_err(|_| invalid_record())?;
        self.time_zone
            .validate_shape()
            .map_err(|_| invalid_record())?;
        self.content.validate().map_err(|_| invalid_record())?;
        validate_attachments(&self.attachments).map_err(|_| invalid_record())?;
        self.missed_fire_policy
            .validate()
            .map_err(|_| invalid_record())?;
        if self.submitted_at_epoch_ms == 0 || self.updated_at_epoch_ms < self.submitted_at_epoch_ms
        {
            return Err(invalid_record());
        }
        if self.downgrade.is_some() && self.precision != SchedulePrecision::InexactAdvisory {
            return Err(invalid_record());
        }
        if self.precision == SchedulePrecision::ExactAlarm && self.downgrade.is_some() {
            return Err(invalid_record());
        }
        if self.delivery_observations.len() > MAX_DELIVERY_OBSERVATIONS
            || self.activation_handoffs.len() > MAX_ACTIVATION_HANDOFFS
        {
            return Err(invalid_record());
        }
        if u32::try_from(self.delivery_observations.len())
            .map(|len| len > self.presented_count)
            .unwrap_or(true)
        {
            return Err(invalid_record());
        }
        if u32::try_from(self.activation_handoffs.len())
            .map(|len| len > self.activation_count)
            .unwrap_or(true)
        {
            return Err(invalid_record());
        }
        for observation in &self.delivery_observations {
            if observation.presented_at_epoch_ms == 0
                || observation.presented_at_epoch_ms < self.submitted_at_epoch_ms
            {
                return Err(invalid_record());
            }
        }
        if let Some(last) = self.delivery_observations.last()
            && (self.presented_count == 0
                || self.last_presented_at_epoch_ms != Some(last.presented_at_epoch_ms))
        {
            return Err(invalid_record());
        }
        let declared_actions: Vec<&ActionId> = self
            .content
            .actions()
            .iter()
            .map(|action| action.id())
            .collect();
        for handoff in &self.activation_handoffs {
            if handoff.schedule_id != self.schedule_id
                || handoff.schedule_epoch == 0
                || handoff.received_at_epoch_ms < self.submitted_at_epoch_ms
            {
                return Err(invalid_record());
            }
            if let Some(action_id) = &handoff.action_id
                && !declared_actions.contains(&action_id)
            {
                return Err(invalid_record());
            }
            if let Some(text) = &handoff.text_input
                && (text.is_empty()
                    || text.len() > MAX_TEXT_INPUT_BYTES
                    || text.trim() != text
                    || text.chars().any(char::is_control))
            {
                return Err(invalid_record());
            }
            if handoff.text_input.is_some()
                && handoff
                    .action_id
                    .as_ref()
                    .is_none_or(|action_id| action_id.as_str() != SCHEDULE_TEXT_INPUT_ACTION_ID)
            {
                return Err(invalid_record());
            }
        }
        if let Some(last) = self.activation_handoffs.last()
            && (self.activation_count == 0
                || self.last_activated_at_epoch_ms != Some(last.received_at_epoch_ms))
        {
            return Err(invalid_record());
        }
        match self.status {
            NotificationScheduleStatus::SuspendedByPolicy => {
                if self.suspension.is_none()
                    || self.next_fire_at_epoch_ms.is_none()
                    || self.catch_up_at_epoch_ms.is_some()
                    || self.provider_fire_at_epoch_ms.is_some()
                    || self.terminal_at_epoch_ms.is_some()
                {
                    return Err(invalid_record());
                }
            }
            NotificationScheduleStatus::Submitted | NotificationScheduleStatus::Scheduled => {
                if self.suspension.is_some()
                    || self.next_fire_at_epoch_ms.is_none()
                    || self.provider_fire_at_epoch_ms.is_none()
                    || self.terminal_at_epoch_ms.is_some()
                {
                    return Err(invalid_record());
                }
            }
            _ => {
                if self.next_fire_at_epoch_ms.is_some()
                    || self.catch_up_at_epoch_ms.is_some()
                    || self.provider_fire_at_epoch_ms.is_some()
                    || self.terminal_at_epoch_ms.is_none()
                {
                    return Err(invalid_record());
                }
            }
        }
        if self.is_active()
            && matches!(
                self.trigger,
                ScheduleTrigger::RepeatingInterval { .. } | ScheduleTrigger::AfterDelay { .. }
            )
            && !self
                .anchor_fire_at_epoch_ms
                .is_some_and(|anchor| anchor > 0)
        {
            return Err(invalid_record());
        }
        if self.catch_up_at_epoch_ms.is_some()
            && self.provider_fire_at_epoch_ms != self.catch_up_at_epoch_ms
        {
            return Err(invalid_record());
        }
        if self.catch_up_at_epoch_ms.is_none()
            && self.next_fire_at_epoch_ms.is_some()
            && self.provider_fire_at_epoch_ms.is_some()
            && self.provider_fire_at_epoch_ms != self.next_fire_at_epoch_ms
        {
            return Err(invalid_record());
        }
        if self.status == NotificationScheduleStatus::Scheduled && self.provider_token.is_none() {
            return Err(invalid_record());
        }
        if self.status == NotificationScheduleStatus::Scheduled
            && self.precision == SchedulePrecision::ExactAlarm
            && self.exact_alarm_fallback == ExactAlarmFallback::AllowInexact
        {
            // An exact-alarm record may only be downgraded; it may not claim
            // exact precision while already carrying a downgrade.
            if self.downgrade.is_some() {
                return Err(invalid_record());
            }
        }
        Ok(())
    }
}

/// Bounded lifecycle events. Submission (`Submitted`/`Scheduled`), presentation
/// (`Presented`) and activation (`Activated`) are deliberately distinct.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NotificationScheduleEvent {
    Submitted {
        schedule_id: ScheduledNotificationId,
        schedule_epoch: u64,
        precision: SchedulePrecision,
        next_fire_at_epoch_ms: Option<u64>,
    },
    Scheduled {
        schedule_id: ScheduledNotificationId,
        schedule_epoch: u64,
        precision: SchedulePrecision,
        next_fire_at_epoch_ms: Option<u64>,
        resubmitted: bool,
    },
    Presented {
        schedule_id: ScheduledNotificationId,
        schedule_epoch: u64,
        presented_at_epoch_ms: u64,
        presented_count: u32,
        next_fire_at_epoch_ms: Option<u64>,
        terminal: bool,
    },
    Activated {
        schedule_id: ScheduledNotificationId,
        schedule_epoch: u64,
        action_id: Option<ActionId>,
        activated_at_epoch_ms: u64,
    },
    DowngradedToInexact {
        schedule_id: ScheduledNotificationId,
        schedule_epoch: u64,
        reason: InexactDowngradeReason,
    },
    Suspended {
        schedule_id: ScheduledNotificationId,
        schedule_epoch: u64,
        reason: SuspensionReason,
    },
    Resumed {
        schedule_id: ScheduledNotificationId,
        schedule_epoch: u64,
        next_fire_at_epoch_ms: Option<u64>,
    },
    Cancelled {
        schedule_id: ScheduledNotificationId,
        schedule_epoch: u64,
        provider_cancelled: bool,
    },
    Expired {
        schedule_id: ScheduledNotificationId,
        schedule_epoch: u64,
        missed_occurrences: u32,
    },
    Completed {
        schedule_id: ScheduledNotificationId,
        schedule_epoch: u64,
        presented_count: u32,
    },
    Rescheduled {
        schedule_id: ScheduledNotificationId,
        schedule_epoch: u64,
        next_fire_at_epoch_ms: u64,
        catch_up: bool,
    },
    CorruptRecordQuarantined {
        record_kind: &'static str,
        bytes: u64,
    },
    Pruned {
        removed: usize,
    },
}

impl NotificationScheduleEvent {
    pub fn schedule_id(&self) -> Option<&ScheduledNotificationId> {
        match self {
            Self::Submitted { schedule_id, .. }
            | Self::Scheduled { schedule_id, .. }
            | Self::Presented { schedule_id, .. }
            | Self::Activated { schedule_id, .. }
            | Self::DowngradedToInexact { schedule_id, .. }
            | Self::Suspended { schedule_id, .. }
            | Self::Resumed { schedule_id, .. }
            | Self::Cancelled { schedule_id, .. }
            | Self::Expired { schedule_id, .. }
            | Self::Completed { schedule_id, .. }
            | Self::Rescheduled { schedule_id, .. } => Some(schedule_id),
            Self::CorruptRecordQuarantined { .. } | Self::Pruned { .. } => None,
        }
    }
}

/// Bounded retention policy for durable schedules.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RetentionPolicy {
    pub max_active_schedules: usize,
    pub max_terminal_records: usize,
    pub terminal_ttl_ms: u64,
}

impl Default for RetentionPolicy {
    fn default() -> Self {
        Self {
            max_active_schedules: DEFAULT_MAX_ACTIVE_SCHEDULES,
            max_terminal_records: DEFAULT_MAX_TERMINAL_RECORDS,
            terminal_ttl_ms: DEFAULT_TERMINAL_TTL_MS,
        }
    }
}

impl RetentionPolicy {
    pub fn validate(&self) -> Result<(), SystemError> {
        if self.max_active_schedules == 0 || self.max_active_schedules > 1_024 {
            return Err(invalid("max_active_schedules must be in 1..=1024"));
        }
        if self.max_terminal_records > 4_096 {
            return Err(invalid("max_terminal_records must be at most 4096"));
        }
        if self.terminal_ttl_ms == 0 || self.terminal_ttl_ms > 365 * 24 * 60 * 60 * 1_000 {
            return Err(invalid("terminal_ttl_ms is outside supported bounds"));
        }
        Ok(())
    }
}

/// Count missed occurrences without iterating an unbounded number of times.
///
/// The result saturates at `u32::MAX`; callers only use it for reporting and
/// never schedule one provider request per missed occurrence.
pub(crate) fn count_missed_occurrences(
    trigger: &ScheduleTrigger,
    anchor: Option<u64>,
    last_due_epoch_ms: u64,
    now_epoch_ms: u64,
    zone: &ResolvedTimeZone,
    gap: DstGapPolicy,
    fold: DstFoldPolicy,
) -> Result<u32, SystemError> {
    if last_due_epoch_ms > now_epoch_ms {
        return Ok(0);
    }
    match trigger {
        ScheduleTrigger::RepeatingInterval { interval_ms, .. } => {
            let anchor = anchor.ok_or_else(|| invalid("Repeating schedule has no anchor"))?;
            let first_due = last_due_epoch_ms.max(anchor);
            let delta = now_epoch_ms - first_due;
            let count = delta / interval_ms + 1;
            Ok(u32::try_from(count).unwrap_or(u32::MAX))
        }
        _ => {
            let mut cursor = last_due_epoch_ms;
            let mut count = 1u32;
            while count < u32::MAX {
                let Some(next) = trigger.next_fire_after(anchor, cursor, zone, gap, fold)? else {
                    break;
                };
                if next > now_epoch_ms {
                    break;
                }
                cursor = next;
                count = count.saturating_add(1);
            }
            Ok(count)
        }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::calendar::{MIN_CALENDAR_YEAR, Weekday};
    use crate::timezone::{LocalResolution, TimeZoneResolver};

    fn content() -> NotificationContent {
        NotificationContent::new("Reminder", "Stand up").expect("content")
    }

    #[test]
    fn identifiers_are_bounded_and_storage_ids_are_hex() {
        let id = ScheduledNotificationId::parse("application/reminder-one").expect("id");
        assert_eq!(id.to_string(), "application/reminder-one");
        assert_eq!(id.storage_id().len(), 64);
        assert!(id.storage_id().bytes().all(|byte| byte.is_ascii_hexdigit()));
        assert!(ScheduledNotificationId::parse("Application/x").is_err());
        assert!(ScheduledNotificationId::parse("app/../etc").is_err());
        assert!(ScheduledNotificationId::parse("app/a b").is_err());
        assert!(ScheduleNamespace::new("A").is_err());
        assert!(ScheduleNamespace::new("a".repeat(65)).is_err());
        assert_ne!(
            ScheduledNotificationId::parse("a/b")
                .expect("a")
                .storage_id(),
            ScheduledNotificationId::parse("a/c")
                .expect("b")
                .storage_id()
        );
    }

    #[test]
    fn trigger_validation_enforces_interval_floor_and_nonzero_delay() {
        assert!(
            ScheduleTrigger::RepeatingInterval {
                interval_ms: MIN_REPEATING_INTERVAL_MS - 1,
                max_occurrences: None
            }
            .validate()
            .is_err()
        );
        assert!(
            ScheduleTrigger::RepeatingInterval {
                interval_ms: MIN_REPEATING_INTERVAL_MS,
                max_occurrences: None
            }
            .validate()
            .is_ok()
        );
        assert!(
            ScheduleTrigger::AfterDelay { delay_ms: 0 }
                .validate()
                .is_err()
        );
        assert!(
            ScheduleTrigger::AfterDelay { delay_ms: 1 }
                .validate()
                .is_ok()
        );
        assert!(
            ScheduleTrigger::RepeatingInterval {
                interval_ms: MIN_REPEATING_INTERVAL_MS,
                max_occurrences: Some(0)
            }
            .validate()
            .is_err()
        );
    }

    #[test]
    fn interval_math_skips_missed_occurrences_exactly() {
        let trigger = ScheduleTrigger::RepeatingInterval {
            interval_ms: 60_000,
            max_occurrences: None,
        };
        let zone = ResolvedTimeZone::utc();
        assert_eq!(
            trigger
                .next_fire_after(
                    Some(1_000_000),
                    1_000_000,
                    &zone,
                    DstGapPolicy::ShiftForward,
                    DstFoldPolicy::FirstOccurrence
                )
                .expect("next"),
            Some(1_060_000)
        );
        // Ten and a half intervals later coalesces to the next exact cadence.
        assert_eq!(
            trigger
                .next_fire_after(
                    Some(1_000_000),
                    1_630_000,
                    &zone,
                    DstGapPolicy::ShiftForward,
                    DstFoldPolicy::FirstOccurrence
                )
                .expect("next"),
            Some(1_660_000)
        );
        assert_eq!(
            count_missed_occurrences(
                &trigger,
                Some(1_000_000),
                1_060_000,
                10 * 86_400_000,
                &zone,
                DstGapPolicy::ShiftForward,
                DstFoldPolicy::FirstOccurrence,
            )
            .expect("count"),
            14_383
        );
    }

    #[test]
    fn attachments_reject_remote_and_traversal_paths() {
        assert!(
            ScheduleAttachment::new(
                AttachmentSource::PrivateFile(String::from("../secret")),
                "image/png",
                10
            )
            .is_err()
        );
        assert!(
            ScheduleAttachment::new(
                AttachmentSource::PrivateFile(String::from("https://example.com/a.png")),
                "image/png",
                10
            )
            .is_err()
        );
        assert!(
            ScheduleAttachment::new(
                AttachmentSource::BundledAsset(String::from("assets/icon.png")),
                "image/png",
                10
            )
            .is_ok()
        );
        assert!(validate_mime_type("image/png; charset=utf-8").is_err());
        assert!(validate_mime_type("image").is_err());
        assert!(validate_mime_type("IMAGE/PNG").is_err());
        let too_large = ScheduleAttachment {
            source: AttachmentSource::BundledAsset(String::from("assets/icon.png")),
            mime_type: String::from("image/png"),
            byte_len: MAX_ATTACHMENT_BYTES + 1,
        };
        assert!(too_large.validate().is_err());
        let total = vec![
            ScheduleAttachment::new(
                AttachmentSource::BundledAsset(String::from("a.png")),
                "image/png",
                MAX_ATTACHMENT_BYTES,
            )
            .expect("a"),
            ScheduleAttachment::new(
                AttachmentSource::BundledAsset(String::from("b.png")),
                "image/png",
                MAX_ATTACHMENT_BYTES,
            )
            .expect("b"),
            ScheduleAttachment::new(
                AttachmentSource::BundledAsset(String::from("c.png")),
                "image/png",
                MAX_ATTACHMENT_BYTES,
            )
            .expect("c"),
        ];
        assert!(validate_attachments(&total).is_err());
    }

    #[test]
    fn submission_defaults_are_safe() {
        let id = ScheduledNotificationId::parse("application/reminder").expect("id");
        let submission = ScheduleSubmission::new(
            id,
            ScheduleTrigger::AfterDelay { delay_ms: 60_000 },
            content(),
        );
        assert_eq!(submission.precision, SchedulePrecision::InexactAdvisory);
        assert_eq!(submission.exact_alarm_fallback, ExactAlarmFallback::Reject);
        assert_eq!(
            submission.conflict_policy,
            ScheduleConflictPolicy::RejectConflict
        );
        submission.validate().expect("valid");
    }

    #[test]
    fn resolver_contract_round_trips_named_zone() {
        let resolver = crate::timezone::BuiltinTimeZoneResolver::new();
        let zone = resolver
            .resolve(&TimeZoneSpec::Named(String::from("Europe/London")))
            .expect("zone");
        let covered =
            crate::calendar::epoch_ms_from_civil(2026, 1, 15, 12, 0, 0).expect("covered instant");
        assert_eq!(
            zone.resolve_local(
                covered,
                DstGapPolicy::ShiftForward,
                DstFoldPolicy::FirstOccurrence
            )
            .expect("resolved"),
            LocalResolution::Unique(covered)
        );
        assert_eq!(Weekday::Monday.index(), 0);
        assert_eq!(MIN_CALENDAR_YEAR, 1970);
    }
}
