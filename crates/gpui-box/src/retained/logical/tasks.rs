//! Scope-owned UI work. Executors stay host-owned; only complete adopted edits
//! launch work, and only the current keyed generation may apply its result.
use super::*;
use std::{
    any::Any,
    future::Future,
    marker::PhantomData,
    rc::Rc,
    sync::{Mutex, MutexGuard},
    task::{Poll, Waker},
};
mod pool;
use pool::Permit;
pub use pool::{UiTaskCapacity, UiTaskPool, UiTaskPoolStats};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UiTaskStatus {
    PendingCommit,
    Running,
    Ready,
    Applied,
    Cancelled,
    /// The launch/worker dropped its completion capability without an output.
    Abandoned,
}
struct State<T> {
    status: UiTaskStatus,
    output: Option<Arc<Output<T>>>,
    waiter: Option<Waker>,
    cancellation_waiter: Option<Waker>,
}
struct Output<T> {
    value: T,
    // Drop payload before releasing the admission slot. Other task handles own
    // no reservation after this output and the worker have both gone away.
    _permit: Permit,
}
struct Shared<T>(Mutex<State<T>>);
fn lock<T>(value: &Mutex<T>) -> MutexGuard<'_, T> {
    value.lock().unwrap_or_else(|error| error.into_inner())
}
fn wake(wakers: impl IntoIterator<Item = Waker>) {
    for waker in wakers {
        waker.wake();
    }
}
impl<T> Shared<T> {
    fn cancel(&self) {
        let (output, wakers) = {
            let mut state = lock(&self.0);
            if !matches!(
                state.status,
                UiTaskStatus::Applied | UiTaskStatus::Abandoned
            ) {
                state.status = UiTaskStatus::Cancelled;
            }
            (
                state.output.take(),
                state
                    .waiter
                    .take()
                    .into_iter()
                    .chain(state.cancellation_waiter.take())
                    .collect::<Vec<_>>(),
            )
        };
        // User values and wakers never drop or run while the mutex is held.
        drop(output);
        wake(wakers);
    }
}
/// Owner-local result capability. Clone retains observation, never task lifetime.
/// Scope retirement/replacement cancels even when this handle escapes.
///
/// ```compile_fail
/// use gpui_box::retained::logical::UiTask;
/// fn escape(task: UiTask<u32>) { std::thread::spawn(move || task.status()); }
/// ```
pub struct UiTask<T> {
    scope: ScopeId,
    key: u64,
    shared: Arc<Shared<T>>,
    owner_local: PhantomData<Rc<()>>,
}
impl<T> Clone for UiTask<T> {
    fn clone(&self) -> Self {
        Self {
            scope: self.scope,
            key: self.key,
            shared: self.shared.clone(),
            owner_local: PhantomData,
        }
    }
}
impl<T> UiTask<T> {
    pub fn status(&self) -> UiTaskStatus {
        lock(&self.shared.0).status
    }
    /// One owner waiter per task. Cancellation/abandonment also wakes it.
    pub fn ready(&mut self) -> impl Future<Output = UiTaskStatus> + '_ {
        std::future::poll_fn(|cx| {
            let next = cx.waker().clone();
            let (status, previous) = {
                let mut state = lock(&self.shared.0);
                if matches!(
                    state.status,
                    UiTaskStatus::PendingCommit | UiTaskStatus::Running
                ) {
                    (None, state.waiter.replace(next))
                } else {
                    (Some(state.status), None)
                }
            };
            drop(previous);
            status.map_or(Poll::Pending, Poll::Ready)
        })
    }
}
/// Move this into a host executor's short-lived UI task. It owns no LogicalUi,
/// callbacks, render tree or Signal writer. Cancellation is cooperative, never
/// rollback of external effects. Use business Command for saves/downloads.
///
/// ```
/// use gpui_box::retained::logical::UiTaskContext;
/// fn send_sync<T: Send + Sync>() {}
/// send_sync::<UiTaskContext<u32>>();
/// ```
pub struct UiTaskContext<T> {
    shared: Arc<Shared<T>>,
    permit: Option<Permit>,
}
impl<T> UiTaskContext<T> {
    pub fn cancellation_requested(&self) -> bool {
        lock(&self.shared.0).status == UiTaskStatus::Cancelled
    }
    /// One cancellation waiter. There is no task polling timer or private pool.
    pub fn cancelled(&mut self) -> impl Future<Output = ()> + '_ {
        std::future::poll_fn(|cx| {
            let next = cx.waker().clone();
            let (cancelled, previous) = {
                let mut state = lock(&self.shared.0);
                if state.status == UiTaskStatus::Cancelled {
                    (true, None)
                } else {
                    (false, state.cancellation_waiter.replace(next))
                }
            };
            drop(previous);
            if cancelled {
                Poll::Ready(())
            } else {
                Poll::Pending
            }
        })
    }
    /// Store one immutable output. A late cancelled completion returns its value
    /// untouched; it cannot publish UI state or replace a newer task's result.
    pub fn complete(mut self, output: T) -> Result<(), T> {
        let (waiter, cancellation_waiter) = {
            let mut state = lock(&self.shared.0);
            if state.status != UiTaskStatus::Running {
                return Err(output);
            }
            state.output = Some(Arc::new(Output {
                value: output,
                _permit: self.permit.take().expect("worker permit"),
            }));
            state.status = UiTaskStatus::Ready;
            (state.waiter.take(), state.cancellation_waiter.take())
        };
        drop(cancellation_waiter);
        wake(waiter);
        Ok(())
    }
}
impl<T> Drop for UiTaskContext<T> {
    fn drop(&mut self) {
        let (waiter, cancellation_waiter) = {
            let mut state = lock(&self.shared.0);
            if !matches!(
                state.status,
                UiTaskStatus::PendingCommit | UiTaskStatus::Running
            ) {
                return;
            }
            state.status = if state.status == UiTaskStatus::PendingCommit {
                UiTaskStatus::Cancelled
            } else {
                UiTaskStatus::Abandoned
            };
            (state.waiter.take(), state.cancellation_waiter.take())
        };
        drop(cancellation_waiter);
        wake(waiter);
    }
}
trait Erased {
    fn as_any(&self) -> &dyn Any;
}
struct Entry<T> {
    shared: Arc<Shared<T>>,
}
impl<T: 'static> Erased for Entry<T> {
    fn as_any(&self) -> &dyn Any {
        self
    }
}
impl<T> Drop for Entry<T> {
    fn drop(&mut self) {
        self.shared.cancel();
    }
}
#[derive(Clone, Default)]
pub(super) struct Registry {
    entries: BTreeMap<(ScopeId, u64), Rc<dyn Erased>>,
    pool: UiTaskPool,
}
impl Registry {
    pub(super) fn clear(&mut self) {
        self.entries.clear();
    }
    pub(super) fn retire(&mut self, dead: &BTreeSet<ScopeId>) {
        self.entries.retain(|(scope, _), _| !dead.contains(scope));
    }
    fn get<T: 'static>(&self, scope: ScopeId, key: u64) -> Result<Option<UiTask<T>>, Error> {
        self.entries
            .get(&(scope, key))
            .map(|entry| {
                let entry = entry
                    .as_any()
                    .downcast_ref::<Entry<T>>()
                    .ok_or(Error::InvalidTask)?;
                Ok(UiTask {
                    scope,
                    key,
                    shared: entry.shared.clone(),
                    owner_local: PhantomData,
                })
            })
            .transpose()
    }
    fn current<T: 'static>(&self, task: &UiTask<T>) -> bool {
        self.get::<T>(task.scope, task.key)
            .ok()
            .flatten()
            .is_some_and(|current| Arc::ptr_eq(&current.shared, &task.shared))
    }
}
pub(super) struct Launch(Option<Box<dyn FnOnce()>>);
impl Launch {
    pub(super) fn run(mut self) {
        (self.0.take().unwrap())();
    }
}
/// Only a successfully adopted result reports Applied. Pending/retired work
/// never executes the supplied source callback.
pub enum TaskApply<R> {
    Applied(ReactiveCommit<R>),
    Pending,
    Ignored(UiTaskStatus),
}
impl LogicalEdit<'_, '_, '_> {
    /// Replace this scope/key's UI task after full candidate adoption. The launch
    /// callback must only schedule work, never synchronously do expensive IO/CPU.
    /// No launch or cancellation escapes a failed candidate.
    pub fn start_task<T: Send + Sync + 'static>(
        &mut self,
        scope: ScopeId,
        key: u64,
        launch: impl FnOnce(UiTaskContext<T>) + 'static,
    ) -> Result<UiTask<T>, Error> {
        self.scope(scope)?;
        if self.retired_scopes()?.contains(&scope) {
            return Err(Error::InvalidScope);
        }
        // Busy is recoverable within a candidate: callers may publish a loading
        // state rather than poison the entire component construction.
        let permit = self
            .tasks
            .as_ref()
            .unwrap_or(self.live_tasks)
            .pool
            .reserve()?;
        let mut attempt = self.component_attempt()?;
        let shared = Arc::new(Shared(Mutex::new(State {
            status: UiTaskStatus::PendingCommit,
            output: None,
            waiter: None,
            cancellation_waiter: None,
        })));
        let task = UiTask {
            scope,
            key,
            shared: shared.clone(),
            owner_local: PhantomData,
        };
        self.tasks
            .get_or_insert_with(|| self.live_tasks.clone())
            .entries
            .insert(
                (scope, key),
                Rc::new(Entry {
                    shared: shared.clone(),
                }),
            );
        let context = UiTaskContext {
            shared: shared.clone(),
            permit: Some(permit),
        };
        self.task_launches.push(Launch(Some(Box::new(move || {
            {
                let mut state = lock(&shared.0);
                if state.status != UiTaskStatus::PendingCommit {
                    return;
                }
                state.status = UiTaskStatus::Running;
            }
            launch(context);
        }))));
        attempt.success = true;
        Ok(task)
    }
    pub fn task<T: 'static>(&self, scope: ScopeId, key: u64) -> Result<Option<UiTask<T>>, Error> {
        self.scope(scope)?;
        self.tasks
            .as_ref()
            .unwrap_or(self.live_tasks)
            .get(scope, key)
    }
    /// Cancel after successful adoption. Task keys have their own per-scope
    /// namespace, independent of source, effect and event keys.
    pub fn cancel_task(&mut self, scope: ScopeId, key: u64) -> Result<bool, Error> {
        self.scope(scope)?;
        if !self
            .tasks
            .as_ref()
            .unwrap_or(self.live_tasks)
            .entries
            .contains_key(&(scope, key))
        {
            return Ok(false);
        }
        Ok(self
            .tasks
            .get_or_insert_with(|| self.live_tasks.clone())
            .entries
            .remove(&(scope, key))
            .is_some())
    }
    pub(super) fn retire_tasks(&mut self, dead: &BTreeSet<ScopeId>) {
        if dead.is_empty() {
            return;
        }
        let registry = self.tasks.as_ref().unwrap_or(self.live_tasks);
        if registry
            .entries
            .keys()
            .any(|(scope, _)| dead.contains(scope))
        {
            self.tasks
                .get_or_insert_with(|| self.live_tasks.clone())
                .retire(dead);
        }
    }
}
impl LogicalUi {
    /// Configure admission before tasks are present/in flight. A pool may be
    /// shared across logical owners/windows. Migrating live work is rejected.
    pub fn set_task_pool(&mut self, pool: UiTaskPool) -> Result<(), Error> {
        if !self.tasks.entries.is_empty() || self.tasks.pool.stats().in_use != 0 {
            return Err(Error::InvalidTaskPool);
        }
        self.tasks.pool = pool;
        Ok(())
    }
    pub fn task_pool(&self) -> UiTaskPool {
        self.tasks.pool.clone()
    }
    pub fn start_task<T: Send + Sync + 'static>(
        &mut self,
        scope: ScopeId,
        key: u64,
        launch: impl FnOnce(UiTaskContext<T>) + 'static,
    ) -> Result<UiTask<T>, Error> {
        self.edit(|_| Ok(()), |(), edit| edit.start_task(scope, key, launch))
            .map(|edit| edit.result.value)
    }
    pub fn task<T: 'static>(&self, scope: ScopeId, key: u64) -> Result<Option<UiTask<T>>, Error> {
        self.scope(scope)?;
        self.tasks.get(scope, key)
    }
    pub fn cancel_task(&mut self, scope: ScopeId, key: u64) -> Result<bool, Error> {
        self.edit(|_| Ok(()), |(), edit| edit.cancel_task(scope, key))
            .map(|edit| edit.result.value)
    }
    /// Apply a current ready output through the normal source/producer/layout
    /// transaction. Failure leaves it ready for retry; success consumes before
    /// ordinary effects run, including when a post-commit effect panics.
    pub fn apply_task<T: Send + Sync + 'static, R>(
        &mut self,
        task: &UiTask<T>,
        apply: impl FnOnce(&T, &mut ReactiveTransaction<'_>) -> Result<R, Error>,
    ) -> Result<TaskApply<R>, Error> {
        if task.scope.owner != self.identity {
            return Err(Error::InvalidTask);
        }
        if !self.tasks.current(task) {
            return Ok(TaskApply::Ignored(task.status()));
        }
        let output = {
            let state = lock(&task.shared.0);
            match state.status {
                UiTaskStatus::PendingCommit | UiTaskStatus::Running => {
                    return Ok(TaskApply::Pending);
                }
                UiTaskStatus::Ready => state.output.clone().ok_or(Error::InvalidTask)?,
                status => return Ok(TaskApply::Ignored(status)),
            }
        };
        let adopted = || {
            let output = {
                let mut state = lock(&task.shared.0);
                // The successful output may itself retire its component through
                // a producer. It was still adopted and must never be replayed.
                state.status = UiTaskStatus::Applied;
                state.output.take()
            };
            drop(output);
        };
        let frame_environment = self
            .ui
            .tree()
            .layout_constraints
            .zip(self.ui.tree().viewport);
        let edit = self.edit_validated_adopting(
            |tx| apply(&output.value, tx),
            |value, _| Ok(value),
            frame_environment,
            None::<fn(&mut UiTree) -> Result<(), Error>>,
            None,
            Some(&adopted),
        )?;
        Ok(TaskApply::Applied(edit.result))
    }
}
#[cfg(test)]
mod pool_tests;
#[cfg(test)]
mod tests;
