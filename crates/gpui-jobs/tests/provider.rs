//! Provider boundary, fake scheduling behaviour and time-slice handoff.

mod support;

use gpui_jobs::*;
use std::sync::Arc;
use support::*;

fn fake_service() -> (JobsService, FakeJobProvider, Arc<ManualClock>) {
    let clock = Arc::new(ManualClock::new(1_000));
    let provider = FakeJobProvider::new();
    let service = service_with(provider.clone(), Arc::clone(&clock));
    (service, provider, clock)
}

#[test]
fn schedule_receipts_are_advisory_and_carry_no_exact_start_time() {
    let (service, provider, clock) = fake_service();
    let job = key("provider-request");
    let receipt = completed(service.register_job(
        &context(),
        job.clone(),
        JobConstraints::default().with_initial_delay_ms(2_000),
        ExistingJobPolicy::KeepExisting,
    ));
    assert_eq!(receipt.status, JobStatus::PendingConstraints);
    assert_eq!(provider.schedule_request_count(), 0);
    clock.advance(2_000);
    completed(service.tick(&context()));
    assert_eq!(provider.schedule_request_count(), 1);
    let request = provider.latest_schedule_request().expect("request");
    assert_eq!(request.key, job);
    assert_eq!(
        request.guarantee,
        SchedulingGuarantee::AdvisoryOpportunistic
    );
    assert_eq!(request.earliest_eligible_after_ms, 3_000);
}

#[test]
fn fake_provider_records_requests_and_cancellations() {
    let (service, provider, clock) = fake_service();
    let job = key("provider-record");
    completed(service.register_job(
        &context(),
        job.clone(),
        JobConstraints::default(),
        ExistingJobPolicy::KeepExisting,
    ));
    let request = provider.latest_schedule_request().expect("request");
    assert_eq!(request.key, job);
    assert_eq!(
        request.guarantee,
        SchedulingGuarantee::AdvisoryOpportunistic
    );
    assert_eq!(request.earliest_eligible_after_ms, 1_000);
    assert!(!request.idempotency_token.as_str().is_empty());

    completed(service.cancel_job(&job, &context()));
    assert_eq!(provider.cancel_calls(), vec![job]);
    assert_eq!(provider.cancel_call_count(), 1);
    clock.advance(1_000);
    completed(service.tick(&context()));
    assert_eq!(provider.cancel_call_count(), 1);
}

#[test]
fn unavailable_provider_is_reported_not_faked() {
    let clock = Arc::new(ManualClock::new(1_000));
    let provider = FakeJobProvider::new();
    provider.set_unavailable(true);
    let service = service_with(provider.clone(), Arc::clone(&clock));
    let job = key("provider-down");
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
            .any(|unmet| matches!(unmet, UnmetConstraint::ProviderUnavailable))
    );
    assert_eq!(provider.schedule_request_count(), 0);
    assert!(!durable_scheduling_available(PlatformTarget::Linux));

    provider.set_unavailable(false);
    completed(service.tick(&context()));
    assert_eq!(service.job_status(&job), Some(JobStatus::Scheduled));
    assert_eq!(provider.schedule_request_count(), 1);
}

#[test]
fn unavailable_contract_provider_never_schedules() {
    let service =
        JobsService::in_memory(Arc::new(UnavailableJobProvider)).expect("unavailable service");
    let job = key("unavailable");
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
            .any(|unmet| matches!(unmet, UnmetConstraint::ProviderUnavailable))
    );
    for platform in PlatformTarget::ALL {
        assert!(!durable_scheduling_available(platform));
    }
}

#[test]
fn failed_schedule_is_retried_without_losing_the_job() {
    let (service, provider, _clock) = fake_service();
    provider.fail_next_schedule();
    let job = key("schedule-failure");
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
            .any(|unmet| matches!(unmet, UnmetConstraint::ProviderUnavailable))
    );
    assert_eq!(
        service.job_status(&job),
        Some(JobStatus::PendingConstraints)
    );
    completed(service.tick(&context()));
    assert_eq!(service.job_status(&job), Some(JobStatus::Scheduled));
}

#[test]
fn provider_time_slice_expiry_preempts_only_the_matching_epoch() {
    let (service, provider, _clock) = fake_service();
    let job = key("time-slice");
    completed(service.register_job(
        &context(),
        job.clone(),
        JobConstraints::default(),
        ExistingJobPolicy::KeepExisting,
    ));
    let token = match completed(service.handle_wakeup(&job, &context())) {
        WakeupOutcome::Started(token) => token,
        other => panic!("expected started, got {other:?}"),
    };
    provider.expire_time_slice(&job, token.run_epoch + 1);
    assert_eq!(service.job_status(&job), Some(JobStatus::Running));

    let subscription = service.subscribe();
    provider.expire_time_slice(&job, token.run_epoch);
    assert!(values(drain(&subscription)).iter().any(|event| matches!(
        event,
        JobsEvent::PreemptionRequested {
            reason: PreemptReason::ProviderTimeSliceExpired,
            ..
        }
    )));
    let record = completed(service.ack_preemption(&token, &context()));
    assert_eq!(record.status, JobStatus::ExpiredBudget);
}

#[test]
fn provider_unavailable_notice_preempts_an_active_attempt() {
    let (service, provider, _clock) = fake_service();
    let job = key("provider-notice");
    completed(service.register_job(
        &context(),
        job.clone(),
        JobConstraints::default(),
        ExistingJobPolicy::KeepExisting,
    ));
    let token = match completed(service.handle_wakeup(&job, &context())) {
        WakeupOutcome::Started(token) => token,
        other => panic!("expected started, got {other:?}"),
    };
    provider.report_unavailable(&job);
    assert_eq!(service.job_status(&job), Some(JobStatus::Running));
    assert_eq!(
        completed(service.ack_preemption(&token, &context())).status,
        JobStatus::Scheduled
    );
}

#[test]
fn service_drop_releases_the_provider_listener() {
    let clock = Arc::new(ManualClock::new(1_000));
    let provider = FakeJobProvider::new();
    {
        let service = service_with(provider.clone(), clock);
        assert_eq!(provider.listener_count(), 1);
        completed(service.register_job(
            &context(),
            key("listener"),
            JobConstraints::default(),
            ExistingJobPolicy::KeepExisting,
        ));
        assert_eq!(provider.listener_count(), 1);
    }
    assert_eq!(provider.listener_count(), 0);
}
