//! Taskbar, Dock, badge, attention and notification task-presentation adapters
//! for `gpui-box`.
//!
//! The default build has no GPUI dependency and installs no native provider.
//! Every presentation is an explicit outcome: unavailable surfaces and denied
//! permissions are reported separately from the business completion of a task.
//!
//! See `docs/S_TASK_PRESENTATION.md`.

mod action;
mod aggregator;
mod model;
mod notification;
mod platform;
mod provider;
mod service;

pub use action::*;
pub use aggregator::*;
pub use model::*;
pub use notification::*;
pub use platform::*;
pub use provider::*;
pub use service::*;

/// The coordinated menu/command contracts used by task action links.
pub use gpui_menus as menus;
/// The coordinated immediate-notification contracts used by the mirror.
pub use gpui_notifications as notifications;

pub use gpui_system::{
    Availability, CancelReason, Capability, Completion, Configuration, Constraint, ErrorCode,
    Event, EventEnvelope, EventSender, EventSubscription, Evidence, OperationContext,
    PermissionState, Platform, ProviderManifest, RecoveryAction, Request, RetryAdvice,
    ServiceResult, Support, SurfaceId, SystemError, SystemServices, UiContext, UiOwner,
};

/// The coordinated upstream GPUI revision used by this distribution.
pub const UPSTREAM_GPUI_REVISION: &str = "f8cfc7ed9e57503def9479138894f3139f5400cc";

/// The gpui-task-presentation package version.
pub const PACKAGE_VERSION: &str = env!("CARGO_PKG_VERSION");

#[cfg(feature = "gpui")]
pub mod gpui_adapter;
