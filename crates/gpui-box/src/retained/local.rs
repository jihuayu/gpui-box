//! Immutable local scene snapshots. World geometry is explicitly resolved on
//! demand; a property edit never rewrites descendant paint payloads.
use super::*;
use std::collections::HashMap;
use std::sync::Weak;
#[cfg(test)]
mod tests;

#[derive(Clone, Debug, PartialEq)]
struct Placement {
    spatial: SpatialState,
    opacity: f32,
    scroll_property: bool,
}

const PLACEMENTS_PER_PAGE: usize = 64;
type PlacementPage = [Option<Placement>; PLACEMENTS_PER_PAGE];

/// Topology-local indices are immutable for this baseline. Copy touched pages
/// once per publication instead of allocating an ordered tree node per edit.
#[derive(Clone, Debug)]
struct Placements {
    pages: Vec<Option<Arc<PlacementPage>>>,
}
impl Placements {
    fn new(len: usize) -> Self {
        Self {
            pages: vec![None; len.div_ceil(PLACEMENTS_PER_PAGE)],
        }
    }
    fn get(&self, index: usize) -> Option<&Placement> {
        self.pages[index / PLACEMENTS_PER_PAGE]
            .as_ref()
            .and_then(|page| page[index % PLACEMENTS_PER_PAGE].as_ref())
    }
    fn set(&mut self, index: usize, value: Placement) {
        let page = self.pages[index / PLACEMENTS_PER_PAGE]
            .get_or_insert_with(|| Arc::new(std::array::from_fn(|_| None)));
        Arc::make_mut(page)[index % PLACEMENTS_PER_PAGE] = Some(value);
    }
}

#[derive(Debug)]
struct Topology {
    indices: HashMap<NodeId, usize>,
    parents: Vec<Option<usize>>,
    // GPU acceleration metadata; CPU geometry resolution uses parents.
    #[cfg_attr(not(any(test, feature = "retained-wgpu")), allow(dead_code))]
    anchors: Vec<usize>,
    #[cfg_attr(not(any(test, feature = "retained-wgpu")), allow(dead_code))]
    ends: Vec<usize>,
}

#[derive(Debug)]
struct Snapshot {
    template: FramePacket,
    topology: Arc<Topology>,
    placements: Placements,
    revision: u64,
    // Optional GPU consumer validates exact predecessor identity.
    #[cfg_attr(not(any(test, feature = "retained-wgpu")), allow(dead_code))]
    base: Weak<Snapshot>,
    changed: Vec<usize>,
    full_publication: bool,
    resolved: std::sync::OnceLock<FramePacket>,
}

/// Complete owned local scene, independent of the mutable UI owner. Its fields
/// are private: consumer incremental reuse is based on exact snapshot identity.
#[derive(Clone, Debug)]
pub struct LocalFrame(Arc<Snapshot>);

/// Internal spatial transport carries no material/resource references. Only
/// anchors need resolved world coordinates; ordinary entries remain local.
#[cfg(any(test, feature = "retained-wgpu"))]
pub(in crate::retained) struct SpatialUpdate {
    pub node: NodeId,
    pub spatial: SpatialState,
    pub anchor: Option<([f32; 2], Option<Rect>)>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LocalFrameStats {
    pub changed_nodes: usize,
    pub retained_items: usize,
    pub full_publication: bool,
}

#[derive(Clone, Copy)]
struct Context {
    origin: [f32; 2],
    clip: Option<Rect>,
    state: PropertyState,
}
struct IgnoreProperties;
impl properties::PropertyOutput for IgnoreProperties {
    fn record(
        &mut self,
        _: NodeId,
        _: Option<PropertyNode<[f32; 2]>>,
        _: Option<PropertyNode<Rect>>,
        _: Option<PropertyNode<[f32; 2]>>,
        _: Option<PropertyNode<f32>>,
    ) {
    }
}

impl LocalFrame {
    fn from_frame(frame: FramePacket) -> Self {
        let mut indices = HashMap::with_capacity(frame.items.len());
        let mut parents = Vec::with_capacity(frame.items.len());
        let mut depths = Vec::with_capacity(frame.items.len());
        let mut anchors = vec![];
        let mut ends = vec![frame.items.len(); frame.items.len()];
        let mut stack = vec![];
        for (i, item) in frame.items.iter().enumerate() {
            let parent = item.spatial.parent.map(|id| indices[&id]);
            while stack.last().copied() != parent {
                ends[stack.pop().expect("published preorder")] = i;
            }
            let depth = parent.map_or(0, |p| (depths[p] + 1) % 32);
            if depth == 0 {
                anchors.push(i);
            }
            depths.push(depth);
            parents.push(parent);
            indices.insert(item.node, i);
            stack.push(i);
        }
        let placements = Placements::new(indices.len());
        Self(Arc::new(Snapshot {
            revision: frame.revision,
            template: frame,
            topology: Arc::new(Topology {
                indices,
                parents,
                anchors,
                ends,
            }),
            placements,
            base: Weak::new(),
            changed: vec![],
            full_publication: true,
            resolved: std::sync::OnceLock::new(),
        }))
    }
    pub fn tree(&self) -> u64 {
        self.0.template.tree
    }
    pub fn revision(&self) -> u64 {
        self.0.revision
    }
    pub fn len(&self) -> usize {
        self.0.template.items.len()
    }
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
    pub fn stats(&self) -> LocalFrameStats {
        LocalFrameStats {
            changed_nodes: self.0.changed.len(),
            retained_items: self.len(),
            full_publication: self.0.full_publication,
        }
    }
    fn placement(&self, index: usize) -> Placement {
        let item = &self.0.template.items[index];
        self.0.placements.get(index).map_or_else(
            || Placement {
                spatial: item.spatial.clone(),
                opacity: self
                    .0
                    .template
                    .properties
                    .effects
                    .get(&item.node)
                    .map_or(1.0, |e| e.value),
                scroll_property: self.0.template.properties.scrolls.contains_key(&item.node),
            },
            Clone::clone,
        )
    }
    fn resolve_at(
        &self,
        index: usize,
        parent: Option<Context>,
        properties: &mut impl properties::PropertyOutput,
    ) -> (PaintItem, Context) {
        let mut item = self.0.template.items[index].clone();
        let placement = self.placement(index);
        item.spatial = placement.spatial;
        let spatial = &item.spatial;
        let parent = parent.unwrap_or(Context {
            origin: [0.0; 2],
            clip: spatial.root_clip,
            state: PropertyState::default(),
        });
        let mut state = parent.state;
        let x = parent.origin[0] + spatial.layout_origin[0] + spatial.translation[0];
        let y = parent.origin[1] + spatial.layout_origin[1] + spatial.translation[1];
        let transform = (spatial.translation != [0.0; 2]).then_some(PropertyNode {
            parent: state.transform,
            value: spatial.translation,
        });
        let clip = spatial.clip.map(|value| PropertyNode {
            parent: state.clip,
            value,
        });
        let effect = (placement.opacity != 1.0).then_some(PropertyNode {
            parent: state.effect,
            value: placement.opacity,
        });
        if transform.is_some() {
            state.transform = Some(item.node);
        }
        if clip.is_some() {
            state.clip = Some(item.node);
        }
        if effect.is_some() {
            state.effect = Some(item.node);
        }
        item.properties = state;
        let scroll = placement.scroll_property.then_some(PropertyNode {
            parent: state.scroll,
            value: spatial.scroll,
        });
        if scroll.is_some() {
            state.scroll = Some(item.node);
        }
        properties.record(item.node, transform, clip, scroll, effect);
        let own = spatial.clip.map(|clip| clip.translated([x, y]));
        item.clip = match (parent.clip, own) {
            (Some(a), Some(b)) => Some(a.intersect(b)),
            (a, b) => a.or(b),
        };
        item.bounds.x = x;
        item.bounds.y = y;
        item.content_bounds.x = (x + item.content_offsets[0]) + item.content_offsets[2];
        item.content_bounds.y = (y + item.content_offsets[1]) + item.content_offsets[3];
        let context = Context {
            origin: [x - spatial.scroll[0], y - spatial.scroll[1]],
            clip: item.clip,
            state,
        };
        (item, context)
    }
    /// Resolve only this node's ancestry, preserving the full extractor's f32
    /// operation order. Foreign/retired identities are absent from this snapshot.
    pub fn resolve_geometry(&self, node: NodeId) -> Option<PaintItem> {
        let mut cursor = Some(*self.0.topology.indices.get(&node)?);
        let mut path = vec![];
        while let Some(i) = cursor {
            path.push(i);
            cursor = self.0.topology.parents[i];
        }
        let mut context = None;
        let mut result = None;
        let mut ignored = IgnoreProperties;
        for i in path.into_iter().rev() {
            let (item, next) = self.resolve_at(i, context, &mut ignored);
            result = Some(item);
            context = Some(next);
        }
        result
    }
    /// Explicit O(N) compatibility/control materialization. The conservative
    /// full damage is intentional until a local-space damage index is available.
    pub fn resolve(&self) -> FramePacket {
        self.resolved_frame().clone()
    }
    pub(in crate::retained) fn resolved_frame(&self) -> &FramePacket {
        self.0.resolved.get_or_init(|| self.resolve_uncached())
    }
    fn resolve_uncached(&self) -> FramePacket {
        let mut items = Vec::with_capacity(self.len());
        let mut contexts: Vec<Context> = Vec::with_capacity(self.len());
        let mut properties = properties::PropertyBuilder::default();
        for i in 0..self.len() {
            let parent = self.0.topology.parents[i].map(|p| contexts[p]);
            let (item, context) = self.resolve_at(i, parent, &mut properties);
            items.push(item);
            contexts.push(context);
        }
        let mut frame = self.0.template.clone();
        frame.revision = self.revision();
        frame.layout_runs = 0;
        frame.recorded_chunks = 0;
        frame.layout_stats = LayoutStats::default();
        frame.build_stats = FrameBuildStats {
            extracted_nodes: self.len(),
            full_rebuild: true,
            ..Default::default()
        };
        frame.base_revision = None;
        frame.items = Arc::new(items.into());
        frame.properties = Arc::new(properties.finish());
        frame.updates = PaintUpdates::indexed(frame.items.clone(), (0..self.len()).collect());
        frame.damage = Damage {
            full: true,
            bounds: None,
        };
        frame.order = None;
        frame.removed = Arc::from([]);
        frame
    }
    /// Hit geometry resolves from this exact snapshot, never from the owner or
    /// an older resolved packet. Linear traversal is the current correctness path.
    pub fn hit_test(&self, x: f32, y: f32) -> Option<NodeId> {
        if !x.is_finite() || !y.is_finite() {
            return None;
        }
        let mut contexts: Vec<Context> = Vec::with_capacity(self.len());
        let mut hit = None;
        let mut ignored = IgnoreProperties;
        for i in 0..self.len() {
            let (item, context) = self.resolve_at(
                i,
                self.0.topology.parents[i].map(|p| contexts[p]),
                &mut ignored,
            );
            let b = item.visible_bounds();
            if x >= b.x && y >= b.y && x < b.x + b.width && y < b.y + b.height {
                hit = Some(item.node);
            }
            contexts.push(context);
        }
        hit
    }
    #[cfg(any(test, feature = "retained-wgpu"))]
    pub(in crate::retained) fn follows(&self, old: &Self) -> bool {
        self.0.base.ptr_eq(&Arc::downgrade(&old.0))
    }
    #[cfg(feature = "retained-wgpu")]
    pub(in crate::retained) fn same(&self, old: &Self) -> bool {
        Arc::ptr_eq(&self.0, &old.0)
    }
    #[cfg(any(test, feature = "retained-wgpu"))]
    pub(in crate::retained) fn spatial_updates(&self, old: &Self) -> Option<Vec<SpatialUpdate>> {
        if !self.follows(old) {
            return None;
        }
        let mut indices = self.0.changed.clone();
        for &i in &self.0.changed {
            if self.placement(i).opacity != old.placement(i).opacity {
                return None;
            }
            let anchors = &self.0.topology.anchors;
            let start = anchors.partition_point(|anchor| *anchor < i);
            indices.extend(
                anchors[start..]
                    .iter()
                    .take_while(|anchor| **anchor < self.0.topology.ends[i]),
            );
        }
        indices.sort_unstable();
        indices.dedup();
        let mut result = Vec::with_capacity(indices.len());
        for i in indices {
            // Non-anchor GPU records consume exclusively local placement.
            let node = self.0.template.items[i].node;
            let anchor = self.0.topology.anchors.binary_search(&i).is_ok().then(|| {
                let item = self.resolve_geometry(node).unwrap();
                ([item.bounds.x, item.bounds.y], item.clip)
            });
            result.push(SpatialUpdate {
                node,
                spatial: self.placement(i).spatial,
                anchor,
            });
        }
        Some(result)
    }
}

impl UiTree {
    /// Publish immutable local placement changes without descendant extraction.
    /// Layout/material/topology changes use the complete existing publisher.
    /// Calling `frame` remains an explicit resolved compatibility path.
    pub fn local_frame(&mut self, viewport: [f32; 2]) -> Result<LocalFrame, Error> {
        if viewport.iter().any(|v| !v.is_finite() || *v < 0.0) {
            return Err(Error::InvalidViewport);
        }
        let reusable = self
            .local_frame
            .as_ref()
            .filter(|old| {
                !self.layout_dirty
                    && !self.full_extract_required
                    && self.paint_dirty.is_empty()
                    && self.geometry_dirty.is_empty()
                    && self.viewport == Some(viewport)
                    && self
                        .frame
                        .as_ref()
                        .is_some_and(|frame| Arc::ptr_eq(&frame.items, &old.0.template.items))
            })
            .cloned();
        let Some(old) = reusable else {
            let frame = LocalFrame::from_frame(self.frame(viewport)?);
            self.local_frame = Some(frame.clone());
            self.local_dirty.clear();
            return Ok(frame);
        };
        if old.revision() == self.revision {
            return Ok(old);
        }
        let mut placements = old.0.placements.clone();
        let mut changed = vec![];
        for &id in self.local_dirty.iter() {
            let Some(&index) = old.0.topology.indices.get(&id) else {
                continue;
            };
            let node = self.node(id)?;
            let previous = old.placement(index);
            let mut value = previous.clone();
            value.spatial.translation = node.translation;
            value.spatial.clip = node.clip;
            value.spatial.scroll = [
                node.scroll[0].min(node.published_layout.scroll_width()),
                node.scroll[1].min(node.published_layout.scroll_height()),
            ];
            value.opacity = node.opacity;
            value.scroll_property = node.scroll != [0.0; 2];
            if value != previous {
                placements.set(index, value);
                changed.push(index);
            }
        }
        let frame = LocalFrame(Arc::new(Snapshot {
            template: old.0.template.clone(),
            topology: old.0.topology.clone(),
            placements,
            revision: self.revision,
            base: Arc::downgrade(&old.0),
            changed,
            full_publication: false,
            resolved: std::sync::OnceLock::new(),
        }));
        self.local_dirty.clear();
        self.local_frame = Some(frame.clone());
        Ok(frame)
    }
}
