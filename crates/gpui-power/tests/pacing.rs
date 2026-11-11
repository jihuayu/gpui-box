mod support;

use gpui_power::*;
use support::*;

fn nominal_snapshot() -> PowerSnapshot {
    PowerSnapshot {
        power_source: PowerSourceStatus::new(PowerSource::Ac, SignalSource::Observed),
        battery: BatteryStatus::new(
            Some(80),
            ChargingState::Full,
            LowPowerMode::Disabled,
            None,
            SignalSource::Observed,
        )
        .expect("battery"),
        sleep_resume: SleepResumeStatus::new(SleepResumeState::Awake, 0, SignalSource::Observed),
        session_lock: SessionLockStatus::new(SessionLockState::Unlocked, SignalSource::Observed),
        thermal: ThermalStatus::new(ThermalCondition::Nominal, SignalSource::Observed),
        memory: MemoryStatus::new(MemoryCondition::Normal, SignalSource::Observed),
        revision: 1,
    }
}

fn battery(level: Option<u8>, charging: ChargingState, low_power: LowPowerMode) -> BatteryStatus {
    BatteryStatus::new(level, charging, low_power, None, SignalSource::Observed).expect("battery")
}

#[test]
fn nominal_conditions_recommend_normal_cadence() {
    let recommendation = PowerBudgetPolicy::default().evaluate(&nominal_snapshot());
    assert_eq!(recommendation.tier, PacingTier::Normal);
    assert_eq!(recommendation.recommended_poll_multiplier, 1);
    assert!(recommendation.allow_speculative_work);
    assert!(recommendation.allow_discretionary_background_sync);
    assert!(recommendation.reasons.is_empty());
    assert!(!recommendation.is_paused());
}

#[test]
fn discharging_battery_reduces_cadence() {
    let mut snapshot = nominal_snapshot();
    snapshot.power_source = PowerSourceStatus::new(PowerSource::Battery, SignalSource::Observed);
    snapshot.battery = battery(Some(70), ChargingState::Discharging, LowPowerMode::Disabled);
    let recommendation = PowerBudgetPolicy::default().evaluate(&snapshot);
    assert_eq!(recommendation.tier, PacingTier::Reduced);
    assert_eq!(recommendation.recommended_poll_multiplier, 2);
    assert!(recommendation.has_reason(PacingReason::BatteryDischarging));
    assert!(recommendation.allow_speculative_work);
}

#[test]
fn low_battery_throttles_and_custom_policy_overrides() {
    let mut snapshot = nominal_snapshot();
    snapshot.battery = battery(Some(10), ChargingState::Discharging, LowPowerMode::Disabled);
    let recommendation = PowerBudgetPolicy::default().evaluate(&snapshot);
    assert_eq!(recommendation.tier, PacingTier::Throttled);
    assert_eq!(recommendation.recommended_poll_multiplier, 4);
    assert!(!recommendation.allow_speculative_work);
    assert!(!recommendation.allow_discretionary_background_sync);
    assert!(recommendation.has_reason(PacingReason::LowBattery));

    let custom = PowerBudgetPolicy::default().with_low_battery_threshold_percent(5);
    let recommendation = custom.evaluate(&snapshot);
    assert_eq!(recommendation.tier, PacingTier::Reduced);
    assert!(!recommendation.has_reason(PacingReason::LowBattery));

    let custom = PowerBudgetPolicy::default()
        .with_low_battery_threshold_percent(50)
        .with_reduce_on_battery_discharging(false);
    let recommendation = custom.evaluate(&snapshot);
    assert_eq!(recommendation.tier, PacingTier::Throttled);
    assert!(recommendation.has_reason(PacingReason::LowBattery));
    assert!(!recommendation.has_reason(PacingReason::BatteryDischarging));
}

#[test]
fn low_power_locked_and_serious_thermal_throttle() {
    let mut snapshot = nominal_snapshot();
    snapshot.battery = battery(Some(80), ChargingState::Full, LowPowerMode::Enabled);
    let recommendation = PowerBudgetPolicy::default().evaluate(&snapshot);
    assert_eq!(recommendation.tier, PacingTier::Throttled);
    assert!(recommendation.has_reason(PacingReason::LowPowerModeEnabled));
    assert!(!recommendation.allow_speculative_work);

    let relaxed = PowerBudgetPolicy::default().with_pause_speculative_on_low_power(false);
    let recommendation = relaxed.evaluate(&snapshot);
    assert_eq!(recommendation.tier, PacingTier::Reduced);
    assert!(recommendation.allow_speculative_work);
    assert!(recommendation.has_reason(PacingReason::LowPowerModeEnabled));

    let mut snapshot = nominal_snapshot();
    snapshot.session_lock =
        SessionLockStatus::new(SessionLockState::Locked, SignalSource::Observed);
    let recommendation = PowerBudgetPolicy::default().evaluate(&snapshot);
    assert_eq!(recommendation.tier, PacingTier::Throttled);
    assert!(recommendation.has_reason(PacingReason::SessionLocked));

    let relaxed = PowerBudgetPolicy::default().with_pause_speculative_on_locked(false);
    let recommendation = relaxed.evaluate(&snapshot);
    assert_eq!(recommendation.tier, PacingTier::Normal);
    assert!(!recommendation.has_reason(PacingReason::SessionLocked));

    let mut snapshot = nominal_snapshot();
    snapshot.thermal = ThermalStatus::new(ThermalCondition::Serious, SignalSource::Observed);
    let recommendation = PowerBudgetPolicy::default().evaluate(&snapshot);
    assert_eq!(recommendation.tier, PacingTier::Throttled);
    assert!(recommendation.has_reason(PacingReason::ThermalSerious));
    assert!(!recommendation.allow_speculative_work);
}

#[test]
fn thermal_and_memory_fairness_reduce_cadence() {
    let mut snapshot = nominal_snapshot();
    snapshot.thermal = ThermalStatus::new(ThermalCondition::Fair, SignalSource::Observed);
    let recommendation = PowerBudgetPolicy::default().evaluate(&snapshot);
    assert_eq!(recommendation.tier, PacingTier::Reduced);
    assert!(recommendation.has_reason(PacingReason::ThermalFair));

    let mut snapshot = nominal_snapshot();
    snapshot.memory = MemoryStatus::new(MemoryCondition::Moderate, SignalSource::Observed);
    let recommendation = PowerBudgetPolicy::default().evaluate(&snapshot);
    assert_eq!(recommendation.tier, PacingTier::Reduced);
    assert!(recommendation.has_reason(PacingReason::MemoryModerate));

    let mut snapshot = nominal_snapshot();
    snapshot.thermal = ThermalStatus::new(ThermalCondition::Unknown, SignalSource::Unknown);
    let recommendation = PowerBudgetPolicy::default().evaluate(&snapshot);
    assert_eq!(recommendation.tier, PacingTier::Reduced);
    assert!(recommendation.has_reason(PacingReason::UnknownThermalConservative));

    let relaxed = PowerBudgetPolicy::default().with_treat_unknown_thermal_conservatively(false);
    let recommendation = relaxed.evaluate(&snapshot);
    assert_eq!(recommendation.tier, PacingTier::Normal);
    assert!(recommendation.reasons.is_empty());
}

#[test]
fn critical_thermal_and_memory_pause_work() {
    let mut snapshot = nominal_snapshot();
    snapshot.thermal = ThermalStatus::new(ThermalCondition::Critical, SignalSource::Observed);
    let recommendation = PowerBudgetPolicy::default().evaluate(&snapshot);
    assert_eq!(recommendation.tier, PacingTier::Paused);
    assert_eq!(recommendation.recommended_poll_multiplier, 0);
    assert!(!recommendation.allow_speculative_work);
    assert!(!recommendation.allow_discretionary_background_sync);
    assert!(recommendation.has_reason(PacingReason::ThermalCritical));

    snapshot.memory = MemoryStatus::new(MemoryCondition::Critical, SignalSource::Observed);
    let recommendation = PowerBudgetPolicy::default().evaluate(&snapshot);
    assert_eq!(recommendation.tier, PacingTier::Paused);
    assert!(recommendation.has_reason(PacingReason::ThermalCritical));
    assert!(recommendation.has_reason(PacingReason::MemoryCritical));
}

#[test]
fn sleep_always_pauses_regardless_of_policy() {
    let mut snapshot = nominal_snapshot();
    snapshot.sleep_resume =
        SleepResumeStatus::new(SleepResumeState::PreparingSleep, 4, SignalSource::Observed);
    let policy = PowerBudgetPolicy::default()
        .with_pause_speculative_on_low_power(false)
        .with_pause_speculative_on_locked(false)
        .with_reduce_on_battery_discharging(false)
        .with_treat_unknown_thermal_conservatively(false);
    let recommendation = policy.evaluate(&snapshot);
    assert_eq!(recommendation.tier, PacingTier::Paused);
    assert_eq!(recommendation.reasons, vec![PacingReason::SystemSleeping]);
    assert!(snapshot_requires_pause(&snapshot));

    snapshot.sleep_resume.state = SleepResumeState::Sleeping;
    assert!(snapshot_requires_pause(&snapshot));
    snapshot.sleep_resume.state = SleepResumeState::Resumed;
    assert!(!snapshot_requires_pause(&snapshot));
}

#[test]
fn policy_is_serializable_and_defaults_are_explicit() {
    let policy = PowerBudgetPolicy::default();
    assert!(policy.pause_speculative_on_low_power);
    assert!(policy.pause_speculative_on_locked);
    assert!(policy.reduce_on_battery_discharging);
    assert!(policy.treat_unknown_thermal_conservatively);
    assert_eq!(policy.low_battery_threshold_percent, 20);
    let json = serde_json::to_string(&policy).expect("serialize");
    let restored: PowerBudgetPolicy = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(restored, policy);
    assert_eq!(
        PowerBudgetPolicy::default()
            .with_low_battery_threshold_percent(255)
            .low_battery_threshold_percent,
        255
    );
}

#[test]
fn work_cadence_alias_and_tier_ordering() {
    let cadence: WorkCadence = PacingTier::Throttled;
    assert_eq!(cadence, PacingTier::Throttled);
    assert!(PacingTier::Normal < PacingTier::Reduced);
    assert!(PacingTier::Reduced < PacingTier::Throttled);
    assert!(PacingTier::Throttled < PacingTier::Paused);
    assert_eq!(PacingTier::Normal.poll_multiplier(), 1);
    assert_eq!(PacingTier::Reduced.poll_multiplier(), 2);
    assert_eq!(PacingTier::Throttled.poll_multiplier(), 4);
    assert_eq!(PacingTier::Paused.poll_multiplier(), 0);
    assert_eq!(WorkPacingRecommendation::normal().tier, PacingTier::Normal);
}

#[test]
fn service_recommendation_tracks_snapshot_and_policy_updates() {
    let (fake, service) = fake_service();
    fake.set_thermal(ThermalCondition::Nominal);
    fake.set_memory(MemoryCondition::Normal);
    fake.set_session_lock(SessionLockState::Unlocked);
    fake.set_sleep_resume_state(SleepResumeState::Awake);
    let recommendation = service.recommend_pacing();
    assert_eq!(recommendation.tier, PacingTier::Normal);

    fake.set_thermal(ThermalCondition::Serious);
    assert_eq!(service.recommend_pacing().tier, PacingTier::Throttled);

    let relaxed = PowerBudgetPolicy::default()
        .with_pause_speculative_on_locked(false)
        .with_treat_unknown_thermal_conservatively(false);
    service.set_budget_policy(relaxed.clone());
    assert_eq!(service.budget_policy(), relaxed);
    assert_eq!(service.recommend_pacing().tier, PacingTier::Throttled);

    fake.set_thermal(ThermalCondition::Nominal);
    assert_eq!(service.recommend_pacing().tier, PacingTier::Normal);
}
