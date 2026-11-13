use super::*;
use gpui_wgpu::{QuadDraw, QuadLayer, QuadLayerId};

fn key(id: NodeId) -> QuadLayerId {
    [id.tree, id.slot as u64, id.generation]
}

fn push_instance(draws: &mut Vec<QuadDraw>, slot: u32) {
    if let Some(QuadDraw::Instances(range)) = draws.last_mut()
        && range.end == slot
    {
        range.end += 1;
    } else {
        draws.push(QuadDraw::Instances(slot..slot + 1));
    }
}

fn push_shadow(draws: &mut Vec<QuadDraw>, slot: u32) {
    if let Some(QuadDraw::Shadows(range)) = draws.last_mut()
        && range.end == slot
    {
        range.end += 1;
    } else {
        draws.push(QuadDraw::Shadows(slot..slot + 1));
    }
}

fn plan(
    frame: &FramePacket,
    instances: &BTreeMap<NodeId, Vec<InstanceHandle>>,
    shadow_instances: &BTreeMap<NodeId, Vec<InstanceHandle>>,
) -> Result<(Vec<QuadDraw>, Vec<QuadLayer>), Error> {
    let mut root = vec![];
    let mut stack: Vec<(NodeId, Vec<QuadDraw>)> = vec![];
    let mut layers = vec![];
    for item in frame.items.iter() {
        let mut path = vec![];
        let mut cursor = item.properties.effect;
        while let Some(id) = cursor {
            path.push(id);
            cursor = frame
                .properties
                .effects
                .get(&id)
                .ok_or(Error::InvalidFrame)?
                .parent;
            if path.len() > frame.properties.effects.len() {
                return Err(Error::InvalidFrame);
            }
        }
        path.reverse();
        let common = path
            .iter()
            .zip(&stack)
            .take_while(|(id, (open, _))| id == &open)
            .count();
        while stack.len() > common {
            let (id, draws) = stack.pop().unwrap();
            layers.push(QuadLayer {
                id: key(id),
                opacity: frame.properties.effects[&id].value,
                bounds: None,
                draws: draws.into(),
            });
        }
        for id in &path[common..] {
            let draws = stack.last_mut().map_or(&mut root, |(_, draws)| draws);
            draws.push(QuadDraw::Layer(key(*id)));
            stack.push((*id, vec![]));
        }
        let draws = stack.last_mut().map_or(&mut root, |(_, draws)| draws);
        for (index, _) in item
            .shadows
            .iter()
            .enumerate()
            .filter(|(_, shadow)| !shadow.inset)
        {
            let handle = shadow_instances
                .get(&item.node)
                .and_then(|handles| handles.get(index))
                .ok_or(Error::InvalidFrame)?;
            push_shadow(draws, handle.slot);
        }
        for handle in instances.get(&item.node).ok_or(Error::InvalidFrame)? {
            push_instance(draws, handle.slot);
        }
        for (index, _) in item
            .shadows
            .iter()
            .enumerate()
            .filter(|(_, shadow)| shadow.inset)
        {
            let handle = shadow_instances
                .get(&item.node)
                .and_then(|handles| handles.get(index))
                .ok_or(Error::InvalidFrame)?;
            push_shadow(draws, handle.slot);
        }
        if item.image.is_some() {
            draws.push(QuadDraw::Image(key(item.node)));
        }
        #[cfg(feature = "retained-text")]
        if item.text.is_some() {
            draws.push(QuadDraw::Glyphs(key(item.node)));
        }
    }
    while let Some((id, draws)) = stack.pop() {
        layers.push(QuadLayer {
            id: key(id),
            opacity: frame.properties.effects[&id].value,
            bounds: None,
            draws: draws.into(),
        });
    }
    Ok((root, layers))
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CompositionStats {
    pub rebuilt: bool,
    pub inspected_items: usize,
    pub planned_items: usize,
    pub layer_records: usize,
    pub bounds_items: usize,
    pub bounds_translated_nodes: usize,
}

struct CachedLayer {
    node: NodeId,
    draws: Arc<[QuadDraw]>,
}
struct Snapshot {
    tree: u64,
    items: Arc<PaintItems>,
    properties: Arc<PropertyTrees>,
    layers: Vec<CachedLayer>,
    bounds_pages: Vec<Arc<BTreeMap<NodeId, Rect>>>,
}
#[derive(Default)]
pub(super) struct CompositionCache {
    snapshot: Option<Snapshot>,
    bounds: BTreeMap<NodeId, Rect>,
}
pub(super) type CompositionPlan = (Option<Vec<QuadDraw>>, Vec<QuadLayer>, CompositionStats);
pub(super) fn same_topology(a: &PaintItem, b: &PaintItem) -> bool {
    let same = a.node == b.node
        && a.commands.len() == b.commands.len()
        && a.image.is_some() == b.image.is_some()
        && a.properties.effect == b.properties.effect
        && (Arc::ptr_eq(&a.shadows, &b.shadows)
            || (a.shadows.len() == b.shadows.len()
                && a.shadows
                    .iter()
                    .zip(b.shadows.iter())
                    .all(|(a, b)| a.inset == b.inset)));
    #[cfg(feature = "retained-text")]
    {
        same && a.text.is_some() == b.text.is_some()
    }
    #[cfg(not(feature = "retained-text"))]
    {
        same
    }
}
impl CompositionCache {
    pub(super) fn local_spatial_layers(&self) -> Vec<QuadLayer> {
        self.snapshot.as_ref().map_or_else(Vec::new, |snapshot| {
            snapshot
                .layers
                .iter()
                .map(|layer| QuadLayer {
                    id: key(layer.node),
                    opacity: snapshot.properties.effects[&layer.node].value,
                    bounds: None,
                    draws: layer.draws.clone(),
                })
                .collect()
        })
    }
    pub(super) fn bound_local_layers(
        &self, layers: &mut [QuadLayer], frame: &local::LocalFrame,
        ink: &local_ink::LocalInk, surface:[u32;2], scale:f32,
    ) {
        for layer in layers {
            let node=NodeId {tree:layer.id[0],slot:layer.id[1] as usize,generation:layer.id[2]};
            layer.bounds=Some(frame.subtree(node).and_then(|range|ink.query(range))
                .map_or([0;4],|ink| physical_bounds(ink,surface,scale)));
        }
    }
    pub(super) fn bound_layers(&self, layers: &mut [QuadLayer], surface: [u32; 2], scale: f32) {
        for layer in layers {
            let node = NodeId {
                tree: layer.id[0],
                slot: layer.id[1] as usize,
                generation: layer.id[2],
            };
            // A pixel fringe covers grayscale glyph snapping and edge AA.
            layer.bounds = Some(self.bounds.get(&node).map_or([0; 4], |ink| {
                let x = (ink.x * scale - 1.0).floor().clamp(0.0, surface[0] as f32) as u32;
                let y = (ink.y * scale - 1.0).floor().clamp(0.0, surface[1] as f32) as u32;
                let right = ((ink.x + ink.width) * scale + 1.0)
                    .ceil()
                    .clamp(0.0, surface[0] as f32) as u32;
                let bottom = ((ink.y + ink.height) * scale + 1.0)
                    .ceil()
                    .clamp(0.0, surface[1] as f32) as u32;
                if right <= x || bottom <= y {
                    [0; 4]
                } else {
                    [x, y, right - x, bottom - y]
                }
            }));
        }
    }
    pub(super) fn resolve(
        &mut self,
        frame: &FramePacket,
        instances: &BTreeMap<NodeId, Vec<InstanceHandle>>,
        shadow_instances: &BTreeMap<NodeId, Vec<InstanceHandle>>,
        force: bool,
    ) -> Result<CompositionPlan, Error> {
        let mut stats = CompositionStats::default();
        let reusable = self.snapshot.as_ref().filter(|old| {
            !force
                && old.tree == frame.tree
                && old.items.len() == frame.items.len()
                && old.items.page_count() == frame.items.page_count()
        });
        let reusable = reusable.is_some_and(|old| {
            let effects_same = old
                .properties
                .effects
                .shares_root(&frame.properties.effects)
                || (old.properties.effects.len() == frame.properties.effects.len()
                    && old
                        .properties
                        .effects
                        .iter()
                        .zip(&frame.properties.effects)
                        .all(|((a, p), (b, q))| a == b && p.parent == q.parent));
            effects_same
                && old
                    .items
                    .pages()
                    .iter()
                    .zip(frame.items.pages())
                    .all(|(a, b)| {
                        if Arc::ptr_eq(a, b) {
                            return true;
                        }
                        a.len() == b.len()
                            && a.iter().zip(b.iter()).all(|(a, b)| {
                                stats.inspected_items += 1;
                                same_topology(a, b)
                            })
                    })
        });
        // Cache nearest-effect ink unions per immutable paint page. Opacity does
        // not affect the union: hidden content must be ready when restored.
        let old = self.snapshot.as_ref().filter(|_| reusable);
        let bounds_pages = frame
            .items
            .pages()
            .iter()
            .enumerate()
            .map(|(index, page)| {
                if let Some(old) = old
                    && Arc::ptr_eq(page, &old.items.pages()[index])
                {
                    return old.bounds_pages[index].clone();
                }
                let mut bounds = BTreeMap::<NodeId, Rect>::new();
                if frame.properties.effects.is_empty() {
                    return Arc::new(bounds);
                }
                for item in page.iter() {
                    stats.bounds_items += 1;
                    if let (Some(effect), Some(ink)) = (item.properties.effect, item.ink_bounds()) {
                        bounds
                            .entry(effect)
                            .and_modify(|old| *old = old.union(ink))
                            .or_insert(ink);
                    }
                }
                Arc::new(bounds)
            })
            .collect::<Vec<_>>();
        let order;
        if reusable {
            order = None;
            let old = self.snapshot.as_mut().unwrap();
            old.items = frame.items.clone();
            old.properties = frame.properties.clone();
        } else {
            stats.rebuilt = true;
            stats.planned_items = frame.items.len();
            let (root, layers) = plan(frame, instances, shadow_instances)?;
            let mut cached = vec![];
            for layer in layers {
                let node = NodeId {
                    tree: layer.id[0],
                    slot: usize::try_from(layer.id[1]).map_err(|_| Error::InvalidFrame)?,
                    generation: layer.id[2],
                };
                cached.push(CachedLayer {
                    node,
                    draws: layer.draws,
                });
            }
            self.snapshot = Some(Snapshot {
                tree: frame.tree,
                items: frame.items.clone(),
                properties: frame.properties.clone(),
                layers: cached,
                bounds_pages: vec![],
            });
            order = Some(root);
        }
        self.snapshot.as_mut().unwrap().bounds_pages = bounds_pages;
        let snapshot = self.snapshot.as_ref().unwrap();
        let mut bounds = BTreeMap::<NodeId, Rect>::new();
        for page in &snapshot.bounds_pages {
            for (node, ink) in page.iter() {
                bounds
                    .entry(*node)
                    .and_modify(|old| *old = old.union(*ink))
                    .or_insert(*ink);
            }
        }
        // Layers are postorder; nested content contributes to every ancestor.
        for layer in &snapshot.layers {
            if let Some(ink) = bounds.get(&layer.node).copied()
                && let Some(parent) = frame.properties.effects[&layer.node].parent
            {
                bounds
                    .entry(parent)
                    .and_modify(|old| *old = old.union(ink))
                    .or_insert(ink);
            }
        }
        let layers = self
            .snapshot
            .as_ref()
            .unwrap()
            .layers
            .iter()
            .map(|layer| QuadLayer {
                id: key(layer.node),
                opacity: frame.properties.effects[&layer.node].value,
                bounds: None,
                draws: layer.draws.clone(),
            })
            .collect::<Vec<_>>();
        stats.layer_records = layers.len();
        self.bounds = bounds;
        Ok((order, layers, stats))
    }
}

fn physical_bounds(ink:Rect,surface:[u32;2],scale:f32)->[u32;4] {
    let x=(ink.x*scale-1.0).floor().clamp(0.0,surface[0] as f32) as u32;
    let y=(ink.y*scale-1.0).floor().clamp(0.0,surface[1] as f32) as u32;
    let right=((ink.x+ink.width)*scale+1.0).ceil().clamp(0.0,surface[0] as f32) as u32;
    let bottom=((ink.y+ink.height)*scale+1.0).ceil().clamp(0.0,surface[1] as f32) as u32;
    if right<=x || bottom<=y {[0;4]}else{[x,y,right-x,bottom-y]}
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bounded_layers_track_nested_clipped_ink_and_reuse_opacity_only_pages() {
        let absolute = |x, y, width, height| Style {
            position: taffy::Position::Absolute,
            inset: taffy::Rect {
                left: length(x),
                top: length(y),
                right: taffy::LengthPercentageAuto::auto(),
                bottom: taffy::LengthPercentageAuto::auto(),
            },
            size: taffy::Size {
                width: length(width),
                height: length(height),
            },
            ..Default::default()
        };
        let mut tree = UiTree::new(absolute(0.0, 0.0, 100.0, 100.0), 0).unwrap();
        let parent = tree
            .append(tree.root(), absolute(10.0, 20.0, 80.0, 70.0), 0)
            .unwrap();
        let child = tree
            .append(parent, absolute(5.0, 6.0, 40.0, 30.0), 0xff0000ff)
            .unwrap();
        let mut tx = Transaction::default();
        tx.opacity(parent, 0.5);
        tx.opacity(child, 0.5);
        tx.clip(
            parent,
            Some(Rect {
                x: 0.0,
                y: 0.0,
                width: 30.0,
                height: 30.0,
            }),
        );
        tree.commit(tx, BackendCapabilities::default()).unwrap();
        let first = tree.frame([100.0; 2]).unwrap();
        let instances = allocations(&first);
        let mut cache = CompositionCache::default();
        let (_, mut layers, stats) = cache
            .resolve(&first, &instances, &BTreeMap::new(), false)
            .unwrap();
        assert_eq!(stats.bounds_items, first.items.len());
        cache.bound_layers(&mut layers, [150; 2], 1.5);
        assert_eq!(layers.len(), 2);
        for layer in &layers {
            assert_eq!(layer.bounds, Some([21, 38, 40, 38]));
        }
        let mut tx = Transaction::default();
        tx.opacity(parent, 0.0);
        tree.commit(tx, BackendCapabilities::default()).unwrap();
        let hidden = tree.frame([100.0; 2]).unwrap();
        let (_, mut layers, stats) = cache
            .resolve(&hidden, &instances, &BTreeMap::new(), false)
            .unwrap();
        assert_eq!(stats.bounds_items, 0);
        cache.bound_layers(&mut layers, [150; 2], 1.5);
        assert!(
            layers
                .iter()
                .all(|layer| layer.bounds == Some([21, 38, 40, 38]))
        );
        let mut tx = Transaction::default();
        tx.color(child, 0);
        tree.commit(tx, BackendCapabilities::default()).unwrap();
        let empty = tree.frame([100.0; 2]).unwrap();
        let (_, mut layers, stats) = cache
            .resolve(&empty, &instances, &BTreeMap::new(), false)
            .unwrap();
        assert_eq!(stats.bounds_items, empty.items.len());
        cache.bound_layers(&mut layers, [150; 2], 1.5);
        assert!(layers.iter().all(|layer| layer.bounds == Some([0; 4])));
        // Old immutable packets retain their original geometry after publication.
        let (_, mut layers, _) = cache
            .resolve(&first, &instances, &BTreeMap::new(), true)
            .unwrap();
        cache.bound_layers(&mut layers, [20, 30], 1.0);
        assert!(
            layers
                .iter()
                .all(|layer| layer.bounds == Some([14, 25, 6, 5]))
        );
        cache.bound_layers(&mut layers, [10; 2], 1.0);
        assert!(layers.iter().all(|layer| layer.bounds == Some([0; 4])));
    }
    fn allocations(frame: &FramePacket) -> BTreeMap<NodeId, Vec<InstanceHandle>> {
        let mut arena = InstanceArena::default();
        frame
            .items
            .iter()
            .map(|item| {
                (
                    item.node,
                    item.commands
                        .iter()
                        .map(|command| arena.allocate(record(item, command, 1.0)))
                        .collect(),
                )
            })
            .collect()
    }
    fn assert_plan(
        frame: &FramePacket,
        instances: &BTreeMap<NodeId, Vec<InstanceHandle>>,
        cache: &mut CompositionCache,
        previous_order: &mut Vec<QuadDraw>,
        force: bool,
    ) -> CompositionStats {
        let (order, layers, stats) = cache
            .resolve(frame, instances, &BTreeMap::new(), force)
            .unwrap();
        if let Some(order) = order {
            *previous_order = order;
        }
        let (root, reference) = plan(frame, instances, &BTreeMap::new()).unwrap();
        assert_eq!(*previous_order, root);
        assert_eq!(layers.len(), reference.len());
        for (a, b) in layers.iter().zip(reference) {
            assert_eq!((a.id, a.opacity, &a.draws), (b.id, b.opacity, &b.draws));
        }
        stats
    }
    #[test]
    fn cached_order_and_layer_draws_match_replanning_across_material_property_and_topology_edits() {
        let mut tree = UiTree::new(Style::default(), 0).unwrap();
        let mut nodes = vec![];
        for _ in 0..200 {
            nodes.push(
                tree.append(tree.root(), Style::default(), 0xff0000ff)
                    .unwrap(),
            );
        }
        let image = ImageResource::rgba8([1, 1], vec![255; 4]).unwrap();
        let mut tx = Transaction::default();
        tx.image(nodes[3], Some(image.clone()));
        tx.opacity(tree.root(), 0.5);
        tree.commit(tx, BackendCapabilities::default()).unwrap();
        let first = tree.frame([100.0; 2]).unwrap();
        let instances = allocations(&first);
        let mut cache = CompositionCache::default();
        let mut order = vec![];
        assert!(assert_plan(&first, &instances, &mut cache, &mut order, false).rebuilt);
        let original_draws = cache.snapshot.as_ref().unwrap().layers[0].draws.clone();
        for round in 0..12 {
            let mut tx = Transaction::default();
            tx.color(
                nodes[70],
                if round % 2 == 0 {
                    0x00ff00ff
                } else {
                    0xff0000ff
                },
            );
            tx.translation(nodes[3], [(round % 2) as f32, 0.0]);
            tx.opacity(tree.root(), if round % 2 == 0 { 0.6 } else { 0.4 });
            tree.commit(
                tx,
                BackendCapabilities {
                    solid_color_patch: true,
                },
            )
            .unwrap();
            let frame = tree.frame([100.0; 2]).unwrap();
            let stats = assert_plan(&frame, &instances, &mut cache, &mut order, false);
            assert!(!stats.rebuilt && stats.planned_items == 0);
            assert!(Arc::ptr_eq(
                &original_draws,
                &cache.snapshot.as_ref().unwrap().layers[0].draws
            ));
        }
        // Adding/removing an image draw, an effect or its parentage must rebuild.
        for step in 0..4 {
            let mut tx = Transaction::default();
            match step {
                0 => tx.image(nodes[3], None),
                1 => tx.image(nodes[3], Some(image.clone())),
                2 => tx.opacity(nodes[3], 0.5),
                _ => tx.opacity(tree.root(), 1.0),
            }
            tree.commit(tx, BackendCapabilities::default()).unwrap();
            let frame = tree.frame([100.0; 2]).unwrap();
            assert!(assert_plan(&frame, &instances, &mut cache, &mut order, false).rebuilt);
        }
        let mut tx = Transaction::default();
        tx.reorder(tree.root(), nodes.iter().rev().copied().collect());
        tree.commit(tx, BackendCapabilities::default()).unwrap();
        let frame = tree.frame([100.0; 2]).unwrap();
        assert!(assert_plan(&frame, &instances, &mut cache, &mut order, false).rebuilt);
        assert!(assert_plan(&frame, &instances, &mut cache, &mut order, true).rebuilt);
        assert!(!assert_plan(&frame, &instances, &mut cache, &mut order, false).rebuilt);
    }
}
