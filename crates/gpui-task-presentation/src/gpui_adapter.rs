//! Optional GPUI adapter over the distribution's coordinated GPUI identity.
//!
//! Installation, lookup and entity helpers must run on the owning GPUI UI
//! thread. The core service is already UI-local; this adapter adds `App` globals,
//! `SystemServices` registration and entity/surface-owned cleanup.

use crate::{
    EventSubscription, OperationContext, SurfaceId, SystemError, SystemServices,
    TaskPresentationEvent, TaskPresentationProvider, TaskPresentationService, UiContext, UiOwner,
};
use gpui::{App, Context};
use gpui_lifecycle::LifecycleState;
use gpui_system::{ErrorCode, gpui_adapter::EntityScope};
use std::rc::Rc;

impl gpui::Global for TaskPresentationService {}

/// Install the service into GPUI's `App` global.
pub fn install_task_presentation_service(service: TaskPresentationService, cx: &mut App) {
    cx.set_global(service);
}

/// Register a service in the typed `SystemServices` registry.
pub fn install_task_presentation_in_services(
    services: &mut SystemServices,
    service: TaskPresentationService,
) -> Result<(), SystemError> {
    services.register(service)
}

/// Build a service, install it as the `App` global and register it in
/// `SystemServices`.
pub fn init_task_presentation(
    cx: &mut App,
    provider: Rc<dyn TaskPresentationProvider>,
) -> TaskPresentationService {
    let service = TaskPresentationService::new(provider);
    cx.set_global(service.clone());
    cx.default_global::<SystemServices>()
        .override_provider(service.clone());
    service
}

/// Resolve the service from the `App` global first, then from `SystemServices`.
pub fn task_presentation_service(cx: &App) -> Result<TaskPresentationService, SystemError> {
    if let Some(service) = cx.try_global::<TaskPresentationService>() {
        return Ok(service.clone());
    }
    let services = cx.try_global::<SystemServices>().ok_or_else(|| {
        SystemError::new(
            ErrorCode::Unavailable,
            "TaskPresentationService and SystemServices are not installed",
        )
    })?;
    services
        .service::<TaskPresentationService>()
        .map(|service| (*service).clone())
}

/// Convenience methods for GPUI applications.
pub trait TaskPresentationAppExt {
    fn install_task_presentation_service(&mut self, service: TaskPresentationService);
    fn init_task_presentation(
        &mut self,
        provider: Rc<dyn TaskPresentationProvider>,
    ) -> TaskPresentationService;
    fn task_presentation_service(&self) -> Result<TaskPresentationService, SystemError>;
}

impl TaskPresentationAppExt for App {
    fn install_task_presentation_service(&mut self, service: TaskPresentationService) {
        install_task_presentation_service(service, self);
    }

    fn init_task_presentation(
        &mut self,
        provider: Rc<dyn TaskPresentationProvider>,
    ) -> TaskPresentationService {
        init_task_presentation(self, provider)
    }

    fn task_presentation_service(&self) -> Result<TaskPresentationService, SystemError> {
        task_presentation_service(self)
    }
}

/// Entity-/surface-owned task-presentation scope.
///
/// The model retains the surface attachment and any event subscriptions. On drop
/// it disposes the surface generation (clearing surface-scoped tasks, badges,
/// attention and thumbbar actions), invalidates subscriptions and disposes the
/// `UiOwner`. Application-scoped tasks registered by the host are not removed.
pub struct TaskPresentationEntityModel {
    owner: UiOwner,
    service: TaskPresentationService,
    surface: SurfaceId,
    generation: u64,
    ui: Option<UiContext>,
    subscriptions: Vec<EventSubscription<TaskPresentationEvent>>,
    entity_scope: Option<EntityScope>,
}

impl TaskPresentationEntityModel {
    /// Create a scope for the owner's current attachment generation.
    ///
    /// A disposed owner produces an inert scope whose [`Self::context`] reports
    /// `ContextExpired`; it never panics in a destructor path.
    pub fn new(owner: UiOwner, service: TaskPresentationService) -> Self {
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

    pub fn try_new(owner: UiOwner, service: TaskPresentationService) -> Result<Self, SystemError> {
        let ui = owner.context(None)?;
        Ok(Self::with_ui(owner, service, ui))
    }

    fn with_ui(owner: UiOwner, service: TaskPresentationService, ui: UiContext) -> Self {
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
    pub fn for_entity<T: 'static>(
        owner: UiOwner,
        service: TaskPresentationService,
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

    pub fn service(&self) -> &TaskPresentationService {
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
                "Task-presentation surface scope has no live UI attachment",
            )
        })?;
        if !ui.is_current() {
            return Err(SystemError::new(
                ErrorCode::ContextExpired,
                "Task-presentation surface scope UI attachment is stale",
            ));
        }
        Ok(OperationContext {
            ui: Some(ui),
            ..OperationContext::default()
        })
    }

    pub fn subscribe(&mut self) -> &EventSubscription<TaskPresentationEvent> {
        self.subscriptions.push(self.service.subscribe());
        self.subscriptions
            .last()
            .expect("a subscription was just retained")
    }

    pub fn subscription_count(&self) -> usize {
        self.subscriptions.len()
    }

    pub fn take_subscription(&mut self) -> Option<EventSubscription<TaskPresentationEvent>> {
        self.subscriptions.pop()
    }

    /// Recreate this surface at a higher generation and replay application tasks.
    pub fn recreate(&mut self, new_generation: u64) -> Result<(), SystemError> {
        if new_generation <= self.generation {
            return Err(SystemError::new(
                ErrorCode::InvalidInput,
                "Recreated surface generation must increase monotonically",
            ));
        }
        while self.generation < new_generation {
            self.owner.advance_generation()?;
            self.generation = self.generation.saturating_add(1);
        }
        let ui = self.owner.context(None)?;
        self.service
            .recreate_surface_at(self.surface, ui.generation, std::time::Instant::now())?;
        self.ui = Some(ui);
        Ok(())
    }

    /// Notify focus and clear focus-owned attention and badges.
    pub fn notify_focused(&self) {
        self.service.notify_surface_focused(self.surface);
    }

    /// Apply a host lifecycle snapshot to the shared service.
    pub fn sync_lifecycle(&self, lifecycle: &LifecycleState) {
        self.service.sync_lifecycle(lifecycle);
    }
}

impl Drop for TaskPresentationEntityModel {
    fn drop(&mut self) {
        self.subscriptions.clear();
        if self.ui.is_some() {
            self.service.dispose_surface(self.surface, self.generation);
        }
        self.owner.dispose();
        drop(self.entity_scope.take());
    }
}

impl std::fmt::Debug for TaskPresentationEntityModel {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("TaskPresentationEntityModel")
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
    use crate::{FakeTaskPresentationProvider, TaskKey, TaskProgressState, TaskScope};
    use gpui::{AppContext, TestAppContext};

    struct ScopeEntity {
        scope: TaskPresentationEntityModel,
    }

    #[gpui::test]
    fn installs_global_and_registry(cx: &mut TestAppContext) {
        cx.update(|cx| {
            gpui_system::gpui_adapter::install_system_services(SystemServices::new(), cx);
            let provider = Rc::new(FakeTaskPresentationProvider::new());
            let service = init_task_presentation(cx, provider);
            assert!(service.shares_state_with(&task_presentation_service(cx).expect("global")));
            assert!(cx.task_presentation_service().is_ok());
            let registered = cx
                .try_global::<SystemServices>()
                .expect("services")
                .service::<TaskPresentationService>()
                .expect("registry");
            assert!(service.shares_state_with(&registered));
        });
    }

    #[gpui::test]
    fn entity_model_attaches_and_disposes(cx: &mut TestAppContext) {
        let service = cx
            .update(|cx| init_task_presentation(cx, Rc::new(FakeTaskPresentationProvider::new())));
        let owner = UiOwner::new(SurfaceId(11));
        let entity = cx.update(|cx| {
            cx.new(|cx| ScopeEntity {
                scope: TaskPresentationEntityModel::for_entity(owner, service.clone(), cx),
            })
        });
        let task = cx.update(|cx| {
            entity.update(cx, |entity, _| {
                assert_eq!(entity.scope.surface(), SurfaceId(11));
                entity.scope.subscribe();
                assert_eq!(entity.scope.subscription_count(), 1);
                service
                    .register_task(
                        TaskKey::new("entity.task").expect("key"),
                        crate::TaskPriority::Normal,
                        TaskScope::Surface {
                            surface: SurfaceId(11),
                            generation: 1,
                        },
                        TaskProgressState::Indeterminate,
                    )
                    .expect("task")
            })
        });
        assert!(service.is_task_active(task.id()));
        drop(entity);
        cx.update(|_| {});
        assert!(!service.is_task_active(task.id()));
    }
}
