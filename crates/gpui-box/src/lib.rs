//! Shared entry point for the gpui-box distribution.
//!
//! Enable `desktop` for the upstream desktop backends or `android` for the
//! distribution's SurfaceView backend and its Kotlin/JNI host.

/// The official Zed revision used by the checked-in GPUI source.
pub const UPSTREAM_REVISION: &str = "f8cfc7ed9e57503def9479138894f3139f5400cc";

/// The gpui-box package version.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Typed system-service contracts, independent of native provider dependencies.
#[cfg(feature = "system")]
pub use gpui_system as system;

/// Permission status, contextual requests and settings recovery.
#[cfg(feature = "permissions")]
pub use gpui_permissions as permissions;

/// GPUI types from the distribution's coordinated source tree.
#[cfg(any(
    all(
        feature = "desktop",
        any(target_os = "windows", target_os = "macos", target_os = "linux")
    ),
    all(feature = "android", target_os = "android")
))]
pub use gpui;

/// Android platform and embedded-runtime entry points for a Kotlin host.
#[cfg(all(feature = "android", target_os = "android"))]
pub use gpui_android as android;

/// Creates a GPUI application using the current desktop platform backend.
#[cfg(all(
    feature = "desktop",
    any(target_os = "windows", target_os = "macos", target_os = "linux")
))]
pub use gpui_platform::application;

/// Private application directories and recoverable settings.
#[cfg(feature = "app-data")]
pub use gpui_app_data as app_data;

/// Account-scoped secure credentials, with explicit native provider policies.
#[cfg(feature = "secrets")]
pub use gpui_secrets as secrets;
