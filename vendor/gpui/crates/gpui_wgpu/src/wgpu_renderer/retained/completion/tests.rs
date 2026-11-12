use super::*;
use std::sync::{
    Weak,
    atomic::{AtomicUsize, Ordering},
};
struct Counter(AtomicUsize);
impl std::task::Wake for Counter {
    fn wake(self: Arc<Self>) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}
fn poll(
    receiver: &mut CompletionReceiver,
    counter: &Arc<Counter>,
) -> Poll<Result<Option<u64>, String>> {
    let waker = Waker::from(counter.clone());
    std::pin::pin!(receiver.ready())
        .as_mut()
        .poll(&mut Context::from_waker(&waker))
}

#[test]
fn newest_completion_survives_waiter_replacement_and_idle_is_pending() {
    let shared = Arc::new(Shared::default());
    let mut receiver = CompletionReceiver {
        shared: shared.clone(),
    };
    let old = Arc::new(Counter(AtomicUsize::new(0)));
    let current = Arc::new(Counter(AtomicUsize::new(0)));
    assert!(poll(&mut receiver, &old).is_pending());
    assert!(poll(&mut receiver, &current).is_pending());
    shared.publish(Ok(4));
    shared.publish(Ok(7));
    assert_eq!(old.0.load(Ordering::SeqCst), 0);
    assert_eq!(current.0.load(Ordering::SeqCst), 1);
    assert_eq!(poll(&mut receiver, &current), Poll::Ready(Ok(Some(7))));
    assert!(poll(&mut receiver, &current).is_pending());
    assert_eq!(
        (receiver.stats().delivered, receiver.stats().notifications),
        (1, 1)
    );
    // Cancellation wakes the waiter and releases its retained executor waker.
    let weak: Weak<Counter> = Arc::downgrade(&current);
    shared.close();
    assert_eq!(poll(&mut receiver, &current), Poll::Ready(Ok(None)));
    assert_eq!(current.0.load(Ordering::SeqCst), 2);
    drop(current);
    assert!(weak.upgrade().is_none());
}

#[test]
fn error_replaces_pending_success_once_and_closed_channels_do_not_publish() {
    let shared = Arc::new(Shared::default());
    let mut receiver = CompletionReceiver {
        shared: shared.clone(),
    };
    let wake = Arc::new(Counter(AtomicUsize::new(0)));
    shared.publish(Ok(2));
    shared.publish(Err("device wait failed".into()));
    shared.publish(Ok(3));
    assert_eq!(
        poll(&mut receiver, &wake),
        Poll::Ready(Err("device wait failed".into()))
    );
    assert_eq!(poll(&mut receiver, &wake), Poll::Ready(Ok(None)));
    assert_eq!(receiver.stats().delivered, 1);
    drop(receiver);
    assert!(shared.lock().stats.closed);
    assert!(shared.lock().event.is_none());
}

#[test]
fn receipt_preserves_worker_observation_across_delayed_owner_poll() {
    let shared = Arc::new(Shared::default());
    let mut receiver = CompletionReceiver {
        shared: shared.clone(),
    };
    let wake = Arc::new(Counter(AtomicUsize::new(0)));
    let waker = Waker::from(wake);
    let start = Instant::now();
    shared.publish(Ok(3));
    shared.publish(Ok(8));
    let before_owner = Instant::now();
    let Poll::Ready(Ok(Some(receipt))) = std::pin::pin!(receiver.ready_receipt())
        .as_mut()
        .poll(&mut Context::from_waker(&waker))
    else {
        panic!("missing receipt")
    };
    assert_eq!(receipt.serial, 8);
    assert!(receipt.observed_at >= start && receipt.observed_at <= before_owner);
    assert_eq!(receiver.stats().delivered, 1);
    shared.close();
    assert!(matches!(
        std::pin::pin!(receiver.ready_receipt())
            .as_mut()
            .poll(&mut Context::from_waker(&waker)),
        Poll::Ready(Ok(None))
    ));
}
