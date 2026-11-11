use crate::{
    AppData, DataLocation, check, completed,
    directories::{remove_entry, valid_name},
    error, io_error,
};
use gpui_system::{ErrorCode, OperationContext, ServiceResult, SystemError};
use serde::{Serialize, de::DeserializeOwned};
use serde_json::Value;
use std::{
    fs::{self, File},
    io::{Read, Write},
    path::PathBuf,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DataScope {
    Application,
    Account(String),
}
impl DataScope {
    fn directory(&self) -> Result<PathBuf, SystemError> {
        Ok(match self {
            Self::Application => PathBuf::from("application"),
            Self::Account(account) => {
                if account.is_empty() || account.len() > 96 {
                    return Err(error(
                        ErrorCode::InvalidInput,
                        "Account namespace is empty or too large",
                    ));
                }
                let encoded: String = account
                    .as_bytes()
                    .iter()
                    .map(|byte| format!("{byte:02x}"))
                    .collect();
                PathBuf::from("accounts").join(encoded)
            }
        })
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Durability {
    FileSynced,
    FileAndDirectorySynced,
    CommittedDirectorySyncUncertain,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WriteReceipt {
    pub schema: u32,
    pub durability: Durability,
}
#[derive(Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Envelope {
    format: u32,
    schema: u32,
    payload: Value,
}
/// Codec-independent persistence adapter for settings, saved UI state, and repositories.
/// Clones serialize access through their AppData owner. Not suitable for credentials.
#[derive(Clone)]
pub struct SettingsStore {
    app: AppData,
    path: PathBuf,
    schema: u32,
    max_bytes: usize,
}
impl AppData {
    pub fn settings(
        &self,
        scope: DataScope,
        name: &str,
        schema: u32,
        max_bytes: usize,
    ) -> Result<SettingsStore, SystemError> {
        valid_name(name)?;
        if schema == 0 || max_bytes == 0 || max_bytes > 16 * 1024 * 1024 {
            return Err(error(
                ErrorCode::InvalidInput,
                "Settings require a positive schema and size bound up to 16 MiB",
            ));
        }
        let path = self
            .location(DataLocation::Config)
            .child(scope.directory()?.join(format!("settings-{name}.json")))?;
        Ok(SettingsStore {
            app: self.clone(),
            path,
            schema,
            max_bytes,
        })
    }
    /// Logout removes only the selected account's private settings and data.
    /// Existing handles to this scope must be discarded by the application first.
    pub fn remove_account(&self, account: &str, context: &OperationContext) -> ServiceResult<()> {
        if let Some(cancelled) = check(context)? {
            return Ok(cancelled);
        }
        let _state = self.state()?;
        let relative = DataScope::Account(account.to_owned()).directory()?;
        for kind in [DataLocation::Config, DataLocation::Data] {
            let path = self.location(kind).child(&relative)?;
            if path.exists() {
                remove_entry(&path)?;
            }
        }
        completed(())
    }
}
impl SettingsStore {
    fn read_envelope(&self) -> Result<Option<Envelope>, SystemError> {
        let file = match File::open(&self.path) {
            Ok(file) => file,
            Err(cause) if cause.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(cause) => return Err(io_error(cause)),
        };
        let mut bytes = Vec::new();
        file.take(self.max_bytes as u64 + 1)
            .read_to_end(&mut bytes)
            .map_err(io_error)?;
        if bytes.len() > self.max_bytes {
            return Err(error(
                ErrorCode::IntegrityFailure,
                "Settings size bound exceeded; original preserved",
            ));
        }
        let envelope: Envelope = serde_json::from_slice(&bytes).map_err(|_| {
            error(
                ErrorCode::IntegrityFailure,
                "Settings are corrupt; original preserved",
            )
        })?;
        if envelope.format != 1 || envelope.schema == 0 {
            return Err(error(
                ErrorCode::IntegrityFailure,
                "Unsupported settings envelope; original preserved",
            ));
        }
        Ok(Some(envelope))
    }
    pub fn load<T: DeserializeOwned>(
        &self,
        context: &OperationContext,
    ) -> ServiceResult<Option<T>> {
        if let Some(cancelled) = check(context)? {
            return Ok(cancelled);
        }
        let _state = self.app.state()?;
        let Some(envelope) = self.read_envelope()? else {
            return completed(None);
        };
        if envelope.schema != self.schema {
            return Err(error(
                ErrorCode::InvalidConfiguration,
                "Settings schema requires an explicit migration",
            ));
        }
        let value = serde_json::from_value(envelope.payload).map_err(|_| {
            error(
                ErrorCode::IntegrityFailure,
                "Settings payload does not match the requested codec",
            )
        })?;
        completed(Some(value))
    }
    /// Replaces only missing or valid current-schema data. Corrupt or older files
    /// must go through migration or explicit quarantine, preventing silent loss.
    pub fn save<T: Serialize>(
        &self,
        value: &T,
        context: &OperationContext,
    ) -> ServiceResult<WriteReceipt> {
        if let Some(cancelled) = check(context)? {
            return Ok(cancelled);
        }
        let _state = self.app.state()?;
        if self
            .read_envelope()?
            .is_some_and(|envelope| envelope.schema != self.schema)
        {
            return Err(error(
                ErrorCode::InvalidConfiguration,
                "Existing settings require migration before replacement",
            ));
        }
        let payload = serde_json::to_value(value)
            .map_err(|_| error(ErrorCode::InvalidInput, "Settings cannot be encoded"))?;
        self.replace(payload, context, |_| Ok(()))
    }
    /// Each migration step transforms vN to vN+1. All steps run on a detached
    /// value before one atomic replacement; failures preserve the original file.
    pub fn migrate(
        &self,
        context: &OperationContext,
        mut step: impl FnMut(u32, Value) -> Result<Value, SystemError>,
    ) -> ServiceResult<WriteReceipt> {
        if let Some(cancelled) = check(context)? {
            return Ok(cancelled);
        }
        let _state = self.app.state()?;
        let Some(mut envelope) = self.read_envelope()? else {
            return Err(error(
                ErrorCode::InvalidInput,
                "No settings exist to migrate",
            ));
        };
        if envelope.schema > self.schema {
            return Err(error(
                ErrorCode::InvalidConfiguration,
                "Settings downgrade is unsupported",
            ));
        }
        while envelope.schema < self.schema {
            if let Some(cancelled) = check(context)? {
                return Ok(cancelled);
            }
            envelope.payload = step(envelope.schema, envelope.payload)?;
            envelope.schema += 1;
        }
        self.replace(envelope.payload, context, |_| Ok(()))
    }
    /// Explicit recovery: moves the original to a unique quarantine file, then
    /// permits a later save. No automatic reset or deletion of corrupt bytes.
    pub fn quarantine(&self, context: &OperationContext) -> ServiceResult<()> {
        if let Some(cancelled) = check(context)? {
            return Ok(cancelled);
        }
        let _state = self.app.state()?;
        if !self.path.exists() {
            return completed(());
        }
        let parent = self.path.parent().ok_or_else(|| {
            error(
                ErrorCode::InvalidConfiguration,
                "Settings directory is missing",
            )
        })?;
        let reservation = tempfile::Builder::new()
            .prefix("quarantine-")
            .tempfile_in(parent)
            .map_err(io_error)?;
        let (_, destination) = reservation
            .keep()
            .map_err(|failure| io_error(failure.error))?;
        if let Err(cause) = fs::rename(&self.path, &destination) {
            let _ = fs::remove_file(destination);
            return Err(io_error(cause));
        }
        completed(())
    }
    fn replace(
        &self,
        payload: Value,
        context: &OperationContext,
        before_commit: impl FnOnce(&mut File) -> std::io::Result<()>,
    ) -> ServiceResult<WriteReceipt> {
        let bytes = serde_json::to_vec(&Envelope {
            format: 1,
            schema: self.schema,
            payload,
        })
        .map_err(|_| error(ErrorCode::InvalidInput, "Settings cannot be encoded"))?;
        if bytes.len() > self.max_bytes {
            return Err(error(
                ErrorCode::InvalidInput,
                "Encoded settings exceed size bound",
            ));
        }
        let parent = self.path.parent().ok_or_else(|| {
            error(
                ErrorCode::InvalidConfiguration,
                "Settings directory is missing",
            )
        })?;
        fs::create_dir_all(parent).map_err(io_error)?;
        let mut staging = tempfile::Builder::new()
            .prefix("settings-")
            .tempfile_in(parent)
            .map_err(io_error)?;
        staging.write_all(&bytes).map_err(io_error)?;
        before_commit(staging.as_file_mut()).map_err(io_error)?;
        staging.as_file().sync_all().map_err(io_error)?;
        if let Some(cancelled) = check(context)? {
            return Ok(cancelled);
        }
        staging
            .persist(&self.path)
            .map_err(|failure| io_error(failure.error))?;
        // The rename has committed: never return a misleading precommit error.
        #[cfg(unix)]
        let durability = match File::open(parent).and_then(|directory| directory.sync_all()) {
            Ok(()) => Durability::FileAndDirectorySynced,
            Err(_) => Durability::CommittedDirectorySyncUncertain,
        };
        #[cfg(not(unix))]
        let durability = Durability::FileSynced;
        completed(WriteReceipt {
            schema: self.schema,
            durability,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn value<T>(result: ServiceResult<T>) -> T {
        match result.unwrap() {
            gpui_system::Completion::Completed(value) => value,
            _ => panic!("unexpected cancellation"),
        }
    }
    #[test]
    fn failed_staging_write_and_interrupted_commit_preserve_original() {
        let root = tempfile::tempdir().unwrap();
        let app = AppData::open(crate::DirectoryRoots::under(root.path())).unwrap();
        let store = app
            .settings(DataScope::Application, "settings", 1, 4096)
            .unwrap();
        let context = OperationContext::default();
        value(store.save(&serde_json::json!({"value":1}), &context));
        let original = fs::read(&store.path).unwrap();
        let failure = store.replace(serde_json::json!({"value":2}), &context, |_| {
            Err(std::io::Error::from(std::io::ErrorKind::StorageFull))
        });
        assert_eq!(failure.unwrap_err().code, ErrorCode::PlatformFailure);
        assert_eq!(fs::read(&store.path).unwrap(), original);
        let failure = store.replace(serde_json::json!({"value":3}), &context, |_| {
            Err(std::io::Error::new(
                std::io::ErrorKind::Interrupted,
                "simulated interruption",
            ))
        });
        assert!(failure.is_err());
        assert_eq!(fs::read(&store.path).unwrap(), original);
    }
}
