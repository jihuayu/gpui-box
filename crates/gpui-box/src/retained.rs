//! Persistent rectangle UI foundation. Layout uses the pinned Taffy engine.
//! The GPUI adapter currently submits the owned frame through ordinary paint.

use std::{
    collections::BTreeMap,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};
pub use taffy;
pub use taffy::Style;
use taffy::prelude::*;
pub use taffy::style_helpers::{length, percent};
pub mod arena;
pub mod frame_queue;
pub mod logical;
mod measure;
pub use measure::{CustomMeasureSnapshot, ImageIntrinsicSize, LayoutStats, MeasureSnapshot};
mod primitive;
mod properties;
pub use primitive::QuadDecoration;
pub mod reactive;
pub mod responsive;
pub use properties::{Damage, PropertyNode, PropertyState, PropertyTrees};
#[cfg(all(feature = "retained-wgpu", not(target_family = "wasm")))]
pub mod wgpu;

/// Identity belongs to one tree and one lifetime of its arena slot.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct NodeId {
    tree: u64,
    slot: usize,
    generation: u64,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

#[derive(Clone, Debug, PartialEq)]
pub enum DisplayCommand {
    Fill(u32),
    DecoratedFill {
        color: u32,
        decoration: QuadDecoration,
    },
}

/// Local commands survive commits that only change layout or translation.
#[derive(Clone, Debug)]
pub struct PaintItem {
    pub node: NodeId,
    pub bounds: Rect,
    pub commands: Arc<[DisplayCommand]>,
    /// Current solid material, independent of the reusable command recording.
    pub fill_color: u32,
    pub clip: Option<Rect>,
    pub properties: PropertyState,
}

/// Owned output can be read after the producing tree has changed or been dropped.
#[derive(Clone, Debug)]
pub struct FramePacket {
    pub tree: u64,
    pub base_revision: Option<u64>,
    pub revision: u64,
    pub items: Arc<[PaintItem]>,
    pub layout_runs: u64,
    pub recorded_chunks: u64,
    pub layout_stats: LayoutStats,
    pub updates: Arc<[PaintItem]>,
    pub removed: Arc<[NodeId]>,
    pub order: Option<Arc<[NodeId]>>,
    pub properties: Arc<PropertyTrees>,
    pub damage: Damage,
}

impl FramePacket {
    pub fn hit_test(&self, x: f32, y: f32) -> Option<NodeId> {
        self.items
            .iter()
            .rev()
            .find(|item| {
                let b = item.visible_bounds();
                x >= b.x && y >= b.y && x < b.x + b.width && y < b.y + b.height
            })
            .map(|item| item.node)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UpdatePath {
    Noop,
    InstancePatch,
    PropertyPatch,
    RepaintChunks,
    Relayout,
    ReconcileStructure,
}

/// Capabilities are declared by the consumer of the frame, not by UI code.
#[derive(Clone, Copy, Debug, Default)]
pub struct BackendCapabilities {
    pub solid_color_patch: bool,
}

#[derive(Clone, Debug, Default)]
pub struct Commit {
    pub revision: u64,
    pub changed_nodes: usize,
    pub request_frame: bool,
    pub routes: Vec<(NodeId, UpdatePath)>,
    pub decisions: Vec<RouteDecision>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RouteReason {
    StableSolidQuadColor,
    BackendColorPatchUnavailable,
    PrimitivePaintChanged,
    LayoutInputsChanged,
    ChildOrderChanged,
    SharedPropertiesChanged,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RouteDecision {
    pub node: NodeId,
    pub path: UpdatePath,
    pub reason: RouteReason,
}
impl Commit {
    fn route(&mut self, node: NodeId, path: UpdatePath, reason: RouteReason) {
        self.routes.push((node, path));
        self.decisions.push(RouteDecision { node, path, reason });
    }
}

#[derive(Clone, Debug)]
pub enum Patch {
    Color(NodeId, u32),
    Style(NodeId, Box<Style>),
    Translation(NodeId, [f32; 2]),
    Reorder(NodeId, Vec<NodeId>),
    Clip(NodeId, Option<Rect>),
    Scroll(NodeId, [f32; 2]),
    Opacity(NodeId, f32),
    Measure(NodeId, Option<MeasureSnapshot>),
    Decoration(NodeId, QuadDecoration),
}

impl Patch {
    fn node(&self) -> NodeId {
        match self {
            Self::Color(id, _)
            | Self::Style(id, _)
            | Self::Translation(id, _)
            | Self::Reorder(id, _)
            | Self::Clip(id, _)
            | Self::Scroll(id, _)
            | Self::Opacity(id, _)
            | Self::Measure(id, _)
            | Self::Decoration(id, _) => *id,
        }
    }
}

#[derive(Default)]
pub struct Transaction {
    patches: Vec<Patch>,
}

impl Transaction {
    pub fn color(&mut self, node: NodeId, rgba: u32) {
        self.patches.push(Patch::Color(node, rgba));
    }
    pub fn style(&mut self, node: NodeId, style: Style) {
        self.patches.push(Patch::Style(node, Box::new(style)));
    }
    pub fn translation(&mut self, node: NodeId, translation: [f32; 2]) {
        self.patches.push(Patch::Translation(node, translation));
    }
    /// Intersect this subtree with a rectangle in the node's border-box coordinates.
    pub fn clip(&mut self, node: NodeId, clip: Option<Rect>) {
        self.patches.push(Patch::Clip(node, clip));
    }
    /// Scroll descendants by a nonnegative offset, clamped to Taffy's content extent.
    pub fn scroll(&mut self, node: NodeId, offset: [f32; 2]) {
        self.patches.push(Patch::Scroll(node, offset));
    }
    /// Composite the complete subtree as an isolated group with opacity in 0..=1.
    pub fn opacity(&mut self, node: NodeId, opacity: f32) {
        self.patches.push(Patch::Opacity(node, opacity));
    }
    /// Replace an owned intrinsic input. Only leaf render nodes accept measures.
    pub fn measure(&mut self, node: NodeId, snapshot: Option<MeasureSnapshot>) {
        self.patches.push(Patch::Measure(node, snapshot));
    }
    pub fn decoration(&mut self, node: NodeId, value: QuadDecoration) {
        self.patches.push(Patch::Decoration(node, value));
    }
    /// Reorder the existing children of one parent, preserving their identity.
    pub fn reorder(&mut self, parent: NodeId, children: Vec<NodeId>) {
        self.patches.push(Patch::Reorder(parent, children));
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Error {
    InvalidNode,
    InvalidTranslation,
    InvalidViewport,
    InvalidChildren,
    CannotRemoveRoot,
    RevisionExhausted,
    Layout(String),
    InvalidFrame,
    StaleFrame,
    Gpu(String),
    InvalidClip,
    InvalidScroll,
    InvalidOpacity,
    InvalidSignal,
    InvalidBinding,
    BindingConflict,
    InvalidScope,
    DuplicateKey,
    ScopeInUse,
    InvalidMeasure,
    MeasuredNodeHasChildren,
    InvalidEnvironment,
    InvalidStyle,
    UnsupportedLayout,
    InvalidBreakpoint,
    UnsafeContainerQuery,
    InvalidPrimitive,
    FrameQueuePoisoned,
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "retained UI error: {self:?}")
    }
}
impl std::error::Error for Error {}
impl From<taffy::TaffyError> for Error {
    fn from(error: taffy::TaffyError) -> Self {
        Self::Layout(error.to_string())
    }
}

struct Node {
    layout: taffy::NodeId,
    children: Vec<NodeId>,
    parent: Option<NodeId>,
    style: Style,
    color: u32,
    translation: [f32; 2],
    chunk: Option<Arc<[DisplayCommand]>>,
    clip: Option<Rect>,
    scroll: [f32; 2],
    opacity: f32,
    measure: Option<MeasureSnapshot>,
    decoration: QuadDecoration,
}
struct Slot {
    generation: u64,
    node: Option<Node>,
}

/// One UI owner mutates the tree. FramePackets own everything they expose.
pub struct UiTree {
    identity: u64,
    slots: Vec<Slot>,
    free: Vec<usize>,
    layout: TaffyTree<MeasureSnapshot>,
    root: NodeId,
    revision: u64,
    dirty: bool,
    layout_dirty: bool,
    viewport: Option<[f32; 2]>,
    frame: Option<FramePacket>,
}

#[derive(Default)]
struct Pending {
    color: Option<u32>,
    style: Option<Box<Style>>,
    translation: Option<[f32; 2]>,
    children: Option<Vec<NodeId>>,
    clip: Option<Option<Rect>>,
    scroll: Option<[f32; 2]>,
    opacity: Option<f32>,
    measure: Option<Option<MeasureSnapshot>>,
    decoration: Option<QuadDecoration>,
}

impl UiTree {
    pub fn new(style: Style, rgba: u32) -> Result<Self, Error> {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        let identity = NEXT
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |n| n.checked_add(1))
            .map_err(|_| Error::RevisionExhausted)?;
        let mut layout = TaffyTree::new();
        layout.disable_rounding();
        let id = layout.new_leaf(style.clone())?;
        let root = NodeId {
            tree: identity,
            slot: 0,
            generation: 0,
        };
        Ok(Self {
            identity,
            slots: vec![Slot {
                generation: 0,
                node: Some(Node {
                    layout: id,
                    children: vec![],
                    parent: None,
                    style,
                    color: rgba,
                    translation: [0.0; 2],
                    chunk: None,
                    clip: None,
                    scroll: [0.0; 2],
                    opacity: 1.0,
                    measure: None,
                    decoration: QuadDecoration::default(),
                }),
            }],
            free: vec![],
            layout,
            root,
            revision: 0,
            dirty: true,
            layout_dirty: true,
            viewport: None,
            frame: None,
        })
    }

    pub fn root(&self) -> NodeId {
        self.root
    }
    /// Most recently published output, including its own committed revision.
    pub fn published_frame(&self) -> Option<&FramePacket> {
        self.frame.as_ref()
    }
    pub fn style(&self, id: NodeId) -> Result<&Style, Error> {
        Ok(&self.node(id)?.style)
    }

    fn node(&self, id: NodeId) -> Result<&Node, Error> {
        if id.tree != self.identity {
            return Err(Error::InvalidNode);
        }
        self.slots
            .get(id.slot)
            .filter(|slot| slot.generation == id.generation)
            .and_then(|slot| slot.node.as_ref())
            .ok_or(Error::InvalidNode)
    }
    fn node_mut(&mut self, id: NodeId) -> &mut Node {
        self.slots[id.slot].node.as_mut().expect("validated node")
    }
    fn advance(&mut self) -> Result<(), Error> {
        self.revision = self
            .revision
            .checked_add(1)
            .ok_or(Error::RevisionExhausted)?;
        self.dirty = true;
        Ok(())
    }

    pub fn append(&mut self, parent: NodeId, style: Style, rgba: u32) -> Result<NodeId, Error> {
        if self.node(parent)?.measure.is_some() {
            return Err(Error::MeasuredNodeHasChildren);
        }
        let parent_layout = self.node(parent)?.layout;
        let next = self
            .revision
            .checked_add(1)
            .ok_or(Error::RevisionExhausted)?;
        let layout = self.layout.new_leaf(style.clone())?;
        self.layout.add_child(parent_layout, layout)?;
        let slot = self.free.pop().unwrap_or(self.slots.len());
        if slot == self.slots.len() {
            self.slots.push(Slot {
                generation: 0,
                node: None,
            });
        }
        let id = NodeId {
            tree: self.identity,
            slot,
            generation: self.slots[slot].generation,
        };
        self.slots[slot].node = Some(Node {
            layout,
            children: vec![],
            parent: Some(parent),
            style,
            color: rgba,
            translation: [0.0; 2],
            chunk: None,
            clip: None,
            scroll: [0.0; 2],
            opacity: 1.0,
            measure: None,
            decoration: QuadDecoration::default(),
        });
        self.node_mut(parent).children.push(id);
        self.revision = next;
        self.dirty = true;
        self.layout_dirty = true;
        Ok(id)
    }

    pub fn remove(&mut self, id: NodeId) -> Result<(), Error> {
        let parent = self.node(id)?.parent.ok_or(Error::CannotRemoveRoot)?;
        self.advance()?;
        let mut order = vec![id];
        let mut i = 0;
        while i < order.len() {
            order.extend(self.node(order[i])?.children.clone());
            i += 1;
        }
        for removed in order.into_iter().rev() {
            let layout = self.node(removed)?.layout;
            self.layout.remove(layout)?;
            let slot = &mut self.slots[removed.slot];
            slot.node = None;
            if let Some(next) = slot.generation.checked_add(1) {
                slot.generation = next;
                self.free.push(removed.slot);
            }
        }
        self.node_mut(parent).children.retain(|child| *child != id);
        self.layout_dirty = true;
        Ok(())
    }

    /// Validate every patch before changing state, then compare final field values.
    /// Writing A→B→A in one transaction produces no UI work.
    pub fn commit(
        &mut self,
        mut transaction: Transaction,
        capabilities: BackendCapabilities,
    ) -> Result<Commit, Error> {
        // Stable sorting preserves last-writer semantics while avoiding a
        // separately allocated tree entry for every node in a large animation.
        transaction.patches.sort_by_key(Patch::node);
        let mut pending: Vec<(NodeId, Pending)> = Vec::new();
        for patch in transaction.patches {
            let id = patch.node();
            self.node(id)?;
            if pending.last().is_none_or(|(previous, _)| *previous != id) {
                pending.push((id, Pending::default()));
            }
            let entry = &mut pending.last_mut().expect("pending node").1;
            match patch {
                Patch::Color(_, color) => entry.color = Some(color),
                Patch::Style(_, style) => entry.style = Some(style),
                Patch::Translation(_, translation) => {
                    if translation.iter().any(|v| !v.is_finite()) {
                        return Err(Error::InvalidTranslation);
                    }
                    entry.translation = Some(translation);
                }
                Patch::Clip(_, clip) => {
                    if clip.is_some_and(|clip| !clip.valid()) {
                        return Err(Error::InvalidClip);
                    }
                    entry.clip = Some(clip);
                }
                Patch::Scroll(_, offset) => {
                    if offset
                        .iter()
                        .any(|value| !value.is_finite() || *value < 0.0)
                    {
                        return Err(Error::InvalidScroll);
                    }
                    entry.scroll = Some(offset);
                }
                Patch::Opacity(_, opacity) => {
                    if !opacity.is_finite() || !(0.0..=1.0).contains(&opacity) {
                        return Err(Error::InvalidOpacity);
                    }
                    entry.opacity = Some(opacity);
                }
                Patch::Measure(_, snapshot) => {
                    if let Some(snapshot) = &snapshot {
                        snapshot.validate()?;
                        if !self.node(id)?.children.is_empty() {
                            return Err(Error::MeasuredNodeHasChildren);
                        }
                    }
                    entry.measure = Some(snapshot);
                }
                Patch::Decoration(_, decoration) => {
                    decoration.validate()?;
                    entry.decoration = Some(decoration);
                }
                Patch::Reorder(_, children) => {
                    let mut previous = self.node(id)?.children.clone();
                    let mut proposed = children.clone();
                    previous.sort_unstable();
                    proposed.sort_unstable();
                    if previous != proposed {
                        return Err(Error::InvalidChildren);
                    }
                    entry.children = Some(children);
                }
            }
        }
        let mut changed = Vec::new();
        for (id, entry) in pending {
            let node = self.node(id)?;
            let style_changed = entry
                .style
                .as_ref()
                .is_some_and(|style| **style != node.style);
            let color_changed = entry.color.is_some_and(|color| color != node.color);
            let decoration_changed = entry
                .decoration
                .is_some_and(|value| value != node.decoration);
            let property_changed = entry
                .translation
                .is_some_and(|value| value != node.translation)
                || entry.clip.is_some_and(|value| value != node.clip)
                || entry.scroll.is_some_and(|value| value != node.scroll)
                || entry.opacity.is_some_and(|value| value != node.opacity);
            let structure_changed = entry
                .children
                .as_ref()
                .is_some_and(|children| *children != node.children);
            let measure_changed = entry
                .measure
                .as_ref()
                .is_some_and(|value| *value != node.measure);
            if style_changed
                || color_changed
                || property_changed
                || structure_changed
                || measure_changed
                || decoration_changed
            {
                changed.push((
                    id,
                    entry,
                    style_changed,
                    color_changed,
                    property_changed,
                    structure_changed,
                    measure_changed,
                    decoration_changed,
                ));
            }
        }
        if changed.is_empty() {
            return Ok(Commit {
                revision: self.revision,
                ..Default::default()
            });
        }
        self.advance()?;
        let mut result = Commit {
            revision: self.revision,
            changed_nodes: changed.len(),
            request_frame: true,
            routes: vec![],
            decisions: vec![],
        };
        for (
            id,
            entry,
            style_changed,
            color_changed,
            property_changed,
            structure_changed,
            measure_changed,
            decoration_changed,
        ) in changed
        {
            if structure_changed {
                let children = entry.children.expect("changed children");
                let layouts = children
                    .iter()
                    .map(|child| self.node(*child).map(|node| node.layout))
                    .collect::<Result<Vec<_>, _>>()?;
                self.layout.set_children(self.node(id)?.layout, &layouts)?;
                self.node_mut(id).children = children;
                self.layout_dirty = true;
                result.route(
                    id,
                    UpdatePath::ReconcileStructure,
                    RouteReason::ChildOrderChanged,
                );
            }
            if style_changed || measure_changed {
                let node = self.node(id)?;
                let style = entry
                    .style
                    .map(|style| *style)
                    .unwrap_or_else(|| node.style.clone());
                let snapshot = entry.measure.unwrap_or_else(|| node.measure.clone());
                let layout = node.layout;
                self.layout
                    .set_style(layout, measure::effective_style(&style, snapshot.as_ref()))?;
                if measure_changed {
                    self.layout.set_node_context(layout, snapshot.clone())?;
                }
                self.node_mut(id).style = style;
                self.node_mut(id).measure = snapshot;
                self.layout_dirty = true;
                result.route(id, UpdatePath::Relayout, RouteReason::LayoutInputsChanged);
            }
            if color_changed {
                let node = self.node_mut(id);
                node.color = entry.color.expect("changed color");
                if !capabilities.solid_color_patch || decoration_changed {
                    node.chunk = None;
                }
                result.route(
                    id,
                    if capabilities.solid_color_patch && !decoration_changed {
                        UpdatePath::InstancePatch
                    } else {
                        UpdatePath::RepaintChunks
                    },
                    if decoration_changed {
                        RouteReason::PrimitivePaintChanged
                    } else if capabilities.solid_color_patch {
                        RouteReason::StableSolidQuadColor
                    } else {
                        RouteReason::BackendColorPatchUnavailable
                    },
                );
            }
            if decoration_changed {
                let node = self.node_mut(id);
                node.decoration = entry.decoration.expect("changed decoration");
                node.chunk = None;
                result.route(
                    id,
                    UpdatePath::RepaintChunks,
                    RouteReason::PrimitivePaintChanged,
                );
            }
            if property_changed {
                let node = self.node_mut(id);
                if let Some(value) = entry.translation {
                    node.translation = value;
                }
                if let Some(value) = entry.clip {
                    node.clip = value;
                }
                if let Some(value) = entry.scroll {
                    node.scroll = value;
                }
                if let Some(value) = entry.opacity {
                    node.opacity = value;
                }
                result.route(
                    id,
                    UpdatePath::PropertyPatch,
                    RouteReason::SharedPropertiesChanged,
                );
            }
        }
        Ok(result)
    }

    pub fn frame(&mut self, viewport: [f32; 2]) -> Result<FramePacket, Error> {
        if viewport.iter().any(|n| !n.is_finite() || *n < 0.0) {
            return Err(Error::InvalidViewport);
        }
        let viewport_changed = self.viewport != Some(viewport);
        if viewport_changed {
            self.advance()?;
            self.layout_dirty = true;
            self.viewport = Some(viewport);
        }
        if !self.dirty {
            let mut cached = self.frame.as_ref().expect("prepared frame").clone();
            cached.layout_runs = 0;
            cached.recorded_chunks = 0;
            cached.layout_stats = LayoutStats::default();
            return Ok(cached);
        }
        let layout_runs = u64::from(self.layout_dirty);
        let mut layout_stats = LayoutStats::default();
        if self.layout_dirty {
            self.layout.compute_layout_with_measure(
                self.node(self.root)?.layout,
                Size {
                    width: AvailableSpace::Definite(viewport[0]),
                    height: AvailableSpace::Definite(viewport[1]),
                },
                |inputs, _, snapshot, style| {
                    measure::layout_leaf(inputs, snapshot.as_deref(), style, &mut layout_stats)
                },
            )?;
        }
        let mut items = Vec::new();
        let mut stack = vec![(
            self.root,
            [0.0, 0.0],
            Some(Rect {
                x: 0.0,
                y: 0.0,
                width: viewport[0],
                height: viewport[1],
            }),
            PropertyState::default(),
        )];
        let mut properties = PropertyTrees::default();
        let mut recorded_chunks = 0;
        while let Some((id, origin, inherited_clip, mut state)) = stack.pop() {
            let node = self.node(id)?;
            if node.style.display == taffy::Display::None {
                continue;
            }
            let layout = *self.layout.layout(node.layout)?;
            let x = origin[0] + layout.location.x + node.translation[0];
            let y = origin[1] + layout.location.y + node.translation[1];
            if node.translation != [0.0; 2] {
                properties.transforms.insert(
                    id,
                    PropertyNode {
                        parent: state.transform,
                        value: node.translation,
                    },
                );
                state.transform = Some(id);
            }
            let clip = node.clip.map(|clip| clip.translated([x, y]));
            if let Some(local_clip) = node.clip {
                properties.clips.insert(
                    id,
                    PropertyNode {
                        parent: state.clip,
                        value: local_clip,
                    },
                );
                state.clip = Some(id);
            }
            let clip = match (inherited_clip, clip) {
                (Some(a), Some(b)) => Some(a.intersect(b)),
                (a, b) => a.or(b),
            };
            if node.opacity != 1.0 {
                properties.effects.insert(
                    id,
                    PropertyNode {
                        parent: state.effect,
                        value: node.opacity,
                    },
                );
                state.effect = Some(id);
            }
            let paint_state = state;
            let scroll = [
                node.scroll[0].min(layout.scroll_width()),
                node.scroll[1].min(layout.scroll_height()),
            ];
            if node.scroll != [0.0; 2] {
                properties.scrolls.insert(
                    id,
                    PropertyNode {
                        parent: state.scroll,
                        value: scroll,
                    },
                );
                state.scroll = Some(id);
            }
            for child in node.children.iter().rev() {
                stack.push((*child, [x - scroll[0], y - scroll[1]], clip, state));
            }
            let node = self.node_mut(id);
            let commands = node
                .chunk
                .get_or_insert_with(|| {
                    recorded_chunks += 1;
                    Arc::from([if node.decoration == QuadDecoration::default() {
                        DisplayCommand::Fill(node.color)
                    } else {
                        DisplayCommand::DecoratedFill {
                            color: node.color,
                            decoration: node.decoration,
                        }
                    }])
                })
                .clone();
            items.push(PaintItem {
                node: id,
                bounds: Rect {
                    x,
                    y,
                    width: layout.size.width,
                    height: layout.size.height,
                },
                commands,
                fill_color: node.color,
                clip,
                properties: paint_state,
            });
        }
        let previous: BTreeMap<_, _> = self
            .frame
            .as_ref()
            .map(|frame| frame.items.iter().map(|item| (item.node, item)).collect())
            .unwrap_or_default();
        let current: BTreeMap<_, _> = items.iter().map(|item| (item.node, item)).collect();
        let updates = items
            .iter()
            .filter(|item| {
                previous.get(&item.node).is_none_or(|old| {
                    old.bounds != item.bounds
                        || old.commands != item.commands
                        || old.fill_color != item.fill_color
                        || old.clip != item.clip
                })
            })
            .cloned()
            .collect::<Vec<_>>();
        let removed = previous
            .keys()
            .filter(|id| !current.contains_key(id))
            .copied()
            .collect::<Vec<_>>();
        let order: Vec<_> = items.iter().map(|item| item.node).collect();
        let order_changed = self.frame.as_ref().is_none_or(|frame| {
            !frame
                .items
                .iter()
                .map(|item| item.node)
                .eq(order.iter().copied())
        });
        let damage = properties::damage(
            self.frame.as_ref(),
            &items,
            &properties,
            viewport_changed,
            order_changed,
        );
        let frame = FramePacket {
            tree: self.identity,
            base_revision: self.frame.as_ref().map(|frame| frame.revision),
            revision: self.revision,
            items: items.into(),
            layout_runs,
            recorded_chunks,
            layout_stats,
            updates: updates.into(),
            removed: removed.into(),
            order: order_changed.then(|| order.into()),
            properties: Arc::new(properties),
            damage,
        };
        self.layout_dirty = false;
        self.dirty = false;
        self.frame = Some(frame.clone());
        Ok(frame)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> (UiTree, NodeId) {
        let mut tree = UiTree::new(
            Style {
                size: Size {
                    width: percent(1.0),
                    height: percent(1.0),
                },
                ..Style::default()
            },
            0xff0000ff,
        )
        .unwrap();
        let child = tree
            .append(
                tree.root(),
                Style {
                    size: Size {
                        width: length(40.0),
                        height: length(20.0),
                    },
                    ..Style::default()
                },
                0x0000ffff,
            )
            .unwrap();
        (tree, child)
    }

    #[test]
    fn child_reorder_is_atomic_preserves_identity_and_reuses_paint() {
        let (mut tree, a) = fixture();
        let b = tree
            .append(tree.root(), tree.style(a).unwrap().clone(), 0x00ff00ff)
            .unwrap();
        let before = tree.frame([100.0, 100.0]).unwrap();
        let mut invalid = Transaction::default();
        invalid.color(a, 0);
        invalid.reorder(tree.root(), vec![a, a]);
        assert_eq!(
            tree.commit(invalid, BackendCapabilities::default())
                .unwrap_err(),
            Error::InvalidChildren
        );
        let mut tx = Transaction::default();
        tx.reorder(tree.root(), vec![b, a]);
        let commit = tree.commit(tx, BackendCapabilities::default()).unwrap();
        assert_eq!(
            commit.routes,
            vec![(tree.root(), UpdatePath::ReconcileStructure)]
        );
        let after = tree.frame([100.0, 100.0]).unwrap();
        assert_eq!((after.layout_runs, after.recorded_chunks), (1, 0));
        assert_eq!(after.items[1].node, b);
        assert_eq!(after.items[2].node, a);
        assert!(Arc::ptr_eq(
            &before.items[1].commands,
            &after.items[2].commands
        ));
        assert_eq!(after.items[2].commands[0], DisplayCommand::Fill(0x0000ffff));
    }

    #[test]
    fn final_values_skip_layout_recording_and_frame_requests() {
        let (mut tree, child) = fixture();
        let before = tree.frame([100.0, 100.0]).unwrap();
        let mut tx = Transaction::default();
        tx.color(child, 0xffffffff);
        tx.color(child, 0x0000ffff);
        tx.style(child, tree.style(child).unwrap().clone());
        let commit = tree.commit(tx, BackendCapabilities::default()).unwrap();
        assert!(!commit.request_frame);
        assert_eq!(commit.changed_nodes, 0);
        let after = tree.frame([100.0, 100.0]).unwrap();
        assert_eq!(before.revision, after.revision);
        assert!(Arc::ptr_eq(&before.items, &after.items));
        assert_eq!((after.layout_runs, after.recorded_chunks), (0, 0));
    }

    #[test]
    fn invalid_batch_preserves_all_earlier_values() {
        let (mut tree, child) = fixture();
        let foreign = fixture().1;
        let before = tree.frame([100.0, 100.0]).unwrap();
        let mut tx = Transaction::default();
        tx.color(child, 0x00ff00ff);
        tx.translation(foreign, [10.0, 0.0]);
        assert_eq!(
            tree.commit(tx, BackendCapabilities::default()).unwrap_err(),
            Error::InvalidNode
        );
        let after = tree.frame([100.0, 100.0]).unwrap();
        assert_eq!(before.revision, after.revision);
        assert!(Arc::ptr_eq(&before.items, &after.items));
    }

    #[test]
    fn color_and_translation_keep_independent_routes_and_share_other_chunks() {
        let (mut tree, child) = fixture();
        let before = tree.frame([100.0, 100.0]).unwrap();
        let mut tx = Transaction::default();
        tx.color(child, 0x00ff00ff);
        tx.translation(child, [50.0, 10.0]);
        let commit = tree.commit(tx, BackendCapabilities::default()).unwrap();
        assert_eq!(
            commit.routes,
            vec![
                (child, UpdatePath::RepaintChunks),
                (child, UpdatePath::PropertyPatch)
            ]
        );
        let after = tree.frame([100.0, 100.0]).unwrap();
        assert_eq!((after.layout_runs, after.recorded_chunks), (0, 1));
        assert!(Arc::ptr_eq(
            &before.items[0].commands,
            &after.items[0].commands
        ));
        assert_eq!(
            after.items[1].bounds,
            Rect {
                x: 50.0,
                y: 10.0,
                width: 40.0,
                height: 20.0
            }
        );
        assert_eq!(after.hit_test(55.0, 15.0), Some(child));
        assert_eq!(after.hit_test(5.0, 5.0), Some(tree.root()));
        assert_eq!(
            before.items[1].commands[0],
            DisplayCommand::Fill(0x0000ffff)
        );
    }

    #[test]
    fn parent_translation_reuses_commands_and_updates_descendant_hit_geometry() {
        let (mut tree, child) = fixture();
        let before = tree.frame([100.0, 100.0]).unwrap();
        let mut tx = Transaction::default();
        tx.translation(tree.root(), [20.0, 30.0]);
        tree.commit(tx, BackendCapabilities::default()).unwrap();
        let after = tree.frame([100.0, 100.0]).unwrap();
        assert_eq!((after.layout_runs, after.recorded_chunks), (0, 0));
        assert!(Arc::ptr_eq(
            &before.items[1].commands,
            &after.items[1].commands
        ));
        assert_eq!(after.hit_test(25.0, 35.0), Some(child));
    }

    #[test]
    fn resized_persistent_flex_matches_a_fresh_tree() {
        let (mut retained, child) = fixture();
        retained.frame([100.0, 100.0]).unwrap();
        let mut tx = Transaction::default();
        let mut style = retained.style(child).unwrap().clone();
        style.flex_grow = 1.0;
        tx.style(child, style.clone());
        retained.commit(tx, BackendCapabilities::default()).unwrap();
        for viewport in [[180.0, 80.0], [80.0, 100.0], [0.0, 0.0]] {
            let a = retained.frame(viewport).unwrap();
            let (mut fresh, fresh_child) = fixture();
            let mut tx = Transaction::default();
            tx.style(fresh_child, style.clone());
            fresh.commit(tx, BackendCapabilities::default()).unwrap();
            let b = fresh.frame(viewport).unwrap();
            assert_eq!(
                a.items.iter().map(|item| item.bounds).collect::<Vec<_>>(),
                b.items.iter().map(|item| item.bounds).collect::<Vec<_>>()
            );
            assert_eq!(a.items[1].node, child);
        }
    }

    #[test]
    fn removal_invalidates_descendants_and_reused_slots_preserve_old_snapshots() {
        let (mut tree, child) = fixture();
        let grandchild = tree.append(child, Style::default(), 0xffffffff).unwrap();
        let before = tree.frame([100.0, 100.0]).unwrap();
        tree.remove(child).unwrap();
        let replacement = tree
            .append(tree.root(), Style::default(), 0xff00ffff)
            .unwrap();
        assert_ne!(child, replacement);
        for old in [child, grandchild] {
            let mut tx = Transaction::default();
            tx.color(old, 0);
            assert_eq!(
                tree.commit(tx, BackendCapabilities::default()).unwrap_err(),
                Error::InvalidNode
            );
        }
        let after = tree.frame([100.0, 100.0]).unwrap();
        assert_eq!((before.items.len(), after.items.len()), (3, 2));
        assert_eq!(before.items[1].node, child);
        assert_eq!(tree.remove(tree.root()), Err(Error::CannotRemoveRoot));
    }

    #[test]
    fn invalid_property_does_not_commit_and_invalid_viewport_can_be_retried() {
        let (mut tree, child) = fixture();
        let mut tx = Transaction::default();
        tx.color(child, 0);
        tx.translation(child, [f32::NAN, 0.0]);
        assert_eq!(
            tree.commit(tx, BackendCapabilities::default()).unwrap_err(),
            Error::InvalidTranslation
        );
        assert_eq!(
            tree.frame([-1.0, 100.0]).unwrap_err(),
            Error::InvalidViewport
        );
        let frame = tree.frame([100.0, 100.0]).unwrap();
        assert_eq!(frame.items[1].commands[0], DisplayCommand::Fill(0x0000ffff));
    }
}
