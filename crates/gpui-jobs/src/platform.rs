//! Six-platform × six-operation support policy.
//!
//! Every cell is [`Evidence::ContractOnly`]. No native provider is installed by
//! this crate, and a `Support::Native` policy row describes the intended native
//! mechanism rather than a compiled or runtime-qualified implementation.

use gpui_system::{Evidence, Support};
use serde::{Deserialize, Serialize};
use std::fmt;

/// The six targets covered by the jobs support matrix.
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

    pub fn operations(self) -> Vec<PlatformOperationSupport> {
        platform_operations(self)
    }
}

impl fmt::Display for PlatformTarget {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// The six durable and in-process job operations covered by the matrix.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JobsOperation {
    ScheduleDurableJob,
    CheckpointAndRecover,
    CancelAndInspectJob,
    EvaluateConstraints,
    RunBoundedCpuTask,
    RunBoundedIoTask,
}

impl JobsOperation {
    pub const ALL: [Self; 6] = [
        Self::ScheduleDurableJob,
        Self::CheckpointAndRecover,
        Self::CancelAndInspectJob,
        Self::EvaluateConstraints,
        Self::RunBoundedCpuTask,
        Self::RunBoundedIoTask,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ScheduleDurableJob => "schedule_durable_job",
            Self::CheckpointAndRecover => "checkpoint_and_recover",
            Self::CancelAndInspectJob => "cancel_and_inspect_job",
            Self::EvaluateConstraints => "evaluate_constraints",
            Self::RunBoundedCpuTask => "run_bounded_cpu_task",
            Self::RunBoundedIoTask => "run_bounded_io_task",
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::ScheduleDurableJob => "ScheduleDurableJob",
            Self::CheckpointAndRecover => "CheckpointAndRecover",
            Self::CancelAndInspectJob => "CancelAndInspectJob",
            Self::EvaluateConstraints => "EvaluateConstraints",
            Self::RunBoundedCpuTask => "RunBoundedCpuTask",
            Self::RunBoundedIoTask => "RunBoundedIoTask",
        }
    }

    /// True when the operation belongs to the in-process pools rather than the
    /// durable OS-scheduled provider.
    pub const fn is_in_process(self) -> bool {
        matches!(self, Self::RunBoundedCpuTask | Self::RunBoundedIoTask)
    }
}

impl fmt::Display for JobsOperation {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// One `(platform, operation)` policy row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlatformOperationSupport {
    pub platform: PlatformTarget,
    pub operation: JobsOperation,
    pub support: Support,
    pub required_host_configuration: &'static str,
    pub evidence: Evidence,
    pub notes: &'static str,
    pub qualification_links: &'static [&'static str],
}

impl PlatformOperationSupport {
    pub const fn new(
        platform: PlatformTarget,
        operation: JobsOperation,
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

const WINDOWS_LINKS: &[&str] = &["#8", "#19", "#21", "#22", "#43", "#54", "#55", "#69", "#70"];
const MACOS_LINKS: &[&str] = &["#8", "#19", "#21", "#22", "#43", "#54", "#55", "#69", "#70"];
const LINUX_LINKS: &[&str] = &[
    "#8", "#20", "#19", "#21", "#22", "#43", "#54", "#55", "#69", "#70",
];
const ANDROID_LINKS: &[&str] = &["#2", "#19", "#21", "#22", "#43", "#54", "#55", "#69", "#70"];
const IOS_LINKS: &[&str] = &["#9", "#19", "#21", "#22", "#43", "#54", "#55", "#69", "#70"];
const WEB_LINKS: &[&str] = &[
    "#18", "#19", "#21", "#22", "#43", "#54", "#55", "#69", "#70",
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

fn platform_support(platform: PlatformTarget, operation: JobsOperation) -> Support {
    match (platform, operation) {
        (PlatformTarget::Web, JobsOperation::ScheduleDurableJob) => Support::Conditional,
        (PlatformTarget::Web, JobsOperation::CheckpointAndRecover) => Support::Conditional,
        (PlatformTarget::Web, JobsOperation::CancelAndInspectJob) => Support::Conditional,
        (PlatformTarget::Web, JobsOperation::EvaluateConstraints) => Support::Conditional,
        (PlatformTarget::Web, JobsOperation::RunBoundedCpuTask) => Support::Conditional,
        (PlatformTarget::Web, JobsOperation::RunBoundedIoTask) => Support::Conditional,
        // The hosted platforms all have a native API, but the OS owns the
        // decision to run. A `Native` policy row is not a scheduling promise.
        (PlatformTarget::Windows, JobsOperation::ScheduleDurableJob) => Support::Native,
        (
            PlatformTarget::MacOs | PlatformTarget::Linux | PlatformTarget::Android,
            JobsOperation::ScheduleDurableJob,
        ) => Support::Conditional,
        (PlatformTarget::Ios, JobsOperation::ScheduleDurableJob) => Support::Conditional,
        (PlatformTarget::Android | PlatformTarget::Ios, JobsOperation::RunBoundedCpuTask) => {
            Support::Conditional
        }
        _ => Support::Native,
    }
}

fn platform_configuration(platform: PlatformTarget, operation: JobsOperation) -> &'static str {
    match (platform, operation) {
        (PlatformTarget::Windows, JobsOperation::ScheduleDurableJob) => {
            "Windows Background Tasks (BackgroundTaskBuilder + SystemTrigger/TimeTrigger) registered by a packaged host, or Task Scheduler 2.0 (ITaskService) for unpackaged agents. Declare the entry point and register the task in the app manifest."
        }
        (PlatformTarget::Windows, JobsOperation::CheckpointAndRecover) => {
            "Per-user private app data through SHGetKnownFolderPath(FOLDERID_LocalAppData); stage, FlushFileBuffers and MoveFileEx(REPLACE_EXISTING) for atomic replacement, with an application-owned quarantine directory."
        }
        (PlatformTarget::Windows, JobsOperation::CancelAndInspectJob) => {
            "Unregister the BackgroundTaskRegistration or delete the Task Scheduler task; retain the last committed job/checkpoint envelope until the host acknowledges cancellation."
        }
        (PlatformTarget::Windows, JobsOperation::EvaluateConstraints) => {
            "SystemConditionType for AC power, battery saver through SYSTEM_POWER_STATUS/SystemParametersInfo, thermal through GetSystemCpuSetInformation or host telemetry, and NetworkListManager cost/reachability."
        }
        (PlatformTarget::Windows, JobsOperation::RunBoundedCpuTask) => {
            "Bound work with the Windows thread pool (CreateThreadpoolWork) or a host executor; keep UI/Compositor capacity separate and never run blocking CPU work on the UI thread."
        }
        (PlatformTarget::Windows, JobsOperation::RunBoundedIoTask) => {
            "Use overlapped I/O or a bounded host executor with explicit cancellation; the process must remain responsive to suspend/terminate notifications."
        }
        (PlatformTarget::MacOs, JobsOperation::ScheduleDurableJob) => {
            "NSBackgroundActivityScheduler or a launchd user agent (LaunchAgent plist with StartInterval/StartCalendarInterval). The app must register the activity and tolerate coalescing, delays and skipped runs."
        }
        (PlatformTarget::MacOs, JobsOperation::CheckpointAndRecover) => {
            "Application Support under the app sandbox/container; write a temporary file in the same directory, fsync, then rename. Store a last-known-good slot and quarantine corrupt files."
        }
        (PlatformTarget::MacOs, JobsOperation::CancelAndInspectJob) => {
            "Invalidate NSBackgroundActivityScheduler activities or unload the launchd job; inspection reads the app-owned durable journal, never private scheduler state."
        }
        (PlatformTarget::MacOs, JobsOperation::EvaluateConstraints) => {
            "IOPSCopyPowerSourcesInfo / IOPSGetPowerSourceDescription for AC and battery, NSProcessInfo.isLowPowerModeEnabled for battery saver, ProcessInfo.thermalState for thermal, and NWPathMonitor for network cost."
        }
        (PlatformTarget::MacOs, JobsOperation::RunBoundedCpuTask) => {
            "Use DispatchQueue/QoS or a bounded host pool with a hard concurrency cap; never block the main run loop."
        }
        (PlatformTarget::MacOs, JobsOperation::RunBoundedIoTask) => {
            "Use URLSession background/foreground tasks or bounded DispatchQueue I/O with cancellation; background URLSession completion may arrive after relaunch."
        }
        (PlatformTarget::Linux, JobsOperation::ScheduleDurableJob) => {
            "systemd --user timers (OnCalendar/OnUnitActiveSec with Persistent=true) or the XDG Background Portal (org.freedesktop.portal.Background) for sandboxed applications. Hosts without systemd or a portal must fall back to an explicit unavailable provider."
        }
        (PlatformTarget::Linux, JobsOperation::CheckpointAndRecover) => {
            "XDG data directory (XDG_DATA_HOME or ~/.local/share/<app-id>); write, fsync, rename and fsync the directory. Respect the XDG background portal's private storage rules."
        }
        (PlatformTarget::Linux, JobsOperation::CancelAndInspectJob) => {
            "Stop and disable the systemd --user timer/unit or close the portal request; inspection uses app-owned checkpoint files and never shells out to systemctl with user input."
        }
        (PlatformTarget::Linux, JobsOperation::EvaluateConstraints) => {
            "UPower/Login1 D-Bus for AC, battery and low-power mode; thermal from the platform thermal zone; device idle from the session idle hint; network cost from NetworkManager Metered."
        }
        (PlatformTarget::Linux, JobsOperation::RunBoundedCpuTask) => {
            "Use a bounded host thread pool or Rayon-style pool with an explicit worker cap; do not compete with the compositor or the UI thread."
        }
        (PlatformTarget::Linux, JobsOperation::RunBoundedIoTask) => {
            "Use non-blocking/async I/O with a bounded queue and cancellation; never assume a writable home directory in a sandbox."
        }
        (PlatformTarget::Android, JobsOperation::ScheduleDurableJob) => {
            "WorkManager (PeriodicWorkRequest/OneTimeWorkRequest) with Constraints for network type, charging, battery-not-low, device idle and storage-not-low. WorkManager is opportunistic; enqueue is not an execution guarantee."
        }
        (PlatformTarget::Android, JobsOperation::CheckpointAndRecover) => {
            "App-private filesDir/noBackupFilesDir with atomic staging and rename; use WorkManager's own database only through its API and keep app checkpoints in a separate journal."
        }
        (PlatformTarget::Android, JobsOperation::CancelAndInspectJob) => {
            "WorkManager.cancelUniqueWork/cancelWorkById and observe WorkInfo; app inspection reads the encrypted-at-rest job journal, never private WorkManager rows."
        }
        (PlatformTarget::Android, JobsOperation::EvaluateConstraints) => {
            "ConnectivityManager NetworkCapabilities (VALIDATED, NOT_METERED, NOT_ROAMING, CAPTIVE_PORTAL), getRestrictBackgroundStatus for data saver, BatteryManager/IsCharging and PowerManager.isDeviceIdleMode, thermal status and StorageManager.getAllocatableBytes."
        }
        (PlatformTarget::Android, JobsOperation::RunBoundedCpuTask) => {
            "Use a bounded host executor/Dispatchers.Default with an explicit parallelism cap; background work must not starve the main looper."
        }
        (PlatformTarget::Android, JobsOperation::RunBoundedIoTask) => {
            "Use a bounded Dispatchers.IO/host executor (or a foreground service for user-visible long work) with cooperative cancellation."
        }
        (PlatformTarget::Ios, JobsOperation::ScheduleDurableJob) => {
            "BGTaskScheduler with a registered BGTaskSchedulerPermittedIdentifiers entry and BGProcessingTask/BGAppRefreshTask handlers; background URLSession for downloads. The system alone decides whether a task runs."
        }
        (PlatformTarget::Ios, JobsOperation::CheckpointAndRecover) => {
            "Application Support in the app container with atomic replace and complete file protection; exclude caches from backup. Background URLSession events may resume the app."
        }
        (PlatformTarget::Ios, JobsOperation::CancelAndInspectJob) => {
            "Cancel BGTaskScheduler pending task requests or invalidate URLSession tasks; inspection reads the app-owned journal through a background context."
        }
        (PlatformTarget::Ios, JobsOperation::EvaluateConstraints) => {
            "NWPathMonitor for reachability/cost, ProcessInfo.isLowPowerModeEnabled, ProcessInfo.thermalState and UIDevice battery state; BGTaskScheduler constraints are advisory and rechecked by the system."
        }
        (PlatformTarget::Ios, JobsOperation::RunBoundedCpuTask) => {
            "Use a bounded DispatchQueue/OperationQueue with QoS and a hard concurrency cap; respect background time limits and beginBackgroundTask where appropriate."
        }
        (PlatformTarget::Ios, JobsOperation::RunBoundedIoTask) => {
            "Use background URLSession or bounded async I/O; completion may arrive while the app is suspended or relaunched."
        }
        (PlatformTarget::Web, JobsOperation::ScheduleDurableJob) => {
            "Capability-gated Service Worker with the Background Sync API (SyncManager) or Periodic Background Sync. Both are optional and unavailable in several browsers; without them return Unavailable and use an in-process bounded task instead."
        }
        (PlatformTarget::Web, JobsOperation::CheckpointAndRecover) => {
            "Origin Private File System (OPFS) or IndexedDB with a schema version and integrity digest; storage may be evicted unless navigator.storage.persist() is granted. Use two committed slots."
        }
        (PlatformTarget::Web, JobsOperation::CancelAndInspectJob) => {
            "Unregister the Background Sync tag or stop the Service Worker task; inspection reads only app-owned IndexedDB/OPFS records."
        }
        (PlatformTarget::Web, JobsOperation::EvaluateConstraints) => {
            "navigator.onLine plus online/offline events are coarse hints; Network Information API and Battery Status API are optional or removed. Report unknown rather than assuming nominal power or unmetered network."
        }
        (PlatformTarget::Web, JobsOperation::RunBoundedCpuTask) => {
            "Web Workers with a host-owned pool size bound; there is no guaranteed minimum parallelism, and workers may be terminated when the page is frozen."
        }
        (PlatformTarget::Web, JobsOperation::RunBoundedIoTask) => {
            "fetch/OPFS/IndexedDB with AbortController and a bounded queue; browser storage and network policy remain authoritative."
        }
    }
}

fn platform_notes(platform: PlatformTarget, operation: JobsOperation) -> &'static str {
    match (platform, operation) {
        (PlatformTarget::Web, JobsOperation::ScheduleDurableJob) => {
            "No Service Worker/Background Sync means the operation is honestly unavailable; a page timer is not durable background execution."
        }
        (PlatformTarget::Linux, JobsOperation::ScheduleDurableJob) => {
            "systemd user timers are not available in every sandbox; XDG Background Portal support varies by desktop and host configuration."
        }
        (PlatformTarget::Android, JobsOperation::ScheduleDurableJob) => {
            "WorkManager deduplicates unique work and may run late; the app must treat the receipt as advisory and re-evaluate constraints when woken."
        }
        (PlatformTarget::Ios, JobsOperation::ScheduleDurableJob) => {
            "BGTaskScheduler may never invoke a task, especially under Low Power Mode; the app must not promise wall-clock execution."
        }
        (_, JobsOperation::ScheduleDurableJob) => {
            "OS scheduling is opportunistic: a lower bound is advisory only. Duplicate wakeups and time-slice expiry are normal and are fenced by run_epoch."
        }
        (_, JobsOperation::CheckpointAndRecover) => {
            "A checkpoint is committed only after the staging file is synced and renamed. A corrupt newest slot must fall back to the previous committed slot and quarantine the damaged file."
        }
        (_, JobsOperation::CancelAndInspectJob) => {
            "Cancellation is terminal for the current run epoch. A late completion or checkpoint from a previous epoch fails closed and cannot resurrect the job."
        }
        (_, JobsOperation::EvaluateConstraints) => {
            "Unknown observations are not nominal. Missing power or network provenance keeps a constrained job pending instead of guessing."
        }
        (_, JobsOperation::RunBoundedCpuTask) => {
            "In-process CPU work is never an OS-scheduled durable job. Pools have independent concurrency and queue bounds so UI capacity is not starved."
        }
        (_, JobsOperation::RunBoundedIoTask) => {
            "In-process I/O work is never durable by itself. Durable cross-process jobs go through the provider boundary and app-data checkpoints."
        }
    }
}

/// The complete six-target, six-operation support matrix (36 cells).
pub fn platform_support_matrix() -> Vec<PlatformOperationSupport> {
    let mut rows = Vec::with_capacity(36);
    for platform in PlatformTarget::ALL {
        for operation in JobsOperation::ALL {
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
    operation: JobsOperation,
) -> PlatformOperationSupport {
    platform_support_matrix()
        .into_iter()
        .find(|row| row.platform == platform && row.operation == operation)
        .expect("the support matrix contains every platform/operation pair")
}

/// All six operation rows for one platform in declaration order.
pub fn platform_operations(platform: PlatformTarget) -> Vec<PlatformOperationSupport> {
    JobsOperation::ALL
        .into_iter()
        .map(|operation| operation_support(platform, operation))
        .collect()
}

/// Maturity of the browser backend.
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

/// Browser background capability policy. Absent APIs fail closed rather than
/// falling back to an in-page timer that is not durable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WebJobsPolicy {
    pub maturity: WebBackendMaturity,
    pub backend_name: &'static str,
    pub service_worker_required: bool,
    pub background_sync_optional: bool,
    pub durable_storage_available: bool,
    pub in_process_workers_available: bool,
    pub schedule_durable_job: Support,
    pub checkpoint_and_recover: Support,
    pub run_bounded_cpu_task: Support,
    pub run_bounded_io_task: Support,
}

impl WebJobsPolicy {
    pub const fn support_for(self, operation: JobsOperation) -> Support {
        match operation {
            JobsOperation::ScheduleDurableJob => self.schedule_durable_job,
            JobsOperation::CheckpointAndRecover => self.checkpoint_and_recover,
            JobsOperation::CancelAndInspectJob => Support::Conditional,
            JobsOperation::EvaluateConstraints => Support::Conditional,
            JobsOperation::RunBoundedCpuTask => self.run_bounded_cpu_task,
            JobsOperation::RunBoundedIoTask => self.run_bounded_io_task,
        }
    }

    pub const fn supports(self, operation: JobsOperation) -> bool {
        matches!(
            self.support_for(operation),
            Support::Native | Support::Conditional
        )
    }
}

/// The browser policy aligned with platform reality.
pub fn web_capability_policy(maturity: WebBackendMaturity) -> WebJobsPolicy {
    WebJobsPolicy {
        maturity,
        backend_name: "Service Worker Background Sync",
        service_worker_required: true,
        background_sync_optional: true,
        durable_storage_available: true,
        in_process_workers_available: true,
        schedule_durable_job: Support::Conditional,
        checkpoint_and_recover: Support::Conditional,
        run_bounded_cpu_task: Support::Conditional,
        run_bounded_io_task: Support::Conditional,
    }
}
