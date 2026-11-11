//! Optional GPUI integration for [`MenuCommandService`].
//!
//! Installation and lookup run on the owning GPUI UI thread. Background command
//! handlers execute on the service's own bounded pool; the adapter only supplies
//! GPUI timers for request deadlines.

use crate::{
    CommandScope, ContributionScope, ErrorCode, MenuCommandService, MenuProvider,
    RegistrationLease, Request, ServiceResult, SurfaceId, SystemError, SystemServices, UiOwner,
};
use gpui::{App, Context, Global, Subscription, Task};
use std::rc::Rc;

struct MenusGlobal(MenuCommandService);

impl Global for MenusGlobal {}

/// Register one explicit provider in `SystemServices`.
pub fn install_menu_service(
    services: &mut SystemServices,
    service: MenuCommandService,
) -> Result<(), SystemError> {
    services.register(service)
}

/// Install a dedicated GPUI global menu service.
pub fn init_menu_service(cx: &mut App, service: MenuCommandService) -> MenuCommandService {
    cx.set_global(MenusGlobal(service.clone()));
    service
}

/// Create and install a service from an explicit provider.
pub fn init_menus(cx: &mut App, provider: Rc<dyn MenuProvider>) -> MenuCommandService {
    init_menu_service(cx, MenuCommandService::new(provider))
}

/// Resolve the service from the dedicated global or the typed registry.
pub fn menus_service(cx: &App) -> Result<MenuCommandService, SystemError> {
    if let Some(global) = cx.try_global::<MenusGlobal>() {
        return Ok(global.0.clone());
    }
    let missing = SystemError::new(
        ErrorCode::Unavailable,
        "MenuCommandService is not installed",
    );
    cx.try_global::<SystemServices>()
        .ok_or(missing)?
        .service::<MenuCommandService>()
        .map(|service| (*service).clone())
}

/// Run a command request on GPUI's local foreground executor with the runtime
/// deadline timer installed.
pub fn run_command_request<T: 'static>(request: Request<T>, cx: &App) -> Task<ServiceResult<T>> {
    gpui_system::gpui_adapter::run_request(request, cx)
}

/// Ergonomic access to the GPUI menu global.
pub trait MenusAppExt {
    fn init_menus(&mut self, provider: Rc<dyn MenuProvider>) -> MenuCommandService;

    fn init_menu_service(&mut self, service: MenuCommandService) -> MenuCommandService;

    fn menus_service(&self) -> Result<MenuCommandService, SystemError>;
}

impl MenusAppExt for App {
    fn init_menus(&mut self, provider: Rc<dyn MenuProvider>) -> MenuCommandService {
        crate::gpui_adapter::init_menus(self, provider)
    }

    fn init_menu_service(&mut self, service: MenuCommandService) -> MenuCommandService {
        crate::gpui_adapter::init_menu_service(self, service)
    }

    fn menus_service(&self) -> Result<MenuCommandService, SystemError> {
        crate::gpui_adapter::menus_service(self)
    }
}

/// Entity- or host-owned contribution scope. Dropping it revokes retained
/// leases, disposes the contribution scope (and therefore every registration),
/// disposes the logical surface in the service and disposes the `UiOwner`.
pub struct MenuContributionScope {
    owner: UiOwner,
    service: MenuCommandService,
    surface: SurfaceId,
    scope: ContributionScope,
    leases: Vec<RegistrationLease>,
    subscription: Option<Subscription>,
}

impl MenuContributionScope {
    pub fn new(owner: UiOwner, service: MenuCommandService) -> Result<Self, SystemError> {
        let surface = owner.context(None)?.surface;
        let scope = service.create_scope(CommandScope::UiOwner(owner.clone()))?;
        Ok(Self {
            owner,
            service,
            surface,
            scope,
            leases: Vec::new(),
            subscription: None,
        })
    }

    /// Tie the scope to a GPUI entity's release callback in addition to `Drop`.
    pub fn for_entity<T: 'static>(
        owner: UiOwner,
        service: MenuCommandService,
        cx: &Context<T>,
    ) -> Result<Self, SystemError> {
        let mut scope = Self::new(owner, service)?;
        let released_owner = scope.owner.clone();
        let released_service = scope.service.clone();
        let released_scope = scope.scope.clone();
        let surface = scope.surface;
        scope.subscription = Some(cx.on_release(move |_, _| {
            released_service.dispose_surface(surface);
            released_owner.dispose();
            released_scope.revoke();
        }));
        Ok(scope)
    }

    pub const fn owner(&self) -> &UiOwner {
        &self.owner
    }

    pub const fn service(&self) -> &MenuCommandService {
        &self.service
    }

    pub const fn surface(&self) -> SurfaceId {
        self.surface
    }

    pub const fn scope(&self) -> &ContributionScope {
        &self.scope
    }

    /// Retain an additional registration lease for the scope's lifetime.
    pub fn retain_lease(&mut self, lease: RegistrationLease) {
        self.leases.push(lease);
    }

    pub fn retain_leases(&mut self, leases: impl IntoIterator<Item = RegistrationLease>) {
        self.leases.extend(leases);
    }

    pub fn lease_count(&self) -> usize {
        self.leases.len()
    }

    pub fn is_active(&self) -> bool {
        self.scope.is_active()
    }
}

impl Drop for MenuContributionScope {
    fn drop(&mut self) {
        for lease in self.leases.drain(..) {
            let _ = lease.revoke();
        }
        let _ = self.scope.revoke();
        self.service.dispose_surface(self.surface);
        self.owner.dispose();
    }
}

#[cfg(all(
    test,
    feature = "gpui",
    any(target_os = "windows", target_os = "macos", target_os = "linux")
))]
mod tests {
    use super::*;
    use crate::{CommandDescriptor, CommandId, FakeMenuProvider};
    use gpui::TestAppContext;

    #[gpui::test]
    fn installs_and_resolves_global_and_service_registry(cx: &mut TestAppContext) {
        cx.update(|cx| {
            let provider = Rc::new(FakeMenuProvider::new());
            let service = init_menus(cx, provider);
            let resolved = menus_service(cx).expect("global service");
            assert!(service.shares_state_with(&resolved));
            assert_eq!(resolved.capabilities().len(), 5);

            let mut services = SystemServices::new();
            install_menu_service(&mut services, service.clone()).expect("registration");
            gpui_system::gpui_adapter::install_system_services(services, cx);
            let from_registry = menus_service(cx).expect("registry service");
            assert!(service.shares_state_with(&from_registry));
            let _task = run_command_request(
                service.invoke_command(
                    &CommandId::new("missing.command").expect("id"),
                    crate::CommandPayload::Unit,
                    crate::InvocationContext::default(),
                ),
                cx,
            );
        });
    }

    #[gpui::test]
    fn contribution_scope_drop_revokes_and_disposes(cx: &mut TestAppContext) {
        cx.update(|cx| {
            let provider = Rc::new(FakeMenuProvider::new());
            let service = init_menus(cx, provider);
            let owner = UiOwner::new(SurfaceId(41));
            let mut scope =
                MenuContributionScope::new(owner.clone(), service.clone()).expect("scope");
            let descriptor =
                CommandDescriptor::ui(CommandId::new("window.close").expect("id"), "Close Window")
                    .expect("descriptor");
            let registration = service
                .register_ui_in(scope.scope(), descriptor, |_: (), _| Ok(()))
                .expect("registration");
            scope.retain_lease(registration.into_parts().1);
            assert!(service.is_registered(&CommandId::new("window.close").expect("id")));
            drop(scope);
            assert!(!service.is_registered(&CommandId::new("window.close").expect("id")));
            assert!(service.surface_generation(SurfaceId(41)).is_none());
            assert!(owner.context(None).is_err());
        });
    }
}
