//! Authorized file picking, opaque resource handles and safe save/export.
//!
//! This crate owns the resource-scoped workflow from
//! [#23](https://github.com/jihuayu/gpui-box/issues/23). Native paths, Android
//! SAF `content://` URIs and Apple security-scoped URLs/bookmarks stay inside a
//! [`FileResource`] backend. Public metadata, `Debug`, diagnostics and errors are
//! path-free. Interactive pickers never become app-wide storage permissions;
//! those remain the `gpui-permissions` permission domain under #20.
//!
//! The default build has no UI dependency. The optional `gpui` feature adds an
//! adapter over the coordinated workspace GPUI identity. Blocking and streaming
//! I/O require a background [`OperationContext`] without a UI attachment.

mod error;
mod export;
mod fake;
mod model;
mod provider;
mod resource;
mod service;

pub use error::DEFAULT_CHUNK_SIZE;
pub use export::ExportResourceLease;
pub use fake::{FakeFileProvider, FaultInjection, LocalAndFakeFileProvider, ScratchFileProvider};
pub use model::{
    AccessMode, AtomicityPolicy, ConflictPolicy, DurableGrantSupport, FileOperation,
    FileTypeFilter, OpenFileOptions, OpenFilesOptions, PROVIDER_MANIFEST, PickDirectoryOptions,
    PickerOutcome, ResourceId, ResourceKind, ResourceMetadata, ResourceRevision, SaveFileOptions,
    WriteAtomicity, WriteAtomicitySupport, WriteOptions, WriteReceipt, unsupported_capability,
};
pub use provider::{
    FilePickerRequest, FileProvider, PickerCompletion, PickerSelection,
    unsupported_provider_operation,
};
pub use resource::{
    ChunkSource, FileResource, GrantRestoreOutcome, PersistedGrant, ResourceBackend,
    ResourceReader, new_resource,
};
pub use service::{FileEvent, FileService};

pub use gpui_system::{
    Availability, CancelReason, Capability, Completion, Configuration, Constraint, ErrorCode,
    Event, EventEnvelope, EventSubscription, Evidence, OperationContext, PermissionState, Platform,
    ProviderManifest, RecoveryAction, Request, RetryAdvice, ServiceResult, Support, SurfaceId,
    SystemError, SystemServices, UiContext, UiOwner,
};

#[cfg(feature = "gpui")]
pub mod gpui_adapter;
