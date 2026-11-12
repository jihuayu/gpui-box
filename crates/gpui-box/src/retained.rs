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
mod image;
pub mod local;
pub mod logical;
pub mod pacing;
pub use image::ImageResource;
mod dirty_nodes;
mod extraction;
#[cfg(test)]
mod extraction_tests;
mod layout_publication;
mod measure;
mod paint_store;
pub use paint_store::{FrameBuildPolicy, FrameBuildStats, HitTestStats, PaintItems, PaintUpdates};
#[cfg(all(feature = "retained-text", not(target_family = "wasm")))]
pub mod paragraph;
#[cfg(all(feature = "retained-text", not(target_family = "wasm")))]
pub mod text;
pub use measure::{CustomMeasureSnapshot, ImageIntrinsicSize, LayoutStats, MeasureSnapshot};
mod primitive;
mod properties;
pub use primitive::{BoxShadow, QuadDecoration, ShadowGeometry};
mod spatial;
pub use spatial::SpatialState;
mod edit;
pub mod reactive;
pub mod responsive;
pub use edit::{TreeEdit, TreeEditCommit};
mod structure;
pub use properties::{
    Damage, PropertyIter, PropertyMap, PropertyNode, PropertyState, PropertyTrees,
};
pub use structure::StructureTransaction;
#[cfg(all(feature = "retained-wgpu", not(target_family = "wasm")))]
pub mod wgpu;

/// Identity belongs to one tree and one lifetime of its arena slot.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
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
    pub shadows: Arc<[BoxShadow]>,
    /// Optional decoded image stretched into the content box above the quad.
    pub image: Option<ImageResource>,
    #[cfg(all(feature = "retained-text", not(target_family = "wasm")))]
    pub text: Option<text::TextPaint>,
    #[cfg(all(feature = "retained-text", not(target_family = "wasm")))]
    pub glyphs: Option<Arc<text::GlyphChunk>>,
    pub content_bounds: Rect,
    /// Border x/y followed by padding x/y; preserve resolved f32 addition order.
    pub content_offsets: [f32; 4],
    pub clip: Option<Rect>,
    pub properties: PropertyState,
    /// Local render placement, separate from resolved paint/hit geometry.
    pub spatial: SpatialState,
}

/// Owned output can be read after the producing tree has changed or been dropped.
#[derive(Clone, Debug)]
pub struct FramePacket {
    pub tree: u64,
    pub base_revision: Option<u64>,
    pub revision: u64,
    pub items: Arc<PaintItems>,
    pub layout_runs: u64,
    pub recorded_chunks: u64,
    pub layout_stats: LayoutStats,
    pub build_stats: FrameBuildStats,
    pub updates: Arc<PaintUpdates>,
    pub removed: Arc<[NodeId]>,
    pub order: Option<Arc<[NodeId]>>,
    pub properties: Arc<PropertyTrees>,
    pub damage: Damage,
}

impl FramePacket {
    pub fn hit_test(&self, x: f32, y: f32) -> Option<NodeId> {
        self.items.hit_test(x, y).0
    }
    pub fn hit_test_with_stats(&self, x: f32, y: f32) -> (Option<NodeId>, HitTestStats) {
        self.items.hit_test(x, y)
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
    ResourcePatch,
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
    RenderTopologyChanged,
    SharedPropertiesChanged,
    ResourcePixelsChanged,
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
    Shadows(NodeId, Arc<[BoxShadow]>),
    Image(NodeId, Option<ImageResource>),
    #[cfg(all(feature = "retained-text", not(target_family = "wasm")))]
    Text(NodeId, Option<text::TextPaint>),
}

impl Patch {
    fn node(&self) -> NodeId {
        #[cfg(all(feature = "retained-text", not(target_family = "wasm")))]
        if let Self::Text(id, _) = self {
            return *id;
        }
        match self {
            Self::Color(id, _)
            | Self::Style(id, _)
            | Self::Translation(id, _)
            | Self::Reorder(id, _)
            | Self::Clip(id, _)
            | Self::Scroll(id, _)
            | Self::Opacity(id, _)
            | Self::Measure(id, _)
            | Self::Decoration(id, _)
            | Self::Shadows(id, _)
            | Self::Image(id, _) => *id,
            #[cfg(all(feature = "retained-text", not(target_family = "wasm")))]
            Self::Text(_, _) => unreachable!(),
        }
    }
}

#[derive(Default)]
pub struct Transaction {
    patches: Vec<Patch>,
    topology_pending: bool,
}

impl Transaction {
    pub fn shadows(&mut self, node: NodeId, shadows: Vec<BoxShadow>) {
        self.patches.push(Patch::Shadows(node, shadows.into()));
    }
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
    /// Attach a decoded image to a leaf. The image owns its intrinsic size;
    /// explicit measure writes while an image is attached are rejected.
    pub fn image(&mut self, node: NodeId, image: Option<ImageResource>) {
        self.patches.push(Patch::Image(node, image));
    }
    #[cfg(all(feature = "retained-text", not(target_family = "wasm")))]
    pub fn text(&mut self, node: NodeId, text: Option<text::TextPaint>) {
        self.patches.push(Patch::Text(node, text));
    }
    /// Reorder the existing children of one parent, preserving their identity.
    pub fn reorder(&mut self, parent: NodeId, children: Vec<NodeId>) {
        self.topology_pending = true;
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
    ReadonlySignal,
    UndeclaredDependency,
    SignalInUse,
    InvalidEffect,
    InvalidUpdateQueue,
    UpdateQueueClosed,
    UpdateQueueFull,
    UpdateQueuePoisoned,
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
    InvalidImage,
    UnsupportedText,
    ParagraphCachePoisoned,
    InvalidTextResource,
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

#[derive(Clone)]
struct Node {
    layout: taffy::NodeId,
    published_layout: taffy::Layout,
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
    shadows: Arc<[BoxShadow]>,
    image: Option<ImageResource>,
    #[cfg(all(feature = "retained-text", not(target_family = "wasm")))]
    text: Option<text::TextPaint>,
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
    next_generation: u64,
    dirty: bool,
    layout_dirty: bool,
    full_extract_required: bool,
    layout_nodes: std::collections::HashMap<taffy::NodeId, NodeId>,
    layout_written: std::collections::BTreeSet<NodeId>,
    geometry_dirty: std::collections::BTreeSet<NodeId>,
    property_dirty: dirty_nodes::DirtyNodes,
    local_dirty: dirty_nodes::DirtyNodes,
    paint_subtree_ends: std::collections::HashMap<NodeId, usize>,
    paint_dirty: std::collections::BTreeSet<NodeId>,
    paint_indices: std::collections::HashMap<NodeId, usize>,
    frame_build_policy: FrameBuildPolicy,
    viewport: Option<[f32; 2]>,
    frame: Option<FramePacket>,
    local_frame: Option<local::LocalFrame>,
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
    shadows: Option<Arc<[BoxShadow]>>,
    image: Option<Option<ImageResource>>,
    #[cfg(all(feature = "retained-text", not(target_family = "wasm")))]
    text: Option<Option<text::TextPaint>>,
}

// Final field changes, prepared without touching layout or retained state.
type PropertyChange = (
    NodeId,
    Pending,
    bool,
    bool,
    bool,
    bool,
    bool,
    bool,
    bool,
    bool,
);

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
                    published_layout: taffy::Layout::new(),
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
                    shadows: Arc::from([]),
                    image: None,
                    #[cfg(all(feature = "retained-text", not(target_family = "wasm")))]
                    text: None,
                }),
            }],
            free: vec![],
            layout,
            root,
            revision: 0,
            next_generation: 1,
            dirty: true,
            layout_dirty: true,
            full_extract_required: true,
            layout_nodes: [(id, root)].into(),
            layout_written: Default::default(),
            geometry_dirty: Default::default(),
            property_dirty: Default::default(),
            local_dirty: Default::default(),
            paint_subtree_ends: Default::default(),
            paint_dirty: Default::default(),
            paint_indices: Default::default(),
            frame_build_policy: Default::default(),
            viewport: None,
            frame: None,
            local_frame: None,
        })
    }

    pub fn root(&self) -> NodeId {
        self.root
    }
    /// Most recently published output, including its own committed revision.
    pub fn published_frame(&self) -> Option<&FramePacket> {
        if let Some(local) = &self.local_frame
            && self
                .frame
                .as_ref()
                .is_some_and(|frame| local.revision() > frame.revision)
        {
            return Some(local.resolved_frame());
        }
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
        let generation = self.issue_node_generation()?;
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
            generation,
        };
        self.slots[slot].generation = generation;
        self.slots[slot].node = Some(Node {
            layout,
            published_layout: taffy::Layout::new(),
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
            shadows: Arc::from([]),
            image: None,
            #[cfg(all(feature = "retained-text", not(target_family = "wasm")))]
            text: None,
        });
        self.node_mut(parent).children.push(id);
        self.layout_nodes.insert(layout, id);
        self.revision = next;
        self.dirty = true;
        self.layout_dirty = true;
        self.full_extract_required = true;
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
            self.layout_nodes.remove(&layout);
            self.layout_written.remove(&removed);
            let slot = &mut self.slots[removed.slot];
            slot.node = None;
            if let Some(next) = slot.generation.checked_add(1) {
                slot.generation = next;
                self.free.push(removed.slot);
            }
        }
        self.node_mut(parent).children.retain(|child| *child != id);
        self.layout_dirty = true;
        self.full_extract_required = true;
        Ok(())
    }

    /// Validate every patch before changing state, then compare final field values.
    /// Writing A→B→A in one transaction produces no UI work.
    pub fn commit(
        &mut self,
        transaction: Transaction,
        capabilities: BackendCapabilities,
    ) -> Result<Commit, Error> {
        let changed = Self::prepare_properties(transaction, |id| self.node(id))?;
        if changed.is_empty() {
            return Ok(Commit {
                revision: self.revision,
                ..Default::default()
            });
        }
        self.advance()?;
        Ok(self.apply_properties(changed, capabilities))
    }
    fn issue_node_generation(&mut self) -> Result<u64, Error> {
        let generation = self.next_generation;
        self.next_generation = generation.checked_add(1).ok_or(Error::RevisionExhausted)?;
        Ok(generation)
    }
    fn prepare_properties<'a>(
        mut transaction: Transaction,
        node_for: impl Fn(NodeId) -> Result<&'a Node, Error>,
    ) -> Result<Vec<PropertyChange>, Error> {
        // Stable sorting preserves last-writer semantics while avoiding a
        // separately allocated tree entry for every node in a large animation.
        transaction.patches.sort_by_key(Patch::node);
        // Coalesce one node at a time. The complete validated change list is
        // still private until every node succeeds, but no second dense array
        // of wide Pending records has to grow and move before final comparison.
        let mut patches = transaction.patches.into_iter().peekable();
        let mut changed = Vec::new();
        while let Some(mut patch) = patches.next() {
            let id = patch.node();
            let node = node_for(id)?;
            let mut entry = Pending::default();
            loop {
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
                        }
                        entry.measure = Some(snapshot);
                    }
                    Patch::Decoration(_, decoration) => {
                        decoration.validate()?;
                        entry.decoration = Some(decoration);
                    }
                    Patch::Shadows(_, shadows) => {
                        for shadow in shadows.iter() {
                            shadow.validate()?;
                        }
                        entry.shadows = Some(shadows);
                    }
                    Patch::Image(_, image) => {
                        entry.image = Some(image);
                    }
                    #[cfg(all(feature = "retained-text", not(target_family = "wasm")))]
                    Patch::Text(_, text) => {
                        entry.text = Some(text);
                    }
                    Patch::Reorder(_, children) => {
                        let mut previous = node.children.clone();
                        let mut proposed = children.clone();
                        previous.sort_unstable();
                        proposed.sort_unstable();
                        if previous != proposed {
                            return Err(Error::InvalidChildren);
                        }
                        entry.children = Some(children);
                    }
                }
                if let Some(next) = patches.next_if(|next| next.node() == id) {
                    patch = next;
                } else {
                    break;
                }
            }
            let final_image = entry.image.as_ref().unwrap_or(&node.image);
            let explicit_measure = entry.measure.is_some();
            #[cfg(all(feature = "retained-text", not(target_family = "wasm")))]
            let text_changed = {
                let final_text = entry.text.as_ref().unwrap_or(&node.text);
                if final_text.is_some() && (final_image.is_some() || explicit_measure) {
                    return Err(Error::InvalidMeasure);
                }
                let changed = entry.text.as_ref().is_some_and(|text| *text != node.text);
                if changed && entry.measure.is_none() && final_image.is_none() {
                    entry.measure =
                        Some(final_text.as_ref().map(|text| {
                            MeasureSnapshot::Paragraph(text.resource.paragraph().clone())
                        }));
                }
                changed
            };
            #[cfg(not(all(feature = "retained-text", not(target_family = "wasm"))))]
            let text_changed = false;
            if final_image.is_some() && explicit_measure {
                return Err(Error::InvalidMeasure);
            }
            let image_changed = entry
                .image
                .as_ref()
                .is_some_and(|image| *image != node.image);
            if image_changed && !explicit_measure {
                entry.measure = Some(final_image.as_ref().map(ImageResource::measure));
                #[cfg(all(feature = "retained-text", not(target_family = "wasm")))]
                if let Some(text) = entry.text.as_ref().unwrap_or(&node.text) {
                    entry.measure = Some(Some(MeasureSnapshot::Paragraph(
                        text.resource.paragraph().clone(),
                    )));
                }
            }
            if entry.measure.as_ref().unwrap_or(&node.measure).is_some()
                && !node.children.is_empty()
            {
                return Err(Error::MeasuredNodeHasChildren);
            }
            let style_changed = entry
                .style
                .as_ref()
                .is_some_and(|style| **style != node.style);
            let color_changed = entry.color.is_some_and(|color| color != node.color);
            let decoration_changed = entry
                .decoration
                .is_some_and(|value| value != node.decoration)
                || entry
                    .shadows
                    .as_ref()
                    .is_some_and(|value| *value != node.shadows);
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
                || image_changed
                || text_changed
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
                    image_changed,
                    text_changed,
                ));
            }
        }
        Ok(changed)
    }
    fn apply_properties(
        &mut self,
        changed: Vec<PropertyChange>,
        capabilities: BackendCapabilities,
    ) -> Commit {
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
            image_changed,
            text_changed,
        ) in changed
        {
            if color_changed || decoration_changed || image_changed || text_changed {
                self.paint_dirty.insert(id);
            }
            if property_changed {
                self.property_dirty.insert(id);
                // A local subscriber needs changes since its last snapshot.
                // Resolved-only users must not accumulate retired generations.
                if self.local_frame.is_some() {
                    self.local_dirty.insert(id);
                }
            }
            if structure_changed {
                let children = entry.children.expect("changed children");
                let layouts = children
                    .iter()
                    .map(|child| self.node(*child).expect("prepared child").layout)
                    .collect::<Vec<_>>();
                self.layout
                    .set_children(self.node(id).expect("prepared node").layout, &layouts)
                    .expect("prepared Taffy children");
                self.node_mut(id).children = children;
                self.layout_dirty = true;
                self.full_extract_required = true;
                result.route(
                    id,
                    UpdatePath::ReconcileStructure,
                    RouteReason::ChildOrderChanged,
                );
            }
            if style_changed || measure_changed {
                let node = self.node(id).expect("prepared node");
                let style = entry
                    .style
                    .map(|style| *style)
                    .unwrap_or_else(|| node.style.clone());
                let snapshot = entry.measure.unwrap_or_else(|| node.measure.clone());
                let layout = node.layout;
                self.full_extract_required |= node.style.display != style.display;
                self.layout
                    .set_style(layout, measure::effective_style(&style, snapshot.as_ref()))
                    .expect("prepared Taffy style");
                if measure_changed {
                    self.layout
                        .set_node_context(layout, snapshot.clone())
                        .expect("prepared Taffy context");
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
            if image_changed {
                self.node_mut(id).image = entry.image.expect("changed image");
                result.route(
                    id,
                    UpdatePath::ResourcePatch,
                    RouteReason::ResourcePixelsChanged,
                );
            }
            if text_changed {
                #[cfg(all(feature = "retained-text", not(target_family = "wasm")))]
                {
                    self.node_mut(id).text = entry.text.expect("changed text");
                }
                result.route(
                    id,
                    UpdatePath::ResourcePatch,
                    RouteReason::ResourcePixelsChanged,
                );
            }
            if decoration_changed {
                let node = self.node_mut(id);
                if let Some(value) = entry.decoration {
                    node.decoration = value;
                }
                if let Some(value) = entry.shadows {
                    node.shadows = value;
                }
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
        result
    }

    pub fn frame(&mut self, viewport: [f32; 2]) -> Result<FramePacket, Error> {
        if viewport.iter().any(|n| !n.is_finite() || *n < 0.0) {
            return Err(Error::InvalidViewport);
        }
        let viewport_changed = self.viewport != Some(viewport);
        if viewport_changed {
            self.advance()?;
            self.layout_dirty = true;
            self.full_extract_required = true;
            self.viewport = Some(viewport);
        }
        if !self.dirty {
            let mut cached = self.frame.as_ref().expect("prepared frame").clone();
            cached.layout_runs = 0;
            cached.recorded_chunks = 0;
            cached.layout_stats = LayoutStats::default();
            cached.build_stats = FrameBuildStats::default();
            return Ok(cached);
        }
        if self.frame_build_policy != FrameBuildPolicy::Full
            && !self.layout_dirty
            && self.frame.is_some()
        {
            return self.patch_paint_frame(0, LayoutStats::default());
        }
        let layout_runs = u64::from(self.layout_dirty);
        let mut layout_stats = LayoutStats::default();
        if self.layout_dirty {
            layout_stats = self.compute_layout(viewport)?;
        }
        if self.frame_build_policy != FrameBuildPolicy::Full
            && !self.full_extract_required
            && self.frame.is_some()
        {
            self.mark_layout_geometry()?;
            let frame = self.patch_paint_frame(layout_runs, layout_stats)?;
            self.finish_layout_publication();
            return Ok(frame);
        }
        let mut properties = properties::PropertyBuilder::default();
        let mut recorded_chunks = 0;
        let items = self.extract_subtree(
            self.root,
            [0.0; 2],
            Some(Rect {
                x: 0.0,
                y: 0.0,
                width: viewport[0],
                height: viewport[1],
            }),
            PropertyState::default(),
            &mut properties,
            &mut recorded_chunks,
        )?;
        let properties = properties.finish();
        let previous: BTreeMap<_, _> = self
            .frame
            .as_ref()
            .map(|frame| frame.items.iter().map(|item| (item.node, item)).collect())
            .unwrap_or_default();
        let current: BTreeMap<_, _> = items.iter().map(|item| (item.node, item)).collect();
        let update_indices = items
            .iter()
            .enumerate()
            .filter_map(|(index, item)| {
                previous
                    .get(&item.node)
                    .is_none_or(|old| {
                        old.bounds != item.bounds
                            || old.spatial != item.spatial
                            || old.shadows != item.shadows
                            || old.commands != item.commands
                            || old.fill_color != item.fill_color
                            || old.image != item.image
                            || !old.same_text(item)
                            || old.content_bounds != item.content_bounds
                            || old.content_offsets != item.content_offsets
                            || old.clip != item.clip
                    })
                    .then_some(index)
            })
            .collect::<Vec<_>>();
        let copying = self.frame_build_policy != FrameBuildPolicy::Selective;
        let updates = copying.then(|| {
            update_indices
                .iter()
                .map(|index| items[*index].clone())
                .collect::<Vec<_>>()
        });
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
        let items = Arc::new(PaintItems::from(items));
        let delta_cloned_items = updates.as_ref().map_or(0, Vec::len);
        let updates = if let Some(updates) = updates {
            Arc::new(PaintUpdates::from(updates))
        } else {
            PaintUpdates::indexed(items.clone(), update_indices)
        };
        let mut frame = FramePacket {
            tree: self.identity,
            base_revision: self.frame.as_ref().map(|frame| frame.revision),
            revision: self.revision,
            build_stats: FrameBuildStats {
                extracted_nodes: items.len(),
                full_rebuild: true,
                delta_cloned_items,
                ..Default::default()
            },
            items,
            layout_runs,
            recorded_chunks,
            layout_stats,
            updates,
            removed: removed.into(),
            order: order_changed.then(|| order.into()),
            properties: Arc::new(properties),
            damage,
        };
        frame.seal_updates(self.frame.as_ref());
        self.layout_dirty = false;
        self.full_extract_required = false;
        self.geometry_dirty.clear();
        self.finish_layout_publication();
        self.property_dirty.clear();
        self.local_dirty.clear();
        self.paint_dirty.clear();
        self.index_paint_items(&frame.items);
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
    fn grouped_preflight_rejects_overwritten_invalid_values_and_late_nodes_atomically() {
        let (mut tree, first) = fixture();
        let last = tree.append(tree.root(), Style::default(), 7).unwrap();
        let before = tree.frame([100.0, 100.0]).unwrap();
        for invalid in [f32::NAN, f32::INFINITY] {
            let mut tx = Transaction::default();
            // Interleave identities: stable grouping must retain each node's
            // writer order and validate even a subsequently overwritten value.
            tx.translation(last, [invalid, 0.0]);
            tx.color(first, 0x123456ff);
            tx.translation(last, [2.0, 3.0]);
            tx.color(first, 0xabcdef01);
            assert_eq!(
                tree.commit(tx, BackendCapabilities::default()).unwrap_err(),
                Error::InvalidTranslation
            );
            let unchanged = tree.frame([100.0, 100.0]).unwrap();
            assert_eq!(unchanged.revision, before.revision);
            assert!(Arc::ptr_eq(&unchanged.items, &before.items));
        }
        let mut tx = Transaction::default();
        tx.translation(last, [1.0, 1.0]);
        tx.color(first, 0x123456ff);
        tx.translation(last, [2.0, 3.0]);
        tx.color(first, 0xabcdef01);
        let commit = tree.commit(tx, BackendCapabilities::default()).unwrap();
        assert_eq!(commit.changed_nodes, 2);
        assert_eq!(commit.revision, before.revision + 1);
        let after = tree.frame([100.0, 100.0]).unwrap();
        assert_eq!(
            after
                .items
                .iter()
                .find(|item| item.node == first)
                .unwrap()
                .fill_color,
            0xabcdef01
        );
        assert_eq!(tree.node(last).unwrap().translation, [2.0, 3.0]);
        assert_eq!(
            before
                .items
                .iter()
                .find(|item| item.node == first)
                .unwrap()
                .fill_color,
            0x0000ffff
        );
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
