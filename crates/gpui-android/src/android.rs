mod bridge;
mod dispatcher;
mod platform;
mod window;

pub use bridge::{
    attach, back, back_enabled, clipboard_action, detach, edit, frame, initialize, input_session,
    lifecycle, memory_warning, resize, text, text_state, touch,
};
pub use platform::AndroidPlatform;

use std::cell::{Cell, RefCell};

// Adapted from the pinned Apache-2.0 GPUI iOS backend. A callback may replace
// itself or enter application code, so it must run outside the RefCell borrow.
pub(super) struct CallbackSlot<T> {
    value: RefCell<Option<T>>,
    generation: Cell<u64>,
}

impl<T> Default for CallbackSlot<T> {
    fn default() -> Self {
        Self {
            value: RefCell::new(None),
            generation: Cell::new(0),
        }
    }
}

impl<T> CallbackSlot<T> {
    fn set(&self, value: T) {
        self.generation.set(self.generation.get().wrapping_add(1));
        self.value.replace(Some(value));
    }

    fn take(&self) -> Option<T> {
        self.generation.set(self.generation.get().wrapping_add(1));
        self.value.borrow_mut().take()
    }

    fn with<R>(&self, callback: impl FnOnce(&mut T) -> R) -> Option<R> {
        let mut value = self.value.borrow_mut().take()?;
        let generation = self.generation.get();
        let result = callback(&mut value);
        if self.generation.get() == generation {
            self.value.replace(Some(value));
        }
        Some(result)
    }
}
