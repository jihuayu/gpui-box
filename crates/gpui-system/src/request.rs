use crate::context::{WakeCell, lock};
use crate::{Completion, DiagnosticEvent, DiagnosticPhase, OperationContext, ServiceResult};
use std::{
    future::Future,
    marker::PhantomData,
    pin::Pin,
    rc::Rc,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
    task::{Context, Poll},
    time::Instant,
};

static NEXT_REQUEST: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RequestId(pub u64);

struct State<T> {
    result: Option<ServiceResult<T>>,
    terminal: bool,
}

struct Shared<T> {
    id: RequestId,
    context: OperationContext,
    state: Mutex<State<T>>,
    wake: Arc<WakeCell>,
}

/// Receiver of exactly one completion. Drop abandons this request without
/// cancelling siblings that share its cancellation token. No blocking destructor.
/// The receiver is local even when native callback payloads are Send.
///
/// ```compile_fail
/// fn assert_send<T: Send>() {}
/// assert_send::<gpui_system::Request<()>>();
/// ```
pub struct Request<T>(Arc<Shared<T>>, PhantomData<Rc<()>>);

/// Cloneable, thread-safe when `T: Send`, native callback completion port.
pub struct CompletionPort<T>(Arc<Shared<T>>);

impl<T> Clone for CompletionPort<T> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

pub fn request<T>(context: OperationContext) -> (Request<T>, CompletionPort<T>) {
    let shared = Arc::new(Shared {
        id: RequestId(NEXT_REQUEST.fetch_add(1, Ordering::Relaxed)),
        context,
        state: Mutex::new(State {
            result: None,
            terminal: false,
        }),
        wake: Arc::default(),
    });
    shared.context.watch(&shared.wake);
    (Request(shared.clone(), PhantomData), CompletionPort(shared))
}

fn interruption<T>(
    context: &OperationContext,
    validation: Result<(), crate::SystemError>,
) -> Option<ServiceResult<T>> {
    if let Err(error) = validation {
        return Some(Err(error));
    }
    context
        .cancellation
        .reason()
        .map(|reason| Ok(Completion::Cancelled(reason)))
}

impl<T> CompletionPort<T> {
    pub fn id(&self) -> RequestId {
        self.0.id
    }

    /// First callback wins. Expired owners, cancellation and deadlines take
    /// precedence when observed before native completion is accepted.
    pub fn complete(&self, result: ServiceResult<T>) -> bool {
        let result = interruption(
            &self.0.context,
            self.0.context.check_lifetime_at(Instant::now()),
        )
        .unwrap_or(result);
        let accepted = {
            let mut state = lock(&self.0.state);
            if state.terminal || state.result.is_some() {
                return false;
            }
            state.result = Some(result);
            true
        };
        self.0.wake.wake();
        accepted
    }

    /// Runtime timer hook. Core owns no threads or event loop. Hosts must
    /// schedule this at the deadline, or use Request::with_deadline_signal.
    pub fn expire_at(&self, now: Instant) {
        let changed = {
            let mut state = lock(&self.0.state);
            if state.terminal || state.result.is_some() {
                false
            } else if let Some(result) =
                interruption(&self.0.context, self.0.context.check_lifetime_at(now))
            {
                state.result = Some(result);
                true
            } else {
                false
            }
        };
        if changed {
            self.0.wake.wake();
        }
    }

    pub fn diagnostic(&self, phase: DiagnosticPhase) -> DiagnosticEvent {
        DiagnosticEvent {
            request: self.id(),
            phase,
            code: None,
        }
    }
}

impl<T> Request<T> {
    pub fn id(&self) -> RequestId {
        self.0.id
    }
    pub fn deadline(&self) -> Option<Instant> {
        self.0.context.deadline
    }

    /// Supply a runtime-owned future which becomes ready at the context deadline.
    /// The returned local future also wakes for cancellation and owner disposal.
    /// The signal must not complete early. The GPUI adapter supplies its timer.
    pub async fn with_deadline_signal(self, signal: impl Future<Output = ()>) -> ServiceResult<T> {
        let mut request = self;
        let mut signal = std::pin::pin!(signal);
        std::future::poll_fn(move |cx| {
            if let Poll::Ready(result) = Pin::new(&mut request).poll(cx) {
                return Poll::Ready(result);
            }
            if signal.as_mut().poll(cx).is_ready() {
                let port = CompletionPort(request.0.clone());
                port.expire_at(Instant::now());
                return Pin::new(&mut request).poll(cx);
            }
            Poll::Pending
        })
        .await
    }
}

impl<T> Future for Request<T> {
    type Output = ServiceResult<T>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        self.0.wake.register(cx.waker());
        let mut state = lock(&self.0.state);
        if state.terminal {
            return Poll::Pending;
        }
        // Revalidate on delivery, including a callback accepted before recreation.
        let discarded = interruption(&self.0.context, self.0.context.check_at(Instant::now()))
            .and_then(|result| state.result.replace(result));
        if let Some(result) = state.result.take() {
            state.terminal = true;
            drop(state);
            drop(discarded);
            Poll::Ready(result)
        } else {
            drop(state);
            drop(discarded);
            Poll::Pending
        }
    }
}

impl<T> Drop for Request<T> {
    fn drop(&mut self) {
        let result = {
            let mut state = lock(&self.0.state);
            state.terminal = true;
            state.result.take()
        };
        drop(result);
    }
}
