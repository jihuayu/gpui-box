//! Deterministic test helpers shared by the `gpui-menus` integration suites.
//!
//! The helpers use explicit channels and completion handles. No test relies on
//! elapsed sleeps for ordering, and no helper assumes a Unix-only path or
//! environment.

#![allow(dead_code)]

use gpui_menus::{
    CommandId, CommandPayload, ContextKey, Event, EventSubscription, FakeMenuProvider,
    InvocationContext, MenuCommandEvent, MenuCommandService, ServiceResult, SurfaceId,
    TriggerSource, UiOwner,
};
use std::{
    future::Future,
    pin::Pin,
    rc::Rc,
    sync::Arc,
    task::{Context, Poll, Wake, Waker},
    thread,
    time::Duration,
};

/// Generous hang guard for a background worker that should progress. Ordering
/// never depends on this duration; the thread must make an explicit transition.
pub const WAIT: Duration = Duration::from_secs(10);

pub fn command(value: &str) -> CommandId {
    CommandId::new(value).expect("valid command identity")
}

pub fn context_key(value: &str) -> ContextKey {
    ContextKey::new(value).expect("valid context key")
}

struct ThreadWaker(thread::Thread);

impl Wake for ThreadWaker {
    fn wake(self: Arc<Self>) {
        self.0.unpark();
    }

    fn wake_by_ref(self: &Arc<Self>) {
        self.0.unpark();
    }
}

/// Poll a local future to completion, parking until its waker is notified.
pub fn block_on<F: Future>(future: F) -> F::Output {
    let waker = Waker::from(Arc::new(ThreadWaker(thread::current())));
    let mut context = Context::from_waker(&waker);
    let mut future = Box::pin(future);
    loop {
        match future.as_mut().poll(&mut context) {
            Poll::Ready(output) => return output,
            Poll::Pending => thread::park(),
        }
    }
}

/// Poll a future exactly once without waiting. Event draining uses this so a
/// missing event cannot hang a test.
pub fn poll_once<F: Future>(future: F) -> Option<F::Output> {
    let waker = Waker::noop();
    let mut context = Context::from_waker(waker);
    let mut future = Box::pin(future);
    match future.as_mut().poll(&mut context) {
        Poll::Ready(output) => Some(output),
        Poll::Pending => None,
    }
}

/// Poll a pinned request in place without consuming it.
pub fn poll_once_ref<F: Future + Unpin>(future: &mut F) -> Option<F::Output> {
    let waker = Waker::noop();
    let mut context = Context::from_waker(waker);
    match Pin::new(future).poll(&mut context) {
        Poll::Ready(output) => Some(output),
        Poll::Pending => None,
    }
}

/// Drain every event currently queued by one subscription.
pub fn drain_events(subscription: &EventSubscription<MenuCommandEvent>) -> Vec<MenuCommandEvent> {
    let mut events = Vec::new();
    loop {
        let next = poll_once(
            subscription
                .try_next()
                .expect("only one event reader is active"),
        );
        match next {
            Some(Some(envelope)) => match envelope.payload {
                Event::Value(event) => events.push(event),
                Event::ResyncRequired => panic!("bounded event stream unexpectedly overflowed"),
            },
            Some(None) => break,
            None => break,
        }
    }
    events
}

/// Block until the next event arrives. Pair with a worker transition that is
/// explicitly signalled, never with a wall-clock delay.
pub fn next_event(subscription: &EventSubscription<MenuCommandEvent>) -> MenuCommandEvent {
    let envelope = block_on(
        subscription
            .try_next()
            .expect("only one event reader is active"),
    )
    .expect("event stream closed before the expected event");
    match envelope.payload {
        Event::Value(event) => event,
        Event::ResyncRequired => panic!("bounded event stream unexpectedly overflowed"),
    }
}

/// Create a service and its inspectable fake provider.
pub fn service() -> (MenuCommandService, Rc<FakeMenuProvider>) {
    let provider = Rc::new(FakeMenuProvider::new());
    let service = MenuCommandService::new(provider.clone());
    (service, provider)
}

/// Create a live UI owner and invocation context for one logical surface.
pub fn ui_context(surface: u64) -> (UiOwner, InvocationContext) {
    let owner = UiOwner::new(SurfaceId(surface));
    let context =
        gpui_menus::invocation_context(&owner, TriggerSource::Menu).expect("live UI context");
    (owner, context)
}

pub fn payload<T: serde::Serialize>(value: &T) -> CommandPayload {
    CommandPayload::from_typed(value).expect("serializable payload")
}

pub fn expect_completed<T>(result: ServiceResult<T>) -> T {
    match result {
        Ok(gpui_menus::Completion::Completed(value)) => value,
        Ok(gpui_menus::Completion::Cancelled(reason)) => {
            panic!("expected completion, received cancellation: {reason:?}")
        }
        Err(error) => panic!("expected completion, received error: {error:?}"),
    }
}

pub fn expect_cancelled<T>(result: ServiceResult<T>) -> gpui_menus::CancelReason {
    match result {
        Ok(gpui_menus::Completion::Cancelled(reason)) => reason,
        Ok(gpui_menus::Completion::Completed(_)) => {
            panic!("expected cancellation, received completion")
        }
        Err(error) => panic!("expected cancellation, received error: {error:?}"),
    }
}

pub fn expect_error<T>(result: ServiceResult<T>, code: gpui_menus::ErrorCode) {
    match result {
        Err(error) => assert_eq!(error.code, code, "unexpected error: {error:?}"),
        Ok(gpui_menus::Completion::Completed(_)) => {
            panic!("expected {code:?}, received completion")
        }
        Ok(gpui_menus::Completion::Cancelled(reason)) => {
            panic!("expected {code:?}, received cancellation: {reason:?}")
        }
    }
}

/// Pin a request so a test can inspect other state before awaiting it.
pub fn pin_request(
    request: gpui_system::Request<CommandPayload>,
) -> Pin<Box<gpui_system::Request<CommandPayload>>> {
    Box::pin(request)
}
