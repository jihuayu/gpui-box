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

fn plan(
    frame: &FramePacket,
    instances: &BTreeMap<NodeId, Vec<InstanceHandle>>,
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
                draws: draws.into(),
            });
        }
        for id in &path[common..] {
            let draws = stack.last_mut().map_or(&mut root, |(_, draws)| draws);
            draws.push(QuadDraw::Layer(key(*id)));
            stack.push((*id, vec![]));
        }
        let draws = stack.last_mut().map_or(&mut root, |(_, draws)| draws);
        for handle in instances.get(&item.node).ok_or(Error::InvalidFrame)? {
            push_instance(draws, handle.slot);
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
}
#[derive(Default)]
pub(super) struct CompositionCache {
    snapshot: Option<Snapshot>,
}
pub(super) type CompositionPlan = (Option<Vec<QuadDraw>>, Vec<QuadLayer>, CompositionStats);
fn same_topology(a: &PaintItem, b: &PaintItem) -> bool {
    let same = a.node == b.node
        && a.commands.len() == b.commands.len()
        && a.image.is_some() == b.image.is_some()
        && a.properties.effect == b.properties.effect;
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
    pub(super) fn resolve(
        &mut self,
        frame: &FramePacket,
        instances: &BTreeMap<NodeId, Vec<InstanceHandle>>,
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
            let effects_same = Arc::ptr_eq(&old.properties, &frame.properties)
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
        let order;
        if reusable {
            order = None;
            let old = self.snapshot.as_mut().unwrap();
            old.items = frame.items.clone();
            old.properties = frame.properties.clone();
        } else {
            stats.rebuilt = true;
            stats.planned_items = frame.items.len();
            let (root, layers) = plan(frame, instances)?;
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
            });
            order = Some(root);
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
                draws: layer.draws.clone(),
            })
            .collect::<Vec<_>>();
        stats.layer_records = layers.len();
        Ok((order, layers, stats))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
        let (order, layers, stats) = cache.resolve(frame, instances, force).unwrap();
        if let Some(order) = order {
            *previous_order = order;
        }
        let (root, reference) = plan(frame, instances).unwrap();
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
