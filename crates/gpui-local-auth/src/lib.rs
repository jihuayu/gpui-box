//! Local user verification, protected-key signing and protected-secret release.
//!
//! The crate separates three operations that are easy to confuse:
//!
//! 1. non-interactive readiness queries;
//! 2. interactive local user presence (`VerifyUser`);
//! 3. authentication-bound key/secret access (`UnlockProtectedSecret`,
//!    `SignChallenge`) that composes with `gpui-secrets`.
//!
//! Remote account, Passkey and WebAuthn authentication are explicitly out of
//! scope. The default build has no GUI dependencies and installs no native
//! provider. See the repository's `docs/SYSTEM_LOCAL_AUTH.md`.

mod fake;
mod flow;
mod model;
mod provider;
mod service;
mod session;

pub use fake::*;
pub use flow::*;
pub use model::*;
pub use provider::*;
pub use service::*;
pub use session::*;

pub use gpui_lifecycle::{
    AppPhase, LifecycleService, LifecycleState, SessionLockState, SurfaceSnapshot,
    SurfaceVisibility,
};
pub use gpui_secrets::SecretBytes;
pub use gpui_system::{
    Availability, CancelReason, Capability, Completion, Configuration, Constraint, DiagnosticPhase,
    ErrorCode, Event, EventEnvelope, EventSubscription, Evidence, OperationContext,
    PermissionState, Platform, ProviderManifest, RecoveryAction, Request, RetryAdvice,
    ServiceResult, Support, SurfaceId, SystemError, SystemServices, UiContext, UiOwner,
};

#[cfg(feature = "gpui")]
pub mod gpui_adapter;
