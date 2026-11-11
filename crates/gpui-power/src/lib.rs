//! Power, session and thermal observations with lifecycle-bound
//! wake-prevention leases.
//!
//! The default build has no GPUI dependency. Every observation carries a
//! [`SignalSource`], and an inhibition is reported as effective only after the
//! provider granted a native token. Refused or revoked grants never become an
//! [`EffectiveWakeStatus`].
//!
//! The optional `gpui` feature adds the `App` global / `SystemServices` adapter
//! and the `gpui_adapter::PowerSurfaceScope`. See `docs/S_POWER.md`.

mod lease;
mod model;
mod pacing;
mod platform;
mod provider;
mod service;

pub use lease::{
    EffectiveWakeStatus, IneffectiveInhibitionReason, MAX_WAKE_REASON_BYTES,
    ProviderInhibitionGrant, ReleasePolicy, WakeInhibitionKind, WakeLeaseId, WakeLeaseRequest,
    WakeLeaseScope, WakeLeaseState, WakeReason, WakeRevocationReason,
};
pub use model::{
    BatteryStatus, ChargingState, LowPowerMode, MemoryCondition, MemoryObservation, MemoryStatus,
    ObservedValue, PowerSnapshot, PowerSource, PowerSourceStatus, SessionLockObservation,
    SessionLockState, SessionLockStatus, SignalSource, SleepResumeObservation, SleepResumeState,
    SleepResumeStatus, ThermalCondition, ThermalObservation, ThermalStatus,
};
pub use pacing::{
    PacingReason, PacingTier, PowerBudgetPolicy, WorkCadence, WorkPacingRecommendation,
    snapshot_requires_pause,
};
pub use platform::{
    PlatformOperationSupport, PlatformTarget, WebBackendMaturity, WebWakeLockPolicy,
    operation_support, platform_operations, platform_support_matrix, web_capability_policy,
};
pub use provider::{
    FakePowerProvider, PROVIDER_MANIFEST, PowerInhibitionRequest, PowerOperation, PowerProvider,
    PowerProviderListener, PowerProviderNotice, UnavailablePowerProvider, WAKE_LOCK_PERMISSION,
};
pub use service::{DEFAULT_POWER_EVENT_CAPACITY, PowerEvent, PowerService, WakePreventionLease};

pub use gpui_system::{
    Availability, CancelReason, Capability, Completion, Configuration, Constraint, ErrorCode,
    Event, EventEnvelope, EventSubscription, Evidence, OperationContext, PermissionState, Platform,
    ProviderManifest, RecoveryAction, Request, RetryAdvice, ServiceResult, Support, SurfaceId,
    SystemError, SystemServices, UiContext, UiOwner,
};

/// The coordinated upstream GPUI revision used by this distribution.
pub const UPSTREAM_GPUI_REVISION: &str = "f8cfc7ed9e57503def9479138894f3139f5400cc";

/// The gpui-power package version.
pub const PACKAGE_VERSION: &str = env!("CARGO_PKG_VERSION");

#[cfg(feature = "gpui")]
pub mod gpui_adapter;
