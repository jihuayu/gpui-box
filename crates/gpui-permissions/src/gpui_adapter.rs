//! Optional GPUI integration for [`PermissionService`].
//!
//! All functions must run on the owning GPUI UI thread. The adapter never
//! installs a native provider implicitly and never runs a prompt on a worker
//! thread.

use crate::{EventSubscription, PermissionChange, PermissionProvider, PermissionService};
use gpui::{App, Context, Task};
use gpui_system::{
    OperationContext, Request, ServiceResult, SystemError, SystemServices, UiOwner, gpui_adapter,
};
use std::rc::Rc;

/// Register one explicit permission service. Call before starting operations.
pub fn install_permission_service(
    services: &mut SystemServices,
    provider: Rc<dyn PermissionProvider>,
) -> Result<(), SystemError> {
    services.register(PermissionService::new(provider))
}

/// Access the installed service from GPUI's typed `SystemServices` global.
pub fn permission_service(cx: &App) -> Result<Rc<PermissionService>, SystemError> {
    cx.global::<SystemServices>().service::<PermissionService>()
}

/// Run a permission request on GPUI's local foreground executor with the
/// runtime deadline timer installed.
pub fn run_permission_request<T: 'static>(request: Request<T>, cx: &App) -> Task<ServiceResult<T>> {
    gpui_adapter::run_request(request, cx)
}

/// Convenience wrapper for a contextual request built from the service.
pub fn request_permission(
    service: &PermissionService,
    prompt: crate::PermissionRequest,
    cx: OperationContext,
    app: &App,
) -> Task<ServiceResult<crate::PermissionOutcome>> {
    run_permission_request(service.request_permission(prompt, cx), app)
}

/// Entity-owned scope which retains both the GPUI release guard and the
/// single-consumer permission observation stream. Dropping it drops the stream
/// first, then invalidates the UI owner through [`gpui_system::gpui_adapter::EntityScope`].
pub struct PermissionEntityScope {
    subscriptions: Vec<EventSubscription<PermissionChange>>,
    scope: gpui_system::gpui_adapter::EntityScope,
}

impl PermissionEntityScope {
    pub fn new<T: 'static>(owner: UiOwner, cx: &Context<T>) -> Self {
        Self {
            subscriptions: Vec::new(),
            scope: gpui_system::gpui_adapter::EntityScope::new(owner, cx),
        }
    }

    pub fn owner(&self) -> &UiOwner {
        self.scope.owner()
    }

    /// Retain a permission stream for the lifetime of the entity scope.
    pub fn retain(&mut self, subscription: EventSubscription<PermissionChange>) {
        self.subscriptions.push(subscription);
    }

    pub fn subscribe(&mut self, service: &PermissionService) {
        self.retain(service.subscribe());
    }

    pub fn subscription_count(&self) -> usize {
        self.subscriptions.len()
    }
}
