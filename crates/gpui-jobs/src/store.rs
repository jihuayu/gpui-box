//! Atomic, bounded and checksummed durable jobs storage under the private
//! `jobs` application-data namespace.
//!
//! Every durable operation is a blocking background primitive. A UI-attached
//! [`gpui_system::OperationContext`] is rejected by the service before this
//! module is reached. Files are staged in the same directory, synced and then
//! atomically renamed. Job records use one committed file; checkpoints use two
//! alternating committed slots so the previous valid checkpoint remains
//! available when the newest slot is truncated or corrupted.

use crate::model::{
    JobCheckpoint, JobKey, JobRecord, MAX_CHECKPOINT_BYTES, MAX_JOB_RECORD_BYTES, hex_encode,
};
use gpui_app_data::{AppData, DataLocation};
use gpui_system::{ErrorCode, SystemError};
use serde::{Serialize, de::DeserializeOwned};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, File},
    io::{self, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
};

/// Durable jobs envelope schema version.
pub const JOBS_FORMAT_VERSION: u32 = 1;
/// Private namespace below the application data directory.
pub const JOBS_NAMESPACE: &str = "jobs";
/// Directory that retains corrupt records instead of deleting evidence.
pub const QUARANTINE_DIR: &str = "quarantine";
/// Hard bound on one serialized envelope, including framing.
pub const MAX_ENVELOPE_BYTES: u64 = MAX_JOB_RECORD_BYTES + MAX_CHECKPOINT_BYTES as u64 + 4096;

const TRUNCATE_DISABLED: u64 = u64::MAX;
static QUARANTINE_SEQUENCE: AtomicU64 = AtomicU64::new(1);

fn error(code: ErrorCode, message: &'static str) -> SystemError {
    SystemError::new(code, message)
}

fn io_error(cause: io::Error) -> SystemError {
    let code = match cause.kind() {
        io::ErrorKind::PermissionDenied => ErrorCode::PermissionDenied,
        io::ErrorKind::WouldBlock => ErrorCode::Busy,
        _ => ErrorCode::PlatformFailure,
    };
    let mut error = error(code, "Background job storage I/O failed");
    error.retry = gpui_system::RetryAdvice::AfterConditionsChange;
    error
}

/// Test/host injection point for deterministic write and rename failures.
pub trait JobsStorageHook: Send + Sync + 'static {
    fn before_write(&self, file: &str) -> io::Result<()> {
        let _ = file;
        Ok(())
    }

    /// Called after the staging bytes are flushed and before they are synced.
    fn after_write(&self, file: &str, staging: &mut File) -> io::Result<()> {
        let _ = (file, staging);
        Ok(())
    }

    fn before_rename(&self, file: &str) -> io::Result<()> {
        let _ = file;
        Ok(())
    }
}

/// Default no-op hook.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoopJobsStorageHook;

impl JobsStorageHook for NoopJobsStorageHook {}

/// One-shot fault injection for disk-full, rename, truncated-write and
/// bit-rot tests.
#[derive(Debug, Default)]
pub struct JobsStorageFaultInjection {
    fail_write: AtomicBool,
    fail_rename: AtomicBool,
    truncate: AtomicU64,
    corrupt: AtomicBool,
}

impl JobsStorageFaultInjection {
    pub fn new() -> Self {
        Self {
            fail_write: AtomicBool::new(false),
            fail_rename: AtomicBool::new(false),
            truncate: AtomicU64::new(TRUNCATE_DISABLED),
            corrupt: AtomicBool::new(false),
        }
    }

    pub fn fail_next_write(&self) {
        self.fail_write.store(true, Ordering::SeqCst);
    }

    pub fn fail_next_rename(&self) {
        self.fail_rename.store(true, Ordering::SeqCst);
    }

    /// Truncate the next staged file to at most `bytes` bytes.
    pub fn truncate_next_write_to(&self, bytes: u64) {
        self.truncate.store(bytes, Ordering::SeqCst);
    }

    /// Replace the next staged file with invalid JSON.
    pub fn corrupt_next_write(&self) {
        self.corrupt.store(true, Ordering::SeqCst);
    }

    pub fn clear(&self) {
        self.fail_write.store(false, Ordering::SeqCst);
        self.fail_rename.store(false, Ordering::SeqCst);
        self.truncate.store(TRUNCATE_DISABLED, Ordering::SeqCst);
        self.corrupt.store(false, Ordering::SeqCst);
    }

    pub fn write_failure_pending(&self) -> bool {
        self.fail_write.load(Ordering::SeqCst)
    }

    pub fn rename_failure_pending(&self) -> bool {
        self.fail_rename.load(Ordering::SeqCst)
    }
}

impl JobsStorageHook for JobsStorageFaultInjection {
    fn before_write(&self, _file: &str) -> io::Result<()> {
        if self.fail_write.swap(false, Ordering::SeqCst) {
            return Err(io::Error::from(io::ErrorKind::StorageFull));
        }
        Ok(())
    }

    fn after_write(&self, _file: &str, staging: &mut File) -> io::Result<()> {
        let requested = self.truncate.swap(TRUNCATE_DISABLED, Ordering::SeqCst);
        if requested != TRUNCATE_DISABLED {
            let length = staging.metadata()?.len().min(requested);
            staging.set_len(length)?;
        }
        if self.corrupt.swap(false, Ordering::SeqCst) {
            staging.set_len(0)?;
            staging.seek(SeekFrom::Start(0))?;
            staging.write_all(b"{\"schema_version\":1,\"checksum\":\"truncated")?;
        }
        Ok(())
    }

    fn before_rename(&self, _file: &str) -> io::Result<()> {
        if self.fail_rename.swap(false, Ordering::SeqCst) {
            return Err(io::Error::from(io::ErrorKind::StorageFull));
        }
        Ok(())
    }
}

/// Compatibility alias for a one-shot storage fault injector.
pub type JobsStorageFault = JobsStorageFaultInjection;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum QuarantineKind {
    JobRecord,
    Checkpoint,
}

impl QuarantineKind {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::JobRecord => "job_record",
            Self::Checkpoint => "checkpoint",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct QuarantinedRecord {
    pub kind: QuarantineKind,
    pub job_key: Option<JobKey>,
    pub bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CheckpointRecovery {
    pub job_key: JobKey,
    pub run_epoch: u64,
    pub attempt: u32,
    pub sequence: u64,
    pub previous_sequence: Option<u64>,
}

#[derive(Debug, Default)]
pub(crate) struct LoadedStore {
    pub jobs: Vec<JobRecord>,
    pub checkpoints: BTreeMap<JobKey, JobCheckpoint>,
    pub quarantined: Vec<QuarantinedRecord>,
    pub recovered_checkpoints: Vec<CheckpointRecovery>,
}

#[derive(Debug)]
enum LoadState<T> {
    Missing,
    Loaded(T),
    Corrupt { bytes: u64 },
}

#[derive(Debug, Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Envelope<T> {
    schema_version: u32,
    checksum: String,
    payload: T,
}

fn envelope_digest(payload: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"gpui-jobs-envelope\0");
    hasher.update(JOBS_FORMAT_VERSION.to_le_bytes());
    hasher.update(payload);
    hex_encode(&hasher.finalize())
}

/// Atomic durable journal for job records and checkpoints.
#[derive(Clone)]
pub(crate) struct JobStore {
    directory: PathBuf,
    hook: Arc<dyn JobsStorageHook>,
}

impl std::fmt::Debug for JobStore {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("JobStore")
            .field("directory_is_private", &true)
            .finish()
    }
}

impl JobStore {
    /// Resolve the private jobs namespace without creating files.
    pub fn open(app: &AppData, hook: Arc<dyn JobsStorageHook>) -> Result<Self, SystemError> {
        let directory = app
            .location(DataLocation::Data)
            .child(JOBS_NAMESPACE)
            .map_err(|_| {
                error(
                    ErrorCode::InvalidConfiguration,
                    "Jobs namespace is not a private storage path",
                )
            })?;
        Ok(Self { directory, hook })
    }

    pub fn directory(&self) -> &Path {
        &self.directory
    }

    /// Load every committed job and checkpoint, quarantining corrupt files
    /// without panicking.
    pub fn load_all(&self) -> Result<LoadedStore, SystemError> {
        let mut loaded = LoadedStore::default();
        let entries = match fs::read_dir(&self.directory) {
            Ok(entries) => entries,
            Err(cause) if cause.kind() == io::ErrorKind::NotFound => return Ok(loaded),
            Err(cause) => return Err(io_error(cause)),
        };
        let mut job_files = Vec::new();
        for entry in entries {
            let entry = entry.map_err(io_error)?;
            let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
                continue;
            };
            if name.starts_with("job-") && name.ends_with(".json") {
                job_files.push(name);
            }
        }
        job_files.sort();
        let mut seen = BTreeSet::new();
        let mut keys = Vec::new();
        for name in job_files {
            let key_hint = self.key_hint(&name);
            match self.load_envelope::<JobRecord>(&name, MAX_JOB_RECORD_BYTES) {
                Ok(LoadState::Missing) => {}
                Ok(LoadState::Loaded(record)) => {
                    if record.key.storage_id() != key_hint || record.validate().is_err() {
                        loaded.quarantined.push(self.quarantine(
                            &name,
                            entry_len(&self.directory.join(&name)),
                            QuarantineKind::JobRecord,
                            Some(record.key.clone()),
                        ));
                        continue;
                    }
                    if seen.insert(record.key.clone()) {
                        keys.push(record.key.clone());
                        loaded.jobs.push(record);
                    }
                }
                Ok(LoadState::Corrupt { bytes }) => {
                    loaded.quarantined.push(self.quarantine(
                        &name,
                        bytes,
                        QuarantineKind::JobRecord,
                        None,
                    ));
                }
                Err(cause) => return Err(cause),
            }
        }
        for key in keys {
            self.load_checkpoints(&key, &mut loaded)?;
        }
        Ok(loaded)
    }

    fn load_checkpoints(&self, key: &JobKey, loaded: &mut LoadedStore) -> Result<(), SystemError> {
        let mut valid = Vec::new();
        let mut corrupt = Vec::new();
        for slot in 0u64..2 {
            let name = checkpoint_file_name(key, slot);
            match self.load_envelope::<JobCheckpoint>(&name, MAX_ENVELOPE_BYTES) {
                Ok(LoadState::Missing) => {}
                Ok(LoadState::Loaded(checkpoint)) => {
                    if checkpoint.job_key != *key
                        || checkpoint.sequence % 2 != slot
                        || !checkpoint.verify()
                    {
                        corrupt.push((name, checkpoint.payload.len() as u64, Some(slot)));
                    } else {
                        valid.push((slot, checkpoint));
                    }
                }
                Ok(LoadState::Corrupt { bytes }) => corrupt.push((name, bytes, Some(slot))),
                Err(cause) => return Err(cause),
            }
        }
        let chosen = valid
            .iter()
            .map(|(_, checkpoint)| checkpoint)
            .max_by_key(|checkpoint| checkpoint.sequence)
            .cloned();
        let previous_sequence = valid
            .iter()
            .filter(|(_, checkpoint)| Some(checkpoint) != chosen.as_ref())
            .map(|(_, checkpoint)| checkpoint.sequence)
            .max();
        if let Some(checkpoint) = &chosen {
            loaded.checkpoints.insert(key.clone(), checkpoint.clone());
            if !corrupt.is_empty() {
                loaded.recovered_checkpoints.push(CheckpointRecovery {
                    job_key: key.clone(),
                    run_epoch: checkpoint.run_epoch,
                    attempt: checkpoint.attempt,
                    sequence: checkpoint.sequence,
                    previous_sequence,
                });
            }
        }
        for (name, bytes, _) in corrupt {
            loaded.quarantined.push(self.quarantine(
                &name,
                bytes,
                QuarantineKind::Checkpoint,
                Some(key.clone()),
            ));
        }
        Ok(())
    }

    /// Commit one job record atomically.
    pub fn save_job(&self, record: &JobRecord) -> Result<(), SystemError> {
        let name = job_file_name(&record.key);
        self.write_envelope(&name, record, MAX_JOB_RECORD_BYTES)
    }

    /// Commit one checkpoint into its alternating slot.
    pub fn save_checkpoint(&self, checkpoint: &JobCheckpoint) -> Result<(), SystemError> {
        if checkpoint.payload.len() > MAX_CHECKPOINT_BYTES || !checkpoint.verify() {
            return Err(error(
                ErrorCode::IntegrityFailure,
                "Checkpoint payload is invalid",
            ));
        }
        let slot = checkpoint.sequence % 2;
        let name = checkpoint_file_name(&checkpoint.job_key, slot);
        self.write_envelope(&name, checkpoint, MAX_ENVELOPE_BYTES)
    }

    /// Remove both checkpoint slots, for an explicit definition replacement.
    pub fn clear_checkpoints(&self, key: &JobKey) -> Result<(), SystemError> {
        for slot in 0u64..2 {
            let path = self.directory.join(checkpoint_file_name(key, slot));
            match fs::remove_file(path) {
                Ok(()) => {}
                Err(cause) if cause.kind() == io::ErrorKind::NotFound => {}
                Err(cause) => return Err(io_error(cause)),
            }
        }
        Ok(())
    }

    fn write_envelope<T: Serialize>(
        &self,
        name: &str,
        payload: &T,
        max_payload: u64,
    ) -> Result<(), SystemError> {
        let payload_bytes = serde_json::to_vec(payload)
            .map_err(|_| error(ErrorCode::IntegrityFailure, "Job record cannot be encoded"))?;
        if payload_bytes.len() as u64 > max_payload {
            return Err(error(
                ErrorCode::InvalidInput,
                "Job record exceeds its durable size bound",
            ));
        }
        let envelope = Envelope {
            schema_version: JOBS_FORMAT_VERSION,
            checksum: envelope_digest(&payload_bytes),
            payload,
        };
        let bytes = serde_json::to_vec(&envelope).map_err(|_| {
            error(
                ErrorCode::IntegrityFailure,
                "Job envelope cannot be encoded",
            )
        })?;
        if bytes.len() as u64 > MAX_ENVELOPE_BYTES {
            return Err(error(
                ErrorCode::InvalidInput,
                "Job envelope exceeds its durable size bound",
            ));
        }
        fs::create_dir_all(&self.directory).map_err(io_error)?;
        let mut staging = tempfile::Builder::new()
            .prefix(".gpui-jobs-")
            .tempfile_in(&self.directory)
            .map_err(io_error)?;
        self.hook.before_write(name).map_err(io_error)?;
        staging.write_all(&bytes).map_err(io_error)?;
        staging.flush().map_err(io_error)?;
        self.hook
            .after_write(name, staging.as_file_mut())
            .map_err(io_error)?;
        staging.as_file().sync_all().map_err(io_error)?;
        self.hook.before_rename(name).map_err(io_error)?;
        staging
            .persist(self.directory.join(name))
            .map_err(|failure| io_error(failure.error))?;
        Ok(())
    }

    fn load_envelope<T>(&self, name: &str, max_bytes: u64) -> Result<LoadState<T>, SystemError>
    where
        T: DeserializeOwned + Serialize,
    {
        let path = self.directory.join(name);
        let metadata = match fs::symlink_metadata(&path) {
            Ok(metadata) => metadata,
            Err(cause) if cause.kind() == io::ErrorKind::NotFound => {
                return Ok(LoadState::Missing);
            }
            Err(cause) => return Err(io_error(cause)),
        };
        if !metadata.is_file() || metadata.file_type().is_symlink() || metadata.len() > max_bytes {
            return Ok(LoadState::Corrupt {
                bytes: metadata.len(),
            });
        }
        let bytes = match fs::read(&path) {
            Ok(bytes) => bytes,
            Err(cause) if cause.kind() == io::ErrorKind::NotFound => {
                return Ok(LoadState::Missing);
            }
            Err(cause) => return Err(io_error(cause)),
        };
        if bytes.len() as u64 > max_bytes {
            return Ok(LoadState::Corrupt {
                bytes: bytes.len() as u64,
            });
        }
        let envelope = match serde_json::from_slice::<Envelope<T>>(&bytes) {
            Ok(envelope) if envelope.schema_version == JOBS_FORMAT_VERSION => envelope,
            Ok(_) | Err(_) => {
                return Ok(LoadState::Corrupt {
                    bytes: bytes.len() as u64,
                });
            }
        };
        let payload_bytes = match serde_json::to_vec(&envelope.payload) {
            Ok(payload_bytes) => payload_bytes,
            Err(_) => {
                return Ok(LoadState::Corrupt {
                    bytes: bytes.len() as u64,
                });
            }
        };
        if envelope.checksum != envelope_digest(&payload_bytes) {
            return Ok(LoadState::Corrupt {
                bytes: bytes.len() as u64,
            });
        }
        Ok(LoadState::Loaded(envelope.payload))
    }

    fn quarantine(
        &self,
        file_name: &str,
        bytes: u64,
        kind: QuarantineKind,
        job_key: Option<JobKey>,
    ) -> QuarantinedRecord {
        let directory = self.directory.join(QUARANTINE_DIR);
        let _ = fs::create_dir_all(&directory);
        let sequence = QUARANTINE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let stored_name = format!("{file_name}.{sequence}.corrupt");
        let source = self.directory.join(file_name);
        let _ = fs::rename(&source, directory.join(stored_name));
        QuarantinedRecord {
            kind,
            job_key,
            bytes,
        }
    }

    fn key_hint(&self, file_name: &str) -> String {
        file_name
            .strip_prefix("job-")
            .and_then(|name| name.strip_suffix(".json"))
            .unwrap_or(file_name)
            .to_owned()
    }
}

fn job_file_name(key: &JobKey) -> String {
    format!("job-{}.json", key.storage_id())
}

fn checkpoint_file_name(key: &JobKey, slot: u64) -> String {
    format!("checkpoint-{}.{slot}.json", key.storage_id())
}

fn entry_len(path: &Path) -> u64 {
    fs::metadata(path)
        .map(|metadata| metadata.len())
        .unwrap_or(0)
}
