//! Optional GPUI adapter over the coordinated GPUI identity.
//!
//! Installation and lookup must run on the owning GPUI UI thread. The core
//! [`JobsService`] remains `Send + Sync`; this adapter only adds UI ownership,
//! `App` globals, surface-scoped in-process work and an entity model.

use crate::{
    DEFAULT_JOBS_EVENT_CAPACITY, EventSubscription, ExecutionOwner, InProcessTaskHandle,
    InProcessWorkClass, JobProvider, JobRecord, JobsEvent, JobsService, OperationContext,
    ServiceResult, SurfaceId, SystemError, SystemServices, UiContext, UiOwner,
};
use gpui::{App, Context, Global};
use gpui_system::{Completion, ErrorCode, gpui_adapter::EntityScope};
use std::{
    collections::VecDeque,
    pin::Pin,
    sync::Arc,
    task::{Context as TaskContext, Poll, Waker},
};

impl Global for JobsService {}

/// Install the service into GPUI's `App` global.
pub fn install_jobs_service(service: JobsService, cx: &mut App) {
    cx.set_global(service);
}

/// Register a service in the typed `SystemServices` registry.
pub fn install_jobs_in_services(
    services: &mut SystemServices,
    service: JobsService,
) -> Result<(), SystemError> {
    services.register(service)
}

/// Build a private in-memory service and install it as the `App` global and in
/// `SystemServices`.
pub fn init_jobs(cx: &mut App, provider: Arc<dyn JobProvider>) -> Result<JobsService, SystemError> {
    let service = JobsService::in_memory(provider)?;
    cx.set_global(service.clone());
    cx.default_global::<SystemServices>()
        .override_provider(service.clone());
    Ok(service)
}

/// Resolve the service from the `App` global first, then from `SystemServices`.
pub fn jobs_service(cx: &App) -> Result<JobsService, SystemError> {
    if let Some(service) = cx.try_global::<JobsService>() {
        return Ok(service.clone());
    }
    let services = cx.try_global::<SystemServices>().ok_or_else(|| {
        SystemError::new(
            ErrorCode::Unavailable,
            "JobsService and SystemServices are not installed",
        )
    })?;
    services
        .service::<JobsService>()
        .map(|service| (*service).clone())
}

/// Convenience methods for GPUI applications.
pub trait JobsAppExt {
    fn install_jobs_service(&mut self, service: JobsService);
    fn init_jobs(&mut self, provider: Arc<dyn JobProvider>) -> Result<JobsService, SystemError>;
    fn jobs_service(&self) -> Result<JobsService, SystemError>;
}

impl JobsAppExt for App {
    fn install_jobs_service(&mut self, service: JobsService) {
        install_jobs_service(service, self);
    }

    fn init_jobs(&mut self, provider: Arc<dyn JobProvider>) -> Result<JobsService, SystemError> {
        init_jobs(self, provider)
    }

    fn jobs_service(&self) -> Result<JobsService, SystemError> {
        jobs_service(self)
    }
}

/// Surface-owned in-process jobs scope.
///
/// The scope retains the current `(SurfaceId, generation)` and on drop disposes
/// exactly that generation, cancelling queued work and fencing running output.
/// It never disposes a newer generation of the same surface.
pub struct SurfaceJobsScope {
    owner: UiOwner,
    service: JobsService,
    surface: SurfaceId,
    generation: u64,
    ui: Option<UiContext>,
    subscriptions: Vec<EventSubscription<JobsEvent>>,
    entity_scope: Option<EntityScope>,
}

impl SurfaceJobsScope {
    /// Create a scope for the owner's current attachment generation. A disposed
    /// owner produces an inert scope whose operations report `ContextExpired`.
    pub fn new(owner: UiOwner, service: JobsService) -> Self {
        match owner.context(None) {
            Ok(ui) => Self::with_ui(owner, service, ui),
            Err(_) => Self {
                owner,
                service,
                surface: SurfaceId(0),
                generation: 0,
                ui: None,
                subscriptions: Vec::new(),
                entity_scope: None,
            },
        }
    }

    /// Fallible form of [`Self::new`].
    pub fn try_new(owner: UiOwner, service: JobsService) -> Result<Self, SystemError> {
        let ui = owner.context(None)?;
        Ok(Self::with_ui(owner, service, ui))
    }

    fn with_ui(owner: UiOwner, service: JobsService, ui: UiContext) -> Self {
        let surface = ui.surface;
        let generation = ui.generation;
        Self {
            owner,
            service,
            surface,
            generation,
            ui: Some(ui),
            subscriptions: Vec::new(),
            entity_scope: None,
        }
    }

    /// Create a scope tied to a GPUI entity lifecycle.
    pub fn for_entity<T: 'static>(owner: UiOwner, service: JobsService, cx: &Context<T>) -> Self {
        let entity_scope = EntityScope::new(owner.clone(), cx);
        let mut scope = Self::new(owner, service);
        scope.entity_scope = Some(entity_scope);
        scope
    }

    pub fn owner(&self) -> &UiOwner {
        &self.owner
    }

    pub fn service(&self) -> &JobsService {
        &self.service
    }

    pub fn surface(&self) -> SurfaceId {
        self.surface
    }

    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// A UI-attached context for this exact surface generation.
    pub fn context(&self) -> Result<OperationContext, SystemError> {
        let ui = self.ui.clone().ok_or_else(|| {
            SystemError::new(
                ErrorCode::ContextExpired,
                "Jobs surface scope has no live UI attachment",
            )
        })?;
        if !ui.is_current() {
            return Err(SystemError::new(
                ErrorCode::ContextExpired,
                "Jobs surface scope UI attachment is stale",
            ));
        }
        Ok(OperationContext {
            ui: Some(ui),
            ..OperationContext::default()
        })
    }

    pub fn subscribe(&mut self) -> &EventSubscription<JobsEvent> {
        self.subscriptions.push(self.service.subscribe());
        self.subscriptions
            .last()
            .expect("a subscription was just retained")
    }

    pub fn subscription_count(&self) -> usize {
        self.subscriptions.len()
    }

    pub fn take_subscription(&mut self) -> Option<EventSubscription<JobsEvent>> {
        self.subscriptions.pop()
    }

    /// Submit bounded CPU work for this surface generation. Blocking work
    /// requires a background context; a UI or renderer owner is rejected.
    pub fn submit_cpu<T, F>(
        &self,
        context: &OperationContext,
        task: F,
    ) -> Result<InProcessTaskHandle<T>, SystemError>
    where
        T: Send + 'static,
        F: FnOnce() -> Result<T, SystemError> + Send + 'static,
    {
        self.submit(InProcessWorkClass::CpuBounded, context, task)
    }

    /// Submit bounded I/O work for this surface generation.
    pub fn submit_io<T, F>(
        &self,
        context: &OperationContext,
        task: F,
    ) -> Result<InProcessTaskHandle<T>, SystemError>
    where
        T: Send + 'static,
        F: FnOnce() -> Result<T, SystemError> + Send + 'static,
    {
        self.submit(InProcessWorkClass::IoBounded, context, task)
    }

    fn submit<T, F>(
        &self,
        class: InProcessWorkClass,
        context: &OperationContext,
        task: F,
    ) -> Result<InProcessTaskHandle<T>, SystemError>
    where
        T: Send + 'static,
        F: FnOnce() -> Result<T, SystemError> + Send + 'static,
    {
        if self.ui.is_none() || self.generation == 0 {
            return Err(SystemError::new(
                ErrorCode::ContextExpired,
                "Jobs surface scope is inert or disposed",
            ));
        }
        let scope = crate::SurfaceTaskScope::new(self.surface, self.generation)?;
        self.service.in_process().submit(
            class,
            ExecutionOwner::Background,
            context,
            Some(scope),
            task,
        )
    }

    /// Cooperative cancellation of this scope's surface generation.
    pub fn dispose(&self) -> Result<crate::SurfaceDisposalReport, SystemError> {
        self.service
            .in_process()
            .dispose_surface_generation(self.surface, self.generation)
    }

    pub fn dispose_surface(&self) -> crate::SurfaceDisposalReport {
        self.service.in_process().dispose_surface(self.surface)
    }
}

impl Drop for SurfaceJobsScope {
    fn drop(&mut self) {
        self.subscriptions.clear();
        if self.generation != 0 {
            let _ = self
                .service
                .in_process()
                .dispose_surface_generation(self.surface, self.generation);
        }
        if self.ui.as_ref().is_some_and(UiContext::is_current) {
            self.owner.dispose();
        }
        drop(self.entity_scope.take());
    }
}

impl std::fmt::Debug for SurfaceJobsScope {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SurfaceJobsScope")
            .field("surface", &self.surface)
            .field("generation", &self.generation)
            .field("subscriptions", &self.subscriptions.len())
            .finish()
    }
}

/// Bounded drain of a [`JobsModel`] event stream.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct JobsEventDrain {
    pub resync_required: bool,
    pub events: Vec<JobsEvent>,
}

/// A read-only jobs snapshot for a GPUI entity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JobsModelSnapshot {
    pub jobs: Vec<JobRecord>,
    pub revision: u64,
    pub resync_required: bool,
}

/// GPUI entity model that owns one surface jobs scope and a bounded event
/// subscription.
pub struct JobsModel {
    scope: SurfaceJobsScope,
    subscription: EventSubscription<JobsEvent>,
    events: VecDeque<JobsEvent>,
    resync_required: bool,
}

impl JobsModel {
    pub const MAX_RETAINED_EVENTS: usize = DEFAULT_JOBS_EVENT_CAPACITY;

    pub fn new(owner: UiOwner, service: JobsService, cx: &mut Context<Self>) -> Self {
        let scope = SurfaceJobsScope::for_entity(owner, service, cx);
        let subscription = scope.service().subscribe();
        Self {
            scope,
            subscription,
            events: VecDeque::new(),
            resync_required: false,
        }
    }

    pub fn service(&self) -> &JobsService {
        self.scope.service()
    }

    pub fn surface(&self) -> SurfaceId {
        self.scope.surface()
    }

    pub fn generation(&self) -> u64 {
        self.scope.generation()
    }

    pub fn snapshot(&self) -> JobsModelSnapshot {
        JobsModelSnapshot {
            jobs: self.service().list_jobs(),
            revision: self.service().event_revision(),
            resync_required: self.resync_required,
        }
    }

    /// Drain all immediately ready events. Provider overflow and local history
    /// overflow set a sticky `resync_required` flag.
    pub fn drain_events(&mut self) -> JobsEventDrain {
        let mut events = Vec::new();
        while let Ok(mut next) = self.subscription.try_next() {
            let waker = Waker::noop();
            let mut context = TaskContext::from_waker(waker);
            match Pin::new(&mut next).poll(&mut context) {
                Poll::Ready(Some(envelope)) => match envelope.payload {
                    crate::Event::Value(event) => {
                        if self.events.len() == Self::MAX_RETAINED_EVENTS {
                            self.events.pop_front();
                            self.resync_required = true;
                        }
                        self.events.push_back(event.clone());
                        events.push(event);
                    }
                    crate::Event::ResyncRequired => {
                        self.resync_required = true;
                    }
                },
                Poll::Ready(None) | Poll::Pending => break,
            }
        }
        JobsEventDrain {
            resync_required: self.resync_required,
            events,
        }
    }

    pub fn resync_required(&self) -> bool {
        self.resync_required
    }

    pub fn clear_resync_required(&mut self) {
        self.resync_required = false;
    }

    pub fn retained_events(&self) -> impl Iterator<Item = &JobsEvent> {
        self.events.iter()
    }

    pub fn take_retained_events(&mut self) -> Vec<JobsEvent> {
        self.events.drain(..).collect()
    }

    pub fn refresh_jobs(&self, cx: &mut Context<Self>) -> JobsModelSnapshot {
        cx.notify();
        self.snapshot()
    }

    /// Submit bounded CPU work for this entity's surface generation.
    pub fn submit_cpu<T, F>(&self, task: F) -> Result<InProcessTaskHandle<T>, SystemError>
    where
        T: Send + 'static,
        F: FnOnce() -> Result<T, SystemError> + Send + 'static,
    {
        self.scope.submit_cpu(&OperationContext::default(), task)
    }

    /// Submit bounded I/O work for this entity's surface generation.
    pub fn submit_io<T, F>(&self, task: F) -> Result<InProcessTaskHandle<T>, SystemError>
    where
        T: Send + 'static,
        F: FnOnce() -> Result<T, SystemError> + Send + 'static,
    {
        self.scope.submit_io(&OperationContext::default(), task)
    }
}

impl Drop for JobsModel {
    fn drop(&mut self) {
        self.events.clear();
    }
}

impl std::fmt::Debug for JobsModel {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("JobsModel")
            .field("surface", &self.surface())
            .field("generation", &self.generation())
            .field("retained_events", &self.events.len())
            .field("resync_required", &self.resync_required)
            .finish()
    }
}

/// Explicitly finish a request-like future on the UI thread. Retained for
/// parity with sibling adapters and to document that jobs never move GPUI
/// objects across threads.
pub fn spawn_local<T: 'static>(
    cx: &App,
    future: impl std::future::Future<Output = T> + 'static,
) -> gpui::Task<T> {
    cx.foreground_executor().spawn(future)
}

/// Compatibility helper for callers that complete a service result on the UI
/// thread.
pub fn complete<T>(value: T) -> ServiceResult<T> {
    Ok(Completion::Completed(value))
}

#[cfg(all(
    test,
    feature = "gpui",
    any(target_os = "windows", target_os = "macos", target_os = "linux")
))]
mod tests {
    use super::*;
    use crate::FakeJobProvider;
    use gpui::{AppContext, TestAppContext};

    #[gpui::test]
    fn installs_app_global_and_system_services(cx: &mut TestAppContext) {
        cx.update(|cx| {
            gpui_system::gpui_adapter::install_system_services(SystemServices::new(), cx);
            let service = init_jobs(cx, Arc::new(FakeJobProvider::new())).expect("init");
            let resolved = jobs_service(cx).expect("app global");
            assert!(service.shares_state_with(&resolved));
            assert!(cx.jobs_service().is_ok());
            let registered = cx
                .try_global::<SystemServices>()
                .expect("services")
                .service::<JobsService>()
                .expect("registry");
            assert!(service.shares_state_with(&registered));
        });
    }

    #[gpui::test]
    fn missing_service_is_typed(cx: &mut TestAppContext) {
        cx.update(|cx| {
            assert_eq!(jobs_service(cx).unwrap_err().code, ErrorCode::Unavailable);
        });
    }

    #[gpui::test]
    fn surface_scope_disposes_generation_and_fences_output(cx: &mut TestAppContext) {
        let service = cx
            .update(|_| JobsService::in_memory(Arc::new(FakeJobProvider::new())).expect("service"));
        let owner = UiOwner::new(SurfaceId(31));
        let probe = owner.clone();
        let scope = cx.update(|_| SurfaceJobsScope::new(owner, service.clone()));
        assert_eq!(scope.surface(), SurfaceId(31));
        assert_eq!(scope.generation(), 1);
        let executor = service.in_process().clone();
        let handle = scope
            .submit_cpu(&OperationContext::default(), || Ok(7u32))
            .expect("submit");
        let running = executor
            .start_next(crate::InProcessWorkClass::CpuBounded)
            .expect("running task");
        assert!(matches!(
            scope.dispose().expect("dispose"),
            crate::SurfaceDisposalReport {
                surface: SurfaceId(31),
                generation: 1,
                ..
            }
        ));
        assert_eq!(
            running.finish().outcome.unwrap_err().code,
            ErrorCode::ContextExpired
        );
        assert_eq!(
            handle.take_result().expect("output").unwrap_err().code,
            ErrorCode::ContextExpired
        );
        drop(scope);
        assert!(probe.context(None).is_err());
    }

    #[gpui::test]
    fn entity_model_drains_bounded_events(cx: &mut TestAppContext) {
        let service = cx
            .update(|_| JobsService::in_memory(Arc::new(FakeJobProvider::new())).expect("service"));
        let owner = UiOwner::new(SurfaceId(32));
        let model = cx.update(|cx| cx.new(|cx| JobsModel::new(owner.clone(), service.clone(), cx)));
        cx.update(|cx| {
            model.update(cx, |model, _cx| {
                assert_eq!(model.surface(), SurfaceId(32));
                assert_eq!(model.generation(), 1);
                let _ = model.drain_events();
                assert_eq!(JobsModel::MAX_RETAINED_EVENTS, 64);
            });
        });
        drop(model);
        cx.update(|_| {});
        assert!(owner.context(None).is_err());
    }
}
