//! One business store publishes whole revisions. UI owners consume snapshots;
//! selectors, UI callbacks and rendering never run under the publication lock.
use std::{
    collections::BTreeMap,
    future::Future,
    pin::Pin,
    sync::{
        Arc, Mutex, Weak,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    task::{Context, Poll, Waker},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StoreError {
    Poisoned,
    Closed,
    ForeignRevision,
    StaleRevision,
    RevisionExhausted,
}

/// Store identity and monotonically increasing commit number, unrelated to UI,
/// scene or displayed frame revisions. Revisions from other stores never match.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StoreRevision {
    owner: u64,
    number: u64,
}
impl StoreRevision {
    pub fn number(self) -> u64 {
        self.number
    }
    pub fn same_store(self, other: Self) -> bool {
        self.owner == other.owner
    }
}

/// Immutable owned state. Equality is store/revision identity, not a deep state
/// comparison. State must not be mutated via interior mutability after commit.
pub struct StoreSnapshot<S> {
    revision: StoreRevision,
    state: Arc<S>,
}
impl<S: std::fmt::Debug> std::fmt::Debug for StoreSnapshot<S> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StoreSnapshot")
            .field("revision", &self.revision)
            .field("state", &self.state)
            .finish()
    }
}
impl<S> Clone for StoreSnapshot<S> {
    fn clone(&self) -> Self {
        Self {
            revision: self.revision,
            state: self.state.clone(),
        }
    }
}
impl<S> PartialEq for StoreSnapshot<S> {
    fn eq(&self, other: &Self) -> bool {
        self.revision == other.revision
    }
}
impl<S> Eq for StoreSnapshot<S> {}
impl<S> StoreSnapshot<S> {
    pub fn revision(&self) -> StoreRevision {
        self.revision
    }
    pub fn state(&self) -> &S {
        &self.state
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StoreCommit {
    pub revision: StoreRevision,
    pub changed: bool,
}
struct Notice {
    waker: Mutex<Option<Waker>>,
    cancelled: AtomicBool,
}
impl Notice {
    fn wake(&self) {
        let waker = self.waker.lock().unwrap_or_else(|e| e.into_inner()).take();
        if let Some(waker) = waker {
            waker.wake();
        }
    }
}
struct State<S> {
    snapshot: StoreSnapshot<S>,
    closed: bool,
    subscribers: BTreeMap<u64, Weak<Notice>>,
    next_subscriber: u64,
}
struct Shared<S> {
    state: Mutex<State<S>>,
}
impl<S> Shared<S> {
    fn lock(&self) -> Result<std::sync::MutexGuard<'_, State<S>>, StoreError> {
        self.state.lock().map_err(|_| StoreError::Poisoned)
    }
}
fn notices<S>(state: &mut State<S>) -> Vec<Arc<Notice>> {
    let mut live = vec![];
    state.subscribers.retain(|_, weak| {
        if let Some(notice) = weak
            .upgrade()
            .filter(|n| !n.cancelled.load(Ordering::Acquire))
        {
            live.push(notice);
            true
        } else {
            false
        }
    });
    live
}

/// Unique business writer. Move it to the application executor as needed; it is
/// deliberately not Clone. Build/validate replacement state before publication.
pub struct StoreWriter<S> {
    shared: Arc<Shared<S>>,
}
impl<S: PartialEq + Send + Sync + 'static> StoreWriter<S> {
    pub fn new(initial: S) -> Result<Self, StoreError> {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        let owner = NEXT
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |n| n.checked_add(1))
            .map_err(|_| StoreError::RevisionExhausted)?;
        Ok(Self {
            shared: Arc::new(Shared {
                state: Mutex::new(State {
                    snapshot: StoreSnapshot {
                        revision: StoreRevision { owner, number: 0 },
                        state: Arc::new(initial),
                    },
                    closed: false,
                    subscribers: BTreeMap::new(),
                    next_subscriber: 0,
                }),
            }),
        })
    }
    pub fn read_only(&self) -> ReadStore<S> {
        ReadStore {
            shared: self.shared.clone(),
        }
    }
    /// Optimistic commit. Compare/prepare outside the lock; a stale expected
    /// revision rejects even if next happens to equal the current state.
    pub fn commit(&mut self, expected: StoreRevision, next: S) -> Result<StoreCommit, StoreError> {
        let before = self.read_only().snapshot()?;
        if expected.owner != before.revision.owner {
            return Err(StoreError::ForeignRevision);
        }
        if expected != before.revision {
            return Err(StoreError::StaleRevision);
        }
        if *before.state == next {
            return Ok(StoreCommit {
                revision: expected,
                changed: false,
            });
        }
        let revision = StoreRevision {
            owner: expected.owner,
            number: expected
                .number
                .checked_add(1)
                .ok_or(StoreError::RevisionExhausted)?,
        };
        let snapshot = StoreSnapshot {
            revision,
            state: Arc::new(next),
        };
        let (old, live) = {
            let mut state = self.shared.lock()?;
            if state.closed {
                return Err(StoreError::Closed);
            }
            let old = std::mem::replace(&mut state.snapshot, snapshot);
            (old, notices(&mut state))
        };
        drop(old);
        for notice in live {
            notice.wake();
        }
        Ok(StoreCommit {
            revision,
            changed: true,
        })
    }
}
impl<S> Drop for StoreWriter<S> {
    fn drop(&mut self) {
        let live = {
            let mut state = self.shared.state.lock().unwrap_or_else(|e| e.into_inner());
            state.closed = true;
            notices(&mut state)
        };
        for notice in live {
            notice.wake();
        }
    }
}

/// Cloneable read-only business endpoint. No writer or UI source conversion.
///
/// ```compile_fail
/// use gpui_box::store::ReadStore;
/// fn mutate(store: ReadStore<u32>) { store.commit(1, 2); }
/// ```
pub struct ReadStore<S> {
    shared: Arc<Shared<S>>,
}
impl<S> Clone for ReadStore<S> {
    fn clone(&self) -> Self {
        Self {
            shared: self.shared.clone(),
        }
    }
}
impl<S: Send + Sync + 'static> ReadStore<S> {
    /// Still readable after the writer closes; old snapshots outlive the store.
    pub fn snapshot(&self) -> Result<StoreSnapshot<S>, StoreError> {
        Ok(self.shared.lock()?.snapshot.clone())
    }
    /// O(1) pending storage per reader: notices never queue field patches or
    /// execute selectors. First take/ready returns the current whole snapshot.
    pub fn subscribe(&self) -> Result<StoreSubscription<S>, StoreError> {
        let notice = Arc::new(Notice {
            waker: Mutex::new(None),
            cancelled: AtomicBool::new(false),
        });
        let mut state = self.shared.lock()?;
        let id = state
            .next_subscriber
            .checked_add(1)
            .ok_or(StoreError::RevisionExhausted)?;
        state.next_subscriber = id;
        state.subscribers.insert(id, Arc::downgrade(&notice));
        Ok(StoreSubscription {
            shared: self.shared.clone(),
            notice,
            id,
            seen: None,
        })
    }
}
/// A latest-whole-revision visual observer, not a business event/command queue.
pub struct StoreSubscription<S> {
    shared: Arc<Shared<S>>,
    notice: Arc<Notice>,
    id: u64,
    seen: Option<StoreRevision>,
}
/// Internal scope-owned cancellation, without exposing a business writer or
/// requiring the mutable host subscription to live inside the component scope.
#[cfg(feature = "retained")]
pub(crate) struct StoreCancellation<S> {
    shared: Weak<Shared<S>>,
    notice: Weak<Notice>,
    id: u64,
}
#[cfg(feature = "retained")]
impl<S> Drop for StoreCancellation<S> {
    fn drop(&mut self) {
        if let (Some(shared), Some(notice)) = (self.shared.upgrade(), self.notice.upgrade()) {
            notice.cancelled.store(true, Ordering::Release);
            shared
                .state
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .subscribers
                .remove(&self.id);
            notice.wake();
        }
    }
}
impl<S: Send + Sync + 'static> StoreSubscription<S> {
    #[cfg(feature = "retained")]
    pub(crate) fn scope_cancellation(&self) -> StoreCancellation<S> {
        StoreCancellation {
            shared: Arc::downgrade(&self.shared),
            notice: Arc::downgrade(&self.notice),
            id: self.id,
        }
    }
    pub fn pending(&self) -> Result<Option<StoreSnapshot<S>>, StoreError> {
        if self.notice.cancelled.load(Ordering::Acquire) {
            return Err(StoreError::Closed);
        }
        let state = self.shared.lock()?;
        Ok((self.seen != Some(state.snapshot.revision)).then(|| state.snapshot.clone()))
    }
    /// Acknowledge only the revision actually consumed. A newer commit arriving
    /// during UI work stays pending; a failed UI candidate need not acknowledge.
    pub fn acknowledge(&mut self, revision: StoreRevision) -> Result<(), StoreError> {
        if self.notice.cancelled.load(Ordering::Acquire) {
            return Err(StoreError::Closed);
        }
        let state = self.shared.lock()?;
        if revision.owner != state.snapshot.revision.owner {
            return Err(StoreError::ForeignRevision);
        }
        if revision.number > state.snapshot.revision.number
            || self.seen.is_some_and(|s| s.number > revision.number)
        {
            return Err(StoreError::StaleRevision);
        }
        self.seen = Some(revision);
        Ok(())
    }
    pub fn take_latest(&mut self) -> Result<Option<StoreSnapshot<S>>, StoreError> {
        let snapshot = self.pending()?;
        if let Some(snapshot) = &snapshot {
            self.acknowledge(snapshot.revision)?;
        }
        Ok(snapshot)
    }
    pub fn ready(&mut self) -> impl Future<Output = Result<bool, StoreError>> + '_ {
        Ready { subscription: self }
    }
    pub fn cancel(&self) {
        self.notice.cancelled.store(true, Ordering::Release);
        self.shared
            .state
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .subscribers
            .remove(&self.id);
        self.notice.wake();
    }
}
impl<S> Drop for StoreSubscription<S> {
    fn drop(&mut self) {
        self.notice.cancelled.store(true, Ordering::Release);
        self.shared
            .state
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .subscribers
            .remove(&self.id);
        self.notice.wake();
    }
}
struct Ready<'a, S> {
    subscription: &'a mut StoreSubscription<S>,
}
impl<S: Send + Sync + 'static> Future for Ready<'_, S> {
    type Output = Result<bool, StoreError>;
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let subscription = &self.subscription;
        // RawWaker clone/drop can run executor code; keep it outside both locks.
        let next_waker = cx.waker().clone();
        if subscription.notice.cancelled.load(Ordering::Acquire) {
            return Poll::Ready(Ok(false));
        }
        let state = match subscription.shared.lock() {
            Ok(s) => s,
            Err(e) => return Poll::Ready(Err(e)),
        };
        if subscription.notice.cancelled.load(Ordering::Acquire) {
            return Poll::Ready(Ok(false));
        }
        if subscription.seen != Some(state.snapshot.revision) {
            return Poll::Ready(Ok(true));
        }
        if state.closed {
            return Poll::Ready(Ok(false));
        }
        let mut waker = subscription
            .notice
            .waker
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let previous = if waker.as_ref().is_none_or(|w| !w.will_wake(&next_waker)) {
            waker.replace(next_waker)
        } else {
            Some(next_waker)
        };
        drop(waker);
        drop(state);
        drop(previous);
        Poll::Pending
    }
}

#[cfg(test)]
mod tests;
