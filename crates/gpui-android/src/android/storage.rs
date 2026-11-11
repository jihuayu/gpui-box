//! Android-owned private roots. Discover/open storage on a background worker.
use gpui_app_data::{BackupPolicy, DirectoryPolicy, DirectoryRoots};
use gpui_system::{ErrorCode, SystemError};
use std::path::PathBuf;

/// Resolve application-context roots without opening AppData's lifetime locks.
/// Call on a worker after the native host has installed its application context.
/// Durable roots use Android's no-backup directory. Cache is separately evictable.
/// Retain a single opened AppData handle for the application and clone it as needed.
pub fn app_data_roots() -> Result<DirectoryRoots, SystemError> {
    let text = super::bridge::storage_directories().map_err(|_| {
        SystemError::new(
            ErrorCode::Unavailable,
            "Android private roots require an installed host and a background worker",
        )
    })?;
    let value: serde_json::Value = serde_json::from_str(&text).map_err(|_| {
        SystemError::new(
            ErrorCode::InvalidConfiguration,
            "Android private directory response is invalid",
        )
    })?;
    let path = |key: &str| -> Result<PathBuf, SystemError> {
        value
            .get(key)
            .and_then(serde_json::Value::as_str)
            .map(PathBuf::from)
            .filter(|path| path.is_absolute())
            .ok_or_else(|| {
                SystemError::new(
                    ErrorCode::InvalidConfiguration,
                    "Android private directory must be absolute",
                )
            })
    };
    Ok(DirectoryRoots {
        control: path("control")?,
        config: path("config")?,
        data: path("data")?,
        logs: path("logs")?,
        cache: path("cache")?,
        temporary: path("temporary")?,
        policy: DirectoryPolicy {
            backup: BackupPolicy::HostManaged,
            sandboxed: true,
        },
    })
}
