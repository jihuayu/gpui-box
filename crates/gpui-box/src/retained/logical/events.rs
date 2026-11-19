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
    pub effects: &'a std::cell::RefCell<UiInputEffects>,
    pub outcomes: &'a std::cell::RefCell<Vec<UiScrollOutcome>>,
    pub skip_stale: bool,
    pub adopted: Option<&'a dyn Fn()>,
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
    control: &'a mut EventControl,
    cancelable: bool,
    scope: ScopeId,
    tasks: &'a mut super::tasks::EventTasks,
    wheel_remaining: Option<&'a mut [f32; 2]>,
    wheel_anchor: Option<&'a mut Option<UiNodeId>>,
    current_target: UiNodeId,
}
impl EventContext<'_, '_> {
    /// Adopt a keyed list data revision and its scroll rebase in this input's
    /// ordinary source transaction. No second post-event commit is needed.
    pub fn rebase_virtual_list<C: Component>(
        &mut self,
        items: Signal<virtual_list::VirtualListItems<C>>,
        scroll: Signal<[f32; 2]>,
        new: virtual_list::VirtualListItems<C>,
        old_viewport: f32,
        new_viewport: f32,
        policy: virtual_list::VirtualListScrollPolicy,
    ) -> Result<virtual_list::VirtualListRebase, Error> {
        self.tx
            .rebase_virtual_list(items, scroll, new, old_viewport, new_viewport, policy)
    }
    /// Remaining logical delta after inner controlled handlers. Present only in
    /// the combined input transaction for Wheel; standalone dispatch owns no default.
    pub fn remaining_wheel_delta(&self) -> Option<[f32; 2]> {
        self.wheel_remaining.as_deref().copied()
    }
    /// Consume a bounded source-owned scroll and leave residual distance for
    /// ancestor handlers/defaults. Uses current transaction values, not stale
    /// installed offsets. Failure rolls back with the entire input candidate.
    pub fn scroll_wheel(
        &mut self,
        offset: Signal<[f32; 2]>,
        maximum: [f32; 2],
        axes: [bool; 2],
    ) -> Result<input::UiScrollStep, Error> {
        let delta = self.remaining_wheel_delta().ok_or(Error::InvalidScroll)?;
        let step = UiScrollState {
            offset: *self.tx.read(offset)?,
            maximum,
            wheel_axes: axes,
        }
        .consume(delta)
        .map_err(Error::Input)?;
        self.tx.set(offset, step.offset)?;
        *self
            .wheel_remaining
            .as_deref_mut()
            .expect("checked wheel context") = step.remaining;
        // This handler consumed its source-owned scroll. Defaults resume at
        // this same surviving route node, even if its virtual row was retired.
        *self
            .wheel_anchor
            .as_deref_mut()
            .expect("combined wheel anchor") = Some(self.current_target);
        Ok(step)
    }
    /// Declare short-lived UI work owned by this handler's scope. Launch only
    /// follows complete transaction adoption; Busy leaves prior work intact.
    pub fn start_task<T: Send + Sync + 'static>(
        &mut self,
        key: u64,
        launch: impl FnOnce(UiTaskContext<T>) + 'static,
    ) -> Result<UiTask<T>, Error> {
        self.tasks.start(self.scope, key, launch)
    }
    pub fn task<T: 'static>(&self, key: u64) -> Result<Option<UiTask<T>>, Error> {
        self.tasks.get(self.scope, key)
    }
    pub fn cancel_task(&mut self, key: u64) -> Result<bool, Error> {
        self.tasks.cancel(self.scope, key)
    }
    /// Stop the remaining ancestor callbacks for this routed event only.
    pub fn stop_propagation(&mut self) {
        self.control.propagation_stopped = true;
    }
    /// Cancel a supported default, independently of propagation. Returns false
    /// for notifications/actions whose default has already happened.
    pub fn prevent_default(&mut self) -> bool {
        if self.cancelable {
            self.control.default_prevented = true;
        }
        self.cancelable
    }
    pub fn default_prevented(&self) -> bool {
        self.control.default_prevented
    }
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
    feedback: BTreeMap<NodeId, FeedbackSource>,
}
#[derive(Clone)]
struct FeedbackSource {
    owner: ScopeId,
    prop: ReadProp<style::StyleState>,
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
            && self
                .feedback
                .iter()
                .all(|(node, entry)| !nodes.contains(node) && !dead.contains(&entry.owner))
        {
            return Ok(None);
        }
        let mut next = self.clone();
        next.bump()?;
        next.handlers.retain(|(scope, _), _| !dead.contains(scope));
        next.policies.retain(|node, _| !nodes.contains(node));
        next.feedback
            .retain(|node, entry| !nodes.contains(node) && !dead.contains(&entry.owner));
        Ok(Some(next))
    }
}
impl LogicalEdit<'_, '_, '_> {
    /// Scope-owned read-only style input. Reuses an explicit keyed source;
    /// repeated declaration preserves router state while replacing only the
    /// caller-owned disabled/selected inputs. One source can serve one node.
    ///
    /// ```compile_fail
    /// use gpui_box::retained::{logical::ReadProp, reactive::ReactiveTransaction, style::StyleState};
    /// fn overwrite(tx: &mut ReactiveTransaction<'_>, input: ReadProp<StyleState>) {
    ///     tx.set(input, StyleState::default()).unwrap();
    /// }
    /// ```
    pub fn interaction_state(
        &mut self,
        scope: ScopeId,
        key: u64,
        node: NodeId,
        disabled: bool,
        selected: bool,
    ) -> Result<ReadProp<style::StyleState>, Error> {
        let mut attempt = self.component_attempt()?;
        self.scope(scope)?;
        if self.retired_scopes()?.contains(&scope) {
            return Err(Error::InvalidScope);
        }
        if self.inserted.get(&node).or_else(|| self.owners.get(&node)) != Some(&scope) {
            return Err(Error::ScopeInUse);
        }
        let previous = self.event_registry().feedback.get(&node).cloned();
        let mut state = if let Some(entry) = &previous {
            if entry.owner != scope {
                return Err(Error::ScopeInUse);
            }
            *self.read_prop(entry.prop)?
        } else {
            style::StyleState::default()
        };
        state.disabled = disabled;
        state.selected = selected;
        let prop = self.prop(scope, key, state)?;
        if previous
            .as_ref()
            .is_some_and(|old| old.prop.dependency() != prop.dependency())
            || self.event_registry().feedback.iter().any(|(other, entry)| {
                *other != node && entry.prop.dependency() == prop.dependency()
            })
        {
            return Err(Error::InvalidEventHandler);
        }
        if previous.is_none() {
            let registry = self.event_registry_mut();
            registry.bump()?;
            registry
                .feedback
                .insert(node, FeedbackSource { owner: scope, prop });
        }
        attempt.success = true;
        Ok(prop)
    }
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
    pub(super) fn stage_style_feedback(
        &mut self,
        feedback: &[(NodeId, UiStyleFeedback, bool)],
    ) -> Result<(), Error> {
        if feedback.is_empty() {
            return Ok(());
        }
        let removed = self.retired_outputs()?;
        let preview = self.render.preview(self.capabilities, &removed)?;
        let mut values = reactive::SourceValues::default();
        for (node, feedback, eligible) in feedback {
            if let Some(entry) = self.event_registry().feedback.get(node) {
                if preview.node(*node).is_err() {
                    continue;
                }
                let mut next = Some(*node);
                let mut enabled = *eligible;
                while let Some(id) = next {
                    let node = preview.node(id)?;
                    enabled &= node.style.display != taffy::Display::None
                        && !self
                            .event_registry()
                            .policies
                            .get(&id)
                            .is_some_and(|policy| policy.disabled);
                    next = node.parent;
                }
                // An old routing tree can outlive a physical reparent. Feedback
                // has no route of its own, so identity alone updates the same
                // live instance; router adoption subsequently refreshes hover.
                let old = *self.render.read(entry.prop.signal)?;
                values.push(entry.prop.signal, feedback.apply_style(old, enabled));
            }
        }
        self.render.prop_values(values)
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
            presentation: None,
        },
        frame,
        identities: Arc::new(identities),
        graph: Arc::new(graph),
        scrolls: Arc::new(scrolls),
    })
}
impl LogicalUi {
    /// Apply router feedback and default scrolling together. Callback dispatch
    /// and host image installation remain explicit. Stale feedback is ignored;
    /// wheel/reveal requests retain their strict final-route validation.
    pub fn apply_input_effects(
        &mut self,
        effects: &UiInputEffects,
    ) -> Result<LogicalScrollCommit<()>, Error> {
        self.edit_input_effects(effects, |_| Ok(()), |(), _| Ok(()))
    }
    pub fn edit_input_effects<S, R>(
        &mut self,
        effects: &UiInputEffects,
        sources: impl FnOnce(&mut ReactiveTransaction<'_>) -> Result<S, Error>,
        render: impl FnOnce(S, &mut LogicalEdit<'_, '_, '_>) -> Result<R, Error>,
    ) -> Result<LogicalScrollCommit<R>, Error> {
        let feedback = self.style_feedback(effects)?;
        self.edit_input_scroll(effects, sources, |value, draft| {
            let result = render(value, draft)?;
            draft.stage_style_feedback(&feedback)?;
            Ok(result)
        })
    }
    fn style_feedback(
        &self,
        effects: &UiInputEffects,
    ) -> Result<Vec<(NodeId, UiStyleFeedback, bool)>, Error> {
        let scene = self.input_scene()?;
        let mapping: BTreeMap<_, _> = scene
            .identities
            .iter()
            .map(|(&node, &id)| (id, node))
            .collect();
        let mut latest = BTreeMap::new();
        for feedback in &effects.feedback {
            if feedback.scene.runtime == scene.id.runtime
                && feedback.scene.epoch == scene.id.epoch
                && feedback.scene.tree == scene.id.tree
                && feedback.scene.source_revision <= scene.id.source_revision
                && let Some(node) = mapping.get(&feedback.node)
            {
                latest.insert(*node, (*feedback, scene.graph.eligible(feedback.node)));
            }
        }
        let feedback: Vec<_> = latest
            .into_iter()
            .map(|(node, (value, eligible))| (node, value, eligible))
            .collect();
        Ok(feedback)
    }
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
        let effects = std::cell::RefCell::new(effects.clone());
        let edit = self.edit_validated(
            sources,
            render,
            Some((constraints, viewport)),
            None::<fn(&mut UiTree) -> Result<(), Error>>,
            Some(ScrollDefaults {
                effects: &effects,
                outcomes: &outcomes,
                skip_stale: false,
                adopted: None,
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
        let tasks = std::cell::RefCell::new(Some(super::tasks::EventTasks::new(&self.tasks)));
        let mut control = EventControl::default();
        let (commit, _) = self.edit_local_frame(
            constraints,
            viewport,
            |tx| {
                let mut tasks = tasks.borrow_mut();
                (handler.0)(
                    event,
                    &mut EventContext {
                        tx,
                        effects: &mut effects,
                        control: &mut control,
                        cancelable: false, // single-dispatch API owns no defaults
                        scope,
                        tasks: tasks.as_mut().expect("event task draft"),
                        wheel_remaining: None,
                        wheel_anchor: None,
                        current_target: event.current_target,
                    },
                )
            },
            |(), edit| {
                tasks
                    .borrow_mut()
                    .take()
                    .expect("event task draft")
                    .stage(edit);
                Ok(())
            },
        )?;
        for effect in effects {
            effect();
        }
        Ok(Some(commit.result))
    }
}

#[cfg(test)]
mod feedback_tests;
mod pump;
#[cfg(test)]
mod scroll_tests;
#[cfg(test)]
mod task_tests;
#[cfg(test)]
mod tests;
pub use pump::{EventControl, LogicalInputCommit};
