//! AccessKit semantic inventory and assistive-technology acceptance.
//!
//! The crate has two deliberately separate evidence channels:
//!
//! - [`AutomationEvidence`] records programmatic tree/action/selection/audit
//!   checks. It is never a substitute for a real screen-reader session.
//! - [`AssistiveReaderSessionEvidence`] records Narrator, NVDA, VoiceOver, Orca
//!   or TalkBack sessions with reader/version, OS/device/channel and scenario
//!   coverage.
//!
//! The service, semantic tracker, modal/virtual-list audits and bounded live
//! queue are platform-neutral. Native bridges remain owned by their adapter
//! issues; enable the optional `gpui` feature only for the GPUI adapter.

mod audit;
mod error;
mod evidence;
mod live;
mod platform;
mod provider;
mod scene;
mod service;
mod text;
mod tracker;

pub use audit::*;
pub use error::*;
pub use evidence::*;
pub use live::*;
pub use platform::*;
pub use provider::*;
pub use scene::*;
pub use service::*;
pub use text::*;
pub use tracker::*;

#[cfg(feature = "gpui")]
pub mod gpui_adapter;
