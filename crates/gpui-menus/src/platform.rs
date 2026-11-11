//! Six-platform by five-operation support matrix and browser shortcut policy.
//!
//! The matrix is a contract, not runtime proof. Native menu integration remains
//! host work; mobile and web hosts render in-canvas menus instead of a native
//! OS menu bar.

use crate::{
    Availability, Configuration, Constraint, Evidence, KeystrokeSpec, MenusOperation, Modifiers,
    PermissionState, Support,
};
use serde::{Deserialize, Serialize};
use std::fmt;

/// The six targets covered by the menu support matrix.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MenuPlatform {
    Windows,
    MacOs,
    Linux,
    Android,
    Ios,
    Web,
}

impl MenuPlatform {
    pub const ALL: [Self; 6] = [
        Self::Windows,
        Self::MacOs,
        Self::Linux,
        Self::Android,
        Self::Ios,
        Self::Web,
    ];

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

    pub const fn has_native_menu_bar(self) -> bool {
        matches!(self, Self::Windows | Self::MacOs | Self::Linux)
    }

    pub const fn supports_hardware_keyboard(self) -> bool {
        matches!(
            self,
            Self::Windows | Self::MacOs | Self::Linux | Self::Android | Self::Ios | Self::Web
        )
    }

    pub const fn is_mobile_or_web(self) -> bool {
        matches!(self, Self::Android | Self::Ios | Self::Web)
    }
}

impl fmt::Display for MenuPlatform {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// Owner links shared by every matrix row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MenuIssueLinks {
    pub components: &'static str,
    pub desktop: &'static str,
    pub mobile: &'static str,
    pub phase_one: &'static str,
    pub system: &'static str,
    pub native_menus: &'static str,
    pub background: &'static str,
    pub qualification: &'static str,
}

impl MenuIssueLinks {
    pub const ALL: [&'static str; 8] = ["#3", "#8", "#9", "#18", "#19", "#24", "#43", "#75"];

    pub fn as_array(self) -> [&'static str; 8] {
        [
            self.components,
            self.desktop,
            self.mobile,
            self.phase_one,
            self.system,
            self.native_menus,
            self.background,
            self.qualification,
        ]
    }
}

impl Default for MenuIssueLinks {
    fn default() -> Self {
        Self {
            components: "#3",
            desktop: "#8",
            mobile: "#9",
            phase_one: "#18",
            system: "#19",
            native_menus: "#24",
            background: "#43",
            qualification: "#75",
        }
    }
}

/// Compatibility alias for hosts that name the platform target explicitly.
pub type MenuPlatformTarget = MenuPlatform;

/// Compatibility alias for the per-operation support row.
pub type PlatformMenuSupport = MenusPlatformSupport;

/// One `(platform, operation)` contract row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MenusPlatformSupport {
    pub platform: MenuPlatform,
    pub operation: MenusOperation,
    pub support: Support,
    pub configuration: Configuration,
    pub availability: Availability,
    pub permission: PermissionState,
    pub evidence: Evidence,
    pub constraints: Vec<Constraint>,
    pub required_host_configuration: String,
    pub runtime_evidence: String,
    pub notes: String,
    pub links: MenuIssueLinks,
}

impl MenusPlatformSupport {
    pub const fn permission_state(&self) -> PermissionState {
        self.permission
    }

    pub fn owner_links(&self) -> [&'static str; 8] {
        self.links.as_array()
    }

    pub const fn is_supported(&self) -> bool {
        matches!(self.support, Support::Native | Support::Conditional)
    }

    pub fn capability(&self, observed_revision: u64) -> crate::Capability<MenusOperation> {
        crate::Capability {
            operation: self.operation,
            support: self.support,
            availability: self.availability,
            configuration: self.configuration,
            permission: self.permission,
            evidence: self.evidence,
            constraints: self.constraints.clone(),
            observed_revision,
        }
    }
}

fn host_configuration(platform: MenuPlatform) -> &'static str {
    match platform {
        MenuPlatform::Windows => {
            "Install a GPUI Windows host with a top-level window and a current OperationContext; register commands before installing the menu bar."
        }
        MenuPlatform::MacOs => {
            "Install a GPUI macOS host with a top-level window and a current OperationContext; the native app menu remains host-owned."
        }
        MenuPlatform::Linux => {
            "Install a GPUI X11 or Wayland host with a current OperationContext; desktop-shell menu integration remains host-supplied."
        }
        MenuPlatform::Android => {
            "Render an in-canvas menu host inside the SurfaceView; Android has no native OS menu bar. Hardware keyboards are optional."
        }
        MenuPlatform::Ios => {
            "Render an in-canvas menu host in the UIKit view hierarchy; iOS has no native OS menu bar. iPadOS hardware keyboards are optional."
        }
        MenuPlatform::Web => {
            "Run gpui_web in an experimental host, render in-canvas menus and reserve browser chords. A browser has no native OS menu bar."
        }
    }
}

fn support_for(platform: MenuPlatform, operation: MenusOperation) -> Support {
    match (platform, operation) {
        (MenuPlatform::Windows | MenuPlatform::MacOs, _) => Support::Native,
        (MenuPlatform::Linux, MenusOperation::InstallMenuBar) => Support::Conditional,
        (MenuPlatform::Linux, _) => Support::Native,
        (
            MenuPlatform::Android | MenuPlatform::Ios | MenuPlatform::Web,
            MenusOperation::InstallMenuBar,
        ) => Support::Unsupported,
        (
            MenuPlatform::Android | MenuPlatform::Ios | MenuPlatform::Web,
            MenusOperation::PresentContextMenu | MenusOperation::SyncKeybindings,
        ) => Support::Conditional,
        (MenuPlatform::Android | MenuPlatform::Ios | MenuPlatform::Web, _) => Support::Native,
    }
}

fn evidence_for(platform: MenuPlatform) -> Evidence {
    match platform {
        MenuPlatform::Windows | MenuPlatform::MacOs | MenuPlatform::Linux => Evidence::Compiled,
        MenuPlatform::Android | MenuPlatform::Ios | MenuPlatform::Web => Evidence::ContractOnly,
    }
}

fn notes_for(platform: MenuPlatform, operation: MenusOperation) -> &'static str {
    match operation {
        MenusOperation::InstallMenuBar if !platform.has_native_menu_bar() => {
            "No native OS menu bar exists; the host must render a component menu bar in-canvas."
        }
        MenusOperation::InstallMenuBar => {
            "The logical menu bar is installed through the provider; native shell acceptance remains host work."
        }
        MenusOperation::PresentContextMenu => {
            "Context menus are resolved against the current surface generation and anchor; stale callbacks are rejected."
        }
        MenusOperation::SyncKeybindings => {
            "Bindings are projected from live context and resolved by scope specificity, priority and registration order."
        }
        MenusOperation::QueryPalette => {
            "Palette queries evaluate visible_when, materialize lazy contributions and rank deterministically."
        }
        MenusOperation::InvokeCommand => {
            "Every invocation re-validates surface generation, cancellation, deadline, capabilities, permissions and enabled_when."
        }
    }
}

fn constraints_for(platform: MenuPlatform, operation: MenusOperation) -> Vec<Constraint> {
    let mut constraints = Vec::new();
    if matches!(
        operation,
        MenusOperation::InstallMenuBar
            | MenusOperation::PresentContextMenu
            | MenusOperation::InvokeCommand
    ) && support_for(platform, operation) != Support::Unsupported
    {
        constraints.push(Constraint::ForegroundOwner);
    }
    if operation == MenusOperation::SyncKeybindings
        && matches!(
            platform,
            MenuPlatform::Android | MenuPlatform::Ios | MenuPlatform::Web
        )
    {
        constraints.push(Constraint::Hardware);
    }
    if platform == MenuPlatform::Web {
        constraints.push(Constraint::Provider("gpui_web"));
    }
    constraints
}

/// Browser-reserved shortcut labels. Hosts must not intercept these chords for
/// application commands; browsers retain them for tab/window lifecycle.
pub const BROWSER_RESERVED_SHORTCUTS: [&str; 5] = [
    "Ctrl/Cmd+W",
    "Ctrl/Cmd+T",
    "Ctrl/Cmd+N",
    "Ctrl/Cmd+L",
    "Ctrl/Cmd+Q",
];

/// Browser shortcut capability and reservation policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BrowserShortcutPolicy {
    pub reserved_shortcuts: &'static [&'static str],
    pub reserved_keys: &'static [&'static str],
    pub hardware_keyboard_supported: bool,
    pub note: &'static str,
}

pub const BROWSER_RESERVED_KEYS: [&str; 5] = ["w", "t", "n", "l", "q"];

pub const fn browser_shortcut_policy() -> BrowserShortcutPolicy {
    BrowserShortcutPolicy {
        reserved_shortcuts: &BROWSER_RESERVED_SHORTCUTS,
        reserved_keys: &BROWSER_RESERVED_KEYS,
        hardware_keyboard_supported: true,
        note: "gpui_web is experimental. Ctrl/Cmd+W/T/N/L/Q stay browser-owned; application shortcuts must choose another chord or an explicit alternative.",
    }
}

/// True when the chord is reserved by the browser policy. Both Ctrl and the
/// platform Command modifier are reserved; Alt/Shift variants are not part of
/// this list.
pub fn is_browser_reserved_shortcut(chord: &KeystrokeSpec) -> bool {
    (chord.modifiers.control || chord.modifiers.platform)
        && !chord.modifiers.alt
        && !chord.modifiers.shift
        && BROWSER_RESERVED_KEYS.contains(&chord.key.as_str())
}

/// True when a binding may be synchronized to a browser host without stealing
/// a reserved browser chord.
pub fn browser_shortcut_allowed(chord: &KeystrokeSpec) -> bool {
    !is_browser_reserved_shortcut(chord)
}

/// Compatibility alias for the reservation predicate.
pub fn browser_shortcut_reserved(chord: &KeystrokeSpec) -> bool {
    is_browser_reserved_shortcut(chord)
}

/// Compatibility alias for the complete platform matrix.
pub fn menu_platform_support_matrix() -> Vec<MenusPlatformSupport> {
    platform_support_matrix()
}

fn row(
    platform: MenuPlatform,
    operation: MenusOperation,
    links: MenuIssueLinks,
) -> MenusPlatformSupport {
    let support = support_for(platform, operation);
    let supported = matches!(support, Support::Native | Support::Conditional);
    MenusPlatformSupport {
        platform,
        operation,
        support,
        configuration: if supported {
            Configuration::Ready
        } else {
            Configuration::Unknown
        },
        availability: if supported {
            Availability::Available
        } else {
            Availability::Unavailable
        },
        permission: PermissionState::NotRequired,
        evidence: evidence_for(platform),
        constraints: constraints_for(platform, operation),
        required_host_configuration: host_configuration(platform).to_owned(),
        runtime_evidence: format!(
            "{} evidence on {}; native menu acceptance is tracked by the owner links.",
            match evidence_for(platform) {
                Evidence::Compiled => "Compiled",
                Evidence::ContractOnly => "Contract-only",
                Evidence::RuntimeVerified => "Runtime-verified",
            },
            platform
        ),
        notes: notes_for(platform, operation).to_owned(),
        links,
    }
}

/// The complete six-target, five-operation support matrix (30 rows).
pub fn platform_support_matrix() -> Vec<MenusPlatformSupport> {
    let links = MenuIssueLinks::default();
    let mut rows = Vec::with_capacity(MenuPlatform::ALL.len() * MenusOperation::ALL.len());
    for platform in MenuPlatform::ALL {
        for operation in MenusOperation::ALL {
            rows.push(row(platform, operation, links));
        }
    }
    rows
}

/// Look up one exact `(platform, operation)` row.
pub fn operation_support(
    platform: MenuPlatform,
    operation: MenusOperation,
) -> MenusPlatformSupport {
    platform_support_matrix()
        .into_iter()
        .find(|row| row.platform == platform && row.operation == operation)
        .expect("the support matrix contains every platform/operation pair")
}

/// All five operations for one platform in declaration order.
pub fn platform_support(platform: MenuPlatform) -> Vec<MenusPlatformSupport> {
    MenusOperation::ALL
        .into_iter()
        .map(|operation| operation_support(platform, operation))
        .collect()
}

/// True when the platform is subject to the browser reserved-shortcut policy.
pub const fn supports_browser_shortcut_policy(platform: MenuPlatform) -> bool {
    matches!(platform, MenuPlatform::Web)
}

/// Convenience projection for tests and documentation: a chord that contains a
/// reserved modifier but is otherwise allowed.
pub fn browser_safe_alternative(key: impl AsRef<str>) -> Result<KeystrokeSpec, crate::SystemError> {
    KeystrokeSpec::new(key, Modifiers::control().with_alt())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matrix_has_every_platform_operation_pair() {
        assert_eq!(MenuPlatform::ALL.len(), 6);
        assert_eq!(MenusOperation::ALL.len(), 5);
        let matrix = platform_support_matrix();
        assert_eq!(matrix.len(), 30);
        for platform in MenuPlatform::ALL {
            assert_eq!(platform_support(platform).len(), 5);
            for operation in MenusOperation::ALL {
                let row = operation_support(platform, operation);
                assert_eq!(row.platform, platform);
                assert_eq!(row.operation, operation);
                assert_eq!(row.permission, PermissionState::NotRequired);
                assert_eq!(row.links, MenuIssueLinks::default());
                assert!(!row.required_host_configuration.is_empty());
                assert!(!row.runtime_evidence.is_empty());
                assert!(!row.notes.is_empty());
            }
        }
    }

    #[test]
    fn browser_policy_reserves_navigation_chords_only() {
        let policy = browser_shortcut_policy();
        assert_eq!(policy.reserved_shortcuts.len(), 5);
        assert!(is_browser_reserved_shortcut(
            &KeystrokeSpec::new("w", Modifiers::command()).expect("chord")
        ));
        assert!(is_browser_reserved_shortcut(
            &KeystrokeSpec::new("t", Modifiers::control()).expect("chord")
        ));
        assert!(!is_browser_reserved_shortcut(
            &KeystrokeSpec::new("s", Modifiers::command()).expect("chord")
        ));
        assert!(!is_browser_reserved_shortcut(
            &KeystrokeSpec::new("w", Modifiers::control().with_alt()).expect("chord")
        ));
        assert!(!browser_shortcut_allowed(
            &KeystrokeSpec::new("q", Modifiers::command()).expect("chord")
        ));
    }

    #[test]
    fn mobile_and_web_have_no_native_menu_bar() {
        for platform in [MenuPlatform::Android, MenuPlatform::Ios, MenuPlatform::Web] {
            assert!(!platform.has_native_menu_bar());
            let row = operation_support(platform, MenusOperation::InstallMenuBar);
            assert_eq!(row.support, Support::Unsupported);
            assert!(!row.is_supported());
            let sync = operation_support(platform, MenusOperation::SyncKeybindings);
            assert!(sync.constraints.contains(&Constraint::Hardware));
        }
        assert!(MenuPlatform::Windows.has_native_menu_bar());
        assert!(supports_browser_shortcut_policy(MenuPlatform::Web));
    }
}
