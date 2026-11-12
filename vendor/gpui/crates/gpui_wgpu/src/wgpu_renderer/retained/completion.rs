//! Native completion waits off the UI thread. One pending newest submission and
//! one completion event are enough: completion serials cover every older lease.
use super::wgpu;
use std::{
    future::Future,
    pin::Pin,
    sync::{
        Arc, Condvar, Mutex,
        atomic::{AtomicU64, Ordering},
    },
    task::{Context, Poll, Waker},
    time::{Duration, Instant},
};

/// Native wait completion observation. Covered serials completed no later than
/// this instant; this is not a GPU timestamp, Present or display completion.
#[derive(Clone, Copy, Debug)]
pub struct CompletionReceipt {
    pub serial: u64,
    pub observed_at: Instant,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CompletionStats {
    pub scheduled: u64,
    pub coalesced: u64,
    pub waits: u64,
    pub notifications: u64,
    pub delivered: u64,
    pub closed: bool,
}
#[derive(Default)]
struct State {
    job: Option<(u64, wgpu::SubmissionIndex)>,
    event: Option<Result<CompletionReceipt, String>>,
    waker: Option<Waker>,
    stats: CompletionStats,
}
#[derive(Default)]
struct Shared {
    state: Mutex<State>,
    changed: Condvar,
}
impl Shared {
    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|error| error.into_inner())
    }
    fn close(&self) {
        let (job, event, waker) = {
            let mut state = self.lock();
            state.stats.closed = true;
            (state.job.take(), state.event.take(), state.waker.take())
        };
        self.changed.notify_one();
        drop((job, event));
        if let Some(waker) = waker {
            waker.wake();
        }
    }
    fn publish(&self, event: Result<u64, String>) {
        let event = event.map(|serial| CompletionReceipt {
            serial,
            observed_at: Instant::now(),
        });
        let waker = {
            let mut state = self.lock();
            if state.stats.closed {
                return;
            }
            if event.is_err() {
                state.stats.closed = true;
                state.job = None;
            }
            state.event = Some(event);
            let waker = state.waker.take();
            state.stats.notifications += u64::from(waker.is_some());
            waker
        };
        if let Some(waker) = waker {
            waker.wake();
        }
    }
}

/// A single host consumer. Await an event, then call the renderer's nonblocking
/// `poll` on its owner to release completed leases. No redraw is needed. Dropping
/// this receiver cancels pending jobs and wakes an idle worker to exit.
pub struct CompletionReceiver {
    shared: Arc<Shared>,
}
impl CompletionReceiver {
    pub async fn ready(&mut self) -> Result<Option<u64>, String> {
        self.ready_receipt()
            .await
            .map(|receipt| receipt.map(|r| r.serial))
    }
    /// Newest receipt covers all earlier submissions. Coalescing deliberately
    /// loses per-submit observation times; use its timestamp as an upper bound.
    pub fn ready_receipt(
        &mut self,
    ) -> impl Future<Output = Result<Option<CompletionReceipt>, String>> + '_ {
        Ready { receiver: self }
    }
    pub fn stats(&self) -> CompletionStats {
        self.shared.lock().stats
    }
}
impl Drop for CompletionReceiver {
    fn drop(&mut self) {
        self.shared.close();
    }
}
struct Ready<'a> {
    receiver: &'a mut CompletionReceiver,
}
impl Future for Ready<'_> {
    type Output = Result<Option<CompletionReceipt>, String>;
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let mut state = self.receiver.shared.lock();
        if let Some(event) = state.event.take() {
            state.stats.delivered += 1;
            return Poll::Ready(event.map(Some));
        }
        if state.stats.closed {
            return Poll::Ready(Ok(None));
        }
        if state
            .waker
            .as_ref()
            .is_none_or(|w| !w.will_wake(cx.waker()))
        {
            state.waker = Some(cx.waker().clone());
        }
        Poll::Pending
    }
}

pub(super) struct CompletionDriver {
    shared: Arc<Shared>,
}
impl CompletionDriver {
    pub(super) fn new(
        device: Arc<wgpu::Device>,
        completed: Arc<AtomicU64>,
    ) -> std::io::Result<(Self, CompletionReceiver)> {
        let shared = Arc::new(Shared::default());
        let worker = shared.clone();
        std::thread::Builder::new()
            .name("retained-gpu-completion".into())
            .spawn(move || {
                loop {
                    let job = {
                        let mut state = worker.lock();
                        while state.job.is_none() && !state.stats.closed {
                            state = worker
                                .changed
                                .wait(state)
                                .unwrap_or_else(|e| e.into_inner());
                        }
                        if state.stats.closed {
                            break;
                        }
                        state.stats.waits += 1;
                        state.job.take().expect("scheduled completion")
                    };
                    // WGPU needs an explicit native poll to dispatch callbacks.
                    // The wait blocks only this worker, and only with queued work.
                    let result = device
                        .poll(wgpu::PollType::Wait {
                            submission_index: Some(job.1),
                            timeout: Some(Duration::from_secs(30)),
                        })
                        .map(|_| {
                            // Another concurrent poll can take callbacks before
                            // firing them. The successful native wait itself
                            // proves completion, independently of that callback.
                            completed.fetch_max(job.0, Ordering::Release);
                            job.0
                        })
                        .map_err(|error| error.to_string());
                    let failed = result.is_err();
                    worker.publish(result);
                    if failed {
                        break;
                    }
                }
            })?;
        Ok((
            Self {
                shared: shared.clone(),
            },
            CompletionReceiver { shared },
        ))
    }
    pub(super) fn closed(&self) -> bool {
        self.shared.lock().stats.closed
    }
    pub(super) fn stats(&self) -> CompletionStats {
        self.shared.lock().stats
    }
    pub(super) fn schedule(&self, serial: u64, submission: wgpu::SubmissionIndex) {
        let mut state = self.shared.lock();
        if state.stats.closed {
            return;
        }
        state.stats.scheduled += 1;
        state.stats.coalesced += u64::from(state.job.replace((serial, submission)).is_some());
        self.shared.changed.notify_one();
    }
}
impl Drop for CompletionDriver {
    fn drop(&mut self) {
        // Never join a driver wait on the UI thread. An active native wait can
        // retain its device clone until completion or the bounded error timeout.
        self.shared.close();
    }
}

#[cfg(test)]
#[path = "completion/tests.rs"]
mod tests;
