//! Validated task-presentation values, progress/badge/attention contracts,
//! clearing ownership and presentation receipts.

use crate::platform::TaskPresentationOperation;
use gpui_system::{CancelReason, ErrorCode, SurfaceId, SystemError};
use serde::{Deserialize, Deserializer, Serialize, Serializer, de::Error as _};
use std::{
    fmt,
    num::NonZeroU64,
    time::{Duration, Instant},
};

/// Maximum accepted task identity length in bytes.
pub const MAX_TASK_ID_BYTES: usize = 128;

/// Maximum accepted task action identity length in bytes.
pub const MAX_TASK_ACTION_ID_BYTES: usize = 64;

/// Maximum accepted error-message length in bytes.
pub const MAX_ERROR_MESSAGE_BYTES: usize = 256;

/// Maximum accepted short badge status length in characters.
pub const MAX_BADGE_STATUS_CHARS: usize = 8;

/// Maximum accepted user-visible task action label length in bytes.
pub const MAX_TASK_ACTION_LABEL_BYTES: usize = 64;

/// The synthetic surface identity used by application-wide presentation.
pub const APPLICATION_SURFACE: SurfaceId = SurfaceId(0);

fn identifier(value: &str, max: usize) -> bool {
    !value.is_empty()
        && value.len() <= max
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-' | b':'))
}

fn bounded_text(value: &str, max: usize) -> bool {
    !value.is_empty()
        && value.len() <= max
        && value.trim() == value
        && !value.chars().any(char::is_control)
}

/// A stable, application-owned task identity.
///
/// The accepted shape is ASCII alphanumeric plus `.`, `_`, `-` and `:` with
/// `1..=128` bytes. The value is validated on construction and deserialization.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TaskKey(String);

impl TaskKey {
    pub fn new(value: impl Into<String>) -> Result<Self, SystemError> {
        let value = value.into();
        if !identifier(&value, MAX_TASK_ID_BYTES) {
            return Err(SystemError::new(
                ErrorCode::InvalidInput,
                "Task identity must be non-empty ASCII alphanumeric plus . _ - : and at most 128 bytes",
            ));
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn is_valid(value: &str) -> bool {
        identifier(value, MAX_TASK_ID_BYTES)
    }
}

impl fmt::Debug for TaskKey {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.debug_tuple("TaskKey").field(&self.0).finish()
    }
}

impl fmt::Display for TaskKey {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl AsRef<str> for TaskKey {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl std::str::FromStr for TaskKey {
    type Err = SystemError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::new(value)
    }
}

impl TryFrom<&str> for TaskKey {
    type Error = SystemError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl TryFrom<String> for TaskKey {
    type Error = SystemError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl Serialize for TaskKey {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for TaskKey {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Self::new(String::deserialize(deserializer)?).map_err(D::Error::custom)
    }
}

/// Alias for hosts that call the same identity a presentation identity.
pub type TaskPresentationId = TaskKey;

/// A validated identity for a task action button or notification action.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TaskActionId(String);

impl TaskActionId {
    pub fn new(value: impl Into<String>) -> Result<Self, SystemError> {
        let value = value.into();
        if !identifier(&value, MAX_TASK_ACTION_ID_BYTES) {
            return Err(SystemError::new(
                ErrorCode::InvalidInput,
                "Task action identity must be non-empty ASCII alphanumeric plus . _ - : and at most 64 bytes",
            ));
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for TaskActionId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_tuple("TaskActionId")
            .field(&self.0)
            .finish()
    }
}

impl fmt::Display for TaskActionId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::str::FromStr for TaskActionId {
    type Err = SystemError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::new(value)
    }
}

impl Serialize for TaskActionId {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for TaskActionId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Self::new(String::deserialize(deserializer)?).map_err(D::Error::custom)
    }
}

/// The coarse kind of a task presentation state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskStateKind {
    Indeterminate,
    Determinate,
    Paused,
    Error,
    Completed,
    Cancelled,
}

impl TaskStateKind {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Indeterminate => "indeterminate",
            Self::Determinate => "determinate",
            Self::Paused => "paused",
            Self::Error => "error",
            Self::Completed => "completed",
            Self::Cancelled => "cancelled",
        }
    }

    pub const fn is_terminal(self) -> bool {
        matches!(self, Self::Completed | Self::Cancelled)
    }
}

/// A taskbar/Dock/notification progress state.
///
/// `Determinate` stores integer units and an integer per-mille value in
/// `0..=1000`. No floating-point arithmetic is used anywhere in this model.
#[derive(Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TaskProgressState {
    Determinate {
        completed_units: u64,
        total_units: NonZeroU64,
        per_mille: u16,
    },
    Indeterminate,
    Paused {
        per_mille: Option<u16>,
    },
    Error {
        per_mille: Option<u16>,
        message: Option<String>,
    },
    Completed,
    Cancelled,
}

impl TaskProgressState {
    /// Build a validated determinate state.
    ///
    /// `completed_units > total_units` is rejected with
    /// [`ErrorCode::InvalidInput`]. The per-mille value is computed through
    /// `u128`, so `u64::MAX` cannot overflow.
    pub fn determinate(completed_units: u64, total_units: NonZeroU64) -> Result<Self, SystemError> {
        if completed_units > total_units.get() {
            return Err(SystemError::new(
                ErrorCode::InvalidInput,
                "Completed units exceed total units",
            ));
        }
        let per_mille = ((completed_units as u128) * 1000 / (total_units.get() as u128)) as u16;
        Ok(Self::Determinate {
            completed_units,
            total_units,
            per_mille,
        })
    }

    /// Build a determinate state, clamping an over-complete value to the total.
    pub fn determinate_clamped(completed_units: u64, total_units: NonZeroU64) -> Self {
        let completed_units = completed_units.min(total_units.get());
        let per_mille = ((completed_units as u128) * 1000 / (total_units.get() as u128)) as u16;
        Self::Determinate {
            completed_units,
            total_units,
            per_mille,
        }
    }

    pub fn paused(per_mille: Option<u16>) -> Result<Self, SystemError> {
        validate_per_mille(per_mille)?;
        Ok(Self::Paused { per_mille })
    }

    pub fn error(
        per_mille: Option<u16>,
        message: Option<impl Into<String>>,
    ) -> Result<Self, SystemError> {
        validate_per_mille(per_mille)?;
        let message = match message {
            Some(message) => {
                let message = message.into();
                let message = message.trim().to_owned();
                validate_error_message(&message)?;
                Some(message)
            }
            None => None,
        };
        Ok(Self::Error { per_mille, message })
    }

    pub const fn kind(&self) -> TaskStateKind {
        match self {
            Self::Determinate { .. } => TaskStateKind::Determinate,
            Self::Indeterminate => TaskStateKind::Indeterminate,
            Self::Paused { .. } => TaskStateKind::Paused,
            Self::Error { .. } => TaskStateKind::Error,
            Self::Completed => TaskStateKind::Completed,
            Self::Cancelled => TaskStateKind::Cancelled,
        }
    }

    pub const fn is_terminal(&self) -> bool {
        self.kind().is_terminal()
    }

    pub const fn per_mille(&self) -> Option<u16> {
        match self {
            Self::Determinate { per_mille, .. }
            | Self::Paused {
                per_mille: Some(per_mille),
            }
            | Self::Error {
                per_mille: Some(per_mille),
                ..
            } => Some(*per_mille),
            Self::Indeterminate
            | Self::Paused { per_mille: None }
            | Self::Error {
                per_mille: None, ..
            }
            | Self::Completed
            | Self::Cancelled => None,
        }
    }

    /// State severity used by concurrent aggregation. Terminal states are never
    /// aggregated and therefore return `None`.
    pub const fn severity_rank(&self) -> Option<u8> {
        match self {
            Self::Error { .. } => Some(3),
            Self::Paused { .. } => Some(2),
            Self::Determinate { .. } => Some(1),
            Self::Indeterminate => Some(0),
            Self::Completed | Self::Cancelled => None,
        }
    }

    /// A discrete signature. Two determinate states with the same signature are
    /// continuous progress ticks and may be coalesced.
    pub fn discrete_signature(&self) -> (TaskStateKind, Option<u16>) {
        (self.kind(), self.per_mille())
    }
}

impl fmt::Debug for TaskProgressState {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Determinate {
                completed_units,
                total_units,
                per_mille,
            } => formatter
                .debug_struct("Determinate")
                .field("completed_units", completed_units)
                .field("total_units", &total_units.get())
                .field("per_mille", per_mille)
                .finish(),
            Self::Indeterminate => formatter.write_str("Indeterminate"),
            Self::Paused { per_mille } => formatter
                .debug_struct("Paused")
                .field("per_mille", per_mille)
                .finish(),
            Self::Error { per_mille, message } => formatter
                .debug_struct("Error")
                .field("per_mille", per_mille)
                .field("message", &message.as_ref().map(|_| "[redacted]"))
                .finish(),
            Self::Completed => formatter.write_str("Completed"),
            Self::Cancelled => formatter.write_str("Cancelled"),
        }
    }
}

fn validate_per_mille(per_mille: Option<u16>) -> Result<(), SystemError> {
    if per_mille.is_some_and(|value| value > 1000) {
        return Err(SystemError::new(
            ErrorCode::InvalidInput,
            "Per-mille progress must be at most 1000",
        ));
    }
    Ok(())
}

fn validate_error_message(message: &str) -> Result<(), SystemError> {
    if !bounded_text(message, MAX_ERROR_MESSAGE_BYTES) {
        return Err(SystemError::new(
            ErrorCode::InvalidInput,
            "Error message must be trimmed, non-empty, control-free and at most 256 bytes",
        ));
    }
    Ok(())
}

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum TaskProgressStateDef {
    Determinate {
        completed_units: u64,
        total_units: NonZeroU64,
        per_mille: u16,
    },
    Indeterminate,
    Paused {
        per_mille: Option<u16>,
    },
    Error {
        per_mille: Option<u16>,
        message: Option<String>,
    },
    Completed,
    Cancelled,
}

impl TryFrom<TaskProgressStateDef> for TaskProgressState {
    type Error = SystemError;

    fn try_from(value: TaskProgressStateDef) -> Result<Self, SystemError> {
        match value {
            TaskProgressStateDef::Determinate {
                completed_units,
                total_units,
                per_mille,
            } => {
                let expected = TaskProgressState::determinate(completed_units, total_units)?;
                let TaskProgressState::Determinate {
                    per_mille: expected,
                    ..
                } = expected
                else {
                    unreachable!("determinate construction returns a determinate state");
                };
                if per_mille != expected {
                    return Err(SystemError::new(
                        ErrorCode::InvalidInput,
                        "Per-mille progress is inconsistent with completed and total units",
                    ));
                }
                TaskProgressState::determinate(completed_units, total_units)
            }
            TaskProgressStateDef::Indeterminate => Ok(TaskProgressState::Indeterminate),
            TaskProgressStateDef::Paused { per_mille } => TaskProgressState::paused(per_mille),
            TaskProgressStateDef::Error { per_mille, message } => {
                if let Some(message) = message.as_deref() {
                    validate_error_message(message)?;
                }
                validate_per_mille(per_mille)?;
                Ok(TaskProgressState::Error { per_mille, message })
            }
            TaskProgressStateDef::Completed => Ok(TaskProgressState::Completed),
            TaskProgressStateDef::Cancelled => Ok(TaskProgressState::Cancelled),
        }
    }
}

impl<'de> Deserialize<'de> for TaskProgressState {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = TaskProgressStateDef::deserialize(deserializer)?;
        Self::try_from(value).map_err(D::Error::custom)
    }
}

/// A bounded application badge value.
///
/// `ShortStatus` is a trimmed, non-control UTF-8 glyph/tag of `1..=8` chars and
/// is validated on construction and deserialization.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum BadgeValue {
    Clear,
    Count(u32),
    Dot,
    ShortStatus(String),
}

impl BadgeValue {
    pub fn short_status(value: impl Into<String>) -> Result<Self, SystemError> {
        let value = value.into();
        if !valid_short_status(&value) {
            return Err(SystemError::new(
                ErrorCode::InvalidInput,
                "Badge status must be a trimmed, control-free value of 1..=8 characters",
            ));
        }
        Ok(Self::ShortStatus(value))
    }

    pub const fn is_clear(&self) -> bool {
        matches!(self, Self::Clear)
    }

    /// Whether a boolean/dot-only surface must downgrade this value.
    pub const fn requires_dot_surface(&self) -> bool {
        matches!(self, Self::Count(_) | Self::ShortStatus(_))
    }

    /// Explicit downgrade used when a surface only supports a boolean/dot badge.
    pub fn downgraded_to_dot(&self) -> Option<Self> {
        match self {
            Self::Count(value) if *value > 0 => Some(Self::Dot),
            Self::Count(_) => Some(Self::Clear),
            Self::ShortStatus(_) => Some(Self::Dot),
            Self::Dot | Self::Clear => None,
        }
    }
}

fn valid_short_status(value: &str) -> bool {
    let count = value.chars().count();
    (1..=MAX_BADGE_STATUS_CHARS).contains(&count)
        && value.trim() == value
        && !value.chars().any(char::is_control)
}

#[derive(Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
enum BadgeValueDef {
    Clear,
    Count(u32),
    Dot,
    ShortStatus(String),
}

impl TryFrom<BadgeValueDef> for BadgeValue {
    type Error = SystemError;

    fn try_from(value: BadgeValueDef) -> Result<Self, SystemError> {
        match value {
            BadgeValueDef::Clear => Ok(BadgeValue::Clear),
            BadgeValueDef::Count(value) => Ok(BadgeValue::Count(value)),
            BadgeValueDef::Dot => Ok(BadgeValue::Dot),
            BadgeValueDef::ShortStatus(value) => BadgeValue::short_status(value),
        }
    }
}

impl<'de> Deserialize<'de> for BadgeValue {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = BadgeValueDef::deserialize(deserializer)?;
        Self::try_from(value).map_err(D::Error::custom)
    }
}

/// Attention urgency mapped to platform taskbar/Dock attention levels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AttentionUrgency {
    Informational,
    Critical,
}

/// A surface-generation-scoped attention request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttentionRequest {
    pub urgency: AttentionUrgency,
    pub surface: SurfaceId,
    pub generation: u64,
}

impl AttentionRequest {
    pub const fn new(urgency: AttentionUrgency, surface: SurfaceId, generation: u64) -> Self {
        Self {
            urgency,
            surface,
            generation,
        }
    }
}

/// Who owns clearing a badge.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BadgeClearPolicy {
    /// Clear when the owning surface is focused or activated.
    ClearOnSurfaceFocus,
    /// Clear when every active task in the badge's scope is terminal or dropped.
    ClearOnAllTasksTerminal,
    /// Only an explicit caller action clears the badge.
    ExplicitCallerOnly,
    /// The badge lives exactly as long as its [`crate::BadgeLease`].
    LeaseScoped,
}

/// Who owns clearing an attention request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AttentionClearPolicy {
    ClearOnSurfaceFocus,
    LeaseScoped,
}

/// The application-wide or surface-generation scope of a badge.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BadgeScope {
    Application,
    Surface { surface: SurfaceId, generation: u64 },
}

impl BadgeScope {
    pub const fn surface(self) -> Option<SurfaceId> {
        match self {
            Self::Application => None,
            Self::Surface { surface, .. } => Some(surface),
        }
    }

    pub const fn generation(self) -> Option<u64> {
        match self {
            Self::Application => None,
            Self::Surface { generation, .. } => Some(generation),
        }
    }

    pub const fn matches_surface(self, surface: SurfaceId) -> bool {
        match self {
            Self::Application => true,
            Self::Surface {
                surface: scoped, ..
            } => scoped.0 == surface.0,
        }
    }
}

/// A validated badge application request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BadgeRequest {
    pub badge: BadgeValue,
    pub scope: BadgeScope,
    pub policy: BadgeClearPolicy,
}

impl BadgeRequest {
    pub fn new(badge: BadgeValue, scope: BadgeScope, policy: BadgeClearPolicy) -> Self {
        Self {
            badge,
            scope,
            policy,
        }
    }

    pub fn application(badge: BadgeValue, policy: BadgeClearPolicy) -> Self {
        Self::new(badge, BadgeScope::Application, policy)
    }

    pub fn surface(
        badge: BadgeValue,
        surface: SurfaceId,
        generation: u64,
        policy: BadgeClearPolicy,
    ) -> Self {
        Self::new(
            badge,
            BadgeScope::Surface {
                surface,
                generation,
            },
            policy,
        )
    }
}

/// Task priority used by deterministic concurrent aggregation.
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum TaskPriority {
    Background,
    #[default]
    Normal,
    High,
    Critical,
}

impl TaskPriority {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Background => "background",
            Self::Normal => "normal",
            Self::High => "high",
            Self::Critical => "critical",
        }
    }
}

/// The application or one live surface generation that a task belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TaskScope {
    Application,
    Surface { surface: SurfaceId, generation: u64 },
}

impl TaskScope {
    pub const fn surface(self) -> Option<SurfaceId> {
        match self {
            Self::Application => None,
            Self::Surface { surface, .. } => Some(surface),
        }
    }

    pub const fn generation(self) -> Option<u64> {
        match self {
            Self::Application => None,
            Self::Surface { generation, .. } => Some(generation),
        }
    }

    pub const fn matches(self, surface: SurfaceId, generation: u64) -> bool {
        match self {
            Self::Application => false,
            Self::Surface {
                surface: scoped,
                generation: scoped_generation,
            } => scoped.0 == surface.0 && scoped_generation == generation,
        }
    }

    pub const fn is_application(self) -> bool {
        matches!(self, Self::Application)
    }
}

/// How concurrent determinate states are combined.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AggregateMode {
    /// Report the winning task's determinate progress.
    #[default]
    HighestPriorityLead,
    /// Sum determinate units with `u128` arithmetic into one per-mille value.
    WeightedDeterminateSum,
}

/// Throttle intervals for continuous presentation updates.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ThrottleConfig {
    pub progress_min_interval: Duration,
    pub notification_min_interval: Duration,
}

impl Default for ThrottleConfig {
    fn default() -> Self {
        Self {
            progress_min_interval: Duration::from_millis(100),
            notification_min_interval: Duration::from_millis(500),
        }
    }
}

impl ThrottleConfig {
    pub const fn new(progress_min_interval: Duration, notification_min_interval: Duration) -> Self {
        Self {
            progress_min_interval,
            notification_min_interval,
        }
    }

    /// Disable coalescing while keeping the deterministic `Instant` API.
    pub const fn immediate() -> Self {
        Self::new(Duration::ZERO, Duration::ZERO)
    }
}

/// How confirmed a presentation update is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AcknowledgmentPrecision {
    /// The host confirmed the indicator update.
    AppliedConfirmed,
    /// Submitted to the OS shell/daemon without a visual-render guarantee.
    AcceptedByHost,
    /// Throttled in memory; it will be flushed after the configured interval.
    CoalescedInMemory,
    /// Rendered with a narrower precision than requested.
    DowngradedApplied { note: String },
    /// The surface is unsupported or absent. Business work is unaffected.
    SkippedUnavailable,
    /// Permission was denied. Business work is unaffected.
    SkippedPermissionDenied,
    /// The provider failed while applying. Other surfaces are unaffected.
    Failed { code: ErrorCode },
}

impl AcknowledgmentPrecision {
    pub const fn is_applied(&self) -> bool {
        matches!(
            self,
            Self::AppliedConfirmed | Self::AcceptedByHost | Self::DowngradedApplied { .. }
        )
    }

    pub const fn is_skipped(&self) -> bool {
        matches!(
            self,
            Self::SkippedUnavailable | Self::SkippedPermissionDenied
        )
    }

    pub const fn is_coalesced(&self) -> bool {
        matches!(self, Self::CoalescedInMemory)
    }

    pub const fn label(&self) -> &'static str {
        match self {
            Self::AppliedConfirmed => "applied_confirmed",
            Self::AcceptedByHost => "accepted_by_host",
            Self::CoalescedInMemory => "coalesced_in_memory",
            Self::DowngradedApplied { .. } => "downgraded_applied",
            Self::SkippedUnavailable => "skipped_unavailable",
            Self::SkippedPermissionDenied => "skipped_permission_denied",
            Self::Failed { .. } => "failed",
        }
    }
}

/// Coarse delivery state for one indicator contribution.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PresentationDeliveryStatus {
    /// The update was applied or accepted by the host.
    Applied,
    /// The update is waiting for the next throttle flush.
    CoalescedPending,
    /// The surface was unavailable or permission was denied.
    Skipped,
    /// The provider failed while applying this surface.
    Failed,
}

impl AcknowledgmentPrecision {
    pub const fn delivery_status(&self) -> PresentationDeliveryStatus {
        match self {
            Self::AppliedConfirmed | Self::AcceptedByHost | Self::DowngradedApplied { .. } => {
                PresentationDeliveryStatus::Applied
            }
            Self::CoalescedInMemory => PresentationDeliveryStatus::CoalescedPending,
            Self::SkippedUnavailable | Self::SkippedPermissionDenied => {
                PresentationDeliveryStatus::Skipped
            }
            Self::Failed { .. } => PresentationDeliveryStatus::Failed,
        }
    }
}

/// The native/notification surface that produced one receipt.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PresentationSurfaceKind {
    Taskbar,
    Dock,
    Badge,
    Attention,
    Thumbbar,
    NotificationMirror,
    Application,
}

impl PresentationSurfaceKind {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Taskbar => "taskbar",
            Self::Dock => "dock",
            Self::Badge => "badge",
            Self::Attention => "attention",
            Self::Thumbbar => "thumbbar",
            Self::NotificationMirror => "notification_mirror",
            Self::Application => "application",
        }
    }
}

/// One surface-scoped presentation receipt. It is deliberately separate from
/// the business result of the task.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SurfacePresentationReceipt {
    pub surface: SurfaceId,
    pub generation: u64,
    pub kind: PresentationSurfaceKind,
    pub operation: TaskPresentationOperation,
    pub precision: AcknowledgmentPrecision,
    pub provider_error: Option<SystemError>,
}

impl SurfacePresentationReceipt {
    pub fn new(
        surface: SurfaceId,
        generation: u64,
        kind: PresentationSurfaceKind,
        operation: TaskPresentationOperation,
        precision: AcknowledgmentPrecision,
    ) -> Self {
        Self {
            surface,
            generation,
            kind,
            operation,
            precision,
            provider_error: None,
        }
    }

    pub fn with_error(mut self, error: SystemError) -> Self {
        self.provider_error = Some(error);
        self
    }

    pub fn is_applied(&self) -> bool {
        self.precision.is_applied()
    }
}

/// The complete presentation outcome of one operation.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TaskPresentationOutcome {
    pub receipts: Vec<SurfacePresentationReceipt>,
    pub coalesced: bool,
}

impl TaskPresentationOutcome {
    pub const fn empty() -> Self {
        Self {
            receipts: Vec::new(),
            coalesced: false,
        }
    }

    pub fn from_receipt(receipt: SurfacePresentationReceipt) -> Self {
        Self {
            receipts: vec![receipt],
            coalesced: false,
        }
    }

    pub fn merge(&mut self, mut other: Self) {
        self.receipts.append(&mut other.receipts);
        self.coalesced |= other.coalesced;
    }

    pub fn receipt_for(&self, surface: SurfaceId) -> Option<&SurfacePresentationReceipt> {
        self.receipts
            .iter()
            .find(|receipt| receipt.surface.0 == surface.0)
    }

    pub fn has_applied(&self) -> bool {
        self.receipts
            .iter()
            .any(SurfacePresentationReceipt::is_applied)
    }

    pub fn all_skipped_or_failed(&self) -> bool {
        !self.receipts.is_empty()
            && self.receipts.iter().all(|receipt| {
                receipt.precision.is_skipped()
                    || matches!(receipt.precision, AcknowledgmentPrecision::Failed { .. })
            })
    }
}

/// Business result of a task, preserved independently of presentation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BusinessTaskCompletion<T> {
    Succeeded(T),
    Failed(String),
    Cancelled(CancelReason),
}

impl<T> BusinessTaskCompletion<T> {
    pub const fn is_success(&self) -> bool {
        matches!(self, Self::Succeeded(_))
    }

    pub const fn is_cancelled(&self) -> bool {
        matches!(self, Self::Cancelled(_))
    }

    pub fn as_ref(&self) -> BusinessTaskCompletion<&T> {
        match self {
            Self::Succeeded(value) => BusinessTaskCompletion::Succeeded(value),
            Self::Failed(message) => BusinessTaskCompletion::Failed(message.clone()),
            Self::Cancelled(reason) => BusinessTaskCompletion::Cancelled(*reason),
        }
    }
}

/// A business completion and its presentation receipts, kept structurally
/// distinct so unavailable surfaces can never rewrite the business result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskCompletionRecord<T> {
    pub business: BusinessTaskCompletion<T>,
    pub presentation: TaskPresentationOutcome,
}

/// Configurable terminal behavior for the notification mirror.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TerminalNotificationPolicy {
    /// Withdraw the ongoing notification without posting a replacement.
    #[default]
    Withdraw,
    /// Replace the ongoing notification with a short terminal summary.
    ReplaceWithSummary,
}

/// One task action button linked to a `gpui-menus` command.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TaskActionButton {
    pub action_id: TaskActionId,
    pub label: String,
    pub command_id: gpui_menus::CommandId,
    pub payload: gpui_menus::CommandPayload,
    pub enabled_in_states: Vec<TaskStateKind>,
}

impl TaskActionButton {
    pub fn new(
        action_id: impl Into<String>,
        label: impl Into<String>,
        command_id: gpui_menus::CommandId,
        payload: gpui_menus::CommandPayload,
        enabled_in_states: impl IntoIterator<Item = TaskStateKind>,
    ) -> Result<Self, SystemError> {
        let action_id = TaskActionId::new(action_id)?;
        let label = label.into();
        if !bounded_text(&label, MAX_TASK_ACTION_LABEL_BYTES) {
            return Err(SystemError::new(
                ErrorCode::InvalidInput,
                "Task action label must be trimmed, non-empty, control-free and at most 64 bytes",
            ));
        }
        Ok(Self {
            action_id,
            label,
            command_id,
            payload,
            enabled_in_states: enabled_in_states.into_iter().collect(),
        })
    }

    pub fn for_all_states(
        action_id: impl Into<String>,
        label: impl Into<String>,
        command_id: gpui_menus::CommandId,
        payload: gpui_menus::CommandPayload,
    ) -> Result<Self, SystemError> {
        Self::new(action_id, label, command_id, payload, Vec::new())
    }

    pub fn enabled_in(mut self, states: impl IntoIterator<Item = TaskStateKind>) -> Self {
        self.enabled_in_states.extend(states);
        self
    }

    pub fn is_enabled_in(&self, state: &TaskProgressState) -> bool {
        self.enabled_in_states.is_empty() || self.enabled_in_states.contains(&state.kind())
    }
}

/// A task registration supplied to [`crate::TaskPresentationService`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskRegistration {
    pub id: TaskPresentationId,
    pub priority: TaskPriority,
    pub scope: TaskScope,
    pub progress: TaskProgressState,
    pub title: Option<String>,
    pub body: Option<String>,
    pub channel_id: Option<String>,
    pub actions: Vec<TaskActionButton>,
}

impl TaskRegistration {
    pub fn new(id: TaskPresentationId, progress: TaskProgressState) -> Self {
        Self {
            id,
            priority: TaskPriority::Normal,
            scope: TaskScope::Application,
            progress,
            title: None,
            body: None,
            channel_id: None,
            actions: Vec::new(),
        }
    }

    pub fn with_priority(mut self, priority: TaskPriority) -> Self {
        self.priority = priority;
        self
    }

    pub fn with_scope(mut self, scope: TaskScope) -> Self {
        self.scope = scope;
        self
    }

    pub fn with_title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    pub fn with_body(mut self, body: impl Into<String>) -> Self {
        self.body = Some(body.into());
        self
    }

    pub fn with_channel_id(mut self, channel_id: impl Into<String>) -> Self {
        self.channel_id = Some(channel_id.into());
        self
    }

    pub fn with_action(mut self, action: TaskActionButton) -> Self {
        self.actions.push(action);
        self
    }

    pub fn with_actions(mut self, actions: impl IntoIterator<Item = TaskActionButton>) -> Self {
        self.actions.extend(actions);
        self
    }

    /// Validate the optional user-visible content and unique action identities.
    pub fn validate(&self) -> Result<(), SystemError> {
        if self
            .title
            .as_deref()
            .is_some_and(|title| !bounded_text(title, 256))
        {
            return Err(SystemError::new(
                ErrorCode::InvalidInput,
                "Task title must be trimmed, non-empty, control-free and at most 256 bytes",
            ));
        }
        if self.body.as_deref().is_some_and(|body| {
            body.len() > 4096
                || body.trim() != body
                || body
                    .chars()
                    .any(|character| character.is_control() && !matches!(character, '\n' | '\t'))
        }) {
            return Err(SystemError::new(
                ErrorCode::InvalidInput,
                "Task body is invalid",
            ));
        }
        let mut ids = std::collections::BTreeSet::new();
        for action in &self.actions {
            if !ids.insert(action.action_id.clone()) {
                return Err(SystemError::new(
                    ErrorCode::InvalidInput,
                    "Task action identities must be unique",
                ));
            }
        }
        Ok(())
    }
}

/// One deterministic aggregation input.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TaskSnapshot {
    pub id_sequence: u64,
    pub priority: TaskPriority,
    pub state: TaskStateKind,
    pub severity: Option<u8>,
    pub per_mille: Option<u16>,
}

/// Identity of one registration for stale-click and generation checks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TaskGeneration(pub u64);

/// A monotonic timestamp pair makes test throttling independent of wall time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PresentationClock {
    pub observed_at: Instant,
}

impl PresentationClock {
    pub const fn at(observed_at: Instant) -> Self {
        Self { observed_at }
    }
}

/// Convenience constructor for a determinate state that cannot fail at runtime.
pub fn per_mille(completed_units: u64, total_units: NonZeroU64) -> u16 {
    ((completed_units as u128) * 1000 / (total_units.get() as u128)) as u16
}
