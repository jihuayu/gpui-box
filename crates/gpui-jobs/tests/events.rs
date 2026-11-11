//! Bounded event delivery and subscription lifetime.

mod support;

use gpui_jobs::*;
use std::sync::Arc;
use support::*;

fn small_event_service(capacity: usize) -> (JobsService, FakeJobProvider, Arc<ManualClock>) {
    let clock = Arc::new(ManualClock::new(1_000));
    let provider = FakeJobProvider::new();
    let service = JobsService::in_memory_with_options(
        Arc::new(provider.clone()),
        Arc::clone(&clock) as Arc<dyn JobsClock>,
        capacity,
        Arc::new(NoopJobsStorageHook),
    )
    .expect("service");
    (service, provider, clock)
}

#[test]
fn event_overflow_requires_an_explicit_resync() {
    let (service, _provider, _clock) = small_event_service(1);
    let subscription = service.subscribe();
    completed(service.register_job(
        &context(),
        key("one"),
        JobConstraints::default(),
        ExistingJobPolicy::KeepExisting,
    ));
    completed(service.register_job(
        &context(),
        key("two"),
        JobConstraints::default(),
        ExistingJobPolicy::KeepExisting,
    ));
    let events = drain(&subscription);
    assert!(
        events
            .iter()
            .any(|event| matches!(event, Event::ResyncRequired))
    );
    assert!(service.event_revision() >= 2);
}

#[test]
fn subscriptions_are_independent_and_do_not_replay_history() {
    let (service, _provider, _clock) = small_event_service(DEFAULT_JOBS_EVENT_CAPACITY);
    let first = service.subscribe();
    completed(service.register_job(
        &context(),
        key("subscription"),
        JobConstraints::default(),
        ExistingJobPolicy::KeepExisting,
    ));
    let second = service.subscribe();
    assert!(!values(drain(&first)).is_empty());
    assert!(values(drain(&second)).is_empty());
    completed(service.cancel_job(&key("subscription"), &context()));
    assert!(
        values(drain(&second))
            .iter()
            .any(|event| matches!(event, JobsEvent::JobCancelled { .. }))
    );
}

#[test]
fn event_revision_only_advances_when_an_event_is_emitted() {
    let (service, _provider, _clock) = small_event_service(DEFAULT_JOBS_EVENT_CAPACITY);
    let _subscription = service.subscribe();
    let initial = service.event_revision();
    completed(service.register_job(
        &context(),
        key("revision"),
        JobConstraints::default(),
        ExistingJobPolicy::KeepExisting,
    ));
    let after = service.event_revision();
    assert!(after > initial);
    completed(service.tick(&context()));
    assert_eq!(service.event_revision(), after);
}

#[test]
fn default_capacity_is_stable() {
    assert_eq!(DEFAULT_JOBS_EVENT_CAPACITY, 64);
}
