//! Six-platform by six-operation rich transfer qualification policy.
//!
//! Every row is [`Evidence::ContractOnly`]: this crate ships no native adapter
//! and never claims runtime qualification. The notes describe the real OS
//! contract a future adapter must satisfy.

use gpui_system::{Evidence, Support};
use serde::{Deserialize, Serialize};
use std::fmt;

/// The six transfer targets covered by the qualification matrix.
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

    pub const fn is_desktop(self) -> bool {
        matches!(self, Self::Windows | Self::MacOs | Self::Linux)
    }

    pub const fn is_mobile_or_web(self) -> bool {
        matches!(self, Self::Android | Self::Ios | Self::Web)
    }

    pub fn support(self, operation: DndOperation) -> Support {
        support_for(self, operation)
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

/// The six coordinated rich-transfer operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DndOperation {
    ReadRichClipboard,
    WriteRichClipboard,
    ProvideDelayedPayload,
    StartDragSession,
    AcceptDropTarget,
    TransferFileResources,
}

impl DndOperation {
    pub const ALL: [Self; 6] = [
        Self::ReadRichClipboard,
        Self::WriteRichClipboard,
        Self::ProvideDelayedPayload,
        Self::StartDragSession,
        Self::AcceptDropTarget,
        Self::TransferFileResources,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ReadRichClipboard => "read_rich_clipboard",
            Self::WriteRichClipboard => "write_rich_clipboard",
            Self::ProvideDelayedPayload => "provide_delayed_payload",
            Self::StartDragSession => "start_drag_session",
            Self::AcceptDropTarget => "accept_drop_target",
            Self::TransferFileResources => "transfer_file_resources",
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::ReadRichClipboard => "Read rich clipboard",
            Self::WriteRichClipboard => "Write rich clipboard",
            Self::ProvideDelayedPayload => "Provide delayed payload",
            Self::StartDragSession => "Start drag session",
            Self::AcceptDropTarget => "Accept drop target",
            Self::TransferFileResources => "Transfer file resources",
        }
    }
}

impl fmt::Display for DndOperation {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// One `(platform, operation)` qualification row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlatformOperationSupport {
    pub platform: PlatformTarget,
    pub operation: DndOperation,
    pub support: Support,
    pub required_host_configuration: &'static str,
    pub evidence: Evidence,
    pub notes: &'static str,
    pub qualification_links: &'static [&'static str],
}

impl PlatformOperationSupport {
    pub const fn new(
        platform: PlatformTarget,
        operation: DndOperation,
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

const WINDOWS_LINKS: &[&str] = &["#52", "#19", "#20", "#21", "#23", "#25", "#43", "#8"];
const MACOS_LINKS: &[&str] = &["#52", "#19", "#20", "#21", "#23", "#25", "#43", "#8"];
const LINUX_LINKS: &[&str] = &["#52", "#19", "#20", "#21", "#23", "#25", "#43", "#8"];
const ANDROID_LINKS: &[&str] = &["#52", "#19", "#20", "#21", "#23", "#25", "#43", "#2"];
const IOS_LINKS: &[&str] = &["#52", "#19", "#20", "#21", "#23", "#25", "#43", "#9"];
const WEB_LINKS: &[&str] = &["#52", "#19", "#20", "#21", "#25", "#70"];

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

fn support_for(platform: PlatformTarget, operation: DndOperation) -> Support {
    match platform {
        PlatformTarget::Windows | PlatformTarget::MacOs => Support::Native,
        PlatformTarget::Linux => Support::Conditional,
        PlatformTarget::Android => match operation {
            DndOperation::ProvideDelayedPayload | DndOperation::TransferFileResources => {
                Support::Conditional
            }
            DndOperation::ReadRichClipboard
            | DndOperation::WriteRichClipboard
            | DndOperation::StartDragSession
            | DndOperation::AcceptDropTarget => Support::Native,
        },
        PlatformTarget::Ios => match operation {
            DndOperation::ReadRichClipboard | DndOperation::WriteRichClipboard => Support::Native,
            DndOperation::ProvideDelayedPayload
            | DndOperation::StartDragSession
            | DndOperation::AcceptDropTarget
            | DndOperation::TransferFileResources => Support::Conditional,
        },
        PlatformTarget::Web => match operation {
            DndOperation::ProvideDelayedPayload => Support::Unsupported,
            DndOperation::ReadRichClipboard
            | DndOperation::WriteRichClipboard
            | DndOperation::StartDragSession
            | DndOperation::AcceptDropTarget
            | DndOperation::TransferFileResources => Support::Conditional,
        },
    }
}

fn configuration_for(platform: PlatformTarget, operation: DndOperation) -> &'static str {
    match (platform, operation) {
        (PlatformTarget::Windows, DndOperation::ReadRichClipboard) => {
            "OpenClipboard with a live message-only or foreground HWND; enumerate and decode CF_UNICODETEXT, CF_HTML, RTF, CF_DIB/PNG and CF_HDROP/IShellItemArray; close the clipboard on every path."
        }
        (PlatformTarget::Windows, DndOperation::WriteRichClipboard) => {
            "EmptyClipboard then SetClipboardData for each eager format; register delayed formats with SetClipboardData(CF_xxx, NULL) and answer WM_RENDERFORMAT/WM_RENDERALLFORMATS on the owning thread."
        }
        (PlatformTarget::Windows, DndOperation::ProvideDelayedPayload) => {
            "Keep the clipboard owner window alive; WM_RENDERFORMAT and WM_RENDERALLFORMATS must read the still-live handoff, produce the promised bytes, and release producer FileResources on exit or clipboard replacement."
        }
        (PlatformTarget::Windows, DndOperation::StartDragSession) => {
            "Implement OLE IDataObject, IDropSource and DoDragDrop with CF_HDROP/IShellItemArray for resources and DROPEFFECT_COPY/MOVE/LINK; cancel on source-window destruction and revoke delayed callbacks on exit."
        }
        (PlatformTarget::Windows, DndOperation::AcceptDropTarget) => {
            "RegisterDragDrop on the target HWND, implement IDropTarget::DragEnter/DragOver/DragLeave/Drop, honor the OLE effect mask and call IDataObject::GetData outside the service's internal locks."
        }
        (PlatformTarget::Windows, DndOperation::TransferFileResources) => {
            "Wrap CF_HDROP or IShellItemArray handles in opaque FileResource values; never expose a path in public metadata or errors and hold the source data object alive through the drop."
        }

        (PlatformTarget::MacOs, DndOperation::ReadRichClipboard) => {
            "Read NSPasteboard.generalPasteboard on the main thread with canReadObjectForClasses/readObjectsForClasses for public.utf8-plain-text, public.html, public.rtf and image UTIs."
        }
        (PlatformTarget::MacOs, DndOperation::WriteRichClipboard) => {
            "Clear and write NSPasteboardItem values, or register an NSPasteboardItemDataProvider for delayed UTIs; keep the provider alive while the pasteboard owns it."
        }
        (PlatformTarget::MacOs, DndOperation::ProvideDelayedPayload) => {
            "NSPasteboardItemDataProvider pasteboard:item:provideDataForType: runs on the main thread; re-check handoff and surface liveness before producing bytes and release file promises on exit."
        }
        (PlatformTarget::MacOs, DndOperation::StartDragSession) => {
            "Start an NSDraggingSession from a view with NSDraggingSource, an NSPasteboardItem and a validated preview; implement sourceOperationMaskForLocal and draggingSession:endedAtPoint:operation:."
        }
        (PlatformTarget::MacOs, DndOperation::AcceptDropTarget) => {
            "Register NSDraggingDestination on the target view; performDragOperation must re-negotiate the pasteboard types and may run the custom evaluator outside service locks."
        }
        (PlatformTarget::MacOs, DndOperation::TransferFileResources) => {
            "Use NSFilePromiseProvider or security-scoped URLs wrapped in opaque FileResource values; keep the promise provider alive and release security scope when the consumer finishes."
        }

        (PlatformTarget::Linux, DndOperation::ReadRichClipboard) => {
            "Own or request the X11 CLIPBOARD selection / Wayland wl_data_device offer on the focused seat and transfer through a pipe or event-loop callback; clipboard-manager harvesting is not guaranteed."
        }
        (PlatformTarget::Linux, DndOperation::WriteRichClipboard) => {
            "Take selection ownership and advertise TARGETS/UTF8_STRING/text/html/image PNG; the owner must stay alive to answer SelectionRequest or the selection disappears when the source exits."
        }
        (PlatformTarget::Linux, DndOperation::ProvideDelayedPayload) => {
            "Delayed X11 selection or Wayland data-source callbacks require a live owner and, on Wayland, a focused seat plus the input serial that created the drag; there is no persistent OS callback after exit."
        }
        (PlatformTarget::Linux, DndOperation::StartDragSession) => {
            "X11 XdndEnter/Position/Leave/Drop with XdndSelection ownership, or Wayland wl_data_device.start_drag using the current input serial and an fd-backed source; cancel when focus or the seat disappears."
        }
        (PlatformTarget::Linux, DndOperation::AcceptDropTarget) => {
            "Advertise XdndAware / accept wl_data_device offers on the current surface; negotiate MIME targets and re-validate target generation at drop because leave/drop ordering is not atomic."
        }
        (PlatformTarget::Linux, DndOperation::TransferFileResources) => {
            "Use text/uri-list for X11/Wayland files and the XDG Desktop Portal FileTransfer interface for sandboxed cross-application transfers; never hand a raw path across a sandbox boundary."
        }

        (PlatformTarget::Android, DndOperation::ReadRichClipboard) => {
            "Use ClipboardManager on the UI thread with Android 10+ input-focus clipboard access; treat background or focus loss as PermissionDenied/Unavailable rather than retrying silently."
        }
        (PlatformTarget::Android, DndOperation::WriteRichClipboard) => {
            "Publish ClipData with text/html, text/plain and content:// URI items; grant only the minimum read permission and keep the Activity in the foreground while publishing."
        }
        (PlatformTarget::Android, DndOperation::ProvideDelayedPayload) => {
            "Conditional: implement a ContentProvider that serves promised bytes through an openFile/content:// stream grant and revoke the grant when the handoff, Activity or surface generation is disposed."
        }
        (PlatformTarget::Android, DndOperation::StartDragSession) => {
            "Call View.startDragAndDrop with DRAG_FLAG_GLOBAL and DRAG_FLAG_GLOBAL_URI_READ; keep the source view and ClipDescription alive for the duration of the drag."
        }
        (PlatformTarget::Android, DndOperation::AcceptDropTarget) => {
            "Implement View.OnDragListener, call requestDragAndDropPermissions for content:// payloads and release DragAndDropPermissions when the drop or surface generation ends."
        }
        (PlatformTarget::Android, DndOperation::TransferFileResources) => {
            "Conditional: expose files only through ContentProvider content:// URI grants or SAF documents; raw filesystem paths are never transferred and clip data must carry a readable URI grant."
        }

        (PlatformTarget::Ios, DndOperation::ReadRichClipboard) => {
            "Read UIPasteboard.general on the main thread; iOS 16+ may show a paste banner, so call from an explicit user action and honor UIPasteControl/gesture privacy rules."
        }
        (PlatformTarget::Ios, DndOperation::WriteRichClipboard) => {
            "Write UIPasteboard items with public.utf8-plain-text, public.html, public.rtf and image types; respect local-only and expiration policy where the host declares it."
        }
        (PlatformTarget::Ios, DndOperation::ProvideDelayedPayload) => {
            "Conditional: register NSItemProvider load handlers for promised UTIs; handlers may run off the main thread, must be thread-safe and are revoked when the source session or scene ends."
        }
        (PlatformTarget::Ios, DndOperation::StartDragSession) => {
            "Conditional: UIDragInteraction/UIDragItem with NSItemProvider on iPadOS multi-window; iPhone generally constrains cross-app drag to a single scene and background suspension cancels the session."
        }
        (PlatformTarget::Ios, DndOperation::AcceptDropTarget) => {
            "Conditional: UIDropInteraction/UIDropSession must load item providers asynchronously after drop; never assume provider data is available during session updates."
        }
        (PlatformTarget::Ios, DndOperation::TransferFileResources) => {
            "Conditional: expose file representations through NSItemProvider and security-scoped URLs wrapped in opaque FileResource values; release scope when the consumer finishes or the scene disposes."
        }

        (PlatformTarget::Web, DndOperation::ReadRichClipboard) => {
            "Conditional: Async Clipboard API navigator.clipboard.read/readText requires a Secure Context (HTTPS) and transient user activation; unsupported MIME requests fail with NotAllowedError."
        }
        (PlatformTarget::Web, DndOperation::WriteRichClipboard) => {
            "Conditional: navigator.clipboard.write/writeText with ClipboardItem requires a Secure Context and transient user activation; rich types may be rejected by the browser."
        }
        (PlatformTarget::Web, DndOperation::ProvideDelayedPayload) => {
            "Unsupported: browsers cannot register persistent OS-level delayed-rendering callbacks on the system clipboard or DataTransfer. The service rejects delayed representations with ErrorCode::Unsupported."
        }
        (PlatformTarget::Web, DndOperation::StartDragSession) => {
            "Conditional: HTML5 dragstart with DataTransfer.setData/items and setDragImage; only plain strings and files are reliably portable, and the page must stay alive for the drag."
        }
        (PlatformTarget::Web, DndOperation::AcceptDropTarget) => {
            "Conditional: dragover must preventDefault to signal acceptance; DataTransfer hides payload data during dragenter/dragover and exposes File blobs only on drop."
        }
        (PlatformTarget::Web, DndOperation::TransferFileResources) => {
            "Conditional: DataTransfer.files appears only on drop; object URLs or FileSystem handles are page-scoped and must be revoked, never converted into machine-local paths."
        }
    }
}

fn notes_for(platform: PlatformTarget, operation: DndOperation) -> &'static str {
    match (platform, operation) {
        (PlatformTarget::Windows, _) => {
            "Native Win32/OLE contract. The clipboard owner and the OLE data object must outlive delayed callbacks; the service still re-validates surface generation, app phase and cancellation."
        }
        (PlatformTarget::MacOs, _) => {
            "Native AppKit contract. NSPasteboard and NSDraggingSession objects are main-thread UI objects; delayed providers are retained by the pasteboard until it changes."
        }
        (PlatformTarget::Linux, _) => {
            "Conditional: X11 selection ownership disappears when the source process exits unless a clipboard manager harvested eager data first. Wayland requires a focused seat and a valid input serial; sandboxed file transfer should use the XDG Desktop Portal FileTransfer interface."
        }
        (
            PlatformTarget::Android,
            DndOperation::ReadRichClipboard | DndOperation::WriteRichClipboard,
        ) => {
            "Android 10+ allows clipboard access only to the focused app. A hidden or background Activity must treat the operation as unavailable; no silent retry loop."
        }
        (PlatformTarget::Android, _) => {
            "Native drag/drop requires a foreground Activity; promised files and delayed content need ContentProvider content:// stream grants and DragAndDropPermissions. Raw paths are never exposed."
        }
        (
            PlatformTarget::Ios,
            DndOperation::ReadRichClipboard | DndOperation::WriteRichClipboard,
        ) => {
            "iOS 16+ paste privacy may show a system banner. Only an explicit user gesture is a valid attestation; UIPasteControl can provide a stronger affordance."
        }
        (PlatformTarget::Ios, _) => {
            "Conditional: iPadOS supports multi-window cross-app drag while iPhone drag is scene-constrained; background suspension cancels item providers and releases security scope."
        }
        (PlatformTarget::Web, DndOperation::ProvideDelayedPayload) => {
            "Delayed OS rendering is not implementable in a browser; the service gate must return ErrorCode::Unsupported before invoking any callback."
        }
        (PlatformTarget::Web, _) => {
            "Conditional: Secure Context HTTPS and transient user activation are required for Async Clipboard; HTML5 DataTransfer hides payload data during dragover and exposes files only on drop."
        }
    }
}

/// The complete six-platform by six-operation matrix (36 rows).
pub fn platform_support_matrix() -> Vec<PlatformOperationSupport> {
    let mut rows = Vec::with_capacity(36);
    for platform in PlatformTarget::ALL {
        for operation in DndOperation::ALL {
            rows.push(PlatformOperationSupport::new(
                platform,
                operation,
                support_for(platform, operation),
                configuration_for(platform, operation),
                Evidence::ContractOnly,
                notes_for(platform, operation),
                platform_links(platform),
            ));
        }
    }
    rows
}

/// Look up one exact `(platform, operation)` row.
pub fn operation_support(
    platform: PlatformTarget,
    operation: DndOperation,
) -> PlatformOperationSupport {
    platform_support_matrix()
        .into_iter()
        .find(|row| row.platform == platform && row.operation == operation)
        .expect("the support matrix contains every platform/operation pair")
}

/// All six operation rows for one platform in declaration order.
pub fn platform_operations(platform: PlatformTarget) -> Vec<PlatformOperationSupport> {
    DndOperation::ALL
        .into_iter()
        .map(|operation| operation_support(platform, operation))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matrix_is_complete_and_matches_os_contracts() {
        let rows = platform_support_matrix();
        assert_eq!(rows.len(), 36);
        assert_eq!(PlatformTarget::ALL.len(), 6);
        assert_eq!(DndOperation::ALL.len(), 6);
        let mut cells = std::collections::BTreeSet::new();
        for platform in PlatformTarget::ALL {
            for operation in DndOperation::ALL {
                let row = operation_support(platform, operation);
                assert_eq!(row.platform, platform);
                assert_eq!(row.operation, operation);
                assert_eq!(row.evidence, Evidence::ContractOnly);
                assert!(!row.required_host_configuration.is_empty());
                assert!(!row.notes.is_empty());
                assert!(row.has_link("#52"));
                assert!(cells.insert((platform, operation)));
            }
        }
        assert_eq!(cells.len(), 36);
        assert!(
            PlatformTarget::ALL
                .iter()
                .all(|target| target.is_desktop() || target.is_mobile_or_web())
        );

        assert_eq!(
            PlatformTarget::Windows.support(DndOperation::ReadRichClipboard),
            Support::Native
        );
        assert_eq!(
            PlatformTarget::Linux.support(DndOperation::TransferFileResources),
            Support::Conditional
        );
        assert_eq!(
            PlatformTarget::Android.support(DndOperation::ProvideDelayedPayload),
            Support::Conditional
        );
        assert_eq!(
            PlatformTarget::Ios.support(DndOperation::StartDragSession),
            Support::Conditional
        );
        assert_eq!(
            PlatformTarget::Web.support(DndOperation::ProvideDelayedPayload),
            Support::Unsupported
        );
        assert_eq!(
            PlatformTarget::Web.support(DndOperation::ReadRichClipboard),
            Support::Conditional
        );
    }
}
