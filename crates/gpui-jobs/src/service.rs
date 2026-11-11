//! Durable job coordination: registration, constraints, advisory scheduling,
//! fencing, checkpoints, preemption, retry/backoff, cancellation and restart
//! reconciliation.
//!
//! All durable operations require a background [`OperationContext`] without a
//! UI attachment. Listener and preemption callbacks are collected outside the
//! service lock and invoked only after every lock is released.

use crate::model::ExistingJobPolicy;
use crate::store::{JobStore, JobsStorageHook, NoopJobsStorageHook};
use crate::{
    AppPhase, ExecutionOwner, IdempotencyToken, InProcessExecutor, JobCheckpoint, JobConstraints,
    JobFailureReason, JobKey, JobNetworkRequirement, JobPermissionRequirement, JobProgress,
    JobProvider, JobProviderListener, JobProviderNotice, JobRecord, JobScheduleRequest, JobStatus,
    JobsOperation, LifecycleState, MAX_CHECKPOINT_BYTES, PREEMPTION_GRACE_MS,
    RegistrationDisposition, ScheduleReceipt, SchedulingGuarantee, UnmetConstraint,
    stale_fencing_error,
};
use gpui_app_data::AppData;
use gpui_network::{ConnectionCost, NetworkSnapshot};
use gpui_permissions::PermissionStatus;
use gpui_power::{PowerBudgetPolicy, PowerSnapshot, ThermalCondition, snapshot_requires_pause};
use gpui_system::{
    CancelReason, Completion, ErrorCode, EventSender, EventSubscription, OperationContext,
    ServiceResult, SystemError, event_channel,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fmt,
    path::Path,
    sync::{
        Arc, Mutex, Weak,
        atomic::{AtomicBool, Ordering},
    },
};
use tempfile::TempDir;

fn error(code: ErrorCode, message: &'static str) -> SystemError {
    SystemError::new(code, message)
}

fn lock<T>(value: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    value
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn background_cancellation(
    context: &OperationContext,
) -> Result<Option<CancelReason>, SystemError> {
    ExecutionOwner::from_context(context).require_background(context)?;
    Ok(context.cancellation.reason())
}

/// The budget that expired when a job ended in [`JobStatus::ExpiredBudget`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BudgetExpiry {
    ExecutionBudget,
    Deadline,
    AttemptsExhausted,
    ProviderTimeSlice,
}

impl BudgetExpiry {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ExecutionBudget => "execution_budget",
            Self::Deadline => "deadline",
            Self::AttemptsExhausted => "attempts_exhausted",
            Self::ProviderTimeSlice => "provider_time_slice",
        }
    }
}

/// Why a running attempt was asked to stop cooperatively.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PreemptReason {
    ConstraintsLost,
    LifecycleSuspending,
    BudgetExpired,
    PermissionRevoked,
    Cancelled,
    ProviderTimeSliceExpired,
}

impl PreemptReason {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ConstraintsLost => "constraints_lost",
            Self::LifecycleSuspending => "lifecycle_suspending",
            Self::BudgetExpired => "budget_expired",
            Self::PermissionRevoked => "permission_revoked",
            Self::Cancelled => "cancelled",
            Self::ProviderTimeSliceExpired => "provider_time_slice_expired",
        }
    }

    const fn budget_expiry(self) -> Option<BudgetExpiry> {
        match self {
            Self::BudgetExpired => Some(BudgetExpiry::ExecutionBudget),
            Self::ProviderTimeSliceExpired => Some(BudgetExpiry::ProviderTimeSlice),
            _ => None,
        }
    }
}

/// A cooperative preemption handoff. The handler may still write one final
/// checkpoint with the same fencing token, then call
/// [`JobsService::ack_preemption`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreemptionSignal {
    pub token: crate::FencingToken,
    pub reason: PreemptReason,
    pub requested_at_ms: u64,
}

/// Callback trait for cooperative preemption.
pub trait JobPreemptionListener: Send + Sync + 'static {
    fn on_job_preemption(&self, signal: &PreemptionSignal);
}

struct PreemptionRecord {
    id: u64,
    live: Arc<AtomicBool>,
    callback: Arc<dyn Fn(&PreemptionSignal) + Send + Sync + 'static>,
}

/// Retained callback registration. Dropping it unregisters without touching
/// the service lock.
pub struct PreemptionSubscription {
    id: u64,
    live: Arc<AtomicBool>,
    service: Weak<ServiceInner>,
}

impl PreemptionSubscription {
    pub fn id(&self) -> u64 {
        self.id
    }
}

impl Drop for PreemptionSubscription {
    fn drop(&mut self) {
        self.live.store(false, Ordering::Release);
        if let Some(inner) = self.service.upgrade() {
            let mut state = lock(&inner.state);
            state
                .preemption_listeners
                .retain(|listener| listener.id != self.id);
        }
    }
}

/// Public-safe durable and in-process job observations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JobsEvent {
    JobRegistered {
        job_key: JobKey,
        disposition: RegistrationDisposition,
        status: JobStatus,
    },
    JobStateChanged {
        job_key: JobKey,
        previous: JobStatus,
        status: JobStatus,
    },
    JobScheduled {
        job_key: JobKey,
        run_epoch: u64,
        earliest_eligible_after_ms: u64,
    },
    AttemptStarted {
        token: crate::FencingToken,
        started_at_ms: u64,
    },
    ProgressReported {
        job_key: JobKey,
        run_epoch: u64,
        basis_points: u16,
    },
    CheckpointCommitted {
        job_key: JobKey,
        run_epoch: u64,
        sequence: u64,
    },
    CheckpointRecovered {
        job_key: JobKey,
        run_epoch: u64,
        sequence: u64,
    },
    CorruptRecordQuarantined {
        record_kind: &'static str,
        bytes: u64,
    },
    DuplicateWakeupIgnored {
        job_key: JobKey,
        run_epoch: u64,
    },
    PreemptionRequested {
        token: crate::FencingToken,
        reason: PreemptReason,
    },
    PreemptionAcknowledged {
        token: crate::FencingToken,
        reason: PreemptReason,
        status: JobStatus,
    },
    JobSucceeded {
        job_key: JobKey,
        run_epoch: u64,
    },
    JobFailed {
        job_key: JobKey,
        run_epoch: u64,
        reason: JobFailureReason,
    },
    JobCancelled {
        job_key: JobKey,
        run_epoch: u64,
    },
    JobExpiredBudget {
        job_key: JobKey,
        reason: BudgetExpiry,
    },
    RetryScheduled {
        job_key: JobKey,
        run_epoch: u64,
        attempt: u32,
        delay_ms: u64,
    },
    JobReconciled {
        job_key: JobKey,
        previous_status: JobStatus,
        status: JobStatus,
        previous_epoch: u64,
        run_epoch: u64,
    },
    OperationFailed {
        operation: JobsOperation,
        code: ErrorCode,
    },
}

/// Result of a provider wakeup or an explicit attempt dispatch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WakeupOutcome {
    Started(crate::FencingToken),
    DuplicateIgnored { run_epoch: u64 },
    Deferred { unmet: Vec<UnmetConstraint> },
    NotDue { remaining_ms: u64 },
    Terminal { status: JobStatus },
    ExpiredBudget { reason: BudgetExpiry },
}

/// Heartbeat receipt for one live attempt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HeartbeatReceipt {
    pub token: crate::FencingToken,
    pub attempt_deadline_at_ms: Option<u64>,
    pub remaining_ms: u64,
}

/// Summary of a constraint/lifecycle/time advancement.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct JobsUpdate {
    pub changed: bool,
    pub scheduled: u32,
    pub pending: u32,
    pub preempted: u32,
    pub expired: u32,
}

/// Outcome of scanning durable storage after a restart.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ReconciliationReport {
    pub jobs_scanned: usize,
    pub running_reconciled: usize,
    pub checkpoints_recovered: usize,
    pub corrupt_records_quarantined: usize,
    pub expired_budget: usize,
    pub rescheduled: usize,
    pub pending_constraints: usize,
    pub retry_backoff: usize,
}

#[derive(Clone)]
struct ActiveAttempt {
    token: crate::FencingToken,
    preemption: Option<PreemptReason>,
    preemption_requested_at_ms: Option<u64>,
    final_checkpoint_saved: bool,
}

struct ServiceState {
    records: BTreeMap<JobKey, JobRecord>,
    active: BTreeMap<JobKey, ActiveAttempt>,
    checkpoints: BTreeMap<JobKey, JobCheckpoint>,
    power: PowerSnapshot,
    power_policy: PowerBudgetPolicy,
    network: NetworkSnapshot,
    lifecycle: AppPhase,
    lifecycle_revision: u64,
    device_idle: bool,
    storage_low: bool,
    permissions: BTreeMap<String, PermissionStatus>,
    loaded: bool,
    last_reconciliation: Option<ReconciliationReport>,
    subscribers: Vec<(Arc<AtomicBool>, EventSender<JobsEvent>)>,
    preemption_listeners: Vec<PreemptionRecord>,
    next_listener_id: u64,
    event_capacity: usize,
}

impl ServiceState {
    fn new(_now_ms: u64) -> Self {
        Self {
            records: BTreeMap::new(),
            active: BTreeMap::new(),
            checkpoints: BTreeMap::new(),
            power: PowerSnapshot::default(),
            power_policy: PowerBudgetPolicy::default(),
            network: NetworkSnapshot::unknown(gpui_network::SignalSource::Unknown),
            lifecycle: AppPhase::Launching,
            lifecycle_revision: 0,
            device_idle: false,
            storage_low: false,
            permissions: BTreeMap::new(),
            loaded: false,
            last_reconciliation: None,
            subscribers: Vec::new(),
            preemption_listeners: Vec::new(),
            next_listener_id: 1,
            event_capacity: crate::DEFAULT_JOBS_EVENT_CAPACITY,
        }
    }
}

struct ServiceInner {
    state: Mutex<ServiceState>,
    op_lock: Mutex<()>,
    store: JobStore,
    provider: Arc<dyn JobProvider>,
    clock: Arc<dyn crate::JobsClock>,
    in_process: InProcessExecutor,
    _app: AppData,
    _temp: Option<Arc<TempDir>>,
}

impl ServiceInner {
    fn now(&self) -> u64 {
        self.clock.now_ms()
    }

    fn emit(&self, events: Vec<JobsEvent>) {
        if events.is_empty() {
            return;
        }
        let senders = {
            let mut state = lock(&self.state);
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

    fn fire_preemptions(&self, signals: Vec<PreemptionSignal>) {
        for signal in signals {
            let callbacks = {
                let mut state = lock(&self.state);
                state
                    .preemption_listeners
                    .retain(|listener| listener.live.load(Ordering::Acquire));
                state
                    .preemption_listeners
                    .iter()
                    .map(|listener| Arc::clone(&listener.callback))
                    .collect::<Vec<_>>()
            };
            for callback in callbacks {
                callback(&signal);
            }
        }
    }

    fn persist_record(&self, record: &JobRecord) -> Result<(), SystemError> {
        self.store.save_job(record)
    }
}

struct ProviderBridge {
    inner: Weak<ServiceInner>,
}

impl JobProviderListener for ProviderBridge {
    fn on_job_provider_notice(&self, notice: JobProviderNotice) {
        if let Some(inner) = self.inner.upgrade() {
            let service = JobsService {
                inner,
                bridge: None,
            };
            service.handle_provider_notice(notice);
        }
    }
}

/// Thread-safe durable job coordinator.
#[derive(Clone)]
pub struct JobsService {
    inner: Arc<ServiceInner>,
    bridge: Option<Arc<ProviderBridge>>,
}

impl JobsService {
    /// Open the durable jobs namespace.
    pub fn open(app: &AppData, provider: Arc<dyn JobProvider>) -> Result<Self, SystemError> {
        Self::open_with_options(
            app,
            provider,
            Arc::new(crate::SystemJobsClock),
            crate::DEFAULT_JOBS_EVENT_CAPACITY,
            Arc::new(NoopJobsStorageHook),
        )
    }

    /// Open with an explicit clock, event capacity and storage fault hook.
    pub fn open_with_options(
        app: &AppData,
        provider: Arc<dyn JobProvider>,
        clock: Arc<dyn crate::JobsClock>,
        event_capacity: usize,
        hook: Arc<dyn JobsStorageHook>,
    ) -> Result<Self, SystemError> {
        Self::build(app.clone(), None, provider, clock, event_capacity, hook)
    }

    /// Open a private temporary store. The directory is removed when the last
    /// clone of the service is dropped.
    pub fn in_memory(provider: Arc<dyn JobProvider>) -> Result<Self, SystemError> {
        Self::in_memory_with_options(
            provider,
            Arc::new(crate::SystemJobsClock),
            crate::DEFAULT_JOBS_EVENT_CAPACITY,
            Arc::new(NoopJobsStorageHook),
        )
    }

    /// Open a private temporary store with an explicit clock, event capacity
    /// and storage fault hook.
    pub fn in_memory_with_options(
        provider: Arc<dyn JobProvider>,
        clock: Arc<dyn crate::JobsClock>,
        event_capacity: usize,
        hook: Arc<dyn JobsStorageHook>,
    ) -> Result<Self, SystemError> {
        let temp = Arc::new(TempDir::new().map_err(|_| {
            error(
                ErrorCode::PlatformFailure,
                "Temporary job storage is unavailable",
            )
        })?);
        let roots = gpui_app_data::DirectoryRoots::under(temp.path());
        let app = AppData::open(roots)?;
        Self::build(app, Some(temp), provider, clock, event_capacity, hook)
    }

    fn build(
        app: AppData,
        temp: Option<Arc<TempDir>>,
        provider: Arc<dyn JobProvider>,
        clock: Arc<dyn crate::JobsClock>,
        event_capacity: usize,
        hook: Arc<dyn JobsStorageHook>,
    ) -> Result<Self, SystemError> {
        let store = JobStore::open(&app, hook)?;
        let now = clock.now_ms();
        let mut state = ServiceState::new(now);
        state.event_capacity = event_capacity.max(1);
        let inner = Arc::new(ServiceInner {
            state: Mutex::new(state),
            op_lock: Mutex::new(()),
            store,
            provider: Arc::clone(&provider),
            clock,
            in_process: InProcessExecutor::with_defaults(),
            _app: app,
            _temp: temp,
        });
        let bridge = Arc::new(ProviderBridge {
            inner: Arc::downgrade(&inner),
        });
        let service = Self {
            inner,
            bridge: Some(bridge),
        };
        service.attach_provider_listener();
        Ok(service)
    }

    /// True when both handles share the same state.
    pub fn shares_state_with(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.inner, &other.inner)
    }

    pub fn namespace_path(&self) -> &Path {
        self.inner.store.directory()
    }

    pub fn clock_now_ms(&self) -> u64 {
        self.inner.now()
    }

    pub fn in_process(&self) -> &InProcessExecutor {
        &self.inner.in_process
    }

    pub fn event_capacity(&self) -> usize {
        lock(&self.inner.state).event_capacity
    }

    /// Register with the provider listener. Called by the constructor once the
    /// bridge is retained.
    fn attach_provider_listener(&self) {
        if let Some(bridge) = &self.bridge {
            let listener: Arc<dyn JobProviderListener> = bridge.clone();
            self.inner
                .provider
                .attach_listener(Arc::downgrade(&listener));
        }
    }

    // -----------------------------------------------------------------
    // Registration and inspection
    // -----------------------------------------------------------------

    pub fn register_job(
        &self,
        context: &OperationContext,
        key: JobKey,
        constraints: JobConstraints,
        policy: ExistingJobPolicy,
    ) -> ServiceResult<ScheduleReceipt> {
        self.register_inner(
            context,
            key,
            constraints,
            policy,
            SchedulingGuarantee::AdvisoryOpportunistic,
        )
    }

    pub fn schedule_job(
        &self,
        context: &OperationContext,
        key: JobKey,
        constraints: JobConstraints,
        policy: ExistingJobPolicy,
    ) -> ServiceResult<ScheduleReceipt> {
        self.register_job(context, key, constraints, policy)
    }

    pub fn schedule_job_expedited(
        &self,
        context: &OperationContext,
        key: JobKey,
        constraints: JobConstraints,
        policy: ExistingJobPolicy,
    ) -> ServiceResult<ScheduleReceipt> {
        self.register_inner(
            context,
            key,
            constraints,
            policy,
            SchedulingGuarantee::ExpeditedBestEffort,
        )
    }

    fn register_inner(
        &self,
        context: &OperationContext,
        key: JobKey,
        constraints: JobConstraints,
        policy: ExistingJobPolicy,
        guarantee: SchedulingGuarantee,
    ) -> ServiceResult<ScheduleReceipt> {
        if let Some(reason) = background_cancellation(context)? {
            return Ok(Completion::Cancelled(reason));
        }
        constraints.validate()?;
        let _op = lock(&self.inner.op_lock);
        let now = self.inner.now();
        let existing = {
            let state = lock(&self.inner.state);
            state.records.get(&key).cloned()
        };
        if let Some(existing) = existing.clone() {
            match policy {
                ExistingJobPolicy::KeepExisting => {
                    return Ok(Completion::Completed(ScheduleReceipt {
                        job_key: key,
                        registration_disposition: RegistrationDisposition::KeptExisting,
                        status: existing.status,
                        run_epoch: existing.run_epoch,
                        guarantee: existing.guarantee,
                        earliest_eligible_after_ms: existing.earliest_eligible_at_ms,
                        unmet_constraints: existing.unmet_constraints,
                    }));
                }
                ExistingJobPolicy::RejectConflict => {
                    let mut conflict = error(
                        ErrorCode::InvalidInput,
                        "Job registration conflicts with an existing job",
                    );
                    conflict.retry = gpui_system::RetryAdvice::ModifyRequest;
                    return Err(conflict);
                }
                ExistingJobPolicy::UpdateConstraints => {
                    let (receipt, events, signals) =
                        self.update_constraints_inner(existing, constraints, now)?;
                    drop(_op);
                    self.inner.emit(events);
                    self.inner.fire_preemptions(signals);
                    return Ok(Completion::Completed(receipt));
                }
                ExistingJobPolicy::Replace => {}
            }
        }

        let replaced = existing.is_some();
        let (active_epoch, checkpoint_epoch) = {
            let state = lock(&self.inner.state);
            (
                state
                    .active
                    .get(&key)
                    .map(|active| active.token.run_epoch)
                    .unwrap_or(0),
                state
                    .checkpoints
                    .get(&key)
                    .map(|checkpoint| checkpoint.run_epoch)
                    .unwrap_or(0),
            )
        };
        let previous_epoch = existing
            .as_ref()
            .map_or(0, |record| record.run_epoch)
            .max(active_epoch)
            .max(checkpoint_epoch);
        let initial_delay_ms = constraints.initial_delay_ms;
        let deadline_ms = constraints.deadline_ms;
        let mut record = JobRecord {
            key: key.clone(),
            status: JobStatus::PendingConstraints,
            constraints,
            run_epoch: previous_epoch.saturating_add(1),
            attempt: 0,
            completed_runs: 0,
            guarantee,
            registered_at_ms: now,
            updated_at_ms: now,
            earliest_eligible_at_ms: now.saturating_add(initial_delay_ms),
            next_attempt_at_ms: now.saturating_add(initial_delay_ms),
            deadline_at_ms: deadline_ms.map(|value| now.saturating_add(value)),
            attempt_deadline_at_ms: None,
            checkpoint: None,
            progress: JobProgress::default(),
            last_failure: None,
            unmet_constraints: Vec::new(),
            idempotency_token: IdempotencyToken::generated(key.namespace(), key.name(), now),
            provider_token: None,
        };
        record.unmet_constraints = evaluate_constraints(&self.inner.state, &record, now);
        let mut events = Vec::new();
        let mut signals = Vec::new();
        if replaced {
            self.inner.store.clear_checkpoints(&key)?;
            self.inner.persist_record(&record)?;
            let _ = self.inner.provider.cancel(&key);
        } else {
            self.inner.persist_record(&record)?;
        }
        if record.unmet_constraints.is_empty() {
            self.try_schedule_provider(&mut record, now, &mut events);
        }
        {
            let mut state = lock(&self.inner.state);
            if replaced {
                state.checkpoints.remove(&key);
                if let Some(active) = state.active.remove(&key) {
                    events.push(JobsEvent::PreemptionRequested {
                        token: active.token.clone(),
                        reason: PreemptReason::Cancelled,
                    });
                    signals.push(PreemptionSignal {
                        token: active.token,
                        reason: PreemptReason::Cancelled,
                        requested_at_ms: now,
                    });
                }
            }
            state.records.insert(key.clone(), record.clone());
        }
        events.insert(
            0,
            JobsEvent::JobRegistered {
                job_key: key.clone(),
                disposition: if replaced {
                    RegistrationDisposition::Replaced
                } else {
                    RegistrationDisposition::Created
                },
                status: record.status,
            },
        );
        drop(_op);
        self.inner.emit(events);
        self.inner.fire_preemptions(signals);
        Ok(Completion::Completed(ScheduleReceipt {
            job_key: key,
            registration_disposition: if replaced {
                RegistrationDisposition::Replaced
            } else {
                RegistrationDisposition::Created
            },
            status: record.status,
            run_epoch: record.run_epoch,
            guarantee: record.guarantee,
            earliest_eligible_after_ms: record.earliest_eligible_at_ms,
            unmet_constraints: record.unmet_constraints,
        }))
    }

    fn update_constraints_inner(
        &self,
        mut record: JobRecord,
        constraints: JobConstraints,
        now: u64,
    ) -> Result<(ScheduleReceipt, Vec<JobsEvent>, Vec<PreemptionSignal>), SystemError> {
        let previous_status = record.status;
        record.constraints = constraints;
        record.updated_at_ms = now;
        record.earliest_eligible_at_ms = now.saturating_add(record.constraints.initial_delay_ms);
        record.next_attempt_at_ms = record
            .next_attempt_at_ms
            .max(record.earliest_eligible_at_ms);
        record.deadline_at_ms = record
            .constraints
            .deadline_ms
            .map(|value| record.registered_at_ms.saturating_add(value));
        record.unmet_constraints = evaluate_constraints(&self.inner.state, &record, now);
        let mut events = Vec::new();
        let mut signals = Vec::new();
        let due = record
            .earliest_eligible_at_ms
            .max(record.next_attempt_at_ms);
        if record.status == JobStatus::Running && !record.unmet_constraints.is_empty() {
            self.inner.persist_record(&record)?;
            let mut state = lock(&self.inner.state);
            state.records.insert(record.key.clone(), record.clone());
            self.preempt_locked(
                &mut state,
                &record.key,
                PreemptReason::ConstraintsLost,
                now,
                &mut events,
                &mut signals,
            );
        } else if record.status == JobStatus::Scheduled && !record.unmet_constraints.is_empty() {
            record.status = JobStatus::PendingConstraints;
            record.provider_token = None;
            self.inner.persist_record(&record)?;
            let _ = self.inner.provider.cancel(&record.key);
            lock(&self.inner.state)
                .records
                .insert(record.key.clone(), record.clone());
            events.push(JobsEvent::JobStateChanged {
                job_key: record.key.clone(),
                previous: previous_status,
                status: record.status,
            });
        } else {
            let schedule = record.unmet_constraints.is_empty()
                && now >= due
                && match record.status {
                    JobStatus::PendingConstraints | JobStatus::RetryBackoff => true,
                    JobStatus::Scheduled => record.provider_token.is_none(),
                    _ => false,
                };
            self.inner.persist_record(&record)?;
            if schedule {
                self.try_schedule_provider(&mut record, now, &mut events);
            }
            lock(&self.inner.state)
                .records
                .insert(record.key.clone(), record.clone());
            if previous_status != record.status {
                events.push(JobsEvent::JobStateChanged {
                    job_key: record.key.clone(),
                    previous: previous_status,
                    status: record.status,
                });
            }
        }
        Ok((
            ScheduleReceipt {
                job_key: record.key,
                registration_disposition: RegistrationDisposition::UpdatedConstraints,
                status: record.status,
                run_epoch: record.run_epoch,
                guarantee: record.guarantee,
                earliest_eligible_after_ms: record.earliest_eligible_at_ms,
                unmet_constraints: record.unmet_constraints,
            },
            events,
            signals,
        ))
    }

    pub fn job_record(&self, key: &JobKey) -> Option<JobRecord> {
        lock(&self.inner.state).records.get(key).cloned()
    }

    pub fn job_status(&self, key: &JobKey) -> Option<JobStatus> {
        lock(&self.inner.state)
            .records
            .get(key)
            .map(|record| record.status)
    }

    pub fn list_jobs(&self) -> Vec<JobRecord> {
        lock(&self.inner.state).records.values().cloned().collect()
    }

    pub fn checkpoint(&self, key: &JobKey) -> Option<JobCheckpoint> {
        lock(&self.inner.state).checkpoints.get(key).cloned()
    }

    pub fn active_token(&self, key: &JobKey) -> Option<crate::FencingToken> {
        lock(&self.inner.state)
            .active
            .get(key)
            .map(|active| active.token.clone())
    }

    pub fn unmet_constraints(&self, key: &JobKey) -> Vec<UnmetConstraint> {
        lock(&self.inner.state)
            .records
            .get(key)
            .map(|record| record.unmet_constraints.clone())
            .unwrap_or_default()
    }

    pub fn evaluate_job(&self, key: &JobKey, now_ms: u64) -> Option<Vec<UnmetConstraint>> {
        let state = lock(&self.inner.state);
        state
            .records
            .get(key)
            .map(|record| evaluate_state(&state, record, now_ms))
    }

    pub fn power_pacing(&self) -> gpui_power::WorkPacingRecommendation {
        let state = lock(&self.inner.state);
        state.power_policy.evaluate(&state.power)
    }

    // -----------------------------------------------------------------
    // Attempt fencing
    // -----------------------------------------------------------------

    fn validate_token(
        state: &ServiceState,
        token: &crate::FencingToken,
    ) -> Result<(), SystemError> {
        let record = state
            .records
            .get(&token.job_key)
            .ok_or_else(|| error(ErrorCode::InvalidInput, "Job is not registered"))?;
        let active = state.active.get(&token.job_key);
        if record.status != JobStatus::Running
            || record.run_epoch != token.run_epoch
            || record.attempt != token.attempt
            || active.is_none_or(|active| active.token != *token)
        {
            return Err(stale_fencing_error());
        }
        Ok(())
    }

    pub fn save_checkpoint(
        &self,
        token: &crate::FencingToken,
        payload: Vec<u8>,
        progress: JobProgress,
        context: &OperationContext,
    ) -> ServiceResult<JobCheckpoint> {
        if let Some(reason) = background_cancellation(context)? {
            return Ok(Completion::Cancelled(reason));
        }
        if payload.len() > MAX_CHECKPOINT_BYTES {
            return Err(error(
                ErrorCode::InvalidInput,
                "Checkpoint payload exceeds the size bound",
            ));
        }
        let _op = lock(&self.inner.op_lock);
        let now = self.inner.now();
        let (checkpoint, updated) = {
            let state = lock(&self.inner.state);
            Self::validate_token(&state, token)?;
            let active = state
                .active
                .get(&token.job_key)
                .ok_or_else(stale_fencing_error)?;
            if active.preemption.is_some() && active.final_checkpoint_saved {
                return Err(stale_fencing_error());
            }
            let sequence = state
                .checkpoints
                .get(&token.job_key)
                .map_or(1, |checkpoint| checkpoint.sequence.saturating_add(1));
            let checkpoint = JobCheckpoint::new(
                token.job_key.clone(),
                token.run_epoch,
                token.attempt,
                sequence,
                progress.clone(),
                payload,
                now,
            )?;
            let mut record = state
                .records
                .get(&token.job_key)
                .cloned()
                .ok_or_else(|| error(ErrorCode::InvalidInput, "Job is not registered"))?;
            record.checkpoint = Some(checkpoint.summary());
            record.progress = progress;
            record.updated_at_ms = now;
            (checkpoint, record)
        };
        self.inner.store.save_checkpoint(&checkpoint)?;
        self.inner.persist_record(&updated)?;
        {
            let mut state = lock(&self.inner.state);
            Self::validate_token(&state, token)?;
            if let Some(active) = state.active.get_mut(&token.job_key)
                && active.preemption.is_some()
            {
                active.final_checkpoint_saved = true;
            }
            state
                .checkpoints
                .insert(token.job_key.clone(), checkpoint.clone());
            state.records.insert(token.job_key.clone(), updated);
        }
        drop(_op);
        self.inner.emit(vec![JobsEvent::CheckpointCommitted {
            job_key: checkpoint.job_key.clone(),
            run_epoch: checkpoint.run_epoch,
            sequence: checkpoint.sequence,
        }]);
        Ok(Completion::Completed(checkpoint))
    }

    pub fn report_progress(
        &self,
        token: &crate::FencingToken,
        progress: JobProgress,
        context: &OperationContext,
    ) -> ServiceResult<JobProgress> {
        if let Some(reason) = background_cancellation(context)? {
            return Ok(Completion::Cancelled(reason));
        }
        let _op = lock(&self.inner.op_lock);
        let now = self.inner.now();
        let updated = {
            let state = lock(&self.inner.state);
            Self::validate_token(&state, token)?;
            if state
                .active
                .get(&token.job_key)
                .is_some_and(|active| active.preemption.is_some())
            {
                return Err(stale_fencing_error());
            }
            let mut record = state
                .records
                .get(&token.job_key)
                .cloned()
                .ok_or_else(|| error(ErrorCode::InvalidInput, "Job is not registered"))?;
            record.progress = progress.clone();
            record.updated_at_ms = now;
            record
        };
        self.inner.persist_record(&updated)?;
        {
            let mut state = lock(&self.inner.state);
            Self::validate_token(&state, token)?;
            state.records.insert(token.job_key.clone(), updated);
        }
        let basis_points = progress.basis_points();
        let key = token.job_key.clone();
        let epoch = token.run_epoch;
        drop(_op);
        self.inner.emit(vec![JobsEvent::ProgressReported {
            job_key: key,
            run_epoch: epoch,
            basis_points,
        }]);
        Ok(Completion::Completed(progress))
    }

    pub fn heartbeat(
        &self,
        token: &crate::FencingToken,
        context: &OperationContext,
    ) -> ServiceResult<HeartbeatReceipt> {
        if let Some(reason) = background_cancellation(context)? {
            return Ok(Completion::Cancelled(reason));
        }
        let _op = lock(&self.inner.op_lock);
        let now = self.inner.now();
        let mut events = Vec::new();
        let mut signals = Vec::new();
        let receipt = {
            let mut state = lock(&self.inner.state);
            Self::validate_token(&state, token)?;
            if state
                .active
                .get(&token.job_key)
                .is_some_and(|active| active.preemption.is_some())
            {
                return Err(stale_fencing_error());
            }
            let deadline = state
                .records
                .get(&token.job_key)
                .and_then(|record| record.attempt_deadline_at_ms);
            if deadline.is_some_and(|deadline| now >= deadline) {
                self.preempt_locked(
                    &mut state,
                    &token.job_key,
                    PreemptReason::BudgetExpired,
                    now,
                    &mut events,
                    &mut signals,
                );
                drop(state);
                drop(_op);
                self.inner.emit(events);
                self.inner.fire_preemptions(signals);
                return Err(error(
                    ErrorCode::TimedOut,
                    "Attempt execution budget elapsed",
                ));
            }
            HeartbeatReceipt {
                token: token.clone(),
                attempt_deadline_at_ms: deadline,
                remaining_ms: deadline.map_or(0, |deadline| deadline.saturating_sub(now)),
            }
        };
        drop(_op);
        self.inner.emit(events);
        self.inner.fire_preemptions(signals);
        Ok(Completion::Completed(receipt))
    }

    pub fn complete_job(
        &self,
        token: &crate::FencingToken,
        context: &OperationContext,
    ) -> ServiceResult<JobRecord> {
        if let Some(reason) = background_cancellation(context)? {
            return Ok(Completion::Cancelled(reason));
        }
        let _op = lock(&self.inner.op_lock);
        let now = self.inner.now();
        let mut events = Vec::new();
        let mut schedule_next = None;
        let record = {
            let mut state = lock(&self.inner.state);
            Self::validate_token(&state, token)?;
            if state
                .active
                .get(&token.job_key)
                .is_some_and(|active| active.preemption.is_some())
            {
                return Err(stale_fencing_error());
            }
            let existing = state
                .records
                .get(&token.job_key)
                .cloned()
                .ok_or_else(|| error(ErrorCode::InvalidInput, "Job is not registered"))?;
            if existing
                .deadline_at_ms
                .is_some_and(|deadline| now >= deadline)
            {
                let mut expired = existing.clone();
                self.mark_expired(
                    &mut state,
                    &mut expired,
                    BudgetExpiry::Deadline,
                    now,
                    &mut events,
                );
                return Ok(Completion::Completed(expired));
            }
            let mut record = existing;
            record.completed_runs = record.completed_runs.saturating_add(1);
            record.attempt_deadline_at_ms = None;
            record.last_failure = None;
            record.progress = JobProgress::from_basis_points(10_000)?;
            record.unmet_constraints.clear();
            record.updated_at_ms = now;
            state.active.remove(&token.job_key);
            if record.constraints.cadence.is_periodic() {
                record.status = JobStatus::Scheduled;
                record.attempt = 0;
                record.next_attempt_at_ms =
                    now.saturating_add(record.constraints.cadence.interval_ms().unwrap_or(60_000));
                record.provider_token = None;
                schedule_next = Some(record.clone());
            } else {
                record.status = JobStatus::Succeeded;
            }
            self.inner.persist_record(&record)?;
            state.records.insert(token.job_key.clone(), record.clone());
            events.push(JobsEvent::JobSucceeded {
                job_key: token.job_key.clone(),
                run_epoch: token.run_epoch,
            });
            if record.status == JobStatus::Scheduled {
                events.push(JobsEvent::JobScheduled {
                    job_key: token.job_key.clone(),
                    run_epoch: record.run_epoch,
                    earliest_eligible_after_ms: record.next_attempt_at_ms,
                });
            }
            record
        };
        if let Some(mut next) = schedule_next {
            self.try_schedule_provider(&mut next, now, &mut events);
            let mut state = lock(&self.inner.state);
            if let Some(current) = state.records.get_mut(&next.key) {
                current.provider_token = next.provider_token;
                current.unmet_constraints = next.unmet_constraints.clone();
                if current.status != JobStatus::Cancelled {
                    current.status = next.status;
                }
                self.inner.persist_record(current)?;
            }
        }
        drop(_op);
        self.inner.emit(events);
        Ok(Completion::Completed(record))
    }

    pub fn fail_job(
        &self,
        token: &crate::FencingToken,
        error_code: ErrorCode,
        retryable: bool,
        context: &OperationContext,
    ) -> ServiceResult<JobRecord> {
        if let Some(reason) = background_cancellation(context)? {
            return Ok(Completion::Cancelled(reason));
        }
        let _op = lock(&self.inner.op_lock);
        let now = self.inner.now();
        let mut events = Vec::new();
        let record = {
            let mut state = lock(&self.inner.state);
            Self::validate_token(&state, token)?;
            if state
                .active
                .get(&token.job_key)
                .is_some_and(|active| active.preemption.is_some())
            {
                return Err(stale_fencing_error());
            }
            let mut record = state
                .records
                .get(&token.job_key)
                .cloned()
                .ok_or_else(|| error(ErrorCode::InvalidInput, "Job is not registered"))?;
            record.attempt_deadline_at_ms = None;
            record.last_failure = Some(error_code.into());
            record.updated_at_ms = now;
            state.active.remove(&token.job_key);
            if record
                .deadline_at_ms
                .is_some_and(|deadline| now >= deadline)
            {
                self.mark_expired(
                    &mut state,
                    &mut record,
                    BudgetExpiry::Deadline,
                    now,
                    &mut events,
                );
                return Ok(Completion::Completed(record));
            }
            if retryable
                && record
                    .constraints
                    .retry
                    .allows_another_attempt(record.attempt)
            {
                let delay = record.constraints.retry.delay_for_attempt(record.attempt);
                record.status = JobStatus::RetryBackoff;
                record.next_attempt_at_ms = now.saturating_add(delay);
                record.unmet_constraints = vec![UnmetConstraint::RetryBackoff {
                    remaining_ms: delay,
                }];
                self.inner.persist_record(&record)?;
                state.records.insert(token.job_key.clone(), record.clone());
                events.push(JobsEvent::RetryScheduled {
                    job_key: token.job_key.clone(),
                    run_epoch: record.run_epoch,
                    attempt: record.attempt,
                    delay_ms: delay,
                });
                events.push(JobsEvent::JobStateChanged {
                    job_key: token.job_key.clone(),
                    previous: JobStatus::Running,
                    status: JobStatus::RetryBackoff,
                });
            } else if retryable {
                self.mark_expired(
                    &mut state,
                    &mut record,
                    BudgetExpiry::AttemptsExhausted,
                    now,
                    &mut events,
                );
            } else {
                record.status = JobStatus::Failed;
                record.unmet_constraints.clear();
                self.inner.persist_record(&record)?;
                state.records.insert(token.job_key.clone(), record.clone());
                events.push(JobsEvent::JobFailed {
                    job_key: token.job_key.clone(),
                    run_epoch: token.run_epoch,
                    reason: error_code.into(),
                });
            }
            record
        };
        drop(_op);
        self.inner.emit(events);
        Ok(Completion::Completed(record))
    }

    pub fn ack_preemption(
        &self,
        token: &crate::FencingToken,
        context: &OperationContext,
    ) -> ServiceResult<JobRecord> {
        if let Some(reason) = background_cancellation(context)? {
            return Ok(Completion::Cancelled(reason));
        }
        let _op = lock(&self.inner.op_lock);
        let now = self.inner.now();
        let mut events = Vec::new();
        let mut record = {
            let mut state = lock(&self.inner.state);
            Self::validate_token(&state, token)?;
            let reason = state
                .active
                .get(&token.job_key)
                .and_then(|active| active.preemption)
                .ok_or_else(|| {
                    error(
                        ErrorCode::InvalidInput,
                        "No preemption is pending for this attempt",
                    )
                })?;
            let mut record = state
                .records
                .get(&token.job_key)
                .cloned()
                .ok_or_else(|| error(ErrorCode::InvalidInput, "Job is not registered"))?;
            state.active.remove(&token.job_key);
            record.run_epoch = record.run_epoch.saturating_add(1);
            record.attempt_deadline_at_ms = None;
            record.provider_token = None;
            record.updated_at_ms = now;
            let deadline_expired = record
                .deadline_at_ms
                .is_some_and(|deadline| now >= deadline);
            if deadline_expired {
                record.status = JobStatus::ExpiredBudget;
                record.unmet_constraints.clear();
                events.push(JobsEvent::JobExpiredBudget {
                    job_key: token.job_key.clone(),
                    reason: BudgetExpiry::Deadline,
                });
            } else if let Some(expiry) = reason.budget_expiry() {
                record.status = JobStatus::ExpiredBudget;
                record.unmet_constraints.clear();
                events.push(JobsEvent::JobExpiredBudget {
                    job_key: token.job_key.clone(),
                    reason: expiry,
                });
            } else {
                record.status = JobStatus::PendingConstraints;
                record.next_attempt_at_ms = now;
                record.unmet_constraints = evaluate_state(&state, &record, now);
            }
            self.inner.persist_record(&record)?;
            state.records.insert(token.job_key.clone(), record.clone());
            events.push(JobsEvent::PreemptionAcknowledged {
                token: token.clone(),
                reason,
                status: record.status,
            });
            record
        };
        if is_schedule_eligible(&record, now) {
            let mut schedule_events = Vec::new();
            self.try_schedule_provider(&mut record, now, &mut schedule_events);
            events.extend(schedule_events);
            let mut state = lock(&self.inner.state);
            state.records.insert(token.job_key.clone(), record.clone());
            self.inner.persist_record(&record)?;
        }
        drop(_op);
        self.inner.emit(events);
        Ok(Completion::Completed(record))
    }

    pub fn cancel_job(&self, key: &JobKey, context: &OperationContext) -> ServiceResult<JobRecord> {
        if let Some(reason) = background_cancellation(context)? {
            return Ok(Completion::Cancelled(reason));
        }
        let _op = lock(&self.inner.op_lock);
        let now = self.inner.now();
        let mut events = Vec::new();
        let mut signals = Vec::new();
        let record = {
            let mut state = lock(&self.inner.state);
            let mut record = state
                .records
                .get(key)
                .cloned()
                .ok_or_else(|| error(ErrorCode::InvalidInput, "Job is not registered"))?;
            if record.status == JobStatus::Cancelled {
                return Ok(Completion::Completed(record));
            }
            if record.status.is_terminal() {
                return Ok(Completion::Completed(record));
            }
            let previous = record.status;
            let active = state.active.remove(key);
            record.status = JobStatus::Cancelled;
            record.run_epoch = record.run_epoch.saturating_add(1);
            record.attempt_deadline_at_ms = None;
            record.provider_token = None;
            record.unmet_constraints.clear();
            record.updated_at_ms = now;
            self.inner.persist_record(&record)?;
            state.records.insert(key.clone(), record.clone());
            events.push(JobsEvent::JobCancelled {
                job_key: key.clone(),
                run_epoch: record.run_epoch,
            });
            if previous != JobStatus::Cancelled {
                events.push(JobsEvent::JobStateChanged {
                    job_key: key.clone(),
                    previous,
                    status: JobStatus::Cancelled,
                });
            }
            if let Some(active) = active {
                events.push(JobsEvent::PreemptionRequested {
                    token: active.token.clone(),
                    reason: PreemptReason::Cancelled,
                });
                signals.push(PreemptionSignal {
                    token: active.token,
                    reason: PreemptReason::Cancelled,
                    requested_at_ms: now,
                });
            }
            record
        };
        let _ = self.inner.provider.cancel(key);
        drop(_op);
        self.inner.emit(events);
        self.inner.fire_preemptions(signals);
        Ok(Completion::Completed(record))
    }

    pub fn preempt_job(
        &self,
        key: &JobKey,
        reason: PreemptReason,
        context: &OperationContext,
    ) -> ServiceResult<bool> {
        if let Some(cancel_reason) = background_cancellation(context)? {
            return Ok(Completion::Cancelled(cancel_reason));
        }
        let _op = lock(&self.inner.op_lock);
        let now = self.inner.now();
        let mut events = Vec::new();
        let mut signals = Vec::new();
        let changed = {
            let mut state = lock(&self.inner.state);
            self.preempt_locked(&mut state, key, reason, now, &mut events, &mut signals)
        };
        drop(_op);
        self.inner.emit(events);
        self.inner.fire_preemptions(signals);
        Ok(Completion::Completed(changed))
    }

    // -----------------------------------------------------------------
    // Dynamic constraints and lifecycle
    // -----------------------------------------------------------------

    pub fn tick(&self, context: &OperationContext) -> ServiceResult<JobsUpdate> {
        if let Some(reason) = background_cancellation(context)? {
            return Ok(Completion::Cancelled(reason));
        }
        let _op = lock(&self.inner.op_lock);
        let now = self.inner.now();
        let outcome = self.advance_locked(now)?;
        drop(_op);
        self.inner.emit(outcome.events);
        self.inner.fire_preemptions(outcome.signals);
        Ok(Completion::Completed(outcome.update))
    }

    pub fn apply_power_snapshot(
        &self,
        snapshot: PowerSnapshot,
        context: &OperationContext,
    ) -> ServiceResult<JobsUpdate> {
        if let Some(reason) = background_cancellation(context)? {
            return Ok(Completion::Cancelled(reason));
        }
        let _op = lock(&self.inner.op_lock);
        lock(&self.inner.state).power = snapshot;
        let outcome = self.advance_locked(self.inner.now())?;
        drop(_op);
        self.inner.emit(outcome.events);
        self.inner.fire_preemptions(outcome.signals);
        Ok(Completion::Completed(outcome.update))
    }

    pub fn apply_network_snapshot(
        &self,
        snapshot: NetworkSnapshot,
        context: &OperationContext,
    ) -> ServiceResult<JobsUpdate> {
        if let Some(reason) = background_cancellation(context)? {
            return Ok(Completion::Cancelled(reason));
        }
        let _op = lock(&self.inner.op_lock);
        lock(&self.inner.state).network = snapshot;
        let outcome = self.advance_locked(self.inner.now())?;
        drop(_op);
        self.inner.emit(outcome.events);
        self.inner.fire_preemptions(outcome.signals);
        Ok(Completion::Completed(outcome.update))
    }

    pub fn apply_lifecycle_state(
        &self,
        lifecycle: &LifecycleState,
        context: &OperationContext,
    ) -> ServiceResult<JobsUpdate> {
        self.apply_lifecycle_phase(lifecycle.phase(), lifecycle.revision(), context)
    }

    pub fn apply_lifecycle_phase(
        &self,
        phase: AppPhase,
        revision: u64,
        context: &OperationContext,
    ) -> ServiceResult<JobsUpdate> {
        if let Some(reason) = background_cancellation(context)? {
            return Ok(Completion::Cancelled(reason));
        }
        let _op = lock(&self.inner.op_lock);
        {
            let mut state = lock(&self.inner.state);
            state.lifecycle = phase;
            state.lifecycle_revision = state.lifecycle_revision.max(revision);
        }
        let outcome = self.advance_locked(self.inner.now())?;
        drop(_op);
        self.inner.emit(outcome.events);
        self.inner.fire_preemptions(outcome.signals);
        Ok(Completion::Completed(outcome.update))
    }

    pub fn apply_permission_status(
        &self,
        permission: JobPermissionRequirement,
        status: PermissionStatus,
        context: &OperationContext,
    ) -> ServiceResult<JobsUpdate> {
        if let Some(reason) = background_cancellation(context)? {
            return Ok(Completion::Cancelled(reason));
        }
        let _op = lock(&self.inner.op_lock);
        lock(&self.inner.state)
            .permissions
            .insert(permission.label().to_owned(), status);
        let outcome = self.advance_locked(self.inner.now())?;
        drop(_op);
        self.inner.emit(outcome.events);
        self.inner.fire_preemptions(outcome.signals);
        Ok(Completion::Completed(outcome.update))
    }

    pub fn apply_permission_kind(
        &self,
        kind: gpui_permissions::PermissionKind,
        status: PermissionStatus,
        context: &OperationContext,
    ) -> ServiceResult<JobsUpdate> {
        self.apply_permission_status(kind.into(), status, context)
    }

    pub fn apply_device_state(
        &self,
        device_idle: bool,
        storage_low: bool,
        context: &OperationContext,
    ) -> ServiceResult<JobsUpdate> {
        if let Some(reason) = background_cancellation(context)? {
            return Ok(Completion::Cancelled(reason));
        }
        let _op = lock(&self.inner.op_lock);
        {
            let mut state = lock(&self.inner.state);
            state.device_idle = device_idle;
            state.storage_low = storage_low;
        }
        let outcome = self.advance_locked(self.inner.now())?;
        drop(_op);
        self.inner.emit(outcome.events);
        self.inner.fire_preemptions(outcome.signals);
        Ok(Completion::Completed(outcome.update))
    }

    pub fn set_power_budget_policy(&self, policy: PowerBudgetPolicy) {
        lock(&self.inner.state).power_policy = policy;
    }

    pub fn current_power(&self) -> PowerSnapshot {
        lock(&self.inner.state).power.clone()
    }

    pub fn current_network(&self) -> NetworkSnapshot {
        lock(&self.inner.state).network.clone()
    }

    pub fn current_lifecycle(&self) -> AppPhase {
        lock(&self.inner.state).lifecycle
    }

    // -----------------------------------------------------------------
    // Provider wakeups and reconciliation
    // -----------------------------------------------------------------

    pub fn handle_wakeup(
        &self,
        key: &JobKey,
        context: &OperationContext,
    ) -> ServiceResult<WakeupOutcome> {
        if let Some(reason) = background_cancellation(context)? {
            return Ok(Completion::Cancelled(reason));
        }
        let _op = lock(&self.inner.op_lock);
        let now = self.inner.now();
        let (outcome, events, signals) = self.wakeup_locked(key, now)?;
        drop(_op);
        self.inner.emit(events);
        self.inner.fire_preemptions(signals);
        Ok(Completion::Completed(outcome))
    }

    pub fn start_attempt(
        &self,
        key: &JobKey,
        context: &OperationContext,
    ) -> ServiceResult<WakeupOutcome> {
        self.handle_wakeup(key, context)
    }

    pub fn reconcile_after_restart(
        &self,
        context: &OperationContext,
    ) -> ServiceResult<ReconciliationReport> {
        if let Some(reason) = background_cancellation(context)? {
            return Ok(Completion::Cancelled(reason));
        }
        let _op = lock(&self.inner.op_lock);
        {
            let state = lock(&self.inner.state);
            if state.loaded {
                return Ok(Completion::Completed(
                    state.last_reconciliation.clone().unwrap_or_default(),
                ));
            }
        }
        let now = self.inner.now();
        let loaded = self.inner.store.load_all()?;
        let mut report = ReconciliationReport {
            jobs_scanned: loaded.jobs.len(),
            corrupt_records_quarantined: loaded.quarantined.len(),
            checkpoints_recovered: loaded.recovered_checkpoints.len(),
            ..ReconciliationReport::default()
        };
        let mut events = Vec::new();
        for quarantined in &loaded.quarantined {
            events.push(JobsEvent::CorruptRecordQuarantined {
                record_kind: quarantined.kind.as_str(),
                bytes: quarantined.bytes,
            });
            events.push(JobsEvent::OperationFailed {
                operation: JobsOperation::CheckpointAndRecover,
                code: ErrorCode::IntegrityFailure,
            });
        }
        for recovered in &loaded.recovered_checkpoints {
            events.push(JobsEvent::CheckpointRecovered {
                job_key: recovered.job_key.clone(),
                run_epoch: recovered.run_epoch,
                sequence: recovered.sequence,
            });
        }
        let mut schedule_calls = Vec::new();
        {
            let mut state = lock(&self.inner.state);
            for mut record in loaded.jobs {
                let previous_status = record.status;
                let previous_epoch = record.run_epoch;
                if let Some(checkpoint) = loaded.checkpoints.get(&record.key) {
                    if checkpoint.run_epoch > record.run_epoch {
                        record.run_epoch = checkpoint.run_epoch;
                    }
                    if checkpoint.progress.basis_points() > record.progress.basis_points() {
                        record.progress = checkpoint.progress.clone();
                    }
                    record.checkpoint = Some(checkpoint.summary());
                    state
                        .checkpoints
                        .insert(record.key.clone(), checkpoint.clone());
                }
                if record.status == JobStatus::Running {
                    report.running_reconciled += 1;
                    record.run_epoch = record.run_epoch.saturating_add(1);
                    record.attempt_deadline_at_ms = None;
                    record.provider_token = None;
                    record.updated_at_ms = now;
                    let deadline_expired = record
                        .deadline_at_ms
                        .is_some_and(|deadline| now >= deadline);
                    let attempts_exhausted =
                        record.attempt >= record.constraints.retry.max_attempts();
                    if deadline_expired || attempts_exhausted {
                        record.status = JobStatus::ExpiredBudget;
                        record.unmet_constraints.clear();
                        report.expired_budget += 1;
                    } else if now < record.next_attempt_at_ms {
                        record.status = JobStatus::RetryBackoff;
                        record.unmet_constraints = evaluate_state(&state, &record, now);
                        report.retry_backoff += 1;
                    } else {
                        record.status = JobStatus::PendingConstraints;
                        record.unmet_constraints = evaluate_state(&state, &record, now);
                        if record.unmet_constraints.is_empty() {
                            record.status = JobStatus::Scheduled;
                            schedule_calls.push(record.clone());
                            report.rescheduled += 1;
                        } else {
                            report.pending_constraints += 1;
                        }
                    }
                } else if matches!(
                    record.status,
                    JobStatus::Scheduled | JobStatus::PendingConstraints | JobStatus::RetryBackoff
                ) {
                    // A provider token from the previous process is stale even
                    // when the record was persisted as Scheduled.
                    record.provider_token = None;
                    record.updated_at_ms = now;
                    let deadline_expired = record
                        .deadline_at_ms
                        .is_some_and(|deadline| now >= deadline);
                    if deadline_expired {
                        record.status = JobStatus::ExpiredBudget;
                        record.unmet_constraints.clear();
                        report.expired_budget += 1;
                    } else {
                        record.unmet_constraints = evaluate_state(&state, &record, now);
                        let due = record
                            .earliest_eligible_at_ms
                            .max(record.next_attempt_at_ms);
                        if !record.unmet_constraints.is_empty() {
                            let status = if record.status == JobStatus::RetryBackoff
                                && now < record.next_attempt_at_ms
                            {
                                JobStatus::RetryBackoff
                            } else {
                                JobStatus::PendingConstraints
                            };
                            if status == JobStatus::PendingConstraints
                                && previous_status != JobStatus::PendingConstraints
                            {
                                report.pending_constraints += 1;
                            }
                            record.status = status;
                        } else if now >= due {
                            record.status = JobStatus::Scheduled;
                            schedule_calls.push(record.clone());
                            report.rescheduled += 1;
                        } else if record.status == JobStatus::Scheduled {
                            // A periodic job that completed an earlier run is
                            // persisted as Scheduled with a future interval. It
                            // must stay Scheduled and be re-registered with the
                            // new provider, not downgraded to RetryBackoff.
                            record.status = JobStatus::Scheduled;
                            schedule_calls.push(record.clone());
                            report.rescheduled += 1;
                        } else if record.status == JobStatus::RetryBackoff {
                            record.unmet_constraints = vec![UnmetConstraint::RetryBackoff {
                                remaining_ms: due.saturating_sub(now),
                            }];
                            report.retry_backoff += 1;
                        } else {
                            // PendingConstraints with an empty unmet set but a
                            // future due time stays pending until its delay.
                            record.status = JobStatus::PendingConstraints;
                        }
                    }
                }
                self.inner.persist_record(&record)?;
                state.records.insert(record.key.clone(), record.clone());
                if previous_status != record.status || previous_epoch != record.run_epoch {
                    events.push(JobsEvent::JobReconciled {
                        job_key: record.key.clone(),
                        previous_status,
                        status: record.status,
                        previous_epoch,
                        run_epoch: record.run_epoch,
                    });
                }
            }
            state.loaded = true;
            state.last_reconciliation = Some(report.clone());
        }
        for mut record in schedule_calls {
            let scheduled_status = record.status;
            let mut schedule_events = Vec::new();
            self.try_schedule_provider(&mut record, now, &mut schedule_events);
            events.extend(schedule_events);
            if record.status != scheduled_status && record.status == JobStatus::PendingConstraints {
                events.push(JobsEvent::JobReconciled {
                    job_key: record.key.clone(),
                    previous_status: scheduled_status,
                    status: record.status,
                    previous_epoch: record.run_epoch,
                    run_epoch: record.run_epoch,
                });
            }
            let mut state = lock(&self.inner.state);
            if let Some(current) = state.records.get_mut(&record.key)
                && !current.status.is_terminal()
            {
                current.status = record.status;
                current.provider_token = record.provider_token;
                current.unmet_constraints = record.unmet_constraints.clone();
                self.inner.persist_record(current)?;
            }
        }
        drop(_op);
        self.inner.emit(events);
        Ok(Completion::Completed(report))
    }

    /// Subscribe to the bounded jobs event stream. Historical events are not
    /// replayed; a new subscriber starts at the current revision.
    pub fn subscribe(&self) -> EventSubscription<JobsEvent> {
        let mut state = lock(&self.inner.state);
        state
            .subscribers
            .retain(|(live, _)| live.load(Ordering::Acquire));
        let live = Arc::new(AtomicBool::new(true));
        let cleanup_live = Arc::clone(&live);
        let (sender, subscription) = event_channel(state.event_capacity, move || {
            cleanup_live.store(false, Ordering::Release);
        });
        state.subscribers.push((live, sender));
        subscription
    }

    /// Highest sequence delivered to any live subscription.
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

    /// Register a preemption callback. Callbacks run outside every service
    /// lock and may reenter the service.
    pub fn on_preemption<F>(&self, callback: F) -> PreemptionSubscription
    where
        F: Fn(&PreemptionSignal) + Send + Sync + 'static,
    {
        let mut state = lock(&self.inner.state);
        let id = state.next_listener_id;
        state.next_listener_id = state.next_listener_id.saturating_add(1);
        let live = Arc::new(AtomicBool::new(true));
        state.preemption_listeners.push(PreemptionRecord {
            id,
            live: Arc::clone(&live),
            callback: Arc::new(callback),
        });
        PreemptionSubscription {
            id,
            live,
            service: Arc::downgrade(&self.inner),
        }
    }

    fn handle_provider_notice(&self, notice: JobProviderNotice) {
        let _op = lock(&self.inner.op_lock);
        let now = self.inner.now();
        let mut events = Vec::new();
        let mut signals = Vec::new();
        match notice {
            JobProviderNotice::Wakeup { key } => match self.wakeup_locked(&key, now) {
                Ok((outcome, mut wakeup_events, mut wakeup_signals)) => {
                    events.append(&mut wakeup_events);
                    signals.append(&mut wakeup_signals);
                    let _ = outcome;
                }
                Err(failure) => events.push(JobsEvent::OperationFailed {
                    operation: JobsOperation::ScheduleDurableJob,
                    code: failure.code,
                }),
            },
            JobProviderNotice::TimeSliceExpired { key, run_epoch } => {
                let mut state = lock(&self.inner.state);
                let matches = state
                    .active
                    .get(&key)
                    .is_some_and(|active| active.token.run_epoch == run_epoch);
                if matches {
                    self.preempt_locked(
                        &mut state,
                        &key,
                        PreemptReason::ProviderTimeSliceExpired,
                        now,
                        &mut events,
                        &mut signals,
                    );
                }
            }
            JobProviderNotice::ProviderUnavailable { key } => {
                let mut state = lock(&self.inner.state);
                if state.active.contains_key(&key) {
                    self.preempt_locked(
                        &mut state,
                        &key,
                        PreemptReason::ConstraintsLost,
                        now,
                        &mut events,
                        &mut signals,
                    );
                }
                if let Some(record) = state.records.get_mut(&key)
                    && !record
                        .unmet_constraints
                        .iter()
                        .any(|unmet| matches!(unmet, UnmetConstraint::ProviderUnavailable))
                {
                    record
                        .unmet_constraints
                        .push(UnmetConstraint::ProviderUnavailable);
                    record.updated_at_ms = now;
                    let clone = record.clone();
                    let _ = self.inner.persist_record(&clone);
                }
                events.push(JobsEvent::OperationFailed {
                    operation: JobsOperation::ScheduleDurableJob,
                    code: ErrorCode::Unavailable,
                });
            }
        }
        drop(_op);
        self.inner.emit(events);
        self.inner.fire_preemptions(signals);
    }

    fn try_schedule_provider(&self, record: &mut JobRecord, now: u64, events: &mut Vec<JobsEvent>) {
        let request = JobScheduleRequest {
            key: record.key.clone(),
            idempotency_token: record.idempotency_token.clone(),
            guarantee: record.guarantee,
            earliest_eligible_after_ms: record
                .earliest_eligible_at_ms
                .max(record.next_attempt_at_ms)
                .max(now),
            cadence: record.constraints.cadence,
            execution_budget_ms: record.constraints.execution_budget_ms,
            deadline_at_ms: record.deadline_at_ms,
            network: record.constraints.network,
            requires_charging: record.constraints.requires_charging,
            requires_device_idle: record.constraints.requires_device_idle,
            requires_storage_not_low: record.constraints.requires_storage_not_low,
        };
        match self.inner.provider.schedule(&request) {
            Ok(handle) => {
                record.status = JobStatus::Scheduled;
                record.provider_token = Some(handle.provider_token);
                record.unmet_constraints.clear();
                record.updated_at_ms = now;
                let _ = self.inner.persist_record(record);
                events.push(JobsEvent::JobScheduled {
                    job_key: record.key.clone(),
                    run_epoch: record.run_epoch,
                    earliest_eligible_after_ms: handle.scheduled_at_ms,
                });
            }
            Err(failure) => {
                record.status = JobStatus::PendingConstraints;
                record.provider_token = None;
                record.updated_at_ms = now;
                if !record
                    .unmet_constraints
                    .iter()
                    .any(|unmet| matches!(unmet, UnmetConstraint::ProviderUnavailable))
                {
                    record
                        .unmet_constraints
                        .push(UnmetConstraint::ProviderUnavailable);
                }
                let _ = self.inner.persist_record(record);
                events.push(JobsEvent::OperationFailed {
                    operation: JobsOperation::ScheduleDurableJob,
                    code: failure.code,
                });
            }
        }
    }

    fn wakeup_locked(
        &self,
        key: &JobKey,
        now: u64,
    ) -> Result<(WakeupOutcome, Vec<JobsEvent>, Vec<PreemptionSignal>), SystemError> {
        let mut events = Vec::new();
        let signals = Vec::new();
        let mut state = lock(&self.inner.state);
        let mut record = state
            .records
            .get(key)
            .cloned()
            .ok_or_else(|| error(ErrorCode::InvalidInput, "Job is not registered"))?;
        if record.status.is_terminal() {
            return Ok((
                WakeupOutcome::Terminal {
                    status: record.status,
                },
                events,
                signals,
            ));
        }
        if let Some(active) = state.active.get(key) {
            events.push(JobsEvent::DuplicateWakeupIgnored {
                job_key: key.clone(),
                run_epoch: active.token.run_epoch,
            });
            return Ok((
                WakeupOutcome::DuplicateIgnored {
                    run_epoch: active.token.run_epoch,
                },
                events,
                signals,
            ));
        }
        if record
            .deadline_at_ms
            .is_some_and(|deadline| now >= deadline)
        {
            self.mark_expired(
                &mut state,
                &mut record,
                BudgetExpiry::Deadline,
                now,
                &mut events,
            );
            return Ok((
                WakeupOutcome::ExpiredBudget {
                    reason: BudgetExpiry::Deadline,
                },
                events,
                signals,
            ));
        }
        if record.status == JobStatus::RetryBackoff
            && !record
                .constraints
                .retry
                .allows_another_attempt(record.attempt)
        {
            self.mark_expired(
                &mut state,
                &mut record,
                BudgetExpiry::AttemptsExhausted,
                now,
                &mut events,
            );
            return Ok((
                WakeupOutcome::ExpiredBudget {
                    reason: BudgetExpiry::AttemptsExhausted,
                },
                events,
                signals,
            ));
        }
        let unmet = evaluate_state(&state, &record, now);
        if !unmet.is_empty() {
            let previous = record.status;
            record.status = JobStatus::PendingConstraints;
            record.provider_token = None;
            record.unmet_constraints = unmet.clone();
            record.updated_at_ms = now;
            self.inner.persist_record(&record)?;
            state.records.insert(key.clone(), record);
            if previous != JobStatus::PendingConstraints {
                events.push(JobsEvent::JobStateChanged {
                    job_key: key.clone(),
                    previous,
                    status: JobStatus::PendingConstraints,
                });
            }
            return Ok((WakeupOutcome::Deferred { unmet }, events, signals));
        }
        let due = record
            .earliest_eligible_at_ms
            .max(record.next_attempt_at_ms);
        if now < due {
            return Ok((
                WakeupOutcome::NotDue {
                    remaining_ms: due.saturating_sub(now),
                },
                events,
                signals,
            ));
        }
        record.run_epoch = record.run_epoch.saturating_add(1);
        record.attempt = record.attempt.saturating_add(1);
        record.status = JobStatus::Running;
        record.provider_token = None;
        record.unmet_constraints.clear();
        record.updated_at_ms = now;
        let budget_deadline = now.saturating_add(record.constraints.execution_budget_ms);
        record.attempt_deadline_at_ms = Some(
            record
                .deadline_at_ms
                .map_or(budget_deadline, |deadline| deadline.min(budget_deadline)),
        );
        self.inner.persist_record(&record)?;
        state.records.insert(key.clone(), record.clone());
        let token = crate::FencingToken::new(
            key.clone(),
            record.run_epoch,
            record.attempt,
            record.idempotency_token.clone(),
        );
        state.active.insert(
            key.clone(),
            ActiveAttempt {
                token: token.clone(),
                preemption: None,
                preemption_requested_at_ms: None,
                final_checkpoint_saved: false,
            },
        );
        events.push(JobsEvent::AttemptStarted {
            token: token.clone(),
            started_at_ms: now,
        });
        Ok((WakeupOutcome::Started(token), events, signals))
    }

    fn preempt_locked(
        &self,
        state: &mut ServiceState,
        key: &JobKey,
        reason: PreemptReason,
        now: u64,
        events: &mut Vec<JobsEvent>,
        signals: &mut Vec<PreemptionSignal>,
    ) -> bool {
        let Some(active) = state.active.get_mut(key) else {
            return false;
        };
        if active.preemption.is_some() {
            return false;
        }
        active.preemption = Some(reason);
        active.preemption_requested_at_ms = Some(now);
        let token = active.token.clone();
        events.push(JobsEvent::PreemptionRequested {
            token: token.clone(),
            reason,
        });
        signals.push(PreemptionSignal {
            token,
            reason,
            requested_at_ms: now,
        });
        true
    }

    fn finalize_preemption_locked(
        &self,
        state: &mut ServiceState,
        key: &JobKey,
        now: u64,
        events: &mut Vec<JobsEvent>,
    ) -> bool {
        let Some(active) = state.active.remove(key) else {
            return false;
        };
        let Some(reason) = active.preemption else {
            state.active.insert(key.clone(), active);
            return false;
        };
        let Some(mut record) = state.records.get(key).cloned() else {
            return false;
        };
        record.run_epoch = record.run_epoch.saturating_add(1);
        record.attempt_deadline_at_ms = None;
        record.provider_token = None;
        record.updated_at_ms = now;
        let deadline_expired = record
            .deadline_at_ms
            .is_some_and(|deadline| now >= deadline);
        if deadline_expired {
            record.status = JobStatus::ExpiredBudget;
            record.unmet_constraints.clear();
            events.push(JobsEvent::JobExpiredBudget {
                job_key: key.clone(),
                reason: BudgetExpiry::Deadline,
            });
        } else if let Some(expiry) = reason.budget_expiry() {
            record.status = JobStatus::ExpiredBudget;
            record.unmet_constraints.clear();
            events.push(JobsEvent::JobExpiredBudget {
                job_key: key.clone(),
                reason: expiry,
            });
        } else {
            record.status = JobStatus::PendingConstraints;
            record.next_attempt_at_ms = now;
            record.unmet_constraints = evaluate_state(state, &record, now);
        }
        let _ = self.inner.persist_record(&record);
        state.records.insert(key.clone(), record);
        events.push(JobsEvent::PreemptionAcknowledged {
            token: active.token,
            reason,
            status: state
                .records
                .get(key)
                .map_or(JobStatus::PendingConstraints, |record| record.status),
        });
        true
    }

    fn mark_expired(
        &self,
        state: &mut ServiceState,
        record: &mut JobRecord,
        reason: BudgetExpiry,
        now: u64,
        events: &mut Vec<JobsEvent>,
    ) {
        record.status = JobStatus::ExpiredBudget;
        record.attempt_deadline_at_ms = None;
        record.provider_token = None;
        record.unmet_constraints.clear();
        record.updated_at_ms = now;
        let _ = self.inner.persist_record(record);
        state.records.insert(record.key.clone(), record.clone());
        events.push(JobsEvent::JobExpiredBudget {
            job_key: record.key.clone(),
            reason,
        });
    }

    fn advance_locked(&self, now: u64) -> Result<AdvanceOutcome, SystemError> {
        let mut outcome = AdvanceOutcome::default();
        let mut schedule_calls = Vec::new();
        {
            let mut state = lock(&self.inner.state);
            let keys = state.records.keys().cloned().collect::<Vec<_>>();
            for key in keys {
                let Some(mut record) = state.records.get(&key).cloned() else {
                    continue;
                };
                if record.status.is_terminal() {
                    continue;
                }
                let original_status = record.status;
                let mut changed = false;
                if record.status == JobStatus::Running {
                    let deadline_expired = record
                        .deadline_at_ms
                        .is_some_and(|deadline| now >= deadline);
                    let budget_expired = record
                        .attempt_deadline_at_ms
                        .is_some_and(|deadline| now >= deadline);
                    let lifecycle_blocked = matches!(
                        state.lifecycle,
                        AppPhase::Background | AppPhase::Suspended | AppPhase::Terminating
                    );
                    if deadline_expired || budget_expired {
                        self.preempt_locked(
                            &mut state,
                            &key,
                            PreemptReason::BudgetExpired,
                            now,
                            &mut outcome.events,
                            &mut outcome.signals,
                        );
                    } else if lifecycle_blocked {
                        self.preempt_locked(
                            &mut state,
                            &key,
                            PreemptReason::LifecycleSuspending,
                            now,
                            &mut outcome.events,
                            &mut outcome.signals,
                        );
                    } else {
                        let unmet = evaluate_state(&state, &record, now);
                        if !unmet.is_empty() {
                            let permission_lost = unmet.iter().any(|unmet| {
                                matches!(
                                    unmet,
                                    UnmetConstraint::PermissionDenied { .. }
                                        | UnmetConstraint::PermissionNotDetermined { .. }
                                )
                            });
                            self.preempt_locked(
                                &mut state,
                                &key,
                                if permission_lost {
                                    PreemptReason::PermissionRevoked
                                } else {
                                    PreemptReason::ConstraintsLost
                                },
                                now,
                                &mut outcome.events,
                                &mut outcome.signals,
                            );
                        }
                    }
                    let grace_elapsed = state.active.get(&key).is_some_and(|active| {
                        active.preemption.is_some()
                            && active.preemption_requested_at_ms.is_some_and(|requested| {
                                now >= requested.saturating_add(PREEMPTION_GRACE_MS)
                            })
                    });
                    if grace_elapsed
                        && self.finalize_preemption_locked(
                            &mut state,
                            &key,
                            now,
                            &mut outcome.events,
                        )
                    {
                        outcome.update.preempted = outcome.update.preempted.saturating_add(1);
                        outcome.update.changed = true;
                        if let Some(finalized) = state.records.get(&key).cloned()
                            && is_schedule_eligible(&finalized, now)
                        {
                            schedule_calls.push(finalized);
                        }
                    }
                    continue;
                }
                if record
                    .deadline_at_ms
                    .is_some_and(|deadline| now >= deadline)
                {
                    self.mark_expired(
                        &mut state,
                        &mut record,
                        BudgetExpiry::Deadline,
                        now,
                        &mut outcome.events,
                    );
                    outcome.update.expired = outcome.update.expired.saturating_add(1);
                    outcome.update.changed = true;
                    continue;
                }
                let unmet = evaluate_state(&state, &record, now);
                let reasons_changed = record.unmet_constraints != unmet;
                record.unmet_constraints = unmet.clone();
                let due = record
                    .earliest_eligible_at_ms
                    .max(record.next_attempt_at_ms);
                let was_scheduled = record.status == JobStatus::Scheduled;
                let provider_pending = record.provider_token.is_none();
                if !unmet.is_empty() {
                    let status_changed = record.status != JobStatus::PendingConstraints;
                    record.status = JobStatus::PendingConstraints;
                    record.provider_token = None;
                    changed = status_changed || reasons_changed;
                    if changed {
                        record.updated_at_ms = now;
                        state.records.insert(key.clone(), record.clone());
                        self.inner.persist_record(&record)?;
                        if status_changed {
                            outcome.events.push(JobsEvent::JobStateChanged {
                                job_key: key.clone(),
                                previous: original_status,
                                status: record.status,
                            });
                        }
                        outcome.update.pending = outcome.update.pending.saturating_add(1);
                    }
                } else if now >= due
                    && (!was_scheduled || provider_pending)
                    && matches!(
                        record.status,
                        JobStatus::PendingConstraints
                            | JobStatus::RetryBackoff
                            | JobStatus::Scheduled
                    )
                {
                    record.status = JobStatus::Scheduled;
                    record.unmet_constraints.clear();
                    record.provider_token = None;
                    record.updated_at_ms = now;
                    state.records.insert(key.clone(), record.clone());
                    self.inner.persist_record(&record)?;
                    schedule_calls.push(record);
                    outcome.update.scheduled = outcome.update.scheduled.saturating_add(1);
                } else if reasons_changed {
                    record.updated_at_ms = now;
                    state.records.insert(key.clone(), record.clone());
                    self.inner.persist_record(&record)?;
                }
                outcome.update.changed |= changed;
            }
        }
        for mut record in schedule_calls {
            self.try_schedule_provider(&mut record, now, &mut outcome.events);
            outcome.update.changed = true;
            let mut state = lock(&self.inner.state);
            if let Some(current) = state.records.get_mut(&record.key)
                && !current.status.is_terminal()
            {
                current.status = record.status;
                current.provider_token = record.provider_token;
                current.unmet_constraints = record.unmet_constraints.clone();
                self.inner.persist_record(current)?;
            }
        }
        Ok(outcome)
    }
}

#[derive(Default)]
struct AdvanceOutcome {
    update: JobsUpdate,
    events: Vec<JobsEvent>,
    signals: Vec<PreemptionSignal>,
}

fn lifecycle_blocks_jobs(phase: AppPhase) -> bool {
    matches!(
        phase,
        AppPhase::Background | AppPhase::Suspended | AppPhase::Terminating
    )
}

fn is_schedule_eligible(record: &JobRecord, now: u64) -> bool {
    !record.status.is_terminal()
        && record.unmet_constraints.is_empty()
        && matches!(
            record.status,
            JobStatus::PendingConstraints | JobStatus::RetryBackoff | JobStatus::Scheduled
        )
        && (record.status != JobStatus::Scheduled || record.provider_token.is_none())
        && now
            >= record
                .earliest_eligible_at_ms
                .max(record.next_attempt_at_ms)
}

fn evaluate_constraints(
    state: &Mutex<ServiceState>,
    record: &JobRecord,
    now: u64,
) -> Vec<UnmetConstraint> {
    evaluate_state(&lock(state), record, now)
}

fn evaluate_power(state: &ServiceState, record: &JobRecord, unmet: &mut Vec<UnmetConstraint>) {
    let power = &state.power;
    if snapshot_requires_pause(power) || state.power_policy.evaluate(power).is_paused() {
        unmet.push(UnmetConstraint::PowerPacingPaused);
    }
    if record.constraints.requires_charging {
        let external = power.power_source.is_observed() && power.power_source.kind.is_external();
        let charging = power.battery.is_observed()
            && matches!(
                power.battery.charging,
                gpui_power::ChargingState::Charging | gpui_power::ChargingState::Full
            );
        if !external && !charging {
            if power.power_source.is_observed() || power.battery.is_observed() {
                unmet.push(UnmetConstraint::RequiresCharging);
            } else {
                unmet.push(UnmetConstraint::PowerUnknown);
            }
        }
    }
    if record.constraints.disallow_battery_saver {
        if !power.battery.is_observed()
            || power.battery.low_power_mode == gpui_power::LowPowerMode::Unknown
        {
            unmet.push(UnmetConstraint::PowerUnknown);
        } else if power.battery.low_power_mode == gpui_power::LowPowerMode::Enabled {
            unmet.push(UnmetConstraint::BatterySaverEnabled);
        }
    }
    if record.constraints.disallow_critical_thermal {
        if !power.thermal.is_observed() || power.thermal.condition == ThermalCondition::Unknown {
            unmet.push(UnmetConstraint::PowerUnknown);
        } else if power.thermal.condition == ThermalCondition::Critical {
            unmet.push(UnmetConstraint::CriticalThermal);
        }
    }
}

fn evaluate_network(state: &ServiceState, record: &JobRecord, unmet: &mut Vec<UnmetConstraint>) {
    if record.constraints.network == JobNetworkRequirement::None {
        return;
    }
    let network = &state.network;
    if !network.source.is_observed() {
        unmet.push(UnmetConstraint::NetworkUnknown);
        return;
    }
    if !network.reachability.is_routable() {
        unmet.push(UnmetConstraint::NetworkUnavailable);
        return;
    }
    if record.constraints.deny_captive_portal && network.captive_portal.blocks_workloads() {
        unmet.push(UnmetConstraint::CaptivePortal);
    }
    if record.constraints.respect_data_saver && network.data_saver.is_constrained() {
        unmet.push(UnmetConstraint::DataSaverEnabled);
    }
    match record.constraints.network {
        JobNetworkRequirement::Unmetered => {
            if network.cost != ConnectionCost::Unmetered {
                unmet.push(UnmetConstraint::MeteredConnection);
            }
        }
        JobNetworkRequirement::NotRoaming => {
            if network.cost == ConnectionCost::CellularRoaming {
                unmet.push(UnmetConstraint::RoamingConnection);
            }
        }
        JobNetworkRequirement::Connected | JobNetworkRequirement::None => {}
    }
}

fn evaluate_state(state: &ServiceState, record: &JobRecord, now: u64) -> Vec<UnmetConstraint> {
    let mut unmet = Vec::new();
    if record.status != JobStatus::Running {
        if now < record.earliest_eligible_at_ms {
            unmet.push(UnmetConstraint::InitialDelay {
                remaining_ms: record.earliest_eligible_at_ms.saturating_sub(now),
            });
        }
        if record.status == JobStatus::RetryBackoff && now < record.next_attempt_at_ms {
            unmet.push(UnmetConstraint::RetryBackoff {
                remaining_ms: record.next_attempt_at_ms.saturating_sub(now),
            });
        }
    }
    if lifecycle_blocks_jobs(state.lifecycle) {
        unmet.push(UnmetConstraint::LifecycleSuspended);
    }
    if record.constraints.requires_device_idle && !state.device_idle {
        unmet.push(UnmetConstraint::DeviceNotIdle);
    }
    if record.constraints.requires_storage_not_low && state.storage_low {
        unmet.push(UnmetConstraint::StorageLow);
    }
    if let Some(permission) = &record.constraints.required_permission {
        match state.permissions.get(permission.label()) {
            Some(status) if status.allows_use() => {}
            Some(status)
                if status.requires_prompt() || *status == PermissionStatus::NotDetermined =>
            {
                unmet.push(UnmetConstraint::PermissionNotDetermined {
                    permission: permission.clone(),
                });
            }
            Some(_) => unmet.push(UnmetConstraint::PermissionDenied {
                permission: permission.clone(),
            }),
            None => unmet.push(UnmetConstraint::PermissionNotDetermined {
                permission: permission.clone(),
            }),
        }
    }
    evaluate_power(state, record, &mut unmet);
    evaluate_network(state, record, &mut unmet);
    unmet
}

#[cfg(feature = "test-support")]
impl JobsService {
    /// Test-only hook that poisons the internal state mutex. Every consumer,
    /// including `Drop`, recovers the guard with `PoisonError::into_inner`.
    #[doc(hidden)]
    pub fn poison_state_lock_for_test(&self) {
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _guard = lock(&self.inner.state);
            panic!("intentional jobs state poison");
        }));
    }
}

impl fmt::Debug for JobsService {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let state = lock(&self.inner.state);
        formatter
            .debug_struct("JobsService")
            .field("jobs", &state.records.len())
            .field("active", &state.active.len())
            .field("loaded", &state.loaded)
            .finish()
    }
}
