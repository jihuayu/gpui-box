use crate::{check, completed, error, io_error};
use gpui_system::{
    Availability, Capability, Configuration, ErrorCode, Evidence, OperationContext,
    PermissionState, ServiceResult, Support, SystemError,
};
use std::{
    collections::BTreeMap,
    fs::{self, File, OpenOptions},
    path::{Component, Path, PathBuf},
    sync::{Arc, Mutex, MutexGuard},
    time::{Duration, SystemTime},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum DataLocation {
    Config,
    Data,
    Cache,
    Logs,
    Temporary,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackupPolicy {
    ApplicationManaged,
    Regenerable,
    HostManaged,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirectoryPolicy {
    pub backup: BackupPolicy,
    pub sandboxed: bool,
}
/// Roots must be private to the application, absolute, and mutually non-overlapping.
/// `control` holds the lifetime lock and must never be cleared by application cleanup.
#[derive(Debug, Clone)]
pub struct DirectoryRoots {
    pub control: PathBuf,
    pub config: PathBuf,
    pub data: PathBuf,
    pub cache: PathBuf,
    pub logs: PathBuf,
    pub temporary: PathBuf,
    pub policy: DirectoryPolicy,
}
impl DirectoryRoots {
    /// Explicit roots for tests, portable apps, and host-provided mobile containers.
    pub fn under(root: impl AsRef<Path>) -> Self {
        let root = root.as_ref();
        Self {
            control: root.join("control"),
            config: root.join("config"),
            data: root.join("data"),
            cache: root.join("cache"),
            logs: root.join("logs"),
            temporary: root.join("temporary"),
            policy: DirectoryPolicy {
                backup: BackupPolicy::ApplicationManaged,
                sandboxed: false,
            },
        }
    }
    /// Resolve desktop known folders/XDG roots without creating them.
    pub fn native(app_id: &str) -> Result<Self, SystemError> {
        valid_name(app_id)?;
        #[cfg(any(target_os = "windows", target_os = "macos", target_os = "linux"))]
        {
            let data = dirs::data_local_dir()
                .ok_or_else(|| {
                    error(
                        ErrorCode::Unavailable,
                        "Native data directory is unavailable",
                    )
                })?
                .join(app_id);
            let config = dirs::config_dir()
                .ok_or_else(|| {
                    error(
                        ErrorCode::Unavailable,
                        "Native configuration directory is unavailable",
                    )
                })?
                .join(app_id);
            let cache = dirs::cache_dir()
                .ok_or_else(|| {
                    error(
                        ErrorCode::Unavailable,
                        "Native cache directory is unavailable",
                    )
                })?
                .join(app_id);
            // Suffix each purpose: known folders may coincide on some platforms.
            Ok(Self {
                control: data.join("control"),
                config: config.join("config"),
                data: data.join("data"),
                cache: cache.join("cache"),
                logs: data.join("logs"),
                temporary: std::env::temp_dir().join(app_id).join("temporary"),
                policy: DirectoryPolicy {
                    backup: BackupPolicy::ApplicationManaged,
                    sandboxed: false,
                },
            })
        }
        #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
        Err(error(
            ErrorCode::InvalidConfiguration,
            "The native host must supply private directory roots and policies",
        ))
    }
}
#[derive(Debug, Clone)]
pub struct AppDirectory {
    path: PathBuf,
    pub policy: DirectoryPolicy,
}
impl AppDirectory {
    pub fn path(&self) -> &Path {
        &self.path
    }
    /// Lexical containment only. Private roots must not be writable by untrusted actors.
    /// Rejects existing symlink children; this is not a race-proof sandbox boundary.
    pub fn child(&self, relative: impl AsRef<Path>) -> Result<PathBuf, SystemError> {
        let relative = relative.as_ref();
        if relative.as_os_str().is_empty()
            || relative
                .components()
                .any(|part| !matches!(part, Component::Normal(_)))
        {
            return Err(error(
                ErrorCode::InvalidInput,
                "A normal relative path is required",
            ));
        }
        let mut current = self.path.clone();
        for part in relative.components() {
            current.push(part.as_os_str());
            if fs::symlink_metadata(&current)
                .is_ok_and(|metadata| metadata.file_type().is_symlink())
            {
                return Err(error(
                    ErrorCode::InvalidInput,
                    "Symlink children are not private storage paths",
                ));
            }
        }
        Ok(current)
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DataOperation {
    ResolveDirectories,
    ReadSettings,
    ReplaceSettings,
    MigrateSettings,
    ClearCache,
    TemporaryExport,
}
pub(crate) struct State {
    pub(crate) active_exports: BTreeMap<PathBuf, Arc<()>>,
}
pub(crate) struct Inner {
    directories: BTreeMap<DataLocation, AppDirectory>,
    pub(crate) state: Mutex<State>,
    _locks: Vec<File>,
}
#[derive(Clone)]
pub struct AppData {
    pub(crate) inner: Arc<Inner>,
}
impl AppData {
    /// Acquires an OS process-lifetime lock. Clone this service for multiple windows.
    pub fn open(roots: DirectoryRoots) -> Result<Self, SystemError> {
        let paths = [
            &roots.control,
            &roots.config,
            &roots.data,
            &roots.cache,
            &roots.logs,
            &roots.temporary,
        ];
        let mut canonical = Vec::new();
        for path in paths {
            if !path.is_absolute() {
                return Err(error(
                    ErrorCode::InvalidConfiguration,
                    "Private directory roots must be absolute",
                ));
            }
            fs::create_dir_all(path).map_err(io_error)?;
            canonical.push(fs::canonicalize(path).map_err(io_error)?);
        }
        for (index, path) in canonical.iter().enumerate() {
            if canonical
                .iter()
                .enumerate()
                .any(|(other, candidate)| index != other && path.starts_with(candidate))
            {
                return Err(error(
                    ErrorCode::InvalidConfiguration,
                    "Private directory roots overlap",
                ));
            }
        }
        // Lock every canonical purpose root, so changing the control directory or
        // sharing a subset of roots cannot bypass ownership. Never delete these.
        let mut locks = Vec::new();
        let mut ordered = canonical.clone();
        ordered.sort();
        for path in ordered {
            let lock = OpenOptions::new()
                .read(true)
                .write(true)
                .create(true)
                .truncate(false)
                .open(path.join(".owner.lock"))
                .map_err(io_error)?;
            lock.try_lock().map_err(|_| {
                error(
                    ErrorCode::Busy,
                    "Private storage is owned by another service or process",
                )
            })?;
            locks.push(lock);
        }
        let directories = [
            DataLocation::Config,
            DataLocation::Data,
            DataLocation::Cache,
            DataLocation::Logs,
            DataLocation::Temporary,
        ]
        .into_iter()
        .zip(canonical.into_iter().skip(1))
        .map(|(kind, path)| {
            let mut policy = roots.policy.clone();
            if matches!(kind, DataLocation::Cache | DataLocation::Temporary) {
                policy.backup = BackupPolicy::Regenerable;
            }
            (kind, AppDirectory { path, policy })
        })
        .collect();
        Ok(Self {
            inner: Arc::new(Inner {
                directories,
                state: Mutex::new(State {
                    active_exports: BTreeMap::new(),
                }),
                _locks: locks,
            }),
        })
    }
    pub fn location(&self, kind: DataLocation) -> AppDirectory {
        self.inner.directories[&kind].clone()
    }
    pub fn capabilities(&self) -> Vec<Capability<DataOperation>> {
        [
            DataOperation::ResolveDirectories,
            DataOperation::ReadSettings,
            DataOperation::ReplaceSettings,
            DataOperation::MigrateSettings,
            DataOperation::ClearCache,
            DataOperation::TemporaryExport,
        ]
        .into_iter()
        .map(|operation| Capability {
            operation,
            support: Support::Native,
            availability: Availability::Available,
            configuration: Configuration::Ready,
            permission: PermissionState::NotRequired,
            evidence: Evidence::Compiled,
            constraints: vec![],
            observed_revision: 1,
        })
        .collect()
    }
    pub(crate) fn state(&self) -> Result<MutexGuard<'_, State>, SystemError> {
        self.inner.state.try_lock().map_err(|_| {
            error(
                ErrorCode::Busy,
                "Private storage is busy or requires reopening",
            )
        })
    }
    /// Clears only this service's cache directory. Temporary exports are separate.
    pub fn clear_cache(&self, context: &OperationContext) -> ServiceResult<usize> {
        if let Some(cancelled) = check(context)? {
            return Ok(cancelled);
        }
        let _state = self.state()?;
        let mut count = 0;
        for entry in fs::read_dir(self.location(DataLocation::Cache).path()).map_err(io_error)? {
            if let Some(cancelled) = check(context)? {
                return Ok(cancelled);
            }
            let entry = entry.map_err(io_error)?;
            if entry.file_name() == ".owner.lock" {
                continue;
            }
            remove_entry(&entry.path())?;
            count += 1;
        }
        completed(count)
    }
    /// An export lease pins content regardless of its cleanup age. Consumers must
    /// retain it until native use ends. Drop releases the pin; it does not delete.
    pub fn temporary_export(
        &self,
        bytes: &[u8],
        max_bytes: usize,
        context: &OperationContext,
    ) -> ServiceResult<ExportLease> {
        if let Some(cancelled) = check(context)? {
            return Ok(cancelled);
        }
        if bytes.len() > max_bytes {
            return Err(error(ErrorCode::InvalidInput, "Export size limit exceeded"));
        }
        let mut state = self.state()?;
        let mut file = tempfile::Builder::new()
            .prefix("export-")
            .tempfile_in(self.location(DataLocation::Temporary).path())
            .map_err(io_error)?;
        std::io::Write::write_all(&mut file, bytes).map_err(io_error)?;
        if let Some(cancelled) = check(context)? {
            return Ok(cancelled);
        }
        let (_, path) = file.keep().map_err(|failure| io_error(failure.error))?;
        let pin = Arc::new(());
        state.active_exports.insert(path.clone(), pin.clone());
        completed(ExportLease {
            _app: self.clone(),
            path,
            _pin: pin,
        })
    }
    /// Age applies only to unleased exports, including those from earlier processes.
    /// Unknown files and symlinks are preserved. No arbitrary directories traversed.
    pub fn cleanup_exports(
        &self,
        minimum_age: Duration,
        context: &OperationContext,
    ) -> ServiceResult<usize> {
        if let Some(cancelled) = check(context)? {
            return Ok(cancelled);
        }
        let mut state = self.state()?;
        state
            .active_exports
            .retain(|_, pin| Arc::strong_count(pin) > 1);
        let mut count = 0;
        for entry in
            fs::read_dir(self.location(DataLocation::Temporary).path()).map_err(io_error)?
        {
            if let Some(cancelled) = check(context)? {
                return Ok(cancelled);
            }
            let entry = entry.map_err(io_error)?;
            let path = entry.path();
            if !entry.file_name().to_string_lossy().starts_with("export-")
                || state.active_exports.contains_key(&path)
            {
                continue;
            }
            let metadata = fs::symlink_metadata(&path).map_err(io_error)?;
            if !metadata.is_file() || metadata.file_type().is_symlink() {
                continue;
            }
            if SystemTime::now()
                .duration_since(metadata.modified().map_err(io_error)?)
                .unwrap_or_default()
                >= minimum_age
            {
                fs::remove_file(path).map_err(io_error)?;
                count += 1;
            }
        }
        completed(count)
    }
}
pub struct ExportLease {
    _app: AppData,
    path: PathBuf,
    _pin: Arc<()>,
}
impl ExportLease {
    pub fn path(&self) -> &Path {
        &self.path
    }
}
pub(crate) fn valid_name(value: &str) -> Result<(), SystemError> {
    if value.is_empty()
        || value.len() > 128
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
        || matches!(value, "." | "..")
        || value.ends_with('.')
    {
        return Err(error(
            ErrorCode::InvalidInput,
            "Storage identifiers require bounded ASCII letters, digits, dot, dash or underscore",
        ));
    }
    let stem = value
        .split('.')
        .next()
        .unwrap_or_default()
        .to_ascii_uppercase();
    if matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || (stem.len() == 4
            && (stem.starts_with("COM") || stem.starts_with("LPT"))
            && matches!(stem.as_bytes()[3], b'1'..=b'9'))
    {
        return Err(error(
            ErrorCode::InvalidInput,
            "Reserved storage identifier",
        ));
    }
    Ok(())
}
pub(crate) fn remove_entry(path: &Path) -> Result<(), SystemError> {
    let metadata = fs::symlink_metadata(path).map_err(io_error)?;
    if metadata.is_dir() && !metadata.file_type().is_symlink() {
        fs::remove_dir_all(path).map_err(io_error)
    } else {
        fs::remove_file(path).map_err(io_error)
    }
}
