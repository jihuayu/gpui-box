//! Bounded in-process CPU and I/O pools, deliberately separate from durable
//! OS-scheduled jobs.
//!
//! The pools use explicit concurrency and queue bounds. They never start a
//! thread of their own: a host or test drives [`InProcessExecutor::run_ready`]
//! or [`InProcessExecutor::start_next`], which keeps scheduling deterministic
//! and avoids blocking sleeps in tests.

use crate::ExecutionOwner;
use gpui_system::{ErrorCode, OperationContext, SurfaceId, SystemError};
use std::{
    collections::{HashMap, VecDeque},
    fmt,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
};

fn error(code: ErrorCode, message: &'static str) -> SystemError {
    SystemError::new(code, message)
}

fn lock<T>(value: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    value
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// The two independent in-process work classes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum InProcessWorkClass {
    /// Bounded CPU work. Never scheduled on the UI thread.
    CpuBounded,
    /// Bounded blocking I/O. Never scheduled on the UI thread.
    IoBounded,
}

impl InProcessWorkClass {
    pub const ALL: [Self; 2] = [Self::CpuBounded, Self::IoBounded];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::CpuBounded => "cpu_bounded",
            Self::IoBounded => "io_bounded",
        }
    }
}

impl fmt::Display for InProcessWorkClass {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// A surface/generation attachment for an in-process task or lease.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SurfaceTaskScope {
    pub surface: SurfaceId,
    pub generation: u64,
}

impl SurfaceTaskScope {
    /// A zero generation is never a live attachment.
    pub fn new(surface: SurfaceId, generation: u64) -> Result<Self, SystemError> {
        if generation == 0 {
            return Err(error(
                ErrorCode::InvalidInput,
                "A surface task scope requires a non-zero generation",
            ));
        }
        Ok(Self {
            surface,
            generation,
        })
    }
}

/// Opaque task identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct InProcessTaskId(pub u64);

/// One shared output slot. The task and a disposal both write through it, so a
/// stale completion cannot expose a result after disposal.
type OutputSlot<T> = Arc<Mutex<Option<Result<T, SystemError>>>>;

/// A handle to one bounded in-process task.
pub struct InProcessTaskHandle<T> {
    id: InProcessTaskId,
    work_class: InProcessWorkClass,
    output: OutputSlot<T>,
}

impl<T> InProcessTaskHandle<T> {
    pub fn id(&self) -> InProcessTaskId {
        self.id
    }

    pub fn work_class(&self) -> InProcessWorkClass {
        self.work_class
    }

    /// Take the output once it has been produced. Cancelled or fenced tasks
    /// report a typed error and never a stale value.
    pub fn take_result(&self) -> Option<Result<T, SystemError>> {
        lock(&self.output).take()
    }

    pub fn is_finished(&self) -> bool {
        lock(&self.output).is_some()
    }
}

impl<T> fmt::Debug for InProcessTaskHandle<T> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("InProcessTaskHandle")
            .field("id", &self.id)
            .field("work_class", &self.work_class)
            .finish()
    }
}

struct QueuedWork {
    id: InProcessTaskId,
    scope: Option<SurfaceTaskScope>,
    run: Option<Box<dyn FnOnce() + Send>>,
    cancel: Option<Box<dyn FnOnce() + Send>>,
}

struct Pool {
    max_concurrency: usize,
    max_queued: usize,
    running: usize,
    queued: VecDeque<QueuedWork>,
}

impl Pool {
    fn new(max_concurrency: usize, max_queued: usize) -> Self {
        Self {
            max_concurrency,
            max_queued,
            running: 0,
            queued: VecDeque::new(),
        }
    }
}

struct ExecutorState {
    cpu: Pool,
    io: Pool,
    latest_generation: HashMap<SurfaceId, u64>,
    disposed_generation: HashMap<SurfaceId, u64>,
}

impl ExecutorState {
    fn pool(&mut self, class: InProcessWorkClass) -> &mut Pool {
        match class {
            InProcessWorkClass::CpuBounded => &mut self.cpu,
            InProcessWorkClass::IoBounded => &mut self.io,
        }
    }

    fn pool_ref(&self, class: InProcessWorkClass) -> &Pool {
        match class {
            InProcessWorkClass::CpuBounded => &self.cpu,
            InProcessWorkClass::IoBounded => &self.io,
        }
    }

    fn is_disposed(&self, scope: SurfaceTaskScope) -> bool {
        self.disposed_generation
            .get(&scope.surface)
            .is_some_and(|disposed| *disposed >= scope.generation)
    }
}

struct ExecutorInner {
    state: Mutex<ExecutorState>,
    next_id: AtomicU64,
}

impl ExecutorInner {
    fn release(&self, class: InProcessWorkClass) {
        let mut state = lock(&self.state);
        let pool = state.pool(class);
        pool.running = pool.running.saturating_sub(1);
    }

    fn is_disposed(&self, scope: SurfaceTaskScope) -> bool {
        lock(&self.state).is_disposed(scope)
    }
}

/// A bounded, host-pumped in-process executor with independent CPU and I/O
/// pools.
#[derive(Clone)]
pub struct InProcessExecutor {
    inner: Arc<ExecutorInner>,
}

/// Default CPU concurrency for a new executor.
pub const DEFAULT_CPU_CONCURRENCY: usize = 2;
/// Default I/O concurrency for a new executor.
pub const DEFAULT_IO_CONCURRENCY: usize = 4;
/// Default queue depth for each pool.
pub const DEFAULT_POOL_QUEUE_DEPTH: usize = 64;

/// Summary of one [`InProcessExecutor::run_ready`] pump.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct InProcessRunReport {
    pub completed: usize,
    pub failed: usize,
    pub cancelled: usize,
}

impl InProcessRunReport {
    fn merge(&mut self, completion: &TaskCompletion) {
        match completion.outcome {
            Ok(()) => self.completed = self.completed.saturating_add(1),
            Err(ref failure) if failure.code == ErrorCode::ContextExpired => {
                self.cancelled = self.cancelled.saturating_add(1);
            }
            Err(_) => self.failed = self.failed.saturating_add(1),
        }
    }
}

/// Result of one explicitly started task.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskCompletion {
    pub id: InProcessTaskId,
    pub work_class: InProcessWorkClass,
    pub outcome: Result<(), SystemError>,
}

impl std::fmt::Debug for InProcessExecutor {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let state = lock(&self.inner.state);
        formatter
            .debug_struct("InProcessExecutor")
            .field("cpu_queued", &state.cpu.queued.len())
            .field("cpu_running", &state.cpu.running)
            .field("io_queued", &state.io.queued.len())
            .field("io_running", &state.io.running)
            .finish()
    }
}

impl InProcessExecutor {
    pub fn new(
        cpu_max_concurrency: usize,
        cpu_max_queued: usize,
        io_max_concurrency: usize,
        io_max_queued: usize,
    ) -> Result<Self, SystemError> {
        if cpu_max_concurrency == 0
            || io_max_concurrency == 0
            || cpu_max_queued == 0
            || io_max_queued == 0
        {
            return Err(error(
                ErrorCode::InvalidConfiguration,
                "In-process pools require positive concurrency and queue bounds",
            ));
        }
        Ok(Self {
            inner: Arc::new(ExecutorInner {
                state: Mutex::new(ExecutorState {
                    cpu: Pool::new(cpu_max_concurrency, cpu_max_queued),
                    io: Pool::new(io_max_concurrency, io_max_queued),
                    latest_generation: HashMap::new(),
                    disposed_generation: HashMap::new(),
                }),
                next_id: AtomicU64::new(1),
            }),
        })
    }

    pub fn with_defaults() -> Self {
        Self::new(
            DEFAULT_CPU_CONCURRENCY,
            DEFAULT_POOL_QUEUE_DEPTH,
            DEFAULT_IO_CONCURRENCY,
            DEFAULT_POOL_QUEUE_DEPTH,
        )
        .expect("default in-process pool bounds are valid")
    }

    /// Submit bounded work. Only [`ExecutionOwner::Background`] is accepted;
    /// UI and renderer ownership is rejected before any side effect.
    pub fn submit<T, F>(
        &self,
        work_class: InProcessWorkClass,
        owner: ExecutionOwner,
        context: &OperationContext,
        scope: Option<SurfaceTaskScope>,
        task: F,
    ) -> Result<InProcessTaskHandle<T>, SystemError>
    where
        T: Send + 'static,
        F: FnOnce() -> Result<T, SystemError> + Send + 'static,
    {
        owner.require_background(context)?;
        if context.cancellation.reason().is_some() {
            return Err(error(
                ErrorCode::ContextExpired,
                "In-process task was cancelled before submission",
            ));
        }
        if let Some(scope) = scope {
            self.validate_scope(scope)?;
        }
        let id = InProcessTaskId(self.inner.next_id.fetch_add(1, Ordering::Relaxed));
        let output: OutputSlot<T> = Arc::new(Mutex::new(None));
        let task_output = Arc::clone(&output);
        let cancellation_output = Arc::clone(&output);
        let id_copy = id;
        let scope_copy = scope;
        let run = move || {
            let result = task();
            let mut slot = lock(&task_output);
            if slot.is_none() {
                *slot = Some(result);
            }
            let _ = (id_copy, scope_copy);
        };
        let cancel: Box<dyn FnOnce() + Send> = Box::new(move || {
            let mut slot = lock(&cancellation_output);
            *slot = Some(Err(error(
                ErrorCode::ContextExpired,
                "Surface-scoped in-process task was disposed",
            )));
        });
        {
            let mut state = lock(&self.inner.state);
            let pool = state.pool(work_class);
            if pool.queued.len() >= pool.max_queued {
                return Err(error(ErrorCode::Busy, "In-process task queue is full"));
            }
            pool.queued.push_back(QueuedWork {
                id,
                scope,
                run: Some(Box::new(run)),
                cancel: Some(cancel),
            });
        }
        Ok(InProcessTaskHandle {
            id,
            work_class,
            output,
        })
    }

    /// Start the next queued task for one class without running it.
    pub fn start_next(&self, work_class: InProcessWorkClass) -> Option<RunningInProcessTask> {
        let mut state = lock(&self.inner.state);
        let pool = state.pool(work_class);
        if pool.running >= pool.max_concurrency || pool.queued.is_empty() {
            return None;
        }
        let work = pool.queued.pop_front()?;
        pool.running = pool.running.saturating_add(1);
        Some(RunningInProcessTask {
            inner: Arc::clone(&self.inner),
            id: work.id,
            work_class,
            scope: work.scope,
            run: work.run,
            cancel: work.cancel,
            finished: false,
        })
    }

    /// Run every task admitted by the current concurrency bounds.
    pub fn run_ready(&self) -> InProcessRunReport {
        let mut report = InProcessRunReport::default();
        for class in InProcessWorkClass::ALL {
            while let Some(task) = self.start_next(class) {
                let completion = task.finish();
                report.merge(&completion);
            }
        }
        report
    }

    /// Dispose every generation of one surface.
    pub fn dispose_surface(&self, surface: SurfaceId) -> SurfaceDisposalReport {
        let generation = lock(&self.inner.state)
            .latest_generation
            .get(&surface)
            .copied();
        match generation {
            Some(generation) => SurfaceDisposalReport {
                surface,
                generation,
                cancelled_queued: self
                    .dispose_generation_inner(surface, generation)
                    .unwrap_or(0),
            },
            None => SurfaceDisposalReport {
                surface,
                generation: 0,
                cancelled_queued: 0,
            },
        }
    }

    /// Dispose exactly one surface generation. A zero or stale generation is
    /// rejected; a newer generation is never affected by an older drop.
    pub fn dispose_surface_generation(
        &self,
        surface: SurfaceId,
        generation: u64,
    ) -> Result<SurfaceDisposalReport, SystemError> {
        if generation == 0 {
            return Err(error(
                ErrorCode::InvalidInput,
                "A surface disposal requires a non-zero generation",
            ));
        }
        let cancelled = self.dispose_generation_inner(surface, generation)?;
        Ok(SurfaceDisposalReport {
            surface,
            generation,
            cancelled_queued: cancelled,
        })
    }

    fn dispose_generation_inner(
        &self,
        surface: SurfaceId,
        generation: u64,
    ) -> Result<usize, SystemError> {
        let mut cancelled = 0usize;
        {
            let mut state = lock(&self.inner.state);
            let latest = state
                .latest_generation
                .get(&surface)
                .copied()
                .unwrap_or(generation);
            if generation < latest {
                return Err(error(
                    ErrorCode::ContextExpired,
                    "Surface generation is stale",
                ));
            }
            if state.is_disposed(SurfaceTaskScope {
                surface,
                generation,
            }) {
                return Ok(0);
            }
            state.disposed_generation.insert(surface, generation);
            let latest_entry = state.latest_generation.entry(surface).or_insert(0);
            *latest_entry = (*latest_entry).max(generation);
            for class in InProcessWorkClass::ALL {
                let pool = state.pool(class);
                let mut retained = VecDeque::with_capacity(pool.queued.len());
                while let Some(mut work) = pool.queued.pop_front() {
                    let matches = work.scope.is_some_and(|scope| {
                        scope.surface == surface && scope.generation <= generation
                    });
                    if matches {
                        if let Some(cancel) = work.cancel.take() {
                            cancel();
                        }
                        cancelled = cancelled.saturating_add(1);
                    } else {
                        retained.push_back(work);
                    }
                }
                pool.queued = retained;
            }
        }
        Ok(cancelled)
    }

    fn validate_scope(&self, scope: SurfaceTaskScope) -> Result<(), SystemError> {
        if scope.generation == 0 {
            return Err(error(
                ErrorCode::InvalidInput,
                "A surface task scope requires a non-zero generation",
            ));
        }
        let mut state = lock(&self.inner.state);
        if let Some(latest) = state.latest_generation.get(&scope.surface)
            && scope.generation < *latest
        {
            return Err(error(
                ErrorCode::ContextExpired,
                "Surface task generation is stale",
            ));
        }
        if state.is_disposed(scope) {
            return Err(error(
                ErrorCode::ContextExpired,
                "Surface task generation is disposed",
            ));
        }
        state
            .latest_generation
            .insert(scope.surface, scope.generation);
        Ok(())
    }

    pub fn queued_len(&self, work_class: InProcessWorkClass) -> usize {
        lock(&self.inner.state).pool_ref(work_class).queued.len()
    }

    pub fn running_len(&self, work_class: InProcessWorkClass) -> usize {
        lock(&self.inner.state).pool_ref(work_class).running
    }

    pub fn max_concurrency(&self, work_class: InProcessWorkClass) -> usize {
        lock(&self.inner.state).pool_ref(work_class).max_concurrency
    }

    pub fn max_queued(&self, work_class: InProcessWorkClass) -> usize {
        lock(&self.inner.state).pool_ref(work_class).max_queued
    }

    pub fn latest_generation(&self, surface: SurfaceId) -> Option<u64> {
        lock(&self.inner.state)
            .latest_generation
            .get(&surface)
            .copied()
    }

    pub fn is_generation_disposed(&self, surface: SurfaceId, generation: u64) -> bool {
        lock(&self.inner.state).is_disposed(SurfaceTaskScope {
            surface,
            generation,
        })
    }
}

impl Default for InProcessExecutor {
    fn default() -> Self {
        Self::with_defaults()
    }
}

/// Report returned when a surface attachment is disposed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SurfaceDisposalReport {
    pub surface: SurfaceId,
    pub generation: u64,
    pub cancelled_queued: usize,
}

/// A task that has been removed from its queue and holds one running slot.
pub struct RunningInProcessTask {
    inner: Arc<ExecutorInner>,
    id: InProcessTaskId,
    work_class: InProcessWorkClass,
    scope: Option<SurfaceTaskScope>,
    run: Option<Box<dyn FnOnce() + Send>>,
    cancel: Option<Box<dyn FnOnce() + Send>>,
    finished: bool,
}

impl RunningInProcessTask {
    pub fn id(&self) -> InProcessTaskId {
        self.id
    }

    pub fn scope(&self) -> Option<SurfaceTaskScope> {
        self.scope
    }

    /// Execute the task unless its surface scope was disposed first. A result
    /// produced by stale work is fenced and replaced with a typed error.
    pub fn finish(mut self) -> TaskCompletion {
        let disposed_before = self
            .scope
            .is_some_and(|scope| self.inner.is_disposed(scope));
        let outcome = if disposed_before {
            if let Some(cancel) = self.cancel.take() {
                cancel();
            }
            Err(error(
                ErrorCode::ContextExpired,
                "Surface-scoped in-process task was disposed",
            ))
        } else {
            if let Some(run) = self.run.take() {
                run();
            }
            if self
                .scope
                .is_some_and(|scope| self.inner.is_disposed(scope))
            {
                if let Some(cancel) = self.cancel.take() {
                    cancel();
                }
                Err(error(
                    ErrorCode::ContextExpired,
                    "Surface-scoped in-process task completed after disposal",
                ))
            } else {
                Ok(())
            }
        };
        self.run = None;
        self.cancel = None;
        self.inner.release(self.work_class);
        self.finished = true;
        TaskCompletion {
            id: self.id,
            work_class: self.work_class,
            outcome,
        }
    }
}

impl Drop for RunningInProcessTask {
    fn drop(&mut self) {
        if self.finished {
            return;
        }
        if let Some(cancel) = self.cancel.take() {
            cancel();
        }
        self.run = None;
        self.inner.release(self.work_class);
    }
}

impl fmt::Debug for RunningInProcessTask {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RunningInProcessTask")
            .field("id", &self.id)
            .field("work_class", &self.work_class)
            .field("scope", &self.scope)
            .finish()
    }
}
