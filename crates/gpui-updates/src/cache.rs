use crate::{ArtifactVerification, UpdateVerifier, VerifiedUpdate, background, error, value};
use gpui_app_data::{AppData, AppDirectory, DataLocation, DataScope, Durability, SettingsStore};
use gpui_system::{Completion, ErrorCode, OperationContext, ServiceResult, SystemError};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Seek, SeekFrom, Write},
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};

/// Transports must stream bounded chunks and negotiate Range/ETag independently.
pub const MAX_UPDATE_CHUNK_BYTES: usize = 1024 * 1024;
static NEXT_CACHE_SESSION: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CacheTicket {
    session: u64,
    generation: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArtifactCacheState {
    Staging,
    Published,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TransferProgress {
    pub received: u64,
    pub total: u64,
    pub state: ArtifactCacheState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CacheTransfer {
    pub ticket: CacheTicket,
    pub progress: TransferProgress,
}

/// Owns a read-only handle to the exact verified bytes. A native provider must
/// inspect APK identity/signature and bind its private installer copy/grant to
/// these bytes. No path or OS installation authorization is supplied here.
pub struct CachedArtifact {
    file: File,
    verification: ArtifactVerification,
    durability: Durability,
}

impl CachedArtifact {
    pub fn verification(&self) -> &ArtifactVerification {
        &self.verification
    }

    pub fn into_parts(self) -> (File, ArtifactVerification, Durability) {
        (self.file, self.verification, self.durability)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Phase {
    Staging,
    Published,
    Discarding,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct TransferCheckpoint {
    payload_sha256: String,
    artifact_sha256: String,
    size: u64,
    received: u64,
    prefix_sha256: String,
    phase: Phase,
}

struct ActiveTransfer {
    update: VerifiedUpdate,
    hash: Sha256,
}

/// One bounded artifact slot per AppData owner. Blocking operations require a
/// background executor. Mutable access and an OS lock serialize local writers.
/// Retain this owner for the entire transfer; resume creates a fresh ticket.
pub struct ArtifactCache {
    data: AppDirectory,
    store: SettingsStore,
    checkpoint: Option<TransferCheckpoint>,
    active: Option<ActiveTransfer>,
    session: u64,
    generation: u64,
    durability: Durability,
    _lock: File,
}

impl ArtifactCache {
    pub fn open(app: &AppData) -> Result<Self, SystemError> {
        // Data is deliberate: ordinary AppData::clear_cache must not race an
        // active transfer or an installer-owned artifact. Removal is explicit.
        let data = app.location(DataLocation::Data);
        let lock = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(data.child("update-cache.lock")?)
            .map_err(io_error)?;
        lock.try_lock().map_err(|_| {
            error(
                ErrorCode::Busy,
                "Another update cache owns this application slot",
            )
        })?;
        let store = app.settings(DataScope::Application, "update-transfer", 1, 4096)?;
        let checkpoint: Option<TransferCheckpoint> =
            value(store.load::<Option<TransferCheckpoint>>(&OperationContext::default()))?
                .flatten();
        if let Some(checkpoint) = &checkpoint {
            crate::metadata::hex::<32>(&checkpoint.payload_sha256)?;
            crate::metadata::hex::<32>(&checkpoint.artifact_sha256)?;
            crate::metadata::hex::<32>(&checkpoint.prefix_sha256)?;
            if checkpoint.size == 0
                || checkpoint.size > 2 * 1024 * 1024 * 1024
                || checkpoint.received > checkpoint.size
                || (checkpoint.phase == Phase::Published
                    && (checkpoint.received != checkpoint.size
                        || checkpoint.prefix_sha256 != checkpoint.artifact_sha256))
            {
                return Err(integrity());
            }
        }
        Ok(Self {
            data,
            store,
            checkpoint,
            active: None,
            session: NEXT_CACHE_SESSION.fetch_add(1, Ordering::Relaxed),
            generation: 0,
            durability: Durability::FileAndDirectorySynced,
            _lock: lock,
        })
    }

    /// Revalidate the authenticated offer and rehash the committed prefix.
    /// A synced tail left before checkpoint commit is adopted without deleting
    /// bytes. Conflicting offers, corruption and missing committed data survive
    /// for diagnosis or an explicit discard. HTTP resume uses returned received.
    pub fn resume(
        &mut self,
        verifier: &UpdateVerifier,
        update: &VerifiedUpdate,
        now: u64,
        context: &OperationContext,
    ) -> ServiceResult<CacheTransfer> {
        if let Some(cancelled) = background(context)? {
            return Ok(cancelled);
        }
        verifier.revalidate(update, now)?;
        self.invalidate()?;
        let artifact = &update.metadata().artifact;
        let mut checkpoint = match &self.checkpoint {
            Some(checkpoint) => {
                if checkpoint.phase == Phase::Discarding {
                    return Err(error(
                        ErrorCode::Busy,
                        "Finish the requested cache discard first",
                    ));
                }
                if checkpoint.payload_sha256 != update.payload_sha256()
                    || checkpoint.artifact_sha256 != artifact.sha256
                    || checkpoint.size != artifact.size
                {
                    return Err(error(
                        ErrorCode::Busy,
                        "Discard the previous update cache before changing offers",
                    ));
                }
                checkpoint.clone()
            }
            None => {
                if self.exists("artifact.part")? || self.exists("artifact.verified")? {
                    return Err(integrity());
                }
                let checkpoint = TransferCheckpoint {
                    payload_sha256: update.payload_sha256().into(),
                    artifact_sha256: artifact.sha256.clone(),
                    size: artifact.size,
                    received: 0,
                    prefix_sha256: format!("{:x}", Sha256::digest([])),
                    phase: Phase::Staging,
                };
                if let Completion::Cancelled(reason) =
                    self.commit(Some(checkpoint.clone()), context)?
                {
                    return Ok(Completion::Cancelled(reason));
                }
                checkpoint
            }
        };
        fs::create_dir_all(self.data.child("update-cache")?).map_err(io_error)?;
        self.record_durability(sync_directory(self.data.path()));
        let published = self.exists("artifact.verified")?;
        if published && self.exists("artifact.part")? {
            return Err(integrity());
        }
        let (hash, received) = if published {
            // Recover a crash after rename and before the Published checkpoint.
            let mut file = self.open_file("artifact.verified", true)?;
            match update.verify_artifact(&mut file, context)? {
                Completion::Cancelled(reason) => return Ok(Completion::Cancelled(reason)),
                Completion::Completed(_) => {}
            }
            file.sync_all().map_err(io_error)?;
            self.record_durability(sync_directory(&self.data.child("update-cache")?));
            checkpoint.phase = Phase::Published;
            // No more appends are allowed in Published state.
            (Sha256::new(), artifact.size)
        } else {
            if checkpoint.phase == Phase::Published {
                return Err(integrity());
            }
            if !self.exists("artifact.part")? {
                if checkpoint.received != 0 {
                    return Err(integrity());
                }
                OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(self.path("artifact.part")?)
                    .map_err(io_error)?
                    .sync_all()
                    .map_err(io_error)?;
                self.record_durability(sync_directory(&self.data.child("update-cache")?));
            }
            let mut file = self.open_file("artifact.part", true)?;
            let recovered = match hash_staging(&mut file, &checkpoint, context)? {
                Completion::Completed(recovered) => recovered,
                Completion::Cancelled(reason) => return Ok(Completion::Cancelled(reason)),
            };
            file.sync_all().map_err(io_error)?;
            recovered
        };
        checkpoint.received = received;
        checkpoint.prefix_sha256 = if published {
            artifact.sha256.clone()
        } else {
            format!("{:x}", hash.clone().finalize())
        };
        if let Completion::Cancelled(reason) = self.commit(Some(checkpoint), context)? {
            return Ok(Completion::Cancelled(reason));
        }
        self.active = Some(ActiveTransfer {
            update: update.clone(),
            hash,
        });
        Ok(Completion::Completed(CacheTransfer {
            ticket: self.ticket(),
            progress: self.progress()?,
        }))
    }

    /// An append commits synced bytes before its exact offset checkpoint. Any
    /// write/checkpoint failure invalidates the ticket; resume recovers its tail.
    /// Duplicate, stale and out-of-order transport callbacks cannot append bytes.
    pub fn append(
        &mut self,
        ticket: CacheTicket,
        offset: u64,
        bytes: &[u8],
        context: &OperationContext,
    ) -> ServiceResult<TransferProgress> {
        if let Some(cancelled) = background(context)? {
            return Ok(cancelled);
        }
        self.check_ticket(ticket)?;
        let mut checkpoint = self.checkpoint.clone().ok_or_else(integrity)?;
        if checkpoint.phase != Phase::Staging
            || offset != checkpoint.received
            || bytes.is_empty()
            || bytes.len() > MAX_UPDATE_CHUNK_BYTES
            || bytes.len() as u64 > checkpoint.size - checkpoint.received
        {
            return Err(error(
                ErrorCode::InvalidInput,
                "Update chunk or exact offset is invalid",
            ));
        }
        let mut file = self.open_file("artifact.part", true)?;
        if file.metadata().map_err(io_error)?.len() != offset || self.exists("artifact.verified")? {
            self.active = None;
            return Err(integrity());
        }
        let mut active = self.active.take().ok_or_else(integrity)?;
        file.seek(SeekFrom::Start(offset)).map_err(io_error)?;
        file.write_all(bytes).map_err(io_error)?;
        file.sync_all().map_err(io_error)?;
        active.hash.update(bytes);
        checkpoint.received += bytes.len() as u64;
        checkpoint.prefix_sha256 = format!("{:x}", active.hash.clone().finalize());
        if let Completion::Cancelled(reason) = self.commit(Some(checkpoint), context)? {
            return Ok(Completion::Cancelled(reason));
        }
        self.active = Some(active);
        Ok(Completion::Completed(self.progress()?))
    }

    /// Full size/digest and current trust are checked on every call, including
    /// cache hits. Rename within one private directory publishes atomically.
    /// Cancellation after rename leaves a recoverable publication, not rollback.
    pub fn publish(
        &mut self,
        ticket: CacheTicket,
        verifier: &UpdateVerifier,
        now: u64,
        context: &OperationContext,
    ) -> ServiceResult<CachedArtifact> {
        if let Some(cancelled) = background(context)? {
            return Ok(cancelled);
        }
        self.check_ticket(ticket)?;
        let update = &self.active.as_ref().ok_or_else(integrity)?.update;
        verifier.revalidate(update, now)?;
        let mut checkpoint = self.checkpoint.clone().ok_or_else(integrity)?;
        if checkpoint.received != checkpoint.size {
            return Err(error(
                ErrorCode::InvalidInput,
                "Update transfer is incomplete",
            ));
        }
        let name = match checkpoint.phase {
            Phase::Staging => "artifact.part",
            Phase::Published => "artifact.verified",
            Phase::Discarding => return Err(integrity()),
        };
        let active = self.active.take().ok_or_else(integrity)?;
        self.open_file(name, true)?.sync_all().map_err(io_error)?;
        let mut file = self.open_file(name, false)?;
        let verification = match active.update.verify_artifact(&mut file, context)? {
            Completion::Completed(verification) => verification,
            Completion::Cancelled(reason) => return Ok(Completion::Cancelled(reason)),
        };
        file.seek(SeekFrom::Start(0)).map_err(io_error)?;
        verifier.revalidate(&active.update, now)?;
        if checkpoint.phase == Phase::Staging {
            if self.exists("artifact.verified")? {
                return Err(integrity());
            }
            fs::rename(self.path(name)?, self.path("artifact.verified")?).map_err(io_error)?;
        }
        self.record_durability(sync_directory(&self.data.child("update-cache")?));
        checkpoint.phase = Phase::Published;
        checkpoint.prefix_sha256 = verification.artifact_sha256.clone();
        if let Completion::Cancelled(reason) = self.commit(Some(checkpoint), context)? {
            return Ok(Completion::Cancelled(reason));
        }
        self.active = Some(active);
        Ok(Completion::Completed(CachedArtifact {
            file,
            verification,
            durability: self.durability,
        }))
    }

    /// Explicitly delete this slot's two known files. Persist deletion intent
    /// first so process death/cancellation can be retried via discard after open.
    /// Unknown files and corrupt settings are preserved for application recovery.
    pub fn discard(&mut self, context: &OperationContext) -> ServiceResult<()> {
        if let Some(cancelled) = background(context)? {
            return Ok(cancelled);
        }
        self.invalidate()?;
        let Some(mut checkpoint) = self.checkpoint.clone() else {
            return Ok(Completion::Completed(()));
        };
        checkpoint.phase = Phase::Discarding;
        if let Completion::Cancelled(reason) = self.commit(Some(checkpoint), context)? {
            return Ok(Completion::Cancelled(reason));
        }
        for name in ["artifact.part", "artifact.verified"] {
            if let Some(cancelled) = background(context)? {
                return Ok(cancelled);
            }
            if self.exists(name)? {
                fs::remove_file(self.path(name)?).map_err(io_error)?;
            }
        }
        if self.data.child("update-cache")?.is_dir() {
            self.record_durability(sync_directory(&self.data.child("update-cache")?));
        }
        self.commit(None, context)
    }

    fn commit(
        &mut self,
        checkpoint: Option<TransferCheckpoint>,
        context: &OperationContext,
    ) -> ServiceResult<()> {
        match self.store.save(&checkpoint, context)? {
            Completion::Completed(receipt) => {
                self.record_durability(receipt.durability);
                self.checkpoint = checkpoint;
                Ok(Completion::Completed(()))
            }
            Completion::Cancelled(reason) => Ok(Completion::Cancelled(reason)),
        }
    }

    fn record_durability(&mut self, durability: Durability) {
        use Durability::*;
        self.durability = match (self.durability, durability) {
            (CommittedDirectorySyncUncertain, _) | (_, CommittedDirectorySyncUncertain) => {
                CommittedDirectorySyncUncertain
            }
            (FileSynced, _) | (_, FileSynced) => FileSynced,
            _ => FileAndDirectorySynced,
        };
    }

    fn progress(&self) -> Result<TransferProgress, SystemError> {
        let checkpoint = self.checkpoint.as_ref().ok_or_else(integrity)?;
        Ok(TransferProgress {
            received: checkpoint.received,
            total: checkpoint.size,
            state: match checkpoint.phase {
                Phase::Staging => ArtifactCacheState::Staging,
                Phase::Published => ArtifactCacheState::Published,
                Phase::Discarding => return Err(integrity()),
            },
        })
    }

    fn ticket(&self) -> CacheTicket {
        CacheTicket {
            session: self.session,
            generation: self.generation,
        }
    }

    fn check_ticket(&self, ticket: CacheTicket) -> Result<(), SystemError> {
        if self.ticket() != ticket || self.active.is_none() {
            return Err(error(
                ErrorCode::ContextExpired,
                "Update transfer ticket expired; resume first",
            ));
        }
        Ok(())
    }

    fn invalidate(&mut self) -> Result<(), SystemError> {
        self.active = None;
        self.generation = self
            .generation
            .checked_add(1)
            .ok_or_else(|| error(ErrorCode::Unavailable, "Update cache generation exhausted"))?;
        Ok(())
    }

    fn path(&self, name: &str) -> Result<PathBuf, SystemError> {
        self.data.child(PathBuf::from("update-cache").join(name))
    }

    fn exists(&self, name: &str) -> Result<bool, SystemError> {
        match fs::symlink_metadata(self.path(name)?) {
            Ok(metadata) if metadata.is_file() && !metadata.file_type().is_symlink() => Ok(true),
            Ok(_) => Err(integrity()),
            Err(cause) if cause.kind() == std::io::ErrorKind::NotFound => Ok(false),
            Err(cause) => Err(io_error(cause)),
        }
    }

    fn open_file(&self, name: &str, write: bool) -> Result<File, SystemError> {
        if !self.exists(name)? {
            return Err(integrity());
        }
        OpenOptions::new()
            .read(true)
            .write(write)
            .open(self.path(name)?)
            .map_err(io_error)
    }
}

fn hash_staging(
    file: &mut File,
    checkpoint: &TransferCheckpoint,
    context: &OperationContext,
) -> ServiceResult<(Sha256, u64)> {
    let length = file.metadata().map_err(io_error)?.len();
    if length < checkpoint.received || length > checkpoint.size {
        return Err(integrity());
    }
    let mut hash = Sha256::new();
    let mut buffer = [0; 65536];
    let mut received = 0;
    // Check the old committed prefix before hashing/adopting its uncommitted tail.
    for end in [checkpoint.received, length] {
        while received < end {
            if let Some(cancelled) = background(context)? {
                return Ok(cancelled);
            }
            let count = (end - received).min(buffer.len() as u64) as usize;
            file.read_exact(&mut buffer[..count]).map_err(io_error)?;
            hash.update(&buffer[..count]);
            received += count as u64;
        }
        if end == checkpoint.received
            && format!("{:x}", hash.clone().finalize()) != checkpoint.prefix_sha256
        {
            return Err(integrity());
        }
    }
    Ok(Completion::Completed((hash, received)))
}

fn sync_directory(path: &std::path::Path) -> Durability {
    #[cfg(unix)]
    {
        match File::open(path).and_then(|directory| directory.sync_all()) {
            Ok(()) => Durability::FileAndDirectorySynced,
            Err(_) => Durability::CommittedDirectorySyncUncertain,
        }
    }
    #[cfg(not(unix))]
    {
        let _ = path;
        Durability::FileSynced
    }
}

fn integrity() -> SystemError {
    error(
        ErrorCode::IntegrityFailure,
        "Update cache is inconsistent; original preserved",
    )
}

fn io_error(_: std::io::Error) -> SystemError {
    error(
        ErrorCode::PlatformFailure,
        "Private update cache I/O failed; resume required",
    )
}
