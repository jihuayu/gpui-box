//! Opaque authorized resources, bounded streaming and safe write semantics.

use crate::error::{
    MAX_GRANT_BYTES, check_background, completed, invalid_input, io_error, revision_conflict,
    unsupported_grant, validated_chunk_size,
};
use crate::model::{
    AccessMode, ConflictPolicy, DurableGrantSupport, ResourceId, ResourceKind, ResourceMetadata,
    ResourceRevision, WriteAtomicity, WriteAtomicitySupport, WriteOptions, WriteReceipt,
};
use gpui_system::{Completion, ErrorCode, OperationContext, ServiceResult, SystemError};
use std::{
    fmt,
    io::{self, Read},
    sync::{
        Arc, Mutex, MutexGuard,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::Instant,
};

static NEXT_RESOURCE: AtomicU64 = AtomicU64::new(1);

pub(crate) fn next_resource_id() -> ResourceId {
    ResourceId(NEXT_RESOURCE.fetch_add(1, Ordering::Relaxed))
}

fn lock<T>(value: &Mutex<T>) -> MutexGuard<'_, T> {
    value
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Provider-private resource operations. Implementations must never place a
/// path or URI into a `SystemError` or into public metadata.
pub trait ResourceBackend: Send + Sync {
    fn access(&self) -> AccessMode;
    fn kind(&self) -> ResourceKind;
    fn durable_grant(&self) -> DurableGrantSupport;
    fn write_atomicity(&self) -> WriteAtomicitySupport;
    fn current_revision(&self) -> Result<ResourceRevision, SystemError>;
    fn refresh_metadata(&self, metadata: &mut ResourceMetadata) -> Result<(), SystemError> {
        metadata.revision = self.current_revision()?;
        Ok(())
    }
    fn exists(&self) -> Result<bool, SystemError>;
    fn open_read(&self) -> Result<Box<dyn Read + Send>, SystemError>;
    fn persist_grant(&self) -> Result<Vec<u8>, SystemError>;
    /// Release any native security-scope, provider stream or file descriptor
    /// held for this resource. Called once on explicit release or when the last
    /// handle drops. Implementations must be idempotent and must not block.
    fn revoke(&self) -> Result<(), SystemError> {
        Ok(())
    }
    fn save(
        &self,
        source: &mut dyn ChunkSource,
        options: &WriteOptions,
        context: &OperationContext,
    ) -> ServiceResult<WriteReceipt>;
}

/// A private streaming source used by [`FileResource::save_from_reader`] and
/// [`FileResource::write_chunks`].
pub trait ChunkSource {
    fn next_chunk(
        &mut self,
        context: &OperationContext,
        max_bytes: usize,
    ) -> ServiceResult<Option<Vec<u8>>>;
}

struct ResourceState {
    metadata: Mutex<ResourceMetadata>,
    backend: Arc<dyn ResourceBackend>,
    released: AtomicBool,
}

impl Drop for ResourceState {
    fn drop(&mut self) {
        // A reader or export handle may outlive every FileResource clone. This
        // is the final cleanup point in that case.
        if !self.released.load(Ordering::Acquire) {
            let _ = self.backend.revoke();
        }
    }
}

/// An authorized, opaque resource handle.
///
/// Clones share revocation. `release` invalidates reads, writes, streaming and
/// grant persistence on every clone immediately. Dropping the final clone also
/// revokes access. Native paths and provider URIs are never exposed by `Debug`.
#[derive(Clone)]
pub struct FileResource(Arc<ResourceState>);

/// Construct an opaque resource from a provider-private backend. This is the
/// extension point for native providers; no path or URI appears in the public
/// metadata or `Debug` representation.
pub fn new_resource(
    backend: Arc<dyn ResourceBackend>,
    mut metadata: ResourceMetadata,
) -> FileResource {
    // Even a provider that constructs metadata by hand cannot place a path or
    // URI in the public display surface.
    metadata.display_name = crate::model::sanitize_display_name(&metadata.display_name);
    metadata.extension = crate::model::extension_of(&metadata.display_name);
    metadata.mime_hint = crate::model::mime_hint_for(metadata.extension.as_deref());
    FileResource(Arc::new(ResourceState {
        metadata: Mutex::new(metadata),
        backend,
        released: AtomicBool::new(false),
    }))
}

impl FileResource {
    /// Provider construction helper equivalent to [`new_resource`].
    pub fn new(backend: Arc<dyn ResourceBackend>, metadata: ResourceMetadata) -> FileResource {
        new_resource(backend, metadata)
    }

    /// Provider construction helper equivalent to [`new_resource`].
    pub fn from_backend(
        backend: Arc<dyn ResourceBackend>,
        metadata: ResourceMetadata,
    ) -> FileResource {
        new_resource(backend, metadata)
    }

    pub fn id(&self) -> ResourceId {
        lock(&self.0.metadata).id
    }

    pub fn metadata(&self) -> ResourceMetadata {
        let mut metadata = lock(&self.0.metadata);
        if !self.0.released.load(Ordering::Acquire) {
            let _ = self.0.backend.refresh_metadata(&mut metadata);
        }
        metadata.clone()
    }

    pub fn revision(&self) -> ResourceRevision {
        self.0
            .backend
            .current_revision()
            .unwrap_or_else(|_| lock(&self.0.metadata).revision)
    }

    pub fn kind(&self) -> ResourceKind {
        self.0.backend.kind()
    }

    pub fn access(&self) -> AccessMode {
        self.0.backend.access()
    }

    pub fn durable_grant_support(&self) -> DurableGrantSupport {
        self.0.backend.durable_grant()
    }

    pub fn write_atomicity_support(&self) -> WriteAtomicitySupport {
        self.0.backend.write_atomicity()
    }

    pub fn is_released(&self) -> bool {
        self.0.released.load(Ordering::Acquire)
    }

    /// Idempotent explicit revocation. Returns true on the transition.
    pub fn release(&self) -> bool {
        let changed = !self.0.released.swap(true, Ordering::AcqRel);
        if changed {
            let _ = self.0.backend.revoke();
        }
        changed
    }

    fn ensure_available(&self) -> Result<(), SystemError> {
        if self.is_released() {
            return Err(crate::error::revoked());
        }
        Ok(())
    }

    fn require_file_read(&self) -> Result<(), SystemError> {
        self.ensure_available()?;
        if self.kind() != ResourceKind::File {
            return Err(invalid_input("Streaming requires a file resource"));
        }
        Ok(())
    }

    fn require_file_write(&self) -> Result<(), SystemError> {
        self.require_file_read()?;
        if !self.access().can_write() {
            let mut error = SystemError::new(
                ErrorCode::PermissionDenied,
                "The resource was granted read-only access",
            );
            error.recovery = Some(gpui_system::RecoveryAction::ReselectResource);
            error.retry = gpui_system::RetryAdvice::ModifyRequest;
            return Err(error);
        }
        Ok(())
    }

    /// Open a bounded reader on a background context. The reader retains the
    /// opaque handle so explicit `release()` interrupts a read in progress.
    pub fn open_reader(&self, context: &OperationContext) -> ServiceResult<ResourceReader> {
        if let Some(cancelled) = check_background(context)? {
            return Ok(cancelled);
        }
        self.require_file_read()?;
        let reader = self.0.backend.open_read()?;
        if self.is_released() {
            return Err(crate::error::revoked());
        }
        Ok(Completion::Completed(ResourceReader {
            state: Arc::clone(&self.0),
            reader,
            context: context.clone(),
        }))
    }

    /// Stream the resource through a callback in bounded chunks.
    pub fn read_chunks(
        &self,
        context: &OperationContext,
        chunk_bytes: usize,
        mut on_chunk: impl FnMut(&[u8]),
    ) -> ServiceResult<u64> {
        let chunk_bytes = validated_chunk_size(chunk_bytes)?;
        let mut reader = match self.open_reader(context)? {
            Completion::Completed(reader) => reader,
            Completion::Cancelled(reason) => return Ok(Completion::Cancelled(reason)),
        };
        let mut total = 0u64;
        loop {
            match reader.read_chunk(context, chunk_bytes)? {
                Completion::Completed(Some(bytes)) => {
                    total = total.saturating_add(bytes.len() as u64);
                    on_chunk(&bytes);
                }
                Completion::Completed(None) => return completed(total),
                Completion::Cancelled(reason) => return Ok(Completion::Cancelled(reason)),
            }
        }
    }

    /// Stream with the default 64 KiB bound.
    pub fn read_chunks_default(
        &self,
        context: &OperationContext,
        on_chunk: impl FnMut(&[u8]),
    ) -> ServiceResult<u64> {
        self.read_chunks(context, crate::DEFAULT_CHUNK_SIZE, on_chunk)
    }

    /// Copy to a writer with the default 64 KiB bound.
    pub fn copy_to_writer_default(
        &self,
        writer: &mut dyn io::Write,
        context: &OperationContext,
    ) -> ServiceResult<u64> {
        self.copy_to_writer(writer, context, crate::DEFAULT_CHUNK_SIZE)
    }

    /// Copy the resource to a caller-provided writer in bounded chunks.
    pub fn copy_to_writer(
        &self,
        writer: &mut dyn io::Write,
        context: &OperationContext,
        chunk_bytes: usize,
    ) -> ServiceResult<u64> {
        let mut reader = match self.open_reader(context)? {
            Completion::Completed(reader) => reader,
            Completion::Cancelled(reason) => return Ok(Completion::Cancelled(reason)),
        };
        reader.copy_to_writer(writer, context, chunk_bytes)
    }

    /// Stream from a blocking reader into the resource using conflict and
    /// atomicity policy. This never runs on a UI-attached context.
    pub fn save_from_reader(
        &self,
        reader: &mut dyn Read,
        options: WriteOptions,
        context: &OperationContext,
    ) -> ServiceResult<WriteReceipt> {
        self.save_internal(&mut ReaderSource { reader }, options, context)
    }

    /// Stream caller-supplied chunks into the resource. Chunks larger than the
    /// 64 KiB bound are split before writing.
    pub fn write_chunks(
        &self,
        chunks: impl IntoIterator<Item = Vec<u8>>,
        options: WriteOptions,
        context: &OperationContext,
    ) -> ServiceResult<WriteReceipt> {
        let mut source = ChunksSource {
            iter: chunks.into_iter(),
            pending: Vec::new(),
            offset: 0,
        };
        self.save_internal(&mut source, options, context)
    }

    fn save_internal(
        &self,
        source: &mut dyn ChunkSource,
        options: WriteOptions,
        context: &OperationContext,
    ) -> ServiceResult<WriteReceipt> {
        if let Some(cancelled) = check_background(context)? {
            return Ok(cancelled);
        }
        self.require_file_write()?;
        if options.atomicity == crate::model::AtomicityPolicy::RequireAtomic
            && self.write_atomicity_support() != WriteAtomicitySupport::AtomicRename
        {
            return Err(crate::error::unsupported_atomic());
        }
        if matches!(options.conflict, ConflictPolicy::ReplaceIfUnchanged { .. })
            && !self.0.backend.exists()?
        {
            return Err(revision_conflict());
        }
        let receipt = self.0.backend.save(source, &options, context)?;
        if let Completion::Completed(receipt) = receipt {
            let mut metadata = lock(&self.0.metadata);
            let _ = self.0.backend.refresh_metadata(&mut metadata);
            metadata.revision = receipt.new_revision;
            Ok(Completion::Completed(receipt))
        } else {
            Ok(receipt)
        }
    }

    /// Create a bounded opaque durable grant when the provider supports one.
    pub fn persist_grant(&self, context: &OperationContext) -> ServiceResult<PersistedGrant> {
        if let Some(cancelled) = check_background(context)? {
            return Ok(cancelled);
        }
        self.ensure_available()?;
        if self.kind() != ResourceKind::File {
            return Err(invalid_input("Only files can produce durable grants"));
        }
        if !self.durable_grant_support().is_supported() {
            return Err(unsupported_grant());
        }
        let bytes = self.0.backend.persist_grant()?;
        if bytes.is_empty() || bytes.len() > MAX_GRANT_BYTES {
            return Err(SystemError::new(
                ErrorCode::IntegrityFailure,
                "Durable resource grant is empty or exceeds its bound",
            ));
        }
        if let Some(cancelled) = check_background(context)? {
            return Ok(cancelled);
        }
        completed(PersistedGrant { bytes })
    }
}

impl fmt::Debug for FileResource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let metadata = lock(&self.0.metadata);
        f.debug_struct("FileResource")
            .field("id", &metadata.id)
            .field("display_name", &metadata.display_name)
            .field("extension", &metadata.extension)
            .field("size", &metadata.size)
            .field("kind", &metadata.kind)
            .field("access", &metadata.access)
            .field("released", &self.is_released())
            .finish()
    }
}

impl Drop for FileResource {
    fn drop(&mut self) {
        if Arc::strong_count(&self.0) == 1 && !self.0.released.swap(true, Ordering::AcqRel) {
            let _ = self.0.backend.revoke();
        }
    }
}

impl PartialEq for FileResource {
    fn eq(&self, other: &Self) -> bool {
        self.id() == other.id()
    }
}

impl Eq for FileResource {}

/// A bounded, cancellation-aware resource reader.
pub struct ResourceReader {
    state: Arc<ResourceState>,
    reader: Box<dyn Read + Send>,
    context: OperationContext,
}

impl ResourceReader {
    /// Read at most `max_bytes`. EOF is `Completed(None)`; cancellation is a
    /// normal outer `Completion::Cancelled`.
    pub fn read_chunk(
        &mut self,
        context: &OperationContext,
        max_bytes: usize,
    ) -> ServiceResult<Option<Vec<u8>>> {
        let max_bytes = validated_chunk_size(max_bytes)?;
        if let Some(cancelled) = check_background(context)? {
            return Ok(cancelled);
        }
        if self.state.released.load(Ordering::Acquire) {
            return Err(crate::error::revoked());
        }
        let mut buffer = vec![0; max_bytes];
        let read = self.reader.read(&mut buffer).map_err(io_error)?;
        if self.state.released.load(Ordering::Acquire) {
            return Err(crate::error::revoked());
        }
        if let Some(cancelled) = check_background(context)? {
            return Ok(cancelled);
        }
        if read == 0 {
            buffer.clear();
            return completed(None);
        }
        buffer.truncate(read);
        completed(Some(buffer))
    }

    /// Read one chunk with the default 64 KiB bound.
    pub fn read_chunk_default(
        &mut self,
        context: &OperationContext,
    ) -> ServiceResult<Option<Vec<u8>>> {
        self.read_chunk(context, crate::DEFAULT_CHUNK_SIZE)
    }

    pub fn copy_to_writer(
        &mut self,
        writer: &mut dyn io::Write,
        context: &OperationContext,
        chunk_bytes: usize,
    ) -> ServiceResult<u64> {
        let chunk_bytes = validated_chunk_size(chunk_bytes)?;
        let mut total = 0u64;
        loop {
            match self.read_chunk(context, chunk_bytes)? {
                Completion::Completed(Some(bytes)) => {
                    writer.write_all(&bytes).map_err(io_error)?;
                    total = total.saturating_add(bytes.len() as u64);
                }
                Completion::Completed(None) => return completed(total),
                Completion::Cancelled(reason) => return Ok(Completion::Cancelled(reason)),
            }
        }
    }
}

impl Read for ResourceReader {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        match check_background::<()>(&self.context) {
            Err(error) => return Err(io_from_system(error)),
            Ok(Some(Completion::Cancelled(_))) => {
                return Err(io::Error::new(
                    io::ErrorKind::Interrupted,
                    "Resource operation was cancelled",
                ));
            }
            Ok(_) => {}
        }
        if self.state.released.load(Ordering::Acquire) {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "Resource access was released",
            ));
        }
        let read = self.reader.read(buffer)?;
        if self.state.released.load(Ordering::Acquire) {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "Resource access was released",
            ));
        }
        match check_background::<()>(&self.context) {
            Err(error) => return Err(io_from_system(error)),
            Ok(Some(Completion::Cancelled(_))) => {
                return Err(io::Error::new(
                    io::ErrorKind::Interrupted,
                    "Resource operation was cancelled",
                ));
            }
            Ok(_) => {}
        }
        Ok(read)
    }
}

fn io_from_system(error: SystemError) -> io::Error {
    let kind = match error.code {
        ErrorCode::PermissionDenied => io::ErrorKind::PermissionDenied,
        ErrorCode::TimedOut => io::ErrorKind::TimedOut,
        ErrorCode::ContextExpired => io::ErrorKind::Interrupted,
        ErrorCode::Busy => io::ErrorKind::WouldBlock,
        ErrorCode::Unsupported => io::ErrorKind::Unsupported,
        ErrorCode::InvalidInput => io::ErrorKind::InvalidInput,
        ErrorCode::ResourceExpired | ErrorCode::Unavailable => io::ErrorKind::NotFound,
        _ => io::ErrorKind::Other,
    };
    io::Error::new(kind, error.message)
}

struct ReaderSource<'a> {
    reader: &'a mut dyn Read,
}

impl ChunkSource for ReaderSource<'_> {
    fn next_chunk(
        &mut self,
        context: &OperationContext,
        max_bytes: usize,
    ) -> ServiceResult<Option<Vec<u8>>> {
        if let Some(cancelled) = check_background(context)? {
            return Ok(cancelled);
        }
        let mut buffer = vec![0; max_bytes];
        let read = self.reader.read(&mut buffer).map_err(io_error)?;
        if let Some(cancelled) = check_background(context)? {
            return Ok(cancelled);
        }
        if read == 0 {
            return completed(None);
        }
        buffer.truncate(read);
        completed(Some(buffer))
    }
}

struct ChunksSource<I> {
    iter: I,
    pending: Vec<u8>,
    offset: usize,
}

impl<I: Iterator<Item = Vec<u8>>> ChunkSource for ChunksSource<I> {
    fn next_chunk(
        &mut self,
        context: &OperationContext,
        max_bytes: usize,
    ) -> ServiceResult<Option<Vec<u8>>> {
        if let Some(cancelled) = check_background(context)? {
            return Ok(cancelled);
        }
        loop {
            if self.offset < self.pending.len() {
                let end = self
                    .offset
                    .saturating_add(max_bytes)
                    .min(self.pending.len());
                let chunk = self.pending[self.offset..end].to_vec();
                self.offset = end;
                if self.offset == self.pending.len() {
                    self.pending.clear();
                    self.offset = 0;
                }
                if let Some(cancelled) = check_background(context)? {
                    return Ok(cancelled);
                }
                return completed(Some(chunk));
            }
            match self.iter.next() {
                Some(chunk) if chunk.is_empty() => continue,
                Some(chunk) => {
                    self.pending = chunk;
                    self.offset = 0;
                }
                None => return completed(None),
            }
        }
    }
}

/// A bounded opaque durable grant. Its serialized bytes are provider-private;
/// `Debug` deliberately reports only the length.
#[derive(Clone, PartialEq, Eq)]
pub struct PersistedGrant {
    bytes: Vec<u8>,
}

impl PersistedGrant {
    /// Validate only the public bound. Provider-specific validation occurs at
    /// restore time and yields [`GrantRestoreOutcome::InvalidOrCorrupt`].
    pub fn from_bytes(bytes: impl Into<Vec<u8>>) -> Result<Self, SystemError> {
        let bytes = bytes.into();
        if bytes.is_empty() || bytes.len() > MAX_GRANT_BYTES {
            return Err(SystemError::new(
                ErrorCode::InvalidInput,
                "Durable resource grant is empty or exceeds its bound",
            ));
        }
        Ok(Self { bytes })
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        self.bytes.clone()
    }

    pub fn len(&self) -> usize {
        self.bytes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.bytes.is_empty()
    }
}

impl AsRef<[u8]> for PersistedGrant {
    fn as_ref(&self) -> &[u8] {
        self.as_bytes()
    }
}

impl TryFrom<Vec<u8>> for PersistedGrant {
    type Error = SystemError;

    fn try_from(bytes: Vec<u8>) -> Result<Self, Self::Error> {
        Self::from_bytes(bytes)
    }
}

impl fmt::Debug for PersistedGrant {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PersistedGrant")
            .field("bytes", &self.bytes.len())
            .finish()
    }
}

/// Recovery result for a durable grant. Every variant except `Restored` is an
/// ordinary recovery state that lets an application re-prompt the user.
#[derive(Clone, PartialEq, Eq)]
pub enum GrantRestoreOutcome {
    Restored {
        resource: FileResource,
        refreshed_grant: PersistedGrant,
    },
    Revoked,
    MissingOrMoved,
    InvalidOrCorrupt,
}

impl GrantRestoreOutcome {
    pub fn is_restored(&self) -> bool {
        matches!(self, Self::Restored { .. })
    }

    pub fn resource(&self) -> Option<&FileResource> {
        match self {
            Self::Restored { resource, .. } => Some(resource),
            _ => None,
        }
    }
}

impl fmt::Debug for GrantRestoreOutcome {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Restored { resource, .. } => f
                .debug_struct("Restored")
                .field("resource", resource)
                .finish_non_exhaustive(),
            Self::Revoked => f.write_str("Revoked"),
            Self::MissingOrMoved => f.write_str("MissingOrMoved"),
            Self::InvalidOrCorrupt => f.write_str("InvalidOrCorrupt"),
        }
    }
}

pub(crate) fn check_interrupt<T>(
    context: &OperationContext,
) -> Result<Option<Completion<T>>, SystemError> {
    context.check_at(Instant::now())?;
    Ok(context.cancellation.reason().map(Completion::Cancelled))
}

pub(crate) fn revision_from_metadata(metadata: &std::fs::Metadata) -> ResourceRevision {
    use std::time::UNIX_EPOCH;
    let modified = metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .unwrap_or_default();
    let mut value = metadata.len().wrapping_mul(0x100_0000_01b3);
    value ^= modified.as_nanos() as u64;
    if value == 0 {
        value = 1;
    }
    ResourceRevision(value)
}

pub(crate) fn write_receipt(
    atomicity: WriteAtomicity,
    bytes_written: u64,
    revision: ResourceRevision,
) -> WriteReceipt {
    WriteReceipt {
        atomicity,
        bytes_written,
        new_revision: revision,
    }
}
