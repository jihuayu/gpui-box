#![allow(dead_code)]

use gpui_lifecycle::{SurfaceSnapshot, SurfaceVisibility};
use gpui_menus::{
    CommandDescriptor, CommandId, ContextMenu, FakeMenuProvider, MenuCommandService, MenuNode,
    RegistrationLease,
};
use gpui_system::{
    Completion, Event, EventEnvelope, EventSubscription, OperationContext, Request, ServiceResult,
    SurfaceId, UiOwner,
};
use gpui_tray::*;
use std::{
    cell::Cell,
    future::Future,
    pin::Pin,
    rc::Rc,
    task::{Context, Poll, Waker},
};

pub fn context() -> OperationContext {
    OperationContext::default()
}

pub fn ui_context(surface: u64) -> (UiOwner, OperationContext) {
    let owner = UiOwner::new(SurfaceId(surface));
    let context = OperationContext {
        ui: Some(owner.context(None).expect("current ui context")),
        ..OperationContext::default()
    };
    (owner, context)
}

pub fn visible_snapshot(generation: u64) -> SurfaceSnapshot {
    SurfaceSnapshot {
        visibility: SurfaceVisibility::Visible { focused: true },
        generation,
    }
}

pub fn hidden_snapshot(generation: u64) -> SurfaceSnapshot {
    SurfaceSnapshot {
        visibility: SurfaceVisibility::Hidden,
        generation,
    }
}

pub fn key(value: &str) -> StatusItemKey {
    StatusItemKey::new(value).expect("valid status item key")
}

pub fn icon_variant(name: &str, scheme: ColorSchemeMatch, scale: ScaleBucket) -> StatusIconVariant {
    StatusIconVariant::new(
        IconPayload::asset_key(format!("tray/{name}.png")).expect("asset key"),
        scheme,
        scale,
    )
}

pub fn icon_set() -> StatusIconSet {
    StatusIconSet::new([icon_variant(
        "default",
        ColorSchemeMatch::Any,
        ScaleBucket::X1,
    )])
    .expect("icon set")
}

pub fn spec(value: &str) -> StatusItemSpec {
    StatusItemSpec::new(key(value), icon_set())
        .with_title("Status")
        .with_accessibility_label("Application status")
}

pub fn fake_service() -> (FakeTrayProvider, TrayService) {
    let fake = FakeTrayProvider::new();
    let service = TrayService::from_fake(fake.clone());
    (fake, service)
}

pub fn fake_service_with_menus() -> (
    FakeTrayProvider,
    TrayService,
    Rc<FakeMenuProvider>,
    MenuCommandService,
) {
    let fake = FakeTrayProvider::new();
    let provider = Rc::new(FakeMenuProvider::new());
    let menus = MenuCommandService::new(provider.clone());
    let service = TrayService::from_fake_with_menus(fake.clone(), menus.clone());
    (fake, service, provider, menus)
}

pub fn command_id(value: &str) -> CommandId {
    CommandId::new(value).expect("valid command id")
}

pub fn register_command(
    menus: &MenuCommandService,
    id: &str,
    runs: Rc<Cell<u32>>,
) -> RegistrationLease {
    let descriptor = CommandDescriptor::ui(command_id(id), "Test command").expect("descriptor");
    menus
        .register_ui(descriptor, move |_: (), _| {
            runs.set(runs.get().saturating_add(1));
            Ok(())
        })
        .expect("register command")
        .into_parts()
        .1
}

pub fn menu_for(command: &CommandId) -> ContextMenu {
    ContextMenu::new(vec![MenuNode::command(command.clone())])
}

pub fn next_payload<T>(subscription: &EventSubscription<T>) -> Option<Event<T>> {
    let mut next = subscription.try_next().ok()?;
    let waker = Waker::noop();
    let mut context = Context::from_waker(waker);
    match Pin::new(&mut next).poll(&mut context) {
        Poll::Ready(Some(EventEnvelope { payload, .. })) => Some(payload),
        Poll::Ready(None) | Poll::Pending => None,
    }
}

pub fn next_value<T>(subscription: &EventSubscription<T>) -> Option<T> {
    match next_payload(subscription) {
        Some(Event::Value(value)) => Some(value),
        Some(Event::ResyncRequired) | None => None,
    }
}

pub fn drain<T>(subscription: &EventSubscription<T>) -> Vec<Event<T>> {
    let mut events = Vec::new();
    while let Some(event) = next_payload(subscription) {
        events.push(event);
    }
    events
}

pub fn block_on<T>(mut request: Request<T>) -> ServiceResult<T> {
    let waker = Waker::noop();
    let mut context = Context::from_waker(waker);
    match Pin::new(&mut request).poll(&mut context) {
        Poll::Ready(result) => result,
        Poll::Pending => panic!("request did not complete synchronously"),
    }
}

pub fn completed<T>(result: ServiceResult<T>) -> T {
    match result.expect("operation succeeded") {
        Completion::Completed(value) => value,
        Completion::Cancelled(reason) => panic!("unexpected cancellation: {reason:?}"),
    }
}
