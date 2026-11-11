//! Six-platform × six-operation notification-scheduling support policy.
//!
//! Every cell is [`Evidence::ContractOnly`]. No native provider is installed by
//! this crate. `Native` or `Conditional` describes the intended host mechanism,
//! not compiled or runtime-qualified evidence.

use gpui_system::{Evidence, Support};
use serde::{Deserialize, Serialize};
use std::fmt;

/// The six targets covered by the notification-schedule matrix.
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

    /// Bridge to the five-variant `gpui-system` platform vocabulary. Web has no
    /// native system platform.
    pub const fn native_platform(self) -> Option<gpui_system::Platform> {
        Some(match self {
            Self::Windows => gpui_system::Platform::Windows,
            Self::MacOs => gpui_system::Platform::MacOs,
            Self::Linux => gpui_system::Platform::Linux,
            Self::Android => gpui_system::Platform::Android,
            Self::Ios => gpui_system::Platform::Ios,
            Self::Web => return None,
        })
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

    pub fn operations(self) -> Vec<PlatformOperationSupport> {
        platform_operations(self)
    }
}

impl fmt::Display for PlatformTarget {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// The six scheduled-notification operations covered by the matrix.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScheduleOperation {
    ScheduleNotification,
    ScheduleExactAlarm,
    RepeatAndCalendarSchedule,
    AttachMediaToSchedule,
    ObserveScheduledDelivery,
    ReconcilePendingSchedules,
}

impl ScheduleOperation {
    pub const ALL: [Self; 6] = [
        Self::ScheduleNotification,
        Self::ScheduleExactAlarm,
        Self::RepeatAndCalendarSchedule,
        Self::AttachMediaToSchedule,
        Self::ObserveScheduledDelivery,
        Self::ReconcilePendingSchedules,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ScheduleNotification => "schedule_notification",
            Self::ScheduleExactAlarm => "schedule_exact_alarm",
            Self::RepeatAndCalendarSchedule => "repeat_and_calendar_schedule",
            Self::AttachMediaToSchedule => "attach_media_to_schedule",
            Self::ObserveScheduledDelivery => "observe_scheduled_delivery",
            Self::ReconcilePendingSchedules => "reconcile_pending_schedules",
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::ScheduleNotification => "ScheduleNotification",
            Self::ScheduleExactAlarm => "ScheduleExactAlarm",
            Self::RepeatAndCalendarSchedule => "RepeatAndCalendarSchedule",
            Self::AttachMediaToSchedule => "AttachMediaToSchedule",
            Self::ObserveScheduledDelivery => "ObserveScheduledDelivery",
            Self::ReconcilePendingSchedules => "ReconcilePendingSchedules",
        }
    }
}

impl fmt::Display for ScheduleOperation {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// One `(platform, operation)` policy row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlatformOperationSupport {
    pub platform: PlatformTarget,
    pub operation: ScheduleOperation,
    pub support: Support,
    pub required_host_configuration: &'static str,
    pub evidence: Evidence,
    pub notes: &'static str,
    pub qualification_links: &'static [&'static str],
}

impl PlatformOperationSupport {
    pub const fn new(
        platform: PlatformTarget,
        operation: ScheduleOperation,
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
    "#8", "#19", "#20", "#21", "#22", "#24", "#26", "#43", "#44", "#54", "#70",
];
const MACOS_LINKS: &[&str] = &[
    "#8", "#19", "#20", "#21", "#22", "#24", "#26", "#43", "#44", "#54", "#70",
];
const LINUX_LINKS: &[&str] = &[
    "#8", "#19", "#20", "#21", "#22", "#24", "#26", "#43", "#44", "#54", "#70",
];
const ANDROID_LINKS: &[&str] = &[
    "#2", "#19", "#20", "#21", "#22", "#24", "#26", "#43", "#44", "#54", "#70",
];
const IOS_LINKS: &[&str] = &[
    "#9", "#19", "#20", "#21", "#22", "#24", "#26", "#43", "#44", "#54", "#70",
];
const WEB_LINKS: &[&str] = &[
    "#19", "#20", "#21", "#22", "#24", "#26", "#43", "#44", "#54", "#70",
];

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

fn platform_support(platform: PlatformTarget, operation: ScheduleOperation) -> Support {
    match (platform, operation) {
        (PlatformTarget::Web, ScheduleOperation::ScheduleNotification) => Support::Conditional,
        (PlatformTarget::Web, ScheduleOperation::ScheduleExactAlarm) => Support::Unsupported,
        (PlatformTarget::Web, ScheduleOperation::RepeatAndCalendarSchedule) => Support::Conditional,
        (PlatformTarget::Web, ScheduleOperation::AttachMediaToSchedule) => Support::Conditional,
        (PlatformTarget::Web, ScheduleOperation::ObserveScheduledDelivery) => Support::Unsupported,
        (PlatformTarget::Web, ScheduleOperation::ReconcilePendingSchedules) => Support::Conditional,
        (PlatformTarget::Android, ScheduleOperation::ScheduleExactAlarm) => Support::Conditional,
        (
            PlatformTarget::Windows
            | PlatformTarget::MacOs
            | PlatformTarget::Linux
            | PlatformTarget::Ios,
            ScheduleOperation::ScheduleExactAlarm,
        ) => Support::Unsupported,
        (_, ScheduleOperation::ObserveScheduledDelivery) if platform == PlatformTarget::Linux => {
            Support::Unsupported
        }
        (_, ScheduleOperation::ScheduleNotification) => Support::Native,
        (
            PlatformTarget::Linux | PlatformTarget::Android | PlatformTarget::Ios,
            ScheduleOperation::RepeatAndCalendarSchedule | ScheduleOperation::AttachMediaToSchedule,
        ) => Support::Conditional,
        (PlatformTarget::Linux, ScheduleOperation::ReconcilePendingSchedules) => {
            Support::Conditional
        }
        (PlatformTarget::Android, ScheduleOperation::ReconcilePendingSchedules) => {
            Support::Conditional
        }
        (_, ScheduleOperation::RepeatAndCalendarSchedule) => Support::Conditional,
        (_, ScheduleOperation::AttachMediaToSchedule) => Support::Conditional,
        (_, ScheduleOperation::ObserveScheduledDelivery) => Support::Conditional,
        (_, ScheduleOperation::ReconcilePendingSchedules) => Support::Native,
    }
}

fn platform_configuration(platform: PlatformTarget, operation: ScheduleOperation) -> &'static str {
    match (platform, operation) {
        (PlatformTarget::Windows, ScheduleOperation::ScheduleNotification) => {
            "Windows.UI.Notifications.ToastNotificationManager / ToastNotifier.AddToSchedule with a ScheduledToastNotification. A packaged AUMID or a registered AppUserModelID plus shortcut is required. Scheduled toasts remain subject to Focus Assist, notification settings and delivery best-effort, and deliver only while the notification platform is available."
        }
        (PlatformTarget::Windows, ScheduleOperation::ScheduleExactAlarm) => {
            "Windows has no exact-alarm notification API. ScheduledToastNotification carries an absolute DeliveryTime but the OS may defer, coalesce or drop it; toast priority and expiration do not create an exact-alarm guarantee. This contract reports Unsupported and requires downgrade to InexactAdvisory or rejection."
        }
        (PlatformTarget::Windows, ScheduleOperation::RepeatAndCalendarSchedule) => {
            "ScheduledToastNotification supports one absolute delivery time (and a bounded snooze/recurrence interval on some builds), not a civil calendar or DST engine. The host must evaluate the calendar rule and re-register the next occurrence after each presentation."
        }
        (PlatformTarget::Windows, ScheduleOperation::AttachMediaToSchedule) => {
            "Adaptive toast XML image sources accept ms-appx:/// and file:/// URIs plus http(s) URLs that the platform may fetch. This contract accepts only private local files and bundled assets; remote images must be cached by the host first. Bound image count and pixel/byte size at the host."
        }
        (PlatformTarget::Windows, ScheduleOperation::ObserveScheduledDelivery) => {
            "ToastNotifier does not report presentation. UserNotificationListener (a restricted capability) or the notification history can report shown/dismissed/activated events when permission is granted; without that listener presentation evidence is unavailable and must not be synthesized."
        }
        (PlatformTarget::Windows, ScheduleOperation::ReconcilePendingSchedules) => {
            "ToastNotifier.GetScheduledToastNotifications enumerates pending scheduled toasts and RemoveFromSchedule withdraws them. The API exposes no epoch, so reconcile by the app-owned tag/group/id and re-add any journal record missing from the OS list."
        }
        (PlatformTarget::MacOs, ScheduleOperation::ScheduleNotification) => {
            "UNUserNotificationCenter.add(_:withCompletionHandler:) with a UNTimeIntervalNotificationTrigger or UNCalendarNotificationTrigger. Requires a bundled app identity and notification authorization. The center may defer or coalesce delivery; the provider receipt is not a display guarantee."
        }
        (PlatformTarget::MacOs, ScheduleOperation::ScheduleExactAlarm) => {
            "There is no public exact-alarm notification API. UNTimeIntervalNotificationTrigger is system-scheduled but may be delayed; critical alerts require a separate entitlement that this contract does not assume. Exact alarms are Unsupported and require downgrade or rejection."
        }
        (PlatformTarget::MacOs, ScheduleOperation::RepeatAndCalendarSchedule) => {
            "UNCalendarNotificationTrigger with DateComponents supports simple component repeats, but not arbitrary multi-field rules, leap-day semantics, DST gap/fold policies or max-occurrence budgets. The host evaluates occurrences and re-registers; the center caps pending requests at 64."
        }
        (PlatformTarget::MacOs, ScheduleOperation::AttachMediaToSchedule) => {
            "UNNotificationAttachment accepts a file URL in the app container; the system copies the file into the notification store. Remote URLs must be downloaded by the host first. Supported UTIs, dimensions and sizes are system-defined and can be rejected."
        }
        (PlatformTarget::MacOs, ScheduleOperation::ObserveScheduledDelivery) => {
            "UNUserNotificationCenterDelegate willPresent/didReceive report foreground presentation and user activation, not background delivery. There is no delivery-receipt API; ObserveScheduledDelivery is Conditional and must stay separate from submission."
        }
        (PlatformTarget::MacOs, ScheduleOperation::ReconcilePendingSchedules) => {
            "getPendingNotificationRequests enumerates pending requests up to the 64-request cap; removePendingNotificationRequests(withIdentifiers:) withdraws them. Re-add journal records that are missing or have a newer epoch, and prune terminal records first."
        }
        (PlatformTarget::Linux, ScheduleOperation::ScheduleNotification) => {
            "There is no portable desktop notification scheduler. org.freedesktop.Notifications Notify() is immediate and session daemons normally do not persist across logout. A systemd --user timer or the XDG Background Portal can wake the app to call Notify(), but neither is guaranteed in a sandbox."
        }
        (PlatformTarget::Linux, ScheduleOperation::ScheduleExactAlarm) => {
            "No standard exact-alarm notification API. A systemd --user OnCalendar timer with a tight AccuracySec is the closest mechanism when systemd is available, but it is not a notification guarantee and many sandboxes lack user timers. Exact alarms are Unsupported."
        }
        (PlatformTarget::Linux, ScheduleOperation::RepeatAndCalendarSchedule) => {
            "systemd --user OnCalendar expressions can express calendar repeats, but DST behavior and accuracy are systemd/glibc-defined and not portable. The host must evaluate this crate's rule and re-register; the XDG Background Portal may forbid persistent timers."
        }
        (PlatformTarget::Linux, ScheduleOperation::AttachMediaToSchedule) => {
            "The freedesktop notification protocol carries image-data/image-path hints whose support varies by server. Only readable private local files are portable; remote URLs are not. Attachment support is Conditional and must fail closed when the server rejects the hint."
        }
        (PlatformTarget::Linux, ScheduleOperation::ObserveScheduledDelivery) => {
            "org.freedesktop.Notifications exposes NotificationClosed and ActionInvoked signals, but no display/presentation confirmation, and events may be lost after logout. There is no durable delivery observation; this row is Unsupported and presentation must not be inferred."
        }
        (PlatformTarget::Linux, ScheduleOperation::ReconcilePendingSchedules) => {
            "There is no standard enumeration API for pending desktop notifications. Reconcile app-owned systemd units or XDG portal requests and never enumerate arbitrary session state. Host configuration is required and the row is Conditional."
        }
        (PlatformTarget::Android, ScheduleOperation::ScheduleNotification) => {
            "NotificationManagerCompat.notify posts immediately; future delivery needs AlarmManager (setExactAndAllowWhileIdle/setAndAllowWhileIdle) or WorkManager to wake the app. POST_NOTIFICATIONS is a runtime permission on API 33+, and a NotificationChannel is required on API 26+."
        }
        (PlatformTarget::Android, ScheduleOperation::ScheduleExactAlarm) => {
            "AlarmManager.setExactAndAllowWhileIdle or setAlarmClock, guarded by canScheduleExactAlarms(). SCHEDULE_EXACT_ALARM or USE_EXACT_ALARM special access is required on API 31+. Doze, App Standby and OEM policy may still defer delivery; the receipt is not a wall-clock promise."
        }
        (PlatformTarget::Android, ScheduleOperation::RepeatAndCalendarSchedule) => {
            "AlarmManager setRepeating is inexact and exact alarms must be re-registered per occurrence. WorkManager PeriodicWorkRequest has a 15-minute floor and is opportunistic. Civil calendar/DST rules require app-side evaluation; Chromium-style multi-transition zones are not handled by AlarmManager."
        }
        (PlatformTarget::Android, ScheduleOperation::AttachMediaToSchedule) => {
            "NotificationCompat.Builder large icons and BigPicture/BigText styles accept content:// URIs with a FileProvider grant or private file paths. Remote http(s) URIs are not accepted by this contract. Notification and channel limits still apply."
        }
        (PlatformTarget::Android, ScheduleOperation::ObserveScheduledDelivery) => {
            "A posted notification has no delivery callback. NotificationListenerService can observe posted/removed notifications only with a user-granted special access; otherwise presentation is unobservable. Activation arrives through PendingIntent, not through a delivery receipt."
        }
        (PlatformTarget::Android, ScheduleOperation::ReconcilePendingSchedules) => {
            "AlarmManager has no public enumeration API. Persist app-owned alarm identities and cancel/re-register by PendingIntent identity; WorkManager can enumerate unique work by name/tag. Reconciliation is journal-driven and Conditional."
        }
        (PlatformTarget::Ios, ScheduleOperation::ScheduleNotification) => {
            "UNUserNotificationCenter.add with UNTimeIntervalNotificationTrigger or UNCalendarNotificationTrigger. Requires a bundle identity and notification authorization. The system decides actual presentation; pending requests are capped at 64."
        }
        (PlatformTarget::Ios, ScheduleOperation::ScheduleExactAlarm) => {
            "There is no exact-alarm notification API and critical alerts require a separate entitlement that this contract does not assume. UNTimeIntervalNotificationTrigger is system-scheduled but not guaranteed to the second. Exact alarms are Unsupported."
        }
        (PlatformTarget::Ios, ScheduleOperation::RepeatAndCalendarSchedule) => {
            "UNCalendarNotificationTrigger with DateComponents supports simple repeats, but not arbitrary multi-field rules, DST gap/fold policy or occurrence budgets. Evaluate and re-register; respect the 64-pending-request cap."
        }
        (PlatformTarget::Ios, ScheduleOperation::AttachMediaToSchedule) => {
            "UNNotificationAttachment requires a file URL in the app container and the system copies it into the notification store. Remote URLs must be downloaded first; supported UTIs and size are system-defined."
        }
        (PlatformTarget::Ios, ScheduleOperation::ObserveScheduledDelivery) => {
            "UNUserNotificationCenterDelegate reports willPresent (foreground) and didReceive (activation) only. There is no background delivery receipt; presentation observation is Conditional and remains separate from activation."
        }
        (PlatformTarget::Ios, ScheduleOperation::ReconcilePendingSchedules) => {
            "getPendingNotificationRequests enumerates pending requests up to the system cap; removePendingNotificationRequests(withIdentifiers:) withdraws. Re-add missing journal records and prune terminal records to stay under 64."
        }
        (PlatformTarget::Web, ScheduleOperation::ScheduleNotification) => {
            "ServiceWorkerRegistration.showNotification with a TimestampTrigger when the non-baseline Notification Triggers API is present. Without it, a page or worker timer is not durable background scheduling and the operation must report Unavailable. Notification permission must be granted."
        }
        (PlatformTarget::Web, ScheduleOperation::ScheduleExactAlarm) => {
            "The Web Notifications API offers no exact-alarm guarantee. TimestampTrigger is best-effort, can be delayed or dropped, and is unavailable in several browsers. Exact alarms are Unsupported on the Web contract."
        }
        (PlatformTarget::Web, ScheduleOperation::RepeatAndCalendarSchedule) => {
            "There is no standard repeating or civil-calendar notification trigger. A Service Worker with Periodic Background Sync may wake opportunistically, but that API is non-baseline, throttled and permission-gated. The app must compute each occurrence and re-register a trigger."
        }
        (PlatformTarget::Web, ScheduleOperation::AttachMediaToSchedule) => {
            "NotificationOptions icon/badge/image accept same-origin URLs; cross-origin images and local filesystem paths are not portable. Fetch and cache bytes first, honor CORS/opaque-response rules, and cap count and size in the host."
        }
        (PlatformTarget::Web, ScheduleOperation::ObserveScheduledDelivery) => {
            "There is no display or delivery receipt. Notification click and close events are interaction evidence, not presentation evidence. Notification Triggers exposes no observation API; this row is Unsupported."
        }
        (PlatformTarget::Web, ScheduleOperation::ReconcilePendingSchedules) => {
            "ServiceWorkerRegistration.getNotifications() can enumerate notifications shown by that registration where supported, but there is no durable scheduled-trigger enumeration. Reconcile from app-owned IndexedDB/OPFS records and re-register triggers; storage may be evicted."
        }
    }
}

fn platform_notes(platform: PlatformTarget, operation: ScheduleOperation) -> &'static str {
    match (platform, operation) {
        (_, ScheduleOperation::ScheduleNotification) => {
            "A provider receipt is submission evidence only. It does not prove display, and a later callback must be recorded as a separate presentation observation."
        }
        (_, ScheduleOperation::ScheduleExactAlarm) => {
            "Exact alarms require both provider capability and the exact-alarm special-access permission. When either is missing, downgrade explicitly to InexactAdvisory or fail closed; never claim exact precision."
        }
        (_, ScheduleOperation::RepeatAndCalendarSchedule) => {
            "Polling and page timers are not durable. Repeating/calendar schedules must be evaluated app-side and re-registered; every provider remains authoritative for actual delivery."
        }
        (_, ScheduleOperation::AttachMediaToSchedule) => {
            "Only bounded private local files and bundled assets are accepted. Remote URLs and traversal are rejected before any provider call."
        }
        (PlatformTarget::Linux, ScheduleOperation::ObserveScheduledDelivery)
        | (PlatformTarget::Web, ScheduleOperation::ObserveScheduledDelivery) => {
            "No portable presentation callback exists; submission must never be upgraded to presentation evidence by default."
        }
        (_, ScheduleOperation::ObserveScheduledDelivery) => {
            "Observed presentation requires an explicit platform callback and the ObserveScheduledDelivery capability. Missing observation is Unknown, never Presented."
        }
        (PlatformTarget::Windows, ScheduleOperation::ReconcilePendingSchedules) => {
            "GetScheduledToastNotifications is authoritative only for this app's scheduled toasts and has no epoch; reconcile by tag and re-add missing journal records."
        }
        (_, ScheduleOperation::ReconcilePendingSchedules) => {
            "Reconciliation compares the durable journal with provider.pending_schedules(), re-submits missing or stale-epoch registrations, and cancels orphaned OS registrations. Clock jumps coalesce missed occurrences."
        }
    }
}

/// The complete six-target, six-operation support matrix (36 cells).
pub fn platform_support_matrix() -> Vec<PlatformOperationSupport> {
    let mut rows = Vec::with_capacity(36);
    for platform in PlatformTarget::ALL {
        for operation in ScheduleOperation::ALL {
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
    operation: ScheduleOperation,
) -> PlatformOperationSupport {
    platform_support_matrix()
        .into_iter()
        .find(|row| row.platform == platform && row.operation == operation)
        .expect("the support matrix contains every platform/operation pair")
}

/// All six operation rows for one platform in declaration order.
pub fn platform_operations(platform: PlatformTarget) -> Vec<PlatformOperationSupport> {
    ScheduleOperation::ALL
        .into_iter()
        .map(|operation| operation_support(platform, operation))
        .collect()
}

/// Maturity of the browser notification backend.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WebBackendMaturity {
    Experimental,
    Preview,
    Stable,
}

impl WebBackendMaturity {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Experimental => "experimental",
            Self::Preview => "preview",
            Self::Stable => "stable",
        }
    }
}

/// Browser schedule policy. Notification Triggers is non-baseline, so an absent
/// or unknown trigger API fails closed rather than falling back to a page timer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WebNotificationSchedulePolicy {
    pub maturity: WebBackendMaturity,
    pub backend_name: &'static str,
    pub service_worker_required: bool,
    pub notification_triggers_optional: bool,
    pub notification_triggers_baseline: bool,
    pub scheduled_delivery_observable: bool,
    pub exact_alarm_supported: bool,
    pub schedule_notification: Support,
    pub schedule_exact_alarm: Support,
    pub repeat_and_calendar_schedule: Support,
    pub attach_media_to_schedule: Support,
    pub observe_scheduled_delivery: Support,
    pub reconcile_pending_schedules: Support,
}

impl WebNotificationSchedulePolicy {
    pub const fn support_for(self, operation: ScheduleOperation) -> Support {
        match operation {
            ScheduleOperation::ScheduleNotification => self.schedule_notification,
            ScheduleOperation::ScheduleExactAlarm => self.schedule_exact_alarm,
            ScheduleOperation::RepeatAndCalendarSchedule => self.repeat_and_calendar_schedule,
            ScheduleOperation::AttachMediaToSchedule => self.attach_media_to_schedule,
            ScheduleOperation::ObserveScheduledDelivery => self.observe_scheduled_delivery,
            ScheduleOperation::ReconcilePendingSchedules => self.reconcile_pending_schedules,
        }
    }

    pub const fn supports(self, operation: ScheduleOperation) -> bool {
        matches!(
            self.support_for(operation),
            Support::Native | Support::Conditional
        )
    }
}

/// The browser policy aligned with platform reality.
pub fn web_capability_policy(maturity: WebBackendMaturity) -> WebNotificationSchedulePolicy {
    WebNotificationSchedulePolicy {
        maturity,
        backend_name: "Service Worker showNotification + Notification Triggers",
        service_worker_required: true,
        notification_triggers_optional: true,
        notification_triggers_baseline: false,
        scheduled_delivery_observable: false,
        exact_alarm_supported: false,
        schedule_notification: Support::Conditional,
        schedule_exact_alarm: Support::Unsupported,
        repeat_and_calendar_schedule: Support::Conditional,
        attach_media_to_schedule: Support::Conditional,
        observe_scheduled_delivery: Support::Unsupported,
        reconcile_pending_schedules: Support::Conditional,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    #[test]
    fn matrix_is_exactly_six_by_six_contract_only() {
        let matrix = platform_support_matrix();
        assert_eq!(matrix.len(), 36);
        let mut pairs = BTreeSet::new();
        for row in &matrix {
            assert!(pairs.insert((row.platform, row.operation)));
            assert_eq!(row.evidence, Evidence::ContractOnly);
            assert!(!row.required_host_configuration.is_empty());
            assert!(!row.notes.is_empty());
            assert!(row.has_link("#44"));
            assert!(row.has_link("#26"));
            assert!(row.has_link("#20"));
            assert!(row.has_link("#19"));
            assert!(row.has_link("#22"));
            assert!(row.has_link("#70"));
        }
        assert_eq!(pairs.len(), 36);
        assert!(
            operation_support(PlatformTarget::Web, ScheduleOperation::ScheduleExactAlarm)
                .has_link("#70")
        );
        assert_eq!(
            operation_support(
                PlatformTarget::Android,
                ScheduleOperation::ScheduleExactAlarm
            )
            .support,
            Support::Conditional
        );
        assert!(
            operation_support(
                PlatformTarget::Android,
                ScheduleOperation::ScheduleExactAlarm
            )
            .has_link("#2")
        );
        assert!(
            operation_support(PlatformTarget::Ios, ScheduleOperation::ScheduleNotification)
                .has_link("#9")
        );
        assert!(
            operation_support(
                PlatformTarget::Windows,
                ScheduleOperation::ScheduleNotification
            )
            .has_link("#8")
        );
    }

    #[test]
    fn web_policy_is_explicit_about_non_baseline_triggers() {
        let policy = web_capability_policy(WebBackendMaturity::Preview);
        assert!(policy.service_worker_required);
        assert!(policy.notification_triggers_optional);
        assert!(!policy.notification_triggers_baseline);
        assert!(!policy.exact_alarm_supported);
        assert!(!policy.scheduled_delivery_observable);
        assert!(policy.supports(ScheduleOperation::ScheduleNotification));
        assert!(!policy.supports(ScheduleOperation::ScheduleExactAlarm));
        assert!(policy.supports(ScheduleOperation::RepeatAndCalendarSchedule));
    }
}
