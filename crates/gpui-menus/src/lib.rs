//! Typed command contributions, native menu projection and explicit execution
//! domains for `gpui-box`.
//!
//! `gpui-menus` owns command identity and descriptors, reactive context
//! expressions, scoped contribution leases, lazy materialization, the bounded
//! background worker pool, and deterministic projections for five trigger
//! surfaces: native menus, keybindings, the command palette, component buttons
//! and system/activation actions.
//!
//! The default build has no GPUI dependency. The optional `gpui` feature adds
//! the `App` global adapter and entity-owned contribution scope.
//!
//! See `docs/S_MENUS.md` for execution-domain, lease, concurrency and six-target
//! qualification contracts.

mod button;
mod command;
mod context;
mod handler;
mod invoke;
mod keybinding;
mod menu;
mod palette;
mod platform;
mod provider;
mod resolve;
mod scope;
mod service;
mod system_action;
mod task;
mod worker;

pub use button::*;
pub use command::*;
pub use context::*;
pub use handler::{
    BackgroundCommandCompletion, BackgroundCommandContext, BackgroundCommandOutcome,
    ContributionBundle, RegisteredCommand, UiCommandCompletion, UiCommandContext, UiCommandOutcome,
    empty_parameters,
};
pub use invoke::{TypedCommandRequest, is_terminal};
pub use keybinding::*;
pub use menu::*;
pub use palette::*;
pub use platform::*;
pub use provider::*;
pub use resolve::{context_menu_context, invocation_context};
pub use scope::*;
pub use service::*;
pub use system_action::*;
pub use task::{MENU_EVENT_CAPACITY, MenuCommandEvent};
pub use worker::WorkerPoolConfig;

pub use gpui_activation::{
    ActivationEvent, ActivationId, ActivationPayload, ActivationSource, ActivationTarget,
    RouteRegistry,
};
pub use gpui_permissions::{
    FakePermissionProvider, PermissionKind, PermissionProvider, PermissionService, PermissionStatus,
};
pub use gpui_system::{
    Availability, CancelReason, CancellationToken, Capability, Completion, Configuration,
    Constraint, ErrorCode, Event, EventEnvelope, EventSubscription, Evidence, LogicalRect,
    OperationContext, PermissionState, Platform, RecoveryAction, Request, RequestId, ServiceResult,
    Support, SurfaceId, SystemError, SystemServices, UiContext, UiOwner,
};

/// Compatibility alias for hosts that prefer the shorter cancellation name.
pub type CancelToken = gpui_system::CancellationToken;

/// The official Zed revision used by the checked-in GPUI source.
pub const UPSTREAM_GPUI_REVISION: &str = "f8cfc7ed9e57503def9479138894f3139f5400cc";

/// The gpui-menus package version.
pub const PACKAGE_VERSION: &str = env!("CARGO_PKG_VERSION");

#[cfg(feature = "gpui")]
pub mod gpui_adapter;
