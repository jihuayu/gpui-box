//! Optional GPUI adapter over the distribution's coordinated GPUI identity.
//!
//! Installation and lookup must run on the owning GPUI UI thread. The core
//! [`TrayService`] remains UI-local; this adapter only adds `App` ownership,
//! `SystemServices` registration and entity-owned surface cleanup.

use crate::{
    Completion, EventSubscription, OperationContext, ServiceResult, StatusItemLease,
    StatusItemSnapshot, StatusItemSpec, SurfaceId, SystemError, SystemPreferences, SystemServices,
    TrayEvent, TrayProvider, TrayService, UiContext, UiOwner,
};
use gpui::{App, Context, Global};
use gpui_system::{ErrorCode, gpui_adapter::EntityScope};
use std::future::Future;

impl Global for TrayService {}

/// Install the service into GPUI's `App` global.
pub fn install_tray_service(service: TrayService, cx: &mut App) {
    cx.set_global(service);
}

/// Register a service in the typed `SystemServices` registry.
pub fn install_tray_in_services(
    services: &mut SystemServices,
    service: TrayService,
) -> Result<(), SystemError> {
    services.register(service)
}

/// Build a service, install it as the `App` global and register it in
/// `SystemServices` for code that resolves services through the registry.
pub fn init_tray(cx: &mut App, provider: std::rc::Rc<dyn TrayProvider>) -> TrayService {
    let service = TrayService::new(provider);
    cx.set_global(service.clone());
    cx.default_global::<SystemServices>()
        .override_provider(service.clone());
    service
}

/// Build a service with a live menu command service and install both handles.
pub fn init_tray_with_menus(
    cx: &mut App,
    provider: std::rc::Rc<dyn TrayProvider>,
    menus: crate::MenuCommandService,
) -> TrayService {
    let service = TrayService::with_menus(provider, menus);
    cx.set_global(service.clone());
    cx.default_global::<SystemServices>()
        .override_provider(service.clone());
    service
}

/// Resolve the service from the `App` global first, then from `SystemServices`.
pub fn tray_service(cx: &App) -> Result<TrayService, SystemError> {
    if let Some(service) = cx.try_global::<TrayService>() {
        return Ok(service.clone());
    }
    let services = cx.try_global::<SystemServices>().ok_or_else(|| {
        SystemError::new(
            ErrorCode::Unavailable,
            "TrayService and SystemServices are not installed",
        )
    })?;
    services
        .service::<TrayService>()
        .map(|service| (*service).clone())
}

/// Convenience methods for GPUI applications.
pub trait TrayAppExt {
    fn install_tray_service(&mut self, service: TrayService);
    fn init_tray(&mut self, provider: std::rc::Rc<dyn TrayProvider>) -> TrayService;
    fn init_tray_with_menus(
        &mut self,
        provider: std::rc::Rc<dyn TrayProvider>,
        menus: crate::MenuCommandService,
    ) -> TrayService;
    fn tray_service(&self) -> Result<TrayService, SystemError>;
}

impl TrayAppExt for App {
    fn install_tray_service(&mut self, service: TrayService) {
        install_tray_service(service, self);
    }

    fn init_tray(&mut self, provider: std::rc::Rc<dyn TrayProvider>) -> TrayService {
        init_tray(self, provider)
    }

    fn init_tray_with_menus(
        &mut self,
        provider: std::rc::Rc<dyn TrayProvider>,
        menus: crate::MenuCommandService,
    ) -> TrayService {
        init_tray_with_menus(self, provider, menus)
    }

    fn tray_service(&self) -> Result<TrayService, SystemError> {
        tray_service(self)
    }
}

/// Synchronize a host lifecycle snapshot into the tray service.
pub fn sync_lifecycle(service: &TrayService, lifecycle: &crate::LifecycleState) {
    service.notify_lifecycle(lifecycle);
}

/// Synchronize appearance/high-contrast and DPI preferences, re-projecting every
/// hosted icon deterministically.
pub fn sync_preferences(
    service: &TrayService,
    preferences: &SystemPreferences,
    scale_factor_milli: u32,
) {
    service.notify_preferences(preferences.clone());
    service.notify_scale_factor_milli(scale_factor_milli);
}

/// Entity-/surface-owned tray scope.
///
/// The scope attaches the surface, retains event subscriptions and creates
/// surface-scoped status items. On drop it disposes the surface (removing every
/// surface-scoped lease) and disposes the `UiOwner`; it never leaves a native
/// item active.
pub struct TraySurfaceScope {
    owner: UiOwner,
    service: TrayService,
    surface: SurfaceId,
    generation: u64,
    ui: Option<UiContext>,
    subscriptions: Vec<EventSubscription<TrayEvent>>,
    entity_scope: Option<EntityScope>,
}

impl TraySurfaceScope {
    /// Create a scope for the owner's current attachment generation.
    pub fn new(owner: UiOwner, service: TrayService) -> Self {
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

    pub fn try_new(owner: UiOwner, service: TrayService) -> Result<Self, SystemError> {
        let ui = owner.context(None)?;
        Ok(Self::with_ui(owner, service, ui))
    }

    fn with_ui(owner: UiOwner, service: TrayService, ui: UiContext) -> Self {
        let surface = ui.surface;
        let generation = ui.generation;
        service.attach_surface(surface, generation, true);
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

    /// Tie the scope to a GPUI entity's release callback in addition to `Drop`.
    pub fn for_entity<T: 'static>(owner: UiOwner, service: TrayService, cx: &Context<T>) -> Self {
        let entity_scope = EntityScope::new(owner.clone(), cx);
        let mut scope = Self::new(owner, service);
        scope.entity_scope = Some(entity_scope);
        scope
    }

    pub fn owner(&self) -> &UiOwner {
        &self.owner
    }

    pub fn service(&self) -> &TrayService {
        &self.service
    }

    pub fn surface(&self) -> SurfaceId {
        self.surface
    }

    pub fn generation(&self) -> u64 {
        self.generation
    }

    pub fn context(&self) -> Result<OperationContext, SystemError> {
        let ui = self.ui.clone().ok_or_else(|| {
            SystemError::new(
                ErrorCode::ContextExpired,
                "Tray surface scope has no live UI attachment",
            )
        })?;
        if !ui.is_current() {
            return Err(SystemError::new(
                ErrorCode::ContextExpired,
                "Tray surface scope UI attachment is stale",
            ));
        }
        Ok(OperationContext {
            ui: Some(ui),
            ..OperationContext::default()
        })
    }

    pub fn subscribe(&mut self) -> &EventSubscription<TrayEvent> {
        self.subscriptions.push(self.service.subscribe());
        self.subscriptions
            .last()
            .expect("a subscription was just retained")
    }

    pub fn subscription_count(&self) -> usize {
        self.subscriptions.len()
    }

    pub fn take_subscription(&mut self) -> Option<EventSubscription<TrayEvent>> {
        self.subscriptions.pop()
    }

    pub fn create_status_item(&self, spec: StatusItemSpec) -> ServiceResult<StatusItemLease> {
        let context = self.context()?;
        self.service
            .create_surface_status_item(spec, self.surface, self.generation, &context)
    }

    pub fn create_application_status_item(
        &self,
        spec: StatusItemSpec,
    ) -> ServiceResult<StatusItemLease> {
        let context = self.context()?;
        self.service.create_application_status_item(spec, &context)
    }

    pub fn notify_visible(&self) {
        self.service
            .notify_surface_visible(self.surface, self.generation);
    }

    pub fn notify_hidden(&self) {
        self.service
            .attach_surface(self.surface, self.generation, false);
    }

    pub fn sync_lifecycle(&self, lifecycle: &crate::LifecycleState) {
        self.service.notify_lifecycle(lifecycle);
    }

    pub fn sync_preferences(&self, preferences: &SystemPreferences, scale_factor_milli: u32) {
        sync_preferences(&self.service, preferences, scale_factor_milli);
    }
}

impl Drop for TraySurfaceScope {
    fn drop(&mut self) {
        self.subscriptions.clear();
        if self.ui.is_some() {
            self.service
                .dispose_surface_generation(self.surface, self.generation);
        }
        // A stale scope must not invalidate a newer generation that shares the
        // same `UiOwner` (native attachments advance generation in place).
        if self.ui.as_ref().is_some_and(UiContext::is_current) {
            self.owner.dispose();
        }
        drop(self.entity_scope.take());
    }
}

impl std::fmt::Debug for TraySurfaceScope {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("TraySurfaceScope")
            .field("surface", &self.surface)
            .field("generation", &self.generation)
            .field("subscriptions", &self.subscriptions.len())
            .finish()
    }
}

/// Bounded drain of a [`TrayEntityModel`] event stream.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TrayEventDrain {
    /// True when the bounded stream reported `Event::ResyncRequired`, or the
    /// model's local history had to discard an event.
    pub resync_required: bool,
    /// Events observed by this drain in delivery order.
    pub events: Vec<TrayEvent>,
}

/// GPUI entity model that owns one surface tray scope and its status items.
///
/// Create it with `cx.new(|cx| TrayEntityModel::new(owner, service, cx))`.
/// It retains a bounded event subscription, keeps at most
/// [`TrayEntityModel::MAX_RETAINED_EVENTS`] events, and removes all owned
/// status items plus its surface on drop/entity release.
pub struct TrayEntityModel {
    leases: Vec<StatusItemLease>,
    scope: TraySurfaceScope,
    subscription: EventSubscription<TrayEvent>,
    events: std::collections::VecDeque<TrayEvent>,
    resync_required: bool,
}

impl TrayEntityModel {
    /// Maximum number of recent events retained by the model.
    pub const MAX_RETAINED_EVENTS: usize = 64;

    /// Build the model and tie its scope to the entity release callback.
    pub fn new(owner: UiOwner, service: TrayService, cx: &mut Context<Self>) -> Self {
        let scope = TraySurfaceScope::for_entity(owner, service, cx);
        let subscription = scope.service().subscribe();
        Self {
            leases: Vec::new(),
            scope,
            subscription,
            events: std::collections::VecDeque::new(),
            resync_required: false,
        }
    }

    pub fn service(&self) -> &TrayService {
        self.scope.service()
    }

    pub fn surface(&self) -> SurfaceId {
        self.scope.surface()
    }

    pub fn generation(&self) -> u64 {
        self.scope.generation()
    }

    pub fn active_lease_count(&self) -> usize {
        self.leases.len()
    }

    pub fn active_leases(&self) -> Vec<StatusItemSnapshot> {
        self.leases
            .iter()
            .filter_map(StatusItemLease::snapshot)
            .collect()
    }

    pub fn status_item_snapshot(&self, id: crate::StatusItemId) -> Option<StatusItemSnapshot> {
        self.leases
            .iter()
            .find(|lease| lease.id() == id)
            .and_then(StatusItemLease::snapshot)
    }

    pub fn residency(&self) -> bool {
        self.service().residing_in_tray()
    }

    /// Create a surface-scoped item and retain its lease.
    pub fn create_status_item(
        &mut self,
        spec: StatusItemSpec,
        cx: &mut Context<Self>,
    ) -> Result<crate::StatusItemId, SystemError> {
        match self.scope.create_status_item(spec) {
            Ok(Completion::Completed(lease)) => {
                let id = lease.id();
                self.leases.push(lease);
                cx.notify();
                Ok(id)
            }
            Ok(Completion::Cancelled(reason)) => Err(SystemError::new(
                ErrorCode::Unavailable,
                cancelled_message(reason),
            )),
            Err(error) => Err(error),
        }
    }

    /// Create an application-scoped item and retain its lease.
    pub fn create_application_status_item(
        &mut self,
        spec: StatusItemSpec,
        cx: &mut Context<Self>,
    ) -> Result<crate::StatusItemId, SystemError> {
        let context = self.scope.context()?;
        match self
            .service()
            .create_application_status_item(spec, &context)
        {
            Ok(Completion::Completed(lease)) => {
                let id = lease.id();
                self.leases.push(lease);
                cx.notify();
                Ok(id)
            }
            Ok(Completion::Cancelled(reason)) => Err(SystemError::new(
                ErrorCode::Unavailable,
                cancelled_message(reason),
            )),
            Err(error) => Err(error),
        }
    }

    /// Update an owned item, returning its new revision.
    pub fn update_status_item(
        &mut self,
        id: crate::StatusItemId,
        spec: StatusItemSpec,
        cx: &mut Context<Self>,
    ) -> Result<u64, SystemError> {
        let context = self.scope.context()?;
        let revision = match self.service().update_status_item(id, spec, &context)? {
            Completion::Completed(snapshot) => snapshot.revision,
            Completion::Cancelled(reason) => {
                return Err(SystemError::new(
                    ErrorCode::Unavailable,
                    cancelled_message(reason),
                ));
            }
        };
        cx.notify();
        Ok(revision)
    }

    /// Remove one owned item. Returns true exactly once.
    pub fn remove_status_item(&mut self, id: crate::StatusItemId, cx: &mut Context<Self>) -> bool {
        let Some(position) = self.leases.iter().position(|lease| lease.id() == id) else {
            return false;
        };
        let lease = self.leases.remove(position);
        let removed = lease.remove();
        drop(lease);
        cx.notify();
        removed
    }

    /// Drain all currently available events without blocking.
    ///
    /// A provider overflow (`Event::ResyncRequired`) is sticky until
    /// [`Self::clear_resync_required`] is called. A local history overflow also
    /// sets it because older retained events were discarded.
    pub fn drain_events(&mut self) -> TrayEventDrain {
        let mut events = Vec::new();
        while let Ok(mut next) = self.subscription.try_next() {
            let waker = std::task::Waker::noop();
            let mut context = std::task::Context::from_waker(waker);
            match std::pin::Pin::new(&mut next).poll(&mut context) {
                std::task::Poll::Ready(Some(envelope)) => match envelope.payload {
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
                std::task::Poll::Ready(None) | std::task::Poll::Pending => break,
            }
        }
        TrayEventDrain {
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

    pub fn retained_events(&self) -> impl Iterator<Item = &TrayEvent> {
        self.events.iter()
    }

    pub fn take_retained_events(&mut self) -> Vec<TrayEvent> {
        self.events.drain(..).collect()
    }

    pub fn sync_preferences(
        &mut self,
        preferences: &SystemPreferences,
        scale_factor_milli: u32,
        cx: &mut Context<Self>,
    ) {
        self.scope.sync_preferences(preferences, scale_factor_milli);
        cx.notify();
    }

    pub fn sync_lifecycle(&mut self, lifecycle: &crate::LifecycleState, cx: &mut Context<Self>) {
        self.scope.sync_lifecycle(lifecycle);
        cx.notify();
    }

    pub fn refresh_residency(&mut self, cx: &mut Context<Self>) {
        self.service().refresh_residency();
        cx.notify();
    }
}

fn cancelled_message(reason: crate::CancelReason) -> &'static str {
    match reason {
        crate::CancelReason::User => "Tray operation was cancelled by the user",
        crate::CancelReason::Caller => "Tray operation was cancelled by the caller",
        crate::CancelReason::OwnerDisposed => "Tray operation owner was disposed",
        crate::CancelReason::ProcessPolicy => "Tray operation was cancelled by process policy",
    }
}

impl Drop for TrayEntityModel {
    fn drop(&mut self) {
        // Removing leases first guarantees the surface disposal path cannot be
        // the only cleanup for a still-hosted item.
        for lease in self.leases.drain(..) {
            drop(lease);
        }
    }
}

impl std::fmt::Debug for TrayEntityModel {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("TrayEntityModel")
            .field("surface", &self.surface())
            .field("generation", &self.generation())
            .field("leases", &self.leases.len())
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
        Appearance, ColorSchemeMatch, IconPayload, LifecycleState, PreferenceValue, ScaleBucket,
        StatusIconSet, StatusIconVariant, StatusItemKey,
    };
    use gpui::{AppContext, TestAppContext};
    use gpui_lifecycle::{MemoryPressure, SessionLockState, SurfaceSnapshot, SurfaceVisibility};
    use std::rc::Rc;

    fn spec() -> StatusItemSpec {
        let icons = StatusIconSet::new([
            StatusIconVariant::new(
                IconPayload::asset_key("tray/light.png").expect("asset"),
                ColorSchemeMatch::Light,
                ScaleBucket::X1,
            ),
            StatusIconVariant::new(
                IconPayload::asset_key("tray/dark.png").expect("asset"),
                ColorSchemeMatch::Dark,
                ScaleBucket::X2,
            ),
        ])
        .expect("icons");
        StatusItemSpec::new(StatusItemKey::new("app.status").expect("key"), icons)
            .with_title("Status")
            .with_accessibility_label("Application status")
    }

    #[gpui::test]
    fn entity_model_owns_scope_drains_events_syncs_and_cleans_up(cx: &mut TestAppContext) {
        let (entity, owner, fake, service) = cx.update(|cx| {
            let fake = crate::FakeTrayProvider::new();
            let service = init_tray(cx, Rc::new(fake.clone()));
            let owner = UiOwner::new(SurfaceId(61));
            let entity = cx.new(|cx| TrayEntityModel::new(owner.clone(), service.clone(), cx));
            (entity, owner, fake, service)
        });
        cx.update(|cx| {
            entity.update(cx, |model, cx| {
                assert_eq!(model.surface(), SurfaceId(61));
                let id = model
                    .create_status_item(spec(), cx)
                    .expect("create status item");
                assert_eq!(model.active_lease_count(), 1);
                assert_eq!(model.active_leases().len(), 1);
                assert!(model.status_item_snapshot(id).is_some());
                assert!(!model.residency());

                let preferences = SystemPreferences {
                    appearance: PreferenceValue::Known(Appearance::Dark),
                    high_contrast: PreferenceValue::Known(false),
                    ..SystemPreferences::default()
                };
                model.sync_preferences(&preferences, 2000, cx);
                assert_eq!(model.service().scale_factor_milli(), 2000);
                assert_eq!(
                    fake.hosted_items()[0].projection.icon.payload,
                    IconPayload::AssetKey("tray/dark.png".to_owned())
                );

                let lifecycle = LifecycleState::from_parts(
                    crate::AppPhase::ForegroundActive,
                    [(
                        SurfaceId(61),
                        SurfaceSnapshot {
                            visibility: SurfaceVisibility::Visible { focused: true },
                            generation: 1,
                        },
                    )],
                    SessionLockState::Unlocked,
                    MemoryPressure::Normal,
                );
                model.sync_lifecycle(&lifecycle, cx);
                let drain = model.drain_events();
                assert!(!drain.events.is_empty());
                assert!(!model.resync_required());

                assert!(model.remove_status_item(id, cx));
                assert_eq!(model.active_lease_count(), 0);
            });
        });
        drop(entity);
        cx.update(|_| {});
        assert!(owner.context(None).is_err());
        assert!(fake.active_item_tokens().is_empty());
        assert_eq!(fake.remove_call_count(), 1);
        assert_eq!(service.active_item_count(), 0);
    }

    #[gpui::test]
    fn entity_model_drop_removes_unremoved_leases(cx: &mut TestAppContext) {
        let (entity, fake) = cx.update(|cx| {
            let fake = crate::FakeTrayProvider::new();
            let service = init_tray(cx, Rc::new(fake.clone()));
            let owner = UiOwner::new(SurfaceId(62));
            let entity = cx.new(|cx| TrayEntityModel::new(owner, service, cx));
            (entity, fake)
        });
        cx.update(|cx| {
            entity.update(cx, |model, cx| {
                model
                    .create_status_item(spec(), cx)
                    .expect("create status item");
                assert_eq!(model.active_lease_count(), 1);
            });
        });
        drop(entity);
        cx.update(|_| {});
        assert_eq!(fake.remove_call_count(), 1);
        assert!(fake.active_item_tokens().is_empty());
    }

    #[gpui::test]
    fn install_service_and_lookup_are_global(cx: &mut TestAppContext) {
        cx.update(|cx| {
            let fake = crate::FakeTrayProvider::new();
            let service = init_tray(cx, Rc::new(fake));
            let resolved = tray_service(cx).expect("global service");
            assert!(service.shares_state_with(&resolved));
            install_tray_service(service.clone(), cx);
            assert!(
                tray_service(cx)
                    .expect("reinstalled")
                    .shares_state_with(&service)
            );
        });
    }
}
