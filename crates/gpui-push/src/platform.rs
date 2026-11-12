//! Six-platform × six-operation push support policy.
//!
//! Every cell is [`Evidence::ContractOnly`]. No native push provider is
//! installed by this crate, and a `Support::Native` or `Support::Conditional`
//! policy row describes the intended host mechanism rather than a compiled or
//! runtime-qualified implementation.
//!
//! `gpui_system::Platform` models the five hosted platforms; push also has to
//! describe the browser, so this matrix uses its own six-variant
//! [`PushPlatform`], matching the sibling `gpui-jobs`/`gpui-network` convention.

use crate::validation::WebPushHostState;
use gpui_system::{Evidence, Support};
use serde::{Deserialize, Serialize};
use std::fmt;

/// The six targets covered by the push support matrix.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PushPlatform {
    Windows,
    MacOs,
    Linux,
    Android,
    Ios,
    Web,
}

impl PushPlatform {
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

    pub fn operations(self) -> Vec<PushOperationSupport> {
        platform_operations(self)
    }
}

impl fmt::Display for PushPlatform {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// The six push operations covered by the matrix.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PushOperation {
    /// Enumerate provider/channel descriptors and their configuration/health.
    DiscoverPushProviders,
    /// Register a device, rotate its token and record the sync revision.
    RegisterAndRotateToken,
    /// Bind an opaque account and add or revoke topic subscriptions.
    BindAccountAndTopics,
    /// Accept a silent data push and hand bounded work to the host.
    ReceiveSilentDataPush,
    /// Present a local notification through the permission-gated presenter.
    PresentPushNotification,
    /// Route a user activation through the activation handoff boundary.
    RoutePushActivation,
}

impl PushOperation {
    pub const ALL: [Self; 6] = [
        Self::DiscoverPushProviders,
        Self::RegisterAndRotateToken,
        Self::BindAccountAndTopics,
        Self::ReceiveSilentDataPush,
        Self::PresentPushNotification,
        Self::RoutePushActivation,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::DiscoverPushProviders => "discover_push_providers",
            Self::RegisterAndRotateToken => "register_and_rotate_token",
            Self::BindAccountAndTopics => "bind_account_and_topics",
            Self::ReceiveSilentDataPush => "receive_silent_data_push",
            Self::PresentPushNotification => "present_push_notification",
            Self::RoutePushActivation => "route_push_activation",
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::DiscoverPushProviders => "DiscoverPushProviders",
            Self::RegisterAndRotateToken => "RegisterAndRotateToken",
            Self::BindAccountAndTopics => "BindAccountAndTopics",
            Self::ReceiveSilentDataPush => "ReceiveSilentDataPush",
            Self::PresentPushNotification => "PresentPushNotification",
            Self::RoutePushActivation => "RoutePushActivation",
        }
    }

    /// True when the operation needs a device credential.
    pub const fn requires_device_credentials(self) -> bool {
        matches!(
            self,
            Self::RegisterAndRotateToken | Self::ReceiveSilentDataPush
        )
    }

    /// True when the operation needs notification authorization.
    pub const fn requires_notification_permission(self) -> bool {
        matches!(
            self,
            Self::PresentPushNotification | Self::RoutePushActivation
        )
    }
}

impl fmt::Display for PushOperation {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// One `(platform, operation)` policy row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PushOperationSupport {
    pub platform: PushPlatform,
    pub operation: PushOperation,
    pub support: Support,
    pub required_host_configuration: &'static str,
    pub evidence: Evidence,
    pub notes: &'static str,
    pub qualification_links: &'static [&'static str],
}

impl PushOperationSupport {
    pub const fn new(
        platform: PushPlatform,
        operation: PushOperation,
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

const WINDOWS_LINKS: &[&str] = &[
    "#8", "#19", "#20", "#21", "#22", "#24", "#26", "#27", "#43", "#45", "#46", "#54", "#70", "#74",
];
const MACOS_LINKS: &[&str] = &[
    "#8", "#9", "#19", "#20", "#21", "#22", "#24", "#26", "#27", "#43", "#45", "#46", "#54", "#70",
    "#74",
];
const LINUX_LINKS: &[&str] = &[
    "#8", "#19", "#20", "#21", "#22", "#24", "#26", "#27", "#43", "#45", "#46", "#54", "#70", "#74",
];
const ANDROID_LINKS: &[&str] = &[
    "#2", "#19", "#20", "#21", "#22", "#24", "#26", "#27", "#43", "#45", "#46", "#54", "#70", "#74",
];
const IOS_LINKS: &[&str] = &[
    "#9", "#19", "#20", "#21", "#22", "#24", "#26", "#27", "#43", "#45", "#46", "#54", "#70", "#74",
];
const WEB_LINKS: &[&str] = &[
    "#18", "#19", "#20", "#21", "#22", "#24", "#26", "#27", "#43", "#45", "#46", "#54", "#70",
    "#74",
];

fn platform_links(platform: PushPlatform) -> &'static [&'static str] {
    match platform {
        PushPlatform::Windows => WINDOWS_LINKS,
        PushPlatform::MacOs => MACOS_LINKS,
        PushPlatform::Linux => LINUX_LINKS,
        PushPlatform::Android => ANDROID_LINKS,
        PushPlatform::Ios => IOS_LINKS,
        PushPlatform::Web => WEB_LINKS,
    }
}

/// Account/topic binding is application state. It needs no OS push transport, so
/// every platform reports `Native` for it while remaining `ContractOnly`.
fn platform_support(platform: PushPlatform, operation: PushOperation) -> Support {
    match (platform, operation) {
        (_, PushOperation::BindAccountAndTopics) => Support::Native,
        _ => Support::Conditional,
    }
}

fn platform_configuration(platform: PushPlatform, operation: PushOperation) -> &'static str {
    match (platform, operation) {
        (PushPlatform::Windows, PushOperation::DiscoverPushProviders) => {
            "A packaged MSIX identity with a declared WNS channel, plus a host adapter exposing channel configuration and reachability. Unpackaged processes have no WNS channel and must report Unavailable."
        }
        (PushPlatform::Windows, PushOperation::RegisterAndRotateToken) => {
            "WNS channel registration through the Push Notification API (PushNotificationChannelManager), refreshed before expiry. The channel URI is a device credential and belongs in gpui-secrets, never in settings."
        }
        (PushPlatform::Windows, PushOperation::BindAccountAndTopics) => {
            "Application-owned account and topic state. No OS API is required; the host server owns account-to-channel association."
        }
        (PushPlatform::Windows, PushOperation::ReceiveSilentDataPush) => {
            "A PushNotificationTrigger background task registered in the package manifest, or the foreground PushNotificationReceived event. The OS may throttle or drop background triggers."
        }
        (PushPlatform::Windows, PushOperation::PresentPushNotification) => {
            "The Windows notification platform with a manifest-declared toast capability, an application identifier and a notification channel. Focus assist and user settings may suppress display."
        }
        (PushPlatform::Windows, PushOperation::RoutePushActivation) => {
            "AppLifecycle activation (OnLaunched/ActivatedEventArgs) or the toast notification Activated callback, forwarded to a single-instance host before any UI surface exists."
        }
        (PushPlatform::MacOs, PushOperation::DiscoverPushProviders) => {
            "A signed bundle with the aps-environment entitlement and a host adapter reporting APNs environment and reachability. An unsigned or unentitled bundle must report Unavailable."
        }
        (PushPlatform::MacOs, PushOperation::RegisterAndRotateToken) => {
            "NSApplication.registerForRemoteNotifications plus the APNs device token in didRegisterForRemoteNotificationsWithDeviceToken. The token is a device credential and belongs in gpui-secrets."
        }
        (PushPlatform::MacOs, PushOperation::BindAccountAndTopics) => {
            "Application-owned account and topic state. APNs topics are bundle-derived and must not be confused with user subscriptions."
        }
        (PushPlatform::MacOs, PushOperation::ReceiveSilentDataPush) => {
            "The UNUserNotificationCenter delegate or NSApplicationDelegate remote-notification callback with content-available. The system decides whether a silent push wakes the process."
        }
        (PushPlatform::MacOs, PushOperation::PresentPushNotification) => {
            "UserNotifications with an authorized UNUserNotificationCenter, a bundle identifier and a notification category. Authorization state is re-probed at execution time."
        }
        (PushPlatform::MacOs, PushOperation::RoutePushActivation) => {
            "The UNUserNotificationCenter response handler or application:didFinishLaunchingWithOptions: with a remote-notification key, forwarded to the activation inbox before a window exists."
        }
        (PushPlatform::Linux, PushOperation::DiscoverPushProviders) => {
            "A host-installed transport such as a D-Bus notification daemon bridge, a self-hosted WebSocket channel or a distro push service. No transport is assumed; a missing one reports Unavailable."
        }
        (PushPlatform::Linux, PushOperation::RegisterAndRotateToken) => {
            "A host-provided transport token obtained from the desktop integration. Flatpak and Snap sandboxes restrict background sockets, so registration must be probed rather than assumed."
        }
        (PushPlatform::Linux, PushOperation::BindAccountAndTopics) => {
            "Application-owned account and topic state. No OS topic API exists on Linux."
        }
        (PushPlatform::Linux, PushOperation::ReceiveSilentDataPush) => {
            "A systemd user service, a D-Bus activation or a host scheduler (gpui-jobs #54) delivering the frame. A page or process-local timer is not a durable wake mechanism."
        }
        (PushPlatform::Linux, PushOperation::PresentPushNotification) => {
            "org.freedesktop.Notifications over D-Bus with a desktop entry, an application icon and a notification server. Headless sessions have no server and must report Unavailable."
        }
        (PushPlatform::Linux, PushOperation::RoutePushActivation) => {
            "The org.freedesktop.Notifications ActionInvoked/NotificationClosed signals or a host deep-link handler, forwarded to the activation inbox."
        }
        (PushPlatform::Android, PushOperation::DiscoverPushProviders) => {
            "A Firebase project with a google-services.json host build input and a manifest-declared FirebaseMessagingService. The service-account JSON used by the server is never a client input."
        }
        (PushPlatform::Android, PushOperation::RegisterAndRotateToken) => {
            "FirebaseMessaging.getInstance().token plus onNewToken refresh. The FCM registration token is a device credential and belongs in gpui-secrets; the sender ID is a public identifier."
        }
        (PushPlatform::Android, PushOperation::BindAccountAndTopics) => {
            "Application-owned account state. FCM topic subscription via FirebaseMessaging.subscribeToTopic is a transport hint, not an authorization boundary."
        }
        (PushPlatform::Android, PushOperation::ReceiveSilentDataPush) => {
            "A high-priority data message to FirebaseMessagingService.onMessageReceived. Doze, App Standby buckets and OEM battery policy may delay or drop it; no exact delivery is promised."
        }
        (PushPlatform::Android, PushOperation::PresentPushNotification) => {
            "NotificationManagerCompat with a created NotificationChannel and the POST_NOTIFICATIONS runtime permission on API 33+. Denied permission must suppress honestly."
        }
        (PushPlatform::Android, PushOperation::RoutePushActivation) => {
            "The notification content PendingIntent or the launcher activity's intent extras, forwarded after cold launch to the activation inbox before Compose/GPUI attaches."
        }
        (PushPlatform::Ios, PushOperation::DiscoverPushProviders) => {
            "A provisioning profile with the aps-environment entitlement and a host adapter reporting the APNs environment. Simulators and unentitled builds must report Unavailable."
        }
        (PushPlatform::Ios, PushOperation::RegisterAndRotateToken) => {
            "UIApplication.registerForRemoteNotifications and didRegisterForRemoteNotificationsWithDeviceToken. APNs may issue a new token at any launch; rotation is normal."
        }
        (PushPlatform::Ios, PushOperation::BindAccountAndTopics) => {
            "Application-owned account state. APNs does not provide per-topic subscription APIs."
        }
        (PushPlatform::Ios, PushOperation::ReceiveSilentDataPush) => {
            "application(_:didReceiveRemoteNotification:fetchCompletionHandler:) with content-available. iOS budgets background time and may coalesce or drop silent pushes."
        }
        (PushPlatform::Ios, PushOperation::PresentPushNotification) => {
            "UNUserNotificationCenter with provisional or explicit authorization, an alert category and a bundle identifier. Denied or not-determined status suppresses presentation."
        }
        (PushPlatform::Ios, PushOperation::RoutePushActivation) => {
            "UNUserNotificationCenterDelegate response handling or launch options carrying a remote notification, forwarded before the first GPUI surface attaches."
        }
        (PushPlatform::Web, PushOperation::DiscoverPushProviders) => {
            "A secure context with a registered Service Worker exposing PushManager, plus a VAPID application-server public key. Missing PushManager or a non-HTTPS origin reports Unavailable."
        }
        (PushPlatform::Web, PushOperation::RegisterAndRotateToken) => {
            "ServiceWorkerRegistration.pushManager.subscribe with the public VAPID key, stored as an endpoint capability URL plus p256dh/auth secrets. A private VAPID key must never reach the client."
        }
        (PushPlatform::Web, PushOperation::BindAccountAndTopics) => {
            "Application-owned account state. Web Push has no topic subscription API; topics are server-side routing labels."
        }
        (PushPlatform::Web, PushOperation::ReceiveSilentDataPush) => {
            "The Service Worker push event with a userVisibleOnly subscription. Payload encryption requires the subscription keys; browsers may terminate the worker at any time."
        }
        (PushPlatform::Web, PushOperation::PresentPushNotification) => {
            "ServiceWorkerRegistration.showNotification with the Notification API, an active permission grant and an icon/badge. Permission prompts require a user gesture."
        }
        (PushPlatform::Web, PushOperation::RoutePushActivation) => {
            "The Service Worker notificationclick event, focused or opened client, and a same-origin route. Cross-origin navigation must be rejected by the host."
        }
    }
}

fn platform_notes(platform: PushPlatform, operation: PushOperation) -> &'static str {
    match (platform, operation) {
        (_, PushOperation::DiscoverPushProviders) => {
            "Discovery reports configuration and health as observed. A provider that is unconfigured, unavailable or in outage is never selected silently."
        }
        (_, PushOperation::RegisterAndRotateToken) => {
            "The raw token is a credential. Only a SHA-256 fingerprint prefix and the monotonic token revision may appear in state, events or errors."
        }
        (_, PushOperation::BindAccountAndTopics) => {
            "Binding is local application state; the server association is updated by an explicit sync revision. Account switches advance the registration epoch."
        }
        (_, PushOperation::ReceiveSilentDataPush) => {
            "Delivery is opportunistic. Duplicate, delayed, reordered and dropped frames are normal and are fenced by message identity and registration epoch."
        }
        (_, PushOperation::PresentPushNotification) => {
            "A presentation receipt is not proof of display. Permission status is re-probed before the presenter is called."
        }
        (_, PushOperation::RoutePushActivation) => {
            "Activation is user-initiated only. A cold-launch activation is queued until a UI surface attaches and is re-fenced against the current account and epoch."
        }
    }
}

/// The complete six-target, six-operation support matrix (36 cells).
pub fn platform_support_matrix() -> Vec<PushOperationSupport> {
    let mut rows = Vec::with_capacity(36);
    for platform in PushPlatform::ALL {
        for operation in PushOperation::ALL {
            rows.push(PushOperationSupport::new(
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
pub fn operation_support(platform: PushPlatform, operation: PushOperation) -> PushOperationSupport {
    platform_support_matrix()
        .into_iter()
        .find(|row| row.platform == platform && row.operation == operation)
        .expect("the support matrix contains every platform/operation pair")
}

/// All six operation rows for one platform in declaration order.
pub fn platform_operations(platform: PushPlatform) -> Vec<PushOperationSupport> {
    PushOperation::ALL
        .into_iter()
        .map(|operation| operation_support(platform, operation))
        .collect()
}

/// Maturity of the browser push backend.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WebPushBackendMaturity {
    Experimental,
    Preview,
    Stable,
}

impl WebPushBackendMaturity {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Experimental => "experimental",
            Self::Preview => "preview",
            Self::Stable => "stable",
        }
    }
}

/// Browser push capability policy. Every requirement is mandatory: a missing
/// secure context, Service Worker, `PushManager`, `Notification` API or VAPID
/// public key fails closed rather than degrading to a page-local heuristic.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WebPushCapabilityPolicy {
    pub maturity: WebPushBackendMaturity,
    pub backend_name: &'static str,
    pub secure_context_required: bool,
    pub service_worker_required: bool,
    pub push_manager_required: bool,
    pub notification_api_required: bool,
    pub vapid_public_key_required: bool,
    pub discover_push_providers: Support,
    pub register_and_rotate_token: Support,
    pub receive_silent_data_push: Support,
    pub present_push_notification: Support,
    pub route_push_activation: Support,
}

impl WebPushCapabilityPolicy {
    pub const fn support_for(self, operation: PushOperation) -> Support {
        match operation {
            PushOperation::DiscoverPushProviders => self.discover_push_providers,
            PushOperation::RegisterAndRotateToken => self.register_and_rotate_token,
            PushOperation::BindAccountAndTopics => Support::Native,
            PushOperation::ReceiveSilentDataPush => self.receive_silent_data_push,
            PushOperation::PresentPushNotification => self.present_push_notification,
            PushOperation::RoutePushActivation => self.route_push_activation,
        }
    }

    pub const fn supports(self, operation: PushOperation) -> bool {
        matches!(
            self.support_for(operation),
            Support::Native | Support::Conditional
        )
    }

    /// Evaluate every mandatory browser requirement against a host observation.
    pub const fn is_available(self, host: WebPushHostState) -> bool {
        (!self.secure_context_required || host.secure_context)
            && (!self.service_worker_required || host.service_worker_registered)
            && (!self.push_manager_required || host.push_manager_supported)
            && (!self.notification_api_required || host.notification_api_supported)
            && (!self.vapid_public_key_required || host.vapid_public_key_configured)
    }

    /// The first unmet requirement, for an honest typed failure.
    pub const fn unavailable_reason(self, host: WebPushHostState) -> Option<&'static str> {
        if self.secure_context_required && !host.secure_context {
            return Some("Web Push requires an HTTPS secure context");
        }
        if self.service_worker_required && !host.service_worker_registered {
            return Some("Web Push requires a registered Service Worker");
        }
        if self.push_manager_required && !host.push_manager_supported {
            return Some("This browser does not expose PushManager");
        }
        if self.notification_api_required && !host.notification_api_supported {
            return Some("This browser does not expose the Notification API");
        }
        if self.vapid_public_key_required && !host.vapid_public_key_configured {
            return Some("Web Push requires a VAPID application-server public key");
        }
        None
    }
}

/// The browser policy aligned with platform reality.
pub fn web_push_capability_policy(maturity: WebPushBackendMaturity) -> WebPushCapabilityPolicy {
    WebPushCapabilityPolicy {
        maturity,
        backend_name: "Service Worker Web Push (VAPID)",
        secure_context_required: true,
        service_worker_required: true,
        push_manager_required: true,
        notification_api_required: true,
        vapid_public_key_required: true,
        discover_push_providers: Support::Conditional,
        register_and_rotate_token: Support::Conditional,
        receive_silent_data_push: Support::Conditional,
        present_push_notification: Support::Conditional,
        route_push_activation: Support::Conditional,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matrix_covers_every_pair_exactly_once() {
        let matrix = platform_support_matrix();
        assert_eq!(matrix.len(), 36);
        for platform in PushPlatform::ALL {
            for operation in PushOperation::ALL {
                assert_eq!(
                    matrix
                        .iter()
                        .filter(|row| row.platform == platform && row.operation == operation)
                        .count(),
                    1
                );
            }
        }
        assert_eq!(platform_operations(PushPlatform::Web).len(), 6);
    }

    #[test]
    fn browser_policy_fails_closed_per_requirement() {
        let policy = web_push_capability_policy(WebPushBackendMaturity::Preview);
        let complete = WebPushHostState {
            secure_context: true,
            service_worker_registered: true,
            push_manager_supported: true,
            notification_api_supported: true,
            vapid_public_key_configured: true,
        };
        assert!(policy.is_available(complete));
        assert_eq!(policy.unavailable_reason(complete), None);
        let insecure = WebPushHostState {
            secure_context: false,
            ..complete
        };
        assert!(!policy.is_available(insecure));
        assert!(policy.unavailable_reason(insecure).is_some());
    }
}
