//! The explicit OS scheduled-notification provider boundary, plus deterministic
//! fake and fail-closed unavailable implementations.
//!
//! A provider schedules a notification with the host notification center or
//! alarm service. Scheduling the same identity with a newer `schedule_epoch`
//! must atomically replace the older registration; older epochs become stale
//! and their delivery/activation callbacks are rejected by the service.

use crate::model::{
    DeliveryObservation, InexactDowngradeReason, NotificationScheduleStatus, ScheduleAttachment,
    SchedulePrecision, ScheduleTrigger, ScheduledNotificationId, stale_epoch_error,
    validate_attachments,
};
use crate::platform::{PlatformTarget, ScheduleOperation};
use crate::timezone::TimeZoneSpec;
use gpui_notifications::NotificationContent;
use gpui_system::{
    Availability, Capability, Configuration, ErrorCode, Evidence, PermissionState,
    ProviderManifest, Support, SystemError,
};
use std::{
    any::Any,
    collections::BTreeMap,
    sync::{Arc, Mutex},
};

fn unavailable() -> SystemError {
    SystemError::new(
        ErrorCode::Unavailable,
        "No native notification schedule provider is installed",
    )
}

/// One request to the host notification scheduler.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScheduledNotificationRequest {
    pub schedule_id: ScheduledNotificationId,
    pub schedule_epoch: u64,
    /// The next registered provider instant. For a coalesced catch-up this is
    /// the immediate catch-up instant, not the logical next occurrence.
    pub fire_at_epoch_ms: u64,
    pub precision: SchedulePrecision,
    pub trigger: ScheduleTrigger,
    pub time_zone: TimeZoneSpec,
    pub content: NotificationContent,
    pub attachments: Vec<ScheduleAttachment>,
}

impl ScheduledNotificationRequest {
    pub fn validate(&self) -> Result<(), SystemError> {
        if self.schedule_epoch == 0 || self.fire_at_epoch_ms == 0 {
            return Err(SystemError::new(
                ErrorCode::InvalidInput,
                "Provider request identity or instant is invalid",
            ));
        }
        self.trigger.validate()?;
        self.time_zone.validate_shape()?;
        self.content.validate()?;
        validate_attachments(&self.attachments)?;
        Ok(())
    }
}

/// The provider's admission receipt. It is submission evidence only; it never
/// asserts that the notification was displayed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderScheduleReceipt {
    pub schedule_id: ScheduledNotificationId,
    pub schedule_epoch: u64,
    pub provider_token: u64,
    pub accepted_at_epoch_ms: u64,
    pub precision: SchedulePrecision,
}

impl ProviderScheduleReceipt {
    pub fn validate_for(&self, request: &ScheduledNotificationRequest) -> Result<(), SystemError> {
        if self.schedule_id != request.schedule_id
            || self.schedule_epoch != request.schedule_epoch
            || self.provider_token == 0
            || self.accepted_at_epoch_ms == 0
        {
            return Err(SystemError::new(
                ErrorCode::IntegrityFailure,
                "Provider receipt does not match its request",
            ));
        }
        if self.precision == SchedulePrecision::ExactAlarm
            && request.precision != SchedulePrecision::ExactAlarm
        {
            return Err(SystemError::new(
                ErrorCode::IntegrityFailure,
                "Provider claimed exact precision for an advisory request",
            ));
        }
        Ok(())
    }
}

/// One pending registration reported by the provider.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingProviderSchedule {
    pub schedule_id: ScheduledNotificationId,
    pub schedule_epoch: u64,
    pub provider_token: u64,
    pub fire_at_epoch_ms: u64,
    pub precision: SchedulePrecision,
}

/// Whether a notification channel can deliver.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ChannelAvailability {
    /// The provider cannot authoritatively report the channel state.
    Unspecified,
    Enabled,
    Disabled,
    Muted,
}

impl ChannelAvailability {
    pub const fn allows_delivery(self) -> bool {
        !matches!(self, Self::Disabled | Self::Muted)
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Unspecified => "unspecified",
            Self::Enabled => "enabled",
            Self::Disabled => "disabled",
            Self::Muted => "muted",
        }
    }
}

/// Native/OS scheduling boundary for local notifications.
pub trait NotificationScheduleProvider: Send + Sync + 'static {
    fn capability(&self, operation: ScheduleOperation) -> Capability<ScheduleOperation>;

    fn capabilities(&self) -> Vec<Capability<ScheduleOperation>> {
        ScheduleOperation::ALL
            .into_iter()
            .map(|operation| self.capability(operation))
            .collect()
    }

    /// Upsert this identity at the requested epoch. A newer epoch replaces the
    /// older registration; a stale epoch is rejected.
    fn schedule(
        &self,
        request: &ScheduledNotificationRequest,
    ) -> Result<ProviderScheduleReceipt, SystemError>;

    /// Withdraw the registration for `schedule_id` at `schedule_epoch` when that
    /// epoch is still current. Cancelling an absent registration is a no-op.
    fn cancel(
        &self,
        schedule_id: &ScheduledNotificationId,
        schedule_epoch: u64,
    ) -> Result<(), SystemError>;

    fn pending_schedules(&self) -> Result<Vec<PendingProviderSchedule>, SystemError>;

    fn pending_ids(&self) -> Result<Vec<ScheduledNotificationId>, SystemError> {
        Ok(self
            .pending_schedules()?
            .into_iter()
            .map(|pending| pending.schedule_id)
            .collect())
    }

    fn channel_status(&self, _channel_id: &str) -> Result<ChannelAvailability, SystemError> {
        Ok(ChannelAvailability::Unspecified)
    }

    fn as_any(&self) -> &dyn Any;
}

/// Conservative manifest: this contract crate installs no native provider.
pub const PROVIDER_MANIFEST: [ProviderManifest; 5] = gpui_system::PROVIDER_MANIFEST;

/// True only when a real native provider is installed for the target. This
/// crate installs [`UnavailableNotificationScheduleProvider`] everywhere, so
/// the answer is always `false`.
pub fn durable_scheduling_available(_platform: PlatformTarget) -> bool {
    false
}

/// Contract-only provider used when no host adapter is installed.
#[derive(Debug, Clone, Copy, Default)]
pub struct UnavailableNotificationScheduleProvider;

impl UnavailableNotificationScheduleProvider {
    pub const fn new() -> Self {
        Self
    }
}

impl NotificationScheduleProvider for UnavailableNotificationScheduleProvider {
    fn capability(&self, operation: ScheduleOperation) -> Capability<ScheduleOperation> {
        Capability {
            operation,
            support: Support::Unsupported,
            availability: Availability::Unavailable,
            configuration: Configuration::Ready,
            permission: PermissionState::NotRequired,
            evidence: Evidence::ContractOnly,
            constraints: Vec::new(),
            observed_revision: 0,
        }
    }

    fn schedule(
        &self,
        _request: &ScheduledNotificationRequest,
    ) -> Result<ProviderScheduleReceipt, SystemError> {
        Err(unavailable())
    }

    fn cancel(
        &self,
        _schedule_id: &ScheduledNotificationId,
        _schedule_epoch: u64,
    ) -> Result<(), SystemError> {
        Err(unavailable())
    }

    fn pending_schedules(&self) -> Result<Vec<PendingProviderSchedule>, SystemError> {
        Err(unavailable())
    }

    fn channel_status(&self, _channel_id: &str) -> Result<ChannelAvailability, SystemError> {
        Err(unavailable())
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

#[derive(Debug, Clone)]
struct PendingEntry {
    schedule_epoch: u64,
    provider_token: u64,
    fire_at_epoch_ms: u64,
    precision: SchedulePrecision,
}

#[derive(Debug)]
struct FakeState {
    capabilities: BTreeMap<ScheduleOperation, Capability<ScheduleOperation>>,
    pending: BTreeMap<ScheduledNotificationId, PendingEntry>,
    requests: Vec<ScheduledNotificationRequest>,
    cancels: Vec<(ScheduledNotificationId, u64)>,
    channels: BTreeMap<String, ChannelAvailability>,
    unavailable: bool,
    fail_next_schedule: Option<SystemError>,
    fail_next_cancel: Option<SystemError>,
    fail_next_pending: bool,
    next_token: u64,
    revision: u64,
}

/// Deterministic provider for schedule requests, replacements, cancellations,
/// orphan injection, simulated restarts and simulated presentation/activation.
#[derive(Clone)]
pub struct FakeNotificationScheduleProvider {
    state: Arc<Mutex<FakeState>>,
}

impl Default for FakeNotificationScheduleProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl FakeNotificationScheduleProvider {
    pub fn new() -> Self {
        let capabilities = ScheduleOperation::ALL
            .into_iter()
            .map(|operation| (operation, baseline_capability(operation, 1)))
            .collect();
        Self {
            state: Arc::new(Mutex::new(FakeState {
                capabilities,
                pending: BTreeMap::new(),
                requests: Vec::new(),
                cancels: Vec::new(),
                channels: BTreeMap::new(),
                unavailable: false,
                fail_next_schedule: None,
                fail_next_cancel: None,
                fail_next_pending: false,
                next_token: 1,
                revision: 1,
            })),
        }
    }

    pub fn set_unavailable(&self, unavailable: bool) {
        lock(&self.state).unavailable = unavailable;
    }

    pub fn fail_next_schedule(&self) {
        self.fail_next_schedule_with(unavailable());
    }

    pub fn fail_next_schedule_with(&self, error: SystemError) {
        lock(&self.state).fail_next_schedule = Some(error);
    }

    pub fn fail_next_cancel(&self) {
        lock(&self.state).fail_next_cancel = Some(unavailable());
    }

    pub fn fail_next_pending(&self) {
        lock(&self.state).fail_next_pending = true;
    }

    pub fn set_capability(
        &self,
        operation: ScheduleOperation,
        capability: Capability<ScheduleOperation>,
    ) {
        let mut state = lock(&self.state);
        state.revision = state.revision.saturating_add(1);
        state.capabilities.insert(operation, capability);
    }

    pub fn set_support(&self, operation: ScheduleOperation, support: Support) {
        let mut state = lock(&self.state);
        state.revision = state.revision.saturating_add(1);
        let revision = state.revision;
        if let Some(capability) = state.capabilities.get_mut(&operation) {
            capability.support = support;
            capability.observed_revision = revision;
        }
    }

    pub fn set_availability(&self, operation: ScheduleOperation, availability: Availability) {
        let mut state = lock(&self.state);
        state.revision = state.revision.saturating_add(1);
        let revision = state.revision;
        if let Some(capability) = state.capabilities.get_mut(&operation) {
            capability.availability = availability;
            capability.observed_revision = revision;
        }
    }

    pub fn set_configuration(&self, operation: ScheduleOperation, configuration: Configuration) {
        let mut state = lock(&self.state);
        state.revision = state.revision.saturating_add(1);
        let revision = state.revision;
        if let Some(capability) = state.capabilities.get_mut(&operation) {
            capability.configuration = configuration;
            capability.observed_revision = revision;
        }
    }

    pub fn set_channel(&self, channel_id: impl Into<String>, availability: ChannelAvailability) {
        lock(&self.state)
            .channels
            .insert(channel_id.into(), availability);
    }

    pub fn requests(&self) -> Vec<ScheduledNotificationRequest> {
        lock(&self.state).requests.clone()
    }

    pub fn request_count(&self) -> usize {
        lock(&self.state).requests.len()
    }

    pub fn latest_request(&self) -> Option<ScheduledNotificationRequest> {
        lock(&self.state).requests.last().cloned()
    }

    pub fn cancel_calls(&self) -> Vec<(ScheduledNotificationId, u64)> {
        lock(&self.state).cancels.clone()
    }

    pub fn cancel_call_count(&self) -> usize {
        lock(&self.state).cancels.len()
    }

    pub fn pending_schedules(&self) -> Vec<PendingProviderSchedule> {
        lock(&self.state)
            .pending
            .iter()
            .map(|(schedule_id, pending)| PendingProviderSchedule {
                schedule_id: schedule_id.clone(),
                schedule_epoch: pending.schedule_epoch,
                provider_token: pending.provider_token,
                fire_at_epoch_ms: pending.fire_at_epoch_ms,
                precision: pending.precision,
            })
            .collect()
    }

    pub fn pending_count(&self) -> usize {
        lock(&self.state).pending.len()
    }

    /// Insert a pending registration that the service journal does not know
    /// about, to exercise orphan cancellation.
    pub fn inject_orphan(
        &self,
        schedule_id: ScheduledNotificationId,
        schedule_epoch: u64,
        fire_at_epoch_ms: u64,
    ) {
        let mut state = lock(&self.state);
        let token = state.next_token;
        state.next_token = state.next_token.saturating_add(1);
        state.pending.insert(
            schedule_id,
            PendingEntry {
                schedule_epoch,
                provider_token: token,
                fire_at_epoch_ms,
                precision: SchedulePrecision::InexactAdvisory,
            },
        );
    }

    pub fn force_pending_epoch(&self, schedule_id: &ScheduledNotificationId, schedule_epoch: u64) {
        if let Some(pending) = lock(&self.state).pending.get_mut(schedule_id) {
            pending.schedule_epoch = schedule_epoch;
        }
    }

    pub fn remove_pending(&self, schedule_id: &ScheduledNotificationId) -> bool {
        lock(&self.state).pending.remove(schedule_id).is_some()
    }

    /// Simulate a provider process restart: call logs are cleared, while the OS
    /// pending registrations survive.
    pub fn simulate_restart(&self) {
        let mut state = lock(&self.state);
        state.requests.clear();
        state.cancels.clear();
        state.fail_next_schedule = None;
        state.fail_next_cancel = None;
        state.fail_next_pending = false;
    }

    /// Simulate a reboot or OS purge that loses every pending registration.
    pub fn simulate_os_lost_pending(&self) {
        lock(&self.state).pending.clear();
    }

    /// Simulate a platform presentation callback. The epoch must match the
    /// pending registration; the fake never invents presentation evidence for a
    /// different epoch.
    pub fn simulate_presentation(
        &self,
        schedule_id: &ScheduledNotificationId,
        schedule_epoch: u64,
        presented_at_epoch_ms: u64,
    ) -> Result<DeliveryObservation, SystemError> {
        let state = lock(&self.state);
        let pending = state.pending.get(schedule_id).ok_or_else(|| {
            SystemError::new(
                ErrorCode::Unavailable,
                "No pending registration exists for this schedule",
            )
        })?;
        if pending.schedule_epoch != schedule_epoch {
            return Err(stale_epoch_error());
        }
        let observation =
            DeliveryObservation::new(schedule_id.clone(), schedule_epoch, presented_at_epoch_ms)
                .with_provider_token(pending.provider_token);
        Ok(observation)
    }

    /// Simulate a user tap or action invocation callback.
    pub fn simulate_activation(
        &self,
        schedule_id: &ScheduledNotificationId,
        schedule_epoch: u64,
        action_id: Option<gpui_notifications::ActionId>,
        text_input: Option<String>,
        activated_at_epoch_ms: u64,
    ) -> crate::model::ActivationObservation {
        crate::model::ActivationObservation {
            schedule_id: schedule_id.clone(),
            schedule_epoch,
            action_id,
            text_input,
            activated_at_epoch_ms,
        }
    }
}

fn baseline_capability(
    operation: ScheduleOperation,
    revision: u64,
) -> Capability<ScheduleOperation> {
    let support = match operation {
        ScheduleOperation::ScheduleExactAlarm => Support::Conditional,
        ScheduleOperation::ObserveScheduledDelivery => Support::Conditional,
        ScheduleOperation::RepeatAndCalendarSchedule
        | ScheduleOperation::AttachMediaToSchedule
        | ScheduleOperation::ReconcilePendingSchedules
        | ScheduleOperation::ScheduleNotification => Support::Native,
    };
    Capability {
        operation,
        support,
        availability: Availability::Available,
        configuration: Configuration::Ready,
        permission: PermissionState::NotRequired,
        evidence: Evidence::ContractOnly,
        constraints: Vec::new(),
        observed_revision: revision,
    }
}

impl NotificationScheduleProvider for FakeNotificationScheduleProvider {
    fn capability(&self, operation: ScheduleOperation) -> Capability<ScheduleOperation> {
        lock(&self.state)
            .capabilities
            .get(&operation)
            .cloned()
            .expect("the fake provider contains every schedule operation")
    }

    fn schedule(
        &self,
        request: &ScheduledNotificationRequest,
    ) -> Result<ProviderScheduleReceipt, SystemError> {
        request.validate()?;
        let mut state = lock(&self.state);
        if state.unavailable {
            return Err(unavailable());
        }
        if let Some(error) = state.fail_next_schedule.take() {
            return Err(error);
        }
        if let Some(pending) = state.pending.get(&request.schedule_id)
            && pending.schedule_epoch > request.schedule_epoch
        {
            return Err(stale_epoch_error());
        }
        let token = state.next_token;
        state.next_token = state.next_token.saturating_add(1);
        state.requests.push(request.clone());
        state.pending.insert(
            request.schedule_id.clone(),
            PendingEntry {
                schedule_epoch: request.schedule_epoch,
                provider_token: token,
                fire_at_epoch_ms: request.fire_at_epoch_ms,
                precision: request.precision,
            },
        );
        Ok(ProviderScheduleReceipt {
            schedule_id: request.schedule_id.clone(),
            schedule_epoch: request.schedule_epoch,
            provider_token: token,
            accepted_at_epoch_ms: request.fire_at_epoch_ms,
            precision: request.precision,
        })
    }

    fn cancel(
        &self,
        schedule_id: &ScheduledNotificationId,
        schedule_epoch: u64,
    ) -> Result<(), SystemError> {
        let mut state = lock(&self.state);
        if state.unavailable {
            return Err(unavailable());
        }
        if let Some(error) = state.fail_next_cancel.take() {
            return Err(error);
        }
        state.cancels.push((schedule_id.clone(), schedule_epoch));
        match state.pending.get(schedule_id) {
            Some(pending) if pending.schedule_epoch > schedule_epoch => Err(stale_epoch_error()),
            Some(_) => {
                state.pending.remove(schedule_id);
                Ok(())
            }
            None => Ok(()),
        }
    }

    fn pending_schedules(&self) -> Result<Vec<PendingProviderSchedule>, SystemError> {
        let mut state = lock(&self.state);
        if state.fail_next_pending {
            state.fail_next_pending = false;
            return Err(unavailable());
        }
        Ok(state
            .pending
            .iter()
            .map(|(schedule_id, pending)| PendingProviderSchedule {
                schedule_id: schedule_id.clone(),
                schedule_epoch: pending.schedule_epoch,
                provider_token: pending.provider_token,
                fire_at_epoch_ms: pending.fire_at_epoch_ms,
                precision: pending.precision,
            })
            .collect())
    }

    fn channel_status(&self, channel_id: &str) -> Result<ChannelAvailability, SystemError> {
        Ok(lock(&self.state)
            .channels
            .get(channel_id)
            .copied()
            .unwrap_or(ChannelAvailability::Unspecified))
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// Classify an exact-alarm downgrade reason from a failed capability check.
pub(crate) fn exact_alarm_downgrade_reason(error: &SystemError) -> InexactDowngradeReason {
    if error.code == ErrorCode::PermissionDenied {
        InexactDowngradeReason::ExactAlarmPermissionDenied
    } else {
        InexactDowngradeReason::ExactAlarmCapabilityUnavailable
    }
}

/// Whether a provider still reports the record as pending and in sync.
pub(crate) fn pending_is_in_sync(
    record: &crate::model::NotificationScheduleRecord,
    pending: &PendingProviderSchedule,
) -> bool {
    record.status == NotificationScheduleStatus::Scheduled
        && pending.schedule_epoch == record.schedule_epoch
        && Some(pending.fire_at_epoch_ms) == record.expected_provider_fire_at()
        && pending.precision == record.precision
}

pub(crate) fn lock<T>(value: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    value
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{ScheduleSubmission, ScheduledNotificationId};

    fn id() -> ScheduledNotificationId {
        ScheduledNotificationId::parse("application/reminder").expect("id")
    }

    fn request(epoch: u64) -> ScheduledNotificationRequest {
        let trigger = ScheduleTrigger::AfterDelay { delay_ms: 60_000 };
        let submission = ScheduleSubmission::new(
            id(),
            trigger.clone(),
            NotificationContent::new("Reminder", "Stand up").expect("content"),
        );
        ScheduledNotificationRequest {
            schedule_id: submission.schedule_id,
            schedule_epoch: epoch,
            fire_at_epoch_ms: 1_060_000,
            precision: SchedulePrecision::InexactAdvisory,
            trigger,
            time_zone: TimeZoneSpec::Utc,
            content: submission.content,
            attachments: Vec::new(),
        }
    }

    #[test]
    fn fake_upserts_by_epoch_and_rejects_stale_cancel() {
        let provider = FakeNotificationScheduleProvider::new();
        let receipt = provider.schedule(&request(2)).expect("scheduled");
        assert_eq!(receipt.schedule_epoch, 2);
        let newer = provider.schedule(&request(3)).expect("replaced");
        assert!(newer.provider_token > receipt.provider_token);
        assert_eq!(provider.pending_count(), 1);
        assert!(provider.cancel(&id(), 2).is_err());
        assert!(provider.cancel(&id(), 3).is_ok());
        assert_eq!(provider.pending_count(), 0);
        assert_eq!(provider.request_count(), 2);
    }

    #[test]
    fn fake_reports_errors_and_simulates_restart() {
        let provider = FakeNotificationScheduleProvider::new();
        provider.fail_next_schedule();
        assert!(provider.schedule(&request(1)).is_err());
        assert!(provider.schedule(&request(1)).is_ok());
        provider.fail_next_pending();
        assert!(NotificationScheduleProvider::pending_schedules(&provider).is_err());
        provider.simulate_os_lost_pending();
        assert_eq!(provider.pending_count(), 0);
        provider.simulate_restart();
        assert_eq!(provider.request_count(), 0);
        provider.set_unavailable(true);
        assert!(provider.schedule(&request(2)).is_err());
    }

    #[test]
    fn unavailable_provider_fails_closed() {
        let provider = UnavailableNotificationScheduleProvider::new();
        for operation in ScheduleOperation::ALL {
            assert_eq!(provider.capability(operation).support, Support::Unsupported);
            assert_eq!(
                provider.capability(operation).evidence,
                Evidence::ContractOnly
            );
        }
        assert!(provider.schedule(&request(1)).is_err());
        assert!(NotificationScheduleProvider::pending_schedules(&provider).is_err());
        assert!(!durable_scheduling_available(PlatformTarget::Ios));
    }
}
