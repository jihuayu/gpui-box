//! Lifecycle and system-preference observation plus versioned UI restoration.
//!
//! The default build depends only on [`gpui_system`] and the Rust standard
//! library. Enable the `gpui` feature for the optional GPUI adapter.

mod app;
mod events;
mod foreground;
mod layout;
mod preferences;
mod restore;
mod service;

pub use app::*;
pub use events::{
    LIFECYCLE_CAPACITY, LifecycleEvent, NetworkStatus, PREFERENCES_CAPACITY, REVALIDATION_CAPACITY,
    RevalidationEvent, RevalidationScope, RevalidationSender, RevalidationSubscription,
    drain_ready, revalidation_channel,
};
pub use foreground::*;
pub use gpui_system::{Event, EventEnvelope, EventSender, EventSubscription, SurfaceId};
pub use layout::*;
pub use preferences::*;
pub use restore::*;
pub use service::*;

#[cfg(feature = "gpui")]
pub mod gpui_adapter;
