//! Public-safe resource metadata and typed picker/write options.
//!
//! Nothing in this module contains a native path, a `content://` URI, a
//! security-scoped bookmark or an Apple URL. Those remain inside the opaque
//! [`crate::FileResource`] backend.

use gpui_system::{Capability, Configuration, Constraint, PermissionState, Support};
use std::fmt;

/// A process-local, opaque resource identity. It is safe to log and compare.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ResourceId(pub(crate) u64);

impl ResourceId {
    pub const fn value(self) -> u64 {
        self.0
    }

    /// Allocate a fresh opaque id. Prefer provider construction through
    /// [`crate::new_resource`] and metadata created by the provider.
    pub fn new() -> Self {
        crate::resource::next_resource_id()
    }
}

impl Default for ResourceId {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for ResourceId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "resource-{}", self.0)
    }
}

/// A public-safe change fingerprint. It is not proof of content integrity.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ResourceRevision(pub u64);

impl ResourceRevision {
    pub const fn value(self) -> u64 {
        self.0
    }

    pub const fn new(value: u64) -> Self {
        Self(value)
    }
}

impl From<u64> for ResourceRevision {
    fn from(value: u64) -> Self {
        Self(value)
    }
}

impl From<ResourceRevision> for u64 {
    fn from(value: ResourceRevision) -> Self {
        value.0
    }
}

/// The shape of an authorized resource.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResourceKind {
    File,
    Directory,
}

/// The access the user or host granted for this resource.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccessMode {
    ReadOnly,
    ReadWrite,
}

impl AccessMode {
    pub const fn can_write(self) -> bool {
        matches!(self, Self::ReadWrite)
    }
}

/// The strongest write behavior a provider can offer for a resource.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WriteAtomicitySupport {
    /// A same-directory staging file can be atomically renamed into place.
    AtomicRename,
    /// Writes truncate and stream directly; a partial write is possible.
    NonAtomicTruncate,
}

/// Durable cross-session access support reported by a provider.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DurableGrantSupport {
    Unsupported,
    SecurityScopedBookmark,
    PersistableUriGrant,
}

impl DurableGrantSupport {
    pub const fn is_supported(self) -> bool {
        !matches!(self, Self::Unsupported)
    }
}

/// The only resource metadata exposed outside the crate.
///
/// `display_name` is a sanitized base name, never a path. `mime_hint` is a
/// coarse static hint, not a trusted content-type claim.
#[derive(Clone, PartialEq, Eq)]
pub struct ResourceMetadata {
    pub id: ResourceId,
    pub display_name: String,
    pub extension: Option<String>,
    pub mime_hint: Option<&'static str>,
    pub size: Option<u64>,
    pub kind: ResourceKind,
    pub access: AccessMode,
    pub write_atomicity: WriteAtomicitySupport,
    pub durable_grant: DurableGrantSupport,
    pub revision: ResourceRevision,
}

impl fmt::Debug for ResourceMetadata {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let display_name = sanitize_display_name(&self.display_name);
        let extension = extension_of(&display_name);
        let mime_hint = mime_hint_for(extension.as_deref());
        f.debug_struct("ResourceMetadata")
            .field("id", &self.id)
            .field("display_name", &display_name)
            .field("extension", &extension)
            .field("mime_hint", &mime_hint)
            .field("size", &self.size)
            .field("kind", &self.kind)
            .field("access", &self.access)
            .field("write_atomicity", &self.write_atomicity)
            .field("durable_grant", &self.durable_grant)
            .field("revision", &self.revision)
            .finish()
    }
}

/// Operations exposed by a file provider. Interactive operations use picker
/// grants; they are deliberately distinct from app-wide storage permissions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum FileOperation {
    OpenFile,
    OpenMultipleFiles,
    PickDirectory,
    SaveFile,
    StreamingIo,
    PersistGrant,
    LeasedExport,
}

impl FileOperation {
    pub const ALL: [Self; 7] = [
        Self::OpenFile,
        Self::OpenMultipleFiles,
        Self::PickDirectory,
        Self::SaveFile,
        Self::StreamingIo,
        Self::PersistGrant,
        Self::LeasedExport,
    ];

    pub const fn label(self) -> &'static str {
        match self {
            Self::OpenFile => "Open file",
            Self::OpenMultipleFiles => "Open multiple files",
            Self::PickDirectory => "Choose directory",
            Self::SaveFile => "Save file",
            Self::StreamingIo => "Stream resource content",
            Self::PersistGrant => "Persist resource access",
            Self::LeasedExport => "Export resource",
        }
    }

    pub const fn is_interactive(self) -> bool {
        matches!(
            self,
            Self::OpenFile | Self::OpenMultipleFiles | Self::PickDirectory | Self::SaveFile
        )
    }
}

/// A typed dialog filter. Filters are advisory hints to a native picker and are
/// re-checked by providers after selection where the platform supports it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileTypeFilter {
    pub label: String,
    pub extensions: Vec<String>,
}

impl FileTypeFilter {
    pub fn new(
        label: impl Into<String>,
        extensions: impl IntoIterator<Item = impl Into<String>>,
    ) -> Self {
        let label = sanitize_label(&label.into());
        let mut normalized = Vec::new();
        for extension in extensions {
            let extension = sanitize_extension(&extension.into());
            if let Some(extension) = extension
                && !normalized.contains(&extension)
            {
                normalized.push(extension);
            }
        }
        Self {
            label,
            extensions: normalized,
        }
    }

    pub fn matches(&self, metadata: &ResourceMetadata) -> bool {
        metadata.extension.as_ref().is_some_and(|extension| {
            self.extensions
                .iter()
                .any(|candidate| candidate.eq_ignore_ascii_case(extension))
        })
    }
}

/// Options for `open_file`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OpenFileOptions {
    pub filters: Vec<FileTypeFilter>,
    pub prompt: Option<String>,
}

impl OpenFileOptions {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_filter(mut self, filter: FileTypeFilter) -> Self {
        self.filters.push(filter);
        self
    }

    pub fn with_prompt(mut self, prompt: impl Into<String>) -> Self {
        self.prompt = Some(prompt.into());
        self
    }
}

/// Options for `open_files`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OpenFilesOptions {
    pub filters: Vec<FileTypeFilter>,
    pub prompt: Option<String>,
}

impl OpenFilesOptions {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_filter(mut self, filter: FileTypeFilter) -> Self {
        self.filters.push(filter);
        self
    }

    pub fn with_prompt(mut self, prompt: impl Into<String>) -> Self {
        self.prompt = Some(prompt.into());
        self
    }
}

/// Options for `pick_directory`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PickDirectoryOptions {
    pub prompt: Option<String>,
}

impl PickDirectoryOptions {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_prompt(mut self, prompt: impl Into<String>) -> Self {
        self.prompt = Some(prompt.into());
        self
    }
}

/// Options for `save_file`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SaveFileOptions {
    pub filters: Vec<FileTypeFilter>,
    pub prompt: Option<String>,
    pub suggested_name: Option<String>,
}

impl SaveFileOptions {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_filter(mut self, filter: FileTypeFilter) -> Self {
        self.filters.push(filter);
        self
    }

    pub fn with_prompt(mut self, prompt: impl Into<String>) -> Self {
        self.prompt = Some(prompt.into());
        self
    }

    pub fn with_suggested_name(mut self, name: impl Into<String>) -> Self {
        self.suggested_name = Some(name.into());
        self
    }
}

/// User dismissal is an ordinary outcome, never a [`gpui_system::SystemError`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PickerOutcome<T> {
    Selected(T),
    Cancelled,
}

impl<T> PickerOutcome<T> {
    pub const fn is_cancelled(&self) -> bool {
        matches!(self, Self::Cancelled)
    }

    pub fn selected(self) -> Option<T> {
        match self {
            Self::Selected(value) => Some(value),
            Self::Cancelled => None,
        }
    }
}

/// Conflict handling for a save operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConflictPolicy {
    /// Fail if any entry already exists at the destination.
    CreateNew,
    /// Replace an existing entry unconditionally.
    Overwrite,
    /// Replace only when the resource still matches the expected revision.
    ReplaceIfUnchanged { expected_revision: ResourceRevision },
}

/// Whether a caller requires renaming atomicity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AtomicityPolicy {
    /// Use a non-atomic provider stream when no atomic primitive exists.
    BestEffort,
    /// Reject providers without an atomic rename primitive before writing.
    RequireAtomic,
}

/// Complete save policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WriteOptions {
    pub conflict: ConflictPolicy,
    pub atomicity: AtomicityPolicy,
}

impl WriteOptions {
    pub const fn new(conflict: ConflictPolicy, atomicity: AtomicityPolicy) -> Self {
        Self {
            conflict,
            atomicity,
        }
    }

    pub const fn overwrite() -> Self {
        Self::new(ConflictPolicy::Overwrite, AtomicityPolicy::RequireAtomic)
    }

    pub const fn create_new() -> Self {
        Self::new(ConflictPolicy::CreateNew, AtomicityPolicy::RequireAtomic)
    }

    pub const fn replace_if_unchanged(expected: ResourceRevision) -> Self {
        Self::new(
            ConflictPolicy::ReplaceIfUnchanged {
                expected_revision: expected,
            },
            AtomicityPolicy::RequireAtomic,
        )
    }
}

/// The atomicity actually used by a completed write.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WriteAtomicity {
    AtomicRename,
    NonAtomicTruncate,
}

/// A completed write receipt. `bytes_written` excludes staging metadata.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WriteReceipt {
    pub atomicity: WriteAtomicity,
    pub bytes_written: u64,
    pub new_revision: ResourceRevision,
}

/// Conservative five-platform provider evidence for this contract-only crate.
pub const PROVIDER_MANIFEST: [gpui_system::ProviderManifest; 5] = gpui_system::PROVIDER_MANIFEST;

/// Capability shape for a provider that does not implement an operation.
pub fn unsupported_capability(operation: FileOperation) -> Capability<FileOperation> {
    Capability {
        operation,
        support: Support::Unsupported,
        availability: gpui_system::Availability::Unknown,
        configuration: Configuration::Unknown,
        permission: PermissionState::Unknown,
        evidence: gpui_system::Evidence::ContractOnly,
        constraints: Vec::<Constraint>::new(),
        observed_revision: 0,
    }
}

pub(crate) fn sanitize_label(value: &str) -> String {
    let mut value: String = value
        .chars()
        .take(64)
        .map(|character| {
            if character.is_control() {
                ' '
            } else {
                character
            }
        })
        .collect();
    if value.trim().is_empty() {
        value = "Files".to_owned();
    }
    value
}

pub(crate) fn sanitize_extension(value: &str) -> Option<String> {
    let value = value.strip_prefix('.').unwrap_or(value);
    if value.is_empty()
        || value.len() > 16
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'+' || byte == b'-')
    {
        return None;
    }
    Some(value.to_ascii_lowercase())
}

pub(crate) fn sanitize_display_name(value: &str) -> String {
    let base = value.rsplit(['/', '\\']).next().unwrap_or(value);
    let mut name: String = base
        .chars()
        .take(128)
        .map(|character| {
            if character.is_control() || matches!(character, '/' | '\\' | ':' | '\0') {
                '_'
            } else {
                character
            }
        })
        .collect();
    if name.trim().is_empty() || name == "." || name == ".." {
        name = "resource".to_owned();
    }
    name
}

pub(crate) fn extension_of(name: &str) -> Option<String> {
    std::path::Path::new(name)
        .extension()
        .and_then(|extension| extension.to_str())
        .and_then(sanitize_extension)
}

pub(crate) fn mime_hint_for(extension: Option<&str>) -> Option<&'static str> {
    match extension? {
        "txt" | "md" | "log" => Some("text/plain"),
        "json" => Some("application/json"),
        "png" => Some("image/png"),
        "jpg" | "jpeg" => Some("image/jpeg"),
        "pdf" => Some("application/pdf"),
        "csv" => Some("text/csv"),
        _ => Some("application/octet-stream"),
    }
}
