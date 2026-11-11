//! Six-platform accessibility policy, ownership links and browser maturity.
//!
//! These are policy snapshots, not native provider claims. Every entry defaults
//! to `ContractOnly`; a platform adapter must publish separate automation and
//! assistive-reader evidence before it can move to `RuntimeVerified`.

use gpui_system::{
    Availability, Capability, Configuration, Constraint, Evidence, PermissionState, Support,
};
use serde::{Deserialize, Serialize};

/// A host platform including Web, which is outside the five-platform
/// `gpui_system::Platform` vocabulary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum AccessibilityPlatform {
    Windows,
    MacOs,
    Linux,
    Android,
    Ios,
    Web,
}

/// Compatibility alias matching the issue's `HostPlatform` wording.
pub type HostPlatform = AccessibilityPlatform;

impl AccessibilityPlatform {
    pub const ALL: [Self; 6] = [
        Self::Windows,
        Self::MacOs,
        Self::Linux,
        Self::Android,
        Self::Ios,
        Self::Web,
    ];

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

    pub const fn bridge(self) -> AssistiveBridgeKind {
        match self {
            Self::Windows => AssistiveBridgeKind::UiAutomation,
            Self::MacOs => AssistiveBridgeKind::NsAccessibility,
            Self::Linux => AssistiveBridgeKind::AtSpi,
            Self::Android => AssistiveBridgeKind::TalkBack,
            Self::Ios => AssistiveBridgeKind::VoiceOver,
            Self::Web => AssistiveBridgeKind::BrowserAria,
        }
    }
}

/// The native accessibility transport family for a platform.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum AssistiveBridgeKind {
    UiAutomation,
    NsAccessibility,
    AtSpi,
    TalkBack,
    VoiceOver,
    BrowserAria,
}

impl AssistiveBridgeKind {
    pub const fn label(self) -> &'static str {
        match self {
            Self::UiAutomation => "Windows UI Automation",
            Self::NsAccessibility => "NSAccessibility",
            Self::AtSpi => "AT-SPI",
            Self::TalkBack => "Android TalkBack / AccessibilityNodeProvider",
            Self::VoiceOver => "VoiceOver / UIAccessibility",
            Self::BrowserAria => "Browser ARIA / DOM accessibility tree",
        }
    }
}

/// Programmatic operations exposed by the semantic service.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum AccessibilityOperation {
    ObserveTree,
    PerformAction,
    EditTextSelection,
    AnnounceLive,
    AuditScene,
}

impl AccessibilityOperation {
    pub const ALL: [Self; 5] = [
        Self::ObserveTree,
        Self::PerformAction,
        Self::EditTextSelection,
        Self::AnnounceLive,
        Self::AuditScene,
    ];

    pub const fn label(self) -> &'static str {
        match self {
            Self::ObserveTree => "ObserveTree",
            Self::PerformAction => "PerformAction",
            Self::EditTextSelection => "EditTextSelection",
            Self::AnnounceLive => "AnnounceLive",
            Self::AuditScene => "AuditScene",
        }
    }
}

/// Explicit issue ownership for an adapter profile.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IssueLinks {
    /// Native adapter owner: #2 Android, #8 desktop, #9 iOS, #70 web.
    pub adapter: &'static str,
    /// Shared component semantics: #3.
    pub components: &'static str,
    /// Phase 1 sequencing: #18.
    pub phase_one: &'static str,
    /// Coordination links: #43, #68 and #70.
    pub coordination: &'static [&'static str],
}

pub const DESKTOP_LINKS: IssueLinks = IssueLinks {
    adapter: "#8",
    components: "#3",
    phase_one: "#18",
    coordination: &["#43", "#68", "#70"],
};

pub const ANDROID_LINKS: IssueLinks = IssueLinks {
    adapter: "#2",
    components: "#3",
    phase_one: "#18",
    coordination: &["#43", "#68", "#70"],
};

pub const IOS_LINKS: IssueLinks = IssueLinks {
    adapter: "#9",
    components: "#3",
    phase_one: "#18",
    coordination: &["#43", "#68", "#70"],
};

pub const WEB_LINKS: IssueLinks = IssueLinks {
    adapter: "#70",
    components: "#3",
    phase_one: "#18",
    coordination: &["#43", "#68", "#70"],
};

/// One platform/operation policy entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlatformOperationPolicy {
    pub platform: AccessibilityPlatform,
    pub operation: AccessibilityOperation,
    pub support: Support,
    pub configuration: Configuration,
    pub availability: Availability,
    pub permission: PermissionState,
    pub bridge: AssistiveBridgeKind,
    pub evidence: Evidence,
    /// Recorded native/session evidence identifiers. Empty until an adapter
    /// records real runtime evidence under its owner issue.
    pub runtime_evidence: &'static [&'static str],
    pub required_configuration: &'static [&'static str],
    pub links: IssueLinks,
}

impl PlatformOperationPolicy {
    pub fn capability(self, observed_revision: u64) -> Capability<AccessibilityOperation> {
        Capability {
            operation: self.operation,
            support: self.support,
            availability: self.availability,
            configuration: self.configuration,
            permission: self.permission,
            evidence: self.evidence,
            constraints: vec![Constraint::Provider(self.bridge.label())],
            observed_revision,
        }
    }

    /// Adapter probe result with fresh runtime availability/configuration.
    pub fn runtime_capability(
        self,
        availability: Availability,
        configuration: Configuration,
        evidence: Evidence,
        observed_revision: u64,
    ) -> Capability<AccessibilityOperation> {
        Capability {
            availability,
            configuration,
            evidence,
            ..self.capability(observed_revision)
        }
    }
}

/// Return the policy for one platform/operation pair. Negated platforms are
/// represented by explicit `Unsupported` entries rather than omitted rows, so a
/// caller always gets a typed answer.
pub fn platform_operation_policy(
    platform: AccessibilityPlatform,
    operation: AccessibilityOperation,
) -> PlatformOperationPolicy {
    let bridge = platform.bridge();
    let links = match platform {
        AccessibilityPlatform::Windows
        | AccessibilityPlatform::MacOs
        | AccessibilityPlatform::Linux => DESKTOP_LINKS,
        AccessibilityPlatform::Android => ANDROID_LINKS,
        AccessibilityPlatform::Ios => IOS_LINKS,
        AccessibilityPlatform::Web => WEB_LINKS,
    };
    let (support, required_configuration) = match (platform, operation) {
        (
            AccessibilityPlatform::Windows,
            AccessibilityOperation::ObserveTree | AccessibilityOperation::PerformAction,
        ) => (
            Support::Native,
            &["Windows UI Automation provider bound to the foreground HWND"][..],
        ),
        (AccessibilityPlatform::Windows, AccessibilityOperation::EditTextSelection) => (
            Support::Native,
            &["UI Automation TextPattern provider and focused HWND"][..],
        ),
        (AccessibilityPlatform::Windows, AccessibilityOperation::AnnounceLive) => (
            Support::Conditional,
            &[
                "UI Automation notification listener",
                "screen-reader notification opt-in",
            ][..],
        ),
        (AccessibilityPlatform::Windows, AccessibilityOperation::AuditScene) => {
            (Support::Native, &["committed AccessKit semantic scene"][..])
        }
        (
            AccessibilityPlatform::MacOs,
            AccessibilityOperation::ObserveTree
            | AccessibilityOperation::PerformAction
            | AccessibilityOperation::EditTextSelection
            | AccessibilityOperation::AnnounceLive,
        ) => (
            Support::Native,
            &["NSAccessibility bridge on the AppKit main thread"][..],
        ),
        (AccessibilityPlatform::MacOs, AccessibilityOperation::AuditScene) => {
            (Support::Native, &["committed AccessKit semantic scene"][..])
        }
        (AccessibilityPlatform::Linux, AccessibilityOperation::AuditScene) => {
            (Support::Native, &["committed AccessKit semantic scene"][..])
        }
        (AccessibilityPlatform::Linux, _) => (
            Support::Conditional,
            &[
                "AT-SPI D-Bus accessibility bus",
                "active desktop accessibility session",
            ][..],
        ),
        (AccessibilityPlatform::Android, AccessibilityOperation::AuditScene) => {
            (Support::Native, &["committed AccessKit semantic scene"][..])
        }
        (AccessibilityPlatform::Android, _) => (
            Support::Conditional,
            &[
                "Android AccessibilityNodeProvider bridge (#2)",
                "TalkBack session for reader acceptance",
            ][..],
        ),
        (AccessibilityPlatform::Ios, AccessibilityOperation::AuditScene) => {
            (Support::Native, &["committed AccessKit semantic scene"][..])
        }
        (AccessibilityPlatform::Ios, _) => (
            Support::Conditional,
            &[
                "UIAccessibility bridge in the iOS host (#9)",
                "VoiceOver session for reader acceptance",
            ][..],
        ),
        (AccessibilityPlatform::Web, AccessibilityOperation::AuditScene) => {
            (Support::Native, &["committed AccessKit semantic scene"][..])
        }
        (AccessibilityPlatform::Web, _) => (
            Support::Conditional,
            &[
                "gpui_web DOM/ARIA bridge (#70)",
                "browser accessibility tree in a secure context",
            ][..],
        ),
    };
    PlatformOperationPolicy {
        platform,
        operation,
        support,
        configuration: Configuration::Required,
        availability: Availability::Unknown,
        permission: PermissionState::NotRequired,
        bridge,
        evidence: Evidence::ContractOnly,
        runtime_evidence: &[],
        required_configuration,
        links,
    }
}

/// All five operation policies for one platform.
pub fn platform_policies(platform: AccessibilityPlatform) -> [PlatformOperationPolicy; 5] {
    AccessibilityOperation::ALL.map(|operation| platform_operation_policy(platform, operation))
}

/// All 30 platform/operation policy rows in stable platform/operation order.
pub fn all_platform_policies() -> Vec<PlatformOperationPolicy> {
    AccessibilityPlatform::ALL
        .into_iter()
        .flat_map(platform_policies)
        .collect()
}

/// Web backend maturity. `gpui_web` exists in the coordinated source tree but
/// is not a release platform in this milestone; browser policy therefore stays
/// explicitly experimental and conditional.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum WebBackendMaturity {
    Experimental,
}

/// Browser capability policy aligned with current `gpui_web` maturity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BrowserCapabilityPolicy {
    pub maturity: WebBackendMaturity,
    pub requires_secure_context: bool,
    pub requires_dom_accessibility_tree: bool,
    pub requires_same_origin_frame: bool,
    pub maximum_tree_nodes: usize,
    pub supported_operations: &'static [AccessibilityOperation],
}

pub const fn web_capability_policy() -> BrowserCapabilityPolicy {
    BrowserCapabilityPolicy {
        maturity: WebBackendMaturity::Experimental,
        requires_secure_context: true,
        requires_dom_accessibility_tree: true,
        requires_same_origin_frame: true,
        maximum_tree_nodes: 5_000,
        supported_operations: &AccessibilityOperation::ALL,
    }
}

/// Conservative six-platform manifest. No native provider is installed by this
/// crate, including Web.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AccessibilityProviderManifest {
    pub platform: AccessibilityPlatform,
    pub bridge: AssistiveBridgeKind,
    pub native_provider_installed: bool,
    pub evidence: Evidence,
}

pub const ACCESSIBILITY_PROVIDER_MANIFEST: [AccessibilityProviderManifest; 6] = [
    AccessibilityProviderManifest {
        platform: AccessibilityPlatform::Windows,
        bridge: AssistiveBridgeKind::UiAutomation,
        native_provider_installed: false,
        evidence: Evidence::ContractOnly,
    },
    AccessibilityProviderManifest {
        platform: AccessibilityPlatform::MacOs,
        bridge: AssistiveBridgeKind::NsAccessibility,
        native_provider_installed: false,
        evidence: Evidence::ContractOnly,
    },
    AccessibilityProviderManifest {
        platform: AccessibilityPlatform::Linux,
        bridge: AssistiveBridgeKind::AtSpi,
        native_provider_installed: false,
        evidence: Evidence::ContractOnly,
    },
    AccessibilityProviderManifest {
        platform: AccessibilityPlatform::Android,
        bridge: AssistiveBridgeKind::TalkBack,
        native_provider_installed: false,
        evidence: Evidence::ContractOnly,
    },
    AccessibilityProviderManifest {
        platform: AccessibilityPlatform::Ios,
        bridge: AssistiveBridgeKind::VoiceOver,
        native_provider_installed: false,
        evidence: Evidence::ContractOnly,
    },
    AccessibilityProviderManifest {
        platform: AccessibilityPlatform::Web,
        bridge: AssistiveBridgeKind::BrowserAria,
        native_provider_installed: false,
        evidence: Evidence::ContractOnly,
    },
];

/// Canonical AccessKit revision and GPUI revision used by this crate.
pub const ACCESSKIT_VERSION: &str = "0.24.0";
pub const GPUI_REVISION: &str = "f8cfc7ed9e57503def9479138894f3139f5400cc";
