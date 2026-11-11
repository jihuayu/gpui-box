//! Immediate local notifications with replacement, withdrawal and activation routing.
//!
//! Submission receipts, observed display and user activation are deliberately
//! separate evidence. The default build has no native provider and installs
//! nothing. Durable activation metadata is a background primitive; the UI-local
//! [`NotificationService`] is the only entry point for immediate operations.
//!
//! Enable the `gpui` feature for the optional adapter over the coordinated
//! workspace GPUI package.

mod activation_store;
mod fake;
mod model;
mod provider;
mod service;

pub use activation_store::*;
pub use fake::*;
pub use model::*;
pub use provider::*;
pub use service::*;

pub use gpui_permissions::{
    PermissionKind, PermissionOperation, PermissionOutcome, PermissionRequest, PermissionService,
    PermissionSnapshot, PermissionStatus, SettingsTarget,
};
pub use gpui_system::{
    Availability, CancelReason, Capability, Completion, Configuration, Constraint, ErrorCode,
    Event, EventEnvelope, EventSender, EventSubscription, Evidence, OperationContext,
    PermissionState, Platform, ProviderManifest, RecoveryAction, Request, RetryAdvice,
    ServiceResult, Support, SurfaceId, SystemError, SystemServices, UiContext, UiOwner,
};

#[cfg(feature = "gpui")]
pub mod gpui_adapter;

fn error(code: ErrorCode, message: &'static str) -> SystemError {
    SystemError::new(code, message)
}
