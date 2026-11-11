use crate::{CancelReason, ErrorCode, SystemError};
use std::{
    sync::{Arc, Mutex, Weak},
    task::Waker,
    time::Instant,
};

pub(crate) fn lock<T>(value: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    value
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

#[derive(Default)]
pub(crate) struct WakeCell(Mutex<Option<Waker>>);

impl WakeCell {
    pub(crate) fn register(&self, waker: &Waker) {
        *lock(&self.0) = Some(waker.clone());
    }

    pub(crate) fn wake(&self) {
        let waker = lock(&self.0).take();
        if let Some(waker) = waker {
            waker.wake();
        }
    }
}

#[derive(Default)]
pub(crate) struct WakeList(Mutex<Vec<Weak<WakeCell>>>);

impl WakeList {
    pub(crate) fn register(&self, cell: &Arc<WakeCell>) {
        let mut cells = lock(&self.0);
        cells.retain(|cell| cell.strong_count() > 0);
        if !cells
            .iter()
            .any(|existing| existing.ptr_eq(&Arc::downgrade(cell)))
        {
            cells.push(Arc::downgrade(cell));
        }
    }

    pub(crate) fn wake_all(&self) {
        let cells = lock(&self.0).clone();
        for cell in cells.into_iter().filter_map(|cell| cell.upgrade()) {
            cell.wake();
        }
    }
}

#[derive(Default)]
struct CancellationState {
    reason: Mutex<Option<CancelReason>>,
    watchers: WakeList,
}

/// Clones share the first cancellation reason. This does not undo side effects.
#[derive(Clone, Default)]
pub struct CancellationToken(Arc<CancellationState>);

impl CancellationToken {
    pub fn cancel(&self, reason: CancelReason) {
        let changed = {
            let mut current = lock(&self.0.reason);
            if current.is_none() {
                *current = Some(reason);
                true
            } else {
                false
            }
        };
        if changed {
            self.0.watchers.wake_all();
        }
    }

    pub fn reason(&self) -> Option<CancelReason> {
        *lock(&self.0.reason)
    }

    pub(crate) fn watch(&self, cell: &Arc<WakeCell>) {
        self.0.watchers.register(cell);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SurfaceId(pub u64);

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LogicalRect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

struct OwnerState {
    generation: Mutex<Option<u64>>,
    watchers: WakeList,
}

struct OwnerLease(Arc<OwnerState>);

impl Drop for OwnerLease {
    fn drop(&mut self) {
        *lock(&self.0.generation) = None;
        self.0.watchers.wake_all();
    }
}

/// The host owns this guard. Recreating a native attachment advances generation;
/// dropping the last owning guard invalidates every outstanding UI context.
#[derive(Clone)]
pub struct UiOwner {
    surface: SurfaceId,
    lease: Arc<OwnerLease>,
}

impl UiOwner {
    pub fn new(surface: SurfaceId) -> Self {
        Self {
            surface,
            lease: Arc::new(OwnerLease(Arc::new(OwnerState {
                generation: Mutex::new(Some(1)),
                watchers: WakeList::default(),
            }))),
        }
    }

    pub fn context(&self, anchor: Option<LogicalRect>) -> Result<UiContext, SystemError> {
        let generation = lock(&self.lease.0.generation)
            .ok_or_else(|| SystemError::new(ErrorCode::ContextExpired, "UI owner is disposed"))?;
        Ok(UiContext {
            surface: self.surface,
            generation,
            anchor,
            state: Arc::downgrade(&self.lease.0),
        })
    }

    pub fn advance_generation(&self) -> Result<(), SystemError> {
        {
            let mut generation = lock(&self.lease.0.generation);
            let next = generation.and_then(|value| value.checked_add(1));
            *generation = next;
        }
        self.lease.0.watchers.wake_all();
        if lock(&self.lease.0.generation).is_none() {
            return Err(SystemError::new(
                ErrorCode::ContextExpired,
                "UI owner is disposed",
            ));
        }
        Ok(())
    }

    pub fn dispose(&self) {
        *lock(&self.lease.0.generation) = None;
        self.lease.0.watchers.wake_all();
    }
}

/// A non-owning, logical UI attachment. Native handles remain provider-private.
#[derive(Clone)]
pub struct UiContext {
    pub surface: SurfaceId,
    pub generation: u64,
    pub anchor: Option<LogicalRect>,
    state: Weak<OwnerState>,
}

impl UiContext {
    pub fn is_current(&self) -> bool {
        self.state
            .upgrade()
            .is_some_and(|owner| *lock(&owner.generation) == Some(self.generation))
    }

    pub(crate) fn watch(&self, cell: &Arc<WakeCell>) {
        if let Some(owner) = self.state.upgrade() {
            owner.watchers.register(cell);
        }
    }
}

#[derive(Clone, Default)]
pub struct OperationContext {
    pub ui: Option<UiContext>,
    pub cancellation: CancellationToken,
    pub deadline: Option<Instant>,
}

impl OperationContext {
    pub fn check_at(&self, now: Instant) -> Result<(), SystemError> {
        if self.ui.as_ref().is_some_and(|ui| !ui.is_current()) {
            return Err(SystemError::new(
                ErrorCode::ContextExpired,
                "UI context has expired",
            ));
        }
        if self.deadline.is_some_and(|deadline| now >= deadline) {
            return Err(SystemError::new(
                ErrorCode::TimedOut,
                "Operation deadline elapsed",
            ));
        }
        Ok(())
    }

    pub fn require_ui(&self) -> Result<&UiContext, SystemError> {
        self.ui
            .as_ref()
            .filter(|ui| ui.is_current())
            .ok_or_else(|| {
                SystemError::new(
                    ErrorCode::ContextExpired,
                    "A current UI context is required",
                )
            })
    }

    pub(crate) fn watch(&self, cell: &Arc<WakeCell>) {
        self.cancellation.watch(cell);
        if let Some(ui) = &self.ui {
            ui.watch(cell);
        }
    }
}
