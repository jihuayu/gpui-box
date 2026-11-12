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

/// Lifecycle, preference, restoration and foreground-use contracts.
#[cfg(feature = "lifecycle")]
pub use gpui_lifecycle as lifecycle;

/// Permission status, contextual requests and settings recovery.
#[cfg(feature = "permissions")]
pub use gpui_permissions as permissions;

/// AccessKit semantic inventory, scene audits and assistive-technology evidence.
#[cfg(feature = "accessibility")]
pub use gpui_accessibility as accessibility;

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

/// Validated URL opening and durable activation routing.
#[cfg(feature = "activation")]
pub use gpui_activation as activation;

/// Immediate local notifications with replacement, withdrawal and activation routing.
#[cfg(feature = "notifications")]
pub use gpui_notifications as notifications;

/// Account-scoped secure credentials, with explicit native provider policies.
#[cfg(feature = "secrets")]
pub use gpui_secrets as secrets;

/// Foreground, user-initiated plain-text clipboard operations.
#[cfg(feature = "clipboard")]
pub use gpui_clipboard as clipboard;

/// Authorized file picking, opaque resources and safe save/export.
#[cfg(feature = "files")]
pub use gpui_files as files;

/// Explicit native text/link sharing and authorized export leases.
#[cfg(feature = "sharing")]
pub use gpui_sharing as sharing;

/// Local user verification, protected-key signing and protected-secret release.
#[cfg(feature = "local-auth")]
pub use gpui_local_auth as local_auth;

/// Browser OAuth 2.0/OIDC, PKCE, account token vault and passkey request flows.
#[cfg(feature = "auth")]
pub use gpui_auth as auth;

/// Signed update metadata and generation-correlated delivery contracts.
#[cfg(feature = "updates")]
pub use gpui_updates as updates;

/// Structured diagnostics, crash records, bounded retention and recoverable export.
#[cfg(feature = "diagnostics")]
pub use gpui_diagnostics as diagnostics;

/// Typed command contributions and native menu projection.
#[cfg(feature = "menus")]
pub use gpui_menus as menus;

/// Power, session and thermal observations with lifecycle-bound wake leases.
#[cfg(feature = "power")]
pub use gpui_power as power;

/// Lifecycle-aware tray and menu-bar status items with scoped leases.
#[cfg(feature = "tray")]
pub use gpui_tray as tray;

/// Typed rich clipboard formats, opaque resource handoffs and drag-and-drop
/// contracts with scoped leases.
#[cfg(feature = "dnd")]
pub use gpui_dnd as dnd;

/// System proxy, trust, connectivity and scoped revalidation policy.
#[cfg(feature = "network")]
pub use gpui_network as network;

/// Persistent background jobs, constraint gating, checkpoint recovery and
/// bounded in-process CPU/I/O pools.
#[cfg(feature = "jobs")]
pub use gpui_jobs as jobs;

/// Persistent local notification scheduling, evidence and reconciliation.
#[cfg(feature = "notification-schedule")]
pub use gpui_notification_schedule as notification_schedule;

/// Taskbar, Dock, badge, attention and notification task presentation.
#[cfg(feature = "task-presentation")]
pub use gpui_task_presentation as task_presentation;

/// Remote push providers, device registration and bounded message routing.
#[cfg(feature = "push")]
pub use gpui_push as push;

#[cfg(test)]
mod tests {
    #[cfg(feature = "jobs")]
    #[test]
    fn jobs_facade_reexports_the_coordinated_contracts() {
        use crate::jobs;
        assert_eq!(jobs::UPSTREAM_GPUI_REVISION, crate::UPSTREAM_REVISION);
        assert_eq!(jobs::PACKAGE_VERSION, crate::VERSION);
        assert_eq!(jobs::JobsOperation::ALL.len(), 6);
        assert_eq!(jobs::PlatformTarget::ALL.len(), 6);
        assert_eq!(jobs::platform_support_matrix().len(), 36);
        assert_eq!(jobs::DEFAULT_JOBS_EVENT_CAPACITY, 64);
        assert_eq!(jobs::PROVIDER_MANIFEST.len(), 5);
        assert!(!jobs::durable_scheduling_available(
            jobs::PlatformTarget::Linux
        ));
        assert!(
            jobs::platform_support_matrix()
                .iter()
                .all(|row| row.evidence == jobs::Evidence::ContractOnly)
        );
    }

    #[cfg(feature = "notification-schedule")]
    #[test]
    fn notification_schedule_facade_reexports_the_coordinated_contracts() {
        use crate::notification_schedule;
        assert_eq!(
            notification_schedule::UPSTREAM_GPUI_REVISION,
            crate::UPSTREAM_REVISION
        );
        assert_eq!(notification_schedule::PACKAGE_VERSION, crate::VERSION);
        assert_eq!(notification_schedule::ScheduleOperation::ALL.len(), 6);
        assert_eq!(notification_schedule::PlatformTarget::ALL.len(), 6);
        assert_eq!(notification_schedule::platform_support_matrix().len(), 36);
        assert_eq!(
            notification_schedule::DEFAULT_NOTIFICATION_SCHEDULE_EVENT_CAPACITY,
            64
        );
        assert_eq!(notification_schedule::PROVIDER_MANIFEST.len(), 5);
        assert_eq!(
            notification_schedule::NOTIFICATION_SCHEDULE_FORMAT_VERSION,
            1
        );
        assert_eq!(notification_schedule::MIN_REPEATING_INTERVAL_MS, 60_000);
        assert_eq!(notification_schedule::DEFAULT_MAX_ACTIVE_SCHEDULES, 64);
        assert!(
            notification_schedule::platform_support_matrix()
                .iter()
                .all(|row| row.evidence == notification_schedule::Evidence::ContractOnly)
        );
    }

    #[cfg(feature = "power")]
    #[test]
    fn power_facade_reexports_the_coordinated_contracts() {
        assert_eq!(
            crate::power::UPSTREAM_GPUI_REVISION,
            crate::UPSTREAM_REVISION
        );
        assert_eq!(crate::power::PACKAGE_VERSION, crate::VERSION);
        assert_eq!(crate::power::PowerOperation::ALL.len(), 5);
        assert_eq!(crate::power::platform_support_matrix().len(), 30);
        assert_eq!(crate::power::DEFAULT_POWER_EVENT_CAPACITY, 64);
    }

    #[cfg(feature = "tray")]
    #[test]
    fn tray_facade_reexports_the_coordinated_contracts() {
        assert_eq!(
            crate::tray::UPSTREAM_GPUI_REVISION,
            crate::UPSTREAM_REVISION
        );
        assert_eq!(crate::tray::PACKAGE_VERSION, crate::VERSION);
        assert_eq!(crate::tray::TrayOperation::ALL.len(), 5);
        assert_eq!(crate::tray::platform_support_matrix().len(), 30);
        assert_eq!(crate::tray::DEFAULT_TRAY_EVENT_CAPACITY, 64);
    }

    #[cfg(feature = "dnd")]
    #[test]
    fn dnd_facade_reexports_the_coordinated_contracts() {
        assert_eq!(crate::dnd::UPSTREAM_GPUI_REVISION, crate::UPSTREAM_REVISION);
        assert_eq!(crate::dnd::PACKAGE_VERSION, crate::VERSION);
        assert_eq!(crate::dnd::DndOperation::ALL.len(), 6);
        assert_eq!(crate::dnd::platform_support_matrix().len(), 36);
        assert_eq!(crate::dnd::DEFAULT_DND_EVENT_CAPACITY, 64);
        assert_eq!(crate::dnd::MAX_ACTIVE_STREAM_LEASES, 64);
    }

    #[cfg(feature = "network")]
    #[test]
    fn network_facade_reexports_the_coordinated_contracts() {
        assert_eq!(
            crate::network::UPSTREAM_GPUI_REVISION,
            crate::UPSTREAM_REVISION
        );
        assert_eq!(crate::network::PACKAGE_VERSION, crate::VERSION);
        assert_eq!(crate::network::NetworkOperation::ALL.len(), 6);
        assert_eq!(crate::network::PlatformTarget::ALL.len(), 6);
        assert_eq!(crate::network::platform_support_matrix().len(), 36);
        assert_eq!(crate::network::DEFAULT_NETWORK_EVENT_CAPACITY, 64);
        assert_eq!(crate::network::MAX_PROXY_AUTH_ATTEMPTS, 2);
    }

    #[cfg(feature = "task-presentation")]
    #[test]
    fn task_presentation_facade_reexports_the_coordinated_contracts() {
        assert_eq!(
            crate::task_presentation::UPSTREAM_GPUI_REVISION,
            crate::UPSTREAM_REVISION
        );
        assert_eq!(crate::task_presentation::PACKAGE_VERSION, crate::VERSION);
        assert_eq!(
            crate::task_presentation::TaskPresentationOperation::ALL.len(),
            5
        );
        assert_eq!(
            crate::task_presentation::platform_support_matrix().len(),
            30
        );
        assert_eq!(
            crate::task_presentation::DEFAULT_TASK_PRESENTATION_EVENT_CAPACITY,
            64
        );
    }

    #[cfg(feature = "diagnostics")]
    #[test]
    fn diagnostics_facade_reexports_the_coordinated_contracts() {
        assert_eq!(
            crate::diagnostics::UPSTREAM_GPUI_REVISION,
            crate::UPSTREAM_REVISION
        );
        assert_eq!(crate::diagnostics::PACKAGE_VERSION, crate::VERSION);
        assert_eq!(crate::diagnostics::DiagnosticsOperation::ALL.len(), 5);
        assert_eq!(crate::diagnostics::platform_support_matrix().len(), 30);
        assert_eq!(
            crate::diagnostics::redact_text("Bearer canary"),
            "[REDACTED:bearer]"
        );
    }

    #[cfg(feature = "auth")]
    #[test]
    fn auth_facade_reexports_the_coordinated_contracts() {
        assert_eq!(
            crate::auth::UPSTREAM_GPUI_REVISION,
            crate::UPSTREAM_REVISION
        );
        assert_eq!(crate::auth::PACKAGE_VERSION, crate::VERSION);
        assert_eq!(crate::auth::AuthPlatform::ALL.len(), 6);
        assert_eq!(crate::auth::AuthOperation::ALL.len(), 10);
        assert_eq!(crate::auth::platform_support_matrix().len(), 60);
    }

    #[cfg(feature = "push")]
    #[test]
    fn push_facade_reexports_the_coordinated_contracts() {
        assert_eq!(
            crate::push::UPSTREAM_GPUI_REVISION,
            crate::UPSTREAM_REVISION
        );
        assert_eq!(crate::push::PACKAGE_VERSION, crate::VERSION);
        assert_eq!(crate::push::PushOperation::ALL.len(), 6);
        assert_eq!(crate::push::PushPlatform::ALL.len(), 6);
        assert_eq!(crate::push::platform_support_matrix().len(), 36);
        assert_eq!(crate::push::DEFAULT_PUSH_EVENT_CAPACITY, 64);
        assert_eq!(crate::push::MAX_PUSH_PAYLOAD_BYTES, 4096);
        assert_eq!(crate::push::PUSH_SECRET_SERVICE, "gpui-push/v1");
        assert!(
            crate::push::platform_support_matrix()
                .iter()
                .all(|row| row.evidence == crate::push::Evidence::ContractOnly)
        );
    }
}
