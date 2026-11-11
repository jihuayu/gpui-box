//! Identity, constraint, receipt, policy and deadline semantics.

mod support;

use gpui_jobs::*;
use std::sync::Arc;
use support::*;

#[test]
fn identities_are_strictly_bounded_ascii() {
    assert!(JobNamespace::new("application").is_ok());
    assert!(JobNamespace::new("worker.v2_1-beta").is_ok());
    for invalid in [
        "",
        ".",
        "..",
        "Upper",
        "with space",
        "with/slash",
        "with\\slash",
        "..hidden",
        "trailing.",
        "-leading",
        "trailing-",
        "tab\t",
        "line\n",
        "é",
    ] {
        assert!(
            JobNamespace::new(invalid).is_err(),
            "namespace {invalid:?} must be rejected"
        );
    }
    assert!(JobNamespace::new("a".repeat(MAX_JOB_IDENTIFIER_BYTES + 1)).is_err());
    assert!(JobNamespace::new("a".repeat(MAX_JOB_IDENTIFIER_BYTES)).is_ok());

    let namespace = JobNamespace::application();
    let job = JobKey::new(namespace.clone(), "sync-messages").expect("key");
    assert_eq!(job.to_string(), "application/sync-messages");
    assert_eq!(
        JobKey::parse("application/sync-messages").expect("parse"),
        job
    );
    assert!(JobKey::parse("missing-separator").is_err());
    assert!(JobKey::parse("a/b/c").is_err());
    assert!(JobKey::parse("/name").is_err());
    assert!(JobKey::parse("namespace/").is_err());
    assert!(!job.storage_id().is_empty());
    assert!(
        job.storage_id()
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit())
    );
}

#[test]
fn progress_is_integer_only_and_clamped() {
    let progress = JobProgress::units(5, Some(10)).expect("progress");
    assert_eq!(progress.basis_points(), 5_000);
    assert_eq!(progress.completed_units(), 5);
    assert_eq!(progress.total_units(), Some(10));
    assert!(!progress.is_complete());

    let clamped = JobProgress::units(50, Some(10)).expect("clamped");
    assert_eq!(clamped.completed_units(), 10);
    assert_eq!(clamped.basis_points(), 10_000);
    assert!(clamped.is_complete());

    assert!(JobProgress::units(1, Some(0)).is_err());
    assert!(JobProgress::from_basis_points(10_001).is_err());
    assert_eq!(
        JobProgress::from_basis_points(10_000)
            .expect("complete")
            .basis_points(),
        10_000
    );
    let staged = JobProgress::units(1, Some(3))
        .expect("progress")
        .with_stage("download")
        .expect("stage");
    assert_eq!(staged.stage(), Some("download"));
    assert!(JobProgress::default().with_stage("").is_err());
    assert!(
        JobProgress::default()
            .with_stage("x".repeat(MAX_STAGE_BYTES + 1))
            .is_err()
    );
    // Integer-only public model: no f32/f64 field exists to format here.
    assert!(!format!("{:?}", staged).contains('.'));
}

#[test]
fn cadence_and_constraints_are_validated() {
    assert!(JobCadence::periodic(59_999, 0).is_err());
    assert!(JobCadence::periodic(MIN_PERIODIC_INTERVAL_MS, 1_000).is_ok());
    assert!(JobCadence::periodic(MIN_PERIODIC_INTERVAL_MS, MIN_PERIODIC_INTERVAL_MS + 1).is_err());
    assert!(JobCadence::OneShot.validate().is_ok());

    let constraints = JobConstraints::default()
        .with_cadence(JobCadence::periodic(MIN_PERIODIC_INTERVAL_MS, 5_000).expect("cadence"))
        .with_execution_budget_ms(10_000)
        .with_initial_delay_ms(1_000)
        .with_deadline_ms(60_000);
    assert!(constraints.validate().is_ok());

    assert!(
        JobConstraints::default()
            .with_execution_budget_ms(0)
            .validate()
            .is_err()
    );
    assert!(
        JobConstraints::default()
            .with_initial_delay_ms(MAX_INITIAL_DELAY_MS + 1)
            .validate()
            .is_err()
    );
    assert!(
        JobConstraints::default()
            .with_retry(RetryBackoffPolicy::linear(0, 10, 2))
            .validate()
            .is_err()
    );
    assert!(
        JobConstraints::default()
            .with_retry(RetryBackoffPolicy::exponential(100, 50, 2))
            .validate()
            .is_err()
    );
}

#[test]
fn retry_backoff_is_saturating_and_integer() {
    let linear = RetryBackoffPolicy::linear(1_000, 3_500, 5);
    assert_eq!(linear.delay_for_attempt(1), 1_000);
    assert_eq!(linear.delay_for_attempt(3), 3_000);
    assert_eq!(linear.delay_for_attempt(4), 3_500);
    assert_eq!(linear.delay_for_attempt(u32::MAX), 3_500);
    assert_eq!(linear.max_attempts(), 5);
    assert!(linear.allows_another_attempt(4));
    assert!(!linear.allows_another_attempt(5));

    let exponential = RetryBackoffPolicy::exponential(100, 5_000, 10);
    assert_eq!(exponential.delay_for_attempt(1), 100);
    assert_eq!(exponential.delay_for_attempt(2), 200);
    assert_eq!(exponential.delay_for_attempt(6), 3_200);
    assert_eq!(exponential.delay_for_attempt(7), 5_000);
    assert_eq!(exponential.delay_for_attempt(63), 5_000);
    assert_eq!(
        RetryBackoffPolicy::exponential(u64::MAX, u64::MAX, 2).delay_for_attempt(63),
        u64::MAX
    );
}

#[test]
fn registration_receipt_is_explicitly_advisory() {
    let clock = Arc::new(ManualClock::new(1_000));
    let provider = FakeJobProvider::new();
    let service = service_with(provider.clone(), Arc::clone(&clock));
    let constraints = JobConstraints::default().with_initial_delay_ms(5_000);
    let receipt = completed(service.register_job(
        &context(),
        key("advisory"),
        constraints,
        ExistingJobPolicy::KeepExisting,
    ));
    assert_eq!(receipt.status, JobStatus::PendingConstraints);
    assert_eq!(
        receipt.registration_disposition,
        RegistrationDisposition::Created
    );
    assert_eq!(
        receipt.guarantee,
        SchedulingGuarantee::AdvisoryOpportunistic
    );
    assert!(receipt.guarantee.is_advisory());
    assert!(receipt.is_advisory());
    assert_eq!(receipt.earliest_eligible_after_ms, 6_000);
    assert!(receipt.is_pending());
    assert!(receipt.unmet_constraints.iter().any(|unmet| {
        matches!(
            unmet,
            UnmetConstraint::InitialDelay {
                remaining_ms: 5_000
            }
        )
    }));
    assert_eq!(provider.schedule_request_count(), 0);

    let expedited = completed(service.schedule_job_expedited(
        &context(),
        key("expedited"),
        JobConstraints::default(),
        ExistingJobPolicy::KeepExisting,
    ));
    assert_eq!(
        expedited.guarantee,
        SchedulingGuarantee::ExpeditedBestEffort
    );
    assert!(expedited.guarantee.is_advisory());
    assert_eq!(provider.schedule_request_count(), 1);
}

#[test]
fn all_registration_policies_are_deterministic() {
    let clock = Arc::new(ManualClock::new(1_000));
    let provider = FakeJobProvider::new();
    let service = service_with(provider.clone(), Arc::clone(&clock));
    let job = key("policy");
    let created = completed(service.register_job(
        &context(),
        job.clone(),
        JobConstraints::default(),
        ExistingJobPolicy::KeepExisting,
    ));
    assert_eq!(
        created.registration_disposition,
        RegistrationDisposition::Created
    );
    assert_eq!(created.status, JobStatus::Scheduled);

    let kept = completed(service.register_job(
        &context(),
        job.clone(),
        JobConstraints::default().with_execution_budget_ms(7_000),
        ExistingJobPolicy::KeepExisting,
    ));
    assert_eq!(
        kept.registration_disposition,
        RegistrationDisposition::KeptExisting
    );
    assert_eq!(
        service
            .job_record(&job)
            .expect("record")
            .constraints
            .execution_budget_ms,
        60_000
    );

    let conflict = service
        .register_job(
            &context(),
            job.clone(),
            JobConstraints::default(),
            ExistingJobPolicy::RejectConflict,
        )
        .expect_err("conflict");
    assert_eq!(conflict.code, ErrorCode::InvalidInput);
    assert_eq!(conflict.retry, RetryAdvice::ModifyRequest);

    let updated = completed(
        service.register_job(
            &context(),
            job.clone(),
            JobConstraints::default()
                .requiring_charging(true)
                .with_execution_budget_ms(7_000),
            ExistingJobPolicy::UpdateConstraints,
        ),
    );
    assert_eq!(
        updated.registration_disposition,
        RegistrationDisposition::UpdatedConstraints
    );
    assert_eq!(updated.status, JobStatus::PendingConstraints);
    let record = service.job_record(&job).expect("record");
    assert_eq!(record.constraints.execution_budget_ms, 7_000);
    assert_eq!(record.run_epoch, created.run_epoch);

    let replaced = completed(service.register_job(
        &context(),
        job.clone(),
        JobConstraints::default(),
        ExistingJobPolicy::Replace,
    ));
    assert_eq!(
        replaced.registration_disposition,
        RegistrationDisposition::Replaced
    );
    assert!(replaced.run_epoch > updated.run_epoch);
    assert_eq!(replaced.status, JobStatus::Scheduled);
    assert!(provider.schedule_request_count() >= 2);
}

#[test]
fn power_and_network_constraints_gate_and_relax() {
    let clock = Arc::new(ManualClock::new(1_000));
    let provider = FakeJobProvider::new();
    let service = service_with(provider.clone(), Arc::clone(&clock));
    completed(service.apply_device_state(false, true, &context()));
    let job = key("constrained");
    let constraints = JobConstraints::default()
        .with_network(JobNetworkRequirement::Unmetered)
        .requiring_charging(true)
        .disallowing_battery_saver(true)
        .disallowing_critical_thermal(true)
        .requiring_device_idle(true)
        .requiring_storage_not_low(true);

    let receipt = completed(service.register_job(
        &context(),
        job.clone(),
        constraints,
        ExistingJobPolicy::KeepExisting,
    ));
    assert_eq!(receipt.status, JobStatus::PendingConstraints);
    let reasons = receipt.unmet_constraints;
    assert!(
        reasons
            .iter()
            .any(|unmet| matches!(unmet, UnmetConstraint::NetworkUnknown))
    );
    assert!(
        reasons
            .iter()
            .any(|unmet| matches!(unmet, UnmetConstraint::PowerUnknown))
    );
    assert!(
        reasons
            .iter()
            .any(|unmet| matches!(unmet, UnmetConstraint::DeviceNotIdle))
    );
    assert!(
        reasons
            .iter()
            .any(|unmet| matches!(unmet, UnmetConstraint::StorageLow))
    );
    assert_eq!(provider.schedule_request_count(), 0);

    let power = observed_power(
        ChargingState::Discharging,
        LowPowerMode::Enabled,
        ThermalCondition::Critical,
    );
    let update = completed(service.apply_power_snapshot(power, &context()));
    assert!(update.pending >= 1);
    let reasons = service.unmet_constraints(&job);
    assert!(
        reasons
            .iter()
            .any(|unmet| matches!(unmet, UnmetConstraint::RequiresCharging))
    );
    assert!(
        reasons
            .iter()
            .any(|unmet| matches!(unmet, UnmetConstraint::BatterySaverEnabled))
    );
    assert!(
        reasons
            .iter()
            .any(|unmet| matches!(unmet, UnmetConstraint::CriticalThermal))
    );

    completed(service.apply_power_snapshot(
        observed_power(
            ChargingState::Charging,
            LowPowerMode::Disabled,
            ThermalCondition::Nominal,
        ),
        &context(),
    ));
    completed(service.apply_network_snapshot(metered_network(), &context()));
    completed(service.apply_device_state(true, false, &context()));
    let reasons = service.unmet_constraints(&job);
    assert!(
        reasons
            .iter()
            .any(|unmet| matches!(unmet, UnmetConstraint::MeteredConnection))
    );
    assert_eq!(provider.schedule_request_count(), 0);

    let update = completed(service.apply_network_snapshot(observed_network(), &context()));
    assert!(update.scheduled >= 1);
    assert_eq!(provider.schedule_request_count(), 1);
    assert_eq!(service.job_status(&job), Some(JobStatus::Scheduled));

    // Roaming and data saver remain explicit unmet reasons.
    let roaming = roaming_network().with_data_saver(DataSaverMode::Constrained);
    completed(service.apply_network_snapshot(roaming, &context()));
    assert_eq!(
        service.job_status(&job),
        Some(JobStatus::PendingConstraints)
    );
    let reasons = service.unmet_constraints(&job);
    assert!(
        reasons
            .iter()
            .any(|unmet| matches!(unmet, UnmetConstraint::MeteredConnection))
    );
    assert!(
        reasons
            .iter()
            .any(|unmet| matches!(unmet, UnmetConstraint::DataSaverEnabled))
    );

    let roaming_job = key("roaming");
    let receipt = completed(service.register_job(
        &context(),
        roaming_job.clone(),
        JobConstraints::default().with_network(JobNetworkRequirement::NotRoaming),
        ExistingJobPolicy::KeepExisting,
    ));
    assert_eq!(receipt.status, JobStatus::PendingConstraints);
    assert!(
        receipt
            .unmet_constraints
            .iter()
            .any(|unmet| matches!(unmet, UnmetConstraint::RoamingConnection))
    );
}

#[test]
fn captive_portal_and_permission_gate() {
    let clock = Arc::new(ManualClock::new(1_000));
    let provider = FakeJobProvider::new();
    let service = service_with(provider.clone(), Arc::clone(&clock));
    let job = key("portal");
    completed(service.apply_network_snapshot(observed_network(), &context()));
    let constraints = JobConstraints::default()
        .with_network(JobNetworkRequirement::Connected)
        .requiring_permission(PermissionKind::Notifications);
    let receipt = completed(service.register_job(
        &context(),
        job.clone(),
        constraints,
        ExistingJobPolicy::KeepExisting,
    ));
    assert_eq!(receipt.status, JobStatus::PendingConstraints);
    assert!(
        receipt
            .unmet_constraints
            .iter()
            .any(|unmet| matches!(unmet, UnmetConstraint::PermissionNotDetermined { .. }))
    );

    let portal = observed_network().with_captive_portal(CaptivePortalStatus::detected(None));
    completed(service.apply_network_snapshot(portal, &context()));
    completed(service.apply_permission_kind(
        PermissionKind::Notifications,
        PermissionStatus::Granted,
        &context(),
    ));
    let reasons = service.unmet_constraints(&job);
    assert!(
        reasons
            .iter()
            .any(|unmet| matches!(unmet, UnmetConstraint::CaptivePortal))
    );

    completed(service.apply_network_snapshot(observed_network(), &context()));
    assert_eq!(service.job_status(&job), Some(JobStatus::Scheduled));

    completed(service.apply_permission_kind(
        PermissionKind::Notifications,
        PermissionStatus::Denied {
            can_request_again: false,
        },
        &context(),
    ));
    assert_eq!(
        service.job_status(&job),
        Some(JobStatus::PendingConstraints)
    );
    assert!(
        service
            .unmet_constraints(&job)
            .iter()
            .any(|unmet| matches!(unmet, UnmetConstraint::PermissionDenied { .. }))
    );
}

#[test]
fn overdue_deadline_expires_pending_and_scheduled_jobs() {
    let clock = Arc::new(ManualClock::new(1_000));
    let provider = FakeJobProvider::new();
    let service = service_with(provider, Arc::clone(&clock));
    let job = key("deadline");
    let constraints = JobConstraints::default()
        .with_initial_delay_ms(10_000)
        .with_deadline_ms(5_000);
    let receipt = completed(service.register_job(
        &context(),
        job.clone(),
        constraints,
        ExistingJobPolicy::KeepExisting,
    ));
    assert_eq!(receipt.status, JobStatus::PendingConstraints);
    clock.advance(6_000);
    let update = completed(service.tick(&context()));
    assert!(update.expired >= 1);
    assert_eq!(service.job_status(&job), Some(JobStatus::ExpiredBudget));
    assert!(
        service
            .job_record(&job)
            .expect("record")
            .last_failure
            .is_none()
    );
}

#[test]
fn lifecycle_suspension_blocks_new_dispatch_until_resume() {
    let clock = Arc::new(ManualClock::new(1_000));
    let provider = FakeJobProvider::new();
    let service = service_with(provider.clone(), Arc::clone(&clock));
    let lifecycle = LifecycleState::from_parts(
        AppPhase::Suspended,
        Vec::new(),
        gpui_lifecycle::SessionLockState::Unlocked,
        gpui_lifecycle::MemoryPressure::Normal,
    );
    completed(service.apply_lifecycle_state(&lifecycle, &context()));
    let job = key("suspended");
    let receipt = completed(service.register_job(
        &context(),
        job.clone(),
        JobConstraints::default(),
        ExistingJobPolicy::KeepExisting,
    ));
    assert_eq!(receipt.status, JobStatus::PendingConstraints);
    assert!(
        receipt
            .unmet_constraints
            .iter()
            .any(|unmet| matches!(unmet, UnmetConstraint::LifecycleSuspended))
    );
    assert_eq!(provider.schedule_request_count(), 0);

    let resumed = LifecycleState::from_parts(
        AppPhase::ForegroundActive,
        Vec::new(),
        gpui_lifecycle::SessionLockState::Unlocked,
        gpui_lifecycle::MemoryPressure::Normal,
    );
    completed(service.apply_lifecycle_state(&resumed, &context()));
    assert_eq!(service.job_status(&job), Some(JobStatus::Scheduled));
    assert_eq!(provider.schedule_request_count(), 1);
}

#[test]
fn pending_job_never_reports_an_exact_start_time() {
    let clock = Arc::new(ManualClock::new(1_000));
    let provider = FakeJobProvider::new();
    let service = service_with(provider, Arc::clone(&clock));
    let job = key("advisory-only");
    let receipt = completed(service.register_job(
        &context(),
        job.clone(),
        JobConstraints::default(),
        ExistingJobPolicy::KeepExisting,
    ));
    assert_eq!(receipt.status, JobStatus::Scheduled);
    let record = service.job_record(&job).expect("record");
    assert!(record.earliest_eligible_at_ms <= service.clock_now_ms());
    assert_eq!(record.guarantee, SchedulingGuarantee::AdvisoryOpportunistic);
}

#[test]
fn update_constraints_cancels_and_reschedules_provider_admission() {
    let clock = Arc::new(ManualClock::new(1_000));
    let provider = FakeJobProvider::new();
    let service = service_with(provider.clone(), Arc::clone(&clock));
    let job = key("constraint-update");
    let created = completed(service.register_job(
        &context(),
        job.clone(),
        JobConstraints::default(),
        ExistingJobPolicy::KeepExisting,
    ));
    assert_eq!(created.status, JobStatus::Scheduled);
    assert_eq!(provider.schedule_request_count(), 1);

    let subscription = service.subscribe();
    let pending = completed(service.register_job(
        &context(),
        job.clone(),
        JobConstraints::default().requiring_charging(true),
        ExistingJobPolicy::UpdateConstraints,
    ));
    assert_eq!(pending.status, JobStatus::PendingConstraints);
    assert_eq!(provider.cancel_call_count(), 1);
    assert_eq!(provider.schedule_request_count(), 1);
    let events = values(drain(&subscription));
    assert!(events.iter().any(|event| matches!(
        event,
        JobsEvent::JobStateChanged {
            previous: JobStatus::Scheduled,
            status: JobStatus::PendingConstraints,
            ..
        }
    )));

    let rescheduled = completed(service.register_job(
        &context(),
        job.clone(),
        JobConstraints::default(),
        ExistingJobPolicy::UpdateConstraints,
    ));
    assert_eq!(rescheduled.status, JobStatus::Scheduled);
    assert_eq!(provider.schedule_request_count(), 2);
    assert_eq!(service.active_token(&job), None);
    let events = values(drain(&subscription));
    assert!(events.iter().any(|event| matches!(
        event,
        JobsEvent::JobScheduled { job_key, .. } if job_key == &job
    )));
    assert!(events.iter().any(|event| matches!(
        event,
        JobsEvent::JobStateChanged {
            previous: JobStatus::PendingConstraints,
            status: JobStatus::Scheduled,
            ..
        }
    )));
}
