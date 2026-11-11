use gpui_system::{
    Capability, Configuration, Constraint, ErrorCode, Evidence, PermissionState, ProviderManifest,
    RecoveryAction, RetryAdvice, Support, SystemError,
};
/// A user-visible permission domain.
///
/// File-picker and share-sheet grants are deliberately not represented here:
/// they are resource-scoped, single-use grants owned by the resource workflow
/// tracked in #23. They do not become durable app-wide permission state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum PermissionKind {
    Notifications,
    Camera,
    Microphone,
    Photos,
    LocationWhenInUse,
    Contacts,
    Biometrics,
    SpecialAccess(&'static str),
}

impl PermissionKind {
    /// The stable, non-special domains used by the built-in status surface.
    pub const ALL: [Self; 7] = [
        Self::Notifications,
        Self::Camera,
        Self::Microphone,
        Self::Photos,
        Self::LocationWhenInUse,
        Self::Contacts,
        Self::Biometrics,
    ];

    /// A public-safe label. Special-access identifiers are supplied by the
    /// platform contract, not by user content.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Notifications => "Notifications",
            Self::Camera => "Camera",
            Self::Microphone => "Microphone",
            Self::Photos => "Photos",
            Self::LocationWhenInUse => "Location",
            Self::Contacts => "Contacts",
            Self::Biometrics => "Biometrics",
            Self::SpecialAccess(name) => name,
        }
    }
}

/// Authoritative, point-in-time permission state.
///
/// `NotRequired` means the operation needs no runtime prompt on this platform;
/// it is not the same as `Granted` and remains valid without any synthetic OS
/// dialog. `Limited` is usable but constrained (for example, selected photos).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PermissionStatus {
    NotRequired,
    Granted,
    Denied { can_request_again: bool },
    Restricted,
    Limited,
    NotDetermined,
    Unavailable,
}

impl PermissionStatus {
    /// True when the caller may proceed without changing authorization.
    pub const fn allows_use(self) -> bool {
        matches!(self, Self::NotRequired | Self::Granted | Self::Limited)
    }

    /// True when an interactive prompt may change the current state.
    pub const fn requires_prompt(self) -> bool {
        matches!(
            self,
            Self::NotDetermined
                | Self::Denied {
                    can_request_again: true,
                }
        )
    }

    pub const fn is_permanently_denied(self) -> bool {
        matches!(
            self,
            Self::Denied {
                can_request_again: false,
            }
        )
    }

    /// Map to the dependency-light `gpui-system` capability vocabulary.
    pub const fn permission_state(self) -> PermissionState {
        match self {
            Self::NotRequired => PermissionState::NotRequired,
            Self::Granted | Self::Limited => PermissionState::Granted,
            Self::Denied { .. } => PermissionState::Denied,
            Self::Restricted => PermissionState::Restricted,
            Self::NotDetermined => PermissionState::NotDetermined,
            Self::Unavailable => PermissionState::Unknown,
        }
    }

    /// The recovery suitable for this state. A missing native configuration
    /// uses `RepairConfiguration`; this method never claims that the user has
    /// already changed an OS setting.
    pub const fn recovery(self) -> Option<RecoveryAction> {
        match self {
            Self::Denied { .. } | Self::Restricted => Some(RecoveryAction::OpenSettings),
            Self::Unavailable => Some(RecoveryAction::ProbeAgain),
            _ => None,
        }
    }

    /// Convert a blocking state to a typed error. `Granted`, `Limited` and
    /// `NotRequired` return `None`; permission denial is distinct from a missing
    /// manifest/usage-description configuration.
    pub fn error(self) -> Option<SystemError> {
        let (code, message, retry) = match self {
            Self::Denied { can_request_again } => (
                ErrorCode::PermissionDenied,
                "Permission was denied",
                if can_request_again {
                    RetryAdvice::Retry
                } else {
                    RetryAdvice::AfterConditionsChange
                },
            ),
            Self::Restricted => (
                ErrorCode::PermissionDenied,
                "Permission is restricted by the platform",
                RetryAdvice::Never,
            ),
            Self::NotDetermined => (
                ErrorCode::PermissionDenied,
                "Permission has not been requested",
                RetryAdvice::Retry,
            ),
            Self::Unavailable => (
                ErrorCode::Unavailable,
                "Permission status is unavailable",
                RetryAdvice::AfterConditionsChange,
            ),
            Self::NotRequired | Self::Granted | Self::Limited => return None,
        };
        let mut error = SystemError::new(code, message);
        error.recovery = self.recovery();
        error.retry = retry;
        Some(error)
    }
}

/// A fresh provider observation. The revision belongs to the provider and is
/// not a durable authorization grant.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PermissionSnapshot {
    pub kind: PermissionKind,
    pub status: PermissionStatus,
    pub revision: u64,
}

impl PermissionSnapshot {
    pub const fn new(kind: PermissionKind, status: PermissionStatus, revision: u64) -> Self {
        Self {
            kind,
            status,
            revision,
        }
    }

    pub const fn allows_use(&self) -> bool {
        self.status.allows_use()
    }

    /// Fail with the status's typed, actionable error when use is not allowed.
    pub fn require_authorized(&self) -> Result<(), SystemError> {
        if self.status.allows_use() {
            return Ok(());
        }
        Err(self.status.error().unwrap_or_else(|| {
            SystemError::new(ErrorCode::PermissionDenied, "Permission is not authorized")
        }))
    }
}

/// An OS settings destination. Handoff never claims that a setting changed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum SettingsTarget {
    AppDetails,
    NotificationSettings,
    SpecialAccess(&'static str),
}

impl SettingsTarget {
    pub const fn label(self) -> &'static str {
        match self {
            Self::AppDetails => "Application settings",
            Self::NotificationSettings => "Notification settings",
            Self::SpecialAccess(name) => name,
        }
    }
}

/// Operations exposed by a permission provider. Capabilities are snapshots;
/// callers still have to perform the operation to obtain current status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PermissionOperation {
    Query(PermissionKind),
    Request(PermissionKind),
    OpenSettings(SettingsTarget),
}

impl PermissionOperation {
    pub const fn kind(self) -> Option<PermissionKind> {
        match self {
            Self::Query(kind) | Self::Request(kind) => Some(kind),
            Self::OpenSettings(_) => None,
        }
    }

    pub const fn settings_target(self) -> Option<SettingsTarget> {
        match self {
            Self::OpenSettings(target) => Some(target),
            Self::Query(_) | Self::Request(_) => None,
        }
    }

    pub const fn is_interactive(self) -> bool {
        matches!(self, Self::Request(_))
    }
}

/// Contextual request input. The service re-probes provider state at execution
/// time rather than trusting a previous capability snapshot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PermissionRequest {
    pub kind: PermissionKind,
}

impl PermissionRequest {
    pub const fn new(kind: PermissionKind) -> Self {
        Self { kind }
    }

    pub const fn kind(self) -> PermissionKind {
        self.kind
    }
}

impl From<PermissionKind> for PermissionRequest {
    fn from(kind: PermissionKind) -> Self {
        Self::new(kind)
    }
}

/// Result of one request attempt. `prompted` is false when the platform
/// reported `NotRequired`, `Granted` or a permanently denied state and no
/// native dialog was synthesized.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PermissionOutcome {
    pub kind: PermissionKind,
    pub previous: PermissionStatus,
    pub status: PermissionStatus,
    pub prompted: bool,
    pub changed: bool,
    pub revision: u64,
}

impl PermissionOutcome {
    /// Recovery for the resulting status, if the caller must act.
    pub fn recovery(&self) -> Option<RecoveryAction> {
        self.status.recovery()
    }

    /// True only for states that normally require an OS settings trip.
    pub const fn requires_settings(&self) -> bool {
        matches!(
            self.status,
            PermissionStatus::Denied {
                can_request_again: false,
            } | PermissionStatus::Restricted
        )
    }

    pub fn error(&self) -> Option<SystemError> {
        self.status.error()
    }

    /// Convert a blocked outcome to the typed error used by callers that treat
    /// denial as failure. Granted, limited and no-prompt states remain `Ok`.
    pub fn into_result(self) -> Result<Self, SystemError> {
        match self.error() {
            Some(error) => Err(error),
            None => Ok(self),
        }
    }

    pub fn require_authorized(&self) -> Result<(), SystemError> {
        if self.status.allows_use() {
            return Ok(());
        }
        Err(self.status.error().unwrap_or_else(|| {
            SystemError::new(ErrorCode::PermissionDenied, "Permission is not authorized")
        }))
    }
}

/// Receipt for an OS settings handoff. It says only that the platform accepted
/// an attempt to present settings, never that the user toggled anything.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsHandoff {
    Opened,
    Unavailable,
}

impl SettingsHandoff {
    pub const fn is_opened(self) -> bool {
        matches!(self, Self::Opened)
    }
}

/// A state delta produced by request completion or resume reconciliation.
/// The revision is the service observation sequence used by the scoped stream.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PermissionChange {
    pub kind: PermissionKind,
    pub previous: PermissionStatus,
    pub current: PermissionStatus,
    pub revision: u64,
}

impl PermissionChange {
    pub const fn authorization_gained(&self) -> bool {
        !self.previous.allows_use() && self.current.allows_use()
    }

    pub const fn authorization_lost(&self) -> bool {
        self.previous.allows_use() && !self.current.allows_use()
    }
}

/// Result of re-querying all observed permissions after OS settings or resume.
/// `revision` is the event-stream revision that a snapshot acknowledgment must
/// use for this reconciliation.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PermissionReconciliation {
    pub changes: Vec<PermissionChange>,
    pub revision: u64,
}

impl PermissionReconciliation {
    pub fn is_empty(&self) -> bool {
        self.changes.is_empty()
    }
}

/// Build the typed error for missing platform declarations (manifest entry,
/// usage description, entitlement or equivalent).
pub fn invalid_configuration(message: &'static str) -> SystemError {
    let mut error = SystemError::new(ErrorCode::InvalidConfiguration, message);
    error.recovery = Some(RecoveryAction::RepairConfiguration);
    error.retry = RetryAdvice::AfterConditionsChange;
    error
}

/// Build the typed error for a user denial. The caller must offer settings
/// recovery rather than looping on a prompt the OS will not show.
pub fn permission_denied(can_request_again: bool) -> SystemError {
    let status = PermissionStatus::Denied { can_request_again };
    status
        .error()
        .unwrap_or_else(|| SystemError::new(ErrorCode::PermissionDenied, "Permission was denied"))
}

/// Conservative five-platform provider evidence. Native providers are separate
/// issues and must publish their own runtime evidence.
pub const PROVIDER_MANIFEST: [ProviderManifest; 5] = gpui_system::PROVIDER_MANIFEST;

/// Capability-shaped operations for a provider that does not implement a
/// custom snapshot surface.
pub fn unsupported_capability(operation: PermissionOperation) -> Capability<PermissionOperation> {
    Capability {
        operation,
        support: Support::Unsupported,
        availability: gpui_system::Availability::Unknown,
        configuration: Configuration::Unknown,
        permission: PermissionState::Unknown,
        evidence: Evidence::ContractOnly,
        constraints: Vec::<Constraint>::new(),
        observed_revision: 0,
    }
}
