//! Scope-owned frontend callbacks over the shared immutable input router.
use super::*;
use crate::retained::{input::*, ui_ir::UiNodeId};
use std::{
    rc::Rc,
    sync::atomic::{AtomicU64, Ordering},
};

type Callback = dyn Fn(&UiInputDispatch, &mut EventContext<'_, '_>) -> Result<(), Error>;
/// One ordinary logical edit including default scrolling. Outcomes describe
/// successfully adopted wheel consumption; dispatches/feedback stay explicit.
pub struct LogicalScrollCommit<R> {
    pub edit: reactive::ReactiveEditCommit<R>,
    pub outcomes: Vec<UiScrollOutcome>,
}
pub(super) struct ScrollDefaults<'a> {
    pub effects: &'a UiInputEffects,
    pub outcomes: &'a std::cell::RefCell<Vec<UiScrollOutcome>>,
}
/// Owner-local immutable callback identity. Clone for equal keyed registration;
/// replacing this handle reserves a fresh UiHandlerId, never rebinds an old ID.
#[derive(Clone)]
pub struct EventHandler(Rc<Callback>);
impl EventHandler {
    pub fn new(
        callback: impl Fn(&UiInputDispatch, &mut EventContext<'_, '_>) -> Result<(), Error> + 'static,
    ) -> Self {
        Self(Rc::new(callback))
    }
}
/// One event's source transaction. Structural construction is settled by existing
/// producers after this handler returns. Business side effects use after_commit.
///
/// ```compile_fail
/// use gpui_box::retained::logical::EventContext;
/// fn unsafe_construction(cx: &mut EventContext<'_, '_>) { cx.properties(); }
/// ```
pub struct EventContext<'a, 'ui> {
    tx: &'a mut ReactiveTransaction<'ui>,
    effects: &'a mut Vec<Box<dyn FnOnce()>>,
}
impl EventContext<'_, '_> {
    pub fn read<T: 'static>(&mut self, signal: Signal<T>) -> Result<&T, Error> {
        self.tx.read(signal)
    }
    pub fn read_prop<T: 'static>(&mut self, prop: ReadProp<T>) -> Result<&T, Error> {
        self.tx.read_prop(prop)
    }
    pub fn read_selector<T: 'static>(&mut self, selector: StoreSelector<T>) -> Result<&T, Error> {
        self.tx.read(selector.signal)
    }
    pub fn set<T: PartialEq + 'static>(
        &mut self,
        signal: Signal<T>,
        value: T,
    ) -> Result<bool, Error> {
        self.tx.set(signal, value)
    }
    /// Runs in order after the complete source/component/layout/frame candidate
    /// commits. Captured Command sender/data stay frontend-owned; Busy/Closed is
    /// handled by this closure, independently of already adopted UI state.
    pub fn after_commit(&mut self, effect: impl FnOnce() + 'static) {
        self.effects.push(Box::new(effect));
    }
}
#[derive(Clone)]
struct Registration {
    id: UiHandlerId,
    callback: EventHandler,
}
#[derive(Clone, Default)]
pub(super) struct Registry {
    revision: u64,
    handlers: BTreeMap<(ScopeId, u64), Registration>,
    policies: BTreeMap<NodeId, UiInteraction>,
}
impl Registry {
    fn bump(&mut self) -> Result<(), Error> {
        self.revision = self
            .revision
            .checked_add(1)
            .ok_or(Error::RevisionExhausted)?;
        Ok(())
    }
    pub(super) fn retire_scopes(
        &self,
        dead: &BTreeSet<ScopeId>,
        nodes: &BTreeSet<NodeId>,
    ) -> Result<Option<Self>, Error> {
        if self.handlers.keys().all(|(scope, _)| !dead.contains(scope))
            && self.policies.keys().all(|node| !nodes.contains(node))
        {
            return Ok(None);
        }
        let mut next = self.clone();
        next.bump()?;
        next.handlers.retain(|(scope, _), _| !dead.contains(scope));
        next.policies.retain(|node, _| !nodes.contains(node));
        Ok(Some(next))
    }
}
impl LogicalEdit<'_, '_, '_> {
    pub(super) fn event_registry(&self) -> &Registry {
        self.events.as_ref().unwrap_or(self.live_events)
    }
    fn event_registry_mut(&mut self) -> &mut Registry {
        self.events.get_or_insert_with(|| self.live_events.clone())
    }
    /// Register one scope/key frontend callback; callback IDs are never reused,
    /// even if a failed provisional registration escaped to user code.
    pub fn event_handler(
        &mut self,
        scope: ScopeId,
        key: u64,
        handler: EventHandler,
    ) -> Result<UiHandlerId, Error> {
        let mut attempt = self.component_attempt()?;
        self.scope(scope)?;
        if self.retired_scopes()?.contains(&scope) {
            return Err(Error::InvalidScope);
        }
        if let Some(old) = self.event_registry().handlers.get(&(scope, key))
            && Rc::ptr_eq(&old.callback.0, &handler.0)
        {
            attempt.success = true;
            return Ok(old.id);
        }
        static NEXT: AtomicU64 = AtomicU64::new(1);
        let id = UiHandlerId(
            NEXT.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |n| n.checked_add(1))
                .map_err(|_| Error::RevisionExhausted)?,
        );
        let registry = self.event_registry_mut();
        registry.bump()?;
        registry.handlers.insert(
            (scope, key),
            Registration {
                id,
                callback: handler,
            },
        );
        attempt.success = true;
        Ok(id)
    }
    /// Input policy for render output owned by this scope. Handler IDs must
    /// belong to the same live scope. None explicitly removes the policy.
    pub fn interaction(
        &mut self,
        scope: ScopeId,
        node: NodeId,
        policy: Option<UiInteraction>,
    ) -> Result<(), Error> {
        let mut attempt = self.component_attempt()?;
        self.scope(scope)?;
        if self.retired_scopes()?.contains(&scope) {
            return Err(Error::InvalidScope);
        }
        if self.inserted.get(&node).or_else(|| self.owners.get(&node)) != Some(&scope) {
            return Err(Error::ScopeInUse);
        }
        if let Some(policy) = policy {
            self.validate_policy(scope, policy)?;
        }
        if self.event_registry().policies.get(&node).copied() != policy {
            let registry = self.event_registry_mut();
            registry.bump()?;
            if let Some(policy) = policy {
                registry.policies.insert(node, policy);
            } else {
                registry.policies.remove(&node);
            }
        }
        attempt.success = true;
        Ok(())
    }
    fn validate_policy(&self, scope: ScopeId, policy: UiInteraction) -> Result<(), Error> {
        for id in [
            policy.pointer_handler,
            policy.focus_handler,
            policy.activate_handler,
        ]
        .into_iter()
        .flatten()
        {
            if !self
                .event_registry()
                .handlers
                .iter()
                .any(|((owner, _), h)| *owner == scope && h.id == id)
            {
                return Err(Error::InvalidEventHandler);
            }
        }
        Ok(())
    }
    pub(super) fn retire_and_validate_events(
        &mut self,
        dead: &BTreeSet<ScopeId>,
    ) -> Result<(), Error> {
        let removed = self.render.tree().removed_ids()?;
        let retired = self.event_registry().retire_scopes(dead, &removed)?;
        if let Some(next) = retired {
            self.events = Some(next);
        }
        for (&node, &policy) in &self.event_registry().policies {
            let scope = *self
                .inserted
                .get(&node)
                .or_else(|| self.owners.get(&node))
                .ok_or(Error::InvalidNode)?;
            if dead.contains(&scope) {
                return Err(Error::InvalidEventHandler);
            }
            self.validate_policy(scope, policy)?;
        }
        Ok(())
    }
}
fn public_id(node: NodeId) -> UiNodeId {
    UiNodeId::new(node.slot as u64, node.generation)
}
fn current_input_frame(tree: &UiTree) -> Result<local::LocalFrame, Error> {
    if tree.layout_dirty || tree.full_extract_required || !tree.geometry_dirty.is_empty() {
        return Err(Error::InvalidFrame);
    }
    if let Some(frame) = tree
        .local_frame
        .as_ref()
        .filter(|frame| frame.revision() == tree.revision)
    {
        Ok(frame.clone())
    } else {
        if tree.dirty {
            return Err(Error::InvalidFrame);
        }
        let frame = tree
            .frame
            .as_ref()
            .filter(|frame| frame.revision == tree.revision)
            .ok_or(Error::InvalidFrame)?;
        Ok(local::LocalFrame::from_frame(frame.clone()))
    }
}
pub(super) fn input_scene(
    tree: &UiTree,
    identity: u64,
    registry: &Registry,
    controlled: &BTreeSet<NodeId>,
) -> Result<UiInputScene, Error> {
    let frame = current_input_frame(tree)?;
    let mut graph = input::InputGraph::default();
    let mut identities = BTreeMap::new();
    let mut scrolls = BTreeMap::new();
    let mut stack = vec![(tree.root(), None, true, true)];
    while let Some((id, parent, enabled, visible)) = stack.pop() {
        let node = tree.node(id)?;
        let policy = registry.policies.get(&id).copied().unwrap_or_default();
        let enabled = enabled && !policy.disabled;
        let visible = visible && node.style.display != taffy::Display::None;
        let source = public_id(id);
        identities.insert(id, source);
        graph.nodes.insert(
            source,
            input::InputNode {
                parent,
                policy,
                enabled,
                visible,
            },
        );
        if enabled && visible && policy.focus == UiFocusPolicy::Tab {
            graph.tab_order.push(source);
        }
        if node.style.overflow.x == taffy::Overflow::Scroll
            || node.style.overflow.y == taffy::Overflow::Scroll
        {
            let maximum = [
                node.published_layout.scroll_width(),
                node.published_layout.scroll_height(),
            ];
            scrolls.insert(
                source,
                UiScrollState {
                    offset: [
                        node.scroll[0].min(maximum[0]),
                        node.scroll[1].min(maximum[1]),
                    ],
                    maximum,
                    wheel_axes: [
                        !controlled.contains(&id)
                            && node.style.overflow.x == taffy::Overflow::Scroll,
                        !controlled.contains(&id)
                            && node.style.overflow.y == taffy::Overflow::Scroll,
                    ],
                },
            );
        }
        stack.extend(
            node.children
                .iter()
                .rev()
                .map(|&child| (child, Some(source), enabled, visible)),
        );
    }
    let source_revision = frame
        .revision()
        .checked_add(registry.revision)
        .ok_or(Error::RevisionExhausted)?;
    Ok(UiInputScene {
        id: UiInputSceneId {
            runtime: identity,
            epoch: 1,
            source_revision,
            tree: frame.tree(),
            frame_revision: frame.revision(),
        },
        frame,
        identities: Arc::new(identities),
        graph: Arc::new(graph),
        scrolls: Arc::new(scrolls),
    })
}
impl LogicalUi {
    /// Apply wheel and focus-reveal defaults with no application source edits.
    /// The caller still dispatches callbacks and installs the returned current
    /// publication in its host. This does not apply style feedback.
    pub fn apply_input_scroll(
        &mut self,
        effects: &UiInputEffects,
    ) -> Result<LogicalScrollCommit<()>, Error> {
        self.edit_input_scroll(effects, |_| Ok(()), |(), _| Ok(()))
    }
    /// Sources and producers settle first; wheel uses final clamped offsets and
    /// reveal uses final layout. Both publish with this same complete candidate.
    /// A signal-bound scroll is application-controlled and skips default writes.
    pub fn edit_input_scroll<S, R>(
        &mut self,
        effects: &UiInputEffects,
        sources: impl FnOnce(&mut ReactiveTransaction<'_>) -> Result<S, Error>,
        render: impl FnOnce(S, &mut LogicalEdit<'_, '_, '_>) -> Result<R, Error>,
    ) -> Result<LogicalScrollCommit<R>, Error> {
        // Require an existing complete input publication and reuse its host
        // environment rather than guessing constraints from window dimensions.
        let tree = self.ui.tree();
        current_input_frame(tree)?;
        let constraints = tree.layout_constraints.ok_or(Error::InvalidFrame)?;
        let viewport = tree.viewport.ok_or(Error::InvalidFrame)?;
        let outcomes = std::cell::RefCell::new(vec![]);
        let edit = self.edit_validated(
            sources,
            render,
            Some((constraints, viewport)),
            None::<fn(&mut UiTree) -> Result<(), Error>>,
            Some(ScrollDefaults {
                effects,
                outcomes: &outcomes,
            }),
        )?;
        Ok(LogicalScrollCommit {
            edit,
            outcomes: outcomes.into_inner(),
        })
    }
    /// Reuse the existing pointer/focus/activation router against this complete
    /// LogicalUi publication. Host must adopt it when installing that frame.
    /// Building the graph currently walks the complete render tree.
    pub fn input_scene(&self) -> Result<UiInputScene, Error> {
        let controlled = self.ui.bound_scroll_nodes();
        input_scene(self.ui.tree(), self.identity, &self.events, &controlled)
    }
    /// Deliver one previously routed event after checking current handler/scope,
    /// eligibility and ancestry. Stale dispatch returns None without user code.
    /// Changed position alone does not retarget a click to a newly hit element.
    pub fn dispatch_event(
        &mut self,
        event: &UiInputDispatch,
    ) -> Result<Option<ReactiveCommit<()>>, Error> {
        let Some((scope, handler)) = self
            .events
            .handlers
            .iter()
            .find(|(_, h)| h.id == event.handler)
            .map(|((scope, _), h)| (*scope, h.callback.clone()))
        else {
            return Ok(None);
        };
        let scene = self.input_scene()?;
        if event.scene.runtime != scene.id.runtime
            || event.scene.epoch != scene.id.epoch
            || event.scene.tree != scene.id.tree
            || event.scene.source_revision > scene.id.source_revision
            || !scene.graph.accepts(event)
        {
            return Ok(None);
        }
        self.scope(scope)?;
        // Input requires a complete current image, so its layout environment is
        // already known. Settle the event against that environment even without
        // persistent producers; ordinary source-only transactions may otherwise
        // defer frame extraction until a later host request.
        let tree = self.ui.tree();
        let constraints = tree.layout_constraints.ok_or(Error::InvalidFrame)?;
        let viewport = tree.viewport.ok_or(Error::InvalidFrame)?;
        let mut effects = vec![];
        let (commit, _) = self.edit_local_frame(
            constraints,
            viewport,
            |tx| {
                (handler.0)(
                    event,
                    &mut EventContext {
                        tx,
                        effects: &mut effects,
                    },
                )
            },
            |(), _| Ok(()),
        )?;
        for effect in effects {
            effect();
        }
        Ok(Some(commit.result))
    }
}

#[cfg(test)]
mod scroll_tests;
#[cfg(test)]
mod tests;
