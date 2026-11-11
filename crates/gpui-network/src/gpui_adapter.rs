//! Optional GPUI adapter over the distribution's coordinated GPUI identity.
//!
//! Installation and lookup must run on the owning GPUI UI thread. The core
//! [`NetworkService`] remains `Send + Sync`; this adapter only adds UI ownership,
//! `App` globals, surface scopes and an entity model.

use crate::{
    ClientId, ClientOwnerScope, DEFAULT_NETWORK_EVENT_CAPACITY, EventSubscription, NetworkEvent,
    NetworkProvider, NetworkService, OperationContext, PolicyEpoch, ScopedRevalidation,
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

impl Global for NetworkService {}

/// Install the service into GPUI's `App` global.
pub fn install_network_service(service: NetworkService, cx: &mut App) {
    cx.set_global(service);
}

/// Register a service in the typed `SystemServices` registry.
pub fn install_network_in_services(
    services: &mut SystemServices,
    service: NetworkService,
) -> Result<(), SystemError> {
    services.register(service)
}

/// Build a service, install it as the `App` global and register it in
/// `SystemServices` for code that resolves services through the registry.
pub fn init_network(cx: &mut App, provider: Arc<dyn NetworkProvider>) -> NetworkService {
    let service = NetworkService::new(provider);
    cx.set_global(service.clone());
    cx.default_global::<SystemServices>()
        .override_provider(service.clone());
    service
}

/// Resolve the service from the `App` global first, then from `SystemServices`.
pub fn network_service(cx: &App) -> Result<NetworkService, SystemError> {
    if let Some(service) = cx.try_global::<NetworkService>() {
        return Ok(service.clone());
    }
    let services = cx.try_global::<SystemServices>().ok_or_else(|| {
        SystemError::new(
            ErrorCode::Unavailable,
            "NetworkService and SystemServices are not installed",
        )
    })?;
    services
        .service::<NetworkService>()
        .map(|service| (*service).clone())
}

/// Convenience methods for GPUI applications.
pub trait NetworkAppExt {
    fn install_network_service(&mut self, service: NetworkService);
    fn init_network(&mut self, provider: Arc<dyn NetworkProvider>) -> NetworkService;
    fn network_service(&self) -> Result<NetworkService, SystemError>;
}

impl NetworkAppExt for App {
    fn install_network_service(&mut self, service: NetworkService) {
        install_network_service(service, self);
    }

    fn init_network(&mut self, provider: Arc<dyn NetworkProvider>) -> NetworkService {
        init_network(self, provider)
    }

    fn network_service(&self) -> Result<NetworkService, SystemError> {
        network_service(self)
    }
}

/// Entity-/surface-owned network scope.
///
/// The scope retains subscriptions and an explicit client lease. On drop it
/// disposes *only its own generation*, so dropping an older generation never
/// revokes clients recreated for a newer generation of the same surface.
pub struct NetworkSurfaceScope {
    owner: UiOwner,
    service: NetworkService,
    surface: SurfaceId,
    generation: u64,
    ui: Option<UiContext>,
    subscriptions: Vec<EventSubscription<NetworkEvent>>,
    entity_scope: Option<EntityScope>,
    client: Option<crate::NetworkClientLease>,
}

impl NetworkSurfaceScope {
    /// Create a scope for the owner's current attachment generation.
    ///
    /// A disposed owner produces an inert scope (`SurfaceId(0)`, generation 0)
    /// whose [`Self::context`] reports `ContextExpired`; it never panics.
    pub fn new(owner: UiOwner, service: NetworkService) -> Self {
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
                client: None,
            },
        }
    }

    /// Fallible form of [`Self::new`] for callers that need the typed error.
    pub fn try_new(owner: UiOwner, service: NetworkService) -> Result<Self, SystemError> {
        let ui = owner.context(None)?;
        Ok(Self::with_ui(owner, service, ui))
    }

    fn with_ui(owner: UiOwner, service: NetworkService, ui: UiContext) -> Self {
        Self {
            owner,
            service,
            surface: ui.surface,
            generation: ui.generation,
            ui: Some(ui),
            subscriptions: Vec::new(),
            entity_scope: None,
            client: None,
        }
    }

    /// Create a scope tied to a GPUI entity lifecycle.
    pub fn for_entity<T: 'static>(
        owner: UiOwner,
        service: NetworkService,
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

    pub fn service(&self) -> &NetworkService {
        &self.service
    }

    pub fn surface(&self) -> SurfaceId {
        self.surface
    }

    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// A UI-attached execution context for this exact surface generation.
    pub fn context(&self) -> Result<OperationContext, SystemError> {
        let ui = self.ui.clone().ok_or_else(|| {
            SystemError::new(
                ErrorCode::ContextExpired,
                "Network surface scope has no live UI attachment",
            )
        })?;
        if !ui.is_current() {
            return Err(SystemError::new(
                ErrorCode::ContextExpired,
                "Network surface scope UI attachment is stale",
            ));
        }
        Ok(OperationContext {
            ui: Some(ui),
            ..OperationContext::default()
        })
    }

    /// Register the scope's surface-generation client lease.
    pub fn register_client(&mut self) -> Result<ClientId, SystemError> {
        if let Some(client) = &self.client
            && client.is_active()
        {
            return Ok(client.client_id());
        }
        let context = self.context()?;
        let scope = ClientOwnerScope::surface(self.surface, self.generation);
        match self.service.register_client(scope, &context)? {
            Completion::Completed(client) => {
                let id = client.client_id();
                self.client = Some(client);
                Ok(id)
            }
            Completion::Cancelled(reason) => Err(SystemError::new(
                ErrorCode::Unavailable,
                cancelled_message(reason),
            )),
        }
    }

    pub fn client_id(&self) -> Option<ClientId> {
        self.client
            .as_ref()
            .filter(|client| client.is_active())
            .map(crate::NetworkClientLease::client_id)
    }

    pub fn client_lease(&self) -> Option<&crate::NetworkClientLease> {
        self.client.as_ref()
    }

    pub fn policy_epoch(&self) -> PolicyEpoch {
        self.service.policy_epoch()
    }

    pub fn subscribe(&mut self) -> &EventSubscription<NetworkEvent> {
        self.subscriptions.push(self.service.subscribe());
        self.subscriptions
            .last()
            .expect("a subscription was just retained")
    }

    pub fn subscription_count(&self) -> usize {
        self.subscriptions.len()
    }

    /// Remove and return a retained subscription for an owned reader loop.
    pub fn take_subscription(&mut self) -> Option<EventSubscription<NetworkEvent>> {
        self.subscriptions.pop()
    }

    pub fn drain_client_directives(&self) -> Vec<ScopedRevalidation> {
        self.client
            .as_ref()
            .map(crate::NetworkClientLease::drain_directives)
            .unwrap_or_default()
    }
}

impl Drop for NetworkSurfaceScope {
    fn drop(&mut self) {
        self.subscriptions.clear();
        if self.ui.is_some() {
            // Generation-checked: an older generation never disposes a newer
            // client lease on the same surface.
            self.service
                .dispose_surface_generation(self.surface, self.generation);
        }
        drop(self.client.take());
        // A stale scope must not invalidate a newer generation that shares the
        // same `UiOwner` (native attachments advance generation in place).
        if self.ui.as_ref().is_some_and(UiContext::is_current) {
            self.owner.dispose();
        }
        drop(self.entity_scope.take());
    }
}

impl std::fmt::Debug for NetworkSurfaceScope {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("NetworkSurfaceScope")
            .field("surface", &self.surface)
            .field("generation", &self.generation)
            .field("subscriptions", &self.subscriptions.len())
            .field("client", &self.client_id())
            .finish()
    }
}

fn cancelled_message(reason: crate::CancelReason) -> &'static str {
    match reason {
        crate::CancelReason::User => "Network operation was cancelled by the user",
        crate::CancelReason::Caller => "Network operation was cancelled by the caller",
        crate::CancelReason::OwnerDisposed => "Network operation owner was disposed",
        crate::CancelReason::ProcessPolicy => "Network operation was cancelled by process policy",
    }
}

/// Bounded drain of a [`NetworkEntityModel`] event stream.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct NetworkEventDrain {
    /// True when the bounded stream reported `Event::ResyncRequired`, or the
    /// model's local history had to discard an event.
    pub resync_required: bool,
    /// Events observed by this drain in delivery order.
    pub events: Vec<NetworkEvent>,
}

/// GPUI entity model that owns one surface network scope and its client lease.
///
/// Create it with `cx.new(|cx| NetworkEntityModel::new(owner, service, cx))`.
/// It retains a bounded event subscription, keeps at most
/// [`NetworkEntityModel::MAX_RETAINED_EVENTS`] events, and releases its client
/// plus surface generation on drop/entity release.
pub struct NetworkEntityModel {
    scope: NetworkSurfaceScope,
    subscription: EventSubscription<NetworkEvent>,
    events: VecDeque<NetworkEvent>,
    resync_required: bool,
    client_error: Option<ErrorCode>,
}

impl NetworkEntityModel {
    /// Maximum number of recent events retained by the model.
    pub const MAX_RETAINED_EVENTS: usize = DEFAULT_NETWORK_EVENT_CAPACITY;

    /// Build the model and tie its scope to the entity release callback.
    pub fn new(owner: UiOwner, service: NetworkService, cx: &mut Context<Self>) -> Self {
        let mut scope = NetworkSurfaceScope::for_entity(owner, service, cx);
        let client_error = scope.register_client().err().map(|error| error.code);
        let subscription = scope.service().subscribe();
        Self {
            scope,
            subscription,
            events: VecDeque::new(),
            resync_required: false,
            client_error,
        }
    }

    pub fn service(&self) -> &NetworkService {
        self.scope.service()
    }

    pub fn surface(&self) -> SurfaceId {
        self.scope.surface()
    }

    pub fn generation(&self) -> u64 {
        self.scope.generation()
    }

    pub fn client_id(&self) -> Option<ClientId> {
        self.scope.client_id()
    }

    pub fn client_error(&self) -> Option<ErrorCode> {
        self.client_error
    }

    pub fn policy_epoch(&self) -> PolicyEpoch {
        self.scope.policy_epoch()
    }

    pub fn acknowledge_epoch(&self, epoch: PolicyEpoch) -> bool {
        self.scope
            .client_lease()
            .is_some_and(|client| client.acknowledge_epoch(epoch))
    }

    pub fn drain_client_directives(&self) -> Vec<ScopedRevalidation> {
        self.scope.drain_client_directives()
    }

    /// Drain all currently available events without blocking.
    ///
    /// A provider overflow (`Event::ResyncRequired`) is sticky until
    /// [`Self::clear_resync_required`] is called. A local history overflow also
    /// sets it because older retained events were discarded.
    pub fn drain_events(&mut self) -> NetworkEventDrain {
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
        NetworkEventDrain {
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

    pub fn retained_events(&self) -> impl Iterator<Item = &NetworkEvent> {
        self.events.iter()
    }

    pub fn take_retained_events(&mut self) -> Vec<NetworkEvent> {
        self.events.drain(..).collect()
    }

    pub fn refresh_snapshot(
        &mut self,
        cx: &mut Context<Self>,
    ) -> ServiceResult<crate::NetworkSnapshot> {
        let context = self.scope.context()?;
        let result = self.service().snapshot(&context);
        cx.notify();
        result
    }

    pub fn set_app_phase(&mut self, phase: crate::AppPhase, cx: &mut Context<Self>) {
        self.service().set_app_phase(phase);
        cx.notify();
    }
}

impl Drop for NetworkEntityModel {
    fn drop(&mut self) {
        self.events.clear();
    }
}

impl std::fmt::Debug for NetworkEntityModel {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("NetworkEntityModel")
            .field("surface", &self.surface())
            .field("generation", &self.generation())
            .field("client", &self.client_id())
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
    use crate::{FakeNetworkProvider, NetworkSnapshot, PlatformTarget, TransportKind};
    use gpui::{AppContext, TestAppContext};

    fn linux_service(fake: FakeNetworkProvider) -> NetworkService {
        NetworkService::with_platform(Arc::new(fake), PlatformTarget::Linux)
    }

    #[gpui::test]
    fn installs_app_global_and_system_services(cx: &mut TestAppContext) {
        cx.update(|cx| {
            gpui_system::gpui_adapter::install_system_services(SystemServices::new(), cx);
            let service = init_network(cx, Arc::new(FakeNetworkProvider::new()));
            let resolved = network_service(cx).expect("app global");
            assert!(service.shares_state_with(&resolved));
            assert!(cx.network_service().is_ok());
            let registered = cx
                .try_global::<SystemServices>()
                .expect("services")
                .service::<NetworkService>()
                .expect("registry");
            assert!(service.shares_state_with(&registered));
        });
    }

    #[gpui::test]
    fn missing_service_is_typed(cx: &mut TestAppContext) {
        cx.update(|cx| {
            assert_eq!(
                network_service(cx).unwrap_err().code,
                ErrorCode::Unavailable
            );
        });
    }

    #[gpui::test]
    fn older_generation_drop_never_revokes_newer_generation(cx: &mut TestAppContext) {
        let service = cx.update(|_| linux_service(FakeNetworkProvider::new()));
        let owner = gpui_system::UiOwner::new(SurfaceId(21));
        let subscription = service.subscribe();
        let mut first = cx.update(|_| NetworkSurfaceScope::new(owner.clone(), service.clone()));
        let first_id = first.register_client().expect("first client");
        owner.advance_generation().expect("generation advance");
        let mut second = cx.update(|_| NetworkSurfaceScope::new(owner.clone(), service.clone()));
        let second_id = second.register_client().expect("second client");
        assert_ne!(first_id, second_id);
        assert_eq!(service.active_client_count(), 2);

        drop(first);
        let events = gpui_lifecycle::drain_ready(&subscription);
        assert!(events.iter().any(|envelope| matches!(
            &envelope.payload,
            crate::Event::Value(NetworkEvent::ClientRevoked {
                client_id,
                reason: crate::ClientRevocationReason::SurfaceDisposed,
                ..
            }) if *client_id == first_id
        )));
        assert_eq!(service.active_client_count(), 1);
        assert!(
            second
                .client_lease()
                .is_some_and(crate::NetworkClientLease::is_active)
        );
        drop(second);
        assert_eq!(service.active_client_count(), 0);
        assert!(owner.context(None).is_err());
    }

    #[gpui::test]
    fn entity_model_owns_scope_and_drains_bounded_events(cx: &mut TestAppContext) {
        let (entity, owner, service) = cx.update(|cx| {
            let fake = FakeNetworkProvider::new();
            let service = linux_service(fake);
            let owner = gpui_system::UiOwner::new(SurfaceId(22));
            let entity = cx.new(|cx| NetworkEntityModel::new(owner.clone(), service.clone(), cx));
            (entity, owner, service)
        });
        cx.update(|cx| {
            entity.update(cx, |model, cx| {
                assert_eq!(model.surface(), SurfaceId(22));
                assert!(model.client_id().is_some());
                assert_eq!(model.client_error(), None);
                model.set_app_phase(crate::AppPhase::ForegroundActive, cx);
                let _ = model.drain_events();
                assert_eq!(NetworkEntityModel::MAX_RETAINED_EVENTS, 64);
            });
        });
        drop(entity);
        cx.update(|_| {});
        assert!(owner.context(None).is_err());
        assert_eq!(service.active_client_count(), 0);
    }

    #[test]
    fn snapshot_helper_builds_routable_state() {
        let snapshot = NetworkSnapshot::routable(TransportKind::Ethernet, false, true, true)
            .expect("routable");
        assert!(snapshot.is_routable());
    }
}
