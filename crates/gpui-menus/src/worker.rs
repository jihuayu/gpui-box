//! Bounded background worker pool.
//!
//! Slots are reserved by the service before dispatch and released only when the
//! invocation reaches a terminal state, including deferred completion. Worker
//! threads themselves are bounded by `max_workers`; `max_queued` bounds waiting
//! work, so a saturated service returns `ErrorCode::Busy` rather than growing
//! threads or queues without limit.

use gpui_system::{ErrorCode, SystemError};
use std::{
    sync::mpsc::{self, Sender},
    thread::{self, JoinHandle},
};

/// Bounded worker configuration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WorkerPoolConfig {
    pub max_workers: usize,
    pub max_queued: usize,
}

impl Default for WorkerPoolConfig {
    fn default() -> Self {
        Self {
            max_workers: 4,
            max_queued: 32,
        }
    }
}

impl WorkerPoolConfig {
    pub fn new(max_workers: usize, max_queued: usize) -> Result<Self, SystemError> {
        let config = Self {
            max_workers,
            max_queued,
        };
        config.validate()?;
        Ok(config)
    }

    pub fn validate(&self) -> Result<(), SystemError> {
        if self.max_workers == 0 || self.max_workers > 64 || self.max_queued > 1024 {
            return Err(SystemError::new(
                ErrorCode::InvalidConfiguration,
                "Worker pool requires 1-64 workers and at most 1024 queued invocations",
            ));
        }
        Ok(())
    }

    pub const fn max_active(&self) -> usize {
        self.max_workers.saturating_add(self.max_queued)
    }
}

type Job = Box<dyn FnOnce() + Send + 'static>;

pub(crate) struct WorkerPool {
    config: WorkerPoolConfig,
    inner: Option<PoolInner>,
}

struct PoolInner {
    sender: Sender<Job>,
    _handles: Vec<JoinHandle<()>>,
}

impl WorkerPool {
    pub(crate) const fn new(config: WorkerPoolConfig) -> Self {
        Self {
            config,
            inner: None,
        }
    }

    pub(crate) fn spawn(&mut self, job: Job) -> Result<(), SystemError> {
        if self.inner.is_none() {
            self.inner = Some(self.start()?);
        }
        let inner = self.inner.as_ref().expect("worker pool was just started");
        inner.sender.send(job).map_err(|_| {
            SystemError::new(
                ErrorCode::PlatformFailure,
                "Background worker pool is unavailable",
            )
        })
    }

    fn start(&self) -> Result<PoolInner, SystemError> {
        let (sender, receiver) = mpsc::channel::<Job>();
        let receiver = std::sync::Arc::new(std::sync::Mutex::new(receiver));
        let mut handles = Vec::with_capacity(self.config.max_workers);
        for index in 0..self.config.max_workers {
            let receiver = receiver.clone();
            let handle = thread::Builder::new()
                .name(format!("gpui-menus-worker-{index}"))
                .spawn(move || {
                    loop {
                        let job = {
                            let receiver = receiver
                                .lock()
                                .unwrap_or_else(std::sync::PoisonError::into_inner);
                            receiver.recv()
                        };
                        match job {
                            Ok(job) => job(),
                            Err(_) => break,
                        }
                    }
                })
                .map_err(|_| {
                    SystemError::new(
                        ErrorCode::PlatformFailure,
                        "Background worker thread could not be started",
                    )
                })?;
            handles.push(handle);
        }
        Ok(PoolInner {
            sender,
            _handles: handles,
        })
    }
}

impl Drop for WorkerPool {
    fn drop(&mut self) {
        // Dropping the sender closes the channel; workers exit after their
        // current job. Join handles are intentionally detached so a stuck
        // native handler cannot block the service destructor.
        self.inner = None;
    }
}
