//! Background update trust verification and generation-correlated orchestration.
//! Native downloads, package inspection, installer UI and installed-version probes
//! belong to platform providers. Contract evidence remains explicit.
mod cache;
mod metadata;
mod state;

pub use cache::*;
pub use metadata::*;
pub use state::*;

use gpui_system::{Completion, ErrorCode, OperationContext, ServiceResult, SystemError};

fn error(code: ErrorCode, message: &'static str) -> SystemError {
    SystemError::new(code, message)
}

fn background<T>(context: &OperationContext) -> Result<Option<Completion<T>>, SystemError> {
    if context.ui.is_some() {
        return Err(error(
            ErrorCode::InvalidInput,
            "Update verification requires a background context",
        ));
    }
    context.check_at(std::time::Instant::now())?;
    Ok(context.cancellation.reason().map(Completion::Cancelled))
}

fn value<T>(result: ServiceResult<T>) -> Result<T, SystemError> {
    match result? {
        Completion::Completed(value) => Ok(value),
        Completion::Cancelled(_) => Err(error(
            ErrorCode::Unavailable,
            "Update trust setup was cancelled",
        )),
    }
}

/// Delivery policy is independent of Cargo's optimization/build profile.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReleaseProfile {
    Development,
    Release,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UpdateChannel {
    Development,
    AndroidPlay,
    AndroidDirectApk,
    WindowsInstaller,
    MacOsSigned,
    LinuxPackageManager,
    IosStore,
    WebDeployment,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpdatePlatform {
    Windows,
    MacOs,
    Linux,
    Android,
    Ios,
    Web,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpdateOperation {
    Check,
    Download,
    VerifyArtifact,
    InstallerHandoff,
    ConfirmInstalled,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdateProviderManifest {
    pub platform: UpdatePlatform,
    pub channel: UpdateChannel,
    pub operation: UpdateOperation,
    pub native_provider_installed: bool,
    pub evidence: gpui_system::Evidence,
}

/// This crate supplies trust/orchestration contracts. Each native adapter must
/// publish its own per-operation manifest after compilation/runtime qualification.
pub fn provider_manifest(
    platform: UpdatePlatform,
    channel: UpdateChannel,
    operation: UpdateOperation,
) -> UpdateProviderManifest {
    UpdateProviderManifest {
        platform,
        channel,
        operation,
        native_provider_installed: false,
        evidence: gpui_system::Evidence::ContractOnly,
    }
}
