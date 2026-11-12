//! One owner/executor waiter, woken by the existing completion task. This is
//! replaceable background visual delivery only, not event transaction gating.
use gpui_box::retained::wgpu::WgpuFrameRenderer;
use std::{
    cell::RefCell,
    rc::Weak,
    task::{Context, Poll, Waker},
};

#[derive(Default)]
pub(super) struct VisualAdmission {
    gpu: RefCell<Option<Weak<RefCell<WgpuFrameRenderer>>>>,
    waiter: RefCell<Option<Waker>>,
    error: RefCell<Option<String>>,
}
impl VisualAdmission {
    pub fn attach(&self, gpu: Weak<RefCell<WgpuFrameRenderer>>) {
        *self.gpu.borrow_mut() = Some(gpu);
        *self.error.borrow_mut() = None;
        self.wake();
    }
    pub fn poll(&self, cx: &mut Context<'_>) -> Poll<Result<(), String>> {
        if let Some(error) = self.error.borrow().as_ref() {
            return Poll::Ready(Err(error.clone()));
        }
        let gpu = self.gpu.borrow();
        let Some(gpu) = gpu.as_ref() else {
            return Poll::Ready(Ok(()));
        };
        let Some(gpu) = gpu.upgrade() else {
            return Poll::Ready(Err("GPU owner dropped".into()));
        };
        match gpu.borrow_mut().poll_frame_capacity() {
            Ok(capacity) if capacity.available() => Poll::Ready(Ok(())),
            Ok(_) => {
                let mut waiter = self.waiter.borrow_mut();
                if waiter.as_ref().is_none_or(|old| !old.will_wake(cx.waker())) {
                    *waiter = Some(cx.waker().clone());
                }
                Poll::Pending
            }
            Err(error) => Poll::Ready(Err(error.to_string())),
        }
    }
    pub fn wake(&self) {
        if let Some(waiter) = self.waiter.borrow_mut().take() {
            waiter.wake();
        }
    }
    pub fn close(&self, error: String) {
        *self.error.borrow_mut() = Some(error);
        self.wake();
    }
}
