use crate::error;
use gpui_activation::{ActivationTarget, RouteRegistry};
use gpui_permissions::SettingsTarget;
use gpui_system::{
    Availability, Capability, Configuration, Constraint, ErrorCode, Evidence, PermissionState,
    Platform, ProviderManifest, RecoveryAction, RetryAdvice, Support, SystemError,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
};

const MAX_IDENTIFIER: usize = 128;
const MAX_PROVIDER_IDENTIFIER: usize = 256;
const MAX_ACTION_IDENTIFIER: usize = 64;
const MAX_TITLE: usize = 256;
const MAX_BODY: usize = 4096;
const MAX_CHANNEL: usize = 64;
const MAX_ACTION_LABEL: usize = 64;
const MAX_PROGRESS_LABEL: usize = 64;
const MAX_ACTIONS: usize = 8;
const MAX_ACTION_TARGETS: usize = 8;
const MAX_PROGRESS_TOTAL: u32 = 1_000_000;

fn identifier(value: &str, max: usize) -> bool {
    !value.is_empty()
        && value.len() <= max
        && value
            .bytes()
            .all(|value| value.is_ascii_alphanumeric() || matches!(value, b'-' | b'_' | b'.'))
}

fn text(value: &str, max: usize, allow_empty: bool, multiline: bool) -> bool {
    if value.len() > max || value.trim() != value || (!allow_empty && value.is_empty()) {
        return false;
    }
    value
        .chars()
        .all(|value| !value.is_control() || (multiline && matches!(value, '\n' | '\t')))
}

/// A stable, application-owned notification identity.
///
/// The accepted shape matches [`gpui_activation::ActivationId`] so the same value
/// can be reconstructed as a durable activation payload. Debug output is redacted.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct NotificationId(String);

impl NotificationId {
    pub fn new(value: impl Into<String>) -> Result<Self, SystemError> {
        let value = value.into();
        if !identifier(&value, MAX_IDENTIFIER) {
            return Err(error(
                ErrorCode::InvalidInput,
                "Notification identity is invalid",
            ));
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for NotificationId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("NotificationId([redacted])")
    }
}

/// An opaque provider or platform notification handle.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ProviderNotificationId(String);

impl ProviderNotificationId {
    pub fn new(value: impl Into<String>) -> Result<Self, SystemError> {
        let value = value.into();
        if value.is_empty()
            || value.len() > MAX_PROVIDER_IDENTIFIER
            || value.trim() != value
            || value.chars().any(char::is_control)
        {
            return Err(error(
                ErrorCode::InvalidInput,
                "Provider notification identity is invalid",
            ));
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for ProviderNotificationId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("ProviderNotificationId([redacted])")
    }
}

/// A stable action identity declared on one notification.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ActionId(String);

impl ActionId {
    pub fn new(value: impl Into<String>) -> Result<Self, SystemError> {
        let value = value.into();
        if !identifier(&value, MAX_ACTION_IDENTIFIER) {
            return Err(error(
                ErrorCode::InvalidInput,
                "Notification action identity is invalid",
            ));
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for ActionId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("ActionId([redacted])")
    }
}

/// One optional action button. The label is user-visible content and is redacted
/// from Debug output.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NotificationAction {
    id: ActionId,
    label: String,
}

impl NotificationAction {
    pub fn new(id: ActionId, label: impl Into<String>) -> Result<Self, SystemError> {
        let label = label.into();
        if !text(&label, MAX_ACTION_LABEL, false, false) {
            return Err(error(
                ErrorCode::InvalidInput,
                "Notification action label is invalid",
            ));
        }
        Ok(Self { id, label })
    }

    pub fn id(&self) -> &ActionId {
        &self.id
    }

    pub fn label(&self) -> &str {
        &self.label
    }

    pub(crate) fn validate(&self) -> Result<(), SystemError> {
        ActionId::new(self.id.as_str())?;
        if !text(&self.label, MAX_ACTION_LABEL, false, false) {
            return Err(error(
                ErrorCode::InvalidInput,
                "Notification action label is invalid",
            ));
        }
        Ok(())
    }
}

impl fmt::Debug for NotificationAction {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("NotificationAction([redacted])")
    }
}

/// A bounded progress value. It describes submitted progress, not an observed
/// platform progress bar. The optional label is redacted from Debug output.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NotificationProgress {
    completed: u32,
    total: u32,
    label: Option<String>,
}

impl NotificationProgress {
    pub fn new(completed: u32, total: u32) -> Result<Self, SystemError> {
        if total == 0 || total > MAX_PROGRESS_TOTAL || completed > total {
            return Err(error(
                ErrorCode::InvalidInput,
                "Notification progress is outside the supported range",
            ));
        }
        Ok(Self {
            completed,
            total,
            label: None,
        })
    }

    pub fn with_label(mut self, label: impl Into<String>) -> Result<Self, SystemError> {
        let label = label.into();
        if !text(&label, MAX_PROGRESS_LABEL, false, false) {
            return Err(error(
                ErrorCode::InvalidInput,
                "Notification progress label is invalid",
            ));
        }
        self.label = Some(label);
        Ok(self)
    }

    pub fn completed(&self) -> u32 {
        self.completed
    }

    pub fn total(&self) -> u32 {
        self.total
    }

    pub fn label(&self) -> Option<&str> {
        self.label.as_deref()
    }

    pub(crate) fn validate(&self) -> Result<(), SystemError> {
        if self.total == 0 || self.total > MAX_PROGRESS_TOTAL || self.completed > self.total {
            return Err(error(
                ErrorCode::InvalidInput,
                "Notification progress is outside the supported range",
            ));
        }
        if self
            .label
            .as_deref()
            .is_some_and(|label| !text(label, MAX_PROGRESS_LABEL, false, false))
        {
            return Err(error(
                ErrorCode::InvalidInput,
                "Notification progress label is invalid",
            ));
        }
        Ok(())
    }
}

impl fmt::Debug for NotificationProgress {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("NotificationProgress")
            .field("completed", &self.completed)
            .field("total", &self.total)
            .field("label", &"[redacted]")
            .finish()
    }
}

/// Reconstruction metadata for notification taps and action buttons.
///
/// The binding contains only [`ActivationTarget`] values. Notification title,
/// body and other user content are never part of it. Debug output is redacted.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NotificationActivationBinding {
    default_target: ActivationTarget,
    action_targets: BTreeMap<ActionId, ActivationTarget>,
}

impl NotificationActivationBinding {
    pub fn new(default_target: ActivationTarget) -> Self {
        Self {
            default_target,
            action_targets: BTreeMap::new(),
        }
    }

    pub fn bind(
        mut self,
        action_id: ActionId,
        target: ActivationTarget,
    ) -> Result<Self, SystemError> {
        if !self.action_targets.contains_key(&action_id)
            && self.action_targets.len() == MAX_ACTION_TARGETS
        {
            return Err(error(
                ErrorCode::InvalidInput,
                "Notification activation has too many action targets",
            ));
        }
        self.action_targets.insert(action_id, target);
        Ok(self)
    }

    pub fn default_target(&self) -> &ActivationTarget {
        &self.default_target
    }

    pub fn target_for(&self, action_id: Option<&ActionId>) -> Option<&ActivationTarget> {
        match action_id {
            Some(action_id) => self.action_targets.get(action_id),
            None => Some(&self.default_target),
        }
    }

    pub fn action_targets(&self) -> &BTreeMap<ActionId, ActivationTarget> {
        &self.action_targets
    }

    pub(crate) fn validate_shape(&self) -> Result<(), SystemError> {
        if self.action_targets.len() > MAX_ACTION_TARGETS {
            return Err(error(
                ErrorCode::InvalidInput,
                "Notification activation has too many action targets",
            ));
        }
        validate_target(&self.default_target)?;
        for (action_id, target) in &self.action_targets {
            ActionId::new(action_id.as_str())?;
            validate_target(target)?;
        }
        Ok(())
    }

    pub fn validate(&self, routes: &RouteRegistry) -> Result<(), SystemError> {
        self.validate_shape()?;
        routes.validate(&self.default_target)?;
        for target in self.action_targets.values() {
            routes.validate(target)?;
        }
        Ok(())
    }
}

fn invalid_target() -> SystemError {
    error(
        ErrorCode::InvalidInput,
        "Notification activation target is invalid",
    )
}

fn validate_target(target: &ActivationTarget) -> Result<(), SystemError> {
    ActivationTarget::new(
        target.route().to_owned(),
        target.version(),
        target.parameters().clone(),
    )
    .map(|_| ())
    .map_err(|_| invalid_target())
}

impl fmt::Debug for NotificationActivationBinding {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("NotificationActivationBinding([redacted])")
    }
}

/// Bounded notification content.
///
/// Content is validated again at submission time. Debug output never exposes the
/// title, body, channel or action labels.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NotificationContent {
    title: String,
    body: String,
    channel_id: Option<String>,
    actions: Vec<NotificationAction>,
    progress: Option<NotificationProgress>,
    activation: Option<NotificationActivationBinding>,
}

impl NotificationContent {
    pub fn new(title: impl Into<String>, body: impl Into<String>) -> Result<Self, SystemError> {
        let title = title.into();
        let body = body.into();
        if !text(&title, MAX_TITLE, false, false) || !text(&body, MAX_BODY, true, true) {
            return Err(error(
                ErrorCode::InvalidInput,
                "Notification title or body is invalid",
            ));
        }
        Ok(Self {
            title,
            body,
            channel_id: None,
            actions: Vec::new(),
            progress: None,
            activation: None,
        })
    }

    pub fn with_channel_id(mut self, channel_id: impl Into<String>) -> Result<Self, SystemError> {
        let channel_id = channel_id.into();
        if !identifier(&channel_id, MAX_CHANNEL) {
            return Err(error(
                ErrorCode::InvalidInput,
                "Notification channel identity is invalid",
            ));
        }
        self.channel_id = Some(channel_id);
        Ok(self)
    }

    pub fn with_actions(mut self, actions: Vec<NotificationAction>) -> Result<Self, SystemError> {
        if actions.len() > MAX_ACTIONS {
            return Err(error(
                ErrorCode::InvalidInput,
                "Notification has too many action buttons",
            ));
        }
        let mut ids = BTreeSet::new();
        for action in &actions {
            action.validate()?;
            if !ids.insert(action.id().clone()) {
                return Err(error(
                    ErrorCode::InvalidInput,
                    "Notification action identities must be unique",
                ));
            }
        }
        self.actions = actions;
        Ok(self)
    }

    pub fn with_progress(mut self, progress: NotificationProgress) -> Self {
        self.progress = Some(progress);
        self
    }

    pub fn with_activation(mut self, activation: NotificationActivationBinding) -> Self {
        self.activation = Some(activation);
        self
    }

    pub fn title(&self) -> &str {
        &self.title
    }

    pub fn body(&self) -> &str {
        &self.body
    }

    pub fn channel_id(&self) -> Option<&str> {
        self.channel_id.as_deref()
    }

    pub fn actions(&self) -> &[NotificationAction] {
        &self.actions
    }

    pub fn progress(&self) -> Option<&NotificationProgress> {
        self.progress.as_ref()
    }

    pub fn activation(&self) -> Option<&NotificationActivationBinding> {
        self.activation.as_ref()
    }

    pub(crate) fn without_actions(&self) -> Self {
        let mut content = self.clone();
        content.actions.clear();
        // Action-specific targets are unreachable once the buttons are omitted.
        // Keep the default target so a body tap still reconstructs the screen.
        if let Some(binding) = content.activation.as_mut() {
            binding.action_targets.clear();
        }
        content
    }

    pub(crate) fn without_progress(&self) -> Self {
        let mut content = self.clone();
        content.progress = None;
        content
    }

    pub fn validate(&self) -> Result<(), SystemError> {
        if !text(&self.title, MAX_TITLE, false, false) || !text(&self.body, MAX_BODY, true, true) {
            return Err(error(
                ErrorCode::InvalidInput,
                "Notification title or body is invalid",
            ));
        }
        if self
            .channel_id
            .as_deref()
            .is_some_and(|channel| !identifier(channel, MAX_CHANNEL))
        {
            return Err(error(
                ErrorCode::InvalidInput,
                "Notification channel identity is invalid",
            ));
        }
        if self.actions.len() > MAX_ACTIONS {
            return Err(error(
                ErrorCode::InvalidInput,
                "Notification has too many action buttons",
            ));
        }
        let mut ids = BTreeSet::new();
        for action in &self.actions {
            action.validate()?;
            if !ids.insert(action.id().clone()) {
                return Err(error(
                    ErrorCode::InvalidInput,
                    "Notification action identities must be unique",
                ));
            }
        }
        if let Some(progress) = &self.progress {
            progress.validate()?;
        }
        if let Some(activation) = &self.activation {
            activation.validate_shape()?;
            let bound: BTreeSet<_> = activation.action_targets.keys().collect();
            let declared: BTreeSet<_> = self.actions.iter().map(NotificationAction::id).collect();
            if bound != declared {
                return Err(error(
                    ErrorCode::InvalidInput,
                    "Every notification action requires exactly one activation target",
                ));
            }
        }
        Ok(())
    }
}

impl fmt::Debug for NotificationContent {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("NotificationContent")
            .field("title", &"[redacted]")
            .field("body", &"[redacted]")
            .field("channel", &self.channel_id.is_some())
            .field("actions", &self.actions.len())
            .field("progress", &self.progress.is_some())
            .field("activation", &self.activation.is_some())
            .finish()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActionPolicy {
    /// Reject the submission when the provider cannot present action buttons.
    RequireSupported,
    /// Submit without action buttons and report `omitted_actions` in the receipt.
    OmitIfUnsupported,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProgressPolicy {
    /// Reject the submission when the provider cannot present progress.
    RequireSupported,
    /// Submit without progress and report `omitted_progress` in the receipt.
    OmitIfUnsupported,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReceiptPolicy {
    /// Reject the submission when the provider cannot return delivery evidence.
    RequireSupported,
    /// Accept the provider's downgraded receipt without provider identity.
    OmitIfUnsupported,
}

/// One validated notification submission.
#[derive(Clone, PartialEq, Eq)]
pub struct NotificationSubmission {
    pub notification_id: NotificationId,
    pub content: NotificationContent,
    pub action_policy: ActionPolicy,
    pub progress_policy: ProgressPolicy,
    pub receipt_policy: ReceiptPolicy,
}

impl NotificationSubmission {
    pub fn new(notification_id: NotificationId, content: NotificationContent) -> Self {
        Self {
            notification_id,
            content,
            action_policy: ActionPolicy::OmitIfUnsupported,
            progress_policy: ProgressPolicy::OmitIfUnsupported,
            receipt_policy: ReceiptPolicy::OmitIfUnsupported,
        }
    }

    pub fn with_action_policy(mut self, policy: ActionPolicy) -> Self {
        self.action_policy = policy;
        self
    }

    pub fn with_progress_policy(mut self, policy: ProgressPolicy) -> Self {
        self.progress_policy = policy;
        self
    }

    pub fn with_receipt_policy(mut self, policy: ReceiptPolicy) -> Self {
        self.receipt_policy = policy;
        self
    }

    pub fn validate(&self) -> Result<(), SystemError> {
        NotificationId::new(self.notification_id.as_str())?;
        self.content.validate()
    }
}

impl fmt::Debug for NotificationSubmission {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("NotificationSubmission")
            .field("notification_id", &self.notification_id)
            .field("content", &self.content)
            .field("action_policy", &self.action_policy)
            .field("progress_policy", &self.progress_policy)
            .field("receipt_policy", &self.receipt_policy)
            .finish()
    }
}

/// Actionable recovery for a rejected or unavailable notification operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RecoveryGuidance {
    pub action: RecoveryAction,
    pub settings: Option<SettingsTarget>,
}

impl RecoveryGuidance {
    pub const fn notification_settings() -> Self {
        Self {
            action: RecoveryAction::OpenSettings,
            settings: Some(SettingsTarget::NotificationSettings),
        }
    }

    pub const fn repair_configuration() -> Self {
        Self {
            action: RecoveryAction::RepairConfiguration,
            settings: None,
        }
    }

    pub const fn probe_again() -> Self {
        Self {
            action: RecoveryAction::ProbeAgain,
            settings: None,
        }
    }
}

/// Why a submission did not reach the provider.
///
/// This is deliberately an outcome rather than an opaque failure: callers can
/// distinguish user denial, a disabled channel, missing identity and an
/// unavailable provider, and can offer the matching recovery.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PostRejectReason {
    PermissionDenied {
        can_request_again: bool,
        recovery: RecoveryGuidance,
    },
    ChannelOrServerDisabled,
    MissingApplicationIdentity,
    UnavailableProvider,
    Unsupported(NotificationOperation),
}

impl PostRejectReason {
    pub const fn error_code(&self) -> ErrorCode {
        match self {
            Self::PermissionDenied { .. } => ErrorCode::PermissionDenied,
            Self::ChannelOrServerDisabled | Self::UnavailableProvider => ErrorCode::Unavailable,
            Self::MissingApplicationIdentity => ErrorCode::InvalidConfiguration,
            Self::Unsupported(_) => ErrorCode::Unsupported,
        }
    }

    pub fn recovery_guidance(&self) -> Option<RecoveryGuidance> {
        match self {
            Self::PermissionDenied { recovery, .. } => Some(*recovery),
            Self::ChannelOrServerDisabled => Some(RecoveryGuidance::notification_settings()),
            Self::MissingApplicationIdentity => Some(RecoveryGuidance::repair_configuration()),
            Self::UnavailableProvider => Some(RecoveryGuidance::probe_again()),
            Self::Unsupported(_) => None,
        }
    }

    pub const fn retry_advice(&self) -> RetryAdvice {
        match self {
            Self::PermissionDenied {
                can_request_again: true,
                ..
            } => RetryAdvice::Retry,
            Self::PermissionDenied { .. }
            | Self::ChannelOrServerDisabled
            | Self::MissingApplicationIdentity
            | Self::UnavailableProvider => RetryAdvice::AfterConditionsChange,
            Self::Unsupported(_) => RetryAdvice::Never,
        }
    }

    pub const fn message(&self) -> &'static str {
        match self {
            Self::PermissionDenied { .. } => "Notification permission is not authorized",
            Self::ChannelOrServerDisabled => "Notification channel or server is disabled",
            Self::MissingApplicationIdentity => {
                "Notification application identity is not configured"
            }
            Self::UnavailableProvider => "Notification provider is unavailable",
            Self::Unsupported(_) => "Notification operation is not supported",
        }
    }

    pub fn suppression_reason(&self) -> SuppressionReason {
        match self {
            Self::PermissionDenied { .. } => SuppressionReason::PermissionDenied,
            Self::ChannelOrServerDisabled => SuppressionReason::ChannelOrServerDisabled,
            Self::MissingApplicationIdentity => SuppressionReason::Configuration,
            Self::UnavailableProvider => SuppressionReason::ProviderUnavailable,
            Self::Unsupported(_) => SuppressionReason::Unsupported,
        }
    }

    pub fn into_error(self) -> SystemError {
        let mut error = SystemError::new(self.error_code(), self.message());
        error.recovery = self.recovery_guidance().map(|guidance| guidance.action);
        error.retry = self.retry_advice();
        error
    }
}

impl fmt::Display for PostRejectReason {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.message())
    }
}

/// How the provider classified an accepted submission.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubmissionState {
    /// The provider accepted the submission but cannot observe center delivery.
    AcceptedByProvider,
    /// The provider reports a handoff to the notification center. This still is
    /// not evidence that the notification was displayed to the user.
    PostedToCenter,
}

/// Receipt for a submission. It never asserts that the notification was shown.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PostReceipt {
    pub notification_id: NotificationId,
    pub provider_id: Option<ProviderNotificationId>,
    pub submission: SubmissionState,
    pub replaced_existing: Option<bool>,
    pub omitted_actions: bool,
    pub omitted_progress: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PostOutcome {
    Posted(PostReceipt),
    Rejected(PostRejectReason),
}

impl PostOutcome {
    pub fn receipt(&self) -> Option<&PostReceipt> {
        match self {
            Self::Posted(receipt) => Some(receipt),
            Self::Rejected(_) => None,
        }
    }

    pub fn rejection(&self) -> Option<&PostRejectReason> {
        match self {
            Self::Posted(_) => None,
            Self::Rejected(reason) => Some(reason),
        }
    }

    pub const fn is_posted(&self) -> bool {
        matches!(self, Self::Posted(_))
    }

    pub fn into_result(self) -> Result<PostReceipt, SystemError> {
        match self {
            Self::Posted(receipt) => Ok(receipt),
            Self::Rejected(reason) => Err(reason.into_error()),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WithdrawOutcome {
    Withdrawn,
    NotFoundOrAlreadyDismissed,
    Unsupported,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SuppressionReason {
    PermissionDenied,
    ChannelOrServerDisabled,
    Configuration,
    ProviderUnavailable,
    Unsupported,
}

/// A provider observation after a submission. Provider implementations enqueue
/// these; the service drains them outside all locks and publishes public events.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NotificationObservation {
    ObservedDisplayed {
        notification_id: NotificationId,
        provider_id: Option<ProviderNotificationId>,
    },
    Suppressed {
        notification_id: NotificationId,
        reason: SuppressionReason,
    },
    /// Out-of-band withdrawal only. `NotificationService::withdraw` publishes the
    /// authoritative `Withdrawn` event from the returned outcome, so providers must
    /// not enqueue a duplicate observation for that same call.
    Withdrawn {
        notification_id: NotificationId,
    },
    DismissedByUser {
        notification_id: NotificationId,
    },
    Activated {
        notification_id: NotificationId,
        action_id: Option<ActionId>,
    },
}

/// Bounded notification lifecycle events.
///
/// `Submitted` is submission evidence. `ObservedDisplayed` requires a provider
/// observation; `Activated` requires a user activation callback. They are kept
/// separate so a post receipt cannot be mistaken for a displayed notification.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NotificationEvent {
    Submitted {
        notification_id: NotificationId,
        submission: SubmissionState,
        replaced_existing: Option<bool>,
    },
    ObservedDisplayed {
        notification_id: NotificationId,
        provider_id: Option<ProviderNotificationId>,
    },
    Suppressed {
        notification_id: NotificationId,
        reason: SuppressionReason,
    },
    /// Out-of-band withdrawal only. `NotificationService::withdraw` publishes the
    /// authoritative `Withdrawn` event from the returned outcome, so providers must
    /// not enqueue a duplicate observation for that same call.
    Withdrawn {
        notification_id: NotificationId,
    },
    DismissedByUser {
        notification_id: NotificationId,
    },
    Activated {
        notification_id: NotificationId,
        action_id: Option<ActionId>,
    },
}

impl NotificationEvent {
    pub fn notification_id(&self) -> &NotificationId {
        match self {
            Self::Submitted {
                notification_id, ..
            }
            | Self::ObservedDisplayed {
                notification_id, ..
            }
            | Self::Suppressed {
                notification_id, ..
            }
            | Self::Withdrawn { notification_id }
            | Self::DismissedByUser { notification_id }
            | Self::Activated {
                notification_id, ..
            } => notification_id,
        }
    }
}

impl From<NotificationObservation> for NotificationEvent {
    fn from(observation: NotificationObservation) -> Self {
        match observation {
            NotificationObservation::ObservedDisplayed {
                notification_id,
                provider_id,
            } => Self::ObservedDisplayed {
                notification_id,
                provider_id,
            },
            NotificationObservation::Suppressed {
                notification_id,
                reason,
            } => Self::Suppressed {
                notification_id,
                reason,
            },
            NotificationObservation::Withdrawn { notification_id } => {
                Self::Withdrawn { notification_id }
            }
            NotificationObservation::DismissedByUser { notification_id } => {
                Self::DismissedByUser { notification_id }
            }
            NotificationObservation::Activated {
                notification_id,
                action_id,
            } => Self::Activated {
                notification_id,
                action_id,
            },
        }
    }
}

/// Operations exposed by a local notification provider.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum NotificationOperation {
    QueryAuthorization,
    RequestAuthorization,
    PostImmediate,
    ReplaceById,
    WithdrawById,
    ActionButtons,
    ProgressUpdates,
    DeliveryReceipt,
    ActivationRouting,
}

impl NotificationOperation {
    pub const ALL: [Self; 9] = [
        Self::QueryAuthorization,
        Self::RequestAuthorization,
        Self::PostImmediate,
        Self::ReplaceById,
        Self::WithdrawById,
        Self::ActionButtons,
        Self::ProgressUpdates,
        Self::DeliveryReceipt,
        Self::ActivationRouting,
    ];

    pub const fn label(self) -> &'static str {
        match self {
            Self::QueryAuthorization => "QueryAuthorization",
            Self::RequestAuthorization => "RequestAuthorization",
            Self::PostImmediate => "PostImmediate",
            Self::ReplaceById => "ReplaceById",
            Self::WithdrawById => "WithdrawById",
            Self::ActionButtons => "ActionButtons",
            Self::ProgressUpdates => "ProgressUpdates",
            Self::DeliveryReceipt => "DeliveryReceipt",
            Self::ActivationRouting => "ActivationRouting",
        }
    }

    pub const fn is_authorization(self) -> bool {
        matches!(self, Self::QueryAuthorization | Self::RequestAuthorization)
    }
}

/// Conservative five-platform contract. No native provider is installed by this
/// crate; platform providers and their runtime evidence remain separate work.
pub const PROVIDER_MANIFEST: [ProviderManifest; 5] = [
    ProviderManifest {
        platform: Platform::Windows,
        native_provider_installed: false,
        evidence: Evidence::ContractOnly,
    },
    ProviderManifest {
        platform: Platform::MacOs,
        native_provider_installed: false,
        evidence: Evidence::ContractOnly,
    },
    ProviderManifest {
        platform: Platform::Linux,
        native_provider_installed: false,
        evidence: Evidence::ContractOnly,
    },
    ProviderManifest {
        platform: Platform::Android,
        native_provider_installed: false,
        evidence: Evidence::ContractOnly,
    },
    ProviderManifest {
        platform: Platform::Ios,
        native_provider_installed: false,
        evidence: Evidence::ContractOnly,
    },
];

pub fn unsupported_capability(
    operation: NotificationOperation,
) -> Capability<NotificationOperation> {
    Capability {
        operation,
        support: Support::Unsupported,
        availability: Availability::Unknown,
        configuration: Configuration::Unknown,
        permission: PermissionState::Unknown,
        evidence: Evidence::ContractOnly,
        constraints: Vec::<Constraint>::new(),
        observed_revision: 0,
    }
}

/// Whether the application has the packaged identity a notification provider
/// needs (bundle identifier, AUMID or equivalent).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApplicationIdentity {
    Ready,
    Missing,
}
