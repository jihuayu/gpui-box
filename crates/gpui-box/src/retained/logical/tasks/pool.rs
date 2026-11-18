//! Admission spans declared work and retained output. Cancellation does not
//! release a running worker's slot until it drops its completion capability.
use super::*;
use std::sync::Weak;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UiTaskPoolStats {
    pub capacity: usize,
    pub in_use: usize,
    pub peak: usize,
    pub admitted: u64,
    pub rejected: u64,
}
struct PoolState {
    stats: UiTaskPoolStats,
    receiver: bool,
    waiter: Option<Waker>,
}
struct Pool(Mutex<PoolState>);
/// Clone and share across window/component owners to enforce one admission
/// limit. This creates no executor, queue or background thread. Slots include
/// unadopted declarations, running/cancelled workers and retained Ready outputs.
#[derive(Clone)]
pub struct UiTaskPool(Arc<Pool>);
impl UiTaskPool {
    pub fn new(capacity: usize) -> Result<Self, Error> {
        if capacity == 0 {
            return Err(Error::InvalidTaskPool);
        }
        Ok(Self(Arc::new(Pool(Mutex::new(PoolState {
            stats: UiTaskPoolStats {
                capacity,
                in_use: 0,
                peak: 0,
                admitted: 0,
                rejected: 0,
            },
            receiver: false,
            waiter: None,
        })))))
    }
    pub fn stats(&self) -> UiTaskPoolStats {
        lock(&self.0.0).stats
    }
    /// One owner may wait for released capacity. Multiple runtime/window owners
    /// can share admission, but their host must centralize retry scheduling.
    pub fn open_capacity(&self) -> Result<UiTaskCapacity, Error> {
        let mut state = lock(&self.0.0);
        if state.receiver {
            return Err(Error::InvalidTaskPool);
        }
        state.receiver = true;
        Ok(UiTaskCapacity(Arc::downgrade(&self.0)))
    }
    pub(super) fn reserve(&self) -> Result<Permit, Error> {
        let mut state = lock(&self.0.0);
        if state.stats.in_use >= state.stats.capacity {
            state.stats.rejected = state.stats.rejected.saturating_add(1);
            return Err(Error::TaskCapacity);
        }
        state.stats.in_use += 1;
        state.stats.peak = state.stats.peak.max(state.stats.in_use);
        state.stats.admitted = state.stats.admitted.saturating_add(1);
        Ok(Permit(self.0.clone()))
    }
}
impl Default for UiTaskPool {
    fn default() -> Self {
        Self::new(64).expect("positive default task capacity")
    }
}
/// Capacity observation is not a reservation. Another owner can acquire the
/// slot before retry. Ready yields None after the pool has no remaining owners.
/// Dropping this receiver allows another to open; it owns no pool capacity.
pub struct UiTaskCapacity(Weak<Pool>);
impl UiTaskCapacity {
    pub fn ready(&mut self) -> impl Future<Output = Option<UiTaskPoolStats>> + '_ {
        std::future::poll_fn(|cx| {
            let next = cx.waker().clone();
            let Some(pool) = self.0.upgrade() else {
                return Poll::Ready(None);
            };
            let (output, previous) = {
                let mut state = lock(&pool.0);
                if state.stats.in_use < state.stats.capacity {
                    (Some(state.stats), None)
                } else {
                    (None, state.waiter.replace(next))
                }
            };
            drop(previous);
            output.map_or(Poll::Pending, |stats| Poll::Ready(Some(stats)))
        })
    }
}
impl Drop for UiTaskCapacity {
    fn drop(&mut self) {
        if let Some(pool) = self.0.upgrade() {
            let waiter = {
                let mut state = lock(&pool.0);
                state.receiver = false;
                state.waiter.take()
            };
            drop(waiter);
        }
    }
}
impl Drop for Pool {
    fn drop(&mut self) {
        let waiter = lock(&self.0).waiter.take();
        wake(waiter);
    }
}
pub(super) struct Permit(Arc<Pool>);
impl Drop for Permit {
    fn drop(&mut self) {
        let waiter = {
            let mut state = lock(&self.0.0);
            state.stats.in_use -= 1;
            state.waiter.take()
        };
        wake(waiter);
    }
}
