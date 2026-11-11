//! Persistent local notification scheduling, separate lifecycle evidence and
//! deterministic restart/clock/permission reconciliation.
//!
//! The default build has no GPUI dependency and installs no native scheduling
//! provider. Durable journal reads and writes, provider submissions,
//! cancellations, delivery observations, activation persistence and every
//! reconciliation require a background [`ExecutionOwner`]; a UI or renderer
//! owner is rejected before any filesystem or provider side effect. Cancellation
//! tokens are honored before each side effect.
//!
//! Three evidence stages are deliberately distinct:
//!
//! - [`ScheduleSubmissionReceipt`] proves validation, durable persistence and
//!   provider acceptance. It never asserts presentation.
//! - [`DeliveryObservation`] records observed presentation only. It never
//!   records user activation.
//! - [`ScheduledActivationHandoff`] records a user tap or action invocation and
//!   never fabricates a presentation timestamp.
//!
//! Every row in the platform matrix is [`Evidence::ContractOnly`]. The optional
//! `gpui` feature adds [`gpui_adapter::NotificationScheduleModel`],
//! [`gpui_adapter::SurfaceNotificationScheduleScope`] and `SystemServices`
//! registration. See `docs/S_NOTIFICATION_SCHEDULE.md`.

#![forbid(unsafe_code)]

mod calendar;
mod clock;
mod model;
mod platform;
mod provider;
mod service;
mod store;
mod timezone;

pub use calendar::{
    CalendarRule, CivilDateTime, MAX_CALENDAR_YEAR, MIN_CALENDAR_YEAR, Weekday, civil_from_days,
    civil_from_epoch_ms, days_from_civil, days_in_month, epoch_ms_from_civil, is_leap_year,
    validate_civil_date,
};
pub use clock::{FakeScheduleClock, ScheduleClock, SystemScheduleClock};
pub use model::{
    ActivationObservation, AttachmentSource, DEFAULT_MAX_ACTIVE_SCHEDULES,
    DEFAULT_MAX_TERMINAL_RECORDS, DEFAULT_NOTIFICATION_SCHEDULE_EVENT_CAPACITY,
    DEFAULT_TERMINAL_TTL_MS, DeliveryObservation, DeliveryObservationReceipt,
    DeliveryObservationSummary, ExactAlarmFallback, InexactDowngrade, InexactDowngradeReason,
    MAX_ACTIVATION_HANDOFFS, MAX_ATTACHMENT_BYTES, MAX_ATTACHMENT_PATH_BYTES, MAX_ATTACHMENTS,
    MAX_DELAY_MS, MAX_DELIVERY_OBSERVATIONS, MAX_MIME_TYPE_BYTES, MAX_MISSED_FIRE_GRACE_MS,
    MAX_REPEATING_INTERVAL_MS, MAX_SCHEDULE_IDENTIFIER_BYTES, MAX_TEXT_INPUT_BYTES,
    MAX_TOTAL_ATTACHMENT_BYTES, MIN_REPEATING_INTERVAL_MS, MissedFirePolicy,
    NotificationScheduleEvent, NotificationScheduleRecord, NotificationScheduleStatus,
    RetentionPolicy, SCHEDULE_EXACT_ALARM_PERMISSION, SCHEDULE_TEXT_INPUT_ACTION_ID,
    ScheduleAttachment, ScheduleConflictPolicy, ScheduleNamespace, SchedulePrecision,
    ScheduleSubmission, ScheduleSubmissionReceipt, ScheduleTrigger, ScheduledActivationHandoff,
    ScheduledNotificationId, SubmissionDisposition, SuspensionReason, capacity_error,
    conflict_error, exact_alarm_permission_kind, stale_epoch_error, validate_attachments,
};
pub use platform::{
    PlatformOperationSupport, PlatformTarget, ScheduleOperation, WebBackendMaturity,
    WebNotificationSchedulePolicy, operation_support, platform_operations, platform_support_matrix,
    web_capability_policy,
};
pub use provider::{
    ChannelAvailability, FakeNotificationScheduleProvider, NotificationScheduleProvider,
    PROVIDER_MANIFEST, PendingProviderSchedule, ProviderScheduleReceipt,
    ScheduledNotificationRequest, UnavailableNotificationScheduleProvider,
    durable_scheduling_available,
};
pub use service::{
    CancellationReceipt, ClockReconciliationReport, NotificationScheduleService,
    PermissionReconciliationReport, ReconciliationReport, RetentionReport, ScheduleAuthorization,
    SchedulePermissionQuery, ensure_not_cancelled,
};
pub use store::{
    MAX_SCHEDULE_ENVELOPE_BYTES, MAX_SCHEDULE_RECORD_BYTES, NOTIFICATION_SCHEDULE_FORMAT_VERSION,
    NOTIFICATION_SCHEDULE_NAMESPACE, NoopScheduleStorageHook, QUARANTINE_DIR, ScheduleStorageFault,
    ScheduleStorageFaultInjection, ScheduleStorageHook,
};
pub use timezone::{
    BUILTIN_DST_FIRST_YEAR, BUILTIN_DST_LAST_YEAR, BuiltinTimeZoneResolver, DstFoldPolicy,
    DstGapPolicy, DstTransitionPoint, DstTransitionRule, LocalResolution, MAX_FIXED_OFFSET_MINUTES,
    MAX_TIME_ZONE_ID_BYTES, MIN_FIXED_OFFSET_MINUTES, ResolvedTimeZone, TimeZoneResolver,
    TimeZoneSpec, local_civil, next_calendar_occurrence,
};

pub use gpui_activation::{
    ActivationId, ActivationPayload, ActivationSource, ActivationTarget, RouteRegistry,
};
pub use gpui_app_data::{AppData, DataLocation, DirectoryPolicy, DirectoryRoots};
pub use gpui_notifications::{
    ActionId, NotificationAction, NotificationActivationBinding, NotificationContent,
    NotificationId, NotificationProgress,
};
pub use gpui_permissions::{PermissionKind, PermissionStatus};
pub use gpui_system::{
    Availability, CancelReason, CancellationToken, Capability, Completion, Configuration,
    Constraint, ErrorCode, Event, EventEnvelope, EventSender, EventSubscription, Evidence,
    OperationContext, PermissionState, Platform, ProviderManifest, RecoveryAction, Request,
    RetryAdvice, ServiceResult, Support, SurfaceId, SystemError, SystemServices, UiContext,
    UiOwner,
};

/// The coordinated upstream GPUI revision used by this distribution.
pub const UPSTREAM_GPUI_REVISION: &str = "f8cfc7ed9e57503def9479138894f3139f5400cc";

/// The gpui-notification-schedule package version.
pub const PACKAGE_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Which execution domain owns a call.
///
/// `gpui-system` models UI affinity as [`OperationContext::ui`]: a UI
/// attachment means `Ui`; no attachment means `Background`. `Renderer` is an
/// explicit class for callers that drive render-thread work; it is never
/// inferred from a context and is never permitted for durable scheduling.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ExecutionOwner {
    Ui,
    Background,
    Renderer,
}

impl ExecutionOwner {
    /// Classify a context. Renderer ownership is never inferred.
    pub fn from_context(context: &OperationContext) -> Self {
        if context.ui.is_some() {
            Self::Ui
        } else {
            Self::Background
        }
    }

    pub const fn is_background(self) -> bool {
        matches!(self, Self::Background)
    }

    /// Durable journal, provider and reconciliation work requires a background
    /// context with no UI attachment. The deadline and cancellation token are
    /// checked before returning success.
    pub fn require_background(self, context: &OperationContext) -> Result<(), SystemError> {
        context.check_at(std::time::Instant::now())?;
        if context.cancellation.reason().is_some() {
            return Err(cancelled_error());
        }
        if !self.is_background() || context.ui.is_some() {
            return Err(background_required_error());
        }
        Ok(())
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Ui => "ui",
            Self::Background => "background",
            Self::Renderer => "renderer",
        }
    }
}

impl std::fmt::Display for ExecutionOwner {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// The typed error returned when a UI or renderer owner attempts durable work.
pub fn background_required_error() -> SystemError {
    SystemError::new(
        ErrorCode::InvalidInput,
        "Notification scheduling requires a background execution owner",
    )
}

/// The typed error returned when a cancellation token fired before a side
/// effect. The operation is guaranteed not to have mutating evidence.
pub fn cancelled_error() -> SystemError {
    SystemError::new(
        ErrorCode::ContextExpired,
        "Notification schedule operation was cancelled before a side effect",
    )
}

#[cfg(feature = "gpui")]
pub mod gpui_adapter;
