//! Durable scheduled-notification coordinator.
//!
//! Every mutating entry point requires [`ExecutionOwner::Background`], honors
//! cancellation before side effects, persists the next durable state before
//! publishing it in memory, and emits events only after all locks are released.
//! Submission, presentation and activation are separate evidence stages.

use crate::ExecutionOwner;
use crate::clock::lock;
use crate::model::{
    ActivationObservation, DeliveryObservation, DeliveryObservationReceipt,
    DeliveryObservationSummary, ExactAlarmFallback, InexactDowngrade, InexactDowngradeReason,
    MissedFirePolicy, NotificationScheduleEvent, NotificationScheduleRecord,
    NotificationScheduleStatus, RetentionPolicy, ScheduleConflictPolicy, SchedulePrecision,
    ScheduleSubmission, ScheduleSubmissionReceipt, ScheduleTrigger, ScheduledActivationHandoff,
    ScheduledNotificationId, SubmissionDisposition, SuspensionReason, capacity_error,
    conflict_error, count_missed_occurrences, exact_alarm_permission_kind, stale_epoch_error,
};
use crate::platform::ScheduleOperation;
use crate::provider::{
    ChannelAvailability, NotificationScheduleProvider, PendingProviderSchedule,
    ScheduledNotificationRequest, exact_alarm_downgrade_reason, pending_is_in_sync,
};
use crate::store::{
    MAX_SCHEDULE_RECORD_BYTES, NOTIFICATION_SCHEDULE_FORMAT_VERSION, ScheduleStorageHook,
    ScheduleStore,
};
use crate::timezone::{ResolvedTimeZone, TimeZoneResolver};
use gpui_activation::{ActivationId, ActivationPayload, ActivationSource};
use gpui_app_data::AppData;
use gpui_notifications::ActionId;
use gpui_permissions::{PermissionKind, PermissionStatus};
use gpui_system::{
    CancellationToken, ErrorCode, EventSender, EventSubscription, OperationContext, RecoveryAction,
    RetryAdvice, SystemError,
};
use std::{
    collections::BTreeMap,
    path::Path,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
};

/// A fresh permission observation source used by submission and reconciliation.
/// Implementations may be dynamic; the service calls them afresh for each check.
pub trait SchedulePermissionQuery: Send + Sync + 'static {
    fn status(&self, kind: PermissionKind) -> PermissionStatus;
}

/// A mutable, test-friendly authorization source. It is a live query, not a
/// durable authorization grant.
#[derive(Debug)]
pub struct ScheduleAuthorization {
    statuses: Mutex<(PermissionStatus, PermissionStatus)>,
}

impl Default for ScheduleAuthorization {
    fn default() -> Self {
        Self::granting()
    }
}

impl ScheduleAuthorization {
    pub fn new(notifications: PermissionStatus, exact_alarm: PermissionStatus) -> Self {
        Self {
            statuses: Mutex::new((notifications, exact_alarm)),
        }
    }

    pub fn granting() -> Self {
        Self::new(PermissionStatus::Granted, PermissionStatus::Granted)
    }

    pub fn set_notifications(&self, status: PermissionStatus) {
        lock(&self.statuses).0 = status;
    }

    pub fn set_exact_alarm(&self, status: PermissionStatus) {
        lock(&self.statuses).1 = status;
    }

    pub fn notifications(&self) -> PermissionStatus {
        lock(&self.statuses).0
    }

    pub fn exact_alarm(&self) -> PermissionStatus {
        lock(&self.statuses).1
    }
}

impl SchedulePermissionQuery for ScheduleAuthorization {
    fn status(&self, kind: PermissionKind) -> PermissionStatus {
        match kind {
            PermissionKind::Notifications => lock(&self.statuses).0,
            PermissionKind::SpecialAccess(name)
                if name == crate::model::SCHEDULE_EXACT_ALARM_PERMISSION =>
            {
                lock(&self.statuses).1
            }
            _ => PermissionStatus::Unavailable,
        }
    }
}

struct ServiceState {
    records: BTreeMap<ScheduledNotificationId, NotificationScheduleRecord>,
    subscribers: Vec<(Arc<AtomicBool>, EventSender<NotificationScheduleEvent>)>,
    event_capacity: usize,
    retention: RetentionPolicy,
}

struct ServiceInner {
    state: Mutex<ServiceState>,
    operation: Mutex<()>,
    provider: Arc<dyn NotificationScheduleProvider>,
    clock: Arc<dyn crate::clock::ScheduleClock>,
    resolver: Arc<dyn TimeZoneResolver>,
    permissions: Arc<dyn SchedulePermissionQuery>,
    store: Option<ScheduleStore>,
    next_epoch: AtomicU64,
}

/// Results of restart reconciliation.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ReconciliationReport {
    pub journal_records: usize,
    pub active_records: usize,
    pub in_sync: usize,
    pub resubmitted: usize,
    pub resubmit_failures: usize,
    pub cancelled_orphans: usize,
    pub quarantined: usize,
    pub suspended: usize,
}

/// Results of clock-jump, DST and timezone reconciliation.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ClockReconciliationReport {
    pub scanned: usize,
    pub clock_jumps: usize,
    pub timezone_updates: usize,
    pub catch_up_resubmissions: usize,
    pub advanced: usize,
    pub expired: usize,
    pub provider_failures: usize,
}

/// Results of permission and channel reconciliation.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PermissionReconciliationReport {
    pub scanned: usize,
    pub suspended: usize,
    pub resumed: usize,
    pub downgraded: usize,
    pub provider_failures: usize,
}

/// Results of terminal-record retention pruning.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RetentionReport {
    pub removed: usize,
    pub remaining_terminal: usize,
}

/// Result of an explicit cancellation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CancellationReceipt {
    pub schedule_epoch: u64,
    pub provider_cancelled: bool,
    pub already_terminal: bool,
}

/// The durable scheduled-notification coordinator.
#[derive(Clone)]
pub struct NotificationScheduleService {
    inner: Arc<ServiceInner>,
}

impl std::fmt::Debug for NotificationScheduleService {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("NotificationScheduleService")
            .field("schedules", &self.schedule_count())
            .field("durable", &self.inner.store.is_some())
            .finish()
    }
}

impl NotificationScheduleService {
    pub fn open(
        app: &AppData,
        provider: Arc<dyn NotificationScheduleProvider>,
        clock: Arc<dyn crate::clock::ScheduleClock>,
        resolver: Arc<dyn TimeZoneResolver>,
        permissions: Arc<dyn SchedulePermissionQuery>,
    ) -> Result<Self, SystemError> {
        Self::open_with_options(
            app,
            provider,
            clock,
            resolver,
            permissions,
            RetentionPolicy::default(),
            Arc::new(crate::store::NoopScheduleStorageHook),
        )
    }

    pub fn open_with_options(
        app: &AppData,
        provider: Arc<dyn NotificationScheduleProvider>,
        clock: Arc<dyn crate::clock::ScheduleClock>,
        resolver: Arc<dyn TimeZoneResolver>,
        permissions: Arc<dyn SchedulePermissionQuery>,
        retention: RetentionPolicy,
        hook: Arc<dyn ScheduleStorageHook>,
    ) -> Result<Self, SystemError> {
        retention.validate()?;
        let store = ScheduleStore::open(app, hook)?;
        Self::from_parts(
            Some(store),
            provider,
            clock,
            resolver,
            permissions,
            retention,
        )
    }

    pub fn in_memory(
        provider: Arc<dyn NotificationScheduleProvider>,
        clock: Arc<dyn crate::clock::ScheduleClock>,
        resolver: Arc<dyn TimeZoneResolver>,
        permissions: Arc<dyn SchedulePermissionQuery>,
    ) -> Result<Self, SystemError> {
        Self::from_parts(
            None,
            provider,
            clock,
            resolver,
            permissions,
            RetentionPolicy::default(),
        )
    }

    fn from_parts(
        store: Option<ScheduleStore>,
        provider: Arc<dyn NotificationScheduleProvider>,
        clock: Arc<dyn crate::clock::ScheduleClock>,
        resolver: Arc<dyn TimeZoneResolver>,
        permissions: Arc<dyn SchedulePermissionQuery>,
        retention: RetentionPolicy,
    ) -> Result<Self, SystemError> {
        retention.validate()?;
        let seed_epoch = store
            .as_ref()
            .map(|store| store.max_persisted_epoch())
            .unwrap_or(0)
            .saturating_add(1)
            .max(1);
        Ok(Self {
            inner: Arc::new(ServiceInner {
                state: Mutex::new(ServiceState {
                    records: BTreeMap::new(),
                    subscribers: Vec::new(),
                    event_capacity: crate::model::DEFAULT_NOTIFICATION_SCHEDULE_EVENT_CAPACITY,
                    retention,
                }),
                operation: Mutex::new(()),
                provider,
                clock,
                resolver,
                permissions,
                store,
                next_epoch: AtomicU64::new(seed_epoch),
            }),
        })
    }

    pub fn provider(&self) -> &Arc<dyn NotificationScheduleProvider> {
        &self.inner.provider
    }

    pub fn clock(&self) -> &Arc<dyn crate::clock::ScheduleClock> {
        &self.inner.clock
    }

    pub fn resolver(&self) -> &Arc<dyn TimeZoneResolver> {
        &self.inner.resolver
    }

    pub fn permissions(&self) -> &Arc<dyn SchedulePermissionQuery> {
        &self.inner.permissions
    }

    pub fn retention_policy(&self) -> RetentionPolicy {
        lock(&self.inner.state).retention
    }

    /// Private namespace path, when this service is durable.
    pub fn namespace_path(&self) -> Option<&Path> {
        self.inner.store.as_ref().map(ScheduleStore::directory)
    }

    pub const fn format_version(&self) -> u32 {
        NOTIFICATION_SCHEDULE_FORMAT_VERSION
    }

    pub const fn max_record_bytes(&self) -> u64 {
        MAX_SCHEDULE_RECORD_BYTES
    }

    pub fn shares_state_with(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.inner, &other.inner)
    }

    pub fn schedule_count(&self) -> usize {
        lock(&self.inner.state).records.len()
    }

    pub fn schedule(&self, id: &ScheduledNotificationId) -> Option<NotificationScheduleRecord> {
        lock(&self.inner.state).records.get(id).cloned()
    }

    pub fn schedules(&self) -> Vec<NotificationScheduleRecord> {
        lock(&self.inner.state).records.values().cloned().collect()
    }

    pub fn active_schedules(&self) -> Vec<NotificationScheduleRecord> {
        lock(&self.inner.state)
            .records
            .values()
            .filter(|record| record.is_active())
            .cloned()
            .collect()
    }

    pub fn subscribe(&self) -> EventSubscription<NotificationScheduleEvent> {
        let mut state = lock(&self.inner.state);
        state
            .subscribers
            .retain(|(live, _)| live.load(Ordering::Acquire));
        let live = Arc::new(AtomicBool::new(true));
        let cleanup_live = Arc::clone(&live);
        let (sender, subscription) = gpui_system::event_channel(state.event_capacity, move || {
            cleanup_live.store(false, Ordering::Release);
        });
        state.subscribers.push((live, sender));
        subscription
    }

    pub fn event_revision(&self) -> u64 {
        let mut state = lock(&self.inner.state);
        state
            .subscribers
            .retain(|(live, _)| live.load(Ordering::Acquire));
        state
            .subscribers
            .iter()
            .map(|(_, sender)| sender.revision())
            .max()
            .unwrap_or(0)
    }

    /// Rehydrate the durable journal, replacing in-memory records and emitting
    /// one quarantine event per corrupt file.
    pub fn rehydrate(&self, context: &OperationContext) -> Result<usize, SystemError> {
        self.require_background(context)?;
        let _op = lock(&self.inner.operation);
        self.require_background(context)?;
        let (count, events) = self.rehydrate_locked()?;
        self.emit(events);
        Ok(count)
    }

    fn require_background(&self, context: &OperationContext) -> Result<(), SystemError> {
        ExecutionOwner::from_context(context).require_background(context)
    }

    fn rehydrate_locked(&self) -> Result<(usize, Vec<NotificationScheduleEvent>), SystemError> {
        let Some(store) = &self.inner.store else {
            return Ok((self.schedule_count(), Vec::new()));
        };
        let loaded = store.load_all()?;
        let max_epoch = loaded
            .records
            .iter()
            .map(|record| record.schedule_epoch)
            .max()
            .unwrap_or(0);
        self.inner
            .next_epoch
            .fetch_max(max_epoch.saturating_add(1), Ordering::SeqCst);
        let count = loaded.records.len();
        {
            let mut state = lock(&self.inner.state);
            state.records = loaded
                .records
                .into_iter()
                .map(|record| (record.schedule_id.clone(), record))
                .collect();
        }
        let events = loaded
            .quarantined
            .iter()
            .map(
                |quarantined| NotificationScheduleEvent::CorruptRecordQuarantined {
                    record_kind: quarantined.kind.as_str(),
                    bytes: quarantined.bytes,
                },
            )
            .collect();
        Ok((count, events))
    }

    fn emit(&self, events: Vec<NotificationScheduleEvent>) {
        if events.is_empty() {
            return;
        }
        let senders = {
            let mut state = lock(&self.inner.state);
            state
                .subscribers
                .retain(|(live, _)| live.load(Ordering::Acquire));
            state
                .subscribers
                .iter()
                .map(|(_, sender)| sender.clone())
                .collect::<Vec<_>>()
        };
        for event in events {
            for sender in &senders {
                sender.send(event.clone());
            }
        }
    }

    fn allocate_epoch(&self) -> u64 {
        let epoch = self.inner.next_epoch.fetch_add(1, Ordering::SeqCst);
        epoch.max(1)
    }

    fn resolve_zone(
        &self,
        zone: &crate::timezone::TimeZoneSpec,
    ) -> Result<ResolvedTimeZone, SystemError> {
        self.inner.resolver.resolve(zone)
    }

    fn permission_error() -> SystemError {
        let mut error = SystemError::new(
            ErrorCode::PermissionDenied,
            "Notification permission is not authorized",
        );
        error.recovery = Some(RecoveryAction::OpenSettings);
        error.retry = RetryAdvice::AfterConditionsChange;
        error
    }

    fn not_found() -> SystemError {
        SystemError::new(
            ErrorCode::InvalidInput,
            "Notification schedule identity is not registered",
        )
    }

    fn capability(&self, operation: ScheduleOperation) -> Result<(), SystemError> {
        self.inner.provider.capability(operation).check()
    }

    /// Submit or replace one schedule after all capability, permission and
    /// attachment checks. The durable intent is committed before the provider
    /// call; the accepted `Scheduled` record is committed before memory and
    /// events are updated.
    pub fn submit(
        &self,
        context: &OperationContext,
        submission: ScheduleSubmission,
    ) -> Result<ScheduleSubmissionReceipt, SystemError> {
        self.require_background(context)?;
        submission.validate()?;
        let _op = lock(&self.inner.operation);
        self.require_background(context)?;

        if !self
            .inner
            .permissions
            .status(PermissionKind::Notifications)
            .allows_use()
        {
            return Err(Self::permission_error());
        }
        self.capability(ScheduleOperation::ScheduleNotification)?;
        if submission.trigger.requires_repeat_capability() {
            self.capability(ScheduleOperation::RepeatAndCalendarSchedule)?;
        }
        if !submission.attachments.is_empty() {
            self.capability(ScheduleOperation::AttachMediaToSchedule)?;
        }

        let now_wall = self.inner.clock.wall_clock_epoch_ms();
        let now_monotonic = self.inner.clock.monotonic_now_ms();
        let mut precision = submission.precision;
        let mut downgrade = None;
        if precision == SchedulePrecision::ExactAlarm {
            let capability = self
                .inner
                .provider
                .capability(ScheduleOperation::ScheduleExactAlarm);
            let capability_result = capability.check();
            let exact_permission = self.inner.permissions.status(exact_alarm_permission_kind());
            let reason = match &capability_result {
                Err(error) => Some(exact_alarm_downgrade_reason(error)),
                Ok(()) if !exact_permission.allows_use() => {
                    Some(InexactDowngradeReason::ExactAlarmPermissionDenied)
                }
                Ok(()) => None,
            };
            if let Some(reason) = reason {
                match submission.exact_alarm_fallback {
                    ExactAlarmFallback::Reject => {
                        capability_result?;
                        return Err(Self::permission_error());
                    }
                    ExactAlarmFallback::AllowInexact => {
                        precision = SchedulePrecision::InexactAdvisory;
                        downgrade = Some(InexactDowngrade {
                            reason,
                            recorded_at_epoch_ms: now_wall,
                        });
                    }
                }
            }
        }

        let zone_spec = if submission.follows_active_time_zone {
            self.inner.clock.active_time_zone()
        } else {
            submission.time_zone.clone()
        };
        let zone = self.resolve_zone(&zone_spec)?;
        let anchor = submission.trigger.first_fire_at(
            now_wall,
            &zone,
            submission.dst_gap_policy,
            submission.dst_fold_policy,
        )?;
        let Some(next_fire) = anchor else {
            return Err(SystemError::new(
                ErrorCode::InvalidInput,
                "Calendar rule has no future occurrence",
            ));
        };
        if matches!(submission.trigger, ScheduleTrigger::OneShotAt { .. }) && next_fire <= now_wall
        {
            return Err(SystemError::new(
                ErrorCode::InvalidInput,
                "One-shot fire instant must be in the future",
            ));
        }

        let existing = self.schedule(&submission.schedule_id);
        let disposition = match (&existing, submission.conflict_policy) {
            (Some(record), ScheduleConflictPolicy::KeepExisting) if record.is_active() => {
                return Ok(Self::receipt_from_record(
                    record,
                    SubmissionDisposition::KeptExisting,
                ));
            }
            (Some(record), ScheduleConflictPolicy::RejectConflict) if record.is_active() => {
                let _ = record;
                return Err(conflict_error());
            }
            (Some(_), ScheduleConflictPolicy::Replace) => SubmissionDisposition::Replaced,
            _ if downgrade.is_some() => SubmissionDisposition::DowngradedToInexact,
            _ => SubmissionDisposition::Created,
        };

        let retention = self.retention_policy();
        let active_count = lock(&self.inner.state)
            .records
            .values()
            .filter(|record| record.is_active() && record.schedule_id != submission.schedule_id)
            .count();
        if active_count >= retention.max_active_schedules {
            return Err(capacity_error());
        }

        let schedule_epoch = self.allocate_epoch();
        let record = NotificationScheduleRecord {
            schedule_id: submission.schedule_id.clone(),
            schedule_epoch,
            status: NotificationScheduleStatus::Submitted,
            trigger: submission.trigger.clone(),
            time_zone: zone_spec,
            follows_active_time_zone: submission.follows_active_time_zone,
            dst_gap_policy: submission.dst_gap_policy,
            dst_fold_policy: submission.dst_fold_policy,
            missed_fire_policy: submission.missed_fire_policy,
            precision,
            exact_alarm_fallback: submission.exact_alarm_fallback,
            downgrade,
            content: submission.content.clone(),
            attachments: submission.attachments.clone(),
            conflict_policy: submission.conflict_policy,
            submitted_at_epoch_ms: now_wall,
            submitted_monotonic_ms: now_monotonic,
            updated_at_epoch_ms: now_wall,
            anchor_fire_at_epoch_ms: Some(next_fire),
            next_fire_at_epoch_ms: Some(next_fire),
            catch_up_at_epoch_ms: None,
            provider_fire_at_epoch_ms: Some(next_fire),
            provider_token: None,
            presented_count: 0,
            last_presented_at_epoch_ms: None,
            activation_count: 0,
            last_activated_at_epoch_ms: None,
            delivery_observations: Vec::new(),
            activation_handoffs: Vec::new(),
            suspension: None,
            terminal_at_epoch_ms: None,
        };

        if let Some(store) = &self.inner.store
            && let Err(error) = store.save_record(&record)
        {
            return Err(error);
        }

        let request = Self::request_from_record(&record, next_fire)?;
        let provider_receipt = match self.inner.provider.schedule(&request) {
            Ok(receipt) => receipt,
            Err(error) => {
                self.rollback_intent(&record, existing.as_ref(), false);
                return Err(error);
            }
        };
        if let Err(error) = provider_receipt.validate_for(&request) {
            let _ = self
                .inner
                .provider
                .cancel(&record.schedule_id, schedule_epoch);
            self.rollback_intent(&record, existing.as_ref(), true);
            return Err(error);
        }

        let mut scheduled = record;
        scheduled.status = NotificationScheduleStatus::Scheduled;
        scheduled.provider_token = Some(provider_receipt.provider_token);
        if let Some(store) = &self.inner.store
            && let Err(error) = store.save_record(&scheduled)
        {
            let _ = self
                .inner
                .provider
                .cancel(&scheduled.schedule_id, schedule_epoch);
            self.rollback_intent(&scheduled, existing.as_ref(), true);
            return Err(error);
        }

        let mut events = vec![NotificationScheduleEvent::Submitted {
            schedule_id: scheduled.schedule_id.clone(),
            schedule_epoch,
            precision,
            next_fire_at_epoch_ms: scheduled.next_fire_at_epoch_ms,
        }];
        if let Some(downgrade) = scheduled.downgrade {
            events.push(NotificationScheduleEvent::DowngradedToInexact {
                schedule_id: scheduled.schedule_id.clone(),
                schedule_epoch,
                reason: downgrade.reason,
            });
        }
        lock(&self.inner.state)
            .records
            .insert(scheduled.schedule_id.clone(), scheduled.clone());
        self.emit(events);
        Ok(Self::receipt_from_record(&scheduled, disposition))
    }

    fn receipt_from_record(
        record: &NotificationScheduleRecord,
        disposition: SubmissionDisposition,
    ) -> ScheduleSubmissionReceipt {
        ScheduleSubmissionReceipt {
            schedule_id: record.schedule_id.clone(),
            schedule_epoch: record.schedule_epoch,
            disposition,
            status: record.status,
            precision: record.precision,
            downgrade: record.downgrade,
            next_fire_at_epoch_ms: record.next_fire_at_epoch_ms,
            provider_token: record.provider_token,
            submitted_at_epoch_ms: record.submitted_at_epoch_ms,
        }
    }

    fn request_from_record(
        record: &NotificationScheduleRecord,
        fire_at_epoch_ms: u64,
    ) -> Result<ScheduledNotificationRequest, SystemError> {
        record.validate()?;
        Ok(ScheduledNotificationRequest {
            schedule_id: record.schedule_id.clone(),
            schedule_epoch: record.schedule_epoch,
            fire_at_epoch_ms,
            precision: record.precision,
            trigger: record.trigger.clone(),
            time_zone: record.time_zone.clone(),
            content: record.content.clone(),
            attachments: record.attachments.clone(),
        })
    }

    /// Restore the durable journal after a failed submission or replacement.
    ///
    /// When the provider registration for the previous record was already
    /// withdrawn (because the replacement overwrote it), the previous record is
    /// re-submitted so the journal and provider cannot silently diverge. If that
    /// re-submission fails, the previous record is persisted as `Submitted`
    /// without a provider token so startup reconciliation repairs it.
    fn rollback_intent(
        &self,
        submitted: &NotificationScheduleRecord,
        existing: Option<&NotificationScheduleRecord>,
        restore_provider: bool,
    ) {
        if let Some(store) = &self.inner.store {
            let _ = store.delete_record(&submitted.schedule_id);
        }
        let Some(existing) = existing else {
            return;
        };
        let mut restored = existing.clone();
        if restore_provider && existing.status == NotificationScheduleStatus::Scheduled {
            let resubmitted = existing
                .expected_provider_fire_at()
                .and_then(|fire| Self::request_from_record(existing, fire).ok())
                .and_then(|request| {
                    self.inner
                        .provider
                        .schedule(&request)
                        .ok()
                        .filter(|receipt| receipt.validate_for(&request).is_ok())
                });
            match resubmitted {
                Some(receipt) => {
                    restored.status = NotificationScheduleStatus::Scheduled;
                    restored.provider_token = Some(receipt.provider_token);
                }
                None => {
                    restored.status = NotificationScheduleStatus::Submitted;
                    restored.provider_token = None;
                }
            }
        }
        if let Some(store) = &self.inner.store {
            let _ = store.save_record(&restored);
        }
        lock(&self.inner.state)
            .records
            .insert(restored.schedule_id.clone(), restored);
    }

    /// Record observed presentation only. It advances the next occurrence or
    /// completes a one-shot, and never records a user activation.
    pub fn record_delivery_observation(
        &self,
        context: &OperationContext,
        observation: DeliveryObservation,
    ) -> Result<DeliveryObservationReceipt, SystemError> {
        self.require_background(context)?;
        observation.validate()?;
        self.capability(ScheduleOperation::ObserveScheduledDelivery)?;
        let _op = lock(&self.inner.operation);
        self.require_background(context)?;

        let current = self
            .schedule(&observation.schedule_id)
            .ok_or_else(Self::not_found)?;
        self.fence_delivery(&current, &observation)?;

        let now = self.inner.clock.wall_clock_epoch_ms();
        let mut next = current.clone();
        next.presented_count = next.presented_count.saturating_add(1);
        next.last_presented_at_epoch_ms = Some(observation.presented_at_epoch_ms);
        next.delivery_observations.push(DeliveryObservationSummary {
            presented_at_epoch_ms: observation.presented_at_epoch_ms,
            provider_token: observation.provider_token,
        });
        if next.delivery_observations.len() > crate::model::MAX_DELIVERY_OBSERVATIONS {
            let excess = next.delivery_observations.len() - crate::model::MAX_DELIVERY_OBSERVATIONS;
            next.delivery_observations.drain(..excess);
        }
        next.updated_at_epoch_ms = now.max(next.updated_at_epoch_ms);
        next.catch_up_at_epoch_ms = None;

        let terminal = if next.trigger.is_one_shot() {
            next.status = NotificationScheduleStatus::Delivered;
            next.terminal_at_epoch_ms = Some(now);
            next.next_fire_at_epoch_ms = None;
            next.provider_fire_at_epoch_ms = None;
            true
        } else if next
            .max_occurrences()
            .is_some_and(|max| next.presented_count >= max)
        {
            next.status = NotificationScheduleStatus::Completed;
            next.terminal_at_epoch_ms = Some(now);
            next.next_fire_at_epoch_ms = None;
            next.provider_fire_at_epoch_ms = None;
            true
        } else {
            let zone = self.resolve_zone(&next.time_zone)?;
            let base = next
                .next_fire_at_epoch_ms
                .unwrap_or(observation.presented_at_epoch_ms)
                .max(observation.presented_at_epoch_ms);
            match next.trigger.next_fire_after(
                next.anchor_fire_at_epoch_ms,
                base,
                &zone,
                next.dst_gap_policy,
                next.dst_fold_policy,
            )? {
                Some(fire) => {
                    next.next_fire_at_epoch_ms = Some(fire);
                    next.provider_fire_at_epoch_ms = Some(fire);
                    // The durable journal must not claim provider acceptance
                    // until the follow-up registration succeeds. A `Submitted`
                    // record is repaired by startup reconciliation.
                    next.status = NotificationScheduleStatus::Submitted;
                    next.provider_token = None;
                    false
                }
                None => {
                    next.status = NotificationScheduleStatus::Completed;
                    next.terminal_at_epoch_ms = Some(now);
                    next.next_fire_at_epoch_ms = None;
                    next.provider_fire_at_epoch_ms = None;
                    true
                }
            }
        };

        if let Some(store) = &self.inner.store {
            store.save_record(&next)?;
        }
        lock(&self.inner.state)
            .records
            .insert(next.schedule_id.clone(), next.clone());

        if terminal {
            let _ = self
                .inner
                .provider
                .cancel(&next.schedule_id, next.schedule_epoch);
        }
        let mut provider_rescheduled = false;
        if !terminal
            && let Some(fire) = next.expected_provider_fire_at()
            && let Ok(request) = Self::request_from_record(&next, fire)
            && let Ok(receipt) = self.inner.provider.schedule(&request)
            && receipt.validate_for(&request).is_ok()
        {
            next.status = NotificationScheduleStatus::Scheduled;
            next.provider_token = Some(receipt.provider_token);
            if let Some(store) = &self.inner.store {
                store.save_record(&next)?;
            }
            lock(&self.inner.state)
                .records
                .insert(next.schedule_id.clone(), next.clone());
            provider_rescheduled = true;
        }

        self.emit(vec![NotificationScheduleEvent::Presented {
            schedule_id: next.schedule_id.clone(),
            schedule_epoch: next.schedule_epoch,
            presented_at_epoch_ms: observation.presented_at_epoch_ms,
            presented_count: next.presented_count,
            next_fire_at_epoch_ms: next.next_fire_at_epoch_ms,
            terminal,
        }]);
        Ok(DeliveryObservationReceipt {
            schedule_id: next.schedule_id,
            schedule_epoch: next.schedule_epoch,
            presented_at_epoch_ms: observation.presented_at_epoch_ms,
            presented_count: next.presented_count,
            next_fire_at_epoch_ms: next.next_fire_at_epoch_ms,
            status: next.status,
            provider_rescheduled,
        })
    }

    fn fence_delivery(
        &self,
        current: &NotificationScheduleRecord,
        observation: &DeliveryObservation,
    ) -> Result<(), SystemError> {
        if current.schedule_epoch != observation.schedule_epoch {
            return Err(stale_epoch_error());
        }
        if !matches!(
            current.status,
            NotificationScheduleStatus::Submitted | NotificationScheduleStatus::Scheduled
        ) {
            return Err(stale_epoch_error());
        }
        Ok(())
    }

    /// Record a user tap or action invocation only. Presentation fields are
    /// never fabricated, even when the platform delivered a tap without a
    /// prior delivery observation.
    pub fn record_activation(
        &self,
        context: &OperationContext,
        observation: ActivationObservation,
    ) -> Result<ScheduledActivationHandoff, SystemError> {
        self.require_background(context)?;
        observation.validate()?;
        let _op = lock(&self.inner.operation);
        self.require_background(context)?;

        let current = self
            .schedule(&observation.schedule_id)
            .ok_or_else(Self::not_found)?;
        if current.schedule_epoch != observation.schedule_epoch
            || matches!(
                current.status,
                NotificationScheduleStatus::Cancelled | NotificationScheduleStatus::Expired
            )
        {
            return Err(stale_epoch_error());
        }
        let declared: Vec<&ActionId> = current
            .content
            .actions()
            .iter()
            .map(|action| action.id())
            .collect();
        match &observation.action_id {
            Some(action_id) => {
                if !declared.contains(&action_id) {
                    return Err(SystemError::new(
                        ErrorCode::InvalidInput,
                        "Activation references an undeclared notification action",
                    ));
                }
            }
            None => {
                if observation.text_input.is_some() {
                    return Err(SystemError::new(
                        ErrorCode::InvalidInput,
                        "Text input requires a declared text-input action",
                    ));
                }
            }
        }
        if observation.text_input.is_some() {
            let reserved = ActionId::new(crate::model::SCHEDULE_TEXT_INPUT_ACTION_ID)?;
            if observation.action_id.as_ref() != Some(&reserved) || !declared.contains(&&reserved) {
                return Err(SystemError::new(
                    ErrorCode::InvalidInput,
                    "Text input requires the reserved schedule-text-input action",
                ));
            }
        }

        let now = self.inner.clock.wall_clock_epoch_ms();
        let notification_activation_id = current.schedule_id.activation_notification_id()?;
        let storage = current.schedule_id.storage_id();
        let activation_event_id = ActivationId::new(format!(
            "schedule-event-{}-{}",
            &storage[..16],
            current.activation_count.saturating_add(1)
        ))?;
        let target = current
            .content
            .activation()
            .and_then(|binding| binding.target_for(observation.action_id.as_ref()).cloned());
        let payload = target
            .as_ref()
            .map(|target| ActivationPayload::Notification {
                notification_id: notification_activation_id.clone(),
                target: target.clone(),
            });
        let handoff = ScheduledActivationHandoff {
            schedule_id: current.schedule_id.clone(),
            schedule_epoch: current.schedule_epoch,
            notification_activation_id,
            activation_event_id,
            source: ActivationSource::Notification,
            action_id: observation.action_id.clone(),
            text_input: observation.text_input.clone(),
            target,
            payload,
            received_at_epoch_ms: observation.activated_at_epoch_ms,
        };

        let mut next = current;
        next.activation_count = next.activation_count.saturating_add(1);
        next.last_activated_at_epoch_ms = Some(observation.activated_at_epoch_ms);
        next.activation_handoffs.push(handoff.clone());
        if next.activation_handoffs.len() > crate::model::MAX_ACTIVATION_HANDOFFS {
            let excess = next.activation_handoffs.len() - crate::model::MAX_ACTIVATION_HANDOFFS;
            next.activation_handoffs.drain(..excess);
        }
        next.updated_at_epoch_ms = now.max(next.updated_at_epoch_ms);
        if let Some(store) = &self.inner.store {
            store.save_record(&next)?;
        }
        lock(&self.inner.state)
            .records
            .insert(next.schedule_id.clone(), next);
        self.emit(vec![NotificationScheduleEvent::Activated {
            schedule_id: handoff.schedule_id.clone(),
            schedule_epoch: handoff.schedule_epoch,
            action_id: handoff.action_id.clone(),
            activated_at_epoch_ms: handoff.received_at_epoch_ms,
        }]);
        Ok(handoff)
    }

    /// Cancel one schedule. The durable journal is updated before memory; a
    /// provider cancellation failure is reported but cannot resurrect a
    /// cancelled record.
    pub fn cancel(
        &self,
        context: &OperationContext,
        schedule_id: &ScheduledNotificationId,
    ) -> Result<CancellationReceipt, SystemError> {
        self.require_background(context)?;
        let _op = lock(&self.inner.operation);
        self.require_background(context)?;
        let current = self.schedule(schedule_id).ok_or_else(Self::not_found)?;
        if current.status.is_terminal() {
            return Ok(CancellationReceipt {
                schedule_epoch: current.schedule_epoch,
                provider_cancelled: false,
                already_terminal: true,
            });
        }
        let previous_epoch = current.schedule_epoch;
        let now = self.inner.clock.wall_clock_epoch_ms();
        let mut next = current;
        next.schedule_epoch = self.allocate_epoch();
        next.status = NotificationScheduleStatus::Cancelled;
        next.terminal_at_epoch_ms = Some(now);
        next.updated_at_epoch_ms = now.max(next.updated_at_epoch_ms);
        next.next_fire_at_epoch_ms = None;
        next.catch_up_at_epoch_ms = None;
        next.provider_fire_at_epoch_ms = None;
        next.suspension = None;
        if let Some(store) = &self.inner.store {
            store.save_record(&next)?;
        }
        lock(&self.inner.state)
            .records
            .insert(next.schedule_id.clone(), next.clone());
        let provider_cancelled = self
            .inner
            .provider
            .cancel(schedule_id, previous_epoch)
            .is_ok();
        self.emit(vec![NotificationScheduleEvent::Cancelled {
            schedule_id: next.schedule_id,
            schedule_epoch: next.schedule_epoch,
            provider_cancelled,
        }]);
        Ok(CancellationReceipt {
            schedule_epoch: next.schedule_epoch,
            provider_cancelled,
            already_terminal: false,
        })
    }

    /// Restart reconciliation: rehydrate, compare the journal with provider
    /// pending schedules, re-submit missing/out-of-sync records and cancel
    /// orphaned or stale-epoch OS registrations.
    pub fn reconcile_startup(
        &self,
        context: &OperationContext,
    ) -> Result<ReconciliationReport, SystemError> {
        self.require_background(context)?;
        self.capability(ScheduleOperation::ReconcilePendingSchedules)?;
        let _op = lock(&self.inner.operation);
        self.require_background(context)?;

        let (journal_records, mut events) = match self.rehydrate_locked() {
            Ok((count, events)) => (count, events),
            Err(error) => return Err(error),
        };
        let quarantined = events.len();
        let pending = self.inner.provider.pending_schedules()?;
        let mut pending: BTreeMap<ScheduledNotificationId, PendingProviderSchedule> = pending
            .into_iter()
            .map(|entry| (entry.schedule_id.clone(), entry))
            .collect();
        let records = self.schedules();
        let mut report = ReconciliationReport {
            journal_records,
            quarantined,
            ..ReconciliationReport::default()
        };
        for record in records {
            if !record.is_active() {
                continue;
            }
            report.active_records += 1;
            if record.status == NotificationScheduleStatus::SuspendedByPolicy {
                report.suspended += 1;
                if let Some(orphan) = pending.remove(&record.schedule_id) {
                    let _ = self
                        .inner
                        .provider
                        .cancel(&orphan.schedule_id, orphan.schedule_epoch);
                    report.cancelled_orphans += 1;
                }
                continue;
            }
            match pending.remove(&record.schedule_id) {
                Some(pending_record) if pending_is_in_sync(&record, &pending_record) => {
                    report.in_sync += 1;
                }
                Some(pending_record) => {
                    let _ = self
                        .inner
                        .provider
                        .cancel(&pending_record.schedule_id, pending_record.schedule_epoch);
                    report.cancelled_orphans += 1;
                    self.resubmit_record(&record, &mut report, &mut events)?;
                }
                None => {
                    self.resubmit_record(&record, &mut report, &mut events)?;
                }
            }
        }
        for orphan in pending.into_values() {
            let _ = self
                .inner
                .provider
                .cancel(&orphan.schedule_id, orphan.schedule_epoch);
            report.cancelled_orphans += 1;
        }
        self.emit(events);
        Ok(report)
    }

    /// Compatibility alias for [`Self::reconcile_startup`].
    pub fn reconcile(
        &self,
        context: &OperationContext,
    ) -> Result<ReconciliationReport, SystemError> {
        self.reconcile_startup(context)
    }

    fn resubmit_record(
        &self,
        record: &NotificationScheduleRecord,
        report: &mut ReconciliationReport,
        events: &mut Vec<NotificationScheduleEvent>,
    ) -> Result<(), SystemError> {
        let Some(fire) = record.expected_provider_fire_at() else {
            return Ok(());
        };
        let request = Self::request_from_record(record, fire)?;
        let receipt = match self.inner.provider.schedule(&request) {
            Ok(receipt) => receipt,
            Err(_) => {
                report.resubmit_failures += 1;
                return Ok(());
            }
        };
        if receipt.validate_for(&request).is_err() {
            report.resubmit_failures += 1;
            let _ = self
                .inner
                .provider
                .cancel(&record.schedule_id, record.schedule_epoch);
            return Ok(());
        }
        let now = self.inner.clock.wall_clock_epoch_ms();
        let mut next = record.clone();
        next.status = NotificationScheduleStatus::Scheduled;
        next.provider_token = Some(receipt.provider_token);
        next.provider_fire_at_epoch_ms = Some(fire);
        next.updated_at_epoch_ms = now.max(next.updated_at_epoch_ms);
        if let Some(store) = &self.inner.store {
            store.save_record(&next)?;
        }
        lock(&self.inner.state)
            .records
            .insert(next.schedule_id.clone(), next.clone());
        report.resubmitted += 1;
        events.push(NotificationScheduleEvent::Scheduled {
            schedule_id: next.schedule_id.clone(),
            schedule_epoch: next.schedule_epoch,
            precision: next.precision,
            next_fire_at_epoch_ms: next.next_fire_at_epoch_ms,
            resubmitted: true,
        });
        Ok(())
    }

    /// Recompute calendar schedules and coalesce missed occurrences after a
    /// clock jump, offline sleep, timezone change or DST-rule update.
    pub fn reconcile_clock_or_timezone_change(
        &self,
        context: &OperationContext,
    ) -> Result<ClockReconciliationReport, SystemError> {
        self.require_background(context)?;
        self.capability(ScheduleOperation::ReconcilePendingSchedules)?;
        let _op = lock(&self.inner.operation);
        self.require_background(context)?;

        let now = self.inner.clock.wall_clock_epoch_ms();
        let active_zone = self.inner.clock.active_time_zone();
        let records = self.schedules();
        let mut report = ClockReconciliationReport::default();
        let mut events = Vec::new();
        for record in records {
            if !record.is_active() || record.status == NotificationScheduleStatus::SuspendedByPolicy
            {
                continue;
            }
            report.scanned += 1;
            let zone_changed = record.follows_active_time_zone
                && record.trigger.is_calendar()
                && record.time_zone != active_zone;
            let zone_spec = if record.follows_active_time_zone && record.trigger.is_calendar() {
                active_zone.clone()
            } else {
                record.time_zone.clone()
            };
            let zone = self.resolve_zone(&zone_spec)?;
            let due = record.next_fire_at_epoch_ms.ok_or_else(Self::not_found)?;
            let is_due = due <= now;
            // Calendar triggers depend on the civil clock, so recompute them on
            // every reconciliation. This picks up DST-rule updates and backward
            // clock shifts even when the previous fire is still in the future.
            let recompute_calendar = record.trigger.is_calendar();
            if !zone_changed && !is_due && !recompute_calendar {
                continue;
            }
            if zone_changed {
                report.timezone_updates += 1;
            }
            if is_due {
                report.clock_jumps += 1;
            }

            let mut next = record.clone();
            next.time_zone = zone_spec;
            next.updated_at_epoch_ms = now.max(next.updated_at_epoch_ms);
            let mut terminal: Option<NotificationScheduleStatus> = None;
            let mut catch_up = false;
            let mut missed = 0u32;

            if is_due {
                missed = count_missed_occurrences(
                    &record.trigger,
                    record.anchor_fire_at_epoch_ms,
                    due,
                    now,
                    &zone,
                    record.dst_gap_policy,
                    record.dst_fold_policy,
                )?;
                let allow_catch_up = match record.missed_fire_policy {
                    MissedFirePolicy::FireOnceLatest => true,
                    MissedFirePolicy::SkipToNext => false,
                    MissedFirePolicy::ExpireAfterGrace { grace_ms } => {
                        now.saturating_sub(due) <= grace_ms
                    }
                };
                let next_after = record.trigger.next_fire_after(
                    record.anchor_fire_at_epoch_ms,
                    now,
                    &zone,
                    record.dst_gap_policy,
                    record.dst_fold_policy,
                )?;
                if record.trigger.is_one_shot() {
                    if allow_catch_up {
                        catch_up = true;
                        next.next_fire_at_epoch_ms = Some(now);
                        next.catch_up_at_epoch_ms = None;
                    } else {
                        terminal = Some(NotificationScheduleStatus::Expired);
                    }
                } else if allow_catch_up {
                    if let Some(fire) = next_after {
                        catch_up = true;
                        next.next_fire_at_epoch_ms = Some(fire);
                        next.catch_up_at_epoch_ms = Some(now);
                    } else {
                        terminal = Some(NotificationScheduleStatus::Expired);
                    }
                } else {
                    match next_after {
                        Some(fire) => {
                            next.next_fire_at_epoch_ms = Some(fire);
                            next.catch_up_at_epoch_ms = None;
                        }
                        None => terminal = Some(NotificationScheduleStatus::Expired),
                    }
                }
            } else {
                // Zone/DST update without a missed fire: recompute the next
                // civil occurrence from now.
                match record.trigger.next_fire_after(
                    record.anchor_fire_at_epoch_ms,
                    now,
                    &zone,
                    record.dst_gap_policy,
                    record.dst_fold_policy,
                )? {
                    Some(fire) => {
                        next.next_fire_at_epoch_ms = Some(fire);
                        next.catch_up_at_epoch_ms = None;
                    }
                    None => terminal = Some(NotificationScheduleStatus::Expired),
                }
            }

            if let Some(status) = terminal {
                next.status = status;
                next.terminal_at_epoch_ms = Some(now);
                next.next_fire_at_epoch_ms = None;
                next.catch_up_at_epoch_ms = None;
                next.provider_fire_at_epoch_ms = None;
                if let Some(store) = &self.inner.store {
                    store.save_record(&next)?;
                }
                lock(&self.inner.state)
                    .records
                    .insert(next.schedule_id.clone(), next.clone());
                let _ = self
                    .inner
                    .provider
                    .cancel(&next.schedule_id, record.schedule_epoch);
                report.expired += 1;
                events.push(notification_terminal_event(&next, missed));
                continue;
            }

            let Some(fire) = next.expected_provider_fire_at() else {
                continue;
            };
            if Some(fire) == record.provider_fire_at_epoch_ms && !zone_changed {
                continue;
            }
            next.provider_fire_at_epoch_ms = Some(fire);
            let request = Self::request_from_record(&next, fire)?;
            match self.inner.provider.schedule(&request) {
                Ok(receipt) if receipt.validate_for(&request).is_ok() => {
                    next.provider_token = Some(receipt.provider_token);
                    if let Some(store) = &self.inner.store {
                        store.save_record(&next)?;
                    }
                    lock(&self.inner.state)
                        .records
                        .insert(next.schedule_id.clone(), next.clone());
                    if catch_up {
                        report.catch_up_resubmissions += 1;
                    } else {
                        report.advanced += 1;
                    }
                    events.push(NotificationScheduleEvent::Rescheduled {
                        schedule_id: next.schedule_id,
                        schedule_epoch: next.schedule_epoch,
                        next_fire_at_epoch_ms: fire,
                        catch_up,
                    });
                }
                _ => report.provider_failures += 1,
            }
        }
        self.emit(events);
        Ok(report)
    }

    /// Suspend schedules when notifications or their channel are unavailable,
    /// downgrade exact alarms when only the exact-alarm permission was revoked,
    /// and re-admit schedules when policy is restored.
    pub fn reconcile_permissions_and_channels(
        &self,
        context: &OperationContext,
    ) -> Result<PermissionReconciliationReport, SystemError> {
        self.require_background(context)?;
        self.capability(ScheduleOperation::ReconcilePendingSchedules)?;
        let _op = lock(&self.inner.operation);
        self.require_background(context)?;

        let notifications = self.inner.permissions.status(PermissionKind::Notifications);
        let exact_alarm = self.inner.permissions.status(exact_alarm_permission_kind());
        let exact_alarm_failure = self.exact_alarm_failure(exact_alarm);
        let records = self.schedules();
        let mut report = PermissionReconciliationReport::default();
        let mut events = Vec::new();
        for record in records {
            if !record.is_active() {
                continue;
            }
            report.scanned += 1;
            let channel_id = record.content.channel_id().map(str::to_owned);
            let channel_blocked = match (&channel_id, notifications.allows_use()) {
                (_, false) => Some(SuspensionReason::NotificationsPermissionDenied),
                (Some(channel), true) => match self.inner.provider.channel_status(channel)? {
                    ChannelAvailability::Disabled | ChannelAvailability::Muted => {
                        Some(SuspensionReason::ChannelDisabled {
                            channel_id: channel.clone(),
                        })
                    }
                    ChannelAvailability::Enabled | ChannelAvailability::Unspecified => None,
                },
                (None, true) => None,
            };

            if let Some(reason) = channel_blocked {
                if record.status != NotificationScheduleStatus::SuspendedByPolicy {
                    let next = self.suspend_record(&record, reason.clone())?;
                    report.suspended += 1;
                    events.push(NotificationScheduleEvent::Suspended {
                        schedule_id: next.schedule_id,
                        schedule_epoch: next.schedule_epoch,
                        reason,
                    });
                }
                continue;
            }

            if record.precision == SchedulePrecision::ExactAlarm
                && let Some((reason, suspension_reason)) = exact_alarm_failure.clone()
            {
                match record.exact_alarm_fallback {
                    ExactAlarmFallback::AllowInexact => {
                        if let Some(fire) = record
                            .expected_provider_fire_at()
                            .or(record.next_fire_at_epoch_ms)
                        {
                            self.downgrade_to_inexact(
                                &record,
                                reason,
                                fire,
                                &mut report,
                                &mut events,
                            )?;
                        }
                    }
                    ExactAlarmFallback::Reject => {
                        if record.status != NotificationScheduleStatus::SuspendedByPolicy
                            || record.suspension.as_ref() != Some(&suspension_reason)
                        {
                            let next = self.suspend_record(&record, suspension_reason.clone())?;
                            report.suspended += 1;
                            events.push(NotificationScheduleEvent::Suspended {
                                schedule_id: next.schedule_id,
                                schedule_epoch: next.schedule_epoch,
                                reason: suspension_reason,
                            });
                        }
                    }
                }
                continue;
            }

            if record.status == NotificationScheduleStatus::SuspendedByPolicy {
                self.resume_suspended(&record, &mut report, &mut events)?;
            }
        }
        self.emit(events);
        Ok(report)
    }

    /// Classify why an exact-alarm record can no longer claim exact precision.
    /// Capability failure is classified with the same helper used by `submit`,
    /// so a provider-side permission error stays distinct from an unsupported or
    /// unavailable provider.
    fn exact_alarm_failure(
        &self,
        exact_alarm: PermissionStatus,
    ) -> Option<(InexactDowngradeReason, SuspensionReason)> {
        match self
            .inner
            .provider
            .capability(ScheduleOperation::ScheduleExactAlarm)
            .check()
        {
            Err(error) => {
                let reason = exact_alarm_downgrade_reason(&error);
                let suspension = match reason {
                    InexactDowngradeReason::ExactAlarmPermissionDenied => {
                        SuspensionReason::ExactAlarmPermissionRevoked
                    }
                    _ => SuspensionReason::ProviderUnavailable,
                };
                Some((reason, suspension))
            }
            Ok(()) if !exact_alarm.allows_use() => Some((
                InexactDowngradeReason::ExactAlarmPermissionDenied,
                SuspensionReason::ExactAlarmPermissionRevoked,
            )),
            Ok(()) => None,
        }
    }

    fn suspend_record(
        &self,
        record: &NotificationScheduleRecord,
        reason: SuspensionReason,
    ) -> Result<NotificationScheduleRecord, SystemError> {
        let mut next = record.clone();
        let fire = next
            .expected_provider_fire_at()
            .or(next.next_fire_at_epoch_ms);
        next.next_fire_at_epoch_ms = fire;
        next.catch_up_at_epoch_ms = None;
        next.provider_fire_at_epoch_ms = None;
        next.status = NotificationScheduleStatus::SuspendedByPolicy;
        next.suspension = Some(reason);
        next.updated_at_epoch_ms = self
            .inner
            .clock
            .wall_clock_epoch_ms()
            .max(next.updated_at_epoch_ms);
        if let Some(store) = &self.inner.store {
            store.save_record(&next)?;
        }
        let _ = self
            .inner
            .provider
            .cancel(&record.schedule_id, record.schedule_epoch);
        lock(&self.inner.state)
            .records
            .insert(next.schedule_id.clone(), next.clone());
        Ok(next)
    }

    fn downgrade_to_inexact(
        &self,
        record: &NotificationScheduleRecord,
        reason: InexactDowngradeReason,
        fire: u64,
        report: &mut PermissionReconciliationReport,
        events: &mut Vec<NotificationScheduleEvent>,
    ) -> Result<(), SystemError> {
        let now = self.inner.clock.wall_clock_epoch_ms();
        let mut next = record.clone();
        next.precision = SchedulePrecision::InexactAdvisory;
        next.downgrade = Some(InexactDowngrade {
            reason,
            recorded_at_epoch_ms: now,
        });
        next.updated_at_epoch_ms = now.max(next.updated_at_epoch_ms);
        if record.status == NotificationScheduleStatus::SuspendedByPolicy {
            next.status = NotificationScheduleStatus::Scheduled;
            next.suspension = None;
            next.catch_up_at_epoch_ms = None;
            next.next_fire_at_epoch_ms = Some(fire);
        }
        next.provider_fire_at_epoch_ms = Some(fire);
        let request = Self::request_from_record(&next, fire)?;
        match self.inner.provider.schedule(&request) {
            Ok(receipt) if receipt.validate_for(&request).is_ok() => {
                next.provider_token = Some(receipt.provider_token);
                if let Some(store) = &self.inner.store {
                    store.save_record(&next)?;
                }
                lock(&self.inner.state)
                    .records
                    .insert(next.schedule_id.clone(), next.clone());
                report.downgraded += 1;
                events.push(NotificationScheduleEvent::DowngradedToInexact {
                    schedule_id: next.schedule_id,
                    schedule_epoch: next.schedule_epoch,
                    reason,
                });
            }
            _ => report.provider_failures += 1,
        }
        Ok(())
    }

    fn resume_suspended(
        &self,
        record: &NotificationScheduleRecord,
        report: &mut PermissionReconciliationReport,
        events: &mut Vec<NotificationScheduleEvent>,
    ) -> Result<(), SystemError> {
        let Some(fire) = record.next_fire_at_epoch_ms else {
            return Ok(());
        };
        if record.precision == SchedulePrecision::ExactAlarm {
            let exact_alarm = self.inner.permissions.status(exact_alarm_permission_kind());
            if let Some((reason, suspension_reason)) = self.exact_alarm_failure(exact_alarm) {
                return match record.exact_alarm_fallback {
                    ExactAlarmFallback::AllowInexact => {
                        self.downgrade_to_inexact(record, reason, fire, report, events)
                    }
                    ExactAlarmFallback::Reject => {
                        if record.suspension.as_ref() != Some(&suspension_reason) {
                            let next = self.suspend_record(record, suspension_reason.clone())?;
                            report.suspended += 1;
                            events.push(NotificationScheduleEvent::Suspended {
                                schedule_id: next.schedule_id,
                                schedule_epoch: next.schedule_epoch,
                                reason: suspension_reason,
                            });
                        }
                        Ok(())
                    }
                };
            }
        }
        let mut next = record.clone();
        next.status = NotificationScheduleStatus::Scheduled;
        next.suspension = None;
        next.catch_up_at_epoch_ms = None;
        next.provider_fire_at_epoch_ms = Some(fire);
        next.updated_at_epoch_ms = self
            .inner
            .clock
            .wall_clock_epoch_ms()
            .max(next.updated_at_epoch_ms);
        let request = Self::request_from_record(&next, fire)?;
        match self.inner.provider.schedule(&request) {
            Ok(receipt) if receipt.validate_for(&request).is_ok() => {
                next.provider_token = Some(receipt.provider_token);
                if let Some(store) = &self.inner.store {
                    store.save_record(&next)?;
                }
                lock(&self.inner.state)
                    .records
                    .insert(next.schedule_id.clone(), next.clone());
                report.resumed += 1;
                events.push(NotificationScheduleEvent::Resumed {
                    schedule_id: next.schedule_id,
                    schedule_epoch: next.schedule_epoch,
                    next_fire_at_epoch_ms: Some(fire),
                });
            }
            _ => report.provider_failures += 1,
        }
        Ok(())
    }

    /// Prune terminal records on disk and in memory using the configured
    /// retention policy.
    pub fn prune(&self, context: &OperationContext) -> Result<RetentionReport, SystemError> {
        let policy = self.retention_policy();
        self.prune_with_policy(context, policy)
    }

    pub fn prune_with_policy(
        &self,
        context: &OperationContext,
        policy: RetentionPolicy,
    ) -> Result<RetentionReport, SystemError> {
        self.require_background(context)?;
        policy.validate()?;
        let _op = lock(&self.inner.operation);
        self.require_background(context)?;
        let now = self.inner.clock.wall_clock_epoch_ms();
        let cutoff = now.saturating_sub(policy.terminal_ttl_ms);
        let mut records = self.schedules();
        records.sort_by_key(|record| record.terminal_at_epoch_ms.unwrap_or(u64::MAX));
        let mut terminal: Vec<NotificationScheduleRecord> = records
            .into_iter()
            .filter(NotificationScheduleRecord::is_terminal)
            .collect();
        let mut remove: Vec<ScheduledNotificationId> = Vec::new();
        let mut kept: Vec<NotificationScheduleRecord> = Vec::new();
        for record in terminal.drain(..) {
            let expired = record
                .terminal_at_epoch_ms
                .is_some_and(|terminal| terminal < cutoff);
            if expired {
                remove.push(record.schedule_id);
            } else {
                kept.push(record);
            }
        }
        while kept.len() > policy.max_terminal_records {
            let oldest = kept.remove(0);
            remove.push(oldest.schedule_id);
        }
        for id in &remove {
            if let Some(store) = &self.inner.store {
                store.delete_record(id)?;
            }
            lock(&self.inner.state).records.remove(id);
        }
        if !remove.is_empty() {
            self.emit(vec![NotificationScheduleEvent::Pruned {
                removed: remove.len(),
            }]);
        }
        Ok(RetentionReport {
            removed: remove.len(),
            remaining_terminal: kept.len(),
        })
    }
}

fn notification_terminal_event(
    record: &NotificationScheduleRecord,
    missed: u32,
) -> NotificationScheduleEvent {
    if record.status == NotificationScheduleStatus::Completed {
        NotificationScheduleEvent::Completed {
            schedule_id: record.schedule_id.clone(),
            schedule_epoch: record.schedule_epoch,
            presented_count: record.presented_count,
        }
    } else {
        NotificationScheduleEvent::Expired {
            schedule_id: record.schedule_id.clone(),
            schedule_epoch: record.schedule_epoch,
            missed_occurrences: missed,
        }
    }
}

/// Ensure a cancellation token that has already fired cannot continue. This is
/// exposed for host adapters that perform their own preflight.
pub fn ensure_not_cancelled(cancellation: &CancellationToken) -> Result<(), SystemError> {
    match cancellation.reason() {
        Some(_) => Err(crate::cancelled_error()),
        None => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn authorization_is_a_live_query() {
        let authorization = ScheduleAuthorization::new(
            PermissionStatus::Denied {
                can_request_again: true,
            },
            PermissionStatus::Granted,
        );
        assert!(
            !authorization
                .status(PermissionKind::Notifications)
                .allows_use()
        );
        authorization.set_notifications(PermissionStatus::Granted);
        assert!(
            authorization
                .status(PermissionKind::Notifications)
                .allows_use()
        );
        assert!(
            authorization
                .status(PermissionKind::SpecialAccess(
                    crate::model::SCHEDULE_EXACT_ALARM_PERMISSION
                ))
                .allows_use()
        );
        assert_eq!(
            authorization.status(PermissionKind::Camera),
            PermissionStatus::Unavailable
        );
    }

    #[test]
    fn retention_policy_bounds_are_validated() {
        assert!(RetentionPolicy::default().validate().is_ok());
        assert!(
            RetentionPolicy {
                max_active_schedules: 0,
                ..RetentionPolicy::default()
            }
            .validate()
            .is_err()
        );
        assert!(
            RetentionPolicy {
                terminal_ttl_ms: 0,
                ..RetentionPolicy::default()
            }
            .validate()
            .is_err()
        );
    }
}
