use crate::context::{WakeCell, lock};
use crate::{ErrorCode, SystemError};
use std::{
    collections::VecDeque,
    future::Future,
    pin::Pin,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
    },
    task::{Context, Poll},
    time::SystemTime,
};

static NEXT_SESSION: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event<T> {
    Value(T),
    ResyncRequired,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EventEnvelope<T> {
    pub session: u64,
    pub sequence: u64,
    pub observed_at: SystemTime,
    pub payload: Event<T>,
}

struct State<T> {
    queue: VecDeque<EventEnvelope<T>>,
    sequence: u64,
    overflow: Option<u64>,
    announce_overflow: bool,
    closed: bool,
}

struct Shared<T> {
    session: u64,
    capacity: usize,
    senders: AtomicUsize,
    waiting: AtomicBool,
    state: Mutex<State<T>>,
    wake: Arc<WakeCell>,
}

pub struct EventSender<T>(Arc<Shared<T>>);

/// A single-consumer stream. Native listener cleanup executes on the owning
/// thread on drop, outside the stream mutex. Only one next future may be active.
pub struct EventSubscription<T> {
    shared: Arc<Shared<T>>,
    cleanup: Option<Box<dyn FnOnce()>>,
}

pub fn event_channel<T>(
    capacity: usize,
    cleanup: impl FnOnce() + 'static,
) -> (EventSender<T>, EventSubscription<T>) {
    let shared = Arc::new(Shared {
        session: NEXT_SESSION.fetch_add(1, Ordering::Relaxed),
        capacity: capacity.max(1),
        senders: AtomicUsize::new(1),
        waiting: AtomicBool::new(false),
        state: Mutex::new(State {
            queue: VecDeque::new(),
            sequence: 0,
            overflow: None,
            announce_overflow: false,
            closed: false,
        }),
        wake: Arc::default(),
    });
    (
        EventSender(shared.clone()),
        EventSubscription {
            shared,
            cleanup: Some(Box::new(cleanup)),
        },
    )
}

impl<T> Clone for EventSender<T> {
    fn clone(&self) -> Self {
        self.0.senders.fetch_add(1, Ordering::Relaxed);
        Self(self.0.clone())
    }
}

impl<T> EventSender<T> {
    /// Returns false after receiver disposal. Producers never block on consumers.
    pub fn send(&self, value: T) -> bool {
        let mut discarded = VecDeque::new();
        {
            let mut state = lock(&self.0.state);
            if state.closed {
                return false;
            }
            let Some(sequence) = state.sequence.checked_add(1) else {
                state.closed = true;
                drop(state);
                self.0.wake.wake();
                return false;
            };
            state.sequence = sequence;
            if state.queue.len() == self.0.capacity {
                std::mem::swap(&mut discarded, &mut state.queue);
                state.overflow = Some(sequence);
                state.announce_overflow = true;
            }
            state.queue.push_back(EventEnvelope {
                session: self.0.session,
                sequence,
                observed_at: SystemTime::now(),
                payload: Event::Value(value),
            });
        }
        drop(discarded);
        self.0.wake.wake();
        true
    }

    pub fn revision(&self) -> u64 {
        lock(&self.0.state).sequence
    }
}

impl<T> Drop for EventSender<T> {
    fn drop(&mut self) {
        if self.0.senders.fetch_sub(1, Ordering::AcqRel) == 1 {
            lock(&self.0.state).closed = true;
            self.0.wake.wake();
        }
    }
}

impl<T> EventSubscription<T> {
    pub fn snapshot_required(&self) -> bool {
        lock(&self.shared.state).overflow.is_some()
    }

    /// A snapshot must describe this service session through `sequence`.
    /// A later overflow is retained so concurrent producers cannot hide loss.
    pub fn acknowledge_snapshot(&self, sequence: u64) {
        let mut discarded = Vec::new();
        let mut state = lock(&self.shared.state);
        if state.overflow.is_some_and(|overflow| overflow <= sequence) {
            state.overflow = None;
            state.announce_overflow = false;
        }
        while state
            .queue
            .front()
            .is_some_and(|event| event.sequence <= sequence)
        {
            discarded.push(state.queue.pop_front());
        }
        drop(state);
        drop(discarded);
    }

    pub fn try_next(&self) -> Result<NextEvent<T>, SystemError> {
        self.shared
            .waiting
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .map_err(|_| SystemError::new(ErrorCode::Busy, "An event reader is already pending"))?;
        Ok(NextEvent {
            shared: self.shared.clone(),
            finished: false,
        })
    }
}

impl<T> Drop for EventSubscription<T> {
    fn drop(&mut self) {
        let discarded = {
            let mut state = lock(&self.shared.state);
            state.closed = true;
            state.announce_overflow = false;
            std::mem::take(&mut state.queue)
        };
        self.shared.wake.wake();
        drop(discarded);
        if let Some(cleanup) = self.cleanup.take() {
            cleanup();
        }
    }
}

pub struct NextEvent<T> {
    shared: Arc<Shared<T>>,
    finished: bool,
}

impl<T> Unpin for NextEvent<T> {}

impl<T> Future for NextEvent<T> {
    type Output = Option<EventEnvelope<T>>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        if self.finished {
            return Poll::Pending;
        }
        self.shared.wake.register(cx.waker());
        let mut state = lock(&self.shared.state);
        let result = if state.announce_overflow {
            state.announce_overflow = false;
            Poll::Ready(Some(EventEnvelope {
                session: self.shared.session,
                sequence: state.overflow.unwrap_or(state.sequence),
                observed_at: SystemTime::now(),
                payload: Event::ResyncRequired,
            }))
        } else if let Some(event) = state.queue.pop_front() {
            Poll::Ready(Some(event))
        } else if state.closed {
            Poll::Ready(None)
        } else {
            Poll::Pending
        };
        drop(state);
        if result.is_ready() {
            self.finished = true;
            self.shared.waiting.store(false, Ordering::Release);
        }
        result
    }
}

impl<T> Drop for NextEvent<T> {
    fn drop(&mut self) {
        if !self.finished {
            self.shared.waiting.store(false, Ordering::Release);
        }
    }
}
