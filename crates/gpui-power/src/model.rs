//! Power, battery, sleep/resume, session, thermal and memory observations.
//!
//! Every observed domain carries a [`SignalSource`]. A value is never assumed
//! to be nominal just because the platform did not report anything: absent
//! observations stay explicitly `Unknown` with the reason recorded.

use crate::{ErrorCode, SystemError};
use serde::{Deserialize, Serialize};
use std::fmt;

/// How an observation was obtained.
///
/// The `Unknown` family is deliberately explicit. Callers must not treat an
/// unobservable domain as `Ac`, `Unlocked`, `Nominal` or `Normal`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SignalSource {
    /// The platform reported a fresh value.
    Observed,
    /// The platform has no public API for this observation.
    UnsupportedByPlatform,
    /// The host or user denied the permission needed to observe.
    PermissionDenied,
    /// The provider exists but is currently unavailable or failed.
    ProviderUnavailable,
    /// No observation has been attempted or none is trustworthy.
    Unknown,
}

impl SignalSource {
    /// True only for a fresh platform observation.
    pub const fn is_observed(self) -> bool {
        matches!(self, Self::Observed)
    }

    /// Stable public-safe label.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Observed => "observed",
            Self::UnsupportedByPlatform => "unsupported_by_platform",
            Self::PermissionDenied => "permission_denied",
            Self::ProviderUnavailable => "provider_unavailable",
            Self::Unknown => "unknown",
        }
    }
}

impl fmt::Display for SignalSource {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// A value paired with its provenance.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ObservedValue<T> {
    pub value: T,
    pub source: SignalSource,
}

impl<T> ObservedValue<T> {
    pub const fn new(value: T, source: SignalSource) -> Self {
        Self { value, source }
    }

    pub const fn is_observed(&self) -> bool {
        self.source.is_observed()
    }

    pub fn map<U>(self, map: impl FnOnce(T) -> U) -> ObservedValue<U> {
        ObservedValue {
            value: map(self.value),
            source: self.source,
        }
    }
}

/// The current power source family.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PowerSource {
    Ac,
    Battery,
    Ups,
    Usb,
    Unknown,
}

impl PowerSource {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Ac => "ac",
            Self::Battery => "battery",
            Self::Ups => "ups",
            Self::Usb => "usb",
            Self::Unknown => "unknown",
        }
    }

    pub const fn is_external(self) -> bool {
        matches!(self, Self::Ac | Self::Ups | Self::Usb)
    }
}

impl fmt::Display for PowerSource {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// A power-source observation with provenance.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct PowerSourceStatus {
    pub kind: PowerSource,
    pub source: SignalSource,
}

impl PowerSourceStatus {
    pub const fn new(kind: PowerSource, source: SignalSource) -> Self {
        Self { kind, source }
    }

    pub const fn unknown(source: SignalSource) -> Self {
        Self {
            kind: PowerSource::Unknown,
            source,
        }
    }

    pub const fn is_observed(&self) -> bool {
        self.source.is_observed()
    }
}

/// Battery charging state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChargingState {
    Charging,
    Discharging,
    Full,
    NotCharging,
    Unknown,
}

impl ChargingState {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Charging => "charging",
            Self::Discharging => "discharging",
            Self::Full => "full",
            Self::NotCharging => "not_charging",
            Self::Unknown => "unknown",
        }
    }

    pub const fn is_discharging(self) -> bool {
        matches!(self, Self::Discharging)
    }
}

impl fmt::Display for ChargingState {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// The OS low-power / battery-saver mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LowPowerMode {
    Enabled,
    Disabled,
    Unknown,
}

impl LowPowerMode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Enabled => "enabled",
            Self::Disabled => "disabled",
            Self::Unknown => "unknown",
        }
    }

    pub const fn is_enabled(self) -> bool {
        matches!(self, Self::Enabled)
    }
}

impl fmt::Display for LowPowerMode {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// Serializable representation used to validate battery percentages on both
/// construction and deserialization.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct BatteryStatusRepr {
    level_percent: Option<u8>,
    charging: ChargingState,
    low_power_mode: LowPowerMode,
    remaining_seconds: Option<u64>,
    source: SignalSource,
}

/// A validated battery observation.
///
/// `level_percent` is private and validated by [`BatteryStatus::new`] and by
/// `Deserialize`; a value above 100 can never exist in this type.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "BatteryStatusRepr", into = "BatteryStatusRepr")]
pub struct BatteryStatus {
    level_percent: Option<u8>,
    pub charging: ChargingState,
    pub low_power_mode: LowPowerMode,
    pub remaining_seconds: Option<u64>,
    pub source: SignalSource,
}

impl BatteryStatus {
    /// Constructs a validated battery observation.
    ///
    /// A percentage above 100 is rejected with [`ErrorCode::InvalidInput`]
    /// before any value is retained.
    pub fn new(
        level_percent: Option<u8>,
        charging: ChargingState,
        low_power_mode: LowPowerMode,
        remaining_seconds: Option<u64>,
        source: SignalSource,
    ) -> Result<Self, SystemError> {
        if level_percent.is_some_and(|level| level > 100) {
            return Err(SystemError::new(
                ErrorCode::InvalidInput,
                "Battery percentage exceeds 100",
            ));
        }
        Ok(Self {
            level_percent,
            charging,
            low_power_mode,
            remaining_seconds,
            source,
        })
    }

    /// An honest unknown battery observation with an explicit provenance.
    pub const fn unknown(source: SignalSource) -> Self {
        Self {
            level_percent: None,
            charging: ChargingState::Unknown,
            low_power_mode: LowPowerMode::Unknown,
            remaining_seconds: None,
            source,
        }
    }

    pub const fn level_percent(&self) -> Option<u8> {
        self.level_percent
    }

    pub const fn charging(&self) -> ChargingState {
        self.charging
    }

    pub const fn low_power_mode(&self) -> LowPowerMode {
        self.low_power_mode
    }

    pub const fn remaining_seconds(&self) -> Option<u64> {
        self.remaining_seconds
    }

    pub const fn source(&self) -> SignalSource {
        self.source
    }

    pub const fn is_observed(&self) -> bool {
        self.source.is_observed()
    }

    pub const fn is_low(&self, threshold_percent: u8) -> bool {
        match self.level_percent {
            Some(level) => level <= threshold_percent,
            None => false,
        }
    }

    pub fn with_level_percent(mut self, level_percent: u8) -> Result<Self, SystemError> {
        if level_percent > 100 {
            return Err(SystemError::new(
                ErrorCode::InvalidInput,
                "Battery percentage exceeds 100",
            ));
        }
        self.level_percent = Some(level_percent);
        Ok(self)
    }

    pub fn with_charging(mut self, charging: ChargingState) -> Self {
        self.charging = charging;
        self
    }

    pub fn with_low_power_mode(mut self, low_power_mode: LowPowerMode) -> Self {
        self.low_power_mode = low_power_mode;
        self
    }

    pub fn with_remaining_seconds(mut self, remaining_seconds: u64) -> Self {
        self.remaining_seconds = Some(remaining_seconds);
        self
    }

    pub fn with_source(mut self, source: SignalSource) -> Self {
        self.source = source;
        self
    }
}

impl From<BatteryStatus> for BatteryStatusRepr {
    fn from(status: BatteryStatus) -> Self {
        Self {
            level_percent: status.level_percent,
            charging: status.charging,
            low_power_mode: status.low_power_mode,
            remaining_seconds: status.remaining_seconds,
            source: status.source,
        }
    }
}

impl TryFrom<BatteryStatusRepr> for BatteryStatus {
    type Error = SystemError;

    fn try_from(repr: BatteryStatusRepr) -> Result<Self, Self::Error> {
        Self::new(
            repr.level_percent,
            repr.charging,
            repr.low_power_mode,
            repr.remaining_seconds,
            repr.source,
        )
    }
}

/// Sleep / resume state reported by the OS or an explicit test provider.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SleepResumeState {
    Awake,
    PreparingSleep,
    Sleeping,
    Resumed,
    Unknown,
}

impl SleepResumeState {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Awake => "awake",
            Self::PreparingSleep => "preparing_sleep",
            Self::Sleeping => "sleeping",
            Self::Resumed => "resumed",
            Self::Unknown => "unknown",
        }
    }

    /// True while the OS is entering or already in system sleep.
    pub const fn is_sleeping(self) -> bool {
        matches!(self, Self::PreparingSleep | Self::Sleeping)
    }

    pub const fn is_resumed(self) -> bool {
        matches!(self, Self::Resumed)
    }
}

impl fmt::Display for SleepResumeState {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// Sleep/resume observation with a monotonic resume epoch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SleepResumeStatus {
    pub state: SleepResumeState,
    /// Incremented when a sleeping/preparing state becomes `Resumed` or
    /// `Awake`. It never decreases.
    pub resume_epoch: u64,
    pub source: SignalSource,
}

impl SleepResumeStatus {
    pub const fn new(state: SleepResumeState, resume_epoch: u64, source: SignalSource) -> Self {
        Self {
            state,
            resume_epoch,
            source,
        }
    }

    pub const fn unknown(source: SignalSource) -> Self {
        Self {
            state: SleepResumeState::Unknown,
            resume_epoch: 0,
            source,
        }
    }

    pub const fn is_observed(&self) -> bool {
        self.source.is_observed()
    }

    pub const fn is_sleeping(&self) -> bool {
        self.state.is_sleeping()
    }
}

/// Session lock observation, including the transition into the lock.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionLockState {
    Unlocked,
    Locking,
    Locked,
    Unknown,
}

impl SessionLockState {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Unlocked => "unlocked",
            Self::Locking => "locking",
            Self::Locked => "locked",
            Self::Unknown => "unknown",
        }
    }

    /// True while the session is locking or locked.
    pub const fn is_locked(self) -> bool {
        matches!(self, Self::Locking | Self::Locked)
    }

    pub const fn is_unlocked(self) -> bool {
        matches!(self, Self::Unlocked)
    }
}

impl fmt::Display for SessionLockState {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl From<gpui_lifecycle::SessionLockState> for SessionLockState {
    fn from(state: gpui_lifecycle::SessionLockState) -> Self {
        match state {
            gpui_lifecycle::SessionLockState::Unlocked => Self::Unlocked,
            gpui_lifecycle::SessionLockState::Locked => Self::Locked,
        }
    }
}

impl From<SessionLockState> for gpui_lifecycle::SessionLockState {
    fn from(state: SessionLockState) -> Self {
        match state {
            SessionLockState::Locking | SessionLockState::Locked => Self::Locked,
            SessionLockState::Unlocked | SessionLockState::Unknown => Self::Unlocked,
        }
    }
}

/// Session lock observation with provenance.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionLockStatus {
    pub state: SessionLockState,
    pub source: SignalSource,
}

impl SessionLockStatus {
    pub const fn new(state: SessionLockState, source: SignalSource) -> Self {
        Self { state, source }
    }

    pub const fn unknown(source: SignalSource) -> Self {
        Self {
            state: SessionLockState::Unknown,
            source,
        }
    }

    pub const fn is_observed(&self) -> bool {
        self.source.is_observed()
    }

    pub const fn is_locked(&self) -> bool {
        self.state.is_locked()
    }
}

/// Thermal pressure reported by the OS.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ThermalCondition {
    Nominal,
    Fair,
    Serious,
    Critical,
    Unknown,
}

impl ThermalCondition {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Nominal => "nominal",
            Self::Fair => "fair",
            Self::Serious => "serious",
            Self::Critical => "critical",
            Self::Unknown => "unknown",
        }
    }

    /// Any reported elevation above nominal. `Unknown` is not elevated.
    pub const fn is_elevated(self) -> bool {
        matches!(self, Self::Fair | Self::Serious | Self::Critical)
    }

    pub const fn is_serious_or_worse(self) -> bool {
        matches!(self, Self::Serious | Self::Critical)
    }

    /// Stable severity order used by deterministic policy evaluation.
    pub const fn severity_rank(self) -> u8 {
        match self {
            Self::Unknown => 0,
            Self::Nominal => 1,
            Self::Fair => 2,
            Self::Serious => 3,
            Self::Critical => 4,
        }
    }
}

impl fmt::Display for ThermalCondition {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl PartialOrd for ThermalCondition {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for ThermalCondition {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.severity_rank().cmp(&other.severity_rank())
    }
}

/// Thermal observation with provenance.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ThermalStatus {
    pub condition: ThermalCondition,
    pub source: SignalSource,
}

impl ThermalStatus {
    pub const fn new(condition: ThermalCondition, source: SignalSource) -> Self {
        Self { condition, source }
    }

    pub const fn unknown(source: SignalSource) -> Self {
        Self {
            condition: ThermalCondition::Unknown,
            source,
        }
    }

    pub const fn is_observed(&self) -> bool {
        self.source.is_observed()
    }
}

/// Memory pressure reported by the OS.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryCondition {
    Normal,
    Moderate,
    Critical,
    Unknown,
}

impl MemoryCondition {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Normal => "normal",
            Self::Moderate => "moderate",
            Self::Critical => "critical",
            Self::Unknown => "unknown",
        }
    }

    pub const fn is_elevated(self) -> bool {
        matches!(self, Self::Moderate | Self::Critical)
    }

    pub const fn is_critical(self) -> bool {
        matches!(self, Self::Critical)
    }

    pub const fn severity_rank(self) -> u8 {
        match self {
            Self::Unknown => 0,
            Self::Normal => 1,
            Self::Moderate => 2,
            Self::Critical => 3,
        }
    }
}

impl fmt::Display for MemoryCondition {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl PartialOrd for MemoryCondition {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for MemoryCondition {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.severity_rank().cmp(&other.severity_rank())
    }
}

impl From<gpui_lifecycle::MemoryPressure> for MemoryCondition {
    fn from(pressure: gpui_lifecycle::MemoryPressure) -> Self {
        match pressure {
            gpui_lifecycle::MemoryPressure::Normal => Self::Normal,
            gpui_lifecycle::MemoryPressure::Warning => Self::Moderate,
            gpui_lifecycle::MemoryPressure::Critical => Self::Critical,
        }
    }
}

/// Memory observation with provenance.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemoryStatus {
    pub condition: MemoryCondition,
    pub source: SignalSource,
}

impl MemoryStatus {
    pub const fn new(condition: MemoryCondition, source: SignalSource) -> Self {
        Self { condition, source }
    }

    pub const fn unknown(source: SignalSource) -> Self {
        Self {
            condition: MemoryCondition::Unknown,
            source,
        }
    }

    pub const fn is_observed(&self) -> bool {
        self.source.is_observed()
    }
}

/// Alias matching the issue's "sleep/resume observation" wording.
pub type SleepResumeObservation = SleepResumeStatus;
/// Alias matching the issue's "session lock observation" wording.
pub type SessionLockObservation = SessionLockStatus;
/// Alias matching the issue's "thermal observation" wording.
pub type ThermalObservation = ThermalStatus;
/// Alias matching the issue's "memory observation" wording.
pub type MemoryObservation = MemoryStatus;

/// The complete cached observation.
///
/// Every domain independently carries its provenance. All six domains default
/// to `Unknown`; construction never fabricates nominal values.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PowerSnapshot {
    pub power_source: PowerSourceStatus,
    pub battery: BatteryStatus,
    pub sleep_resume: SleepResumeStatus,
    pub session_lock: SessionLockStatus,
    pub thermal: ThermalStatus,
    pub memory: MemoryStatus,
    pub revision: u64,
}

impl PowerSnapshot {
    /// An entirely unknown snapshot with one explicit provenance.
    pub fn unknown(source: SignalSource) -> Self {
        Self {
            power_source: PowerSourceStatus::unknown(source),
            battery: BatteryStatus::unknown(source),
            sleep_resume: SleepResumeStatus::unknown(source),
            session_lock: SessionLockStatus::unknown(source),
            thermal: ThermalStatus::unknown(source),
            memory: MemoryStatus::unknown(source),
            revision: 0,
        }
    }

    pub fn with_revision(mut self, revision: u64) -> Self {
        self.revision = revision;
        self
    }

    pub const fn is_observed(&self) -> bool {
        self.power_source.source.is_observed()
            && self.battery.source.is_observed()
            && self.sleep_resume.source.is_observed()
            && self.session_lock.source.is_observed()
            && self.thermal.source.is_observed()
            && self.memory.source.is_observed()
    }
}

impl Default for PowerSnapshot {
    fn default() -> Self {
        Self::unknown(SignalSource::Unknown)
    }
}
