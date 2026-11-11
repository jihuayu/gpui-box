//! Optional adapters over the distribution's single coordinated GPUI identity.
//! These functions must be called from the owning GPUI UI thread.

use crate::{Request, ServiceResult, SystemServices, UiOwner};
use gpui::{App, Context, Entity, Subscription, Task};
use std::future::Future;

impl gpui::Global for SystemServices {}

/// Install explicitly registered providers. No native providers are guessed.
pub fn install_system_services(services: SystemServices, cx: &mut App) {
    cx.set_global(services);
}

/// Spawn a non-Send UI future on GPUI's local foreground executor.
pub fn spawn_local<T: 'static>(cx: &App, future: impl Future<Output = T> + 'static) -> Task<T> {
    cx.foreground_executor().spawn(future)
}

/// Supply a GPUI timer so an unresponsive provider cannot leave deadlines pending.
/// Hold the returned Task, or detach intentionally; dropping it abandons Request.
pub fn run_request<T: 'static>(request: Request<T>, cx: &App) -> Task<ServiceResult<T>> {
    if let Some(deadline) = request.deadline() {
        let timer = cx
            .background_executor()
            .timer(deadline.saturating_duration_since(std::time::Instant::now()));
        spawn_local(cx, request.with_deadline_signal(timer))
    } else {
        spawn_local(cx, request)
    }
}

/// Entity-owned native context and observations. Retain this scope in the entity;
/// dropping it releases subscriptions and invalidates outstanding native work.
pub struct EntityScope {
    owner: UiOwner,
    subscriptions: Vec<Subscription>,
}

impl EntityScope {
    pub fn new<T: 'static>(owner: UiOwner, cx: &Context<T>) -> Self {
        let on_release = owner.clone();
        let subscription = cx.on_release(move |_, _| on_release.dispose());
        Self {
            owner,
            subscriptions: vec![subscription],
        }
    }

    pub fn owner(&self) -> &UiOwner {
        &self.owner
    }

    pub fn retain(&mut self, subscription: Subscription) {
        self.subscriptions.push(subscription);
    }
}

impl Drop for EntityScope {
    fn drop(&mut self) {
        self.owner.dispose();
    }
}

/// Observe only meaningful selector changes. GPUI retains its existing batched
/// notify scheduling; this does not build a separate reactive state graph.
pub fn observe_selector<T: 'static, V: Clone + PartialEq + 'static>(
    entity: &Entity<T>,
    select: impl Fn(&T) -> V + 'static,
    mut changed: impl FnMut(V, &mut App) + 'static,
    cx: &mut App,
) -> Subscription {
    let mut previous = select(entity.read(cx));
    cx.observe(entity, move |entity, cx| {
        let value = select(entity.read(cx));
        if value != previous {
            previous = value.clone();
            changed(value, cx);
        }
    })
}
