//! Shared entry point for the gpui-box distribution.
//!
//! Enable `desktop` to use the vendored GPUI desktop backends. Platform-specific
//! mobile hosts are integrated in their respective implementation milestones.

/// The official Zed revision used by the checked-in GPUI source.
pub const UPSTREAM_REVISION: &str = "f8cfc7ed9e57503def9479138894f3139f5400cc";

/// The gpui-box package version.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// GPUI types from the distribution's coordinated source tree.
#[cfg(all(
    feature = "desktop",
    any(target_os = "windows", target_os = "macos", target_os = "linux")
))]
pub use gpui;

/// Creates a GPUI application using the current desktop platform backend.
#[cfg(all(
    feature = "desktop",
    any(target_os = "windows", target_os = "macos", target_os = "linux")
))]
pub use gpui_platform::application;
