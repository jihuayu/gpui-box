mod support;

use gpui_lifecycle::{MemoryPressure, SurfaceId};
use gpui_power::*;

#[test]
fn signal_sources_are_distinct_and_never_imply_observation() {
    let sources = [
        SignalSource::Observed,
        SignalSource::UnsupportedByPlatform,
        SignalSource::PermissionDenied,
        SignalSource::ProviderUnavailable,
        SignalSource::Unknown,
    ];
    assert!(SignalSource::Observed.is_observed());
    assert!(!SignalSource::UnsupportedByPlatform.is_observed());
    assert!(!SignalSource::PermissionDenied.is_observed());
    assert!(!SignalSource::ProviderUnavailable.is_observed());
    assert!(!SignalSource::Unknown.is_observed());
    let labels = sources.map(SignalSource::as_str);
    assert_eq!(
        labels,
        [
            "observed",
            "unsupported_by_platform",
            "permission_denied",
            "provider_unavailable",
            "unknown"
        ]
    );
    let observed = ObservedValue::new(PowerSource::Ac, SignalSource::Observed);
    assert!(observed.is_observed());
    assert!(observed.map(|source| source.is_external()).value);
}

#[test]
fn unknown_snapshot_never_claims_nominal_or_ac() {
    let snapshot = PowerSnapshot::unknown(SignalSource::ProviderUnavailable);
    assert_eq!(snapshot.revision, 0);
    assert_eq!(snapshot.power_source.kind, PowerSource::Unknown);
    assert_eq!(
        snapshot.power_source.source,
        SignalSource::ProviderUnavailable
    );
    assert_eq!(snapshot.battery.charging, ChargingState::Unknown);
    assert_eq!(snapshot.battery.low_power_mode, LowPowerMode::Unknown);
    assert_eq!(snapshot.battery.level_percent(), None);
    assert_eq!(snapshot.battery.source, SignalSource::ProviderUnavailable);
    assert_eq!(snapshot.sleep_resume.state, SleepResumeState::Unknown);
    assert_eq!(snapshot.sleep_resume.resume_epoch, 0);
    assert_eq!(
        snapshot.sleep_resume.source,
        SignalSource::ProviderUnavailable
    );
    assert_eq!(snapshot.session_lock.state, SessionLockState::Unknown);
    assert_eq!(snapshot.thermal.condition, ThermalCondition::Unknown);
    assert_eq!(snapshot.memory.condition, MemoryCondition::Unknown);
    assert!(!snapshot.is_observed());
    assert_eq!(PowerSnapshot::default().revision, 0);
}

#[test]
fn battery_status_rejects_percent_above_one_hundred_at_construction() {
    assert!(
        BatteryStatus::new(
            Some(101),
            ChargingState::Charging,
            LowPowerMode::Disabled,
            None,
            SignalSource::Observed
        )
        .is_err()
    );
    let error = BatteryStatus::new(
        Some(101),
        ChargingState::Charging,
        LowPowerMode::Disabled,
        None,
        SignalSource::Observed,
    )
    .expect_err("101 is invalid");
    assert_eq!(error.code, ErrorCode::InvalidInput);

    let valid = BatteryStatus::new(
        Some(100),
        ChargingState::Full,
        LowPowerMode::Disabled,
        Some(0),
        SignalSource::Observed,
    )
    .expect("100 is valid");
    assert_eq!(valid.level_percent(), Some(100));
    assert_eq!(valid.charging(), ChargingState::Full);
    assert_eq!(valid.remaining_seconds(), Some(0));
    assert!(valid.with_level_percent(101).is_err());
}

#[test]
fn battery_status_rejects_percent_above_one_hundred_on_deserialization() {
    let invalid = r#"{
        "level_percent": 101,
        "charging": "charging",
        "low_power_mode": "disabled",
        "remaining_seconds": null,
        "source": "observed"
    }"#;
    let error = serde_json::from_str::<BatteryStatus>(invalid).expect_err("101 must be rejected");
    assert!(error.to_string().contains("Battery percentage exceeds 100"));

    let valid = r#"{
        "level_percent": 42,
        "charging": "discharging",
        "low_power_mode": "enabled",
        "remaining_seconds": 3600,
        "source": "observed"
    }"#;
    let status = serde_json::from_str::<BatteryStatus>(valid).expect("valid battery");
    assert_eq!(status.level_percent(), Some(42));
    assert_eq!(status.charging, ChargingState::Discharging);
    assert_eq!(status.low_power_mode, LowPowerMode::Enabled);
    assert_eq!(status.remaining_seconds, Some(3600));
    assert_eq!(
        serde_json::to_value(status).expect("serialize")["level_percent"],
        serde_json::json!(42)
    );
}

#[test]
fn snapshot_round_trips_through_serde() {
    let battery = BatteryStatus::new(
        Some(77),
        ChargingState::Discharging,
        LowPowerMode::Disabled,
        Some(900),
        SignalSource::Observed,
    )
    .expect("battery");
    let snapshot = PowerSnapshot {
        power_source: PowerSourceStatus::new(PowerSource::Battery, SignalSource::Observed),
        battery,
        sleep_resume: SleepResumeStatus::new(SleepResumeState::Awake, 3, SignalSource::Observed),
        session_lock: SessionLockStatus::new(SessionLockState::Unlocked, SignalSource::Observed),
        thermal: ThermalStatus::new(ThermalCondition::Fair, SignalSource::Observed),
        memory: MemoryStatus::new(MemoryCondition::Moderate, SignalSource::Observed),
        revision: 9,
    };
    let json = serde_json::to_string(&snapshot).expect("serialize snapshot");
    let restored: PowerSnapshot = serde_json::from_str(&json).expect("deserialize snapshot");
    assert_eq!(restored, snapshot);
    assert!(restored.is_observed());
}

#[test]
fn lifecycle_conversions_are_explicit() {
    assert_eq!(
        SessionLockState::from(gpui_lifecycle::SessionLockState::Unlocked),
        SessionLockState::Unlocked
    );
    assert_eq!(
        SessionLockState::from(gpui_lifecycle::SessionLockState::Locked),
        SessionLockState::Locked
    );
    assert_eq!(
        gpui_lifecycle::SessionLockState::from(SessionLockState::Locking),
        gpui_lifecycle::SessionLockState::Locked
    );
    assert_eq!(
        MemoryCondition::from(MemoryPressure::Normal),
        MemoryCondition::Normal
    );
    assert_eq!(
        MemoryCondition::from(MemoryPressure::Warning),
        MemoryCondition::Moderate
    );
    assert_eq!(
        MemoryCondition::from(MemoryPressure::Critical),
        MemoryCondition::Critical
    );
    assert!(ThermalCondition::Serious.is_elevated());
    assert!(ThermalCondition::Serious.is_serious_or_worse());
    assert!(!ThermalCondition::Fair.is_serious_or_worse());
    assert!(ThermalCondition::Critical.severity_rank() > ThermalCondition::Fair.severity_rank());
    assert!(ThermalCondition::Unknown < ThermalCondition::Nominal);
    assert!(ThermalCondition::Fair < ThermalCondition::Serious);
    assert!(ThermalCondition::Serious < ThermalCondition::Critical);
    assert!(MemoryCondition::Critical.is_critical());
    assert!(MemoryCondition::Moderate.is_elevated());
    assert!(MemoryCondition::Unknown < MemoryCondition::Normal);
    assert!(MemoryCondition::Moderate < MemoryCondition::Critical);
}

#[test]
fn fake_provider_transitions_all_six_domains_with_monotonic_revisions() {
    let fake = FakePowerProvider::new();
    let mut revision = fake.snapshot().revision;

    fake.set_power_source(PowerSource::Battery);
    let snapshot = fake.snapshot();
    assert_eq!(snapshot.power_source.kind, PowerSource::Battery);
    assert_eq!(snapshot.power_source.source, SignalSource::Observed);
    assert!(snapshot.revision > revision);
    revision = snapshot.revision;

    let battery = BatteryStatus::new(
        Some(55),
        ChargingState::Discharging,
        LowPowerMode::Disabled,
        Some(1800),
        SignalSource::Observed,
    )
    .expect("battery");
    fake.set_battery(battery.clone());
    assert_eq!(fake.snapshot().battery, battery);
    assert!(fake.snapshot().revision > revision);
    revision = fake.snapshot().revision;

    fake.set_sleep_resume_state(SleepResumeState::Awake);
    fake.set_sleep_resume_state(SleepResumeState::PreparingSleep);
    fake.set_sleep_resume_state(SleepResumeState::Sleeping);
    fake.set_sleep_resume_state(SleepResumeState::Resumed);
    let resumed = fake.snapshot();
    assert_eq!(resumed.sleep_resume.state, SleepResumeState::Resumed);
    assert_eq!(resumed.sleep_resume.resume_epoch, 1);
    assert!(resumed.revision > revision);
    fake.set_sleep_resume_state(SleepResumeState::Awake);
    assert_eq!(fake.snapshot().sleep_resume.resume_epoch, 1);
    fake.set_sleep_resume_state(SleepResumeState::PreparingSleep);
    fake.set_sleep_resume_state(SleepResumeState::Sleeping);
    fake.set_sleep_resume_state(SleepResumeState::Resumed);
    assert_eq!(fake.snapshot().sleep_resume.resume_epoch, 2);

    for expected in [
        SessionLockState::Unlocked,
        SessionLockState::Locking,
        SessionLockState::Locked,
        SessionLockState::Unlocked,
    ] {
        fake.set_session_lock(expected);
        assert_eq!(fake.snapshot().session_lock.state, expected);
    }

    fake.set_low_power_mode(LowPowerMode::Enabled);
    assert_eq!(
        fake.snapshot().battery.low_power_mode,
        LowPowerMode::Enabled
    );
    fake.set_low_power_mode(LowPowerMode::Disabled);

    for expected in [
        ThermalCondition::Nominal,
        ThermalCondition::Fair,
        ThermalCondition::Serious,
        ThermalCondition::Critical,
        ThermalCondition::Nominal,
    ] {
        fake.set_thermal(expected);
        assert_eq!(fake.snapshot().thermal.condition, expected);
    }

    for expected in [
        MemoryCondition::Normal,
        MemoryCondition::Moderate,
        MemoryCondition::Critical,
        MemoryCondition::Normal,
    ] {
        fake.set_memory(expected);
        assert_eq!(fake.snapshot().memory.condition, expected);
    }

    for expected in [
        ChargingState::Charging,
        ChargingState::Discharging,
        ChargingState::Full,
        ChargingState::NotCharging,
    ] {
        fake.set_charging(expected);
        assert_eq!(fake.snapshot().battery.charging, expected);
    }
    for expected in [
        PowerSource::Ac,
        PowerSource::Battery,
        PowerSource::Ups,
        PowerSource::Usb,
    ] {
        fake.set_power_source(expected);
        assert_eq!(fake.snapshot().power_source.kind, expected);
        assert_eq!(fake.snapshot().power_source.source, SignalSource::Observed);
    }
    assert!(fake.snapshot().revision > revision);
}

#[test]
fn surface_ids_remain_opaque_numeric_values() {
    assert_eq!(SurfaceId(7), SurfaceId(7));
    assert_ne!(SurfaceId(7), SurfaceId(8));
}

#[test]
fn signal_sources_are_preserved_independently_per_domain() {
    let snapshot = PowerSnapshot {
        power_source: PowerSourceStatus::new(
            PowerSource::Battery,
            SignalSource::UnsupportedByPlatform,
        ),
        battery: BatteryStatus::new(
            Some(64),
            ChargingState::Discharging,
            LowPowerMode::Unknown,
            None,
            SignalSource::PermissionDenied,
        )
        .expect("battery"),
        sleep_resume: SleepResumeStatus::new(SleepResumeState::Awake, 2, SignalSource::Observed),
        session_lock: SessionLockStatus::new(
            SessionLockState::Locked,
            SignalSource::ProviderUnavailable,
        ),
        thermal: ThermalStatus::new(ThermalCondition::Nominal, SignalSource::Unknown),
        memory: MemoryStatus::new(MemoryCondition::Moderate, SignalSource::Observed),
        revision: 4,
    };
    assert_eq!(
        snapshot.power_source.source,
        SignalSource::UnsupportedByPlatform
    );
    assert_eq!(snapshot.battery.source, SignalSource::PermissionDenied);
    assert_eq!(snapshot.sleep_resume.source, SignalSource::Observed);
    assert_eq!(
        snapshot.session_lock.source,
        SignalSource::ProviderUnavailable
    );
    assert_eq!(snapshot.thermal.source, SignalSource::Unknown);
    assert_eq!(snapshot.memory.source, SignalSource::Observed);
    assert!(!snapshot.is_observed());
    let restored: PowerSnapshot =
        serde_json::from_str(&serde_json::to_string(&snapshot).expect("serialize"))
            .expect("deserialize");
    assert_eq!(restored, snapshot);
}
