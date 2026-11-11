//! Optional GPUI adapter over the distribution's coordinated GPUI identity.
//!
//! Installation and lookup must run on the owning GPUI UI thread. The core
//! [`RichTransferService`] remains UI-local; this adapter adds `App` globals,
//! `SystemServices` registration, entity-owned surface cleanup and bounded event
//! drainage.

use crate::{
    Completion, DndEvent, DndEventDrain, DragCancelReason, DragPoint, DragSessionLease,
    DragSourceSpec, DropCompletion, DropTargetLease, DropTargetSpec, EventSubscription,
    HandoffLease, HandoffReceipt, LifecycleState, LogicalRect, RichClipboardWrite,
    RichTransferProvider, RichTransferService, ServiceResult, SurfaceId, SystemError,
    SystemServices, TransferInteraction, UiContext, UiOwner,
};
use gpui::{App, Context, Global};
use gpui_system::{ErrorCode, gpui_adapter::EntityScope};
use std::collections::VecDeque;

impl Global for RichTransferService {}

/// Install the service into GPUI's `App` global.
pub fn install_dnd_service(service: RichTransferService, cx: &mut App) {
    cx.set_global(service);
}

/// Register a service in the typed `SystemServices` registry.
pub fn install_dnd_in_services(
    services: &mut SystemServices,
    service: RichTransferService,
) -> Result<(), SystemError> {
    services.register(service)
}

/// Build a service, install it as the `App` global and register it in
/// `SystemServices` for code that resolves services through the registry.
pub fn init_dnd(
    cx: &mut App,
    provider: std::rc::Rc<dyn RichTransferProvider>,
) -> RichTransferService {
    let service = RichTransferService::new(provider);
    cx.set_global(service.clone());
    cx.default_global::<SystemServices>()
        .override_provider(service.clone());
    service
}

/// Resolve the service from the `App` global first, then from `SystemServices`.
pub fn dnd_service(cx: &App) -> Result<RichTransferService, SystemError> {
    if let Some(service) = cx.try_global::<RichTransferService>() {
        return Ok(service.clone());
    }
    let services = cx.try_global::<SystemServices>().ok_or_else(|| {
        SystemError::new(
            ErrorCode::Unavailable,
            "RichTransferService and SystemServices are not installed",
        )
    })?;
    services
        .service::<RichTransferService>()
        .map(|service| (*service).clone())
}

/// Convenience methods for GPUI applications.
pub trait DndAppExt {
    fn install_dnd_service(&mut self, service: RichTransferService);
    fn init_dnd(&mut self, provider: std::rc::Rc<dyn RichTransferProvider>) -> RichTransferService;
    fn dnd_service(&self) -> Result<RichTransferService, SystemError>;
}

impl DndAppExt for App {
    fn install_dnd_service(&mut self, service: RichTransferService) {
        install_dnd_service(service, self);
    }

    fn init_dnd(&mut self, provider: std::rc::Rc<dyn RichTransferProvider>) -> RichTransferService {
        init_dnd(self, provider)
    }

    fn dnd_service(&self) -> Result<RichTransferService, SystemError> {
        dnd_service(self)
    }
}

/// Synchronize a host lifecycle snapshot into the rich-transfer service.
pub fn sync_lifecycle(service: &RichTransferService, lifecycle: &LifecycleState) {
    service.notify_lifecycle(lifecycle);
}

fn cancelled_message(reason: gpui_system::CancelReason) -> &'static str {
    match reason {
        gpui_system::CancelReason::User => "Rich transfer was cancelled by the user",
        gpui_system::CancelReason::Caller => "Rich transfer was cancelled by the caller",
        gpui_system::CancelReason::OwnerDisposed => "Rich transfer owner was disposed",
        gpui_system::CancelReason::ProcessPolicy => "Rich transfer was cancelled by process policy",
    }
}

/// Entity-/surface-owned transfer scope.
///
/// On drop it disposes **exactly** its `(surface, generation)` pair, so dropping
/// a stale scope never invalidates a newer generation of the same surface. The
/// `UiOwner` is disposed only while this scope's own UI attachment is still
/// current.
pub struct DndSurfaceScope {
    owner: UiOwner,
    service: RichTransferService,
    surface: SurfaceId,
    generation: u64,
    ui: Option<UiContext>,
    subscriptions: Vec<EventSubscription<DndEvent>>,
    entity_scope: Option<EntityScope>,
}

impl DndSurfaceScope {
    /// Create a scope for the owner's current attachment generation.
    pub fn new(owner: UiOwner, service: RichTransferService) -> Self {
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

    pub fn try_new(owner: UiOwner, service: RichTransferService) -> Result<Self, SystemError> {
        let ui = owner.context(None)?;
        Ok(Self::with_ui(owner, service, ui))
    }

    fn with_ui(owner: UiOwner, service: RichTransferService, ui: UiContext) -> Self {
        let surface = ui.surface;
        let generation = ui.generation;
        let _ = service.attach_surface(surface, generation, true);
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
    pub fn for_entity<T: 'static>(
        owner: UiOwner,
        service: RichTransferService,
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

    pub fn service(&self) -> &RichTransferService {
        &self.service
    }

    pub const fn surface(&self) -> SurfaceId {
        self.surface
    }

    pub const fn generation(&self) -> u64 {
        self.generation
    }

    pub fn context(&self) -> Result<gpui_system::OperationContext, SystemError> {
        let ui = self.ui.clone().ok_or_else(|| {
            SystemError::new(
                ErrorCode::ContextExpired,
                "Rich-transfer surface scope has no live UI attachment",
            )
        })?;
        if !ui.is_current() {
            return Err(SystemError::new(
                ErrorCode::ContextExpired,
                "Rich-transfer surface scope UI attachment is stale",
            ));
        }
        Ok(gpui_system::OperationContext {
            ui: Some(ui),
            ..gpui_system::OperationContext::default()
        })
    }

    pub fn user_initiated(&self) -> Result<TransferInteraction, SystemError> {
        let ui = self.ui.clone().ok_or_else(|| {
            SystemError::new(
                ErrorCode::ContextExpired,
                "Rich-transfer surface scope has no live UI attachment",
            )
        })?;
        self.service.user_initiated_with(ui)
    }

    pub fn subscribe(&mut self) -> &EventSubscription<DndEvent> {
        self.subscriptions.push(self.service.subscribe());
        self.subscriptions
            .last()
            .expect("a subscription was just retained")
    }

    pub fn subscription_count(&self) -> usize {
        self.subscriptions.len()
    }

    pub fn take_subscription(&mut self) -> Option<EventSubscription<DndEvent>> {
        self.subscriptions.pop()
    }

    pub fn write_clipboard(&self, write: RichClipboardWrite) -> ServiceResult<HandoffLease> {
        if let crate::HandoffScope::Surface {
            surface,
            generation,
        } = write.scope
            && (surface != self.surface || generation != self.generation)
        {
            return Err(SystemError::new(
                ErrorCode::InvalidInput,
                "Clipboard handoff belongs to another surface generation",
            ));
        }
        self.service.write_clipboard(self.user_initiated()?, write)
    }

    pub fn write_clipboard_receipt(
        &self,
        write: RichClipboardWrite,
    ) -> ServiceResult<(HandoffReceipt, HandoffLease)> {
        if let crate::HandoffScope::Surface {
            surface,
            generation,
        } = write.scope
            && (surface != self.surface || generation != self.generation)
        {
            return Err(SystemError::new(
                ErrorCode::InvalidInput,
                "Clipboard handoff belongs to another surface generation",
            ));
        }
        self.service
            .write_clipboard_receipt(self.user_initiated()?, write)
    }

    pub fn read_clipboard(
        &self,
        preferences: &[crate::FormatDescriptor],
    ) -> ServiceResult<crate::ClipboardReadOutcome> {
        self.service
            .read_clipboard(self.user_initiated()?, preferences)
    }

    pub fn register_drop_target(
        &self,
        spec: DropTargetSpec,
    ) -> Result<DropTargetLease, SystemError> {
        if spec.surface != self.surface || spec.generation != self.generation {
            return Err(SystemError::new(
                ErrorCode::InvalidInput,
                "Drop target belongs to another surface generation",
            ));
        }
        self.service.register_drop_target(spec)
    }

    /// Register a target rectangle in this scope's surface generation.
    pub fn register_drop_target_at(
        &self,
        rect: LogicalRect,
        accepted_formats: impl IntoIterator<Item = crate::FormatDescriptor>,
    ) -> Result<DropTargetLease, SystemError> {
        let spec = DropTargetSpec::new(self.surface, self.generation, rect, accepted_formats)?;
        self.service.register_drop_target(spec)
    }

    pub fn start_drag(&self, spec: DragSourceSpec) -> ServiceResult<DragSessionLease> {
        let spec = if let Some((surface, generation)) = spec.source_scope() {
            if surface != self.surface || generation != self.generation {
                return Err(SystemError::new(
                    ErrorCode::InvalidInput,
                    "Drag source belongs to another surface generation",
                ));
            }
            spec
        } else {
            spec.for_surface(self.surface, self.generation)
        };
        self.service.start_drag(self.user_initiated()?, spec)
    }

    pub fn drag_enter(
        &self,
        session: &DragSessionLease,
        point: DragPoint,
        modifiers: crate::DragModifiers,
    ) -> ServiceResult<crate::DragUpdate> {
        session.drag_enter(self.surface, self.generation, point, modifiers)
    }

    pub fn drag_over(
        &self,
        session: &DragSessionLease,
        point: DragPoint,
        modifiers: crate::DragModifiers,
    ) -> ServiceResult<crate::DragUpdate> {
        session.drag_over(self.surface, self.generation, point, modifiers)
    }

    pub fn complete_drop(
        &self,
        session: &DragSessionLease,
        point: DragPoint,
        modifiers: crate::DragModifiers,
    ) -> ServiceResult<DropCompletion> {
        session.complete_drop(self.surface, self.generation, point, modifiers)
    }

    pub fn notify_visible(&self) {
        self.service
            .notify_surface_visible(self.surface, self.generation);
    }

    pub fn notify_hidden(&self) {
        self.service
            .notify_surface_hidden(self.surface, self.generation);
    }

    pub fn sync_lifecycle(&self, lifecycle: &LifecycleState) {
        self.service.notify_lifecycle(lifecycle);
    }
}

impl Drop for DndSurfaceScope {
    fn drop(&mut self) {
        self.subscriptions.clear();
        if self.ui.is_some() {
            self.service
                .dispose_surface_generation(self.surface, self.generation);
        }
        // Never invalidate a newer generation sharing this `UiOwner`.
        if self.ui.as_ref().is_some_and(UiContext::is_current) {
            self.owner.dispose();
        }
        drop(self.entity_scope.take());
    }
}

impl std::fmt::Debug for DndSurfaceScope {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("DndSurfaceScope")
            .field("surface", &self.surface)
            .field("generation", &self.generation)
            .field("subscriptions", &self.subscriptions.len())
            .finish()
    }
}

/// GPUI entity model that owns one surface scope, its handoff leases, drop
/// targets and optional drag session.
///
/// Create it with `cx.new(|cx| DndEntityModel::new(owner, service, cx))`. It
/// retains a bounded event subscription, keeps at most
/// [`Self::MAX_RETAINED_EVENTS`] events, and releases every lease plus its
/// surface generation on drop/entity release.
pub struct DndEntityModel {
    scope: DndSurfaceScope,
    handoffs: Vec<HandoffLease>,
    targets: Vec<DropTargetLease>,
    drag: Option<DragSessionLease>,
    subscription: EventSubscription<DndEvent>,
    events: VecDeque<DndEvent>,
    resync_required: bool,
}

impl DndEntityModel {
    pub const MAX_RETAINED_EVENTS: usize = 64;

    pub fn new(owner: UiOwner, service: RichTransferService, cx: &mut Context<Self>) -> Self {
        let scope = DndSurfaceScope::for_entity(owner, service, cx);
        let subscription = scope.service().subscribe();
        Self {
            scope,
            handoffs: Vec::new(),
            targets: Vec::new(),
            drag: None,
            subscription,
            events: VecDeque::new(),
            resync_required: false,
        }
    }

    pub fn service(&self) -> &RichTransferService {
        self.scope.service()
    }

    pub const fn surface(&self) -> SurfaceId {
        self.scope.surface()
    }

    pub const fn generation(&self) -> u64 {
        self.scope.generation()
    }

    pub fn active_handoff_count(&self) -> usize {
        self.handoffs.len()
    }

    pub fn active_target_count(&self) -> usize {
        self.targets.len()
    }

    pub fn has_active_drag(&self) -> bool {
        self.drag.as_ref().is_some_and(DragSessionLease::is_active)
    }

    pub fn drag_session(&self) -> Option<&DragSessionLease> {
        self.drag.as_ref()
    }

    pub fn write_clipboard(
        &mut self,
        write: RichClipboardWrite,
        cx: &mut Context<Self>,
    ) -> Result<HandoffReceipt, SystemError> {
        match self.scope.write_clipboard(write) {
            Ok(Completion::Completed(lease)) => {
                let receipt = lease.receipt().ok_or_else(|| {
                    SystemError::new(
                        ErrorCode::ContextExpired,
                        "Clipboard handoff disappeared immediately after publication",
                    )
                })?;
                self.handoffs.push(lease);
                cx.notify();
                Ok(receipt)
            }
            Ok(Completion::Cancelled(reason)) => Err(SystemError::new(
                ErrorCode::Unavailable,
                cancelled_message(reason),
            )),
            Err(error) => Err(error),
        }
    }

    pub fn read_clipboard(
        &mut self,
        preferences: &[crate::FormatDescriptor],
        cx: &mut Context<Self>,
    ) -> Result<crate::ClipboardReadOutcome, SystemError> {
        let outcome = match self.scope.read_clipboard(preferences)? {
            Completion::Completed(outcome) => outcome,
            Completion::Cancelled(reason) => {
                return Err(SystemError::new(
                    ErrorCode::Unavailable,
                    cancelled_message(reason),
                ));
            }
        };
        cx.notify();
        Ok(outcome)
    }

    pub fn register_drop_target(
        &mut self,
        spec: DropTargetSpec,
        cx: &mut Context<Self>,
    ) -> Result<crate::DropTargetId, SystemError> {
        let lease = self.scope.register_drop_target(spec)?;
        let id = lease.id();
        self.targets.push(lease);
        cx.notify();
        Ok(id)
    }

    pub fn start_drag(
        &mut self,
        spec: DragSourceSpec,
        cx: &mut Context<Self>,
    ) -> Result<crate::DragSessionId, SystemError> {
        if self.has_active_drag() {
            return Err(SystemError::new(
                ErrorCode::Busy,
                "The entity already owns an active drag session",
            ));
        }
        let lease = match self.scope.start_drag(spec)? {
            Completion::Completed(lease) => lease,
            Completion::Cancelled(reason) => {
                return Err(SystemError::new(
                    ErrorCode::Unavailable,
                    cancelled_message(reason),
                ));
            }
        };
        let id = lease.id();
        self.drag = Some(lease);
        cx.notify();
        Ok(id)
    }

    pub fn cancel_drag(
        &mut self,
        reason: DragCancelReason,
        cx: &mut Context<Self>,
    ) -> Result<crate::DragCancelOutcome, SystemError> {
        let Some(drag) = self.drag.take() else {
            return Ok(crate::DragCancelOutcome::AlreadyTerminal);
        };
        let outcome = match drag.cancel(reason)? {
            Completion::Completed(outcome) => outcome,
            Completion::Cancelled(cancelled) => {
                return Err(SystemError::new(
                    ErrorCode::Unavailable,
                    cancelled_message(cancelled),
                ));
            }
        };
        drop(drag);
        cx.notify();
        Ok(outcome)
    }

    pub fn complete_drop(
        &mut self,
        point: DragPoint,
        modifiers: crate::DragModifiers,
        cx: &mut Context<Self>,
    ) -> Result<DropCompletion, SystemError> {
        let Some(drag) = self.drag.take() else {
            return Err(SystemError::new(
                ErrorCode::Unavailable,
                "The entity owns no drag session",
            ));
        };
        let completion =
            match drag.complete_drop(self.surface(), self.generation(), point, modifiers)? {
                Completion::Completed(completion) => completion,
                Completion::Cancelled(cancelled) => {
                    return Err(SystemError::new(
                        ErrorCode::Unavailable,
                        cancelled_message(cancelled),
                    ));
                }
            };
        drop(drag);
        cx.notify();
        Ok(completion)
    }

    pub fn sync_lifecycle(&mut self, lifecycle: &LifecycleState, cx: &mut Context<Self>) {
        self.scope.sync_lifecycle(lifecycle);
        cx.notify();
    }

    pub fn notify_visible(&mut self, cx: &mut Context<Self>) {
        self.scope.notify_visible();
        cx.notify();
    }

    pub fn notify_hidden(&mut self, cx: &mut Context<Self>) {
        self.scope.notify_hidden();
        cx.notify();
    }

    /// Drain all currently available events without blocking.
    ///
    /// Provider overflow is sticky until [`Self::clear_resync_required`] is
    /// called. Local history overflow also sets it.
    pub fn drain_events(&mut self) -> DndEventDrain {
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
        DndEventDrain {
            resync_required: self.resync_required,
            events,
        }
    }

    pub const fn resync_required(&self) -> bool {
        self.resync_required
    }

    pub fn clear_resync_required(&mut self) {
        self.resync_required = false;
    }

    pub fn retained_events(&self) -> impl Iterator<Item = &DndEvent> {
        self.events.iter()
    }

    pub fn take_retained_events(&mut self) -> Vec<DndEvent> {
        self.events.drain(..).collect()
    }
}

impl Drop for DndEntityModel {
    fn drop(&mut self) {
        // Explicitly release owned leases before the surface scope disposes its
        // generation. Both paths are idempotent.
        self.drag = None;
        self.targets.clear();
        self.handoffs.clear();
    }
}

impl std::fmt::Debug for DndEntityModel {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("DndEntityModel")
            .field("surface", &self.surface())
            .field("generation", &self.generation())
            .field("handoffs", &self.handoffs.len())
            .field("targets", &self.targets.len())
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
    use crate::{AllowedEffects, DragPreview, FakeRichTransferProvider, FormatDescriptor};
    use gpui::{AppContext, TestAppContext};
    use gpui_lifecycle::{AppPhase, MemoryPressure, SessionLockState};
    use std::rc::Rc;

    fn preview() -> DragPreview {
        DragPreview::new("drag").expect("preview")
    }

    #[gpui::test]
    fn scope_generation_drop_never_invalidates_a_newer_generation(cx: &mut TestAppContext) {
        cx.update(|cx| {
            let fake = FakeRichTransferProvider::new();
            let service = init_dnd(cx, Rc::new(fake.clone()));
            assert!(dnd_service(cx).expect("global").shares_state_with(&service));
            let mut services = SystemServices::new();
            install_dnd_in_services(&mut services, service.clone()).expect("registration");
            assert!(
                services
                    .service::<RichTransferService>()
                    .expect("registered")
                    .shares_state_with(&service)
            );
            let lifecycle = LifecycleState::from_parts(
                AppPhase::ForegroundActive,
                [],
                SessionLockState::Unlocked,
                MemoryPressure::Normal,
            );
            sync_lifecycle(&service, &lifecycle);
            let owner = UiOwner::new(SurfaceId(71));
            let old = DndSurfaceScope::new(owner.clone(), service.clone());
            let target = old
                .register_drop_target_at(
                    LogicalRect::new(0, 0, 1000, 1000).expect("rect"),
                    [FormatDescriptor::plain_text()],
                )
                .expect("target");
            assert_eq!(service.drop_target_ids().len(), 1);
            owner.advance_generation().expect("advance");
            let new = DndSurfaceScope::new(owner.clone(), service.clone());
            let new_target = new
                .register_drop_target_at(
                    LogicalRect::new(0, 0, 1000, 1000).expect("rect"),
                    [FormatDescriptor::plain_text()],
                )
                .expect("target");
            drop(old);
            assert_eq!(service.drop_target_ids(), vec![new_target.id()]);
            assert!(!target.is_active());
            assert!(owner.context(None).is_ok(), "new gen owner survives");
            assert_eq!(new.generation(), 2);
        });
    }

    #[gpui::test]
    fn entity_model_writes_reads_drags_and_drains_events(cx: &mut TestAppContext) {
        let (entity, fake, service, owner) = cx.update(|cx| {
            let fake = FakeRichTransferProvider::new();
            let service = init_dnd(cx, Rc::new(fake.clone()));
            let owner = UiOwner::new(SurfaceId(72));
            let entity = cx.new(|cx| DndEntityModel::new(owner.clone(), service.clone(), cx));
            (entity, fake, service, owner)
        });
        cx.update(|cx| {
            entity.update(cx, |model, cx| {
                let write = RichClipboardWrite::application()
                    .with_eager(crate::TypedPayload::plain_text("hello").expect("text"))
                    .expect("write");
                let receipt = model.write_clipboard(write, cx).expect("write clipboard");
                assert_eq!(receipt.eager_formats, vec![FormatDescriptor::plain_text()]);
                assert_eq!(model.active_handoff_count(), 1);
                let outcome = model
                    .read_clipboard(&[FormatDescriptor::plain_text()], cx)
                    .expect("read");
                assert_eq!(
                    outcome.matched().and_then(|payload| payload.as_text()),
                    Some("hello")
                );
                let target = DropTargetSpec::new(
                    model.surface(),
                    model.generation(),
                    LogicalRect::new(0, 0, 1000, 1000).expect("rect"),
                    [FormatDescriptor::plain_text()],
                )
                .expect("target");
                model.register_drop_target(target, cx).expect("register");
                let source = DragSourceSpec::new(preview(), AllowedEffects::all())
                    .with_eager(crate::TypedPayload::plain_text("drag").expect("text"))
                    .expect("source");
                let session = model.start_drag(source, cx).expect("start drag");
                assert_eq!(fake.start_drag_call_count(), 1);
                let drag = model.drag.as_ref().expect("drag");
                let update = match model
                    .scope
                    .drag_over(
                        drag,
                        DragPoint::from_milli(10, 10),
                        crate::DragModifiers::default(),
                    )
                    .expect("over")
                {
                    Completion::Completed(update) => update,
                    Completion::Cancelled(reason) => panic!("unexpected cancel: {reason:?}"),
                };
                assert!(update.is_accepted(), "{update:?}");
                let completion = model
                    .complete_drop(
                        DragPoint::from_milli(10, 10),
                        crate::DragModifiers::default(),
                        cx,
                    )
                    .expect("drop");
                assert!(completion.is_dropped(), "{completion:?}");
                assert_eq!(session, completion.outcome().expect("outcome").session_id);
                let lifecycle = LifecycleState::from_parts(
                    AppPhase::ForegroundActive,
                    [],
                    SessionLockState::Unlocked,
                    MemoryPressure::Normal,
                );
                model.sync_lifecycle(&lifecycle, cx);
                let drain = model.drain_events();
                assert!(!drain.events.is_empty());
                assert!(!model.resync_required());
            });
        });
        drop(entity);
        cx.update(|_| {});
        assert!(service.drop_target_ids().is_empty());
        assert!(fake.active_drag_tokens().is_empty());
        assert!(owner.context(None).is_err());
    }
}
