use super::*;
use std::{
    future::Future,
    sync::atomic::{AtomicUsize, Ordering},
    task::Wake,
};

#[derive(Default)]
struct Counter(AtomicUsize);
impl Wake for Counter {
    fn wake(self: Arc<Self>) {
        self.0.fetch_add(1, Ordering::Relaxed);
    }
    fn wake_by_ref(self: &Arc<Self>) {
        self.0.fetch_add(1, Ordering::Relaxed);
    }
}
fn ready(receiver: &mut UpdateReceiver, waker: &Waker) -> Poll<Result<bool, Error>> {
    std::pin::pin!(receiver.ready())
        .as_mut()
        .poll(&mut Context::from_waker(waker))
}

#[test]
fn worker_burst_coalesces_before_computation_sinks_effects_and_extraction() {
    let (mut ui, child) = super::super::tests::fixture();
    let input = ui.signal(0u32);
    let derived = ui
        .computed(&[input.dependency()], move |i| {
            Ok(*i.read(input)? << 8 | 255)
        })
        .unwrap();
    ui.bind(derived, Property::color(child)).unwrap();
    let observed = Rc::new(Cell::new(0));
    let output = observed.clone();
    ui.effect(&[derived.dependency()], move |i| {
        output.set(*i.read(derived).unwrap());
        None
    })
    .unwrap();
    let before = ui.frame([100.0; 2]).unwrap();
    let mut receiver = ui.open_updates(2).unwrap();
    let sender = ui.update_sender(&receiver, input).unwrap();
    let counter = Arc::new(Counter::default());
    let waker = Waker::from(counter.clone());
    assert_eq!(ready(&mut receiver, &waker), Poll::Pending);
    let worker = sender.clone();
    std::thread::spawn(move || {
        for value in 1..=1000 {
            assert!(worker.send(value).unwrap());
        }
    })
    .join()
    .unwrap();
    assert!(!sender.send(1000).unwrap());
    assert_eq!(counter.0.load(Ordering::Relaxed), 1);
    assert_eq!(*ui.read(input).unwrap(), 0);
    assert_eq!(ui.stats().transactions, 0);
    assert_eq!(ready(&mut receiver, &waker), Poll::Ready(Ok(true)));
    let batch = ui.apply_updates(&mut receiver).unwrap();
    assert_eq!((batch.applied, batch.retired), (1, 0));
    let commit = batch.commit.unwrap();
    assert_eq!(
        (
            commit.computed_evaluations,
            commit.sink_writes,
            commit.effect_runs
        ),
        (1, 1, 1)
    );
    assert_eq!(commit.commit.revision, before.revision + 1);
    assert_eq!(observed.get(), 1000 << 8 | 255);
    assert_eq!(ui.stats().frame_requests, 1);
    let frame = ui.frame([100.0; 2]).unwrap();
    assert_eq!((frame.layout_runs, frame.recorded_chunks), (0, 0));
    assert_eq!(
        (
            receiver.stats().unwrap().coalesced,
            receiver.stats().unwrap().equal_pending
        ),
        (999, 1)
    );
    assert!(ui.apply_updates(&mut receiver).unwrap().commit.is_none());
    assert_eq!(ready(&mut receiver, &waker), Poll::Pending);
    sender.send(1000).unwrap();
    let batch = ui.apply_updates(&mut receiver).unwrap();
    assert!(!batch.commit.unwrap().commit.request_frame);
    assert_eq!(ui.stats().computed_evaluations, 1);
    assert_eq!(counter.0.load(Ordering::Relaxed), 2);
}

#[test]
fn bounded_replacement_retired_sources_and_invalid_batches_do_not_revive_state() {
    let (mut ui, child) = super::super::tests::fixture();
    let color = ui.signal(0x0000ffffu32);
    let opacity = ui.signal(1.0f32);
    ui.bind(color, Property::color(child)).unwrap();
    ui.bind(opacity, Property::opacity(child)).unwrap();
    let before = ui.frame([100.0; 2]).unwrap();
    let mut receiver = ui.open_updates(1).unwrap();
    let a = ui.update_sender(&receiver, color).unwrap();
    let b = ui.update_sender(&receiver, opacity).unwrap();
    a.send(0xffffffff).unwrap();
    assert_eq!(b.send(0.5), Err(Error::UpdateQueueFull));
    a.send(0x112233ff).unwrap();
    ui.dispose_signal(color).unwrap();
    assert_eq!(a.send(0), Err(Error::InvalidSignal));
    let fresh = ui.signal(7u32);
    let batch = ui.apply_updates(&mut receiver).unwrap();
    assert_eq!((batch.applied, batch.retired), (0, 1));
    assert!(batch.commit.is_none());
    assert_eq!(*ui.read(fresh).unwrap(), 7);
    assert_eq!(ui.frame([100.0; 2]).unwrap().revision, before.revision);
    drop(receiver);
    let mut receiver = ui.open_updates(2).unwrap();
    let a = ui.update_sender(&receiver, fresh).unwrap();
    let b = ui.update_sender(&receiver, opacity).unwrap();
    a.send(10).unwrap();
    b.send(f32::NAN).unwrap();
    assert!(matches!(
        ui.apply_updates(&mut receiver),
        Err(Error::InvalidOpacity)
    ));
    assert_eq!(
        (*ui.read(fresh).unwrap(), *ui.read(opacity).unwrap()),
        (7, 1.0)
    );
    assert_eq!(receiver.stats().unwrap().rejected, 2);
    assert_eq!(receiver.stats().unwrap().pending, 0);
    a.send(11).unwrap();
    b.send(0.75).unwrap();
    assert_eq!(ui.apply_updates(&mut receiver).unwrap().applied, 2);
}

#[test]
fn ownership_readonly_closure_and_waiter_replacement_are_checked() {
    let (mut ui, _) = super::super::tests::fixture();
    let (mut other, _) = super::super::tests::fixture();
    let input = ui.signal(1u32);
    let derived = ui
        .computed(&[input.dependency()], move |i| Ok(*i.read(input)?))
        .unwrap();
    assert!(matches!(ui.open_updates(0), Err(Error::InvalidUpdateQueue)));
    let mut receiver = ui.open_updates(1).unwrap();
    assert!(matches!(ui.open_updates(1), Err(Error::InvalidUpdateQueue)));
    assert!(matches!(
        other.update_sender(&receiver, input),
        Err(Error::InvalidUpdateQueue)
    ));
    assert!(matches!(
        ui.update_sender(&receiver, derived),
        Err(Error::ReadonlySignal)
    ));
    assert!(matches!(
        other.apply_updates(&mut receiver),
        Err(Error::InvalidUpdateQueue)
    ));
    let sender = ui.update_sender(&receiver, input).unwrap();
    let first = Arc::new(Counter::default());
    let second = Arc::new(Counter::default());
    assert_eq!(
        ready(&mut receiver, &Waker::from(first.clone())),
        Poll::Pending
    );
    assert_eq!(
        ready(&mut receiver, &Waker::from(second.clone())),
        Poll::Pending
    );
    sender.send(2).unwrap();
    assert_eq!(first.0.load(Ordering::Relaxed), 0);
    assert_eq!(second.0.load(Ordering::Relaxed), 1);
    ui.apply_updates(&mut receiver).unwrap();
    assert_eq!(
        ready(&mut receiver, &Waker::from(second.clone())),
        Poll::Pending
    );
    drop(ui);
    assert_eq!(second.0.load(Ordering::Relaxed), 2);
    assert_eq!(
        ready(&mut receiver, &Waker::from(second)),
        Poll::Ready(Ok(false))
    );
    assert!(sender.stats().unwrap().closed);
    assert!(matches!(
        sender.send(3),
        Err(Error::InvalidSignal) | Err(Error::UpdateQueueClosed)
    ));
}

#[test]
fn concurrent_senders_are_drained_without_lost_updates_or_shared_ui() {
    let (mut ui, _) = super::super::tests::fixture();
    let signals: Vec<_> = (0..4).map(|_| ui.signal(0u32)).collect();
    let mut receiver = ui.open_updates(4).unwrap();
    let threads: Vec<_> = signals
        .iter()
        .map(|signal| {
            let sender = ui.update_sender(&receiver, *signal).unwrap();
            std::thread::spawn(move || {
                for value in 1..=500 {
                    sender.send(value).unwrap();
                    std::thread::yield_now();
                }
            })
        })
        .collect();
    while threads.iter().any(|worker| !worker.is_finished()) {
        ui.apply_updates(&mut receiver).unwrap();
        std::thread::yield_now();
    }
    for worker in threads {
        worker.join().unwrap();
    }
    ui.apply_updates(&mut receiver).unwrap();
    for signal in signals {
        assert_eq!(*ui.read(signal).unwrap(), 500);
    }
    let stats = receiver.stats().unwrap();
    assert_eq!(stats.enqueued, stats.coalesced + stats.applied);
    assert_eq!((stats.pending, stats.retired, stats.rejected), (0, 0, 0));
}
