//! Fencing, duplicate wakeups, cancellation, retry budgets and cooperative
//! preemption.

mod support;

use gpui_jobs::*;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use support::*;

fn started(service: &JobsService, job: &JobKey) -> FencingToken {
    match completed(service.handle_wakeup(job, &context())) {
        WakeupOutcome::Started(token) => token,
        other => panic!("expected started, got {other:?}"),
    }
}

fn assert_stale<T: std::fmt::Debug>(result: ServiceResult<T>, label: &str) {
    let failure = result.expect_err(label);
    assert_eq!(failure.code, ErrorCode::ContextExpired, "{label}");
}

#[test]
fn duplicate_os_wakeups_are_coalesced_without_restarting_the_attempt() {
    let clock = Arc::new(ManualClock::new(1_000));
    let provider = FakeJobProvider::new();
    let service = service_with(provider.clone(), Arc::clone(&clock));
    let job = key("duplicate");
    completed(service.register_job(
        &context(),
        job.clone(),
        JobConstraints::default(),
        ExistingJobPolicy::KeepExisting,
    ));
    let token = started(&service, &job);
    let subscription = service.subscribe();
    provider.fire_wakeup(&job);
    provider.fire_wakeup(&job);
    assert_eq!(service.job_status(&job), Some(JobStatus::Running));
    assert_eq!(service.active_token(&job).expect("token"), token);
    let events = values(drain(&subscription));
    assert_eq!(
        events
            .iter()
            .filter(|event| matches!(event, JobsEvent::DuplicateWakeupIgnored { .. }))
            .count(),
        2
    );
    assert!(
        !events
            .iter()
            .any(|event| matches!(event, JobsEvent::AttemptStarted { .. }))
    );
    assert_eq!(
        service.job_record(&job).expect("record").attempt,
        token.attempt
    );
    completed(service.complete_job(&token, &context()));
    assert_eq!(service.job_status(&job), Some(JobStatus::Succeeded));
}

#[test]
fn cancellation_is_terminal_and_fences_late_work() {
    let clock = Arc::new(ManualClock::new(1_000));
    let provider = FakeJobProvider::new();
    let service = service_with(provider.clone(), Arc::clone(&clock));
    let job = key("cancel");
    completed(service.register_job(
        &context(),
        job.clone(),
        JobConstraints::default(),
        ExistingJobPolicy::KeepExisting,
    ));
    let token = started(&service, &job);
    let subscription = service.subscribe();
    let cancelled = completed(service.cancel_job(&job, &context()));
    assert_eq!(cancelled.status, JobStatus::Cancelled);
    assert!(cancelled.run_epoch > token.run_epoch);
    assert_eq!(provider.cancel_call_count(), 1);
    assert_eq!(service.active_token(&job), None);
    let events = values(drain(&subscription));
    assert!(
        events
            .iter()
            .any(|event| matches!(event, JobsEvent::JobCancelled { .. }))
    );
    assert!(events.iter().any(|event| matches!(
        event,
        JobsEvent::PreemptionRequested {
            reason: PreemptReason::Cancelled,
            ..
        }
    )));

    assert_stale(
        service.complete_job(&token, &context()),
        "completion after cancel",
    );
    assert_stale(
        service.save_checkpoint(&token, b"late".to_vec(), JobProgress::default(), &context()),
        "checkpoint after cancel",
    );
    assert_stale(
        service.report_progress(&token, JobProgress::default(), &context()),
        "progress after cancel",
    );
    assert_stale(
        service.heartbeat(&token, &context()),
        "heartbeat after cancel",
    );
    let cancelled_again = completed(service.cancel_job(&job, &context()));
    assert_eq!(cancelled_again.status, JobStatus::Cancelled);
    assert_eq!(provider.cancel_call_count(), 1);
}

#[test]
fn constraint_loss_preempts_with_final_checkpoint_and_resumes() {
    let clock = Arc::new(ManualClock::new(1_000));
    let provider = FakeJobProvider::new();
    let service = service_with(provider.clone(), Arc::clone(&clock));
    completed(service.apply_power_snapshot(
        observed_power(
            ChargingState::Charging,
            LowPowerMode::Disabled,
            ThermalCondition::Nominal,
        ),
        &context(),
    ));
    let job = key("constraint-loss");
    completed(service.register_job(
        &context(),
        job.clone(),
        JobConstraints::default().requiring_charging(true),
        ExistingJobPolicy::KeepExisting,
    ));
    let token = started(&service, &job);
    completed(service.save_checkpoint(
        &token,
        b"progress".to_vec(),
        JobProgress::from_basis_points(2_000).expect("progress"),
        &context(),
    ));
    let subscription = service.subscribe();
    completed(service.apply_power_snapshot(
        observed_power(
            ChargingState::Discharging,
            LowPowerMode::Disabled,
            ThermalCondition::Nominal,
        ),
        &context(),
    ));
    let events = values(drain(&subscription));
    assert!(events.iter().any(|event| matches!(
        event,
        JobsEvent::PreemptionRequested {
            reason: PreemptReason::ConstraintsLost,
            ..
        }
    )));
    assert_eq!(service.job_status(&job), Some(JobStatus::Running));
    assert_eq!(service.active_token(&job).expect("active"), token);

    // A final checkpoint is the one mutation allowed during the handoff.
    let final_checkpoint = completed(service.save_checkpoint(
        &token,
        b"final".to_vec(),
        JobProgress::from_basis_points(2_500).expect("progress"),
        &context(),
    ));
    assert_eq!(final_checkpoint.sequence, 2);
    assert_stale(
        service.report_progress(&token, JobProgress::default(), &context()),
        "progress during preemption",
    );

    let acknowledged = completed(service.ack_preemption(&token, &context()));
    assert_eq!(acknowledged.status, JobStatus::PendingConstraints);
    assert_eq!(acknowledged.run_epoch, token.run_epoch + 1);
    assert!(
        acknowledged
            .unmet_constraints
            .iter()
            .any(|unmet| matches!(unmet, UnmetConstraint::RequiresCharging))
    );
    assert_stale(
        service.complete_job(&token, &context()),
        "completion after preemption",
    );
    assert_stale(
        service.save_checkpoint(
            &token,
            b"stale".to_vec(),
            JobProgress::default(),
            &context(),
        ),
        "checkpoint after preemption",
    );

    completed(service.apply_power_snapshot(
        observed_power(
            ChargingState::Charging,
            LowPowerMode::Disabled,
            ThermalCondition::Nominal,
        ),
        &context(),
    ));
    assert_eq!(service.job_status(&job), Some(JobStatus::Scheduled));
    let resumed = started(&service, &job);
    assert!(resumed.run_epoch > token.run_epoch);
    assert_eq!(
        service.checkpoint(&job).expect("checkpoint").payload,
        b"final"
    );
    assert_eq!(provider.schedule_request_count(), 2);
}

#[test]
fn lifecycle_suspension_preempts_and_resume_schedules() {
    let clock = Arc::new(ManualClock::new(1_000));
    let service = service_with(FakeJobProvider::new(), Arc::clone(&clock));
    let job = key("lifecycle");
    completed(service.register_job(
        &context(),
        job.clone(),
        JobConstraints::default(),
        ExistingJobPolicy::KeepExisting,
    ));
    let token = started(&service, &job);
    let suspended = LifecycleState::from_parts(
        AppPhase::Suspended,
        Vec::new(),
        gpui_lifecycle::SessionLockState::Unlocked,
        gpui_lifecycle::MemoryPressure::Normal,
    );
    let subscription = service.subscribe();
    completed(service.apply_lifecycle_state(&suspended, &context()));
    assert!(values(drain(&subscription)).iter().any(|event| matches!(
        event,
        JobsEvent::PreemptionRequested {
            reason: PreemptReason::LifecycleSuspending,
            ..
        }
    )));
    assert_eq!(
        completed(service.ack_preemption(&token, &context())).status,
        JobStatus::PendingConstraints
    );

    let resumed = LifecycleState::from_parts(
        AppPhase::ForegroundActive,
        Vec::new(),
        gpui_lifecycle::SessionLockState::Unlocked,
        gpui_lifecycle::MemoryPressure::Normal,
    );
    completed(service.apply_lifecycle_state(&resumed, &context()));
    assert_eq!(service.job_status(&job), Some(JobStatus::Scheduled));
}

#[test]
fn revoked_permission_preempts_with_a_typed_reason() {
    let clock = Arc::new(ManualClock::new(1_000));
    let service = service_with(FakeJobProvider::new(), Arc::clone(&clock));
    completed(service.apply_permission_kind(
        PermissionKind::Camera,
        PermissionStatus::Granted,
        &context(),
    ));
    let job = key("permission");
    completed(service.register_job(
        &context(),
        job.clone(),
        JobConstraints::default().requiring_permission(PermissionKind::Camera),
        ExistingJobPolicy::KeepExisting,
    ));
    let token = started(&service, &job);
    let subscription = service.subscribe();
    completed(service.apply_permission_kind(
        PermissionKind::Camera,
        PermissionStatus::Denied {
            can_request_again: false,
        },
        &context(),
    ));
    assert!(values(drain(&subscription)).iter().any(|event| matches!(
        event,
        JobsEvent::PreemptionRequested {
            reason: PreemptReason::PermissionRevoked,
            ..
        }
    )));
    assert_eq!(
        completed(service.ack_preemption(&token, &context())).status,
        JobStatus::PendingConstraints
    );
    assert!(
        service
            .unmet_constraints(&job)
            .iter()
            .any(|unmet| matches!(unmet, UnmetConstraint::PermissionDenied { .. }))
    );
}

#[test]
fn execution_budget_preempts_and_expires() {
    let clock = Arc::new(ManualClock::new(1_000));
    let service = service_with(FakeJobProvider::new(), Arc::clone(&clock));
    let job = key("execution-budget");
    completed(service.register_job(
        &context(),
        job.clone(),
        JobConstraints::default().with_execution_budget_ms(1_000),
        ExistingJobPolicy::KeepExisting,
    ));
    let token = started(&service, &job);
    let receipt = completed(service.heartbeat(&token, &context()));
    assert_eq!(receipt.remaining_ms, 1_000);
    clock.advance(1_001);
    let subscription = service.subscribe();
    let update = completed(service.tick(&context()));
    assert_eq!(update.preempted, 0);
    assert!(values(drain(&subscription)).iter().any(|event| matches!(
        event,
        JobsEvent::PreemptionRequested {
            reason: PreemptReason::BudgetExpired,
            ..
        }
    )));
    let record = completed(service.ack_preemption(&token, &context()));
    assert_eq!(record.status, JobStatus::ExpiredBudget);
    assert_eq!(record.run_epoch, token.run_epoch + 1);
    assert_stale(
        service.complete_job(&token, &context()),
        "completion after budget expiry",
    );
}

#[test]
fn heartbeat_rejects_an_overdue_attempt_budget() {
    let clock = Arc::new(ManualClock::new(1_000));
    let service = service_with(FakeJobProvider::new(), Arc::clone(&clock));
    let job = key("heartbeat-budget");
    completed(service.register_job(
        &context(),
        job.clone(),
        JobConstraints::default().with_execution_budget_ms(500),
        ExistingJobPolicy::KeepExisting,
    ));
    let token = started(&service, &job);
    clock.advance(501);
    let failure = service.heartbeat(&token, &context()).expect_err("overdue");
    assert_eq!(failure.code, ErrorCode::TimedOut);
    assert_eq!(
        completed(service.ack_preemption(&token, &context())).status,
        JobStatus::ExpiredBudget
    );
}

#[test]
fn deadline_preempts_a_running_attempt() {
    let clock = Arc::new(ManualClock::new(1_000));
    let service = service_with(FakeJobProvider::new(), Arc::clone(&clock));
    let job = key("deadline-run");
    completed(service.register_job(
        &context(),
        job.clone(),
        JobConstraints::default().with_deadline_ms(500),
        ExistingJobPolicy::KeepExisting,
    ));
    let token = started(&service, &job);
    clock.advance(600);
    completed(service.tick(&context()));
    let record = completed(service.ack_preemption(&token, &context()));
    assert_eq!(record.status, JobStatus::ExpiredBudget);
}

#[test]
fn retry_backoff_respects_attempts_and_then_expires() {
    let clock = Arc::new(ManualClock::new(1_000));
    let provider = FakeJobProvider::new();
    let service = service_with(provider.clone(), Arc::clone(&clock));
    let job = key("retry");
    completed(service.register_job(
        &context(),
        job.clone(),
        JobConstraints::default().with_retry(RetryBackoffPolicy::linear(100, 100, 2)),
        ExistingJobPolicy::KeepExisting,
    ));
    let first = started(&service, &job);
    let after_failure =
        completed(service.fail_job(&first, ErrorCode::PlatformFailure, true, &context()));
    assert_eq!(after_failure.status, JobStatus::RetryBackoff);
    assert_eq!(after_failure.next_attempt_at_ms, 1_100);
    assert_eq!(after_failure.unmet_constraints.len(), 1);
    match service.handle_wakeup(&job, &context()).expect("deferred") {
        Completion::Completed(WakeupOutcome::Deferred { unmet }) => {
            assert!(
                unmet.iter().any(|unmet| matches!(
                    unmet,
                    UnmetConstraint::RetryBackoff { remaining_ms: 100 }
                ))
            );
        }
        other => panic!("expected deferred, got {other:?}"),
    }
    clock.advance(100);
    completed(service.tick(&context()));
    assert_eq!(service.job_status(&job), Some(JobStatus::Scheduled));
    let second = started(&service, &job);
    assert_eq!(second.run_epoch, first.run_epoch + 1);
    assert_eq!(second.attempt, 2);
    assert_eq!(second.idempotency_token, first.idempotency_token);
    let terminal =
        completed(service.fail_job(&second, ErrorCode::PlatformFailure, true, &context()));
    assert_eq!(terminal.status, JobStatus::ExpiredBudget);
    assert_eq!(
        terminal.last_failure,
        Some(JobFailureReason::PlatformFailure)
    );
    assert_eq!(provider.schedule_request_count(), 2);
}

#[test]
fn non_retryable_failure_is_terminal() {
    let clock = Arc::new(ManualClock::new(1_000));
    let service = service_with(FakeJobProvider::new(), Arc::clone(&clock));
    let job = key("fatal");
    completed(service.register_job(
        &context(),
        job.clone(),
        JobConstraints::default(),
        ExistingJobPolicy::KeepExisting,
    ));
    let token = started(&service, &job);
    let record = completed(service.fail_job(&token, ErrorCode::InvalidInput, false, &context()));
    assert_eq!(record.status, JobStatus::Failed);
    assert_eq!(record.last_failure, Some(JobFailureReason::InvalidInput));
    assert_stale(
        service.complete_job(&token, &context()),
        "completion after failure",
    );
}

#[test]
fn unacknowledged_preemption_is_finalized_after_the_grace_period() {
    let clock = Arc::new(ManualClock::new(1_000));
    let service = service_with(FakeJobProvider::new(), Arc::clone(&clock));
    completed(service.apply_power_snapshot(
        observed_power(
            ChargingState::Charging,
            LowPowerMode::Disabled,
            ThermalCondition::Nominal,
        ),
        &context(),
    ));
    let job = key("grace");
    completed(service.register_job(
        &context(),
        job.clone(),
        JobConstraints::default().requiring_charging(true),
        ExistingJobPolicy::KeepExisting,
    ));
    let token = started(&service, &job);
    completed(service.apply_power_snapshot(
        observed_power(
            ChargingState::Discharging,
            LowPowerMode::Disabled,
            ThermalCondition::Nominal,
        ),
        &context(),
    ));
    clock.advance(PREEMPTION_GRACE_MS);
    let update = completed(service.tick(&context()));
    assert_eq!(update.preempted, 1);
    assert_eq!(
        service.job_status(&job),
        Some(JobStatus::PendingConstraints)
    );
    assert_eq!(service.active_token(&job), None);
    assert_stale(
        service.complete_job(&token, &context()),
        "completion after grace",
    );
}

#[test]
fn preemption_callbacks_are_invoked_outside_locks_and_may_reenter() {
    let clock = Arc::new(ManualClock::new(1_000));
    let service = service_with(FakeJobProvider::new(), Arc::clone(&clock));
    completed(service.apply_power_snapshot(
        observed_power(
            ChargingState::Charging,
            LowPowerMode::Disabled,
            ThermalCondition::Nominal,
        ),
        &context(),
    ));
    let job = key("callback");
    completed(service.register_job(
        &context(),
        job.clone(),
        JobConstraints::default().requiring_charging(true),
        ExistingJobPolicy::KeepExisting,
    ));
    let calls = Arc::new(AtomicUsize::new(0));
    let observed = Arc::clone(&calls);
    let reentrant_service = service.clone();
    let _subscription = service.on_preemption(move |_signal| {
        assert!(!reentrant_service.list_jobs().is_empty());
        let _second = reentrant_service.on_preemption(|_| {});
        observed.fetch_add(1, Ordering::SeqCst);
    });
    let _token = started(&service, &job);
    completed(service.apply_power_snapshot(
        observed_power(
            ChargingState::Discharging,
            LowPowerMode::Disabled,
            ThermalCondition::Nominal,
        ),
        &context(),
    ));
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[cfg(feature = "test-support")]
#[test]
fn poisoned_state_lock_is_recovered_in_drop_paths() {
    let clock = Arc::new(ManualClock::new(1_000));
    let service = service_with(FakeJobProvider::new(), Arc::clone(&clock));
    let job = key("poison");
    completed(service.register_job(
        &context(),
        job.clone(),
        JobConstraints::default(),
        ExistingJobPolicy::KeepExisting,
    ));
    let _token = started(&service, &job);
    let subscription = service.on_preemption(|_| {});
    service.poison_state_lock_for_test();
    drop(subscription);
    assert_eq!(service.job_status(&job), Some(JobStatus::Running));
    completed(service.cancel_job(&job, &context()));
    assert_eq!(service.job_status(&job), Some(JobStatus::Cancelled));
}

#[test]
fn replace_running_job_fences_admission_and_clears_active_and_checkpoint() {
    let clock = Arc::new(ManualClock::new(1_000));
    let provider = FakeJobProvider::new();
    let service = service_with(provider.clone(), clock);
    let job = key("replace-running");
    completed(service.register_job(
        &context(),
        job.clone(),
        JobConstraints::default(),
        ExistingJobPolicy::KeepExisting,
    ));
    let old = started(&service, &job);
    completed(service.save_checkpoint(
        &old,
        b"old".to_vec(),
        JobProgress::from_basis_points(2_000).expect("progress"),
        &context(),
    ));
    let callback_calls = Arc::new(AtomicUsize::new(0));
    let observed = Arc::clone(&callback_calls);
    let _callback = service.on_preemption(move |signal| {
        assert_eq!(signal.reason, PreemptReason::Cancelled);
        observed.fetch_add(1, Ordering::SeqCst);
    });
    let subscription = service.subscribe();
    let receipt = completed(service.register_job(
        &context(),
        job.clone(),
        JobConstraints::default(),
        ExistingJobPolicy::Replace,
    ));
    assert_eq!(
        receipt.registration_disposition,
        RegistrationDisposition::Replaced
    );
    assert!(receipt.run_epoch > old.run_epoch);
    assert_eq!(service.active_token(&job), None);
    assert!(service.checkpoint(&job).is_none());
    assert_eq!(provider.cancel_call_count(), 1);
    assert_eq!(callback_calls.load(Ordering::SeqCst), 1);
    let events = values(drain(&subscription));
    assert!(events.iter().any(|event| matches!(
        event,
        JobsEvent::PreemptionRequested {
            token,
            reason: PreemptReason::Cancelled,
        } if token == &old
    )));
    assert_stale(
        service.complete_job(&old, &context()),
        "old completion after replace",
    );
    assert_stale(
        service.save_checkpoint(&old, b"late".to_vec(), JobProgress::default(), &context()),
        "old checkpoint after replace",
    );
    let fresh = started(&service, &job);
    assert!(fresh.run_epoch > receipt.run_epoch);
    assert_eq!(service.job_status(&job), Some(JobStatus::Running));
    completed(service.complete_job(&fresh, &context()));
    assert_eq!(service.job_status(&job), Some(JobStatus::Succeeded));
}

#[test]
fn preemption_handoff_allows_only_one_final_checkpoint() {
    let clock = Arc::new(ManualClock::new(1_000));
    let service = service_with(FakeJobProvider::new(), Arc::clone(&clock));
    completed(service.apply_power_snapshot(
        observed_power(
            ChargingState::Charging,
            LowPowerMode::Disabled,
            ThermalCondition::Nominal,
        ),
        &context(),
    ));
    let job = key("one-final-checkpoint");
    completed(service.register_job(
        &context(),
        job.clone(),
        JobConstraints::default().requiring_charging(true),
        ExistingJobPolicy::KeepExisting,
    ));
    let token = started(&service, &job);
    completed(service.apply_power_snapshot(
        observed_power(
            ChargingState::Discharging,
            LowPowerMode::Disabled,
            ThermalCondition::Nominal,
        ),
        &context(),
    ));
    let first = completed(service.save_checkpoint(
        &token,
        b"final".to_vec(),
        JobProgress::from_basis_points(9_000).expect("progress"),
        &context(),
    ));
    assert_eq!(first.sequence, 1);
    let second = service
        .save_checkpoint(
            &token,
            b"too-late".to_vec(),
            JobProgress::from_basis_points(9_500).expect("progress"),
            &context(),
        )
        .expect_err("only one final checkpoint is allowed");
    assert_eq!(second.code, ErrorCode::ContextExpired);
    assert_eq!(service.checkpoint(&job).expect("checkpoint").sequence, 1);
    assert_eq!(
        service.checkpoint(&job).expect("checkpoint").payload,
        b"final"
    );
}

struct FailAtWrite {
    writes: AtomicUsize,
    fail_at: AtomicUsize,
}

impl FailAtWrite {
    fn new() -> Self {
        Self {
            writes: AtomicUsize::new(0),
            fail_at: AtomicUsize::new(0),
        }
    }

    fn arm(&self, fail_at: usize) {
        self.writes.store(0, Ordering::SeqCst);
        self.fail_at.store(fail_at, Ordering::SeqCst);
    }
}

impl JobsStorageHook for FailAtWrite {
    fn before_write(&self, _file: &str) -> std::io::Result<()> {
        let write = self.writes.fetch_add(1, Ordering::SeqCst) + 1;
        if write == self.fail_at.load(Ordering::SeqCst) {
            Err(std::io::Error::from(std::io::ErrorKind::StorageFull))
        } else {
            Ok(())
        }
    }
}

#[test]
fn checkpoint_and_progress_write_failures_do_not_mutate_memory() {
    let clock = Arc::new(ManualClock::new(1_000));
    let provider = FakeJobProvider::new();
    let hook = Arc::new(FailAtWrite::new());
    let service = JobsService::in_memory_with_options(
        Arc::new(provider),
        Arc::clone(&clock) as Arc<dyn JobsClock>,
        DEFAULT_JOBS_EVENT_CAPACITY,
        hook.clone(),
    )
    .expect("service");
    let job = key("transactional-writes");
    completed(service.register_job(
        &context(),
        job.clone(),
        JobConstraints::default(),
        ExistingJobPolicy::KeepExisting,
    ));
    let token = started(&service, &job);
    let baseline = service.job_record(&job).expect("record");
    assert_eq!(baseline.progress.basis_points(), 0);

    hook.arm(1);
    let progress_failure = service
        .report_progress(
            &token,
            JobProgress::from_basis_points(5_000).expect("progress"),
            &context(),
        )
        .expect_err("progress persistence fails");
    assert_eq!(progress_failure.code, ErrorCode::PlatformFailure);
    let after_progress = service.job_record(&job).expect("record");
    assert_eq!(after_progress.progress, baseline.progress);
    assert_eq!(after_progress.updated_at_ms, baseline.updated_at_ms);

    // The checkpoint envelope commits first; the record update is the second
    // write and fails. Neither in-memory map may be changed.
    hook.arm(2);
    let checkpoint_failure = service
        .save_checkpoint(
            &token,
            b"uncommitted".to_vec(),
            JobProgress::from_basis_points(7_000).expect("progress"),
            &context(),
        )
        .expect_err("record persistence fails");
    assert_eq!(checkpoint_failure.code, ErrorCode::PlatformFailure);
    assert!(service.checkpoint(&job).is_none());
    let after_checkpoint = service.job_record(&job).expect("record");
    assert!(after_checkpoint.checkpoint.is_none());
    assert_eq!(after_checkpoint.progress, baseline.progress);
}

#[test]
fn ack_reschedules_when_constraints_recover_before_handoff() {
    let clock = Arc::new(ManualClock::new(1_000));
    let provider = FakeJobProvider::new();
    let service = service_with(provider.clone(), Arc::clone(&clock));
    completed(service.apply_power_snapshot(
        observed_power(
            ChargingState::Charging,
            LowPowerMode::Disabled,
            ThermalCondition::Nominal,
        ),
        &context(),
    ));
    let job = key("ack-reschedule");
    completed(service.register_job(
        &context(),
        job.clone(),
        JobConstraints::default().requiring_charging(true),
        ExistingJobPolicy::KeepExisting,
    ));
    let token = started(&service, &job);
    completed(service.apply_power_snapshot(
        observed_power(
            ChargingState::Discharging,
            LowPowerMode::Disabled,
            ThermalCondition::Nominal,
        ),
        &context(),
    ));
    completed(service.apply_power_snapshot(
        observed_power(
            ChargingState::Charging,
            LowPowerMode::Disabled,
            ThermalCondition::Nominal,
        ),
        &context(),
    ));
    let record = completed(service.ack_preemption(&token, &context()));
    assert_eq!(record.status, JobStatus::Scheduled);
    assert!(record.unmet_constraints.is_empty());
    assert_eq!(provider.schedule_request_count(), 2);
    assert_eq!(service.active_token(&job), None);
}

#[test]
fn grace_finalization_reschedules_when_constraints_recover() {
    let clock = Arc::new(ManualClock::new(1_000));
    let provider = FakeJobProvider::new();
    let service = service_with(provider.clone(), Arc::clone(&clock));
    completed(service.apply_power_snapshot(
        observed_power(
            ChargingState::Charging,
            LowPowerMode::Disabled,
            ThermalCondition::Nominal,
        ),
        &context(),
    ));
    let job = key("grace-reschedule");
    completed(service.register_job(
        &context(),
        job.clone(),
        JobConstraints::default().requiring_charging(true),
        ExistingJobPolicy::KeepExisting,
    ));
    let _token = started(&service, &job);
    completed(service.apply_power_snapshot(
        observed_power(
            ChargingState::Discharging,
            LowPowerMode::Disabled,
            ThermalCondition::Nominal,
        ),
        &context(),
    ));
    completed(service.apply_power_snapshot(
        observed_power(
            ChargingState::Charging,
            LowPowerMode::Disabled,
            ThermalCondition::Nominal,
        ),
        &context(),
    ));
    clock.advance(PREEMPTION_GRACE_MS);
    completed(service.tick(&context()));
    assert_eq!(service.job_status(&job), Some(JobStatus::Scheduled));
    assert_eq!(provider.schedule_request_count(), 2);
    assert_eq!(service.active_token(&job), None);
}
