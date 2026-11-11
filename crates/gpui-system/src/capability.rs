use crate::{ErrorCode, RecoveryAction, SystemError};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Support {
    Native,
    Conditional,
    FallbackAvailable,
    Unsupported,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Availability {
    Available,
    Unavailable,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Configuration {
    Ready,
    Required,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PermissionState {
    NotRequired,
    Granted,
    Denied,
    Restricted,
    NotDetermined,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Evidence {
    ContractOnly,
    Compiled,
    RuntimeVerified,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Constraint {
    MinimumOsVersion(&'static str),
    NativeIdentity,
    ForegroundOwner,
    Hardware,
    Provider(&'static str),
}

/// A point-in-time observation, never a durable authorization grant.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Capability<O> {
    pub operation: O,
    pub support: Support,
    pub availability: Availability,
    pub configuration: Configuration,
    pub permission: PermissionState,
    pub evidence: Evidence,
    pub constraints: Vec<Constraint>,
    pub observed_revision: u64,
}

impl<O> Capability<O> {
    /// Useful to providers after freshly probing conditions; callers cannot use
    /// an old snapshot to bypass the provider's execution-time validation.
    pub fn check(&self) -> Result<(), SystemError> {
        if matches!(
            self.support,
            Support::Unsupported | Support::Unknown | Support::FallbackAvailable
        ) {
            return Err(SystemError::new(
                ErrorCode::Unsupported,
                "Operation is not supported",
            ));
        }
        if self.configuration != Configuration::Ready {
            let mut error = SystemError::new(
                ErrorCode::InvalidConfiguration,
                "Provider configuration is required",
            );
            error.recovery = Some(RecoveryAction::RepairConfiguration);
            return Err(error);
        }
        if self.availability != Availability::Available {
            return Err(SystemError::new(
                ErrorCode::Unavailable,
                "Provider is unavailable",
            ));
        }
        if !matches!(
            self.permission,
            PermissionState::Granted | PermissionState::NotRequired
        ) {
            let mut error =
                SystemError::new(ErrorCode::PermissionDenied, "Operation is not authorized");
            error.recovery = Some(RecoveryAction::OpenSettings);
            return Err(error);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Platform {
    Windows,
    MacOs,
    Linux,
    Android,
    Ios,
}

/// Native providers are separate work. This manifest describes only this crate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProviderManifest {
    pub platform: Platform,
    pub native_provider_installed: bool,
    pub evidence: Evidence,
}

pub const PROVIDER_MANIFEST: [ProviderManifest; 5] = [
    ProviderManifest {
        platform: Platform::Windows,
        native_provider_installed: false,
        evidence: Evidence::ContractOnly,
    },
    ProviderManifest {
        platform: Platform::MacOs,
        native_provider_installed: false,
        evidence: Evidence::ContractOnly,
    },
    ProviderManifest {
        platform: Platform::Linux,
        native_provider_installed: false,
        evidence: Evidence::ContractOnly,
    },
    ProviderManifest {
        platform: Platform::Android,
        native_provider_installed: false,
        evidence: Evidence::ContractOnly,
    },
    ProviderManifest {
        platform: Platform::Ios,
        native_provider_installed: false,
        evidence: Evidence::ContractOnly,
    },
];
