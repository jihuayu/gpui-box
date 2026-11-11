//! The explicit OS-scheduling provider boundary, plus deterministic fake and
//! unavailable implementations.
//!
//! This boundary is deliberately separate from the bounded in-process pools:
//! a provider schedules work that survives the process, while an in-process
//! task can never outlive its owning executor.

use crate::{
    IdempotencyToken, JobCadence, JobKey, JobNetworkRequirement, JobsOperation, SchedulingGuarantee,
};
use gpui_system::{
    Availability, Capability, Configuration, ErrorCode, Evidence, PermissionState,
    ProviderManifest, Support, SystemError,
};
use std::{
    any::Any,
    collections::BTreeMap,
    sync::{Arc, Mutex, Weak},
};

fn unavailable() -> SystemError {
    SystemError::new(
        ErrorCode::Unavailable,
        "No native background job provider is installed",
    )
}

/// One advisory request to the host's durable scheduler.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JobScheduleRequest {
    pub key: JobKey,
    pub idempotency_token: IdempotencyToken,
    pub guarantee: SchedulingGuarantee,
    /// Advisory lower bound in host monotonic milliseconds.
    pub earliest_eligible_after_ms: u64,
    pub cadence: JobCadence,
    pub execution_budget_ms: u64,
    pub deadline_at_ms: Option<u64>,
    pub network: JobNetworkRequirement,
    pub requires_charging: bool,
    pub requires_device_idle: bool,
    pub requires_storage_not_low: bool,
}

/// The provider's opaque admission receipt. It is not an execution promise.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct JobScheduleHandle {
    pub provider_token: u64,
    pub scheduled_at_ms: u64,
    pub advisory: bool,
}

impl JobScheduleHandle {
    pub const fn new(provider_token: u64, scheduled_at_ms: u64) -> Self {
        Self {
            provider_token,
            scheduled_at_ms,
            advisory: true,
        }
    }
}

/// Provider-to-service pushes. Delivered synchronously and never emitted while
/// the provider holds its own state lock.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JobProviderNotice {
    /// The OS admitted a wakeup for this job. It may be a duplicate.
    Wakeup { key: JobKey },
    /// The OS revoked or exhausted this attempt's execution slice.
    TimeSliceExpired { key: JobKey, run_epoch: u64 },
    /// The native provider became unavailable for this job.
    ProviderUnavailable { key: JobKey },
}

/// The service's weak provider listener.
pub trait JobProviderListener: Send + Sync + 'static {
    fn on_job_provider_notice(&self, notice: JobProviderNotice);
}

/// Explicit native/OS scheduling boundary.
///
/// Every method is safe to call from a background thread. Capability snapshots
/// are advisory and never an execution guarantee; the service re-evaluates all
/// constraints at dispatch time.
pub trait JobProvider: Send + Sync + 'static {
    fn capability(&self, operation: JobsOperation) -> Capability<JobsOperation>;

    fn capabilities(&self) -> Vec<Capability<JobsOperation>> {
        JobsOperation::ALL
            .into_iter()
            .map(|operation| self.capability(operation))
            .collect()
    }

    fn schedule(&self, request: &JobScheduleRequest) -> Result<JobScheduleHandle, SystemError>;

    fn cancel(&self, key: &JobKey) -> Result<(), SystemError>;

    fn attach_listener(&self, listener: Weak<dyn JobProviderListener>);

    fn as_any(&self) -> &dyn Any;
}

/// Conservative manifest: this contract crate installs no native provider.
pub const PROVIDER_MANIFEST: [ProviderManifest; 5] = gpui_system::PROVIDER_MANIFEST;

/// Contract-only provider used when no host adapter is installed.
#[derive(Debug, Clone, Copy, Default)]
pub struct UnavailableJobProvider;

impl UnavailableJobProvider {
    pub const fn new() -> Self {
        Self
    }
}

impl JobProvider for UnavailableJobProvider {
    fn capability(&self, operation: JobsOperation) -> Capability<JobsOperation> {
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

    fn schedule(&self, _request: &JobScheduleRequest) -> Result<JobScheduleHandle, SystemError> {
        Err(unavailable())
    }

    fn cancel(&self, _key: &JobKey) -> Result<(), SystemError> {
        Err(unavailable())
    }

    fn attach_listener(&self, _listener: Weak<dyn JobProviderListener>) {}

    fn as_any(&self) -> &dyn Any {
        self
    }
}

struct FakeState {
    schedule_requests: Vec<JobScheduleRequest>,
    cancel_calls: Vec<JobKey>,
    listeners: Vec<Weak<dyn JobProviderListener>>,
    capabilities: BTreeMap<JobsOperation, Capability<JobsOperation>>,
    unavailable: bool,
    fail_next_schedule: bool,
    next_token: u64,
}

/// Deterministic provider for OS schedule requests, cancellations,
/// opportunistic and duplicate wakeups, time-slice expiry and unavailability.
#[derive(Clone)]
pub struct FakeJobProvider {
    state: Arc<Mutex<FakeState>>,
}

impl Default for FakeJobProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl FakeJobProvider {
    pub fn new() -> Self {
        let capabilities = JobsOperation::ALL
            .into_iter()
            .map(|operation| {
                (
                    operation,
                    Capability {
                        operation,
                        support: Support::Native,
                        availability: Availability::Available,
                        configuration: Configuration::Ready,
                        permission: PermissionState::NotRequired,
                        evidence: Evidence::ContractOnly,
                        constraints: Vec::new(),
                        observed_revision: 1,
                    },
                )
            })
            .collect();
        Self {
            state: Arc::new(Mutex::new(FakeState {
                schedule_requests: Vec::new(),
                cancel_calls: Vec::new(),
                listeners: Vec::new(),
                capabilities,
                unavailable: false,
                fail_next_schedule: false,
                next_token: 1,
            })),
        }
    }

    pub fn set_unavailable(&self, unavailable: bool) {
        lock(&self.state).unavailable = unavailable;
    }

    pub fn fail_next_schedule(&self) {
        lock(&self.state).fail_next_schedule = true;
    }

    pub fn schedule_requests(&self) -> Vec<JobScheduleRequest> {
        lock(&self.state).schedule_requests.clone()
    }

    pub fn schedule_request_count(&self) -> usize {
        lock(&self.state).schedule_requests.len()
    }

    pub fn latest_schedule_request(&self) -> Option<JobScheduleRequest> {
        lock(&self.state).schedule_requests.last().cloned()
    }

    pub fn cancel_calls(&self) -> Vec<JobKey> {
        lock(&self.state).cancel_calls.clone()
    }

    pub fn cancel_call_count(&self) -> usize {
        lock(&self.state).cancel_calls.len()
    }

    pub fn set_capability(&self, operation: JobsOperation, capability: Capability<JobsOperation>) {
        lock(&self.state).capabilities.insert(operation, capability);
    }

    pub fn capability(&self, operation: JobsOperation) -> Capability<JobsOperation> {
        lock(&self.state)
            .capabilities
            .get(&operation)
            .cloned()
            .expect("the fake provider contains every jobs operation")
    }

    pub fn listener_count(&self) -> usize {
        let mut state = lock(&self.state);
        state
            .listeners
            .retain(|listener| listener.strong_count() > 0);
        state.listeners.len()
    }

    /// Simulate an OS wakeup. Calling this while an attempt is `Running`
    /// exercises duplicate-wakeup coalescing.
    pub fn fire_wakeup(&self, key: &JobKey) {
        self.notify(JobProviderNotice::Wakeup { key: key.clone() });
    }

    pub fn fire_wakeup_twice(&self, key: &JobKey) {
        self.fire_wakeup(key);
        self.fire_wakeup(key);
    }

    pub fn expire_time_slice(&self, key: &JobKey, run_epoch: u64) {
        self.notify(JobProviderNotice::TimeSliceExpired {
            key: key.clone(),
            run_epoch,
        });
    }

    pub fn report_unavailable(&self, key: &JobKey) {
        self.notify(JobProviderNotice::ProviderUnavailable { key: key.clone() });
    }

    fn notify(&self, notice: JobProviderNotice) {
        let listeners = {
            let mut state = lock(&self.state);
            state
                .listeners
                .retain(|listener| listener.strong_count() > 0);
            state
                .listeners
                .iter()
                .filter_map(Weak::upgrade)
                .collect::<Vec<_>>()
        };
        for listener in listeners {
            listener.on_job_provider_notice(notice.clone());
        }
    }
}

impl JobProvider for FakeJobProvider {
    fn capability(&self, operation: JobsOperation) -> Capability<JobsOperation> {
        Self::capability(self, operation)
    }

    fn schedule(&self, request: &JobScheduleRequest) -> Result<JobScheduleHandle, SystemError> {
        let mut state = lock(&self.state);
        if state.unavailable {
            return Err(unavailable());
        }
        if state.fail_next_schedule {
            state.fail_next_schedule = false;
            return Err(unavailable());
        }
        state.schedule_requests.push(request.clone());
        let token = state.next_token;
        state.next_token = state.next_token.saturating_add(1);
        Ok(JobScheduleHandle::new(
            token,
            request.earliest_eligible_after_ms,
        ))
    }

    fn cancel(&self, key: &JobKey) -> Result<(), SystemError> {
        let mut state = lock(&self.state);
        if state.unavailable {
            return Err(unavailable());
        }
        state.cancel_calls.push(key.clone());
        Ok(())
    }

    fn attach_listener(&self, listener: Weak<dyn JobProviderListener>) {
        let mut state = lock(&self.state);
        state.listeners.retain(|current| current.strong_count() > 0);
        state.listeners.push(listener);
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// True when the platform policy exposes a durable scheduling mechanism.
/// True only when a real durable scheduling provider is installed for the
/// target. This contract crate installs [`UnavailableJobProvider`] everywhere,
/// so the answer is always `false`; the platform matrix describes intended host
/// mechanisms, not compiled or qualified providers.
pub fn durable_scheduling_available(_platform: crate::PlatformTarget) -> bool {
    false
}

fn lock<T>(value: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    value
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}
