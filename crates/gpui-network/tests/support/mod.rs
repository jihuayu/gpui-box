#![allow(dead_code)]

use gpui_network::*;
use gpui_system::{Completion, Event, EventEnvelope, EventSubscription, OperationContext};
use std::{
    future::Future,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll, Waker},
};

pub fn context() -> OperationContext {
    OperationContext::default()
}

pub fn linux_service(fake: FakeNetworkProvider) -> NetworkService {
    NetworkService::with_platform(Arc::new(fake), PlatformTarget::Linux)
}

pub fn routable(transport: TransportKind) -> NetworkSnapshot {
    NetworkSnapshot::routable(transport, false, true, true).expect("valid routable snapshot")
}

pub fn fake_service() -> (FakeNetworkProvider, NetworkService) {
    let fake = FakeNetworkProvider::with_snapshot(routable(TransportKind::Ethernet));
    let service = linux_service(fake.clone());
    (fake, service)
}

pub fn origin(value: &str) -> EndpointOrigin {
    EndpointOrigin::parse(value).expect("valid origin")
}

pub fn completed<T>(result: ServiceResult<T>) -> T {
    match result.expect("operation succeeded") {
        Completion::Completed(value) => value,
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

pub fn values<T>(events: Vec<Event<T>>) -> Vec<T> {
    events
        .into_iter()
        .filter_map(|event| match event {
            Event::Value(value) => Some(value),
            Event::ResyncRequired => None,
        })
        .collect()
}
