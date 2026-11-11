#![allow(dead_code)]

use gpui_jobs::*;
use std::{
    future::Future,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll, Waker},
};

pub fn context() -> OperationContext {
    OperationContext::default()
}

pub fn key(name: &str) -> JobKey {
    JobKey::new(JobNamespace::application(), name).expect("valid job key")
}

pub fn completed<T>(result: ServiceResult<T>) -> T {
    match result.expect("operation succeeded") {
        Completion::Completed(value) => value,
        Completion::Cancelled(reason) => panic!("unexpected cancellation: {reason:?}"),
    }
}

pub fn observed_power(
    charging: ChargingState,
    saver: LowPowerMode,
    thermal: ThermalCondition,
) -> PowerSnapshot {
    PowerSnapshot {
        power_source: PowerSourceStatus::new(
            if matches!(charging, ChargingState::Charging | ChargingState::Full) {
                PowerSource::Ac
            } else {
                PowerSource::Battery
            },
            PowerSignalSource::Observed,
        ),
        battery: BatteryStatus::new(Some(70), charging, saver, None, PowerSignalSource::Observed)
            .expect("battery"),
        sleep_resume: gpui_power::SleepResumeStatus::new(
            gpui_power::SleepResumeState::Awake,
            1,
            PowerSignalSource::Observed,
        ),
        session_lock: gpui_power::SessionLockStatus::new(
            gpui_power::SessionLockState::Unlocked,
            PowerSignalSource::Observed,
        ),
        thermal: ThermalStatus::new(thermal, PowerSignalSource::Observed),
        memory: gpui_power::MemoryStatus::new(
            gpui_power::MemoryCondition::Normal,
            PowerSignalSource::Observed,
        ),
        revision: 1,
    }
}

pub fn observed_network() -> NetworkSnapshot {
    NetworkSnapshot::routable(TransportKind::WiFi, false, true, true).expect("routable")
}

pub fn metered_network() -> NetworkSnapshot {
    observed_network().with_cost(ConnectionCost::Metered)
}

pub fn roaming_network() -> NetworkSnapshot {
    observed_network().with_cost(ConnectionCost::CellularRoaming)
}

pub fn next_payload<T>(subscription: &EventSubscription<T>) -> Option<Event<T>> {
    let mut next = subscription.try_next().ok()?;
    let waker = Waker::noop();
    let mut context = Context::from_waker(waker);
    match Pin::new(&mut next).poll(&mut context) {
        Poll::Ready(Some(EventEnvelope { payload, .. })) => Some(payload),
        Poll::Ready(None) | Poll::Pending => None,
    }
}

pub fn next_value<T>(subscription: &EventSubscription<T>) -> Option<T> {
    match next_payload(subscription) {
        Some(Event::Value(value)) => Some(value),
        Some(Event::ResyncRequired) | None => None,
    }
}

pub fn drain<T>(subscription: &EventSubscription<T>) -> Vec<Event<T>> {
    let mut events = Vec::new();
    while let Some(event) = next_payload(subscription) {
        events.push(event);
    }
    events
}

pub fn values<T>(events: Vec<Event<T>>) -> Vec<T> {
    events
        .into_iter()
        .filter_map(|event| match event {
            Event::Value(value) => Some(value),
            Event::ResyncRequired => None,
        })
        .collect()
}

pub fn wait_for<F: Fn() -> bool>(predicate: F) -> bool {
    let mut spins = 0u32;
    while !predicate() {
        if spins >= 10_000 {
            return false;
        }
        spins += 1;
        std::thread::yield_now();
    }
    true
}

pub fn service_with(provider: FakeJobProvider, clock: Arc<ManualClock>) -> JobsService {
    JobsService::in_memory_with_options(
        Arc::new(provider),
        clock,
        DEFAULT_JOBS_EVENT_CAPACITY,
        Arc::new(NoopJobsStorageHook),
    )
    .expect("in-memory jobs service")
}
