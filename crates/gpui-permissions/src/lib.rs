//! Permission status, contextual requests and settings recovery.
//!
//! The default build depends only on the standard library and the shared
//! `gpui-system` contracts. Providers are UI-local and explicit; this crate
//! installs no native provider and never synthesizes a platform dialog.
//! File-picker and share-sheet grants remain resource-scoped under #23 and are
//! intentionally absent from [`PermissionKind`].

mod fake;
mod model;
mod provider;
mod service;

pub use fake::*;
pub use model::*;
pub use provider::*;
pub use service::*;

pub use gpui_system::{
    Availability, CancelReason, Capability, Completion, Configuration, Constraint, ErrorCode,
    Event, EventEnvelope, EventSubscription, Evidence, OperationContext, PermissionState, Platform,
    ProviderManifest, RecoveryAction, Request, RetryAdvice, ServiceResult, Support, SurfaceId,
    SystemError, SystemServices, UiContext, UiOwner,
};

#[cfg(feature = "gpui")]
pub mod gpui_adapter;
