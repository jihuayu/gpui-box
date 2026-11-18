use super::*;
use std::sync::atomic::AtomicUsize;

#[derive(Debug, PartialEq, Eq)]
struct Save(u32);
impl Command for Save {
    type Output = u32;
    type Error = &'static str;
}
fn accepted(sender: &CommandSender<Save>, value: u32) -> CommandTicket<Save> {
    sender
        .send(Save(value))
        .unwrap_or_else(|e| panic!("unexpected rejection: {:?}", e.reason))
}
struct WakeCount(AtomicUsize);
impl std::task::Wake for WakeCount {
    fn wake(self: Arc<Self>) {
        self.0.fetch_add(1, Ordering::Relaxed);
    }
}
#[test]
fn typed_commands_fifo_capacity_includes_running_and_rejection_returns_request() {
    assert!(matches!(
        command_channel::<Save>(0),
        Err(CommandQueueError::InvalidCapacity)
    ));
    let (sender, mut receiver) = command_channel(2).unwrap();
    let mut first = accepted(&sender, 1);
    let second = accepted(&sender, 2);
    assert_ne!(first.id(), second.id());
    let mut running = receiver.try_recv().unwrap();
    assert_eq!(running.id(), first.id());
    assert_eq!(running.take_command(), Some(Save(1)));
    assert!(running.take_command().is_none());
    assert_eq!(first.status(), CommandStatus::Running);
    let rejected = sender.send(Save(3)).err().unwrap();
    assert_eq!(
        (rejected.reason, rejected.command),
        (CommandRejectReason::Busy, Save(3))
    );
    running.succeed(10, None);
    assert_eq!(
        first.try_result(),
        Some(Ok(CommandSuccess {
            output: 10,
            committed_revision: None
        }))
    );
    let third = accepted(&sender, 3);
    assert_eq!(receiver.try_recv().unwrap().id(), second.id());
    assert_eq!(receiver.try_recv().unwrap().id(), third.id());
}
#[test]
fn typed_commands_queued_cancel_releases_capacity_and_prevents_dispatch() {
    let (sender, mut receiver) = command_channel(1).unwrap();
    let mut ticket = accepted(&sender, 1);
    assert!(ticket.request_cancel());
    assert!(!ticket.request_cancel());
    assert_eq!(ticket.status(), CommandStatus::Cancelled);
    assert_eq!(ticket.try_result(), Some(Err(CommandFailure::Cancelled)));
    assert!(receiver.try_recv().is_none());
    let next = accepted(&sender, 2);
    assert_eq!(receiver.try_recv().unwrap().id(), next.id());
}
#[test]
fn typed_commands_running_cancellation_is_a_request_and_committed_success_can_win() {
    let (sender, mut receiver) = command_channel(1).unwrap();
    let mut ticket = accepted(&sender, 1);
    let mut request = receiver.try_recv().unwrap();
    let count = Arc::new(WakeCount(AtomicUsize::new(0)));
    let waker = Waker::from(count.clone());
    let mut cx = Context::from_waker(&waker);
    assert!(
        std::pin::pin!(request.cancelled())
            .poll(&mut cx)
            .is_pending()
    );
    assert!(ticket.request_cancel());
    assert_eq!(count.0.load(Ordering::Relaxed), 1);
    assert!(request.cancellation_requested());
    assert!(std::pin::pin!(request.cancelled()).poll(&mut cx).is_ready());
    assert_eq!(ticket.status(), CommandStatus::Running);
    request.succeed(20, None);
    assert_eq!(ticket.status(), CommandStatus::Succeeded);
    assert_eq!(ticket.try_result().unwrap().unwrap().output, 20);
    assert!(!ticket.request_cancel());
}
#[test]
fn typed_commands_confirmed_cancel_and_business_error_are_distinct_terminal_results() {
    let (sender, mut receiver) = command_channel(1).unwrap();
    let mut cancelled = accepted(&sender, 1);
    receiver.try_recv().unwrap().cancel();
    assert_eq!(cancelled.status(), CommandStatus::Cancelled);
    assert_eq!(cancelled.try_result(), Some(Err(CommandFailure::Cancelled)));
    let mut failed = accepted(&sender, 2);
    receiver.try_recv().unwrap().fail("stale revision");
    assert_eq!(failed.status(), CommandStatus::Failed);
    assert_eq!(
        failed.try_result(),
        Some(Err(CommandFailure::Business("stale revision")))
    );
}
#[test]
fn typed_commands_receiver_drop_fails_queued_but_running_request_can_complete() {
    let (sender, mut receiver) = command_channel(2).unwrap();
    let mut running_ticket = accepted(&sender, 1);
    let mut queued_ticket = accepted(&sender, 2);
    let running = receiver.try_recv().unwrap();
    drop(receiver);
    assert_eq!(
        queued_ticket.try_result(),
        Some(Err(CommandFailure::ExecutorClosed))
    );
    let rejected = sender.send(Save(3)).err().unwrap();
    assert_eq!(rejected.reason, CommandRejectReason::Closed);
    running.succeed(10, None);
    assert_eq!(running_ticket.try_result().unwrap().unwrap().output, 10);
}
#[test]
fn typed_commands_close_drains_and_last_sender_drop_wakes_receiver() {
    let (sender, mut receiver) = command_channel::<Save>(2).unwrap();
    let clone = sender.clone();
    let count = Arc::new(WakeCount(AtomicUsize::new(0)));
    let waker = Waker::from(count.clone());
    let mut cx = Context::from_waker(&waker);
    assert!(std::pin::pin!(receiver.ready()).poll(&mut cx).is_pending());
    drop(sender);
    assert_eq!(count.0.load(Ordering::Relaxed), 0);
    drop(clone);
    assert_eq!(count.0.load(Ordering::Relaxed), 1);
    assert!(matches!(
        std::pin::pin!(receiver.ready()).poll(&mut cx),
        Poll::Ready(false)
    ));
    let (sender, mut receiver) = command_channel(1).unwrap();
    let ticket = accepted(&sender, 1);
    receiver.close();
    assert!(matches!(
        std::pin::pin!(receiver.ready()).poll(&mut cx),
        Poll::Ready(true)
    ));
    assert_eq!(receiver.try_recv().unwrap().id(), ticket.id());
    assert!(matches!(
        std::pin::pin!(receiver.ready()).poll(&mut cx),
        Poll::Ready(false)
    ));
}
#[test]
fn typed_commands_dropped_running_request_or_panic_reports_abandoned_and_frees_capacity() {
    let (sender, mut receiver) = command_channel(1).unwrap();
    for unwind in [false, true] {
        let mut ticket = accepted(&sender, 1);
        let request = receiver.try_recv().unwrap();
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
            let _request = request;
            if unwind {
                panic!("expected command worker panic");
            }
        }));
        assert_eq!(outcome.is_err(), unwind);
        assert_eq!(
            ticket.try_result(),
            Some(Err(CommandFailure::ExecutorAbandoned))
        );
        assert_eq!(ticket.status(), CommandStatus::Failed);
    }
}
#[test]
fn typed_commands_ticket_drop_does_not_cancel_accepted_business_work() {
    let (sender, mut receiver) = command_channel(1).unwrap();
    let ticket = accepted(&sender, 42);
    drop(ticket);
    let mut request = receiver.try_recv().unwrap();
    assert!(!request.cancellation_requested());
    assert_eq!(request.take_command(), Some(Save(42)));
    request.succeed(42, None);
    assert!(sender.send(Save(43)).is_ok());
}
#[test]
fn typed_commands_executor_wakes_receiver_and_one_owned_result_waiter() {
    let (sender, mut receiver) = command_channel(1).unwrap();
    let count = Arc::new(WakeCount(AtomicUsize::new(0)));
    let waker = Waker::from(count.clone());
    let mut cx = Context::from_waker(&waker);
    assert!(std::pin::pin!(receiver.ready()).poll(&mut cx).is_pending());
    let mut ticket = accepted(&sender, 1);
    assert_eq!(count.0.load(Ordering::Relaxed), 1);
    assert!(std::pin::pin!(ticket.result()).poll(&mut cx).is_pending());
    receiver.try_recv().unwrap().succeed(9, None);
    assert_eq!(count.0.load(Ordering::Relaxed), 2);
    assert!(matches!(
        std::pin::pin!(ticket.result()).poll(&mut cx),
        Poll::Ready(Ok(CommandSuccess { output: 9, .. }))
    ));
    assert_eq!(ticket.status(), CommandStatus::Succeeded);
    assert!(ticket.try_result().is_none());
}
#[test]
fn typed_commands_cross_thread_commit_revision_is_distinct_from_ui_adoption() {
    use crate::store::StoreWriter;
    let mut writer = StoreWriter::new(0u32).unwrap();
    let read = writer.read_only();
    let old = read.snapshot().unwrap();
    let (sender, mut receiver) = command_channel(2).unwrap();
    let mut first = accepted(&sender, 3);
    let mut second = accepted(&sender, 7);
    let worker = std::thread::spawn(move || {
        while let Some(mut request) = receiver.try_recv() {
            let Save(value) = request.take_command().unwrap();
            let revision = writer
                .commit(writer.read_only().snapshot().unwrap().revision(), value)
                .unwrap()
                .revision;
            request.succeed(value, Some(revision));
        }
    });
    worker.join().unwrap();
    let one = first.try_result().unwrap().unwrap();
    let two = second.try_result().unwrap().unwrap();
    assert_eq!((one.output, two.output), (3, 7));
    assert_eq!(one.committed_revision.unwrap().number(), 1);
    assert_eq!(two.committed_revision.unwrap().number(), 2);
    assert_eq!(*read.snapshot().unwrap().state(), 7);
    assert_eq!(*old.state(), 0);
}

#[test]
fn typed_commands_cancel_racing_receiver_drop_has_one_terminal_outcome() {
    for _ in 0..64 {
        let (sender, receiver) = command_channel(1).unwrap();
        let mut ticket = accepted(&sender, 1);
        let barrier = Arc::new(std::sync::Barrier::new(2));
        let start = barrier.clone();
        let worker = std::thread::spawn(move || {
            start.wait();
            drop(receiver);
        });
        barrier.wait();
        ticket.request_cancel();
        worker.join().unwrap();
        assert!(matches!(
            ticket.try_result(),
            Some(Err(
                CommandFailure::Cancelled | CommandFailure::ExecutorClosed
            ))
        ));
        assert!(!ticket.request_cancel());
        assert!(matches!(
            sender.send(Save(2)),
            Err(CommandRejection {
                reason: CommandRejectReason::Closed,
                ..
            })
        ));
    }
}
