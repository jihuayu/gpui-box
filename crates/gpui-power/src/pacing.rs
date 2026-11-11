//! Explicit product power-budget policy and deterministic work pacing.

use crate::{
    BatteryStatus, ChargingState, LowPowerMode, MemoryCondition, PowerSnapshot, SessionLockState,
    SleepResumeState, ThermalCondition,
};
use serde::{Deserialize, Serialize};

/// Deterministic cadence tier recommended to background producers.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, Default,
)]
#[serde(rename_all = "snake_case")]
pub enum PacingTier {
    /// No power-related restriction.
    #[default]
    Normal,
    /// Prefer longer intervals; speculative work remains allowed.
    Reduced,
    /// Only essential work; speculative and discretionary sync are disabled.
    Throttled,
    /// Stop pollable work until the condition clears.
    Paused,
}

impl PacingTier {
    pub const ALL: [Self; 4] = [Self::Normal, Self::Reduced, Self::Throttled, Self::Paused];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Normal => "normal",
            Self::Reduced => "reduced",
            Self::Throttled => "throttled",
            Self::Paused => "paused",
        }
    }

    /// Poll interval multiplier. Zero means paused.
    pub const fn poll_multiplier(self) -> u32 {
        match self {
            Self::Normal => 1,
            Self::Reduced => 2,
            Self::Throttled => 4,
            Self::Paused => 0,
        }
    }

    pub const fn allows_speculative_work(self) -> bool {
        matches!(self, Self::Normal | Self::Reduced)
    }

    pub const fn allows_discretionary_background_sync(self) -> bool {
        matches!(self, Self::Normal | Self::Reduced)
    }
}

impl std::fmt::Display for PacingTier {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// Alias matching the issue's work-cadence wording.
pub type WorkCadence = PacingTier;

/// One deterministic explanation for the recommended tier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PacingReason {
    SystemSleeping,
    ThermalCritical,
    MemoryCritical,
    ThermalSerious,
    ThermalFair,
    MemoryModerate,
    LowPowerModeEnabled,
    LowBattery,
    BatteryDischarging,
    SessionLocked,
    UnknownThermalConservative,
}

impl PacingReason {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::SystemSleeping => "system_sleeping",
            Self::ThermalCritical => "thermal_critical",
            Self::MemoryCritical => "memory_critical",
            Self::ThermalSerious => "thermal_serious",
            Self::ThermalFair => "thermal_fair",
            Self::MemoryModerate => "memory_moderate",
            Self::LowPowerModeEnabled => "low_power_mode_enabled",
            Self::LowBattery => "low_battery",
            Self::BatteryDischarging => "battery_discharging",
            Self::SessionLocked => "session_locked",
            Self::UnknownThermalConservative => "unknown_thermal_conservative",
        }
    }
}

impl std::fmt::Display for PacingReason {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// Product-owned power-budget policy.
///
/// The policy is data, not hidden product behavior: callers can inspect,
/// serialize, replace and override it. Platform invariants (for example sleep)
/// still apply regardless of these flags.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct PowerBudgetPolicy {
    pub pause_speculative_on_low_power: bool,
    pub pause_speculative_on_locked: bool,
    pub reduce_on_battery_discharging: bool,
    pub treat_unknown_thermal_conservatively: bool,
    pub low_battery_threshold_percent: u8,
}

impl PowerBudgetPolicy {
    pub const fn new() -> Self {
        Self {
            pause_speculative_on_low_power: true,
            pause_speculative_on_locked: true,
            reduce_on_battery_discharging: true,
            treat_unknown_thermal_conservatively: true,
            low_battery_threshold_percent: 20,
        }
    }

    pub const fn with_pause_speculative_on_low_power(mut self, pause: bool) -> Self {
        self.pause_speculative_on_low_power = pause;
        self
    }

    pub const fn with_pause_speculative_on_locked(mut self, pause: bool) -> Self {
        self.pause_speculative_on_locked = pause;
        self
    }

    pub const fn with_reduce_on_battery_discharging(mut self, reduce: bool) -> Self {
        self.reduce_on_battery_discharging = reduce;
        self
    }

    pub const fn with_treat_unknown_thermal_conservatively(mut self, conservative: bool) -> Self {
        self.treat_unknown_thermal_conservatively = conservative;
        self
    }

    pub const fn with_low_battery_threshold_percent(mut self, threshold: u8) -> Self {
        self.low_battery_threshold_percent = threshold;
        self
    }

    /// Evaluate the recommendation with a fixed, documented precedence.
    pub fn evaluate(&self, snapshot: &PowerSnapshot) -> WorkPacingRecommendation {
        if snapshot.sleep_resume.state.is_sleeping() {
            return WorkPacingRecommendation::for_tier(
                PacingTier::Paused,
                vec![PacingReason::SystemSleeping],
            );
        }

        if snapshot.thermal.condition == ThermalCondition::Critical
            || snapshot.memory.condition == MemoryCondition::Critical
        {
            let mut reasons = Vec::new();
            if snapshot.thermal.condition == ThermalCondition::Critical {
                reasons.push(PacingReason::ThermalCritical);
            }
            if snapshot.memory.condition == MemoryCondition::Critical {
                reasons.push(PacingReason::MemoryCritical);
            }
            return WorkPacingRecommendation::for_tier(PacingTier::Paused, reasons);
        }

        let low_power = snapshot.battery.low_power_mode == LowPowerMode::Enabled;
        let locked = matches!(
            snapshot.session_lock.state,
            SessionLockState::Locking | SessionLockState::Locked
        ) && self.pause_speculative_on_locked;
        let low_battery = battery_is_low(&snapshot.battery, self.low_battery_threshold_percent);
        let discharging = snapshot.battery.charging == ChargingState::Discharging;
        let unknown_thermal = snapshot.thermal.condition == ThermalCondition::Unknown
            && self.treat_unknown_thermal_conservatively;

        let throttled = snapshot.thermal.condition == ThermalCondition::Serious
            || (low_power && self.pause_speculative_on_low_power)
            || locked
            || low_battery;
        let reduced = !throttled
            && (snapshot.thermal.condition == ThermalCondition::Fair
                || snapshot.memory.condition == MemoryCondition::Moderate
                || (discharging && self.reduce_on_battery_discharging)
                || (low_power && !self.pause_speculative_on_low_power)
                || unknown_thermal);

        let mut reasons = Vec::new();
        if snapshot.thermal.condition == ThermalCondition::Serious {
            reasons.push(PacingReason::ThermalSerious);
        }
        if snapshot.thermal.condition == ThermalCondition::Fair {
            reasons.push(PacingReason::ThermalFair);
        }
        if snapshot.memory.condition == MemoryCondition::Moderate {
            reasons.push(PacingReason::MemoryModerate);
        }
        if low_power {
            reasons.push(PacingReason::LowPowerModeEnabled);
        }
        if low_battery {
            reasons.push(PacingReason::LowBattery);
        }
        if discharging && self.reduce_on_battery_discharging {
            reasons.push(PacingReason::BatteryDischarging);
        }
        if locked {
            reasons.push(PacingReason::SessionLocked);
        }
        if unknown_thermal {
            reasons.push(PacingReason::UnknownThermalConservative);
        }

        let tier = if throttled {
            PacingTier::Throttled
        } else if reduced {
            PacingTier::Reduced
        } else {
            PacingTier::Normal
        };
        WorkPacingRecommendation::for_tier(tier, reasons)
    }
}

impl Default for PowerBudgetPolicy {
    fn default() -> Self {
        Self::new()
    }
}

fn battery_is_low(battery: &BatteryStatus, threshold: u8) -> bool {
    if battery.charging != ChargingState::Discharging {
        return false;
    }
    battery.is_low(threshold)
}

/// Deterministic recommendation consumed by background schedulers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkPacingRecommendation {
    pub tier: PacingTier,
    pub allow_speculative_work: bool,
    pub allow_discretionary_background_sync: bool,
    pub recommended_poll_multiplier: u32,
    pub reasons: Vec<PacingReason>,
}

impl WorkPacingRecommendation {
    pub fn for_tier(tier: PacingTier, reasons: Vec<PacingReason>) -> Self {
        Self {
            tier,
            allow_speculative_work: tier.allows_speculative_work(),
            allow_discretionary_background_sync: tier.allows_discretionary_background_sync(),
            recommended_poll_multiplier: tier.poll_multiplier(),
            reasons,
        }
    }

    pub fn normal() -> Self {
        Self::for_tier(PacingTier::Normal, Vec::new())
    }

    pub fn is_paused(&self) -> bool {
        self.tier == PacingTier::Paused
    }

    pub fn has_reason(&self, reason: PacingReason) -> bool {
        self.reasons.contains(&reason)
    }
}

impl Default for WorkPacingRecommendation {
    fn default() -> Self {
        Self::normal()
    }
}

/// The snapshot's sleep state alone forces a pause, regardless of policy flags.
pub fn snapshot_requires_pause(snapshot: &PowerSnapshot) -> bool {
    matches!(
        snapshot.sleep_resume.state,
        SleepResumeState::PreparingSleep | SleepResumeState::Sleeping
    )
}
