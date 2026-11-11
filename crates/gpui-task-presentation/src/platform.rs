//! Six-platform task-presentation support policy.
//!
//! These rows are a contract, not runtime proof. This crate installs no native
//! provider, so every row is [`Evidence::ContractOnly`]. Native adapters must
//! publish their own qualification evidence before changing that.

use gpui_system::{Evidence, Support};
use serde::{Deserialize, Serialize};
use std::fmt;

/// The five task-presentation operations covered by the support matrix.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskPresentationOperation {
    PresentTaskProgress,
    PresentAppBadge,
    RequestUserAttention,
    MirrorProgressNotification,
    InvokeTaskAction,
}

impl TaskPresentationOperation {
    pub const ALL: [Self; 5] = [
        Self::PresentTaskProgress,
        Self::PresentAppBadge,
        Self::RequestUserAttention,
        Self::MirrorProgressNotification,
        Self::InvokeTaskAction,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::PresentTaskProgress => "present_task_progress",
            Self::PresentAppBadge => "present_app_badge",
            Self::RequestUserAttention => "request_user_attention",
            Self::MirrorProgressNotification => "mirror_progress_notification",
            Self::InvokeTaskAction => "invoke_task_action",
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::PresentTaskProgress => "PresentTaskProgress",
            Self::PresentAppBadge => "PresentAppBadge",
            Self::RequestUserAttention => "RequestUserAttention",
            Self::MirrorProgressNotification => "MirrorProgressNotification",
            Self::InvokeTaskAction => "InvokeTaskAction",
        }
    }

    pub const fn is_progress(self) -> bool {
        matches!(self, Self::PresentTaskProgress)
    }

    pub const fn is_badge(self) -> bool {
        matches!(self, Self::PresentAppBadge)
    }

    pub const fn is_attention(self) -> bool {
        matches!(self, Self::RequestUserAttention)
    }
}

impl fmt::Display for TaskPresentationOperation {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// The six targets covered by the task-presentation matrix.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlatformTarget {
    Windows,
    MacOs,
    Linux,
    Android,
    Ios,
    Web,
}

impl PlatformTarget {
    pub const ALL: [Self; 6] = [
        Self::Windows,
        Self::MacOs,
        Self::Linux,
        Self::Android,
        Self::Ios,
        Self::Web,
    ];

    /// The compile-time target running this code, if it is one of the rows.
    pub const fn current() -> Option<Self> {
        if cfg!(target_arch = "wasm32") {
            Some(Self::Web)
        } else if cfg!(target_os = "windows") {
            Some(Self::Windows)
        } else if cfg!(target_os = "macos") {
            Some(Self::MacOs)
        } else if cfg!(target_os = "linux") {
            Some(Self::Linux)
        } else if cfg!(target_os = "android") {
            Some(Self::Android)
        } else if cfg!(target_os = "ios") {
            Some(Self::Ios)
        } else {
            None
        }
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Windows => "windows",
            Self::MacOs => "macos",
            Self::Linux => "linux",
            Self::Android => "android",
            Self::Ios => "ios",
            Self::Web => "web",
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::Windows => "Windows",
            Self::MacOs => "macOS",
            Self::Linux => "Linux",
            Self::Android => "Android",
            Self::Ios => "iOS",
            Self::Web => "Web",
        }
    }

    pub const fn taskbar_name(self) -> &'static str {
        match self {
            Self::Windows => "Windows taskbar",
            Self::MacOs => "macOS Dock",
            Self::Linux => "Linux launcher",
            Self::Android => "Android launcher",
            Self::Ios => "iOS Home Screen",
            Self::Web => "Web app badge",
        }
    }

    /// All five operation rows for this platform in declaration order.
    pub fn operations(self) -> Vec<PlatformOperationSupport> {
        platform_operations(self)
    }
}

impl fmt::Display for PlatformTarget {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// One `(platform, operation)` policy row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlatformOperationSupport {
    pub platform: PlatformTarget,
    pub operation: TaskPresentationOperation,
    pub support: Support,
    pub required_host_configuration: &'static str,
    pub evidence: Evidence,
    pub notes: &'static str,
    /// Whether a numeric or short-status badge must be reported as a dot.
    pub badge_downgrades_to_dot: bool,
    pub qualification_links: &'static [&'static str],
}

impl PlatformOperationSupport {
    #[allow(clippy::too_many_arguments)]
    pub const fn new(
        platform: PlatformTarget,
        operation: TaskPresentationOperation,
        support: Support,
        required_host_configuration: &'static str,
        evidence: Evidence,
        notes: &'static str,
        badge_downgrades_to_dot: bool,
        qualification_links: &'static [&'static str],
    ) -> Self {
        Self {
            platform,
            operation,
            support,
            required_host_configuration,
            evidence,
            notes,
            badge_downgrades_to_dot,
            qualification_links,
        }
    }

    pub const fn is_supported(&self) -> bool {
        matches!(self.support, Support::Native | Support::Conditional)
    }

    pub const fn requires_string_configuration(&self) -> bool {
        !self.required_host_configuration.is_empty()
    }

    pub fn has_link(&self, link: &str) -> bool {
        self.qualification_links.contains(&link)
    }
}

const WINDOWS_LINKS: &[&str] = &["#53", "#19", "#21", "#43"];
const MACOS_LINKS: &[&str] = &["#53", "#19", "#21"];
const LINUX_LINKS: &[&str] = &["#53", "#19", "#21", "#43"];
const ANDROID_LINKS: &[&str] = &["#53", "#19", "#21", "#43"];
const IOS_LINKS: &[&str] = &["#53", "#19", "#21"];
const WEB_LINKS: &[&str] = &["#53", "#19"];

fn platform_links(platform: PlatformTarget) -> &'static [&'static str] {
    match platform {
        PlatformTarget::Windows => WINDOWS_LINKS,
        PlatformTarget::MacOs => MACOS_LINKS,
        PlatformTarget::Linux => LINUX_LINKS,
        PlatformTarget::Android => ANDROID_LINKS,
        PlatformTarget::Ios => IOS_LINKS,
        PlatformTarget::Web => WEB_LINKS,
    }
}

fn platform_support(platform: PlatformTarget, operation: TaskPresentationOperation) -> Support {
    use PlatformTarget as P;
    use TaskPresentationOperation as O;
    match (platform, operation) {
        (P::Windows, O::PresentTaskProgress | O::PresentAppBadge | O::RequestUserAttention) => {
            Support::Native
        }
        (P::Windows, O::MirrorProgressNotification | O::InvokeTaskAction) => Support::Conditional,
        (P::MacOs, O::PresentTaskProgress | O::PresentAppBadge | O::RequestUserAttention) => {
            Support::Native
        }
        (P::MacOs, O::MirrorProgressNotification | O::InvokeTaskAction) => Support::Conditional,
        (P::Linux, _) => Support::Conditional,
        (P::Android, O::PresentTaskProgress) => Support::Unsupported,
        (P::Android, O::MirrorProgressNotification) => Support::Native,
        (P::Android, O::PresentAppBadge | O::RequestUserAttention | O::InvokeTaskAction) => {
            Support::Conditional
        }
        (P::Ios, O::PresentTaskProgress | O::RequestUserAttention) => Support::Unsupported,
        (P::Ios, O::PresentAppBadge) => Support::Native,
        (P::Ios, O::MirrorProgressNotification | O::InvokeTaskAction) => Support::Conditional,
        (P::Web, O::PresentTaskProgress | O::RequestUserAttention | O::InvokeTaskAction) => {
            Support::Unsupported
        }
        (P::Web, O::PresentAppBadge | O::MirrorProgressNotification) => Support::Conditional,
    }
}

fn platform_configuration(
    platform: PlatformTarget,
    operation: TaskPresentationOperation,
) -> &'static str {
    use PlatformTarget as P;
    use TaskPresentationOperation as O;
    match (platform, operation) {
        (
            P::Windows,
            O::PresentTaskProgress
            | O::PresentAppBadge
            | O::RequestUserAttention
            | O::InvokeTaskAction,
        ) => {
            "Hold a live HWND on a message-pumping UI thread and resolve ITaskbarList3 (CLSID_TaskbarList) before the first update; COM initialization must already be balanced."
        }
        (P::Windows, O::MirrorProgressNotification) => {
            "Use gpui-notifications over WinRT toast notifications; ongoing progress requires a compatible Windows build and a registered packaged/unpackaged application identity."
        }
        (P::MacOs, O::PresentTaskProgress | O::PresentAppBadge) => {
            "Use NSApplication.dockTile (NSDockTile) on the main run loop. NSDockTile.contentView/display provides Dock progress; badgeLabel is text, not a numeric-only API."
        }
        (P::MacOs, O::RequestUserAttention) => {
            "Call NSApplication.requestUserAttention(.informationalRequest or .criticalRequest) from the main run loop and cancel it when the surface is focused."
        }
        (P::MacOs, O::MirrorProgressNotification | O::InvokeTaskAction) => {
            "Use gpui-notifications with UNUserNotificationCenter; progress and action buttons are conditional on the notification style and user authorization."
        }
        (P::Linux, O::PresentTaskProgress) => {
            "Use com.canonical.Unity.LauncherEntry over the session D-Bus for shells that export it. Progress is accepted by the daemon without a visual-render guarantee; X11-only shells and plain Wayland compositors may show nothing."
        }
        (P::Linux, O::PresentAppBadge) => {
            "Prefer com.canonical.Unity.LauncherEntry's count/count-visible properties, falling back to a dot/urgent hint where the shell only exposes a boolean indicator."
        }
        (P::Linux, O::RequestUserAttention) => {
            "Set the X11 WM_HINTS urgency hint or demand attention where the window manager honors it; on Wayland use xdg-activation-v1 with a token from the triggering user action. There is no universal global attention API."
        }
        (P::Linux, O::MirrorProgressNotification) => {
            "Use org.freedesktop.Notifications over the session bus through gpui-notifications. The daemon may omit progress or hints and remains authoritative for display."
        }
        (P::Linux, O::InvokeTaskAction) => {
            "Route action buttons through gpui-notifications when the daemon supports actions, or through a host-provided thumbbar/proxy surface. Do not synthesize X11/Wayland global shortcuts."
        }
        (P::Android, O::PresentTaskProgress) => {
            "Unsupported: Android has no taskbar/Dock progress contract for ordinary applications. WorkManager/foreground-service progress belongs to the notification mirror."
        }
        (P::Android, O::PresentAppBadge) => {
            "Use NotificationChannel.setShowBadge plus a notification with a numeric count. Launcher support is conditional and requires POST_NOTIFICATIONS; long-press/launcher behavior is not observed through the notification API."
        }
        (P::Android, O::RequestUserAttention) => {
            "Use a heads-up notification with IMPORTANCE_HIGH or a foreground-service notification; exact launcher/taskbar blinking is not available to ordinary applications."
        }
        (P::Android, O::MirrorProgressNotification) => {
            "Use a foreground-service or ongoing notification with setProgress and a stable notification id; POST_NOTIFICATIONS is required on Android 13+, and Doze may defer non-foreground updates."
        }
        (P::Android, O::InvokeTaskAction) => {
            "Add NotificationCompat.Action entries and route PendingIntent broadcasts back into the UI thread; verify the task generation before executing."
        }
        (P::Ios, O::PresentTaskProgress) => {
            "Unsupported: NSExtension/NSDockTile APIs are unavailable to iOS applications and there is no Dock progress surface."
        }
        (P::Ios, O::PresentAppBadge) => {
            "Set UNUserNotificationCenter badge or UIApplication.applicationIconBadgeNumber after authorization; badge authorization is distinct from alert authorization."
        }
        (P::Ios, O::RequestUserAttention) => {
            "Unsupported as a taskbar/Dock attention request. Use a user-visible notification instead; do not claim that a Dock bounce exists on iOS."
        }
        (P::Ios, O::MirrorProgressNotification) => {
            "Use gpui-notifications with UNUserNotificationCenter. Live Activities are eligible only for explicitly supported task boundaries and require an ActivityKit extension; ordinary local notifications cannot host an arbitrary progress bar."
        }
        (P::Ios, O::InvokeTaskAction) => {
            "UNNotificationAction buttons route through UNUserNotificationCenterDelegate; Live Activity buttons require ActivityKit intents. Reject actions after the task is terminal."
        }
        (P::Web, O::PresentTaskProgress) => {
            "Unsupported: there is no browser taskbar/Dock progress API. Use the notification mirror or the page's own UI."
        }
        (P::Web, O::PresentAppBadge) => {
            "Call navigator.setAppBadge / clearAppBadge when available in a secure context, with a document-title fallback. Badging is installed-PWA and browser dependent; permissions-policy may reject it."
        }
        (P::Web, O::RequestUserAttention) => {
            "Unsupported: the Web platform exposes no taskbar/Dock attention API. Do not shake the favicon or play audio as a substitute."
        }
        (P::Web, O::MirrorProgressNotification) => {
            "Use the Notification API through gpui-notifications only after Notification.requestPermission; progress bars are not standardized and may be omitted."
        }
        (P::Web, O::InvokeTaskAction) => {
            "Unsupported for taskbar thumbbar buttons. Notification actions where implemented require service-worker registration and are outside this contract."
        }
    }
}

fn platform_notes(platform: PlatformTarget, operation: TaskPresentationOperation) -> &'static str {
    use PlatformTarget as P;
    use TaskPresentationOperation as O;
    match (platform, operation) {
        (_, O::PresentTaskProgress) => {
            "Progress is presentation only. A cancelled or dropped task must clear its contribution immediately; terminal cleanup never waits for the throttle interval."
        }
        (P::Windows, O::PresentAppBadge) => {
            "ITaskbarList3 overlay icons are boolean/icon-shaped, so numeric or short-status badges downgrade to a dot with an explicit receipt."
        }
        (P::Linux, O::PresentAppBadge) => {
            "LauncherEntry availability depends on the desktop shell and its D-Bus surface; a dot downgrade is an accepted, reported outcome, never a confirmed numeric badge."
        }
        (P::Android, O::PresentAppBadge) => {
            "Launcher badges are user/launcher policy. The count is an input to the notification, not proof that a launcher rendered it."
        }
        (P::Web, O::PresentAppBadge) => {
            "navigator.setAppBadge accepts a number where implemented, but browser and installed-PWA support is conditional. The document-title fallback must be removable."
        }
        (_, O::RequestUserAttention) => {
            "Attention is generation-bound and must be cleared on surface focus. Critical is not a promise that the platform flashes visibly."
        }
        (_, O::MirrorProgressNotification) => {
            "Submission receipts are not display evidence. Permission denial and daemon unavailability are reported per surface without failing the business task."
        }
        (_, O::InvokeTaskAction) => {
            "Action buttons always revalidate task existence, generation and state before invoking a gpui-menus command. Stale clicks are rejected without execution."
        }
        (_, O::PresentAppBadge) => {
            "A badge surface may only support a dot; numeric or short-status values are explicitly downgraded and reported."
        }
    }
}

fn platform_badge_downgrade(platform: PlatformTarget) -> bool {
    matches!(platform, PlatformTarget::Windows | PlatformTarget::Linux)
}

/// The complete six-target, five-operation support matrix (30 rows).
pub fn platform_support_matrix() -> Vec<PlatformOperationSupport> {
    let mut rows = Vec::with_capacity(30);
    for platform in PlatformTarget::ALL {
        for operation in TaskPresentationOperation::ALL {
            rows.push(PlatformOperationSupport::new(
                platform,
                operation,
                platform_support(platform, operation),
                platform_configuration(platform, operation),
                Evidence::ContractOnly,
                platform_notes(platform, operation),
                platform_badge_downgrade(platform),
                platform_links(platform),
            ));
        }
    }
    rows
}

/// Look up one exact `(platform, operation)` row.
pub fn operation_support(
    platform: PlatformTarget,
    operation: TaskPresentationOperation,
) -> PlatformOperationSupport {
    platform_support_matrix()
        .into_iter()
        .find(|row| row.platform == platform && row.operation == operation)
        .expect("the support matrix contains every platform/operation pair")
}

/// All five operation rows for one platform in declaration order.
pub fn platform_operations(platform: PlatformTarget) -> Vec<PlatformOperationSupport> {
    TaskPresentationOperation::ALL
        .into_iter()
        .map(|operation| operation_support(platform, operation))
        .collect()
}

/// Current-platform badge downgrade rule, defaulting to `false` off-platform.
pub fn current_platform_badge_downgrades_to_dot() -> bool {
    PlatformTarget::current().is_some_and(platform_badge_downgrade)
}
