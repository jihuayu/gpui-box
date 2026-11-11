//! Optional GPUI adapter over the distribution's coordinated GPUI identity.
//!
//! Installation and lookup must run on the owning GPUI UI thread. The core
//! [`PowerService`] remains `Send + Sync`; this adapter only adds UI ownership,
//! `App` globals and surface-scoped cleanup.

use crate::{
    EventSubscription, OperationContext, PowerEvent, PowerProvider, PowerService, ReleasePolicy,
    ServiceResult, SurfaceId, SystemError, SystemServices, UiContext, UiOwner, WakeInhibitionKind,
    WakeLeaseRequest, WakeLeaseScope, WakePreventionLease, WakeReason,
};
use gpui::{App, Context};
use gpui_system::{ErrorCode, gpui_adapter::EntityScope};

impl gpui::Global for PowerService {}

/// Install the service into GPUI's `App` global.
pub fn install_power_service(service: PowerService, cx: &mut App) {
    cx.set_global(service);
}

/// Register a service in the typed `SystemServices` registry.
pub fn install_power_in_services(
    services: &mut SystemServices,
    service: PowerService,
) -> Result<(), SystemError> {
    services.register(service)
}

/// Build a service, install it as the `App` global and register it in
/// `SystemServices` for code that resolves services through the registry.
pub fn init_power(cx: &mut App, provider: std::sync::Arc<dyn PowerProvider>) -> PowerService {
    let service = PowerService::new(provider);
    cx.set_global(service.clone());
    cx.default_global::<SystemServices>()
        .override_provider(service.clone());
    service
}

/// Resolve the service from the `App` global first, then from `SystemServices`.
pub fn power_service(cx: &App) -> Result<PowerService, SystemError> {
    if let Some(service) = cx.try_global::<PowerService>() {
        return Ok(service.clone());
    }
    let services = cx.try_global::<SystemServices>().ok_or_else(|| {
        SystemError::new(
            ErrorCode::Unavailable,
            "PowerService and SystemServices are not installed",
        )
    })?;
    services
        .service::<PowerService>()
        .map(|service| (*service).clone())
}

/// Convenience methods for GPUI applications.
pub trait PowerAppExt {
    fn install_power_service(&mut self, service: PowerService);
    fn init_power(&mut self, provider: std::sync::Arc<dyn PowerProvider>) -> PowerService;
    fn power_service(&self) -> Result<PowerService, SystemError>;
}

impl PowerAppExt for App {
    fn install_power_service(&mut self, service: PowerService) {
        install_power_service(service, self);
    }

    fn init_power(&mut self, provider: std::sync::Arc<dyn PowerProvider>) -> PowerService {
        init_power(self, provider)
    }

    fn power_service(&self) -> Result<PowerService, SystemError> {
        power_service(self)
    }
}

/// Entity-/surface-owned power scope.
///
/// The scope retains observation subscriptions and the surface attachment. On
/// drop it invalidates subscriptions, disposes the surface (revoking every
/// surface-scoped lease with [`crate::WakeRevocationReason::SurfaceDisposed`])
/// and disposes the `UiOwner`. Dropping it never leaves a native token active.
pub struct PowerSurfaceScope {
    owner: UiOwner,
    service: PowerService,
    surface: SurfaceId,
    generation: u64,
    ui: Option<UiContext>,
    subscriptions: Vec<EventSubscription<PowerEvent>>,
    entity_scope: Option<EntityScope>,
}

impl PowerSurfaceScope {
    /// Create a scope for the owner's current attachment generation.
    ///
    /// A disposed owner produces an inert scope (`SurfaceId(0)`, generation 0)
    /// whose [`Self::context`] reports `ContextExpired`; it never panics in a
    /// destructor path.
    pub fn new(owner: UiOwner, service: PowerService) -> Self {
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

    /// Fallible form of [`Self::new`] for callers that need the typed error.
    pub fn try_new(owner: UiOwner, service: PowerService) -> Result<Self, SystemError> {
        let ui = owner.context(None)?;
        Ok(Self::with_ui(owner, service, ui))
    }

    fn with_ui(owner: UiOwner, service: PowerService, ui: UiContext) -> Self {
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

    /// Create a scope tied to a GPUI entity lifecycle.
    pub fn for_entity<T: 'static>(owner: UiOwner, service: PowerService, cx: &Context<T>) -> Self {
        let entity_scope = EntityScope::new(owner.clone(), cx);
        let mut scope = Self::new(owner, service);
        scope.entity_scope = Some(entity_scope);
        scope
    }

    pub fn owner(&self) -> &UiOwner {
        &self.owner
    }

    pub fn service(&self) -> &PowerService {
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
                "Power surface scope has no live UI attachment",
            )
        })?;
        if !ui.is_current() {
            return Err(SystemError::new(
                ErrorCode::ContextExpired,
                "Power surface scope UI attachment is stale",
            ));
        }
        Ok(OperationContext {
            ui: Some(ui),
            ..OperationContext::default()
        })
    }

    pub fn subscribe(&mut self) -> &EventSubscription<PowerEvent> {
        self.subscriptions.push(self.service.subscribe());
        self.subscriptions
            .last()
            .expect("a subscription was just retained")
    }

    pub fn subscription_count(&self) -> usize {
        self.subscriptions.len()
    }

    /// Remove and return a retained subscription for an owned reader loop.
    pub fn take_subscription(&mut self) -> Option<EventSubscription<PowerEvent>> {
        self.subscriptions.pop()
    }

    pub fn acquire_display_wake_lease(
        &self,
        reason: WakeReason,
    ) -> ServiceResult<WakePreventionLease> {
        self.acquire(WakeInhibitionKind::DisplayAndSystem, reason)
    }

    pub fn acquire_system_wake_lease(
        &self,
        reason: WakeReason,
    ) -> ServiceResult<WakePreventionLease> {
        self.acquire(WakeInhibitionKind::SystemIdleSleep, reason)
    }

    fn acquire(
        &self,
        kind: WakeInhibitionKind,
        reason: WakeReason,
    ) -> ServiceResult<WakePreventionLease> {
        let policy = ReleasePolicy::for_kind(kind);
        let scope = WakeLeaseScope::surface(self.surface, self.generation, policy);
        let context = self.context()?;
        self.service.acquire_wake_lease(
            WakeLeaseRequest::new(kind, reason).with_scope(scope),
            &context,
        )
    }
}

impl Drop for PowerSurfaceScope {
    fn drop(&mut self) {
        self.subscriptions.clear();
        if self.ui.is_some() {
            self.service.dispose_surface(self.surface);
        }
        self.owner.dispose();
        drop(self.entity_scope.take());
    }
}

impl std::fmt::Debug for PowerSurfaceScope {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("PowerSurfaceScope")
            .field("surface", &self.surface)
            .field("generation", &self.generation)
            .field("subscriptions", &self.subscriptions.len())
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
    use crate::{Completion, FakePowerProvider};
    use gpui::{AppContext, TestAppContext};
    use std::sync::Arc;

    struct ScopeEntity {
        scope: PowerSurfaceScope,
    }

    #[gpui::test]
    fn installs_app_global_and_system_services(cx: &mut TestAppContext) {
        cx.update(|cx| {
            gpui_system::gpui_adapter::install_system_services(SystemServices::new(), cx);
            let provider = Arc::new(FakePowerProvider::new());
            let service = init_power(cx, provider);
            let resolved = power_service(cx).expect("app global");
            assert!(service.shares_state_with(&resolved));
            assert!(cx.power_service().is_ok());
            let registered = cx
                .try_global::<SystemServices>()
                .expect("services")
                .service::<PowerService>()
                .expect("registry");
            assert!(service.shares_state_with(&registered));

            let mut services = SystemServices::new();
            install_power_in_services(&mut services, service.clone()).expect("registration");
            assert!(
                service.shares_state_with(&services.service::<PowerService>().expect("registered"))
            );
        });
    }

    #[gpui::test]
    fn missing_service_is_typed(cx: &mut TestAppContext) {
        cx.update(|cx| {
            assert_eq!(power_service(cx).unwrap_err().code, ErrorCode::Unavailable);
        });
    }

    #[gpui::test]
    fn surface_scope_revokes_leases_and_disposes_owner_on_drop(cx: &mut TestAppContext) {
        let provider = Arc::new(FakePowerProvider::new());
        let service = cx.update(|cx| init_power(cx, provider.clone()));
        let owner = UiOwner::new(SurfaceId(9));
        let probe = owner.clone();
        let entity = cx.update(|cx| {
            cx.new(|cx| ScopeEntity {
                scope: PowerSurfaceScope::for_entity(owner, service.clone(), cx),
            })
        });
        let lease = cx.update(|cx| {
            entity.update(cx, |entity, _cx| {
                entity.scope.subscribe();
                assert_eq!(entity.scope.subscription_count(), 1);
                let reason = WakeReason::new("visible entity work").expect("reason");
                match entity
                    .scope
                    .acquire_display_wake_lease(reason)
                    .expect("acquire")
                {
                    Completion::Completed(lease) => lease,
                    Completion::Cancelled(reason) => panic!("cancelled: {reason:?}"),
                }
            })
        });
        assert!(lease.is_effective());
        assert_eq!(provider.active_native_tokens().len(), 1);
        assert_eq!(service.active_lease_count(), 1);
        drop(entity);
        cx.update(|_| {});
        assert!(probe.context(None).is_err());
        assert!(!lease.is_effective());
        assert_eq!(
            lease.revocation_reason(),
            Some(crate::WakeRevocationReason::SurfaceDisposed)
        );
        // A revoked lease cannot release its native token a second time.
        assert!(!lease.release());
        drop(lease);
        assert_eq!(service.active_lease_count(), 0);
        assert!(provider.active_native_tokens().is_empty());
        assert_eq!(provider.release_call_count(), 1);
    }
}
