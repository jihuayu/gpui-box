#![allow(dead_code)]

use gpui_lifecycle::{SurfaceSnapshot, SurfaceVisibility};
use gpui_power::*;
use gpui_system::{Completion, Event, EventEnvelope, EventSubscription, OperationContext};
use std::{
    future::Future,
    pin::Pin,
    task::{Context, Poll, Waker},
};

pub fn context() -> OperationContext {
    OperationContext::default()
}

pub fn reason(text: impl Into<String>) -> WakeReason {
    WakeReason::new(text).expect("valid test reason")
}

pub fn fake_service() -> (FakePowerProvider, PowerService) {
    let fake = FakePowerProvider::new();
    let service = PowerService::from_fake(fake.clone());
    (fake, service)
}

pub fn completed(
    service: &PowerService,
    request: WakeLeaseRequest,
) -> Result<WakePreventionLease, gpui_system::SystemError> {
    match service.acquire_wake_lease(request, &context())? {
        Completion::Completed(lease) => Ok(lease),
        Completion::Cancelled(reason) => panic!("unexpected cancellation: {reason:?}"),
    }
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

pub fn visible_snapshot(generation: u64) -> SurfaceSnapshot {
    SurfaceSnapshot {
        visibility: SurfaceVisibility::Visible { focused: true },
        generation,
    }
}

pub fn hidden_snapshot(generation: u64) -> SurfaceSnapshot {
    SurfaceSnapshot {
        visibility: SurfaceVisibility::Hidden,
        generation,
    }
}
