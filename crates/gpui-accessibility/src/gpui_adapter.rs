//! Optional GPUI adapter. All functions must run on the owning GPUI UI thread.

use crate::{AccessibilityProvider, AccessibilityService};
use gpui::{App, Global};
use gpui_system::{
    OperationContext, Request, ServiceResult, SurfaceId, SystemError, SystemServices, UiOwner,
    gpui_adapter,
};
use std::rc::Rc;

struct AccessibilityGlobal(Rc<AccessibilityService>);

impl Global for AccessibilityGlobal {}

/// Register one explicit provider in `SystemServices`. Call before starting
/// operations; this crates no native provider or platform bridge implicitly.
pub fn install_accessibility_service(
    services: &mut SystemServices,
    provider: Rc<dyn AccessibilityProvider>,
) -> Result<(), SystemError> {
    services.register(AccessibilityService::new(provider))
}

/// Install a dedicated GPUI global service for entity-local usage. This does
/// not require or replace `SystemServices`, so the caller may also register the
/// same service there when a screen-scoped registry is preferred.
pub fn init_accessibility(
    cx: &mut App,
    provider: Rc<dyn AccessibilityProvider>,
) -> Rc<AccessibilityService> {
    let service = Rc::new(AccessibilityService::new(provider));
    cx.set_global(AccessibilityGlobal(service.clone()));
    service
}

/// Resolve the installed service from either the dedicated global or the
/// typed `SystemServices` registry.
pub fn accessibility_service(cx: &App) -> Result<Rc<AccessibilityService>, SystemError> {
    if let Some(global) = cx.try_global::<AccessibilityGlobal>() {
        return Ok(global.0.clone());
    }
    let missing = SystemError::new(
        gpui_system::ErrorCode::Unavailable,
        "SystemServices is not installed",
    );
    cx.try_global::<SystemServices>()
        .ok_or(missing)?
        .service::<AccessibilityService>()
}

/// Run an accessibility request on GPUI's local foreground executor with the
/// runtime deadline timer installed.
pub fn run_accessibility_request<T: 'static>(
    request: Request<T>,
    cx: &App,
) -> gpui::Task<ServiceResult<T>> {
    gpui_adapter::run_request(request, cx)
}

/// Entity-owned surface scope. Dropping it disposes the semantic surface and
/// pending live announcements before invalidating the UI owner.
pub struct AccessibilitySurfaceScope {
    owner: UiOwner,
    service: AccessibilityService,
    surface: SurfaceId,
}

impl AccessibilitySurfaceScope {
    pub fn new(owner: UiOwner, service: AccessibilityService) -> Self {
        let surface = owner
            .context(None)
            .map_or(SurfaceId(0), |context| context.surface);
        Self {
            owner,
            service,
            surface,
        }
    }

    pub fn owner(&self) -> &UiOwner {
        &self.owner
    }

    pub fn service(&self) -> &AccessibilityService {
        &self.service
    }

    pub fn surface(&self) -> SurfaceId {
        self.surface
    }

    pub fn context(&self) -> Result<OperationContext, SystemError> {
        Ok(OperationContext {
            ui: Some(self.owner.context(None)?),
            ..OperationContext::default()
        })
    }
}

impl Drop for AccessibilitySurfaceScope {
    fn drop(&mut self) {
        self.service.dispose_surface(self.surface);
        self.owner.dispose();
    }
}

/// Ergonomic access to the GPUI accessibility global.
pub trait AccessibilityAppExt {
    fn init_accessibility(
        &mut self,
        provider: Rc<dyn AccessibilityProvider>,
    ) -> Rc<AccessibilityService>;

    fn accessibility_service(&self) -> Result<Rc<AccessibilityService>, SystemError>;
}

impl AccessibilityAppExt for App {
    fn init_accessibility(
        &mut self,
        provider: Rc<dyn AccessibilityProvider>,
    ) -> Rc<AccessibilityService> {
        crate::gpui_adapter::init_accessibility(self, provider)
    }

    fn accessibility_service(&self) -> Result<Rc<AccessibilityService>, SystemError> {
        crate::gpui_adapter::accessibility_service(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{AccessibilityPlatform, FakeAccessibilityProvider};
    use gpui::TestAppContext;

    #[gpui::test]
    fn global_adapter_installs_and_resolves_service(cx: &mut TestAppContext) {
        let provider = Rc::new(FakeAccessibilityProvider::new(
            AccessibilityPlatform::Windows,
        ));
        let service = cx.update(|cx| init_accessibility(cx, provider));
        let resolved = cx.update(|cx| accessibility_service(cx).expect("installed service"));
        assert!(Rc::ptr_eq(&service, &resolved));
        assert_eq!(resolved.platform(), AccessibilityPlatform::Windows);
    }

    #[gpui::test]
    fn system_services_registration_is_typed(cx: &mut TestAppContext) {
        let provider = Rc::new(FakeAccessibilityProvider::new(AccessibilityPlatform::Linux));
        let service = cx.update(|cx| {
            let mut services = SystemServices::new();
            install_accessibility_service(&mut services, provider).expect("registration");
            gpui_system::gpui_adapter::install_system_services(services, cx);
            accessibility_service(cx).expect("registered service")
        });
        assert_eq!(service.platform(), AccessibilityPlatform::Linux);
        assert_eq!(
            service
                .capability(crate::AccessibilityOperation::ObserveTree)
                .operation,
            crate::AccessibilityOperation::ObserveTree
        );
    }
}
