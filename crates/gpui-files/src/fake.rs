//! Deterministic scratch-directory and simulated SAF/cloud provider.
//!
//! This provider is a contract harness. It performs no OS dialog, never touches
//! a real Android provider and exposes no path or URI through public metadata,
//! `Debug` or `SystemError`. `root()` exists only so tests can mutate the
//! scratch directory to simulate moves, deletions and revision changes.

use crate::error::{
    MAX_GRANT_BYTES, check_background, completed, destination_exists, invalid_input, io_error,
    provider_io_failure, revision_conflict, unsupported_atomic, unsupported_grant,
};
use crate::model::{
    AccessMode, ConflictPolicy, DurableGrantSupport, FileOperation, OpenFileOptions,
    OpenFilesOptions, PickDirectoryOptions, ResourceKind, ResourceMetadata, ResourceRevision,
    SaveFileOptions, WriteAtomicity, WriteAtomicitySupport, WriteOptions, WriteReceipt,
};
use crate::provider::{FilePickerRequest, FileProvider, PickerCompletion};
use crate::resource::{
    ChunkSource, FileResource, GrantRestoreOutcome, PersistedGrant, ResourceBackend,
    check_interrupt, new_resource, next_resource_id, revision_from_metadata, write_receipt,
};
use gpui_system::{
    Availability, Capability, Completion, Configuration, Constraint, ErrorCode, Evidence,
    OperationContext, PermissionState, ServiceResult, Support, SystemError,
};
use std::{
    cell::RefCell,
    collections::{BTreeMap, BTreeSet, VecDeque},
    fs::{self, File},
    io::{self, Cursor, Read, Write},
    path::{Component, Path, PathBuf},
    sync::{
        Arc, Mutex, MutexGuard,
        atomic::{AtomicU64, Ordering},
    },
};
use tempfile::TempDir;

const LOCAL_TAG: &[u8] = b"GPFL1";
const STREAM_TAG: &[u8] = b"GPFS1";

fn lock<T>(value: &Mutex<T>) -> MutexGuard<'_, T> {
    value
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn access_code(access: AccessMode) -> u8 {
    match access {
        AccessMode::ReadOnly => 0,
        AccessMode::ReadWrite => 1,
    }
}

fn access_from_code(code: u8) -> Option<AccessMode> {
    match code {
        0 => Some(AccessMode::ReadOnly),
        1 => Some(AccessMode::ReadWrite),
        _ => None,
    }
}

fn durable_code(support: DurableGrantSupport) -> u8 {
    match support {
        DurableGrantSupport::Unsupported => 0,
        DurableGrantSupport::SecurityScopedBookmark => 1,
        DurableGrantSupport::PersistableUriGrant => 2,
    }
}

fn durable_from_code(code: u8) -> Option<DurableGrantSupport> {
    match code {
        0 => Some(DurableGrantSupport::Unsupported),
        1 => Some(DurableGrantSupport::SecurityScopedBookmark),
        2 => Some(DurableGrantSupport::PersistableUriGrant),
        _ => None,
    }
}

fn encode_grant(
    tag: &[u8],
    access: AccessMode,
    durable: DurableGrantSupport,
    payload: &str,
) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(tag.len() + 2 + payload.len());
    bytes.extend_from_slice(tag);
    bytes.push(access_code(access));
    bytes.push(durable_code(durable));
    bytes.extend_from_slice(payload.as_bytes());
    bytes
}

fn decode_grant(bytes: &[u8]) -> Option<(&[u8], AccessMode, DurableGrantSupport, &str)> {
    if bytes.len() < LOCAL_TAG.len() + 2 {
        return None;
    }
    let tag_len = LOCAL_TAG.len();
    let tag = &bytes[..tag_len];
    let access = access_from_code(bytes[tag_len])?;
    let durable = durable_from_code(bytes[tag_len + 1])?;
    let payload = std::str::from_utf8(&bytes[tag_len + 2..]).ok()?;
    Some((tag, access, durable, payload))
}

fn metadata_for_path(
    path: &Path,
    kind: ResourceKind,
    access: AccessMode,
    durable: DurableGrantSupport,
) -> ResourceMetadata {
    let display_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .map(crate::model::sanitize_display_name)
        .unwrap_or_else(|| "resource".to_owned());
    let extension = crate::model::extension_of(&display_name);
    let metadata = fs::metadata(path).ok();
    let size = match kind {
        ResourceKind::File => metadata.as_ref().map(std::fs::Metadata::len),
        ResourceKind::Directory => None,
    };
    let revision = metadata
        .as_ref()
        .map(revision_from_metadata)
        .unwrap_or_default();
    let mime_hint = crate::model::mime_hint_for(extension.as_deref());
    ResourceMetadata {
        id: next_resource_id(),
        display_name,
        extension,
        mime_hint,
        size,
        kind,
        access,
        write_atomicity: WriteAtomicitySupport::AtomicRename,
        durable_grant: durable,
        revision,
    }
}

fn stream_metadata(
    display_name: &str,
    size: usize,
    access: AccessMode,
    durable: DurableGrantSupport,
) -> ResourceMetadata {
    let display_name = crate::model::sanitize_display_name(display_name);
    let extension = crate::model::extension_of(&display_name);
    let mime_hint = crate::model::mime_hint_for(extension.as_deref());
    ResourceMetadata {
        id: next_resource_id(),
        display_name,
        extension,
        mime_hint,
        size: Some(size as u64),
        kind: ResourceKind::File,
        access,
        write_atomicity: WriteAtomicitySupport::NonAtomicTruncate,
        durable_grant: durable,
        revision: ResourceRevision(1),
    }
}

struct LocalBackend {
    path: PathBuf,
    access: AccessMode,
    kind: ResourceKind,
    durable: DurableGrantSupport,
}

impl LocalBackend {
    fn check_access(&self) -> Result<(), SystemError> {
        if !self.access.can_write() {
            return Err(SystemError::new(
                ErrorCode::PermissionDenied,
                "The resource was granted read-only access",
            ));
        }
        Ok(())
    }
}

impl ResourceBackend for LocalBackend {
    fn access(&self) -> AccessMode {
        self.access
    }

    fn kind(&self) -> ResourceKind {
        self.kind
    }

    fn durable_grant(&self) -> DurableGrantSupport {
        self.durable
    }

    fn write_atomicity(&self) -> WriteAtomicitySupport {
        WriteAtomicitySupport::AtomicRename
    }

    fn current_revision(&self) -> Result<ResourceRevision, SystemError> {
        let metadata = fs::symlink_metadata(&self.path).map_err(io_error)?;
        Ok(revision_from_metadata(&metadata))
    }

    fn refresh_metadata(&self, metadata: &mut ResourceMetadata) -> Result<(), SystemError> {
        let disk = fs::metadata(&self.path).map_err(io_error)?;
        metadata.size = if self.kind == ResourceKind::File {
            Some(disk.len())
        } else {
            None
        };
        metadata.revision = revision_from_metadata(&disk);
        Ok(())
    }

    fn exists(&self) -> Result<bool, SystemError> {
        match fs::symlink_metadata(&self.path) {
            Ok(_) => Ok(true),
            Err(cause) if cause.kind() == io::ErrorKind::NotFound => Ok(false),
            Err(cause) => Err(io_error(cause)),
        }
    }

    fn open_read(&self) -> Result<Box<dyn Read + Send>, SystemError> {
        if self.kind != ResourceKind::File {
            return Err(invalid_input("Directories cannot be streamed as files"));
        }
        Ok(Box::new(File::open(&self.path).map_err(io_error)?))
    }

    fn persist_grant(&self) -> Result<Vec<u8>, SystemError> {
        if !self.durable.is_supported() {
            return Err(unsupported_grant());
        }
        let payload = self.path.to_string_lossy();
        let bytes = encode_grant(LOCAL_TAG, self.access, self.durable, &payload);
        if bytes.len() > MAX_GRANT_BYTES {
            return Err(SystemError::new(
                ErrorCode::IntegrityFailure,
                "Durable resource grant exceeds its bound",
            ));
        }
        Ok(bytes)
    }

    fn save(
        &self,
        source: &mut dyn ChunkSource,
        options: &WriteOptions,
        context: &OperationContext,
    ) -> ServiceResult<WriteReceipt> {
        self.check_access()?;
        if self.kind != ResourceKind::File {
            return Err(invalid_input("Directories cannot be written as files"));
        }
        save_local(&self.path, source, options, context)
    }
}

fn check_conflict(path: &Path, policy: ConflictPolicy) -> Result<(), SystemError> {
    match policy {
        ConflictPolicy::CreateNew => match fs::symlink_metadata(path) {
            Ok(_) => Err(destination_exists()),
            Err(cause) if cause.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(cause) => Err(io_error(cause)),
        },
        ConflictPolicy::Overwrite => Ok(()),
        ConflictPolicy::ReplaceIfUnchanged { expected_revision } => {
            let metadata = fs::symlink_metadata(path).map_err(|cause| {
                if cause.kind() == io::ErrorKind::NotFound {
                    revision_conflict()
                } else {
                    io_error(cause)
                }
            })?;
            if revision_from_metadata(&metadata) == expected_revision {
                Ok(())
            } else {
                Err(revision_conflict())
            }
        }
    }
}

fn save_local(
    path: &Path,
    source: &mut dyn ChunkSource,
    options: &WriteOptions,
    context: &OperationContext,
) -> ServiceResult<WriteReceipt> {
    if let Some(cancelled) = check_background(context)? {
        return Ok(cancelled);
    }
    check_conflict(path, options.conflict)?;
    let parent = path
        .parent()
        .ok_or_else(|| invalid_input("The destination directory is missing"))?;
    if !parent.is_dir() {
        return Err(invalid_input("The destination directory does not exist"));
    }
    let mut staging = tempfile::Builder::new()
        .prefix(".gpui-files-")
        .tempfile_in(parent)
        .map_err(io_error)?;
    let mut written = 0u64;
    loop {
        match source.next_chunk(context, crate::error::DEFAULT_CHUNK_SIZE)? {
            Completion::Completed(Some(chunk)) => {
                staging.write_all(&chunk).map_err(io_error)?;
                written = written.saturating_add(chunk.len() as u64);
                if let Some(cancelled) = check_interrupt(context)? {
                    return Ok(cancelled);
                }
            }
            Completion::Completed(None) => break,
            Completion::Cancelled(reason) => return Ok(Completion::Cancelled(reason)),
        }
    }
    staging.as_file_mut().flush().map_err(io_error)?;
    staging.as_file().sync_all().map_err(io_error)?;
    if let Some(cancelled) = check_interrupt(context)? {
        return Ok(cancelled);
    }
    // Re-check immediately before commit. A concurrent writer may have changed
    // the revision or created the name since staging began.
    check_conflict(path, options.conflict)?;
    if let Some(cancelled) = check_interrupt(context)? {
        return Ok(cancelled);
    }
    let persist = match options.conflict {
        ConflictPolicy::CreateNew => staging.persist_noclobber(path),
        _ => staging.persist(path),
    };
    if let Err(failure) = persist {
        if failure.error.kind() == io::ErrorKind::AlreadyExists {
            return Err(destination_exists());
        }
        return Err(io_error(failure.error));
    }
    #[cfg(unix)]
    if let Some(parent) = path.parent()
        && let Ok(directory) = File::open(parent)
    {
        let _ = directory.sync_all();
    }
    let metadata = fs::metadata(path).map_err(io_error)?;
    let revision = revision_from_metadata(&metadata);
    completed(write_receipt(
        WriteAtomicity::AtomicRename,
        written,
        revision,
    ))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StreamFault {
    None,
    ReadAfter(usize),
    WriteAfter(usize),
}

struct StreamState {
    data: Mutex<Vec<u8>>,
    revision: AtomicU64,
    access: AccessMode,
    durable: DurableGrantSupport,
    display_name: String,
    handle: String,
    fault: StreamFault,
}

struct StreamBackend {
    state: Arc<StreamState>,
}

impl StreamBackend {
    fn check_access(&self) -> Result<(), SystemError> {
        if !self.state.access.can_write() {
            return Err(SystemError::new(
                ErrorCode::PermissionDenied,
                "The resource was granted read-only access",
            ));
        }
        Ok(())
    }

    fn revision(&self) -> ResourceRevision {
        ResourceRevision(self.state.revision.load(Ordering::Acquire))
    }
}

impl ResourceBackend for StreamBackend {
    fn access(&self) -> AccessMode {
        self.state.access
    }

    fn kind(&self) -> ResourceKind {
        ResourceKind::File
    }

    fn durable_grant(&self) -> DurableGrantSupport {
        self.state.durable
    }

    fn write_atomicity(&self) -> WriteAtomicitySupport {
        WriteAtomicitySupport::NonAtomicTruncate
    }

    fn current_revision(&self) -> Result<ResourceRevision, SystemError> {
        Ok(self.revision())
    }

    fn refresh_metadata(&self, metadata: &mut ResourceMetadata) -> Result<(), SystemError> {
        metadata.size = Some(lock(&self.state.data).len() as u64);
        metadata.revision = self.revision();
        Ok(())
    }

    fn exists(&self) -> Result<bool, SystemError> {
        Ok(true)
    }

    fn open_read(&self) -> Result<Box<dyn Read + Send>, SystemError> {
        let data = lock(&self.state.data).clone();
        let fault_after = match self.state.fault {
            StreamFault::ReadAfter(bytes) => Some(bytes),
            _ => None,
        };
        Ok(Box::new(FaultyReader {
            cursor: Cursor::new(data),
            fault_after,
            read: 0,
        }))
    }

    fn persist_grant(&self) -> Result<Vec<u8>, SystemError> {
        if !self.state.durable.is_supported() {
            return Err(unsupported_grant());
        }
        Ok(encode_grant(
            STREAM_TAG,
            self.state.access,
            self.state.durable,
            &self.state.handle,
        ))
    }

    fn save(
        &self,
        source: &mut dyn ChunkSource,
        options: &WriteOptions,
        context: &OperationContext,
    ) -> ServiceResult<WriteReceipt> {
        self.check_access()?;
        if options.atomicity == crate::model::AtomicityPolicy::RequireAtomic {
            return Err(unsupported_atomic());
        }
        if let Some(cancelled) = check_background(context)? {
            return Ok(cancelled);
        }
        match options.conflict {
            ConflictPolicy::CreateNew => return Err(destination_exists()),
            ConflictPolicy::Overwrite => {}
            ConflictPolicy::ReplaceIfUnchanged { expected_revision } => {
                if expected_revision != self.revision() {
                    return Err(revision_conflict());
                }
            }
        }
        let fault_after = match self.state.fault {
            StreamFault::WriteAfter(bytes) => Some(bytes),
            _ => None,
        };
        let mut data = lock(&self.state.data);
        // Non-atomic providers truncate in place. A failure or cancellation
        // leaves the partial stream and is reported honestly as such.
        data.clear();
        let mut written = 0u64;
        loop {
            match source.next_chunk(context, crate::error::DEFAULT_CHUNK_SIZE)? {
                Completion::Completed(Some(chunk)) => {
                    if let Some(limit) = fault_after {
                        if written >= limit as u64 {
                            self.state.revision.fetch_add(1, Ordering::AcqRel);
                            return Err(provider_io_failure());
                        }
                        let allowed = (limit as u64).saturating_sub(written) as usize;
                        let take = allowed.min(chunk.len());
                        data.extend_from_slice(&chunk[..take]);
                        written = written.saturating_add(take as u64);
                        if take < chunk.len() {
                            self.state.revision.fetch_add(1, Ordering::AcqRel);
                            return Err(provider_io_failure());
                        }
                    } else {
                        data.extend_from_slice(&chunk);
                        written = written.saturating_add(chunk.len() as u64);
                    }
                    if let Some(cancelled) = check_interrupt(context)? {
                        self.state.revision.fetch_add(1, Ordering::AcqRel);
                        return Ok(cancelled);
                    }
                }
                Completion::Completed(None) => break,
                Completion::Cancelled(reason) => {
                    self.state.revision.fetch_add(1, Ordering::AcqRel);
                    return Ok(Completion::Cancelled(reason));
                }
            }
        }
        drop(data);
        let revision = ResourceRevision(self.state.revision.fetch_add(1, Ordering::AcqRel) + 1);
        completed(write_receipt(
            WriteAtomicity::NonAtomicTruncate,
            written,
            revision,
        ))
    }
}

struct FaultyReader {
    cursor: Cursor<Vec<u8>>,
    fault_after: Option<usize>,
    read: usize,
}

impl Read for FaultyReader {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        if let Some(limit) = self.fault_after {
            if self.read >= limit {
                return Err(io::Error::new(
                    io::ErrorKind::ConnectionReset,
                    "simulated provider stream failure",
                ));
            }
            let remaining = limit - self.read;
            let end = remaining.min(buffer.len());
            let read = self.cursor.read(&mut buffer[..end])?;
            self.read += read;
            Ok(read)
        } else {
            self.cursor.read(buffer)
        }
    }
}

/// Deterministic stream fault injection for provider-backed resources.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FaultInjection {
    read_after: Option<usize>,
    write_after: Option<usize>,
}

impl FaultInjection {
    pub const fn none() -> Self {
        Self {
            read_after: None,
            write_after: None,
        }
    }

    pub const fn read_after(bytes: usize) -> Self {
        Self {
            read_after: Some(bytes),
            write_after: None,
        }
    }

    pub const fn write_after(bytes: usize) -> Self {
        Self {
            read_after: None,
            write_after: Some(bytes),
        }
    }

    const fn stream_fault(self) -> StreamFault {
        if let Some(bytes) = self.read_after {
            StreamFault::ReadAfter(bytes)
        } else if let Some(bytes) = self.write_after {
            StreamFault::WriteAfter(bytes)
        } else {
            StreamFault::None
        }
    }
}

enum PendingPicker {
    Single {
        completion: PickerCompletion<FileResource>,
    },
    Many {
        completion: PickerCompletion<Vec<FileResource>>,
    },
}

#[derive(Default)]
struct ProviderState {
    pending: VecDeque<PendingPicker>,
    selection: VecDeque<Vec<FileResource>>,
    auto_complete: bool,
    requests: Vec<FilePickerRequest>,
    revoked_grants: BTreeSet<Vec<u8>>,
    streams: BTreeMap<String, Arc<StreamState>>,
    next_stream: u64,
}

/// Deterministic scratch-directory and simulated-SAF/cloud provider.
pub struct ScratchFileProvider {
    root: PathBuf,
    _temporary: Option<TempDir>,
    state: RefCell<ProviderState>,
}

/// Familiar aliases for examples and downstream harnesses.
pub type FakeFileProvider = ScratchFileProvider;
pub type LocalAndFakeFileProvider = ScratchFileProvider;

impl ScratchFileProvider {
    pub fn new() -> Self {
        let temporary = tempfile::tempdir().expect("scratch directory is unavailable");
        Self {
            root: temporary.path().to_path_buf(),
            _temporary: Some(temporary),
            state: RefCell::new(ProviderState::default()),
        }
    }

    /// Reopen a deterministic scratch root, allowing cross-restart grant tests.
    pub fn open(root: impl AsRef<Path>) -> Result<Self, SystemError> {
        let root = root.as_ref();
        fs::create_dir_all(root).map_err(io_error)?;
        let root = fs::canonicalize(root).map_err(io_error)?;
        Ok(Self {
            root,
            _temporary: None,
            state: RefCell::new(ProviderState::default()),
        })
    }

    /// Harness-only access for move/delete/revision simulations. Never use this
    /// path in diagnostics or `Debug` output.
    pub fn root(&self) -> &Path {
        &self.root
    }

    fn scratch_path(&self, relative: impl AsRef<Path>) -> Result<PathBuf, SystemError> {
        let relative = relative.as_ref();
        if relative.is_absolute()
            || relative
                .components()
                .any(|component| !matches!(component, Component::Normal(_)))
        {
            return Err(invalid_input("A normal relative scratch path is required"));
        }
        Ok(self.root.join(relative))
    }

    pub fn create_file(
        &self,
        relative: impl AsRef<Path>,
        bytes: &[u8],
    ) -> Result<FileResource, SystemError> {
        self.create_file_with(
            relative,
            bytes,
            AccessMode::ReadWrite,
            DurableGrantSupport::Unsupported,
        )
    }

    pub fn create_read_only_file(
        &self,
        relative: impl AsRef<Path>,
        bytes: &[u8],
    ) -> Result<FileResource, SystemError> {
        self.create_file_with(
            relative,
            bytes,
            AccessMode::ReadOnly,
            DurableGrantSupport::Unsupported,
        )
    }

    pub fn create_durable_file(
        &self,
        relative: impl AsRef<Path>,
        bytes: &[u8],
        support: DurableGrantSupport,
    ) -> Result<FileResource, SystemError> {
        self.create_file_with(relative, bytes, AccessMode::ReadWrite, support)
    }

    pub fn create_file_with(
        &self,
        relative: impl AsRef<Path>,
        bytes: &[u8],
        access: AccessMode,
        durable: DurableGrantSupport,
    ) -> Result<FileResource, SystemError> {
        let path = self.scratch_path(relative)?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(io_error)?;
        }
        fs::write(&path, bytes).map_err(io_error)?;
        Ok(self.resource_at(path, access, durable))
    }

    pub fn create_directory(
        &self,
        relative: impl AsRef<Path>,
    ) -> Result<FileResource, SystemError> {
        let path = self.scratch_path(relative)?;
        fs::create_dir_all(&path).map_err(io_error)?;
        let metadata = metadata_for_path(
            &path,
            ResourceKind::Directory,
            AccessMode::ReadWrite,
            DurableGrantSupport::Unsupported,
        );
        Ok(new_resource(
            Arc::new(LocalBackend {
                path,
                access: AccessMode::ReadWrite,
                kind: ResourceKind::Directory,
                durable: DurableGrantSupport::Unsupported,
            }),
            metadata,
        ))
    }

    /// Wrap an existing path or a save target. The path stays inside the opaque
    /// resource and is never printed.
    pub fn resource_at(
        &self,
        path: impl AsRef<Path>,
        access: AccessMode,
        durable: DurableGrantSupport,
    ) -> FileResource {
        let path = path.as_ref().to_path_buf();
        let metadata = metadata_for_path(&path, ResourceKind::File, access, durable);
        new_resource(
            Arc::new(LocalBackend {
                path,
                access,
                kind: ResourceKind::File,
                durable,
            }),
            metadata,
        )
    }

    pub fn create_stream(
        &self,
        display_name: &str,
        bytes: &[u8],
        durable: DurableGrantSupport,
    ) -> FileResource {
        self.create_stream_with_fault(display_name, bytes, durable, FaultInjection::none())
    }

    pub fn create_read_only_stream(
        &self,
        display_name: &str,
        bytes: &[u8],
        durable: DurableGrantSupport,
    ) -> FileResource {
        self.create_stream_with_access(
            display_name,
            bytes,
            AccessMode::ReadOnly,
            durable,
            FaultInjection::none(),
        )
    }

    pub fn create_stream_with_fault(
        &self,
        display_name: &str,
        bytes: &[u8],
        durable: DurableGrantSupport,
        fault: FaultInjection,
    ) -> FileResource {
        self.create_stream_with_access(display_name, bytes, AccessMode::ReadWrite, durable, fault)
    }

    pub fn create_stream_with_access(
        &self,
        display_name: &str,
        bytes: &[u8],
        access: AccessMode,
        durable: DurableGrantSupport,
        fault: FaultInjection,
    ) -> FileResource {
        let display_name = crate::model::sanitize_display_name(display_name);
        let mut state = self.state.borrow_mut();
        let id = state.next_stream;
        state.next_stream = state.next_stream.saturating_add(1);
        let handle = format!("content://gpui-files.stream/{id}");
        let stream = Arc::new(StreamState {
            data: Mutex::new(bytes.to_vec()),
            revision: AtomicU64::new(1),
            access,
            durable,
            display_name: display_name.clone(),
            handle: handle.clone(),
            fault: fault.stream_fault(),
        });
        state.streams.insert(handle, Arc::clone(&stream));
        drop(state);
        let metadata = stream_metadata(&display_name, bytes.len(), access, durable);
        new_resource(Arc::new(StreamBackend { state: stream }), metadata)
    }

    pub fn queue_selection(&self, resources: Vec<FileResource>) {
        self.state.borrow_mut().selection.push_back(resources);
    }

    pub fn set_auto_complete(&self, auto_complete: bool) {
        self.state.borrow_mut().auto_complete = auto_complete;
    }

    pub fn pending_count(&self) -> usize {
        self.state.borrow().pending.len()
    }

    pub fn requests(&self) -> Vec<FilePickerRequest> {
        self.state.borrow().requests.clone()
    }

    pub fn complete_next(&self, resources: Vec<FileResource>) -> bool {
        let pending = self.state.borrow_mut().pending.pop_front();
        match pending {
            Some(PendingPicker::Single { completion, .. }) => match resources.into_iter().next() {
                Some(resource) => completion.complete(resource),
                None => completion.cancel(),
            },
            Some(PendingPicker::Many { completion, .. }) => {
                if resources.is_empty() {
                    completion.cancel()
                } else {
                    completion.complete_all(resources)
                }
            }
            None => false,
        }
    }

    pub fn cancel_next(&self) -> bool {
        let pending = self.state.borrow_mut().pending.pop_front();
        match pending {
            Some(PendingPicker::Single { completion, .. }) => completion.cancel(),
            Some(PendingPicker::Many { completion, .. }) => completion.cancel(),
            None => false,
        }
    }

    pub fn fail_next(&self, error: SystemError) -> bool {
        let pending = self.state.borrow_mut().pending.pop_front();
        match pending {
            Some(PendingPicker::Single { completion, .. }) => completion.fail(error),
            Some(PendingPicker::Many { completion, .. }) => completion.fail(error),
            None => false,
        }
    }

    pub fn revoke_grant(&self, grant: &PersistedGrant) {
        self.state
            .borrow_mut()
            .revoked_grants
            .insert(grant.to_bytes());
    }

    pub fn revoke_grant_bytes(&self, bytes: impl Into<Vec<u8>>) {
        self.state.borrow_mut().revoked_grants.insert(bytes.into());
    }

    fn pop_selection(&self) -> Vec<FileResource> {
        self.state
            .borrow_mut()
            .selection
            .pop_front()
            .unwrap_or_default()
    }

    fn record(&self, request: FilePickerRequest) {
        self.state.borrow_mut().requests.push(request);
    }

    fn dispatch_single(
        &self,
        request: FilePickerRequest,
        completion: PickerCompletion<FileResource>,
    ) -> Result<(), SystemError> {
        self.record(request);
        let auto = self.state.borrow().auto_complete;
        if auto {
            match self.pop_selection().into_iter().next() {
                Some(resource) => {
                    completion.complete(resource);
                }
                None => {
                    completion.cancel();
                }
            }
            return Ok(());
        }
        self.state
            .borrow_mut()
            .pending
            .push_back(PendingPicker::Single { completion });
        Ok(())
    }

    fn dispatch_many(
        &self,
        request: FilePickerRequest,
        completion: PickerCompletion<Vec<FileResource>>,
    ) -> Result<(), SystemError> {
        self.record(request);
        let auto = self.state.borrow().auto_complete;
        if auto {
            let selection = self.pop_selection();
            if selection.is_empty() {
                completion.cancel();
            } else {
                completion.complete_all(selection);
            }
            return Ok(());
        }
        self.state
            .borrow_mut()
            .pending
            .push_back(PendingPicker::Many { completion });
        Ok(())
    }

    fn restore_local(
        &self,
        access: AccessMode,
        durable: DurableGrantSupport,
        payload: &str,
    ) -> GrantRestoreOutcome {
        let path = PathBuf::from(payload);
        if !durable.is_supported()
            || !path.is_absolute()
            || path
                .components()
                .any(|component| matches!(component, Component::ParentDir))
        {
            return GrantRestoreOutcome::InvalidOrCorrupt;
        }
        let Ok(canonical_root) = fs::canonicalize(&self.root) else {
            return GrantRestoreOutcome::InvalidOrCorrupt;
        };
        let metadata = match fs::symlink_metadata(&path) {
            Ok(metadata) => metadata,
            Err(cause) if cause.kind() == io::ErrorKind::NotFound => {
                if path.starts_with(&self.root) || path.starts_with(&canonical_root) {
                    return GrantRestoreOutcome::MissingOrMoved;
                }
                return GrantRestoreOutcome::InvalidOrCorrupt;
            }
            Err(_) => return GrantRestoreOutcome::InvalidOrCorrupt,
        };
        if metadata.file_type().is_symlink() || !metadata.file_type().is_file() {
            return GrantRestoreOutcome::InvalidOrCorrupt;
        }
        let Ok(canonical_path) = fs::canonicalize(&path) else {
            return GrantRestoreOutcome::InvalidOrCorrupt;
        };
        if !canonical_path.starts_with(&canonical_root) {
            return GrantRestoreOutcome::InvalidOrCorrupt;
        }
        let backend = Arc::new(LocalBackend {
            path: canonical_path.clone(),
            access,
            kind: ResourceKind::File,
            durable,
        });
        let metadata = metadata_for_path(&canonical_path, ResourceKind::File, access, durable);
        let refreshed_grant = match backend.persist_grant() {
            Ok(bytes) => match PersistedGrant::from_bytes(bytes) {
                Ok(grant) => grant,
                Err(_) => return GrantRestoreOutcome::InvalidOrCorrupt,
            },
            Err(_) => return GrantRestoreOutcome::InvalidOrCorrupt,
        };
        GrantRestoreOutcome::Restored {
            resource: new_resource(backend, metadata),
            refreshed_grant,
        }
    }
}

impl Default for ScratchFileProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl FileProvider for ScratchFileProvider {
    fn capability(&self, operation: FileOperation) -> Capability<FileOperation> {
        Capability {
            operation,
            support: Support::Native,
            availability: Availability::Available,
            configuration: Configuration::Ready,
            permission: PermissionState::NotRequired,
            evidence: Evidence::ContractOnly,
            constraints: if operation.is_interactive() {
                vec![Constraint::ForegroundOwner]
            } else {
                Vec::new()
            },
            observed_revision: 1,
        }
    }

    fn open_file(
        &self,
        _options: OpenFileOptions,
        request: FilePickerRequest,
        completion: PickerCompletion<FileResource>,
    ) -> Result<(), SystemError> {
        self.dispatch_single(request, completion)
    }

    fn open_files(
        &self,
        _options: OpenFilesOptions,
        request: FilePickerRequest,
        completion: PickerCompletion<Vec<FileResource>>,
    ) -> Result<(), SystemError> {
        self.dispatch_many(request, completion)
    }

    fn pick_directory(
        &self,
        _options: PickDirectoryOptions,
        request: FilePickerRequest,
        completion: PickerCompletion<FileResource>,
    ) -> Result<(), SystemError> {
        self.dispatch_single(request, completion)
    }

    fn save_file(
        &self,
        _options: SaveFileOptions,
        request: FilePickerRequest,
        completion: PickerCompletion<FileResource>,
    ) -> Result<(), SystemError> {
        self.dispatch_single(request, completion)
    }

    fn restore_grant(&self, grant: &PersistedGrant) -> Result<GrantRestoreOutcome, SystemError> {
        if grant.as_bytes().is_empty() || grant.len() > MAX_GRANT_BYTES {
            return Ok(GrantRestoreOutcome::InvalidOrCorrupt);
        }
        if self
            .state
            .borrow()
            .revoked_grants
            .contains(grant.as_bytes())
        {
            return Ok(GrantRestoreOutcome::Revoked);
        }
        let Some((tag, access, durable, payload)) = decode_grant(grant.as_bytes()) else {
            return Ok(GrantRestoreOutcome::InvalidOrCorrupt);
        };
        if tag == LOCAL_TAG {
            return Ok(self.restore_local(access, durable, payload));
        }
        if tag == STREAM_TAG {
            if !durable.is_supported() {
                return Ok(GrantRestoreOutcome::InvalidOrCorrupt);
            }
            let stream = self.state.borrow().streams.get(payload).cloned();
            let Some(stream) = stream else {
                return Ok(GrantRestoreOutcome::MissingOrMoved);
            };
            if access != stream.access || durable != stream.durable {
                return Ok(GrantRestoreOutcome::InvalidOrCorrupt);
            }
            let backend = Arc::new(StreamBackend { state: stream });
            let bytes = match backend.persist_grant() {
                Ok(bytes) => bytes,
                Err(_) => return Ok(GrantRestoreOutcome::InvalidOrCorrupt),
            };
            let refreshed_grant = match PersistedGrant::from_bytes(bytes) {
                Ok(grant) => grant,
                Err(_) => return Ok(GrantRestoreOutcome::InvalidOrCorrupt),
            };
            let size = lock(&backend.state.data).len();
            let mut metadata = stream_metadata(&backend.state.display_name, size, access, durable);
            metadata.revision = backend.revision();
            return Ok(GrantRestoreOutcome::Restored {
                resource: new_resource(backend, metadata),
                refreshed_grant,
            });
        }
        Ok(GrantRestoreOutcome::InvalidOrCorrupt)
    }
}
