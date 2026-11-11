//! Blocking account-scoped credentials. Execute on a background worker.
//! No provider falls back to plaintext settings or files.
mod model;
mod service;
#[cfg(target_os = "windows")]
#[allow(unsafe_code)]
mod windows;
pub use model::*;
pub use service::*;
#[cfg(target_os = "windows")]
pub use windows::WindowsCredentials;
