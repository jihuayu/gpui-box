//! Scoped wake-prevention requests, validated reasons and release policy.

use gpui_system::{ErrorCode, RecoveryAction, SurfaceId, SystemError};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt;

/// Maximum public-safe wake reason length in bytes.
pub const MAX_WAKE_REASON_BYTES: usize = 128;

/// The two kinds of display/system inhibition a host can request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WakeInhibitionKind {
    /// Prevent idle system sleep while allowing the display to dim or turn off.
    SystemIdleSleep,
    /// Prevent both display sleep/dimming and idle system sleep.
    DisplayAndSystem,
}

impl WakeInhibitionKind {
    pub const ALL: [Self; 2] = [Self::SystemIdleSleep, Self::DisplayAndSystem];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::SystemIdleSleep => "system_idle_sleep",
            Self::DisplayAndSystem => "display_and_system",
        }
    }

    /// True when this kind also keeps the display awake.
    pub const fn requires_display(self) -> bool {
        matches!(self, Self::DisplayAndSystem)
    }
}

impl fmt::Display for WakeInhibitionKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// A bounded, public-safe, control-character-free reason for holding a lease.
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct WakeReason(String);

impl WakeReason {
    /// Validates a caller-supplied reason.
    ///
    /// Empty reasons, whitespace-only reasons, reasons longer than
    /// [`MAX_WAKE_REASON_BYTES`] and reasons containing control characters are
    /// rejected with [`ErrorCode::InvalidInput`].
    pub fn new(reason: impl Into<String>) -> Result<Self, SystemError> {
        let reason = reason.into();
        if reason.trim().is_empty() {
            return Err(SystemError::new(
                ErrorCode::InvalidInput,
                "Wake reason must not be empty",
            ));
        }
        if reason.len() > MAX_WAKE_REASON_BYTES {
            return Err(SystemError::new(
                ErrorCode::InvalidInput,
                "Wake reason exceeds the byte bound",
            ));
        }
        if reason.chars().any(char::is_control) {
            return Err(SystemError::new(
                ErrorCode::InvalidInput,
                "Wake reason contains a control character",
            ));
        }
        Ok(Self(reason))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl fmt::Display for WakeReason {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl TryFrom<String> for WakeReason {
    type Error = SystemError;

    fn try_from(reason: String) -> Result<Self, Self::Error> {
        Self::new(reason)
    }
}

impl TryFrom<&str> for WakeReason {
    type Error = SystemError;

    fn try_from(reason: &str) -> Result<Self, Self::Error> {
        Self::new(reason)
    }
}

impl std::str::FromStr for WakeReason {
    type Err = SystemError;

    fn from_str(reason: &str) -> Result<Self, Self::Err> {
        Self::new(reason)
    }
}

impl fmt::Debug for WakeReason {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.debug_tuple("WakeReason").field(&self.0).finish()
    }
}

impl AsRef<str> for WakeReason {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl Serialize for WakeReason {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for WakeReason {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let reason = String::deserialize(deserializer)?;
        Self::new(reason).map_err(serde::de::Error::custom)
    }
}

/// Which lifecycle transitions automatically revoke a lease.
///
/// These flags are caller policy, not a bypass: a `DisplayAndSystem` lease is
/// always released when the session locks or the system sleeps, and no lease can
/// stay effective once the OS is preparing for sleep.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ReleasePolicy {
    pub revoke_on_background: bool,
    pub revoke_on_session_lock: bool,
    pub revoke_on_system_sleep: bool,
    pub revoke_on_low_power_mode: bool,
}

impl ReleasePolicy {
    /// Explicit defaults for one lease kind.
    pub const fn for_kind(kind: WakeInhibitionKind) -> Self {
        match kind {
            WakeInhibitionKind::DisplayAndSystem => Self {
                revoke_on_background: true,
                revoke_on_session_lock: true,
                revoke_on_system_sleep: true,
                revoke_on_low_power_mode: false,
            },
            WakeInhibitionKind::SystemIdleSleep => Self {
                revoke_on_background: false,
                revoke_on_session_lock: false,
                revoke_on_system_sleep: true,
                revoke_on_low_power_mode: false,
            },
        }
    }

    pub const fn with_revoke_on_background(mut self, revoke: bool) -> Self {
        self.revoke_on_background = revoke;
        self
    }

    pub const fn with_revoke_on_session_lock(mut self, revoke: bool) -> Self {
        self.revoke_on_session_lock = revoke;
        self
    }

    pub const fn with_revoke_on_system_sleep(mut self, revoke: bool) -> Self {
        self.revoke_on_system_sleep = revoke;
        self
    }

    pub const fn with_revoke_on_low_power_mode(mut self, revoke: bool) -> Self {
        self.revoke_on_low_power_mode = revoke;
        self
    }

    /// Whether a lease of `kind` must be revoked when the app backgrounds or a
    /// surface becomes hidden.
    pub const fn revokes_on_background_for(self, kind: WakeInhibitionKind) -> bool {
        matches!(kind, WakeInhibitionKind::DisplayAndSystem) || self.revoke_on_background
    }

    /// Whether a lease of `kind` must be revoked when the session locks.
    pub const fn revokes_on_session_lock_for(self, kind: WakeInhibitionKind) -> bool {
        matches!(kind, WakeInhibitionKind::DisplayAndSystem) || self.revoke_on_session_lock
    }

    pub const fn revokes_on_system_sleep_for(self, _kind: WakeInhibitionKind) -> bool {
        true
    }
}

impl Default for ReleasePolicy {
    fn default() -> Self {
        Self::for_kind(WakeInhibitionKind::SystemIdleSleep)
    }
}

/// Why a provider could not make an inhibition effective.
///
/// An ineffective grant is never converted into a successful lease.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IneffectiveInhibitionReason {
    /// The platform has no API for this inhibition kind.
    UnsupportedKind,
    /// The host or user denied the required permission.
    PermissionDenied,
    /// The document or surface is hidden or unfocused.
    HiddenSurfaceOrDocument,
    /// OS battery saver / low-power policy blocks the inhibition.
    BatterySaverOverride,
    /// The session is locking or locked.
    SessionLocked,
    /// The OS is already preparing for or in system sleep.
    SystemSleeping,
    /// The application is backgrounded or suspended.
    AppBackgrounded,
    /// The provider exists but cannot be used right now.
    ProviderUnavailable,
    /// Required host configuration (manifest entry, entitlement, policy) is
    /// missing.
    ConfigurationRequired,
}

impl IneffectiveInhibitionReason {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::UnsupportedKind => "unsupported_kind",
            Self::PermissionDenied => "permission_denied",
            Self::HiddenSurfaceOrDocument => "hidden_surface_or_document",
            Self::BatterySaverOverride => "battery_saver_override",
            Self::SessionLocked => "session_locked",
            Self::SystemSleeping => "system_sleeping",
            Self::AppBackgrounded => "app_backgrounded",
            Self::ProviderUnavailable => "provider_unavailable",
            Self::ConfigurationRequired => "configuration_required",
        }
    }

    /// Deterministic typed error for a refused grant.
    pub fn error(self) -> SystemError {
        match self {
            Self::UnsupportedKind => SystemError::new(
                ErrorCode::Unsupported,
                "Wake inhibition kind is unsupported",
            ),
            Self::PermissionDenied => {
                let mut error = SystemError::new(
                    ErrorCode::PermissionDenied,
                    "Wake lock permission is not authorized",
                );
                error.recovery = Some(RecoveryAction::OpenSettings);
                error
            }
            Self::ConfigurationRequired => {
                let mut error = SystemError::new(
                    ErrorCode::InvalidConfiguration,
                    "Wake lock host configuration is required",
                );
                error.recovery = Some(RecoveryAction::RepairConfiguration);
                error
            }
            Self::HiddenSurfaceOrDocument
            | Self::BatterySaverOverride
            | Self::SessionLocked
            | Self::SystemSleeping
            | Self::AppBackgrounded
            | Self::ProviderUnavailable => SystemError::new(
                ErrorCode::Unavailable,
                "Wake inhibition is not effective under current conditions",
            ),
        }
    }
}

impl fmt::Display for IneffectiveInhibitionReason {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// The provider's authoritative answer to an acquisition attempt.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderInhibitionGrant {
    /// The native token is held and the inhibition is effective.
    Effective { native_token: u64 },
    /// The provider refused; the caller must not register a lease.
    Ineffective { reason: IneffectiveInhibitionReason },
}

/// The scope a lease belongs to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WakeLeaseScope {
    /// Process/application-scoped background work.
    Application { policy: ReleasePolicy },
    /// One UI surface attachment `(SurfaceId, generation)`.
    Surface {
        surface: SurfaceId,
        generation: u64,
        policy: ReleasePolicy,
    },
}

impl WakeLeaseScope {
    pub const fn application(policy: ReleasePolicy) -> Self {
        Self::Application { policy }
    }

    pub const fn surface(surface: SurfaceId, generation: u64, policy: ReleasePolicy) -> Self {
        Self::Surface {
            surface,
            generation,
            policy,
        }
    }

    pub const fn policy(&self) -> ReleasePolicy {
        match self {
            Self::Application { policy } | Self::Surface { policy, .. } => *policy,
        }
    }

    pub const fn surface_id(&self) -> Option<SurfaceId> {
        match self {
            Self::Application { .. } => None,
            Self::Surface { surface, .. } => Some(*surface),
        }
    }

    pub const fn generation(&self) -> Option<u64> {
        match self {
            Self::Application { .. } => None,
            Self::Surface { generation, .. } => Some(*generation),
        }
    }

    pub const fn is_surface_scoped(&self) -> bool {
        matches!(self, Self::Surface { .. })
    }

    pub const fn with_policy(mut self, policy: ReleasePolicy) -> Self {
        match &mut self {
            Self::Application { policy: current }
            | Self::Surface {
                policy: current, ..
            } => {
                *current = policy;
            }
        }
        self
    }
}

/// One request to prevent idle system sleep or display sleep.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WakeLeaseRequest {
    pub kind: WakeInhibitionKind,
    pub reason: WakeReason,
    pub scope: WakeLeaseScope,
}

impl WakeLeaseRequest {
    pub fn new(kind: WakeInhibitionKind, reason: WakeReason) -> Self {
        Self {
            kind,
            reason,
            scope: WakeLeaseScope::application(ReleasePolicy::for_kind(kind)),
        }
    }

    pub fn system_idle(reason: WakeReason) -> Self {
        Self::new(WakeInhibitionKind::SystemIdleSleep, reason)
    }

    pub fn display_and_system(reason: WakeReason) -> Self {
        Self::new(WakeInhibitionKind::DisplayAndSystem, reason)
    }

    pub fn with_scope(mut self, scope: WakeLeaseScope) -> Self {
        self.scope = scope;
        self
    }

    pub fn with_policy(mut self, policy: ReleasePolicy) -> Self {
        self.scope = self.scope.with_policy(policy);
        self
    }

    pub const fn policy(&self) -> ReleasePolicy {
        self.scope.policy()
    }
}

/// Opaque lease identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct WakeLeaseId(pub u64);

/// Why a previously effective lease stopped being effective.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WakeRevocationReason {
    /// The OS or provider revoked the native token.
    SystemRevoked,
    /// The session locked.
    SessionLocked,
    /// The system began preparing for or entered sleep.
    SystemSleeping,
    /// The application moved to the background or a surface became hidden.
    AppBackgrounded,
    /// The OS low-power / battery-saver policy revoked the inhibition.
    LowPowerSaver,
    /// The owning surface was disposed.
    SurfaceDisposed,
    /// A newer attachment generation superseded the lease.
    GenerationExpired,
    /// The required permission was revoked.
    PermissionRevoked,
}

impl WakeRevocationReason {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::SystemRevoked => "system_revoked",
            Self::SessionLocked => "session_locked",
            Self::SystemSleeping => "system_sleeping",
            Self::AppBackgrounded => "app_backgrounded",
            Self::LowPowerSaver => "low_power_saver",
            Self::SurfaceDisposed => "surface_disposed",
            Self::GenerationExpired => "generation_expired",
            Self::PermissionRevoked => "permission_revoked",
        }
    }
}

impl fmt::Display for WakeRevocationReason {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// The authoritative state of a [`crate::WakePreventionLease`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WakeLeaseState {
    Effective,
    Revoked(WakeRevocationReason),
    Released,
}

impl WakeLeaseState {
    pub const fn is_effective(self) -> bool {
        matches!(self, Self::Effective)
    }

    pub const fn revocation_reason(self) -> Option<WakeRevocationReason> {
        match self {
            Self::Revoked(reason) => Some(reason),
            Self::Effective | Self::Released => None,
        }
    }
}

/// Aggregated effectiveness across all currently effective leases.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct EffectiveWakeStatus {
    pub inhibits_system_sleep: bool,
    pub inhibits_display_sleep: bool,
    pub active_lease_count: usize,
    pub system_lease_count: usize,
    pub display_lease_count: usize,
}

impl EffectiveWakeStatus {
    pub const fn none() -> Self {
        Self {
            inhibits_system_sleep: false,
            inhibits_display_sleep: false,
            active_lease_count: 0,
            system_lease_count: 0,
            display_lease_count: 0,
        }
    }

    pub const fn is_effective(&self) -> bool {
        self.active_lease_count > 0
    }
}

impl WakeInhibitionKind {
    /// The capability-gated operation required to acquire this kind.
    pub const fn operation(self) -> crate::PowerOperation {
        match self {
            Self::SystemIdleSleep => crate::PowerOperation::InhibitSystemSleep,
            Self::DisplayAndSystem => crate::PowerOperation::InhibitDisplaySleep,
        }
    }
}
