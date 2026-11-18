use super::*;
use std::sync::atomic::AtomicUsize;

#[derive(PartialEq, Debug)]
struct Model {
    title: u32,
    body: u32,
}
struct WakeCount(AtomicUsize);
impl std::task::Wake for WakeCount {
    fn wake(self: Arc<Self>) {
        self.0.fetch_add(1, Ordering::Relaxed);
    }
}
#[test]
fn business_store_whole_revisions_coalesce_across_thread_without_field_mixing() {
    let mut writer = StoreWriter::new(Model { title: 0, body: 0 }).unwrap();
    let read = writer.read_only();
    let mut observer = read.subscribe().unwrap();
    let original = observer.take_latest().unwrap().unwrap();
    let worker = std::thread::spawn(move || {
        for value in 1..=200 {
            let expected = writer.read_only().snapshot().unwrap().revision();
            writer
                .commit(
                    expected,
                    Model {
                        title: value,
                        body: value * 2,
                    },
                )
                .unwrap();
        }
        writer
    });
    let writer = worker.join().unwrap();
    let current = observer.take_latest().unwrap().unwrap();
    assert_eq!(current.revision().number(), 200);
    assert_eq!(
        *current.state(),
        Model {
            title: 200,
            body: 400
        }
    );
    assert!(observer.pending().unwrap().is_none());
    drop((writer, read, observer));
    assert_eq!(*original.state(), Model { title: 0, body: 0 });
    assert_eq!(
        *current.state(),
        Model {
            title: 200,
            body: 400
        }
    );
}
#[test]
fn business_store_expected_revision_and_equal_commit_preserve_snapshot_identity() {
    let mut writer = StoreWriter::new(10u32).unwrap();
    let read = writer.read_only();
    let initial = read.snapshot().unwrap();
    assert!(!writer.commit(initial.revision(), 10).unwrap().changed);
    assert_eq!(read.snapshot().unwrap(), initial);
    let other = StoreWriter::new(10u32).unwrap();
    assert_eq!(
        writer.commit(other.read_only().snapshot().unwrap().revision(), 20),
        Err(StoreError::ForeignRevision)
    );
    writer.commit(initial.revision(), 20).unwrap();
    assert_eq!(
        writer.commit(initial.revision(), 20),
        Err(StoreError::StaleRevision)
    );
    assert_eq!(*read.snapshot().unwrap().state(), 20);
}
#[test]
fn business_store_acknowledging_captured_revision_keeps_newer_commit_pending() {
    let mut writer = StoreWriter::new(0u32).unwrap();
    let mut observer = writer.read_only().subscribe().unwrap();
    let initial = observer.take_latest().unwrap().unwrap();
    writer.commit(initial.revision(), 1).unwrap();
    let captured = observer.pending().unwrap().unwrap();
    writer.commit(captured.revision(), 2).unwrap();
    observer.acknowledge(captured.revision()).unwrap();
    assert_eq!(*observer.pending().unwrap().unwrap().state(), 2);
    assert_eq!(
        observer.acknowledge(initial.revision()),
        Err(StoreError::StaleRevision)
    );
}
#[test]
fn business_store_wake_and_close_deliver_final_snapshot_then_end() {
    let mut writer = StoreWriter::new(0u32).unwrap();
    let mut observer = writer.read_only().subscribe().unwrap();
    let initial = observer.take_latest().unwrap().unwrap();
    let counter = Arc::new(WakeCount(AtomicUsize::new(0)));
    let waker = Waker::from(counter.clone());
    let mut cx = Context::from_waker(&waker);
    assert!(std::pin::pin!(observer.ready()).poll(&mut cx).is_pending());
    writer.commit(initial.revision(), 1).unwrap();
    assert_eq!(counter.0.load(Ordering::Relaxed), 1);
    assert!(matches!(
        std::pin::pin!(observer.ready()).poll(&mut cx),
        Poll::Ready(Ok(true))
    ));
    drop(writer);
    assert_eq!(*observer.take_latest().unwrap().unwrap().state(), 1);
    assert!(matches!(
        std::pin::pin!(observer.ready()).poll(&mut cx),
        Poll::Ready(Ok(false))
    ));
}
#[test]
fn business_store_cancel_wakes_and_unsubscribes_without_closing_writer() {
    let mut writer = StoreWriter::new(0u32).unwrap();
    let read = writer.read_only();
    let mut observer = read.subscribe().unwrap();
    let initial = observer.take_latest().unwrap().unwrap();
    let counter = Arc::new(WakeCount(AtomicUsize::new(0)));
    let waker = Waker::from(counter.clone());
    let mut cx = Context::from_waker(&waker);
    assert!(std::pin::pin!(observer.ready()).poll(&mut cx).is_pending());
    observer.cancel();
    assert_eq!(counter.0.load(Ordering::Relaxed), 1);
    assert!(matches!(observer.pending(), Err(StoreError::Closed)));
    assert!(matches!(
        std::pin::pin!(observer.ready()).poll(&mut cx),
        Poll::Ready(Ok(false))
    ));
    writer.commit(initial.revision(), 1).unwrap();
    assert_eq!(*read.snapshot().unwrap().state(), 1);
}
