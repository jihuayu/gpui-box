//! Optional GPUI adapter over the coordinated GPUI identity.
//!
//! Installation and lookup must run on the owning GPUI UI thread. The core
//! [`PushService`] remains `Send + Sync`; this adapter only adds UI ownership,
//! `App` globals, surface-scoped leases and an entity model.
//!
//! The adapter never reads credentials, never registers a device and never
//! performs an activation handoff implicitly. Those remain background service
//! calls owned by the host.

use crate::{
    DEFAULT_PUSH_EVENT_CAPACITY, Event, EventSubscription, OperationContext, PushEvent,
    PushService, PushSnapshot, SurfaceId, SystemError, SystemServices, UiContext, UiOwner,
};
use gpui::{App, Context, Global};
use gpui_system::{ErrorCode, gpui_adapter::EntityScope};
use std::{
    collections::VecDeque,
    pin::Pin,
    task::{Context as TaskContext, Poll, Waker},
};

impl Global for PushService {}

/// Install the service into GPUI's `App` global.
pub fn install_push_service(service: PushService, cx: &mut App) {
    cx.set_global(service);
}

/// Register the service in the typed `SystemServices` registry.
pub fn install_push_in_services(
    services: &mut SystemServices,
    service: PushService,
) -> Result<(), SystemError> {
    services.register(service)
}

/// Install the service as the `App` global and in `SystemServices`.
pub fn init_push(cx: &mut App, service: PushService) -> PushService {
    cx.set_global(service.clone());
    cx.default_global::<SystemServices>()
        .override_provider(service.clone());
    service
}

/// Resolve the service from the `App` global first, then `SystemServices`.
pub fn push_service(cx: &App) -> Result<PushService, SystemError> {
    if let Some(service) = cx.try_global::<PushService>() {
        return Ok(service.clone());
    }
    let services = cx.try_global::<SystemServices>().ok_or_else(|| {
        SystemError::new(
            ErrorCode::Unavailable,
            "PushService and SystemServices are not installed",
        )
    })?;
    services
        .service::<PushService>()
        .map(|service| (*service).clone())
}

/// Convenience methods for GPUI applications.
pub trait PushAppExt {
    fn install_push_service(&mut self, service: PushService);
    fn init_push(&mut self, service: PushService) -> PushService;
    fn push_service(&self) -> Result<PushService, SystemError>;
}

impl PushAppExt for App {
    fn install_push_service(&mut self, service: PushService) {
        install_push_service(service, self);
    }

    fn init_push(&mut self, service: PushService) -> PushService {
        init_push(self, service)
    }

    fn push_service(&self) -> Result<PushService, SystemError> {
        push_service(self)
    }
}

/// Surface-owned push scope.
///
/// The scope registers and disposes exactly one `(SurfaceId, generation)`. On
/// drop it disposes only that generation, so dropping a stale scope never
/// clobbers a newer generation of the same surface. The drop path is
/// poison-safe and never panics.
pub struct SurfacePushScope {
    owner: UiOwner,
    service: PushService,
    surface: SurfaceId,
    generation: u64,
    ui: Option<UiContext>,
    entity_scope: Option<EntityScope>,
}

impl SurfacePushScope {
    /// Create a scope for the owner's current attachment generation. A disposed
    /// owner produces an inert scope whose [`Self::context`] reports expiry.
    pub fn new(owner: UiOwner, service: PushService) -> Self {
        match owner.context(None) {
            Ok(ui) => Self::with_ui(owner, service, ui),
            Err(_) => Self {
                owner,
                service,
                surface: SurfaceId(0),
                generation: 0,
                ui: None,
                entity_scope: None,
            },
        }
    }

    pub fn try_new(owner: UiOwner, service: PushService) -> Result<Self, SystemError> {
        let ui = owner.context(None)?;
        Ok(Self::with_ui(owner, service, ui))
    }

    fn with_ui(owner: UiOwner, service: PushService, ui: UiContext) -> Self {
        service.register_surface_generation(ui.surface, ui.generation);
        Self {
            owner,
            service,
            surface: ui.surface,
            generation: ui.generation,
            ui: Some(ui),
            entity_scope: None,
        }
    }

    /// Tie the scope to a GPUI entity release callback.
    pub fn for_entity<T: 'static>(owner: UiOwner, service: PushService, cx: &Context<T>) -> Self {
        let entity_scope = EntityScope::new(owner.clone(), cx);
        let mut scope = Self::new(owner, service);
        scope.entity_scope = Some(entity_scope);
        scope
    }

    pub fn service(&self) -> &PushService {
        &self.service
    }

    pub fn owner(&self) -> &UiOwner {
        &self.owner
    }

    pub fn surface(&self) -> SurfaceId {
        self.surface
    }

    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// True when this scope still holds a live UI attachment.
    pub fn is_current(&self) -> bool {
        self.ui.as_ref().is_some_and(UiContext::is_current)
    }

    /// A background-safe operation context tagged with this exact surface
    /// generation. It is never used for credential access.
    pub fn context(&self) -> Result<OperationContext, SystemError> {
        let ui = self.ui.clone().ok_or_else(|| {
            SystemError::new(
                ErrorCode::ContextExpired,
                "Push surface scope has no live UI attachment",
            )
        })?;
        if !ui.is_current() {
            return Err(SystemError::new(
                ErrorCode::ContextExpired,
                "Push surface scope UI attachment is stale",
            ));
        }
        Ok(OperationContext {
            ui: Some(ui),
            ..OperationContext::default()
        })
    }

    /// A public-safe snapshot for this scope.
    pub fn snapshot(&self) -> PushSnapshot {
        self.service.snapshot()
    }
}

impl Drop for SurfacePushScope {
    fn drop(&mut self) {
        if self.ui.is_some() {
            // Exact generation: an older scope never disposes a newer one.
            self.service
                .dispose_surface_generation(self.surface, self.generation);
        }
        drop(self.entity_scope.take());
    }
}

impl std::fmt::Debug for SurfacePushScope {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SurfacePushScope")
            .field("surface", &self.surface)
            .field("generation", &self.generation)
            .finish_non_exhaustive()
    }
}

/// Alias matching the sibling adapter naming.
pub type SurfacePush = SurfacePushScope;

/// Bounded drain of a [`PushModel`] event stream.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PushEventDrain {
    pub resync_required: bool,
    pub events: Vec<PushEvent>,
}

/// A GPUI entity model that owns one surface scope and a bounded event ring.
pub struct PushModel {
    scope: SurfacePushScope,
    subscription: EventSubscription<PushEvent>,
    events: VecDeque<PushEvent>,
    resync_required: bool,
    max_events: usize,
    snapshot: PushSnapshot,
}

impl PushModel {
    /// The default retained-event capacity, matching the service stream.
    pub const MAX_RETAINED_EVENTS: usize = DEFAULT_PUSH_EVENT_CAPACITY;

    pub fn new(owner: UiOwner, service: PushService, cx: &mut Context<Self>) -> Self {
        Self::with_capacity(owner, service, cx, Self::MAX_RETAINED_EVENTS)
    }

    /// Create a model whose retained ring is bounded by `max_events`. Overflow
    /// sets a sticky `resync_required` flag.
    pub fn with_capacity(
        owner: UiOwner,
        service: PushService,
        cx: &mut Context<Self>,
        max_events: usize,
    ) -> Self {
        let scope = SurfacePushScope::for_entity(owner, service, cx);
        let subscription = scope.service().subscribe();
        let snapshot = scope.service().snapshot();
        Self {
            scope,
            subscription,
            events: VecDeque::new(),
            resync_required: false,
            max_events: max_events.max(1),
            snapshot,
        }
    }

    pub fn service(&self) -> &PushService {
        self.scope.service()
    }

    pub fn surface(&self) -> SurfaceId {
        self.scope.surface()
    }

    pub fn generation(&self) -> u64 {
        self.scope.generation()
    }

    pub fn max_events(&self) -> usize {
        self.max_events
    }

    pub fn resync_required(&self) -> bool {
        self.resync_required
    }

    pub fn clear_resync_required(&mut self) {
        self.resync_required = false;
    }

    pub fn snapshot(&self) -> &PushSnapshot {
        &self.snapshot
    }

    pub fn retained_events(&self) -> impl Iterator<Item = &PushEvent> {
        self.events.iter()
    }

    pub fn take_retained_events(&mut self) -> Vec<PushEvent> {
        self.events.drain(..).collect()
    }

    /// Refresh the cached snapshot. The caller decides whether to `notify`.
    pub fn refresh_snapshot(&mut self) -> PushSnapshot {
        let snapshot = self.scope.service().snapshot();
        self.snapshot = snapshot.clone();
        snapshot
    }

    /// Drain all immediately ready events. Provider overflow and local ring
    /// overflow set a sticky `resync_required` flag.
    pub fn drain_events(&mut self) -> PushEventDrain {
        let mut events = Vec::new();
        while let Ok(mut next) = self.subscription.try_next() {
            let waker = Waker::noop();
            let mut task = TaskContext::from_waker(waker);
            match Pin::new(&mut next).poll(&mut task) {
                Poll::Ready(Some(envelope)) => match envelope.payload {
                    Event::Value(event) => {
                        if self.events.len() >= self.max_events {
                            self.events.pop_front();
                            self.resync_required = true;
                        }
                        self.events.push_back(event.clone());
                        events.push(event);
                    }
                    Event::ResyncRequired => {
                        self.resync_required = true;
                    }
                },
                Poll::Ready(None) | Poll::Pending => break,
            }
        }
        if !events.is_empty() || self.resync_required {
            self.snapshot = self.scope.service().snapshot();
        }
        PushEventDrain {
            resync_required: self.resync_required,
            events,
        }
    }
}

impl Drop for PushModel {
    fn drop(&mut self) {
        self.events.clear();
    }
}

impl std::fmt::Debug for PushModel {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("PushModel")
            .field("surface", &self.surface())
            .field("generation", &self.generation())
            .field("retained_events", &self.events.len())
            .field("max_events", &self.max_events)
            .field("resync_required", &self.resync_required)
            .finish()
    }
}

#[cfg(all(
    test,
    feature = "gpui",
    any(target_os = "windows", target_os = "macos", target_os = "linux")
))]
mod tests {
    use super::*;
    use crate::{
        FakePushProvider, FixedPermissionProbe, ManualPushClock, MemoryCredentials,
        NoopActivationHandoffSink, PushConfig, PushProviderKind, PushRuntime, RecordingPresenter,
        Secrets,
    };
    use gpui::{AppContext, TestAppContext};
    use std::sync::Arc;

    fn service() -> PushService {
        let secrets =
            Secrets::with_provider("gpui-push-adapter", Arc::new(MemoryCredentials::new()))
                .expect("secrets");
        PushService::in_memory(
            vec![Arc::new(FakePushProvider::ready(
                PushProviderKind::Fcm,
                "fcm-adapter",
            ))],
            secrets,
            PushConfig::default(),
            PushRuntime::new(
                Arc::new(ManualPushClock::new(1_000)),
                Arc::new(RecordingPresenter::new()),
                Arc::new(FixedPermissionProbe::default()),
                Arc::new(NoopActivationHandoffSink),
            ),
        )
        .expect("service")
    }

    #[gpui::test]
    fn installs_app_global_and_system_services(cx: &mut TestAppContext) {
        let service = service();
        cx.update(|cx| {
            gpui_system::gpui_adapter::install_system_services(SystemServices::new(), cx);
            let installed = init_push(cx, service.clone());
            assert!(service.shares_state_with(&installed));
            let resolved = push_service(cx).expect("app global");
            assert!(service.shares_state_with(&resolved));
            assert!(cx.push_service().is_ok());
            let registered = cx
                .try_global::<SystemServices>()
                .expect("services")
                .service::<PushService>()
                .expect("registry");
            assert!(service.shares_state_with(&registered));
        });
    }

    #[gpui::test]
    fn missing_service_is_typed(cx: &mut TestAppContext) {
        cx.update(|cx| {
            assert_eq!(push_service(cx).unwrap_err().code, ErrorCode::Unavailable);
        });
    }

    #[gpui::test]
    fn stale_generation_never_clobbers_a_newer_one(cx: &mut TestAppContext) {
        let service = service();
        let owner = UiOwner::new(SurfaceId(202));
        let stale = cx.update(|_| SurfacePushScope::new(owner.clone(), service.clone()));
        assert_eq!(stale.generation(), 1);
        assert_eq!(service.surface_generation(SurfaceId(202)), Some(1));
        owner.advance_generation().expect("advance generation");
        let current = cx.update(|_| SurfacePushScope::new(owner.clone(), service.clone()));
        assert_eq!(current.generation(), 2);
        assert_eq!(service.surface_generation(SurfaceId(202)), Some(2));
        drop(stale);
        assert_eq!(service.surface_generation(SurfaceId(202)), Some(2));
        drop(current);
        assert_eq!(service.surface_generation(SurfaceId(202)), None);
    }

    #[gpui::test]
    fn entity_model_drains_a_bounded_ring(cx: &mut TestAppContext) {
        let service = service();
        let owner = UiOwner::new(SurfaceId(303));
        let model = cx.update(|cx| {
            cx.new(|cx| PushModel::with_capacity(owner.clone(), service.clone(), cx, 1))
        });
        cx.update(|_| {
            let context = OperationContext::default();
            let _ = service.discover_providers(crate::ExecutionOwner::Background, &context);
            let _ = service.discover_providers(crate::ExecutionOwner::Background, &context);
        });
        cx.update(|cx| {
            model.update(cx, |model, _cx| {
                assert_eq!(model.surface(), SurfaceId(303));
                assert_eq!(model.generation(), 1);
                assert_eq!(model.max_events(), 1);
                assert_eq!(PushModel::MAX_RETAINED_EVENTS, 64);
                let drain = model.drain_events();
                assert_eq!(drain.events.len(), 2);
                assert!(drain.resync_required);
                assert_eq!(model.retained_events().count(), 1);
                assert!(model.resync_required());
                let _ = model.refresh_snapshot();
            });
        });
        drop(model);
        cx.update(|_| {});
        assert!(owner.context(None).is_err());
    }
}
