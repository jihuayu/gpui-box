mod accessibility;
mod bridge;
mod dispatcher;
mod entrypoint;
mod host;
mod permissions;
mod platform;
mod window;

pub use bridge::{
    attach, back, back_enabled, back_gesture, caret_bounds, clipboard_action, detach, edit,
    editor_action, editor_info, frame, initialize, input_session, insets, key, lifecycle,
    memory_warning, on_activation, on_saved_state, open_url_event, resize, restore_state,
    save_state, take_saved_state, text, text_state, touch,
};
pub use host::{host_snapshot, host_state, lifecycle_service};
pub use permissions::{
    AndroidPermissionProvider, permission_context, permission_pending, permission_result,
    permission_service,
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

/// JNI types used by the Android application entrypoint macro.
pub use jni;

pub use accessibility::{accessibility_action, accessibility_snapshot};
