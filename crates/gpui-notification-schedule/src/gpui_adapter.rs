//! Optional GPUI adapter over the coordinated GPUI identity.
//!
//! Installation and lookup must run on the owning GPUI UI thread. The core
//! [`NotificationScheduleService`] remains `Send + Sync`; this adapter only adds
//! UI ownership, `App` globals, an exact `(SurfaceId, generation)` disposal
//! fence and a bounded entity model. Durable scheduling itself is always a
//! background operation and is never called from a UI context here.

use crate::{
    DEFAULT_NOTIFICATION_SCHEDULE_EVENT_CAPACITY, EventSubscription,
    FakeNotificationScheduleProvider, NotificationScheduleEvent, NotificationScheduleRecord,
    NotificationScheduleService, OperationContext, ScheduleAuthorization, ScheduleClock, SurfaceId,
    SystemError, SystemServices, TimeZoneResolver, UiContext, UiOwner,
};
use gpui::{App, Context, Global};
use gpui_system::{Completion, ErrorCode, gpui_adapter::EntityScope};
use std::{
    collections::{BTreeSet, VecDeque},
    pin::Pin,
    sync::{Arc, Mutex},
    task::{Context as TaskContext, Poll, Waker},
};

impl Global for NotificationScheduleService {}

/// Install the service into GPUI's `App` global.
pub fn install_notification_schedule_service(service: NotificationScheduleService, cx: &mut App) {
    cx.set_global(service);
}

/// Register a service in the typed `SystemServices` registry.
pub fn install_notification_schedule_in_services(
    services: &mut SystemServices,
    service: NotificationScheduleService,
) -> Result<(), SystemError> {
    services.register(service)
}

/// Build a private in-memory service and install it as the `App` global and in
/// `SystemServices`.
pub fn init_notification_schedule(
    cx: &mut App,
    provider: Arc<dyn crate::NotificationScheduleProvider>,
    clock: Arc<dyn ScheduleClock>,
    resolver: Arc<dyn TimeZoneResolver>,
    permissions: Arc<dyn crate::SchedulePermissionQuery>,
) -> Result<NotificationScheduleService, SystemError> {
    let service = NotificationScheduleService::in_memory(provider, clock, resolver, permissions)?;
    cx.set_global(service.clone());
    cx.default_global::<SystemServices>()
        .override_provider(service.clone());
    Ok(service)
}

/// Resolve the service from the `App` global first, then from `SystemServices`.
pub fn notification_schedule_service(cx: &App) -> Result<NotificationScheduleService, SystemError> {
    if let Some(service) = cx.try_global::<NotificationScheduleService>() {
        return Ok(service.clone());
    }
    let services = cx.try_global::<SystemServices>().ok_or_else(|| {
        SystemError::new(
            ErrorCode::Unavailable,
            "NotificationScheduleService and SystemServices are not installed",
        )
    })?;
    services
        .service::<NotificationScheduleService>()
        .map(|service| (*service).clone())
}

/// Convenience methods for GPUI applications.
pub trait NotificationScheduleAppExt {
    fn install_notification_schedule_service(&mut self, service: NotificationScheduleService);
    fn init_notification_schedule(
        &mut self,
        provider: Arc<dyn crate::NotificationScheduleProvider>,
        clock: Arc<dyn ScheduleClock>,
        resolver: Arc<dyn TimeZoneResolver>,
        permissions: Arc<dyn crate::SchedulePermissionQuery>,
    ) -> Result<NotificationScheduleService, SystemError>;
    fn notification_schedule_service(&self) -> Result<NotificationScheduleService, SystemError>;
}

impl NotificationScheduleAppExt for App {
    fn install_notification_schedule_service(&mut self, service: NotificationScheduleService) {
        install_notification_schedule_service(service, self);
    }

    fn init_notification_schedule(
        &mut self,
        provider: Arc<dyn crate::NotificationScheduleProvider>,
        clock: Arc<dyn ScheduleClock>,
        resolver: Arc<dyn TimeZoneResolver>,
        permissions: Arc<dyn crate::SchedulePermissionQuery>,
    ) -> Result<NotificationScheduleService, SystemError> {
        init_notification_schedule(self, provider, clock, resolver, permissions)
    }

    fn notification_schedule_service(&self) -> Result<NotificationScheduleService, SystemError> {
        notification_schedule_service(self)
    }
}

#[derive(Debug, Default)]
struct DisposalFence {
    disposed: Mutex<BTreeSet<(u64, u64)>>,
}

impl DisposalFence {
    fn dispose(&self, surface: SurfaceId, generation: u64) {
        if generation != 0 {
            lock(&self.disposed).insert((surface.0, generation));
        }
    }

    fn is_disposed(&self, surface: SurfaceId, generation: u64) -> bool {
        lock(&self.disposed).contains(&(surface.0, generation))
    }
}

/// Surface-owned schedule scope.
///
/// The scope retains the exact `(SurfaceId, generation)` active at creation.
/// Dropping or disposing it records and fences exactly that generation; it can
/// never dispose a newer generation of the same surface.
pub struct SurfaceNotificationScheduleScope {
    owner: UiOwner,
    service: NotificationScheduleService,
    surface: SurfaceId,
    generation: u64,
    ui: Option<UiContext>,
    subscriptions: Vec<EventSubscription<NotificationScheduleEvent>>,
    entity_scope: Option<EntityScope>,
    fence: Arc<DisposalFence>,
}

impl SurfaceNotificationScheduleScope {
    /// Create a scope for the owner's current attachment generation. A disposed
    /// owner produces an inert scope whose operations report `ContextExpired`.
    pub fn new(owner: UiOwner, service: NotificationScheduleService) -> Self {
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
                fence: Arc::new(DisposalFence::default()),
            },
        }
    }

    pub fn try_new(
        owner: UiOwner,
        service: NotificationScheduleService,
    ) -> Result<Self, SystemError> {
        let ui = owner.context(None)?;
        Ok(Self::with_ui(owner, service, ui))
    }

    fn with_ui(owner: UiOwner, service: NotificationScheduleService, ui: UiContext) -> Self {
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
            fence: Arc::new(DisposalFence::default()),
        }
    }

    /// Create a scope tied to a GPUI entity lifecycle.
    pub fn for_entity<T: 'static>(
        owner: UiOwner,
        service: NotificationScheduleService,
        cx: &Context<T>,
    ) -> Self {
        let entity_scope = EntityScope::new(owner.clone(), cx);
        let mut scope = Self::new(owner, service);
        scope.entity_scope = Some(entity_scope);
        scope
    }

    pub fn owner(&self) -> &UiOwner {
        &self.owner
    }

    pub fn service(&self) -> &NotificationScheduleService {
        &self.service
    }

    pub fn surface(&self) -> SurfaceId {
        self.surface
    }

    pub fn generation(&self) -> u64 {
        self.generation
    }

    pub fn is_inert(&self) -> bool {
        self.ui.is_none() || self.generation == 0
    }

    /// True only when this exact attachment generation is live and has not
    /// been disposed.
    pub fn is_current(&self) -> bool {
        !self.is_inert()
            && !self.fence.is_disposed(self.surface, self.generation)
            && self.ui.as_ref().is_some_and(UiContext::is_current)
    }

    /// A UI-attached context for this exact surface generation. It is never
    /// accepted by durable scheduling methods.
    pub fn context(&self) -> Result<OperationContext, SystemError> {
        let ui = self.ui.clone().ok_or_else(|| {
            SystemError::new(
                ErrorCode::ContextExpired,
                "Notification schedule scope has no live UI attachment",
            )
        })?;
        if !self.is_current() {
            return Err(SystemError::new(
                ErrorCode::ContextExpired,
                "Notification schedule scope UI attachment is stale or disposed",
            ));
        }
        Ok(OperationContext {
            ui: Some(ui),
            ..OperationContext::default()
        })
    }

    pub fn subscribe(&mut self) -> &EventSubscription<NotificationScheduleEvent> {
        self.subscriptions.push(self.service.subscribe());
        self.subscriptions
            .last()
            .expect("a subscription was just retained")
    }

    pub fn subscription_count(&self) -> usize {
        self.subscriptions.len()
    }

    pub fn take_subscription(&mut self) -> Option<EventSubscription<NotificationScheduleEvent>> {
        self.subscriptions.pop()
    }

    /// Record disposal of exactly this generation. Returns true the first time;
    /// repeating disposal is idempotent and never affects another generation.
    pub fn dispose(&self) -> bool {
        if self.is_inert() {
            return false;
        }
        let was_disposed = self.fence.is_disposed(self.surface, self.generation);
        self.fence.dispose(self.surface, self.generation);
        !was_disposed
    }

    pub fn is_disposed(&self) -> bool {
        !self.is_inert() && self.fence.is_disposed(self.surface, self.generation)
    }
}

impl Drop for SurfaceNotificationScheduleScope {
    fn drop(&mut self) {
        self.subscriptions.clear();
        if self.is_current() {
            self.fence.dispose(self.surface, self.generation);
        }
        if self.ui.as_ref().is_some_and(UiContext::is_current) {
            self.owner.dispose();
        }
        drop(self.entity_scope.take());
    }
}

impl std::fmt::Debug for SurfaceNotificationScheduleScope {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SurfaceNotificationScheduleScope")
            .field("surface", &self.surface)
            .field("generation", &self.generation)
            .field("disposed", &self.is_disposed())
            .field("subscriptions", &self.subscriptions.len())
            .finish()
    }
}

/// Bounded drain of a [`NotificationScheduleModel`] event stream.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct NotificationScheduleEventDrain {
    pub resync_required: bool,
    pub events: Vec<NotificationScheduleEvent>,
}

/// A read-only snapshot for a GPUI entity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NotificationScheduleModelSnapshot {
    pub schedules: Vec<NotificationScheduleRecord>,
    pub revision: u64,
    pub resync_required: bool,
    pub surface: SurfaceId,
    pub generation: u64,
}

/// GPUI entity model that owns one surface scope and a bounded event
/// subscription.
pub struct NotificationScheduleModel {
    scope: SurfaceNotificationScheduleScope,
    subscription: EventSubscription<NotificationScheduleEvent>,
    events: VecDeque<NotificationScheduleEvent>,
    resync_required: bool,
}

impl NotificationScheduleModel {
    pub const MAX_RETAINED_EVENTS: usize = DEFAULT_NOTIFICATION_SCHEDULE_EVENT_CAPACITY;

    pub fn new(
        owner: UiOwner,
        service: NotificationScheduleService,
        cx: &mut Context<Self>,
    ) -> Self {
        let scope = SurfaceNotificationScheduleScope::for_entity(owner, service, cx);
        let subscription = scope.service().subscribe();
        Self {
            scope,
            subscription,
            events: VecDeque::new(),
            resync_required: false,
        }
    }

    pub fn service(&self) -> &NotificationScheduleService {
        self.scope.service()
    }

    pub fn surface(&self) -> SurfaceId {
        self.scope.surface()
    }

    pub fn generation(&self) -> u64 {
        self.scope.generation()
    }

    pub fn snapshot(&self) -> NotificationScheduleModelSnapshot {
        NotificationScheduleModelSnapshot {
            schedules: self.service().schedules(),
            revision: self.service().event_revision(),
            resync_required: self.resync_required || !self.scope.is_current(),
            surface: self.surface(),
            generation: self.generation(),
        }
    }

    /// Drain all immediately ready events. Provider overflow and local history
    /// overflow set a sticky `resync_required` flag. A disposed scope drains
    /// nothing and requires a resync.
    pub fn drain_events(&mut self) -> NotificationScheduleEventDrain {
        if !self.scope.is_current() {
            self.resync_required = true;
            return NotificationScheduleEventDrain {
                resync_required: true,
                events: Vec::new(),
            };
        }
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
        NotificationScheduleEventDrain {
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

    pub fn retained_events(&self) -> impl Iterator<Item = &NotificationScheduleEvent> {
        self.events.iter()
    }

    pub fn take_retained_events(&mut self) -> Vec<NotificationScheduleEvent> {
        self.events.drain(..).collect()
    }

    pub fn refresh_schedules(
        &self,
        cx: &mut Context<Self>,
    ) -> Result<NotificationScheduleModelSnapshot, SystemError> {
        if !self.scope.is_current() {
            return Err(SystemError::new(
                ErrorCode::ContextExpired,
                "Notification schedule model scope is disposed",
            ));
        }
        cx.notify();
        Ok(self.snapshot())
    }
}

impl Drop for NotificationScheduleModel {
    fn drop(&mut self) {
        self.events.clear();
    }
}

impl std::fmt::Debug for NotificationScheduleModel {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("NotificationScheduleModel")
            .field("surface", &self.surface())
            .field("generation", &self.generation())
            .field("retained_events", &self.events.len())
            .field("resync_required", &self.resync_required)
            .finish()
    }
}

/// Explicitly finish a request-like future on the UI thread. Durable schedule
/// operations themselves must run on a background owner.
pub fn spawn_local<T: 'static>(
    cx: &App,
    future: impl std::future::Future<Output = T> + 'static,
) -> gpui::Task<T> {
    cx.foreground_executor().spawn(future)
}

/// Compatibility helper for callers that complete a service result on the UI
/// thread.
pub fn complete<T>(value: T) -> crate::ServiceResult<T> {
    Ok(Completion::Completed(value))
}

fn lock<T>(value: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    value
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Build the default deterministic in-memory service used by examples and
/// tests.
pub fn in_memory_service() -> Result<NotificationScheduleService, SystemError> {
    let clock = Arc::new(crate::FakeScheduleClock::new(1_000_000));
    NotificationScheduleService::in_memory(
        Arc::new(FakeNotificationScheduleProvider::new()),
        clock,
        Arc::new(crate::BuiltinTimeZoneResolver::new()),
        Arc::new(ScheduleAuthorization::granting()),
    )
}

#[cfg(all(
    test,
    feature = "gpui",
    any(target_os = "windows", target_os = "macos", target_os = "linux")
))]
mod tests {
    use super::*;
    use crate::FakeScheduleClock;
    use gpui::{AppContext, TestAppContext};

    fn service() -> NotificationScheduleService {
        in_memory_service().expect("service")
    }

    #[gpui::test]
    fn installs_app_global_and_system_services(cx: &mut TestAppContext) {
        cx.update(|cx| {
            gpui_system::gpui_adapter::install_system_services(SystemServices::new(), cx);
            let service = init_notification_schedule(
                cx,
                Arc::new(FakeNotificationScheduleProvider::new()),
                Arc::new(FakeScheduleClock::new(1_000)),
                Arc::new(crate::BuiltinTimeZoneResolver::new()),
                Arc::new(ScheduleAuthorization::granting()),
            )
            .expect("init");
            let resolved = notification_schedule_service(cx).expect("app global");
            assert!(service.shares_state_with(&resolved));
            assert!(cx.notification_schedule_service().is_ok());
            let registered = cx
                .try_global::<SystemServices>()
                .expect("services")
                .service::<NotificationScheduleService>()
                .expect("registry");
            assert!(service.shares_state_with(&registered));
        });
    }

    #[gpui::test]
    fn missing_service_is_typed(cx: &mut TestAppContext) {
        cx.update(|cx| {
            assert_eq!(
                notification_schedule_service(cx).unwrap_err().code,
                ErrorCode::Unavailable
            );
        });
    }

    #[gpui::test]
    fn surface_scope_fences_exact_generation_and_model_is_bounded(cx: &mut TestAppContext) {
        let service = cx.update(|_| service());
        let owner = UiOwner::new(SurfaceId(41));
        let probe = owner.clone();
        let mut scope =
            cx.update(|_| SurfaceNotificationScheduleScope::new(owner.clone(), service.clone()));
        assert_eq!(scope.surface(), SurfaceId(41));
        assert_eq!(scope.generation(), 1);
        assert!(scope.is_current());
        assert!(scope.dispose());
        assert!(!scope.dispose());
        assert!(!scope.is_current());
        assert!(scope.context().is_err());
        assert_eq!(scope.subscription_count(), 0);
        let _subscription = scope.subscribe();
        assert_eq!(scope.subscription_count(), 1);
        drop(scope);
        assert!(owner.context(None).is_err());

        let model_owner = cx.update(|_| UiOwner::new(SurfaceId(42)));
        let model_probe = model_owner.clone();
        let model = cx.update(|cx| {
            let service = service.clone();
            cx.new(|cx| NotificationScheduleModel::new(model_owner.clone(), service, cx))
        });
        cx.update(|cx| {
            model.update(cx, |model, _cx| {
                assert_eq!(model.surface(), SurfaceId(42));
                assert_eq!(model.generation(), 1);
                assert_eq!(NotificationScheduleModel::MAX_RETAINED_EVENTS, 64);
                let drain = model.drain_events();
                assert!(!drain.resync_required);
            });
        });
        drop(model);
        cx.update(|_| {});
        assert!(model_probe.context(None).is_err());
        assert!(probe.context(None).is_err());
    }
}
