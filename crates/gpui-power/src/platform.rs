//! Six-platform power/session/thermal support policy and browser wake-lock
//! rules.
//!
//! These rows are a contract, not runtime proof. No native provider is installed
//! by this crate, so every row is [`Evidence::ContractOnly`]. Native adapters
//! must publish their own qualification evidence before flipping that.

use crate::PowerOperation;
use gpui_system::{Evidence, Support};
use serde::{Deserialize, Serialize};
use std::fmt;

/// The six targets covered by the power support matrix.
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
    pub operation: PowerOperation,
    pub support: Support,
    pub required_host_configuration: &'static str,
    pub evidence: Evidence,
    pub notes: &'static str,
    pub qualification_links: &'static [&'static str],
}

impl PlatformOperationSupport {
    pub const fn new(
        platform: PlatformTarget,
        operation: PowerOperation,
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

const WINDOWS_LINKS: &[&str] = &["#2", "#8", "#19", "#21", "#43"];
const MACOS_LINKS: &[&str] = &["#8", "#19", "#21", "#43"];
const LINUX_LINKS: &[&str] = &["#8", "#19", "#21", "#43"];
const ANDROID_LINKS: &[&str] = &["#2", "#19", "#20", "#21", "#43"];
const IOS_LINKS: &[&str] = &["#9", "#19", "#21", "#43"];
const WEB_LINKS: &[&str] = &["#18", "#19", "#20", "#21", "#43"];

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

fn platform_support(platform: PlatformTarget, operation: PowerOperation) -> Support {
    match (platform, operation) {
        (PlatformTarget::Windows, PowerOperation::ObserveThermalMemory) => Support::Conditional,
        (
            PlatformTarget::Linux,
            PowerOperation::ObservePower
            | PowerOperation::ObserveSession
            | PowerOperation::ObserveThermalMemory
            | PowerOperation::InhibitSystemSleep
            | PowerOperation::InhibitDisplaySleep,
        ) => Support::Conditional,
        (PlatformTarget::Android, PowerOperation::ObserveSession) => Support::Conditional,
        (PlatformTarget::Android, PowerOperation::ObserveThermalMemory) => Support::Conditional,
        (PlatformTarget::Android, PowerOperation::InhibitSystemSleep) => Support::Conditional,
        (PlatformTarget::Android, PowerOperation::InhibitDisplaySleep) => Support::Conditional,
        (PlatformTarget::Ios, PowerOperation::InhibitSystemSleep) => Support::Unsupported,
        (PlatformTarget::Ios, PowerOperation::InhibitDisplaySleep) => Support::Conditional,
        (PlatformTarget::Web, PowerOperation::ObservePower) => Support::Conditional,
        (PlatformTarget::Web, PowerOperation::ObserveSession) => Support::Conditional,
        (PlatformTarget::Web, PowerOperation::ObserveThermalMemory) => Support::Unsupported,
        (PlatformTarget::Web, PowerOperation::InhibitSystemSleep) => Support::Unsupported,
        (PlatformTarget::Web, PowerOperation::InhibitDisplaySleep) => Support::Conditional,
        _ => Support::Native,
    }
}

fn platform_configuration(platform: PlatformTarget, operation: PowerOperation) -> &'static str {
    match (platform, operation) {
        (PlatformTarget::Windows, PowerOperation::ObservePower) => {
            "Enable GetSystemPowerStatus and RegisterPowerSettingNotification for GUID_ACDC_POWER_SOURCE, GUID_BATTERY_PERCENTAGE_REMAINING and GUID_POWER_SAVING_STATUS on a message-pumping UI thread."
        }
        (PlatformTarget::Windows, PowerOperation::ObserveSession) => {
            "Register WTSRegisterSessionNotification (WM_WTSSESSION_CHANGE, WTS_SESSION_LOCK/WTS_SESSION_UNLOCK) and PowerRegisterSuspendResumeNotification (WM_POWERBROADCAST, PBT_APMSUSPEND/PBT_APMRESUMEAUTOMATIC)."
        }
        (PlatformTarget::Windows, PowerOperation::ObserveThermalMemory) => {
            "Call GlobalMemoryStatusEx / CreateMemoryResourceNotification for memory. Thermal zones are OEM/hardware-conditional; report SignalSource::UnsupportedByPlatform and ThermalCondition::Unknown when absent."
        }
        (PlatformTarget::Windows, PowerOperation::InhibitSystemSleep) => {
            "Create a PowerCreateRequest handle and call PowerSetRequest(PowerRequestSystemRequired) with a public-safe reason, or use thread-bound SetThreadExecutionState(ES_CONTINUOUS | ES_SYSTEM_REQUIRED)."
        }
        (PlatformTarget::Windows, PowerOperation::InhibitDisplaySleep) => {
            "PowerCreateRequest + PowerSetRequest(PowerRequestDisplayRequired), or SetThreadExecutionState(ES_CONTINUOUS | ES_SYSTEM_REQUIRED | ES_DISPLAY_REQUIRED)."
        }
        (PlatformTarget::MacOs, PowerOperation::ObservePower) => {
            "IOPSCopyPowerSourcesInfo (IOKit.ps) and NSProcessInfo.processInfo.isLowPowerModeEnabled with NSProcessInfoPowerStateDidChangeNotification."
        }
        (PlatformTarget::MacOs, PowerOperation::ObserveSession) => {
            "Observe NSWorkspace screensDidSleepNotification, willSleepNotification and didWakeNotification plus session/screen lock notifications on the main run loop."
        }
        (PlatformTarget::MacOs, PowerOperation::ObserveThermalMemory) => {
            "NSProcessInfo.processInfo.thermalState with NSProcessInfoThermalStateDidChangeNotification and dispatch_source_create(DISPATCH_SOURCE_TYPE_MEMORYPRESSURE, ...)."
        }
        (PlatformTarget::MacOs, PowerOperation::InhibitSystemSleep) => {
            "IOPMAssertionCreateWithName(kIOPMAssertionTypePreventUserIdleSystemSleep, ...) with a bounded reason string."
        }
        (PlatformTarget::MacOs, PowerOperation::InhibitDisplaySleep) => {
            "IOPMAssertionCreateWithName(kIOPMAssertionTypePreventUserIdleDisplaySleep, ...) with a bounded reason string."
        }
        (PlatformTarget::Linux, PowerOperation::ObservePower) => {
            "UPower D-Bus (org.freedesktop.UPower, DisplayDevice) and net.hadess.PowerProfiles / org.freedesktop.UPower.PowerProfiles for profile and low-power state."
        }
        (PlatformTarget::Linux, PowerOperation::ObserveSession) => {
            "systemd-logind org.freedesktop.login1.Manager PrepareForSleep and org.freedesktop.login1.Session Lock/Unlock/LockedHint; use org.freedesktop.portal.Inhibit for sandboxed sessions."
        }
        (PlatformTarget::Linux, PowerOperation::ObserveThermalMemory) => {
            "Read /proc/pressure/memory PSI and org.freedesktop.LowMemoryMonitor; sysfs thermal zones are conditional across distributions and containers."
        }
        (PlatformTarget::Linux, PowerOperation::InhibitSystemSleep) => {
            "Take a systemd-logind Inhibit(\"sleep:idle\", ...) fd lock or call org.freedesktop.portal.Inhibit with the Idle flag (4)."
        }
        (PlatformTarget::Linux, PowerOperation::InhibitDisplaySleep) => {
            "Use org.freedesktop.ScreenSaver.Inhibit, org.freedesktop.portal.Inhibit (Idle + Suspend flags), or zwp_idle_inhibit_manager_v1 bound to a visible wl_surface."
        }
        (PlatformTarget::Android, PowerOperation::ObservePower) => {
            "BatteryManager ACTION_BATTERY_CHANGED and PowerManager.isPowerSaveMode with ACTION_POWER_SAVE_MODE_CHANGED."
        }
        (PlatformTarget::Android, PowerOperation::ObserveSession) => {
            "KeyguardManager.isDeviceLocked, ACTION_SCREEN_OFF / ACTION_USER_PRESENT, Activity/Process lifecycle (#21) and ACTION_DEVICE_IDLE_MODE_CHANGED for Doze."
        }
        (PlatformTarget::Android, PowerOperation::ObserveThermalMemory) => {
            "PowerManager.addThermalStatusListener (API 29+) and ComponentCallbacks2.onTrimMemory."
        }
        (PlatformTarget::Android, PowerOperation::InhibitSystemSleep) => {
            "PARTIAL_WAKE_LOCK requires android.permission.WAKE_LOCK in AndroidManifest.xml and is subject to Doze, App Standby and battery-restriction revocations (#20, #43)."
        }
        (PlatformTarget::Android, PowerOperation::InhibitDisplaySleep) => {
            "Attach FLAG_KEEP_SCREEN_ON to a visible foreground Activity/Surface; the platform makes it ineffective when backgrounded or locked."
        }
        (PlatformTarget::Ios, PowerOperation::ObservePower) => {
            "UIDevice.current.isBatteryMonitoringEnabled with batteryLevel/batteryState and ProcessInfo.processInfo.isLowPowerModeEnabled with NSProcessInfoPowerStateDidChange."
        }
        (PlatformTarget::Ios, PowerOperation::ObserveSession) => {
            "Observe UIApplicationProtectedDataWillBecomeUnavailable / DidBecomeAvailable and UIScene lifecycle (#21)."
        }
        (PlatformTarget::Ios, PowerOperation::ObserveThermalMemory) => {
            "ProcessInfo.processInfo.thermalState with thermalStateDidChangeNotification and UIApplicationDidReceiveMemoryWarningNotification / os_proc_available_memory."
        }
        (PlatformTarget::Ios, PowerOperation::InhibitSystemSleep) => {
            "Unsupported on iOS: there is no general-purpose app API to prevent background system idle sleep. Background execution requires narrow task-specific background modes or BGTaskScheduler under #43."
        }
        (PlatformTarget::Ios, PowerOperation::InhibitDisplaySleep) => {
            "UIApplication.shared.isIdleTimerDisabled = true only in a foreground active scene; reset it on background/lock. Low Power Mode can shorten or override auto-lock."
        }
        (PlatformTarget::Web, PowerOperation::ObservePower) => {
            "navigator.getBattery() (W3C Battery Status) is Chromium-only in secure contexts; Safari and Firefox must report SignalSource::UnsupportedByPlatform with PowerSource::Unknown rather than assuming AC."
        }
        (PlatformTarget::Web, PowerOperation::ObserveSession) => {
            "The Page Visibility API observes tab visibility, not OS screen lock; report SessionLockState::Unknown with SignalSource::UnsupportedByPlatform."
        }
        (PlatformTarget::Web, PowerOperation::ObserveThermalMemory) => {
            "Compute Pressure and performance.memory are non-standard or experimental; report both domains as Unknown with SignalSource::UnsupportedByPlatform."
        }
        (PlatformTarget::Web, PowerOperation::InhibitSystemSleep) => {
            "Unsupported: navigator.wakeLock only supports the 'screen' type; the Web platform has no system wake lock."
        }
        (PlatformTarget::Web, PowerOperation::InhibitDisplaySleep) => {
            "navigator.wakeLock.request('screen') requires HTTPS (window.isSecureContext), Permissions-Policy: screen-wake-lock, and document.visibilityState === \"visible\"."
        }
    }
}

fn platform_notes(platform: PlatformTarget, operation: PowerOperation) -> &'static str {
    match (platform, operation) {
        (_, PowerOperation::ObservePower) => {
            "AC, battery, UPS and USB are observations only; an unavailable collector stays Unknown, never AC."
        }
        (_, PowerOperation::ObserveSession) => {
            "Session lock and tab visibility are distinct signals; a reported foreground tab does not prove an unlocked OS session."
        }
        (PlatformTarget::Windows, PowerOperation::ObserveThermalMemory) => {
            "Memory is native; thermal sensing is conditional on hardware/OEM thermal zones."
        }
        (PlatformTarget::Linux, PowerOperation::ObserveThermalMemory) => {
            "PSI availability varies across kernels, containers and sandboxes; unobservable domains report UnsupportedByPlatform or ProviderUnavailable."
        }
        (PlatformTarget::Android, PowerOperation::ObserveThermalMemory) => {
            "Thermal status requires API 29+; older hosts report UnsupportedByPlatform."
        }
        (PlatformTarget::Ios, PowerOperation::InhibitSystemSleep) => {
            "No wake lock exists on iOS; do not simulate one with a background task or silent audio."
        }
        (PlatformTarget::Web, PowerOperation::InhibitDisplaySleep) => {
            "The browser releases the screen wake lock when the document becomes hidden; a request while hidden fails rather than reporting effective success."
        }
        (_, PowerOperation::ObserveThermalMemory) => {
            "Memory and thermal pressure are coarse advisory signals; Unknown is not Normal."
        }
        (_, PowerOperation::InhibitSystemSleep | PowerOperation::InhibitDisplaySleep) => {
            "A refuted or later-revoked native request never counts as an effective lease in this contract."
        }
    }
}

/// The complete six-target, five-operation support matrix (30 rows).
pub fn platform_support_matrix() -> Vec<PlatformOperationSupport> {
    let mut rows = Vec::with_capacity(30);
    for platform in PlatformTarget::ALL {
        for operation in PowerOperation::ALL {
            rows.push(PlatformOperationSupport::new(
                platform,
                operation,
                platform_support(platform, operation),
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
    operation: PowerOperation,
) -> PlatformOperationSupport {
    platform_support_matrix()
        .into_iter()
        .find(|row| row.platform == platform && row.operation == operation)
        .expect("the support matrix contains every platform/operation pair")
}

/// All five operation rows for one platform in declaration order.
pub fn platform_operations(platform: PlatformTarget) -> Vec<PlatformOperationSupport> {
    PowerOperation::ALL
        .into_iter()
        .map(|operation| operation_support(platform, operation))
        .collect()
}

/// Maturity of the browser backend.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WebBackendMaturity {
    Experimental,
}

impl WebBackendMaturity {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Experimental => "experimental",
        }
    }
}

/// Browser Screen Wake Lock policy.
///
/// The Web platform has no system wake lock. Only `navigator.wakeLock` with the
/// `'screen'` type is available, conditionally, and only for a visible document
/// in a secure context with the `screen-wake-lock` permission policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WebWakeLockPolicy {
    pub maturity: WebBackendMaturity,
    pub backend_name: &'static str,
    pub secure_context_required: bool,
    pub permissions_policy: &'static str,
    pub visible_document_required: bool,
    pub observe_power: Support,
    pub observe_session: Support,
    pub observe_thermal_memory: Support,
    pub inhibit_system_sleep: Support,
    pub inhibit_display_sleep: Support,
    pub system_wake_lock_available: bool,
    pub screen_wake_lock_type: &'static str,
    pub releases_on_visibility_hidden: bool,
    pub rejects_hidden_requests: bool,
    pub low_power_mode_overrides: bool,
}

impl WebWakeLockPolicy {
    pub const fn support_for(self, operation: PowerOperation) -> Support {
        match operation {
            PowerOperation::ObservePower => self.observe_power,
            PowerOperation::ObserveSession => self.observe_session,
            PowerOperation::ObserveThermalMemory => self.observe_thermal_memory,
            PowerOperation::InhibitSystemSleep => self.inhibit_system_sleep,
            PowerOperation::InhibitDisplaySleep => self.inhibit_display_sleep,
        }
    }

    pub const fn supports(self, operation: PowerOperation) -> bool {
        matches!(
            self.support_for(operation),
            Support::Native | Support::Conditional
        )
    }

    /// A display wake lock is effective only for a visible, secure document.
    pub const fn display_lease_effective(self, secure_context: bool, visible: bool) -> bool {
        secure_context && visible && self.supports(PowerOperation::InhibitDisplaySleep)
    }
}

/// The Web capability policy aligned with browser reality.
pub fn web_capability_policy() -> WebWakeLockPolicy {
    WebWakeLockPolicy {
        maturity: WebBackendMaturity::Experimental,
        backend_name: "navigator.wakeLock",
        secure_context_required: true,
        permissions_policy: "screen-wake-lock",
        visible_document_required: true,
        observe_power: Support::Conditional,
        observe_session: Support::Conditional,
        observe_thermal_memory: Support::Unsupported,
        inhibit_system_sleep: Support::Unsupported,
        inhibit_display_sleep: Support::Conditional,
        system_wake_lock_available: false,
        screen_wake_lock_type: "screen",
        releases_on_visibility_hidden: true,
        rejects_hidden_requests: true,
        low_power_mode_overrides: true,
    }
}
