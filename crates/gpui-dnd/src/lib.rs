//! Typed rich clipboard and drag-and-drop contracts over the coordinated
//! `gpui-box` system, lifecycle, file and plain-text clipboard services.
//!
//! The default build has no GPUI dependency. Enable `gpui` for the optional
//! `App` global, `SystemServices` registration, `DndSurfaceScope` and
//! `DndEntityModel` adapter.
//!
//! Security properties:
//!
//! - only `gpui-files` opaque [`FileResource`] handles cross the transfer
//!   boundary; no path, `file://` URL or `content://` URI is ever public;
//! - every [`SystemError`] message is a static, public-safe string;
//! - delayed callbacks and custom drop evaluators always run outside internal
//!   locks and are re-validated afterwards for TOCTOU safety;
//! - eager clipboard representations remain pasteable after a handoff lease
//!   drops, while delayed callbacks and promised resources are revoked
//!   immediately.

mod error;
mod format;
mod model;
mod platform;
mod provider;
mod service;
mod stream;

pub use format::{
    DeliveryMode, FILE_RESOURCES_MIME, FILE_RESOURCES_UTI, FormatClass, FormatDescriptor,
    HTML_MIME, HTML_UTI, JPEG_MIME, JPEG_UTI, MARKDOWN_MIME, MARKDOWN_UTI, MAX_MIME_TYPE_BYTES,
    MAX_UTI_BYTES, MimeType, NegotiatedFormat, PLAIN_TEXT_ESSENCE, PLAIN_TEXT_MIME, PLAIN_TEXT_UTI,
    PNG_MIME, PNG_UTI, PayloadOffer, RGBA8_MIME, RGBA8_UTI, RTF_MIME, RTF_UTI, StandardFormat,
    UtiIdentifier, WEBP_MIME, WEBP_UTI, negotiate_format, standard_formats,
};
pub use model::{
    AllowedEffects, DragEffect, DragModifiers, DragPoint, DragPreview, DragPreviewVisual,
    DragSessionId, DragSourceSpec, DropEvaluationContext, DropEvaluator, DropTargetId,
    DropTargetSpec, DropTargetVerdict, HandoffId, LogicalRect, MAX_ACTIVE_STREAM_LEASES,
    MAX_CUSTOM_BYTES, MAX_IMAGE_BYTES, MAX_OFFERED_FORMATS, MAX_PREVIEW_ASSET_BYTES,
    MAX_PREVIEW_LABEL_BYTES, MAX_RICH_TEXT_BYTES, MAX_TEXT_BYTES, MAX_TRANSFERRED_FILES,
    RgbaImageBuffer, SourceRepresentation, TypedPayload,
};
pub use platform::{
    DndOperation, PlatformOperationSupport, PlatformTarget, operation_support, platform_operations,
    platform_support_matrix,
};
pub use provider::{
    ClipboardEntry, ClipboardHandoff, ClipboardSnapshot, FAKE_PROVIDER_MAX_FILES,
    FakeDragStartEvent, FakeDragStartHook, FakeHostedDrag, FakeIncomingDrag, FakePublishEvent,
    FakePublishHook, FakeRichTransferProvider, ProviderClipboardGrant, ProviderDragGrant,
    ProviderDragRequest, ProviderDropOutcome, RichTransferProvider, RichTransferProviderListener,
    RichTransferProviderNotice, UnsupportedRichTransferProvider,
};
pub use service::{
    DEFAULT_DND_EVENT_CAPACITY, DndEvent, DndEventDrain, DragCancelOutcome, DragCancelReason,
    DragSessionLease, DragSessionStatus, DragUpdate, DropCompletion, DropOutcome, DropTargetLease,
    RichTransferService, StaleCallbackReason,
};
pub use stream::{
    ClipboardReadOutcome, ConsumedPayload, HandoffLease, HandoffReceipt, HandoffRevokeReason,
    HandoffScope, LazyPayloadCallback, LazyPayloadProvider, LazyRepresentation, PayloadStreamLease,
    RichClipboardWrite, TransferInteraction, read_plain_text_through, read_plain_text_through_with,
    read_plain_text_via_provider, write_plain_text_through, write_plain_text_through_with,
    write_plain_text_via_provider,
};

pub use gpui_clipboard::{
    Clipboard, ClipboardInteraction, ClipboardOperation, ClipboardProvider, ClipboardReceipt,
    ClipboardText, UnavailableClipboard,
};
pub use gpui_files::{
    AccessMode, DurableGrantSupport, FakeFileProvider, FileResource, ResourceId, ResourceKind,
    ResourceMetadata, ResourceReader, new_resource,
};
pub use gpui_lifecycle::{AppPhase, LifecycleState};
pub use gpui_system::{
    Availability, CancelReason, CancellationToken, Capability, Completion, Configuration,
    Constraint, ErrorCode, Event, EventEnvelope, EventSubscription, Evidence, OperationContext,
    PermissionState, Platform, ProviderManifest, RecoveryAction, Request, RetryAdvice,
    ServiceResult, Support, SurfaceId, SystemError, SystemServices, UiContext, UiOwner,
};

/// The coordinated upstream GPUI revision used by this distribution.
pub const UPSTREAM_GPUI_REVISION: &str = "f8cfc7ed9e57503def9479138894f3139f5400cc";

/// The gpui-dnd package version.
pub const PACKAGE_VERSION: &str = env!("CARGO_PKG_VERSION");

#[cfg(feature = "gpui")]
pub mod gpui_adapter;
