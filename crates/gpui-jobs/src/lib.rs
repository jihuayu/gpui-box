//! Persistent background jobs, constraint gating, checkpoint recovery and
//! bounded in-process CPU/I/O pools.
//!
//! The default build has no GPUI dependency. Every row in the platform matrix is
//! [`Evidence::ContractOnly`]; no native OS scheduling provider is installed by
//! this crate. Durable jobs use private [`gpui_app_data`] storage and require a
//! background [`OperationContext`] (no UI attachment).
//!
//! Two execution models are deliberately distinct:
//!
//! - [`JobProvider`] schedules durable, cross-process work through an OS API.
//!   It is opportunistic and its [`ScheduleReceipt`] never promises a start time.
//! - [`InProcessExecutor`] runs bounded CPU or I/O work inside this process.
//!   It has independent concurrency and queue bounds and never runs UI work.
//!
//! The optional `gpui` feature adds [`gpui_adapter::JobsModel`],
//! [`gpui_adapter::SurfaceJobsScope`] and `SystemServices` registration. See
//! `docs/S_BACKGROUND_JOBS.md`.

#![forbid(unsafe_code)]

mod clock;
mod executor;
mod model;
mod platform;
mod provider;
mod service;
mod store;

pub use clock::{JobsClock, ManualClock, SystemJobsClock};
pub use executor::{
    DEFAULT_CPU_CONCURRENCY, DEFAULT_IO_CONCURRENCY, DEFAULT_POOL_QUEUE_DEPTH, InProcessExecutor,
    InProcessRunReport, InProcessTaskHandle, InProcessTaskId, InProcessWorkClass,
    RunningInProcessTask, SurfaceDisposalReport, SurfaceTaskScope, TaskCompletion,
};
pub use model::{
    DEFAULT_JOBS_EVENT_CAPACITY, ExistingJobPolicy, FencingToken, IdempotencyToken, JobCadence,
    JobCheckpoint, JobCheckpointSummary, JobConstraints, JobFailureReason, JobKey, JobNamespace,
    JobNetworkRequirement, JobPermissionRequirement, JobProgress, JobRecord, JobStage, JobStatus,
    MAX_CHECKPOINT_BYTES, MAX_DEADLINE_MS, MAX_INITIAL_DELAY_MS, MAX_JOB_IDENTIFIER_BYTES,
    MAX_RETRY_ATTEMPTS, MAX_STAGE_BYTES, MIN_PERIODIC_INTERVAL_MS, PREEMPTION_GRACE_MS,
    RegistrationDisposition, RetryBackoffPolicy, ScheduleReceipt, SchedulingGuarantee,
    UnmetConstraint, stale_fencing_error,
};
pub use platform::{
    JobsOperation, PlatformOperationSupport, PlatformTarget, WebBackendMaturity, WebJobsPolicy,
    operation_support, platform_operations, platform_support_matrix, web_capability_policy,
};
pub use provider::{
    FakeJobProvider, JobProvider, JobProviderListener, JobProviderNotice, JobScheduleHandle,
    JobScheduleRequest, PROVIDER_MANIFEST, UnavailableJobProvider, durable_scheduling_available,
};
pub use service::{
    BudgetExpiry, HeartbeatReceipt, JobPreemptionListener, JobsEvent, JobsService, JobsUpdate,
    PreemptReason, PreemptionSignal, PreemptionSubscription, ReconciliationReport, WakeupOutcome,
};
pub use store::{
    JOBS_FORMAT_VERSION, JOBS_NAMESPACE, JobsStorageFault, JobsStorageFaultInjection,
    JobsStorageHook, NoopJobsStorageHook, QUARANTINE_DIR,
};

pub use gpui_app_data::{AppData, DataLocation, DirectoryPolicy, DirectoryRoots};
pub use gpui_lifecycle::{AppPhase, LifecycleState};
pub use gpui_network::{
    CaptivePortalStatus, ConnectionCost, DataSaverMode, InterfaceReachability, NetworkSnapshot,
    SignalSource as NetworkSignalSource, TransportKind,
};
pub use gpui_permissions::{PermissionKind, PermissionStatus};
pub use gpui_power::{
    BatteryStatus, ChargingState, LowPowerMode, PowerBudgetPolicy, PowerSnapshot, PowerSource,
    PowerSourceStatus, SignalSource as PowerSignalSource, ThermalCondition, ThermalStatus,
    WorkPacingRecommendation,
};
pub use gpui_system::{
    Availability, CancelReason, Capability, Completion, Configuration, Constraint, ErrorCode,
    Event, EventEnvelope, EventSubscription, Evidence, OperationContext, PermissionState, Platform,
    ProviderManifest, RecoveryAction, Request, RetryAdvice, ServiceResult, Support, SurfaceId,
    SystemError, SystemServices, UiContext, UiOwner,
};

/// The coordinated upstream GPUI revision used by this distribution.
pub const UPSTREAM_GPUI_REVISION: &str = "f8cfc7ed9e57503def9479138894f3139f5400cc";

/// The gpui-jobs package version.
pub const PACKAGE_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Which executor owns a call.
///
/// `gpui-system` models this today through [`OperationContext::ui`]: a UI
/// attachment means `Ui`; no attachment means `Background`. `Renderer` is an
/// explicit class used by callers that drive render-thread work; it is never
/// permitted for durable storage or blocking in-process pools. A renderer
/// cannot be inferred from an [`OperationContext`], so callers that represent
/// one declare it explicitly.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
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

    /// Durable storage and blocking in-process work require a background
    /// context with no UI attachment. UI and renderer ownership fail before any
    /// side effect.
    pub fn require_background(self, context: &OperationContext) -> Result<(), SystemError> {
        if !self.is_background() || context.ui.is_some() {
            return Err(SystemError::new(
                ErrorCode::InvalidInput,
                "Durable and blocking jobs require a background execution owner",
            ));
        }
        context.check_at(std::time::Instant::now())?;
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

#[cfg(feature = "gpui")]
pub mod gpui_adapter;
