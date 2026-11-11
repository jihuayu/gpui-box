//! Deterministic crash reconciliation of jobs left `Running`.

mod support;

use gpui_jobs::*;
use std::sync::Arc;
use support::*;
use tempfile::TempDir;

fn open(app: &AppData, provider: FakeJobProvider, clock: Arc<ManualClock>) -> JobsService {
    JobsService::open_with_options(
        app,
        Arc::new(provider),
        clock,
        DEFAULT_JOBS_EVENT_CAPACITY,
        Arc::new(NoopJobsStorageHook),
    )
    .expect("service")
}

fn start(service: &JobsService, job: &JobKey) -> FencingToken {
    match completed(service.handle_wakeup(job, &context())) {
        WakeupOutcome::Started(token) => token,
        other => panic!("expected started, got {other:?}"),
    }
}

#[test]
fn running_job_reconciles_to_scheduled_and_preserves_checkpoint() {
    let temp = TempDir::new().expect("temp");
    let clock = Arc::new(ManualClock::new(1_000));
    let provider = FakeJobProvider::new();
    let job = key("restart-scheduled");
    let token;
    {
        let app = AppData::open(DirectoryRoots::under(temp.path())).expect("app");
        let service = open(&app, provider.clone(), Arc::clone(&clock));
        completed(service.register_job(
            &context(),
            job.clone(),
            JobConstraints::default(),
            ExistingJobPolicy::KeepExisting,
        ));
        token = start(&service, &job);
        completed(service.save_checkpoint(
            &token,
            b"crash-state".to_vec(),
            JobProgress::from_basis_points(4_000).expect("progress"),
            &context(),
        ));
    }
    {
        let app = AppData::open(DirectoryRoots::under(temp.path())).expect("app");
        let service = open(&app, provider.clone(), clock);
        let subscription = service.subscribe();
        let report = completed(service.reconcile_after_restart(&context()));
        assert_eq!(report.jobs_scanned, 1);
        assert_eq!(report.running_reconciled, 1);
        assert_eq!(report.rescheduled, 1);
        assert_eq!(report.expired_budget, 0);
        let record = service.job_record(&job).expect("record");
        assert_eq!(record.status, JobStatus::Scheduled);
        assert_eq!(record.run_epoch, token.run_epoch + 1);
        assert_eq!(record.checkpoint.expect("summary").sequence, 1);
        assert_eq!(
            service.checkpoint(&job).expect("checkpoint").payload,
            b"crash-state"
        );
        assert!(values(drain(&subscription)).iter().any(|event| matches!(
            event,
            JobsEvent::JobReconciled {
                previous_status: JobStatus::Running,
                status: JobStatus::Scheduled,
                previous_epoch,
                run_epoch,
                ..
            } if *previous_epoch == token.run_epoch && *run_epoch == token.run_epoch + 1
        )));
        // A zombie attempt from before the crash can never complete.
        assert_eq!(
            service
                .complete_job(&token, &context())
                .expect_err("stale fence")
                .code,
            ErrorCode::ContextExpired
        );
        assert_eq!(
            service
                .save_checkpoint(
                    &token,
                    b"zombie".to_vec(),
                    JobProgress::default(),
                    &context()
                )
                .expect_err("stale fence")
                .code,
            ErrorCode::ContextExpired
        );
        assert_eq!(provider.schedule_request_count(), 2);
    }
}

#[test]
fn running_job_with_future_wake_reconciles_to_retry_backoff() {
    let temp = TempDir::new().expect("temp");
    let clock = Arc::new(ManualClock::new(1_000));
    let provider = FakeJobProvider::new();
    let job = key("restart-backoff");
    let token;
    {
        let app = AppData::open(DirectoryRoots::under(temp.path())).expect("app");
        let service = open(&app, provider.clone(), Arc::clone(&clock));
        completed(service.register_job(
            &context(),
            job.clone(),
            JobConstraints::default(),
            ExistingJobPolicy::KeepExisting,
        ));
        token = start(&service, &job);
        completed(service.register_job(
            &context(),
            job.clone(),
            JobConstraints::default().with_initial_delay_ms(10_000),
            ExistingJobPolicy::UpdateConstraints,
        ));
        assert_eq!(service.job_status(&job), Some(JobStatus::Running));
    }
    {
        let app = AppData::open(DirectoryRoots::under(temp.path())).expect("app");
        let service = open(&app, provider, clock);
        let report = completed(service.reconcile_after_restart(&context()));
        assert_eq!(report.running_reconciled, 1);
        assert_eq!(report.retry_backoff, 1);
        assert_eq!(report.rescheduled, 0);
        let record = service.job_record(&job).expect("record");
        assert_eq!(record.status, JobStatus::RetryBackoff);
        assert_eq!(record.run_epoch, token.run_epoch + 1);
        assert!(
            record
                .unmet_constraints
                .iter()
                .any(|unmet| matches!(unmet, UnmetConstraint::InitialDelay { .. }))
        );
    }
}

#[test]
fn running_job_with_lost_network_reconciles_to_pending() {
    let temp = TempDir::new().expect("temp");
    let clock = Arc::new(ManualClock::new(1_000));
    let provider = FakeJobProvider::new();
    let job = key("restart-pending");
    {
        let app = AppData::open(DirectoryRoots::under(temp.path())).expect("app");
        let service = open(&app, provider.clone(), Arc::clone(&clock));
        completed(service.apply_network_snapshot(observed_network(), &context()));
        let receipt = completed(service.register_job(
            &context(),
            job.clone(),
            JobConstraints::default().with_network(JobNetworkRequirement::Connected),
            ExistingJobPolicy::KeepExisting,
        ));
        assert_eq!(receipt.status, JobStatus::Scheduled);
        start(&service, &job);
    }
    {
        let app = AppData::open(DirectoryRoots::under(temp.path())).expect("app");
        let service = open(&app, provider, clock);
        let report = completed(service.reconcile_after_restart(&context()));
        assert_eq!(report.running_reconciled, 1);
        assert_eq!(report.pending_constraints, 1);
        let record = service.job_record(&job).expect("record");
        assert_eq!(record.status, JobStatus::PendingConstraints);
        assert!(
            record
                .unmet_constraints
                .iter()
                .any(|unmet| matches!(unmet, UnmetConstraint::NetworkUnknown))
        );
    }
}

#[test]
fn overdue_deadline_reconciles_to_expired_budget() {
    let temp = TempDir::new().expect("temp");
    let clock = Arc::new(ManualClock::new(1_000));
    let provider = FakeJobProvider::new();
    let job = key("restart-deadline");
    {
        let app = AppData::open(DirectoryRoots::under(temp.path())).expect("app");
        let service = open(&app, provider.clone(), Arc::clone(&clock));
        completed(service.register_job(
            &context(),
            job.clone(),
            JobConstraints::default().with_deadline_ms(500),
            ExistingJobPolicy::KeepExisting,
        ));
        start(&service, &job);
        clock.advance(600);
    }
    {
        let app = AppData::open(DirectoryRoots::under(temp.path())).expect("app");
        let service = open(&app, provider, clock);
        let report = completed(service.reconcile_after_restart(&context()));
        assert_eq!(report.expired_budget, 1);
        let record = service.job_record(&job).expect("record");
        assert_eq!(record.status, JobStatus::ExpiredBudget);
        assert!(record.unmet_constraints.is_empty());
    }
}

#[test]
fn exhausted_attempt_budget_reconciles_to_expired_budget() {
    let temp = TempDir::new().expect("temp");
    let clock = Arc::new(ManualClock::new(1_000));
    let provider = FakeJobProvider::new();
    let job = key("restart-attempts");
    {
        let app = AppData::open(DirectoryRoots::under(temp.path())).expect("app");
        let service = open(&app, provider.clone(), Arc::clone(&clock));
        completed(service.register_job(
            &context(),
            job.clone(),
            JobConstraints::default().with_retry(RetryBackoffPolicy::once()),
            ExistingJobPolicy::KeepExisting,
        ));
        start(&service, &job);
    }
    {
        let app = AppData::open(DirectoryRoots::under(temp.path())).expect("app");
        let service = open(&app, provider, clock);
        let report = completed(service.reconcile_after_restart(&context()));
        assert_eq!(report.expired_budget, 1);
        assert_eq!(service.job_status(&job), Some(JobStatus::ExpiredBudget));
    }
}

#[test]
fn reconciliation_is_idempotent_and_does_not_double_advance() {
    let temp = TempDir::new().expect("temp");
    let clock = Arc::new(ManualClock::new(1_000));
    let provider = FakeJobProvider::new();
    let job = key("restart-idempotent");
    {
        let app = AppData::open(DirectoryRoots::under(temp.path())).expect("app");
        let service = open(&app, provider.clone(), Arc::clone(&clock));
        completed(service.register_job(
            &context(),
            job.clone(),
            JobConstraints::default(),
            ExistingJobPolicy::KeepExisting,
        ));
        start(&service, &job);
    }
    {
        let app = AppData::open(DirectoryRoots::under(temp.path())).expect("app");
        let service = open(&app, provider, clock);
        let first = completed(service.reconcile_after_restart(&context()));
        let epoch = service.job_record(&job).expect("record").run_epoch;
        let second = completed(service.reconcile_after_restart(&context()));
        assert_eq!(first, second);
        assert_eq!(service.job_record(&job).expect("record").run_epoch, epoch);
        assert_eq!(service.job_status(&job), Some(JobStatus::Scheduled));
    }
}

#[test]
fn scheduled_jobs_are_reregistered_with_the_new_provider_after_restart() {
    let temp = TempDir::new().expect("temp");
    let clock = Arc::new(ManualClock::new(1_000));
    let old_provider = FakeJobProvider::new();
    let new_provider = FakeJobProvider::new();
    let job = key("restart-scheduled-provider");
    {
        let app = AppData::open(DirectoryRoots::under(temp.path())).expect("app");
        let service = open(&app, old_provider.clone(), Arc::clone(&clock));
        completed(service.register_job(
            &context(),
            job.clone(),
            JobConstraints::default(),
            ExistingJobPolicy::KeepExisting,
        ));
        assert_eq!(old_provider.schedule_request_count(), 1);
    }
    {
        let app = AppData::open(DirectoryRoots::under(temp.path())).expect("app");
        let service = open(&app, new_provider.clone(), clock);
        let report = completed(service.reconcile_after_restart(&context()));
        assert_eq!(report.jobs_scanned, 1);
        assert_eq!(report.rescheduled, 1);
        assert_eq!(new_provider.schedule_request_count(), 1);
        assert_eq!(service.job_status(&job), Some(JobStatus::Scheduled));
        assert!(
            service
                .job_record(&job)
                .expect("record")
                .provider_token
                .is_some()
        );
        let second = completed(service.reconcile_after_restart(&context()));
        assert_eq!(second, report);
        assert_eq!(new_provider.schedule_request_count(), 1);
    }
}

#[test]
fn overdue_scheduled_and_pending_records_expire_during_restart() {
    let temp = TempDir::new().expect("temp");
    let clock = Arc::new(ManualClock::new(1_000));
    let provider = FakeJobProvider::new();
    let scheduled = key("restart-scheduled-expired");
    let pending = key("restart-pending-expired");
    {
        let app = AppData::open(DirectoryRoots::under(temp.path())).expect("app");
        let service = open(&app, provider.clone(), Arc::clone(&clock));
        completed(service.register_job(
            &context(),
            scheduled.clone(),
            JobConstraints::default().with_deadline_ms(500),
            ExistingJobPolicy::KeepExisting,
        ));
        completed(
            service.register_job(
                &context(),
                pending.clone(),
                JobConstraints::default()
                    .with_initial_delay_ms(10_000)
                    .with_deadline_ms(500),
                ExistingJobPolicy::KeepExisting,
            ),
        );
        assert_eq!(service.job_status(&scheduled), Some(JobStatus::Scheduled));
        assert_eq!(
            service.job_status(&pending),
            Some(JobStatus::PendingConstraints)
        );
        clock.advance(600);
    }
    {
        let app = AppData::open(DirectoryRoots::under(temp.path())).expect("app");
        let service = open(&app, provider, clock);
        let report = completed(service.reconcile_after_restart(&context()));
        assert_eq!(report.jobs_scanned, 2);
        assert_eq!(report.expired_budget, 2);
        assert_eq!(
            service.job_status(&scheduled),
            Some(JobStatus::ExpiredBudget)
        );
        assert_eq!(service.job_status(&pending), Some(JobStatus::ExpiredBudget));
    }
}

#[test]
fn periodic_scheduled_job_with_future_interval_stays_scheduled_on_restart() {
    let temp = TempDir::new().expect("temp");
    let clock = Arc::new(ManualClock::new(1_000));
    let old_provider = FakeJobProvider::new();
    let new_provider = FakeJobProvider::new();
    let job = key("restart-periodic-scheduled");
    let next_attempt;
    {
        let app = AppData::open(DirectoryRoots::under(temp.path())).expect("app");
        let service = open(&app, old_provider.clone(), Arc::clone(&clock));
        completed(service.register_job(
            &context(),
            job.clone(),
            JobConstraints::default().with_cadence(
                JobCadence::periodic(MIN_PERIODIC_INTERVAL_MS, 0).expect("periodic cadence"),
            ),
            ExistingJobPolicy::KeepExisting,
        ));
        let token = start(&service, &job);
        let completed_record = completed(service.complete_job(&token, &context()));
        assert_eq!(completed_record.status, JobStatus::Scheduled);
        next_attempt = completed_record.next_attempt_at_ms;
        assert!(next_attempt > clock.now_ms());
    }
    {
        let app = AppData::open(DirectoryRoots::under(temp.path())).expect("app");
        let service = open(&app, new_provider.clone(), Arc::clone(&clock));
        let subscription = service.subscribe();
        let report = completed(service.reconcile_after_restart(&context()));
        assert_eq!(report.jobs_scanned, 1);
        assert_eq!(report.rescheduled, 1);
        assert_eq!(report.retry_backoff, 0);
        assert_eq!(report.pending_constraints, 0);
        let record = service.job_record(&job).expect("record");
        assert_eq!(record.status, JobStatus::Scheduled);
        assert_eq!(record.next_attempt_at_ms, next_attempt);
        assert_eq!(new_provider.schedule_request_count(), 1);
        let request = new_provider.latest_schedule_request().expect("request");
        assert_eq!(request.earliest_eligible_after_ms, next_attempt);
        let events = values(drain(&subscription));
        assert!(!events.iter().any(|event| matches!(
            event,
            JobsEvent::JobReconciled {
                status: JobStatus::PendingConstraints | JobStatus::RetryBackoff,
                ..
            }
        )));
        assert!(events.iter().any(|event| matches!(
            event,
            JobsEvent::JobScheduled { job_key, .. } if job_key == &job
        )));
    }
}
