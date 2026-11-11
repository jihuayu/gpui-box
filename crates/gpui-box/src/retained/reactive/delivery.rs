//! Bounded latest-value source delivery. Mutable UI state never crosses threads.
use super::*;
use std::{
    future::Future,
    pin::Pin,
    sync::{Mutex, Weak},
    task::{Context, Poll, Waker},
};

trait Pending: Send {
    fn as_any(&self) -> &dyn Any;
    fn stage(self: Box<Self>, tx: &mut ReactiveTransaction<'_>) -> Result<(), Error>;
}
struct Write<T> {
    signal: Signal<T>,
    value: T,
}
impl<T: PartialEq + Send + 'static> Pending for Write<T> {
    fn as_any(&self) -> &dyn Any {
        &self.value
    }
    fn stage(self: Box<Self>, tx: &mut ReactiveTransaction<'_>) -> Result<(), Error> {
        tx.set(self.signal, self.value)?;
        Ok(())
    }
}
struct Entry {
    lifetime: Weak<()>,
    write: Box<dyn Pending>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct UpdateQueueStats {
    pub enqueued: u64,
    pub coalesced: u64,
    pub equal_pending: u64,
    pub wake_transitions: u64,
    pub waker_notifications: u64,
    pub drains: u64,
    pub applied: u64,
    pub retired: u64,
    pub rejected: u64,
    pub pending: usize,
    pub closed: bool,
}
#[derive(Default)]
struct State {
    pending: BTreeMap<SourceId, Entry>,
    waker: Option<Waker>,
    stats: UpdateQueueStats,
}
pub(super) struct Shared {
    owner: u64,
    capacity: usize,
    state: Mutex<State>,
}
impl Shared {
    pub(super) fn close(&self) {
        // Closing is also used by Drop. If another thread poisoned the mutex,
        // still release payloads and wake the waiting owner to terminate.
        let (pending, waker) = {
            let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
            state.stats.closed = true;
            state.stats.pending = 0;
            (std::mem::take(&mut state.pending), state.waker.take())
        };
        drop(pending);
        if let Some(waker) = waker {
            waker.wake();
        }
    }
    fn stats(&self) -> Result<UpdateQueueStats, Error> {
        Ok(self
            .state
            .lock()
            .map_err(|_| Error::UpdateQueuePoisoned)?
            .stats)
    }
}

/// Cloneable producer endpoint for a validated writable source. Only owned Send
/// values cross threads; no callback, Rc, or mutable UI owner is shared. This is
/// a last-value visual channel, not an ordered business/lifecycle command queue.
pub struct UpdateSender<T> {
    shared: Arc<Shared>,
    lifetime: Weak<()>,
    signal: Signal<T>,
}
impl<T> Clone for UpdateSender<T> {
    fn clone(&self) -> Self {
        Self {
            shared: self.shared.clone(),
            lifetime: self.lifetime.clone(),
            signal: self.signal,
        }
    }
}
impl<T: PartialEq + Send + 'static> UpdateSender<T> {
    /// Return true if pending input changed; equality against the committed
    /// source is checked later on the owner. Only empty→pending wakes a waiter.
    pub fn send(&self, value: T) -> Result<bool, Error> {
        let lifetime = self.lifetime.upgrade().ok_or(Error::InvalidSignal)?;
        let (waker, replaced) = {
            let mut state = self
                .shared
                .state
                .lock()
                .map_err(|_| Error::UpdateQueuePoisoned)?;
            if state.stats.closed {
                return Err(Error::UpdateQueueClosed);
            }
            if state.pending.get(&self.signal.id).is_some_and(|entry| {
                entry
                    .write
                    .as_any()
                    .downcast_ref::<T>()
                    .is_some_and(|pending| *pending == value)
            }) {
                state.stats.equal_pending += 1;
                return Ok(false);
            }
            if !state.pending.contains_key(&self.signal.id)
                && state.pending.len() == self.shared.capacity
            {
                return Err(Error::UpdateQueueFull);
            }
            let wake = state.pending.is_empty();
            let replaced = state.pending.insert(
                self.signal.id,
                Entry {
                    lifetime: Arc::downgrade(&lifetime),
                    write: Box::new(Write {
                        signal: self.signal,
                        value,
                    }),
                },
            );
            state.stats.enqueued += 1;
            state.stats.coalesced += u64::from(replaced.is_some());
            state.stats.pending = state.pending.len();
            let waker = if wake {
                state.stats.wake_transitions += 1;
                state.waker.take()
            } else {
                None
            };
            state.stats.waker_notifications += u64::from(waker.is_some());
            (waker, replaced)
        };
        // Dropping user values and invoking executor wake never holds the lock.
        drop(replaced);
        if let Some(waker) = waker {
            waker.wake();
        }
        Ok(true)
    }
    pub fn stats(&self) -> Result<UpdateQueueStats, Error> {
        self.shared.stats()
    }
}

/// Single UI-owner consumer. `ready().await` integrates with a host executor's
/// wake mechanism, with no polling timer. Drop closes delivery and releases
/// pending values even while cloned producers remain alive.
pub struct UpdateReceiver {
    shared: Arc<Shared>,
}
impl UpdateReceiver {
    pub fn ready(&mut self) -> impl Future<Output = Result<bool, Error>> + '_ {
        Ready { receiver: self }
    }
    pub fn stats(&self) -> Result<UpdateQueueStats, Error> {
        self.shared.stats()
    }
}
impl Drop for UpdateReceiver {
    fn drop(&mut self) {
        self.shared.close();
    }
}
struct Ready<'a> {
    receiver: &'a mut UpdateReceiver,
}
impl Future for Ready<'_> {
    type Output = Result<bool, Error>;
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let mut state = match self.receiver.shared.state.lock() {
            Ok(state) => state,
            Err(_) => return Poll::Ready(Err(Error::UpdateQueuePoisoned)),
        };
        if state.stats.closed {
            return Poll::Ready(Ok(false));
        }
        if !state.pending.is_empty() {
            return Poll::Ready(Ok(true));
        }
        if state
            .waker
            .as_ref()
            .is_none_or(|waker| !waker.will_wake(cx.waker()))
        {
            state.waker = Some(cx.waker().clone());
        }
        Poll::Pending
    }
}

pub struct UpdateBatch {
    /// Empty/retired-only drains produce no reactive transaction.
    pub commit: Option<ReactiveCommit<()>>,
    pub applied: usize,
    pub retired: usize,
}
impl ReactiveUi {
    /// Open one live inbox per runtime. Capacity bounds distinct pending source
    /// values, not payload bytes. A replacement never needs an extra slot.
    pub fn open_updates(&mut self, capacity: usize) -> Result<UpdateReceiver, Error> {
        if capacity == 0 {
            return Err(Error::InvalidUpdateQueue);
        }
        if let Some(shared) = self.delivery.as_ref().and_then(Weak::upgrade)
            && !shared.stats()?.closed
        {
            return Err(Error::InvalidUpdateQueue);
        }
        let shared = Arc::new(Shared {
            owner: self.identity,
            capacity,
            state: Mutex::new(State::default()),
        });
        self.delivery = Some(Arc::downgrade(&shared));
        Ok(UpdateReceiver { shared })
    }
    pub fn update_sender<T: PartialEq + Send + 'static>(
        &mut self,
        receiver: &UpdateReceiver,
        signal: Signal<T>,
    ) -> Result<UpdateSender<T>, Error> {
        if receiver.shared.owner != self.identity {
            return Err(Error::InvalidUpdateQueue);
        }
        if receiver.stats()?.closed {
            return Err(Error::UpdateQueueClosed);
        }
        self.read(signal)?;
        if self.source(signal.id)?.computation.is_some() {
            return Err(Error::ReadonlySignal);
        }
        let source = self.sources[signal.id.slot].source.as_mut().unwrap();
        let lifetime = source.delivery_lifetime.get_or_insert_with(|| Arc::new(()));
        Ok(UpdateSender {
            shared: receiver.shared.clone(),
            lifetime: Arc::downgrade(lifetime),
            signal,
        })
    }
    /// Drain once and publish live final values in one transaction. Retired
    /// generations are dropped, never rebound. Invalid compute/sink batches are
    /// rejected atomically and consumed (no automatic busy-loop retry).
    pub fn apply_updates(&mut self, receiver: &mut UpdateReceiver) -> Result<UpdateBatch, Error> {
        if receiver.shared.owner != self.identity {
            return Err(Error::InvalidUpdateQueue);
        }
        let pending = {
            let mut state = receiver
                .shared
                .state
                .lock()
                .map_err(|_| Error::UpdateQueuePoisoned)?;
            if state.stats.closed {
                return Err(Error::UpdateQueueClosed);
            }
            if state.pending.is_empty() {
                return Ok(UpdateBatch {
                    commit: None,
                    applied: 0,
                    retired: 0,
                });
            }
            state.stats.drains += 1;
            state.stats.pending = 0;
            std::mem::take(&mut state.pending)
        };
        let (live, dead): (Vec<_>, Vec<_>) = pending
            .into_iter()
            .partition(|(id, entry)| entry.lifetime.strong_count() > 0 && self.source(*id).is_ok());
        let applied = live.len();
        let retired = dead.len();
        let result = if live.is_empty() {
            Ok(None)
        } else {
            self.transaction(|tx| {
                for (_, entry) in live {
                    entry.write.stage(tx)?;
                }
                Ok(())
            })
            .map(Some)
        };
        let mut state = receiver
            .shared
            .state
            .lock()
            .map_err(|_| Error::UpdateQueuePoisoned)?;
        state.stats.retired += retired as u64;
        if result.is_ok() {
            state.stats.applied += applied as u64;
        } else {
            state.stats.rejected += applied as u64;
        }
        result.map(|commit| UpdateBatch {
            commit,
            applied,
            retired,
        })
    }
}

#[cfg(test)]
mod tests;
