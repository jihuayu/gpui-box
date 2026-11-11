//! Six-platform tray support policy.
//!
//! These rows are a contract, not runtime proof. The crate installs no native
//! status-item provider, so every row is [`Evidence::ContractOnly`]. Native
//! adapters must publish their own qualification evidence before changing that.
//!
//! Android, iOS and the Web are deliberately [`Support::Unsupported`] for all
//! five native tray operations. They use the explicit
//! [`crate::MobileAlternativeSurface`] contract instead.

use crate::TrayOperation;
use gpui_system::{Evidence, Support};
use serde::{Deserialize, Serialize};
use std::fmt;

/// The six targets covered by the tray matrix.
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

    /// Whether this is a desktop target with a possible native shell tray.
    pub const fn is_desktop(self) -> bool {
        matches!(self, Self::Windows | Self::MacOs | Self::Linux)
    }

    /// Whether this target is a mobile or browser target where native tray
    /// operations are always unsupported.
    pub const fn is_mobile_or_web(self) -> bool {
        matches!(self, Self::Android | Self::Ios | Self::Web)
    }

    pub fn operations(self) -> Vec<PlatformOperationSupport> {
        platform_operations(self)
    }

    pub fn support(self, operation: TrayOperation) -> Support {
        support_for(self, operation)
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
    pub operation: TrayOperation,
    pub support: Support,
    pub required_host_configuration: &'static str,
    pub evidence: Evidence,
    pub notes: &'static str,
    pub qualification_links: &'static [&'static str],
}

impl PlatformOperationSupport {
    pub const fn new(
        platform: PlatformTarget,
        operation: TrayOperation,
        support: Support,
        required_host_configuration: &'static str,
        evidence: Evidence,
        notes: &'static str,
        qualification_links: &'static [&'static str],
    ) -> Self {
        Self {
            platform,
            operation,
            support,
            required_host_configuration,
            evidence,
            notes,
            qualification_links,
        }
    }

    pub const fn is_supported(&self) -> bool {
        matches!(self.support, Support::Native | Support::Conditional)
    }

    pub fn has_link(&self, link: &str) -> bool {
        self.qualification_links.contains(&link)
    }
}

const WINDOWS_LINKS: &[&str] = &["#50", "#19", "#21", "#49", "#43"];
const MACOS_LINKS: &[&str] = &["#50", "#19", "#21", "#49", "#43"];
const LINUX_LINKS: &[&str] = &["#50", "#19", "#21", "#49", "#43"];
const ANDROID_LINKS: &[&str] = &["#50", "#21", "#43"];
const IOS_LINKS: &[&str] = &["#50", "#21", "#43"];
const WEB_LINKS: &[&str] = &["#50", "#18", "#21"];

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

fn support_for(platform: PlatformTarget, _operation: TrayOperation) -> Support {
    match platform {
        PlatformTarget::Windows | PlatformTarget::MacOs => Support::Native,
        PlatformTarget::Linux => Support::Conditional,
        PlatformTarget::Android | PlatformTarget::Ios | PlatformTarget::Web => Support::Unsupported,
    }
}

fn platform_configuration(platform: PlatformTarget, operation: TrayOperation) -> &'static str {
    match (platform, operation) {
        (PlatformTarget::Windows, TrayOperation::CreateStatusItem) => {
            "Register a message-only or hidden window, call Shell_NotifyIconW(NIM_ADD, NOTIFYICONDATAW) with a stable uID, and register the \"TaskbarCreated\" message for Explorer restart recovery."
        }
        (PlatformTarget::Windows, TrayOperation::UpdateStatusItem) => {
            "Call Shell_NotifyIconW(NIM_MODIFY) with the current HICON, szTip, szInfoTitle and accessibility text on the owning UI thread; rebuild the HICON when theme or DPI changes."
        }
        (PlatformTarget::Windows, TrayOperation::RemoveStatusItem) => {
            "Call Shell_NotifyIconW(NIM_DELETE) exactly once per uID and destroy the generated HICON; a duplicate delete is treated as a no-op."
        }
        (PlatformTarget::Windows, TrayOperation::PresentStatusMenu) => {
            "Call TrackPopupMenuEx on the window that owns the status icon, set TS_TRUE for the foreground window, and route WM_COMMAND item IDs back through gpui-menus command identity."
        }
        (PlatformTarget::Windows, TrayOperation::ManageCloseToTrayResidency) => {
            "Intercept WM_CLOSE (or the host close request) before hiding the last visible window; require a live NIM_ADD item with a reachable restore or quit command and re-check shell availability at execution time."
        }
        (PlatformTarget::MacOs, TrayOperation::CreateStatusItem) => {
            "Call NSStatusBar.systemStatusBar statusItemWithLength on the main thread, retain the NSStatusItem, and use NSStatusItem.button.image with isTemplate for monochrome template images."
        }
        (PlatformTarget::MacOs, TrayOperation::UpdateStatusItem) => {
            "Update NSStatusItem.button.image, toolTip, accessibilityLabel and NSMenu on the main thread; select a new template image when appearance or display scale changes."
        }
        (PlatformTarget::MacOs, TrayOperation::RemoveStatusItem) => {
            "Remove the NSStatusItem with removeStatusItem and release the retained item and menu; a second removal is a no-op."
        }
        (PlatformTarget::MacOs, TrayOperation::PresentStatusMenu) => {
            "Assign an NSMenu to NSStatusItem.menu or call popUpMenuPositioningItem; dispatch selected NSMenuItem actions through gpui-menus command identity instead of duplicating command state."
        }
        (PlatformTarget::MacOs, TrayOperation::ManageCloseToTrayResidency) => {
            "Intercept windowShouldClose / the host close request; hiding the last window is allowed only while a live NSStatusItem exposes an enabled restore or quit path."
        }
        (PlatformTarget::Linux, TrayOperation::CreateStatusItem) => {
            "Export org.kde.StatusNotifierItem on the session bus and register with org.kde.StatusNotifierWatcher; this is conditional because the watcher and a StatusNotifierHost extension may be absent."
        }
        (PlatformTarget::Linux, TrayOperation::UpdateStatusItem) => {
            "Emit NewIcon, NewToolTip, NewTitle and NewStatus signals on org.kde.StatusNotifierItem; the watcher forwards them only while the item is registered."
        }
        (PlatformTarget::Linux, TrayOperation::RemoveStatusItem) => {
            "Unregister the StatusNotifierItem object and drop its bus name exactly once; if the watcher is gone the removal is already complete and must remain a no-op."
        }
        (PlatformTarget::Linux, TrayOperation::PresentStatusMenu) => {
            "Export com.canonical.dbusmenu for the item, evaluate the menu against live gpui-menus context before presenting it, and route the returned item id back to the command service."
        }
        (PlatformTarget::Linux, TrayOperation::ManageCloseToTrayResidency) => {
            "Treat an absent StatusNotifierWatcher, a host without a tray extension (common on GNOME Wayland), or a lost bus name as shell absence and never hide the last window."
        }
        (PlatformTarget::Android, _) => {
            "Unsupported: Android has no native desktop-status-bar item. Use MobileAlternativeSurface::OngoingNotification (notification channel, API 33 POST_NOTIFICATIONS), InAppStatusStrip, or QuickSettingsOrShortcut instead."
        }
        (PlatformTarget::Ios, _) => {
            "Unsupported: iOS has no NSStatusBar equivalent for third-party apps. Use MobileAlternativeSurface::InAppStatusStrip or QuickSettingsOrShortcut; an ongoing OS notification is not available."
        }
        (PlatformTarget::Web, _) => {
            "Unsupported: a browser document cannot own an OS status-bar item. Use MobileAlternativeSurface::InAppStatusStrip; ongoing notifications and OS quick-settings shortcuts are not browser-applicable."
        }
    }
}

fn platform_notes(platform: PlatformTarget, operation: TrayOperation) -> &'static str {
    match (platform, operation) {
        (_, TrayOperation::CreateStatusItem) => {
            "Status items are host UI-thread objects. A provider grant is not proof that the shell remained available; the service re-validates shell epoch and lease liveness after creation."
        }
        (_, TrayOperation::UpdateStatusItem) => {
            "Icon selection is deterministic per SystemPreferences and scale bucket; updates on removed, disposed or superseded leases never resurrect a native item."
        }
        (_, TrayOperation::RemoveStatusItem) => {
            "Removal is idempotent at both the lease and provider boundary. One native item is removed at most once even when Drop races an explicit remove."
        }
        (PlatformTarget::Linux, TrayOperation::PresentStatusMenu) => {
            "Conditional: com.canonical.dbusmenu is the portable Linux menu projection, but some hosts only support activation and expose no menu."
        }
        (
            PlatformTarget::Android | PlatformTarget::Ios | PlatformTarget::Web,
            TrayOperation::ManageCloseToTrayResidency,
        ) => {
            "Native close-to-tray residency is never granted on mobile or Web. Hosts use the explicit alternative surface and OS lifecycle rules instead."
        }
        (_, TrayOperation::PresentStatusMenu) => {
            "Menus are resolved live through gpui-menus so hidden, disabled and checked state cannot drift from the command registry."
        }
        (_, TrayOperation::ManageCloseToTrayResidency) => {
            "The no-invisible-non-quittable-process invariant is enforced before hiding and continuously while resident."
        }
    }
}

/// The complete six-target, five-operation support matrix (30 rows).
pub fn platform_support_matrix() -> Vec<PlatformOperationSupport> {
    let mut rows = Vec::with_capacity(30);
    for platform in PlatformTarget::ALL {
        for operation in TrayOperation::ALL {
            rows.push(PlatformOperationSupport::new(
                platform,
                operation,
                support_for(platform, operation),
                platform_configuration(platform, operation),
                Evidence::ContractOnly,
                platform_notes(platform, operation),
                platform_links(platform),
            ));
        }
    }
    rows
}

/// Look up one exact `(platform, operation)` row.
pub fn operation_support(
    platform: PlatformTarget,
    operation: TrayOperation,
) -> PlatformOperationSupport {
    platform_support_matrix()
        .into_iter()
        .find(|row| row.platform == platform && row.operation == operation)
        .expect("the support matrix contains every platform/operation pair")
}

/// All five operation rows for one platform in declaration order.
pub fn platform_operations(platform: PlatformTarget) -> Vec<PlatformOperationSupport> {
    TrayOperation::ALL
        .into_iter()
        .map(|operation| operation_support(platform, operation))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matrix_is_complete_and_mobile_is_unsupported() {
        let rows = platform_support_matrix();
        assert_eq!(rows.len(), 30);
        for platform in PlatformTarget::ALL {
            for operation in TrayOperation::ALL {
                let row = operation_support(platform, operation);
                assert_eq!(row.platform, platform);
                assert_eq!(row.operation, operation);
                assert_eq!(row.evidence, Evidence::ContractOnly);
                if platform.is_mobile_or_web() {
                    assert_eq!(row.support, Support::Unsupported);
                } else {
                    assert!(row.is_supported());
                }
            }
        }
    }
}
