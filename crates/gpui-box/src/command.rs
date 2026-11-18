//! Reliable typed business requests. This queue never coalesces requests/results
//! and does not execute user handlers. Applications own execution and Store commits.
use crate::store::StoreRevision;
use std::{
    collections::VecDeque,
    future::Future,
    pin::Pin,
    sync::{
        Arc, Mutex, MutexGuard, Weak,
        atomic::{AtomicU64, Ordering},
    },
    task::{Context, Poll, Waker},
};

/// One business intention and its typed response. Request/result values cross
/// application threads; no UI owner or renderer callback is carried by the API.
pub trait Command: Send + 'static {
    type Output: Send + 'static;
    type Error: Send + 'static;
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CommandId {
    queue: u64,
    serial: u64,
}
impl CommandId {
    pub fn sequence(self) -> u64 {
        self.serial
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CommandStatus {
    Accepted,
    Running,
    Succeeded,
    Failed,
    Cancelled,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CommandRejectReason {
    Busy,
    Closed,
    IdExhausted,
}
/// Rejection is synchronous: no ticket was accepted; caller retains the request.
#[derive(Debug)]
pub struct CommandRejection<C> {
    pub reason: CommandRejectReason,
    pub command: C,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CommandQueueError {
    InvalidCapacity,
    IdExhausted,
}
#[derive(Debug, PartialEq, Eq)]
pub struct CommandSuccess<T> {
    pub output: T,
    /// Business commit evidence only; does not imply UI applied or displayed.
    pub committed_revision: Option<StoreRevision>,
}
#[derive(Debug, PartialEq, Eq)]
pub enum CommandFailure<E> {
    Business(E),
    Cancelled,
    /// The receiver disappeared before this accepted request started.
    ExecutorClosed,
    /// Running request dropped/panicked without reporting a result.
    ExecutorAbandoned,
}
pub type CommandResult<C> =
    Result<CommandSuccess<<C as Command>::Output>, CommandFailure<<C as Command>::Error>>;

struct CompletionState<C: Command> {
    status: CommandStatus,
    cancel_requested: bool,
    result: Option<CommandResult<C>>,
    waiter: Option<Waker>,
    cancel_waiter: Option<Waker>,
}
struct Completion<C: Command>(Mutex<CompletionState<C>>);
fn lock<T>(value: &Mutex<T>) -> MutexGuard<'_, T> {
    value.lock().unwrap_or_else(|e| e.into_inner())
}
fn wake(wakers: impl IntoIterator<Item = Waker>) {
    for waker in wakers {
        waker.wake();
    }
}
fn terminal<C: Command>(state: &mut CompletionState<C>, result: CommandResult<C>) -> Vec<Waker> {
    state.status = match &result {
        Ok(_) => CommandStatus::Succeeded,
        Err(CommandFailure::Cancelled) => CommandStatus::Cancelled,
        Err(_) => CommandStatus::Failed,
    };
    state.result = Some(result);
    state
        .waiter
        .take()
        .into_iter()
        .chain(state.cancel_waiter.take())
        .collect()
}
struct Envelope<C: Command> {
    id: CommandId,
    command: C,
    completion: Arc<Completion<C>>,
}
struct Queue<C: Command> {
    pending: VecDeque<Envelope<C>>,
    outstanding: usize,
    next: u64,
    senders: usize,
    closed: bool,
    waiter: Option<Waker>,
}
struct Shared<C: Command> {
    id: u64,
    capacity: usize,
    queue: Mutex<Queue<C>>,
}

/// Accepted unresolved requests (queued + running) never exceed capacity.
/// No worker/thread/runtime is created. One receiver dispatches in FIFO order.
pub fn command_channel<C: Command>(
    capacity: usize,
) -> Result<(CommandSender<C>, CommandReceiver<C>), CommandQueueError> {
    if capacity == 0 {
        return Err(CommandQueueError::InvalidCapacity);
    }
    static NEXT: AtomicU64 = AtomicU64::new(1);
    let id = NEXT
        .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |n| n.checked_add(1))
        .map_err(|_| CommandQueueError::IdExhausted)?;
    let shared = Arc::new(Shared {
        id,
        capacity,
        queue: Mutex::new(Queue {
            pending: VecDeque::new(),
            outstanding: 0,
            next: 0,
            senders: 1,
            closed: false,
            waiter: None,
        }),
    });
    Ok((
        CommandSender {
            shared: shared.clone(),
        },
        CommandReceiver { shared },
    ))
}
/// Cloneable capability for exactly one command type. Dropping a sender/ticket
/// does not cancel accepted work. Last sender closes admission, preserving FIFO.
///
/// ```compile_fail
/// use gpui_box::command::{Command, CommandSender};
/// struct Save;
/// impl Command for Save { type Output = (); type Error = (); }
/// fn wrong_intent(sender: CommandSender<Save>) { sender.send("delete"); }
/// ```
pub struct CommandSender<C: Command> {
    shared: Arc<Shared<C>>,
}
impl<C: Command> Clone for CommandSender<C> {
    fn clone(&self) -> Self {
        lock(&self.shared.queue).senders += 1;
        Self {
            shared: self.shared.clone(),
        }
    }
}
impl<C: Command> Drop for CommandSender<C> {
    fn drop(&mut self) {
        let waiter = {
            let mut queue = lock(&self.shared.queue);
            queue.senders -= 1;
            if queue.senders == 0 {
                queue.closed = true;
                queue.waiter.take()
            } else {
                None
            }
        };
        wake(waiter);
    }
}
impl<C: Command> CommandSender<C> {
    #[cfg(feature = "retained")]
    pub(crate) fn same_channel(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.shared, &other.shared)
    }
    pub fn send(&self, command: C) -> Result<CommandTicket<C>, CommandRejection<C>> {
        // Allocate before holding the queue lock. There is no handler invocation.
        let completion = Arc::new(Completion(Mutex::new(CompletionState {
            status: CommandStatus::Accepted,
            cancel_requested: false,
            result: None,
            waiter: None,
            cancel_waiter: None,
        })));
        let (id, waiter) = {
            let mut queue = lock(&self.shared.queue);
            let reason = if queue.closed {
                Some(CommandRejectReason::Closed)
            } else if queue.outstanding >= self.shared.capacity {
                Some(CommandRejectReason::Busy)
            } else if queue.next == u64::MAX {
                Some(CommandRejectReason::IdExhausted)
            } else {
                None
            };
            if let Some(reason) = reason {
                return Err(CommandRejection { reason, command });
            }
            queue.next += 1;
            let id = CommandId {
                queue: self.shared.id,
                serial: queue.next,
            };
            queue.pending.push_back(Envelope {
                id,
                command,
                completion: completion.clone(),
            });
            queue.outstanding += 1;
            (id, queue.waiter.take())
        };
        wake(waiter);
        Ok(CommandTicket {
            id,
            shared: Arc::downgrade(&self.shared),
            completion,
            consumed: false,
        })
    }
}

/// Single request dispatcher. close() rejects new sends and drains accepted
/// work; dropping it reports ExecutorClosed to queued tickets. Running requests
/// can still finish, since their ownership has passed to the application.
pub struct CommandReceiver<C: Command> {
    shared: Arc<Shared<C>>,
}
impl<C: Command> CommandReceiver<C> {
    pub fn close(&mut self) {
        let waiter = {
            let mut queue = lock(&self.shared.queue);
            queue.closed = true;
            queue.waiter.take()
        };
        wake(waiter);
    }
    pub fn try_recv(&mut self) -> Option<CommandRequest<C>> {
        let envelope = {
            let mut queue = lock(&self.shared.queue);
            let envelope = queue.pending.pop_front()?;
            lock(&envelope.completion.0).status = CommandStatus::Running;
            envelope
        };
        Some(CommandRequest {
            id: envelope.id,
            command: Some(envelope.command),
            shared: self.shared.clone(),
            completion: envelope.completion,
            finished: false,
        })
    }
    pub fn ready(&mut self) -> impl Future<Output = bool> + '_ {
        ReceiverReady { receiver: self }
    }
}
impl<C: Command> Drop for CommandReceiver<C> {
    fn drop(&mut self) {
        let (pending, wakers) = {
            let mut queue = lock(&self.shared.queue);
            queue.closed = true;
            let pending = std::mem::take(&mut queue.pending);
            queue.outstanding -= pending.len();
            let mut wakers: Vec<_> = queue.waiter.take().into_iter().collect();
            // Queue -> completion is the common lock order, also used by
            // cancellation/dispatch. Mark terminal before removing ownership,
            // so a racing cancel never sees Accepted without a queued envelope.
            for envelope in &pending {
                wakers.extend(terminal(
                    &mut lock(&envelope.completion.0),
                    Err(CommandFailure::ExecutorClosed),
                ));
            }
            (pending, wakers)
        };
        // Destructors and executor callbacks run without either lock held.
        drop(pending);
        wake(wakers);
    }
}
struct ReceiverReady<'a, C: Command> {
    receiver: &'a mut CommandReceiver<C>,
}
impl<C: Command> Future for ReceiverReady<'_, C> {
    type Output = bool;
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<bool> {
        let next = cx.waker().clone();
        let mut queue = lock(&self.receiver.shared.queue);
        if !queue.pending.is_empty() {
            return Poll::Ready(true);
        }
        if queue.closed {
            return Poll::Ready(false);
        }
        let previous = queue.waiter.replace(next);
        drop(queue);
        drop(previous);
        Poll::Pending
    }
}

/// Request ID/status/cancellation and exactly one owned result. Await or poll
/// result() from any executor. Status remains terminal after the result is taken.
/// Dropping this ticket only stops observing; accepted work is not UI-scoped.
pub struct CommandTicket<C: Command> {
    id: CommandId,
    shared: Weak<Shared<C>>,
    completion: Arc<Completion<C>>,
    consumed: bool,
}
impl<C: Command> CommandTicket<C> {
    pub fn id(&self) -> CommandId {
        self.id
    }
    pub fn status(&self) -> CommandStatus {
        lock(&self.completion.0).status
    }
    pub fn cancellation_requested(&self) -> bool {
        lock(&self.completion.0).cancel_requested
    }
    /// Queued cancellation removes the request, completes Cancelled and releases
    /// capacity immediately. Running cancellation wakes the worker only; worker
    /// must explicitly report Cancelled/success/failure after handling effects.
    pub fn request_cancel(&self) -> bool {
        let Some(shared) = self.shared.upgrade() else {
            return false;
        };
        let (removed, wakers) = {
            let mut queue = lock(&shared.queue);
            let mut state = lock(&self.completion.0);
            if state.cancel_requested
                || !matches!(
                    state.status,
                    CommandStatus::Accepted | CommandStatus::Running
                )
            {
                return false;
            }
            state.cancel_requested = true;
            if state.status == CommandStatus::Accepted {
                let index = queue
                    .pending
                    .iter()
                    .position(|e| e.id == self.id)
                    .expect("accepted queued request");
                let removed = queue.pending.remove(index);
                queue.outstanding -= 1;
                (
                    removed,
                    terminal(&mut state, Err(CommandFailure::Cancelled)),
                )
            } else {
                (None, state.cancel_waiter.take().into_iter().collect())
            }
        };
        drop(removed);
        wake(wakers);
        true
    }
    pub fn try_result(&mut self) -> Option<CommandResult<C>> {
        let result = lock(&self.completion.0).result.take();
        self.consumed |= result.is_some();
        result
    }
    pub fn result(&mut self) -> impl Future<Output = CommandResult<C>> + '_ {
        TicketResult { ticket: self }
    }
}
struct TicketResult<'a, C: Command> {
    ticket: &'a mut CommandTicket<C>,
}
impl<C: Command> Future for TicketResult<'_, C> {
    type Output = CommandResult<C>;
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.get_mut();
        assert!(!this.ticket.consumed, "command result already consumed");
        let next = cx.waker().clone();
        let mut state = lock(&this.ticket.completion.0);
        if let Some(result) = state.result.take() {
            this.ticket.consumed = true;
            return Poll::Ready(result);
        }
        let previous = state.waiter.replace(next);
        drop(state);
        drop(previous);
        Poll::Pending
    }
}

/// Running application-owned request. Taking the payload permits owned IO.
/// Capacity is held until completion/drop, even after payload was taken. No
/// auto-retry or implicit Store mutation is performed by this executor boundary.
pub struct CommandRequest<C: Command> {
    id: CommandId,
    command: Option<C>,
    shared: Arc<Shared<C>>,
    completion: Arc<Completion<C>>,
    finished: bool,
}
impl<C: Command> CommandRequest<C> {
    pub fn id(&self) -> CommandId {
        self.id
    }
    pub fn take_command(&mut self) -> Option<C> {
        self.command.take()
    }
    pub fn cancellation_requested(&self) -> bool {
        lock(&self.completion.0).cancel_requested
    }
    pub fn cancelled(&mut self) -> impl Future<Output = ()> + '_ {
        Cancellation {
            completion: &self.completion,
        }
    }
    pub fn succeed(self, output: C::Output, committed_revision: Option<StoreRevision>) {
        self.finish(Ok(CommandSuccess {
            output,
            committed_revision,
        }));
    }
    pub fn fail(self, error: C::Error) {
        self.finish(Err(CommandFailure::Business(error)));
    }
    pub fn cancel(self) {
        self.finish(Err(CommandFailure::Cancelled));
    }
    fn finish(mut self, result: CommandResult<C>) {
        let wakers = {
            let mut queue = lock(&self.shared.queue);
            let mut state = lock(&self.completion.0);
            queue.outstanding -= 1;
            terminal(&mut state, result)
        };
        self.finished = true;
        wake(wakers);
    }
}
impl<C: Command> Drop for CommandRequest<C> {
    fn drop(&mut self) {
        if !self.finished {
            let wakers = {
                let mut queue = lock(&self.shared.queue);
                queue.outstanding -= 1;
                terminal(
                    &mut lock(&self.completion.0),
                    Err(CommandFailure::ExecutorAbandoned),
                )
            };
            wake(wakers);
        }
    }
}
struct Cancellation<'a, C: Command> {
    completion: &'a Completion<C>,
}
impl<C: Command> Future for Cancellation<'_, C> {
    type Output = ();
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
        let next = cx.waker().clone();
        let mut state = lock(&self.completion.0);
        if state.cancel_requested {
            return Poll::Ready(());
        }
        let previous = state.cancel_waiter.replace(next);
        drop(state);
        drop(previous);
        Poll::Pending
    }
}

#[cfg(test)]
mod tests;
