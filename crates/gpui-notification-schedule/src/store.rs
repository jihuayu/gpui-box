//! Atomic, bounded and checksummed durable schedule storage under the private
//! `notification-schedule` application-data namespace.
//!
//! One committed envelope per schedule identity. Files are staged in the same
//! directory, flushed, synced and atomically renamed. Corrupt, oversized,
//! wrong-version, checksum-mismatched or semantically invalid files are moved
//! into `quarantine/` and reported without aborting healthy records.

use crate::model::{NotificationScheduleRecord, ScheduledNotificationId, hex_encode};
use gpui_app_data::{AppData, DataLocation};
use gpui_system::{ErrorCode, SystemError};
use serde::{Serialize, de::DeserializeOwned};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File},
    io::{self, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
};

/// Durable notification-schedule envelope schema version.
pub const NOTIFICATION_SCHEDULE_FORMAT_VERSION: u32 = 1;
/// Private namespace below the application data directory.
pub const NOTIFICATION_SCHEDULE_NAMESPACE: &str = "notification-schedule";
/// Directory that retains corrupt records instead of deleting evidence.
pub const QUARANTINE_DIR: &str = "quarantine";
/// Largest accepted serialized schedule record.
pub const MAX_SCHEDULE_RECORD_BYTES: u64 = 256 * 1024;
/// Hard bound on one serialized envelope, including framing.
pub const MAX_SCHEDULE_ENVELOPE_BYTES: u64 = MAX_SCHEDULE_RECORD_BYTES + 4096;

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
    let mut error = error(code, "Notification schedule storage I/O failed");
    error.retry = gpui_system::RetryAdvice::AfterConditionsChange;
    error
}

/// Test/host injection point for deterministic write and rename failures.
pub trait ScheduleStorageHook: Send + Sync + 'static {
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
pub struct NoopScheduleStorageHook;

impl ScheduleStorageHook for NoopScheduleStorageHook {}

/// One-shot fault injection for disk-full, rename, truncated-write and
/// bit-rot tests.
#[derive(Debug, Default)]
pub struct ScheduleStorageFaultInjection {
    fail_write: AtomicBool,
    fail_rename: AtomicBool,
    truncate: AtomicU64,
    corrupt: AtomicBool,
}

impl ScheduleStorageFaultInjection {
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

impl ScheduleStorageHook for ScheduleStorageFaultInjection {
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
pub type ScheduleStorageFault = ScheduleStorageFaultInjection;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum QuarantineKind {
    ScheduleRecord,
}

impl QuarantineKind {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::ScheduleRecord => "schedule_record",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct QuarantinedRecord {
    pub kind: QuarantineKind,
    pub schedule_id: Option<ScheduledNotificationId>,
    pub bytes: u64,
}

#[derive(Debug, Default)]
pub(crate) struct LoadedScheduleStore {
    pub records: Vec<NotificationScheduleRecord>,
    pub quarantined: Vec<QuarantinedRecord>,
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
    hasher.update(b"gpui-notification-schedule-envelope\0");
    hasher.update(NOTIFICATION_SCHEDULE_FORMAT_VERSION.to_le_bytes());
    hasher.update(payload);
    hex_encode(&hasher.finalize())
}

/// Atomic durable journal for scheduled notifications.
#[derive(Clone)]
pub(crate) struct ScheduleStore {
    directory: PathBuf,
    hook: Arc<dyn ScheduleStorageHook>,
}

impl std::fmt::Debug for ScheduleStore {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ScheduleStore")
            .field("directory_is_private", &true)
            .finish()
    }
}

impl ScheduleStore {
    /// Resolve the private namespace without creating files.
    pub fn open(app: &AppData, hook: Arc<dyn ScheduleStorageHook>) -> Result<Self, SystemError> {
        let directory = app
            .location(DataLocation::Data)
            .child(NOTIFICATION_SCHEDULE_NAMESPACE)
            .map_err(|_| {
                error(
                    ErrorCode::InvalidConfiguration,
                    "Notification schedule namespace is not a private storage path",
                )
            })?;
        Ok(Self { directory, hook })
    }

    pub fn directory(&self) -> &Path {
        &self.directory
    }

    /// Highest durable schedule epoch, used to seed epoch allocation before
    /// the first reconciliation so a fresh process cannot reuse an epoch that
    /// is still on disk. Corrupt and unrelated files are ignored here; they are
    /// reported by [`Self::load_all`].
    pub fn max_persisted_epoch(&self) -> u64 {
        let Ok(entries) = fs::read_dir(&self.directory) else {
            return 0;
        };
        let mut max_epoch = 0;
        for entry in entries.flatten() {
            let name = entry.file_name();
            let Some(name) = name.to_str() else {
                continue;
            };
            if !name.starts_with("schedule-") || !name.ends_with(".json") {
                continue;
            }
            let Ok(bytes) = fs::read(entry.path()) else {
                continue;
            };
            let Ok(envelope) =
                serde_json::from_slice::<Envelope<NotificationScheduleRecord>>(&bytes)
            else {
                continue;
            };
            if envelope.schema_version == NOTIFICATION_SCHEDULE_FORMAT_VERSION {
                max_epoch = max_epoch.max(envelope.payload.schedule_epoch);
            }
        }
        max_epoch
    }

    /// Load every committed schedule, quarantining corrupt files without
    /// panicking.
    pub fn load_all(&self) -> Result<LoadedScheduleStore, SystemError> {
        let mut loaded = LoadedScheduleStore::default();
        let entries = match fs::read_dir(&self.directory) {
            Ok(entries) => entries,
            Err(cause) if cause.kind() == io::ErrorKind::NotFound => return Ok(loaded),
            Err(cause) => return Err(io_error(cause)),
        };
        let mut files = Vec::new();
        for entry in entries {
            let entry = entry.map_err(io_error)?;
            let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
                continue;
            };
            if name.starts_with("schedule-") && name.ends_with(".json") {
                files.push(name);
            }
        }
        files.sort();
        for name in files {
            let hint = self.storage_hint(&name);
            match self.load_envelope::<NotificationScheduleRecord>(&name) {
                Ok(LoadState::Missing) => {}
                Ok(LoadState::Loaded(record)) => {
                    if record.schedule_id.storage_id() != hint || record.validate().is_err() {
                        loaded.quarantined.push(self.quarantine(
                            &name,
                            entry_len(&self.directory.join(&name)),
                            Some(record.schedule_id),
                        ));
                        continue;
                    }
                    loaded.records.push(record);
                }
                Ok(LoadState::Corrupt { bytes }) => {
                    loaded.quarantined.push(self.quarantine(&name, bytes, None));
                }
                Err(cause) => return Err(cause),
            }
        }
        Ok(loaded)
    }

    /// Commit one schedule record atomically after semantic validation.
    pub fn save_record(&self, record: &NotificationScheduleRecord) -> Result<(), SystemError> {
        record.validate()?;
        self.write_envelope(&schedule_file_name(&record.schedule_id), record)
    }

    /// Remove one committed record. A missing file is not an error.
    pub fn delete_record(&self, schedule_id: &ScheduledNotificationId) -> Result<(), SystemError> {
        let path = self.directory.join(schedule_file_name(schedule_id));
        match fs::remove_file(path) {
            Ok(()) => Ok(()),
            Err(cause) if cause.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(cause) => Err(io_error(cause)),
        }
    }

    fn write_envelope<T: Serialize>(&self, name: &str, payload: &T) -> Result<(), SystemError> {
        let payload_bytes = serde_json::to_vec(payload).map_err(|_| {
            error(
                ErrorCode::IntegrityFailure,
                "Schedule record cannot be encoded",
            )
        })?;
        if payload_bytes.len() as u64 > MAX_SCHEDULE_RECORD_BYTES {
            return Err(error(
                ErrorCode::InvalidInput,
                "Schedule record exceeds its durable size bound",
            ));
        }
        let envelope = Envelope {
            schema_version: NOTIFICATION_SCHEDULE_FORMAT_VERSION,
            checksum: envelope_digest(&payload_bytes),
            payload,
        };
        let bytes = serde_json::to_vec(&envelope).map_err(|_| {
            error(
                ErrorCode::IntegrityFailure,
                "Schedule envelope cannot be encoded",
            )
        })?;
        if bytes.len() as u64 > MAX_SCHEDULE_ENVELOPE_BYTES {
            return Err(error(
                ErrorCode::InvalidInput,
                "Schedule envelope exceeds its durable size bound",
            ));
        }
        fs::create_dir_all(&self.directory).map_err(io_error)?;
        let mut staging = tempfile::Builder::new()
            .prefix(".gpui-notification-schedule-")
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

    fn load_envelope<T>(&self, name: &str) -> Result<LoadState<T>, SystemError>
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
        if !metadata.is_file()
            || metadata.file_type().is_symlink()
            || metadata.len() > MAX_SCHEDULE_ENVELOPE_BYTES
        {
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
        if bytes.len() as u64 > MAX_SCHEDULE_ENVELOPE_BYTES {
            return Ok(LoadState::Corrupt {
                bytes: bytes.len() as u64,
            });
        }
        let envelope = match serde_json::from_slice::<Envelope<T>>(&bytes) {
            Ok(envelope) if envelope.schema_version == NOTIFICATION_SCHEDULE_FORMAT_VERSION => {
                envelope
            }
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
        schedule_id: Option<ScheduledNotificationId>,
    ) -> QuarantinedRecord {
        let directory = self.directory.join(QUARANTINE_DIR);
        let _ = fs::create_dir_all(&directory);
        let sequence = QUARANTINE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let stored_name = format!("{file_name}.{sequence}.corrupt");
        let source = self.directory.join(file_name);
        let _ = fs::rename(&source, directory.join(stored_name));
        QuarantinedRecord {
            kind: QuarantineKind::ScheduleRecord,
            schedule_id,
            bytes,
        }
    }

    fn storage_hint(&self, file_name: &str) -> String {
        file_name
            .strip_prefix("schedule-")
            .and_then(|name| name.strip_suffix(".json"))
            .unwrap_or(file_name)
            .to_owned()
    }
}

fn schedule_file_name(schedule_id: &ScheduledNotificationId) -> String {
    format!("schedule-{}.json", schedule_id.storage_id())
}

fn entry_len(path: &Path) -> u64 {
    fs::metadata(path)
        .map(|metadata| metadata.len())
        .unwrap_or(0)
}
