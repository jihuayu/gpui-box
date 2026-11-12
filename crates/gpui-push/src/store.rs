//! Atomic, bounded and checksummed push storage under the private `push`
//! application-data namespace.
//!
//! Every durable operation is a blocking background primitive; the service
//! rejects a UI-attached [`gpui_system::OperationContext`] before this module is
//! reached. Files are staged in the same directory, synced and then atomically
//! renamed. Each file is a versioned envelope whose SHA-256 digest covers a
//! domain separator, the schema version and the serialized payload.
//!
//! A corrupt, truncated or tampered payload is moved into a bounded
//! `quarantine/` directory and recovered as an empty default. Loading never
//! panics and never silently accepts damaged evidence.

use crate::dedup::DedupJournal;
use crate::model::PushDurableState;
use gpui_app_data::{AppData, DataLocation};
use gpui_system::{ErrorCode, SystemError};
use serde::{Serialize, de::DeserializeOwned};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{self, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    time::SystemTime,
};

/// Durable push envelope schema version.
pub const PUSH_FORMAT_VERSION: u32 = 1;
/// Private namespace below the application data directory.
pub const PUSH_NAMESPACE: &str = "push";
/// Directory that retains corrupt payloads instead of deleting evidence.
pub const QUARANTINE_DIR: &str = "quarantine";
/// Largest number of quarantined payloads retained.
pub const MAX_QUARANTINE_ENTRIES: usize = 8;
/// Hard bound on one durable push state envelope.
pub const MAX_STATE_ENVELOPE_BYTES: u64 = 512 * 1024;
/// Hard bound on one durable dedup journal envelope.
pub const MAX_JOURNAL_ENVELOPE_BYTES: u64 = 512 * 1024;

const STATE_FILE: &str = "push-state.json";
const JOURNAL_FILE: &str = "dedup-journal.json";
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
    let mut error = error(code, "Push storage I/O failed");
    error.retry = gpui_system::RetryAdvice::AfterConditionsChange;
    error
}

fn hex_encode(bytes: &[u8]) -> String {
    let mut encoded = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        encoded.push_str(&format!("{byte:02x}"));
    }
    encoded
}

#[derive(Debug, Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Envelope<T> {
    format_version: u32,
    checksum: String,
    payload: T,
}

fn envelope_digest(payload: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"gpui-push-envelope\0");
    hasher.update(PUSH_FORMAT_VERSION.to_le_bytes());
    hasher.update(payload);
    hex_encode(&hasher.finalize())
}

#[derive(Debug)]
enum LoadState<T> {
    Missing,
    Loaded(T),
    Corrupt { bytes: u64 },
}

/// A payload that failed integrity validation and was retained for inspection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuarantinedPayload {
    pub file: &'static str,
    pub bytes: u64,
    pub quarantined_files: usize,
}

/// The recovered value plus optional quarantine diagnostics.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoadOutcome<T> {
    pub value: T,
    pub quarantined: Option<QuarantinedPayload>,
}

/// Atomic durable push store rooted at the private `push` data location.
#[derive(Clone)]
pub struct PushStore {
    directory: PathBuf,
}

impl std::fmt::Debug for PushStore {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("PushStore")
            .field("directory_is_private", &true)
            .finish()
    }
}

impl PushStore {
    /// Resolve the private push namespace without creating files.
    pub fn open(app: &AppData) -> Result<Self, SystemError> {
        let directory = app
            .location(DataLocation::Data)
            .child(PUSH_NAMESPACE)
            .map_err(|_| {
                error(
                    ErrorCode::InvalidConfiguration,
                    "Push namespace is not a private storage path",
                )
            })?;
        Ok(Self { directory })
    }

    pub fn directory(&self) -> &Path {
        &self.directory
    }

    pub fn quarantine_root(&self) -> PathBuf {
        self.directory.join(QUARANTINE_DIR)
    }

    pub fn state_path(&self) -> PathBuf {
        self.directory.join(STATE_FILE)
    }

    pub fn journal_path(&self) -> PathBuf {
        self.directory.join(JOURNAL_FILE)
    }

    /// Retained quarantine files, oldest names first.
    pub fn quarantine_entries(&self) -> Vec<PathBuf> {
        let Ok(entries) = fs::read_dir(self.quarantine_root()) else {
            return Vec::new();
        };
        let mut paths: Vec<PathBuf> = entries
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| path.is_file())
            .collect();
        paths.sort();
        paths
    }

    /// Load the durable profile, quarantining a corrupt payload.
    pub fn load_state(&self) -> Result<LoadOutcome<PushDurableState>, SystemError> {
        match self.load_envelope::<PushDurableState>(STATE_FILE, MAX_STATE_ENVELOPE_BYTES)? {
            LoadState::Missing => Ok(LoadOutcome {
                value: PushDurableState::default(),
                quarantined: None,
            }),
            LoadState::Loaded(state) => {
                if state.validate().is_ok() {
                    Ok(LoadOutcome {
                        value: state,
                        quarantined: None,
                    })
                } else {
                    Ok(self.recover(STATE_FILE, MAX_STATE_ENVELOPE_BYTES))
                }
            }
            LoadState::Corrupt { .. } => Ok(self.recover(STATE_FILE, MAX_STATE_ENVELOPE_BYTES)),
        }
    }

    /// Commit the durable profile atomically.
    pub fn save_state(&self, state: &PushDurableState) -> Result<(), SystemError> {
        state.validate()?;
        self.write_envelope(STATE_FILE, state, MAX_STATE_ENVELOPE_BYTES)
    }

    /// Load the bounded message-ID journal, quarantining a corrupt payload and
    /// falling back to `fallback`.
    pub fn load_journal(
        &self,
        fallback: DedupJournal,
    ) -> Result<LoadOutcome<DedupJournal>, SystemError> {
        match self.load_envelope::<DedupJournal>(JOURNAL_FILE, MAX_JOURNAL_ENVELOPE_BYTES)? {
            LoadState::Missing => Ok(LoadOutcome {
                value: fallback,
                quarantined: None,
            }),
            LoadState::Loaded(journal) => {
                if journal.validate().is_ok() {
                    Ok(LoadOutcome {
                        value: journal,
                        quarantined: None,
                    })
                } else {
                    Ok(LoadOutcome {
                        value: fallback,
                        quarantined: Some(
                            self.quarantine(JOURNAL_FILE, entry_len(&self.journal_path())),
                        ),
                    })
                }
            }
            LoadState::Corrupt { bytes } => Ok(LoadOutcome {
                value: fallback,
                quarantined: Some(self.quarantine(JOURNAL_FILE, bytes)),
            }),
        }
    }

    /// Commit the bounded message-ID journal atomically.
    pub fn save_journal(&self, journal: &DedupJournal) -> Result<(), SystemError> {
        journal.validate()?;
        self.write_envelope(JOURNAL_FILE, journal, MAX_JOURNAL_ENVELOPE_BYTES)
    }

    /// Remove the durable journal, used when an explicit reset is requested.
    pub fn clear_journal(&self) -> Result<(), SystemError> {
        match fs::remove_file(self.journal_path()) {
            Ok(()) => Ok(()),
            Err(cause) if cause.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(cause) => Err(io_error(cause)),
        }
    }

    fn recover<T: Default>(&self, name: &'static str, max_bytes: u64) -> LoadOutcome<T> {
        let bytes = entry_len(&self.directory.join(name)).min(max_bytes);
        LoadOutcome {
            value: T::default(),
            quarantined: Some(self.quarantine(name, bytes)),
        }
    }

    fn write_envelope<T: Serialize>(
        &self,
        name: &str,
        payload: &T,
        max_bytes: u64,
    ) -> Result<(), SystemError> {
        let payload_bytes = serde_json::to_vec(payload)
            .map_err(|_| error(ErrorCode::IntegrityFailure, "Push record cannot be encoded"))?;
        if payload_bytes.len() as u64 > max_bytes {
            return Err(error(
                ErrorCode::InvalidInput,
                "Push record exceeds its durable size bound",
            ));
        }
        let envelope = Envelope {
            format_version: PUSH_FORMAT_VERSION,
            checksum: envelope_digest(&payload_bytes),
            payload,
        };
        let bytes = serde_json::to_vec(&envelope).map_err(|_| {
            error(
                ErrorCode::IntegrityFailure,
                "Push envelope cannot be encoded",
            )
        })?;
        if bytes.len() as u64 > max_bytes {
            return Err(error(
                ErrorCode::InvalidInput,
                "Push envelope exceeds its durable size bound",
            ));
        }
        fs::create_dir_all(&self.directory).map_err(io_error)?;
        let mut staging = tempfile::Builder::new()
            .prefix(".gpui-push-")
            .tempfile_in(&self.directory)
            .map_err(io_error)?;
        staging.write_all(&bytes).map_err(io_error)?;
        staging.flush().map_err(io_error)?;
        staging.as_file().sync_all().map_err(io_error)?;
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
            Err(cause) if cause.kind() == io::ErrorKind::NotFound => return Ok(LoadState::Missing),
            Err(cause) => return Err(io_error(cause)),
        };
        if !metadata.is_file() || metadata.file_type().is_symlink() || metadata.len() > max_bytes {
            return Ok(LoadState::Corrupt {
                bytes: metadata.len(),
            });
        }
        let bytes = match fs::read(&path) {
            Ok(bytes) => bytes,
            Err(cause) if cause.kind() == io::ErrorKind::NotFound => return Ok(LoadState::Missing),
            Err(cause) => return Err(io_error(cause)),
        };
        if bytes.len() as u64 > max_bytes {
            return Ok(LoadState::Corrupt {
                bytes: bytes.len() as u64,
            });
        }
        let envelope = match serde_json::from_slice::<Envelope<T>>(&bytes) {
            Ok(envelope) if envelope.format_version == PUSH_FORMAT_VERSION => envelope,
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

    fn quarantine(&self, file_name: &'static str, bytes: u64) -> QuarantinedPayload {
        let directory = self.quarantine_root();
        let _ = fs::create_dir_all(&directory);
        let sequence = QUARANTINE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let stored = directory.join(format!("{file_name}.{sequence}.corrupt"));
        let source = self.directory.join(file_name);
        if fs::rename(&source, &stored).is_err() {
            let _ = fs::remove_file(&source);
        }
        prune_quarantine(&directory);
        QuarantinedPayload {
            file: file_name,
            bytes,
            quarantined_files: self.quarantine_entries().len(),
        }
    }
}

fn prune_quarantine(directory: &Path) {
    let Ok(entries) = fs::read_dir(directory) else {
        return;
    };
    let mut files: Vec<(SystemTime, PathBuf)> = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let Ok(metadata) = entry.metadata() else {
            continue;
        };
        let Ok(modified) = metadata.modified() else {
            continue;
        };
        files.push((modified, path));
    }
    if files.len() <= MAX_QUARANTINE_ENTRIES {
        return;
    }
    files.sort();
    let excess = files.len() - MAX_QUARANTINE_ENTRIES;
    for (_, path) in files.into_iter().take(excess) {
        let _ = fs::remove_file(path);
    }
}

fn entry_len(path: &Path) -> u64 {
    fs::metadata(path)
        .map(|metadata| metadata.len())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{PushProviderKind, RegistrationSyncState, TokenFingerprint};
    use crate::{AppDeliveryCondition, PendingActivation, PushAccountId, PushMessageId};
    use gpui_app_data::DirectoryRoots;

    fn store(directory: &Path) -> PushStore {
        let app = AppData::open(DirectoryRoots::under(directory)).expect("app data");
        PushStore::open(&app).expect("push store")
    }

    #[test]
    fn state_round_trips_and_missing_files_load_default() {
        let root = tempfile::tempdir().expect("temp dir");
        let store = store(root.path());
        let loaded = store.load_state().expect("load");
        assert_eq!(loaded.value, PushDurableState::default());
        assert!(loaded.quarantined.is_none());
        let mut state = PushDurableState {
            token_revision: 1,
            fingerprint: Some(TokenFingerprint::of(b"token")),
            kind: Some(PushProviderKind::Fcm),
            sync: RegistrationSyncState::PendingServerSync { revision: 1 },
            ..PushDurableState::default()
        };
        state.registration_epoch = 3;
        store.save_state(&state).expect("save");
        assert_eq!(store.load_state().expect("load").value, state);
    }

    #[test]
    fn tampered_state_is_quarantined_and_recovers_empty() {
        let root = tempfile::tempdir().expect("temp dir");
        let store = store(root.path());
        let state = PushDurableState {
            token_revision: 1,
            fingerprint: Some(TokenFingerprint::of(b"token")),
            kind: Some(PushProviderKind::Apns),
            sync: RegistrationSyncState::SyncedAtRevision { revision: 1 },
            ..PushDurableState::default()
        };
        store.save_state(&state).expect("save");
        let raw = fs::read_to_string(store.state_path()).expect("read");
        let tampered = raw.replace("\"revision\":1", "\"revision\":9");
        fs::write(store.state_path(), tampered).expect("write");
        let loaded = store.load_state().expect("load");
        assert_eq!(loaded.value, PushDurableState::default());
        let quarantined = loaded.quarantined.expect("quarantine diagnostics");
        assert_eq!(quarantined.file, STATE_FILE);
        assert_eq!(store.quarantine_entries().len(), 1);
    }

    #[test]
    fn valid_checksum_but_unsafe_account_identity_is_quarantined() {
        let root = tempfile::tempdir().expect("temp dir");
        let store = store(root.path());
        let state = PushDurableState {
            account: Some(PushAccountId::new("account-first").expect("account")),
            ..PushDurableState::default()
        };
        let mut payload = serde_json::to_value(&state).expect("value");
        // A syntactically valid envelope whose account smuggles the reserved
        // device namespace separator.
        payload["account"] = serde_json::json!("gpui-push/device");
        store
            .write_envelope(STATE_FILE, &payload, MAX_STATE_ENVELOPE_BYTES)
            .expect("write envelope");
        let loaded = store.load_state().expect("load");
        assert_eq!(loaded.value, PushDurableState::default());
        let quarantined = loaded.quarantined.expect("quarantine diagnostics");
        assert_eq!(quarantined.file, STATE_FILE);
        assert_eq!(store.quarantine_entries().len(), 1);
        // Recovery is clean: the next save/load round-trips.
        store
            .save_state(&PushDurableState::default())
            .expect("save after recovery");
        assert_eq!(
            store.load_state().expect("load").value,
            PushDurableState::default()
        );
    }

    #[test]
    fn valid_checksum_but_unsafe_activation_route_is_quarantined() {
        let root = tempfile::tempdir().expect("temp dir");
        let store = store(root.path());
        let state = PushDurableState {
            registration_epoch: 1,
            next_activation_sequence: 2,
            pending_cold_activations: vec![PendingActivation {
                sequence: 1,
                message_id: PushMessageId::new("message-1").expect("message id"),
                route: "https://app.example.test/orders/7".to_owned(),
                account: None,
                registration_epoch: 1,
                condition: AppDeliveryCondition::TerminatedColdLaunch,
                queued_at_ms: 1,
            }],
            ..PushDurableState::default()
        };
        let mut payload = serde_json::to_value(&state).expect("value");
        payload["pending_cold_activations"][0]["route"] = serde_json::json!("javascript:alert(1)");
        store
            .write_envelope(STATE_FILE, &payload, MAX_STATE_ENVELOPE_BYTES)
            .expect("write envelope");
        let loaded = store.load_state().expect("load");
        assert_eq!(loaded.value, PushDurableState::default());
        assert_eq!(
            loaded.quarantined.expect("quarantine diagnostics").file,
            STATE_FILE
        );
        assert_eq!(store.quarantine_entries().len(), 1);
    }

    #[test]
    fn valid_checksum_but_unsafe_journal_identity_is_quarantined() {
        let root = tempfile::tempdir().expect("temp dir");
        let store = store(root.path());
        let mut journal = DedupJournal::new(4, 1_000).expect("journal");
        journal.record(PushMessageId::new("message-1").expect("message id"), 1);
        let mut payload = serde_json::to_value(&journal).expect("value");
        payload["entries"][0]["message_id"] = serde_json::json!("bad message id");
        store
            .write_envelope(JOURNAL_FILE, &payload, MAX_JOURNAL_ENVELOPE_BYTES)
            .expect("write envelope");
        let fallback = DedupJournal::new(4, 1_000).expect("journal");
        let loaded = store.load_journal(fallback.clone()).expect("load");
        assert_eq!(loaded.value, fallback);
        assert_eq!(
            loaded.quarantined.expect("quarantine diagnostics").file,
            JOURNAL_FILE
        );
    }

    #[test]
    fn corrupt_journal_is_quarantined_and_bounded() {
        let root = tempfile::tempdir().expect("temp dir");
        let store = store(root.path());
        let fallback = DedupJournal::new(4, 1_000).expect("journal");
        fs::create_dir_all(store.directory()).expect("namespace directory");
        for _ in 0..(MAX_QUARANTINE_ENTRIES + 3) {
            fs::write(store.journal_path(), b"{ not json").expect("corrupt");
            let loaded = store.load_journal(fallback.clone()).expect("load");
            assert!(loaded.quarantined.is_some());
        }
        assert!(store.quarantine_entries().len() <= MAX_QUARANTINE_ENTRIES);
    }
}
