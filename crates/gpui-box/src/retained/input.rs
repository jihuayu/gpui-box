//! Backend-independent primary-pointer/focus routing over explicitly adopted
//! immutable publications. Host adoption is evidence, not automatic display proof.
use super::ui_ir::{UiDocument, UiError, UiNodeId, UiOperation};
use super::*;

pub(in crate::retained) mod reveal;
mod router;
pub(in crate::retained) mod scroll;
pub use scroll::{UiScrollOutcome, UiScrollPlan, UiScrollState, UiWheelScroll};
#[cfg(test)]
mod reveal_tests;
#[cfg(test)]
mod tests;
pub use router::UiInputRouter;

/// Default keyboard/programmatic focus reveal, targeted in the adopted scene.
/// Planning rechecks identity/ancestry; actual geometry is read after the
/// ordinary batch's final style/layout settlement.
#[derive(Clone, Debug, PartialEq)]
pub struct UiFocusReveal {
    pub scene: UiInputSceneId,
    pub evidence: UiInputEvidence,
    pub target: UiNodeId,
    pub route: Arc<[UiNodeId]>,
}

/// Immutable frontend registry identity. Nonzero; never rebind an ID to another
/// callback in the same runtime/epoch. Closures and registry remain frontend-owned.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UiHandlerId(pub u64);

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum UiFocusPolicy {
    #[default]
    None,
    Programmatic,
    Tab,
}

/// Pointer targeting is independent of fill/opacity. A pass-through node skips
/// its own box, not descendants. Disabled subtrees still block pointer hits but
/// cannot dispatch, capture, focus or activate. Hidden layout is ineligible.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UiInteraction {
    pub pointer_target: bool,
    pub disabled: bool,
    pub focus: UiFocusPolicy,
    pub pointer_handler: Option<UiHandlerId>,
    pub focus_handler: Option<UiHandlerId>,
    pub activate_handler: Option<UiHandlerId>,
}
impl Default for UiInteraction {
    fn default() -> Self {
        Self {
            pointer_target: true,
            disabled: false,
            focus: UiFocusPolicy::None,
            pointer_handler: None,
            focus_handler: None,
            activate_handler: None,
        }
    }
}
impl UiInteraction {
    pub(crate) fn validate(self) -> Result<(), UiError> {
        if [
            self.pointer_handler,
            self.focus_handler,
            self.activate_handler,
        ]
        .into_iter()
        .flatten()
        .any(|id| id.0 == 0)
        {
            return Err(UiError::InvalidHandler);
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UiInputSceneId {
    pub runtime: u64,
    pub epoch: u64,
    pub source_revision: u64,
    pub tree: u64,
    pub frame_revision: u64,
}
/// The host names what it actually knows when adopting geometry. No tool or
/// renderer completion is inferred from these caller-supplied evidence labels.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UiInputEvidence {
    Published,
    ImageInstalled,
    PresentAttempted,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UiActivationKey {
    Enter,
    Space,
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum UiInput {
    /// Host-normalized logical pixels. Positive components increase offsets;
    /// native sign, line/page units and modifiers are host responsibilities.
    Wheel {
        position: [f32; 2],
        delta: [f32; 2],
    },
    PointerMove([f32; 2]),
    PrimaryDown([f32; 2]),
    PrimaryUp([f32; 2]),
    PointerLeave,
    Cancel,
    WindowFocusLost,
    Tab {
        backwards: bool,
    },
    Focus(Option<UiNodeId>),
    /// Host forwards unmodified activation keys only; shortcuts/IME stay host-owned.
    KeyDown {
        key: UiActivationKey,
        repeat: bool,
    },
    KeyUp(UiActivationKey),
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UiInputEventKind {
    PointerEnter,
    PointerLeave,
    PointerMove,
    PointerDown,
    PointerUp,
    PointerCancel,
    Focus,
    Blur,
    Activate,
}
#[derive(Clone, Debug, PartialEq)]
pub struct UiInputDispatch {
    pub scene: UiInputSceneId,
    pub evidence: UiInputEvidence,
    pub target: UiNodeId,
    pub current_target: UiNodeId,
    /// Render ancestry at routing time, target first. Reparenting invalidates
    /// a delayed event even if the callback ID and leaf generation survive.
    pub route: Arc<[UiNodeId]>,
    pub handler: UiHandlerId,
    pub kind: UiInputEventKind,
    pub position: Option<[f32; 2]>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UiStyleFeedback {
    pub scene: UiInputSceneId,
    pub node: UiNodeId,
    pub hovered: bool,
    pub pressed: bool,
    pub focus_visible: bool,
}
#[derive(Clone, Debug, Default, PartialEq)]
pub struct UiInputEffects {
    pub dispatches: Vec<UiInputDispatch>,
    pub feedback: Vec<UiStyleFeedback>,
    pub wheel_scrolls: Vec<UiWheelScroll>,
    pub focus_reveals: Vec<UiFocusReveal>,
}
impl UiInputEffects {
    /// Preserve caller-owned disabled/selected state. Ignore retired identities;
    /// these ordinary typed operations still need a normal atomic UiBatch.
    pub fn style_operations(&self, document: &UiDocument) -> Vec<UiOperation> {
        let current_scene = document.input_scene();
        let current = current_scene.id();
        self.feedback
            .iter()
            .filter_map(|feedback| {
                if feedback.scene.runtime != current.runtime
                    || feedback.scene.epoch != current.epoch
                    || feedback.scene.tree != current.tree
                {
                    return None;
                }
                let node = document.node(feedback.node)?;
                let eligible = current_scene.graph.eligible(feedback.node);
                let mut value = node.style_state;
                value.hovered = eligible && feedback.hovered;
                value.pressed = eligible && feedback.pressed;
                value.focus_visible = eligible && feedback.focus_visible;
                (value != node.style_state).then_some(UiOperation::StyleState {
                    node: feedback.node,
                    value,
                })
            })
            .collect()
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UiInputError {
    NoScene,
    WrongRuntime,
    WrongEpoch,
    StaleScene,
    InvalidPosition,
    IneligibleFocus,
    InvalidScrollDelta,
    StaleScrollRoute,
    RevisionExhausted,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct InputNode {
    pub parent: Option<UiNodeId>,
    pub policy: UiInteraction,
    pub enabled: bool,
    pub visible: bool,
}
#[derive(Clone, Debug, PartialEq, Default)]
pub(crate) struct InputGraph {
    pub nodes: BTreeMap<UiNodeId, InputNode>,
    pub tab_order: Vec<UiNodeId>,
}
impl InputGraph {
    pub(crate) fn accepts(&self, event: &UiInputDispatch) -> bool {
        if !self.eligible(event.target)
            || !self.eligible(event.current_target)
            || self.path(event.target).as_slice() != event.route.as_ref()
            || !event.route.contains(&event.current_target)
        {
            return false;
        }
        let policy = self.nodes[&event.current_target].policy;
        let handler = match event.kind {
            UiInputEventKind::Focus | UiInputEventKind::Blur => policy.focus_handler,
            UiInputEventKind::Activate => policy.activate_handler,
            _ => policy.pointer_handler,
        };
        handler == Some(event.handler)
    }
    pub(crate) fn build(root: UiNodeId, nodes: &super::ui_ir::NodeMap) -> Self {
        let mut result = Self::default();
        let mut stack = vec![(root, None, true, true)];
        while let Some((id, parent, enabled, visible)) = stack.pop() {
            let node = &nodes[&id];
            let enabled = enabled && !node.interaction.disabled;
            let visible = visible && node.layout.display != LayoutDisplay::None;
            result.nodes.insert(
                id,
                InputNode {
                    parent,
                    policy: node.interaction,
                    enabled,
                    visible,
                },
            );
            if enabled && visible && node.interaction.focus == UiFocusPolicy::Tab {
                result.tab_order.push(id);
            }
            stack.extend(
                node.children
                    .iter()
                    .rev()
                    .map(|&child| (child, Some(id), enabled, visible)),
            );
        }
        result
    }
    pub(in crate::retained) fn eligible(&self, id: UiNodeId) -> bool {
        self.nodes.get(&id).is_some_and(|n| n.enabled && n.visible)
    }
    pub(in crate::retained) fn path(&self, id: UiNodeId) -> Vec<UiNodeId> {
        let mut path = vec![];
        let mut next = Some(id);
        while let Some(id) = next {
            path.push(id);
            next = self.nodes.get(&id).and_then(|n| n.parent);
        }
        path
    }
    pub(in crate::retained) fn focusable(&self, id: UiNodeId) -> bool {
        self.eligible(id) && self.nodes[&id].policy.focus != UiFocusPolicy::None
    }
    fn owner(&self, id: UiNodeId) -> UiNodeId {
        let path = self.path(id);
        path.iter()
            .copied()
            .find(|id| self.nodes[id].policy.activate_handler.is_some())
            .or_else(|| path.into_iter().find(|&id| self.focusable(id)))
            .unwrap_or(id)
    }
}

/// Complete source policy + exact immutable geometry. Holds no UiTree, engine
/// style or platform callback. Clone for the consumer installing a complete frame.
#[derive(Clone)]
pub struct UiInputScene {
    pub(crate) id: UiInputSceneId,
    pub(crate) frame: local::LocalFrame,
    pub(crate) identities: Arc<BTreeMap<NodeId, UiNodeId>>,
    pub(crate) graph: Arc<InputGraph>,
    pub(crate) scrolls: Arc<BTreeMap<UiNodeId, UiScrollState>>,
}
impl UiInputScene {
    /// Identity of a render node in this exact immutable input publication.
    /// Useful for explicit programmatic focus without exposing arena internals.
    pub fn node_id(&self, node: NodeId) -> Option<UiNodeId> {
        self.identities.get(&node).copied()
    }
    pub fn id(&self) -> UiInputSceneId {
        self.id
    }
    pub fn scroll_state(&self, node: UiNodeId) -> Option<UiScrollState> {
        self.scrolls.get(&node).copied()
    }
    pub fn hit_test(&self, position: [f32; 2]) -> Option<UiNodeId> {
        let internal = self
            .frame
            .hit_test_filtered(position[0], position[1], |id| {
                self.identities
                    .get(&id)
                    .and_then(|id| self.graph.nodes.get(id))
                    .is_some_and(|node| node.visible && node.policy.pointer_target)
            })?;
        self.identities.get(&internal).copied()
    }
    fn dispatch(
        &self,
        evidence: UiInputEvidence,
        target: UiNodeId,
        kind: UiInputEventKind,
        position: Option<[f32; 2]>,
        out: &mut UiInputEffects,
    ) {
        let cleanup = matches!(
            kind,
            UiInputEventKind::Blur
                | UiInputEventKind::PointerCancel
                | UiInputEventKind::PointerLeave
        );
        if !cleanup && !self.graph.eligible(target) {
            return;
        }
        let route: Arc<[UiNodeId]> = self.graph.path(target).into();
        for &current in route.iter() {
            let node = &self.graph.nodes[&current];
            if !cleanup && (!node.enabled || !node.visible) {
                break;
            }
            let handler = match kind {
                UiInputEventKind::Focus | UiInputEventKind::Blur => node.policy.focus_handler,
                UiInputEventKind::Activate => node.policy.activate_handler,
                _ => node.policy.pointer_handler,
            };
            if let Some(handler) = handler {
                out.dispatches.push(UiInputDispatch {
                    scene: self.id,
                    evidence,
                    target,
                    current_target: current,
                    route: route.clone(),
                    handler,
                    kind,
                    position,
                });
            }
            if matches!(
                kind,
                UiInputEventKind::Focus
                    | UiInputEventKind::Blur
                    | UiInputEventKind::PointerEnter
                    | UiInputEventKind::PointerLeave
            ) || kind == UiInputEventKind::Activate && handler.is_some()
            {
                break;
            }
        }
    }
}
