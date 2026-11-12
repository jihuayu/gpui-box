//! Reuse validation only for private immutable pages held by the cache itself.
use super::*;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ValidationStats {
    pub full_scan: bool,
    pub validated_pages: usize,
    pub reused_pages: usize,
    pub validated_items: usize,
    /// Effect-reference checks may still walk the scene when the effect map changes.
    pub effect_reference_items: usize,
    pub validated_delta_items: usize,
    pub shared_delta_items: usize,
}

struct Snapshot {
    tree: u64,
    scale: f32,
    items: Arc<PaintItems>,
    properties: Arc<PropertyTrees>,
    indices: BTreeMap<NodeId, usize>,
    children: BTreeMap<NodeId, Vec<usize>>,
}

#[derive(Default)]
pub(super) struct ValidationCache {
    snapshot: Option<Snapshot>,
}

impl ValidationCache {
    pub(super) fn paint_index(&self, node: NodeId) -> Option<usize> {
        self.snapshot.as_ref()?.indices.get(&node).copied()
    }
    pub(super) fn check(
        &mut self,
        frame: &FramePacket,
        surface: [u32; 2],
        scale: f32,
        reuse: bool,
    ) -> Result<ValidationStats, Error> {
        validate_header(frame, surface, scale)?;
        let previous = self.snapshot.as_ref().filter(|old| {
            reuse
                && old.tree == frame.tree
                && old.scale == scale
                && old.items.len() == frame.items.len()
                && old.items.page_count() == frame.items.page_count()
        });
        let changed = previous.map(|old| {
            frame
                .items
                .pages()
                .iter()
                .zip(old.items.pages())
                .enumerate()
                .filter_map(|(i, (a, b))| (!Arc::ptr_eq(a, b)).then_some(i))
                .collect::<Vec<_>>()
        });
        // A changed page must preserve every node's exact position. Otherwise
        // rebuilding the node index is required (including duplicate detection).
        let stable = previous.zip(changed.as_ref()).filter(|(old, changed)| {
            changed.iter().all(|index| {
                let a = &frame.items.pages()[*index];
                let b = &old.items.pages()[*index];
                a.len() == b.len()
                    && a.iter()
                        .zip(b.iter())
                        .all(|(a, b)| a.node == b.node && a.spatial.parent == b.spatial.parent)
            })
        });
        if let Some((old, changed)) = stable {
            let mut stats = ValidationStats {
                validated_pages: changed.len(),
                reused_pages: frame.items.page_count() - changed.len(),
                ..Default::default()
            };
            for index in changed {
                for item in frame.items.pages()[*index].iter() {
                    validate_item(item, scale)?;
                    stats.validated_items += 1;
                }
            }
            let mut spatial_indices = BTreeSet::new();
            for page in changed {
                for item in frame.items.pages()[*page].iter() {
                    spatial_indices.insert(old.indices[&item.node]);
                    spatial_indices
                        .extend(old.children.get(&item.node).into_iter().flatten().copied());
                }
            }
            for index in spatial_indices {
                validate_spatial(frame, index, &old.indices)?;
            }
            if frame
                .properties
                .effects
                .shares_root(&old.properties.effects)
            {
                for index in changed {
                    for item in frame.items.pages()[*index].iter() {
                        validate_effect_reference(item, &frame.properties)?;
                        stats.effect_reference_items += 1;
                    }
                }
            } else {
                validate_effects(frame, &old.indices)?;
                stats.effect_reference_items = frame.items.len();
            }
            let (validated, shared) = validate_delta(frame, &old.indices)?;
            stats.validated_delta_items = validated;
            stats.shared_delta_items = shared;
            let cached = self.snapshot.as_mut().expect("matched snapshot");
            cached.items = frame.items.clone();
            cached.properties = frame.properties.clone();
            return Ok(stats);
        }
        let mut indices = BTreeMap::new();
        for (index, item) in frame.items.iter().enumerate() {
            validate_item(item, scale)?;
            if item.node.tree != frame.tree || indices.insert(item.node, index).is_some() {
                return Err(Error::InvalidFrame);
            }
        }
        let mut children = BTreeMap::<NodeId, Vec<usize>>::new();
        for (index, item) in frame.items.iter().enumerate() {
            validate_spatial(frame, index, &indices)?;
            if let Some(parent) = item.spatial.parent {
                children.entry(parent).or_default().push(index);
            }
        }
        validate_effects(frame, &indices)?;
        let (validated_delta_items, shared_delta_items) = validate_delta(frame, &indices)?;
        let stats = ValidationStats {
            full_scan: true,
            validated_pages: frame.items.page_count(),
            validated_items: frame.items.len(),
            effect_reference_items: frame.items.len(),
            validated_delta_items,
            shared_delta_items,
            ..Default::default()
        };
        self.snapshot = Some(Snapshot {
            tree: frame.tree,
            scale,
            items: frame.items.clone(),
            properties: frame.properties.clone(),
            indices,
            children,
        });
        Ok(stats)
    }
}

pub(super) fn validate(frame: &FramePacket, surface: [u32; 2], scale: f32) -> Result<(), Error> {
    ValidationCache::default()
        .check(frame, surface, scale, false)
        .map(|_| ())
}

fn validate_header(frame: &FramePacket, surface: [u32; 2], scale: f32) -> Result<(), Error> {
    if !scale.is_finite()
        || scale <= 0.0
        || surface
            .iter()
            .any(|size| *size == 0 || *size > i32::MAX as u32)
        || frame
            .base_revision
            .is_some_and(|base| base >= frame.revision)
        || frame.damage.bounds.is_some_and(|bounds| !bounds.valid())
    {
        return Err(Error::InvalidFrame);
    }
    Ok(())
}

fn validate_item(item: &PaintItem, scale: f32) -> Result<(), Error> {
    #[cfg(feature = "retained-text")]
    if item.text.is_some() != item.glyphs.is_some()
        || item
            .text
            .as_ref()
            .is_some_and(|text| text.resource.scale() != scale)
    {
        return Err(Error::InvalidFrame);
    }
    for shadow in item.shadows.iter() {
        let geometry = item.shadow_geometry(*shadow);
        if shadow.validate().is_err()
            || [geometry.bounds, geometry.ink].iter().any(|b| {
                !b.valid()
                    || [b.x, b.y, b.width, b.height, b.x + b.width, b.y + b.height]
                        .iter()
                        .any(|v| !(*v * scale).is_finite())
            })
            || !((shadow.blur_radius * scale).is_finite() && (geometry.radius * scale).is_finite())
        {
            return Err(Error::InvalidFrame);
        }
    }
    for command in item.commands.iter() {
        let (_, decoration) = command.quad();
        if decoration.validate().is_err()
            || [
                decoration.resolved(item.bounds).radius,
                decoration.resolved(item.bounds).border_width,
            ]
            .iter()
            .any(|v| !(*v * scale).is_finite())
        {
            return Err(Error::InvalidFrame);
        }
    }
    for b in [Some(item.bounds), Some(item.content_bounds), item.clip]
        .into_iter()
        .flatten()
    {
        if !b.valid()
            || [b.x, b.y, b.width, b.height, b.x + b.width, b.y + b.height]
                .iter()
                .any(|v| !(*v * scale).is_finite())
        {
            return Err(Error::InvalidFrame);
        }
    }
    Ok(())
}

fn validate_spatial(
    frame: &FramePacket,
    index: usize,
    indices: &BTreeMap<NodeId, usize>,
) -> Result<(), Error> {
    let item = &frame.items[index];
    let spatial = &item.spatial;
    if spatial
        .layout_origin
        .iter()
        .chain(&spatial.translation)
        .chain(&spatial.scroll)
        .any(|v| !v.is_finite())
        || spatial.scroll.iter().any(|v| *v < 0.0)
        || spatial
            .clip
            .into_iter()
            .chain(spatial.root_clip)
            .any(|b| !b.valid())
    {
        return Err(Error::InvalidFrame);
    }
    let parent = if let Some(parent) = spatial.parent {
        let parent_index = *indices.get(&parent).ok_or(Error::InvalidFrame)?;
        if parent_index >= index || spatial.root_clip.is_some() {
            return Err(Error::InvalidFrame);
        }
        Some(&frame.items[parent_index])
    } else {
        if index != 0 {
            return Err(Error::InvalidFrame);
        }
        None
    };
    let (origin, clip) = spatial.resolve(parent);
    if origin != [item.bounds.x, item.bounds.y] || clip != item.clip {
        return Err(Error::InvalidFrame);
    }
    Ok(())
}

fn validate_effect_reference(item: &PaintItem, properties: &PropertyTrees) -> Result<(), Error> {
    if item
        .properties
        .effect
        .is_some_and(|id| !properties.effects.contains_key(&id))
    {
        return Err(Error::InvalidFrame);
    }
    Ok(())
}

fn validate_effects(frame: &FramePacket, indices: &BTreeMap<NodeId, usize>) -> Result<(), Error> {
    for (id, effect) in &frame.properties.effects {
        if !indices.contains_key(id)
            || !effect.value.is_finite()
            || !(0.0..=1.0).contains(&effect.value)
        {
            return Err(Error::InvalidFrame);
        }
        let mut cursor = effect.parent;
        let mut depth = 0;
        while let Some(parent) = cursor {
            depth += 1;
            if depth > frame.properties.effects.len() {
                return Err(Error::InvalidFrame);
            }
            cursor = frame
                .properties
                .effects
                .get(&parent)
                .ok_or(Error::InvalidFrame)?
                .parent;
        }
    }
    for item in frame.items.iter() {
        validate_effect_reference(item, &frame.properties)?;
    }
    Ok(())
}

fn validate_delta(
    frame: &FramePacket,
    indices: &BTreeMap<NodeId, usize>,
) -> Result<(usize, usize), Error> {
    let mut updates = BTreeSet::new();
    let trusted_view = frame.updates.matches_items(&frame.items);
    if !trusted_view {
        for item in frame.updates.iter() {
            if !updates.insert(item.node)
                || indices.get(&item.node).is_none_or(|index| {
                    let value = &frame.items[*index];
                    value.spatial != item.spatial
                        || value.bounds != item.bounds
                        || value.commands != item.commands
                        || value.fill_color != item.fill_color
                        || value.shadows != item.shadows
                        || value.image != item.image
                        || !value.same_text(item)
                        || value.content_bounds != item.content_bounds
                        || value.clip != item.clip
                        || value.properties != item.properties
                })
            {
                return Err(Error::InvalidFrame);
            }
        }
    }
    if frame
        .removed
        .iter()
        .any(|id| id.tree != frame.tree || indices.contains_key(id))
        || frame.order.as_ref().is_some_and(|order| {
            !order
                .iter()
                .copied()
                .eq(frame.items.iter().map(|item| item.node))
        })
    {
        return Err(Error::InvalidFrame);
    }
    Ok(if trusted_view {
        (0, frame.updates.len())
    } else {
        (frame.updates.len(), 0)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn delta_views_require_exact_immutable_snapshot_identity_and_mutation_falls_back() {
        let (_, _, first) = fixture();
        let mut cache = ValidationCache::default();
        let stats = cache.check(&first, [100; 2], 1.0, true).unwrap();
        assert_eq!(
            (stats.validated_delta_items, stats.shared_delta_items),
            (0, first.updates.len())
        );
        let mut detached = first.clone();
        let color = detached.items[70].fill_color;
        Arc::make_mut(&mut detached.items)[70].fill_color = color;
        let stats = cache.check(&detached, [100; 2], 1.0, true).unwrap();
        assert_eq!(
            (stats.validated_delta_items, stats.shared_delta_items),
            (first.updates.len(), 0)
        );
        let mut malformed = first.clone();
        Arc::make_mut(&mut malformed.updates)[70].fill_color = 0;
        assert_eq!(
            cache.check(&malformed, [100; 2], 1.0, true),
            Err(Error::InvalidFrame)
        );
        let mut malformed = first.clone();
        Arc::make_mut(&mut malformed.items)[70].fill_color = 0;
        assert_eq!(
            cache.check(&malformed, [100; 2], 1.0, true),
            Err(Error::InvalidFrame)
        );
        let mut duplicate = first.clone();
        duplicate.updates = Arc::new(vec![first.items[70].clone(), first.items[70].clone()].into());
        assert_eq!(
            cache.check(&duplicate, [100; 2], 1.0, true),
            Err(Error::InvalidFrame)
        );
        assert!(
            cache
                .check(&first, [100; 2], 1.0, true)
                .unwrap()
                .shared_delta_items
                > 0
        );
    }
    #[test]
    fn spatial_validation_checks_edges_across_reused_pages_and_rejects_mismatched_deltas() {
        let (_, _, first) = fixture();
        let mut cache = ValidationCache::default();
        cache.check(&first, [100; 2], 1.0, true).unwrap();
        let mut invalid = first.clone();
        Arc::make_mut(&mut invalid.items)[0].spatial.scroll[0] = 1.0;
        assert_eq!(
            cache.check(&invalid, [100; 2], 1.0, true),
            Err(Error::InvalidFrame)
        );
        let mut invalid = first.clone();
        Arc::make_mut(&mut invalid.items)[150].spatial.parent = Some(first.items[150].node);
        assert_eq!(
            cache.check(&invalid, [100; 2], 1.0, true),
            Err(Error::InvalidFrame)
        );
        let mut invalid = first.clone();
        Arc::make_mut(&mut invalid.items)[150].spatial.translation[0] = 1.0;
        assert_eq!(
            cache.check(&invalid, [100; 2], 1.0, true),
            Err(Error::InvalidFrame)
        );
        let mut invalid = first.clone();
        let mut item = first.items[150].clone();
        item.spatial.scroll[0] = 1.0;
        invalid.updates = Arc::new([item].into());
        assert_eq!(
            cache.check(&invalid, [100; 2], 1.0, true),
            Err(Error::InvalidFrame)
        );
        assert_eq!(
            cache
                .check(&first, [100; 2], 1.0, true)
                .unwrap()
                .validated_items,
            0
        );
    }
    fn fixture() -> (UiTree, Vec<NodeId>, FramePacket) {
        let mut tree = UiTree::new(Style::default(), 0).unwrap();
        let mut nodes = vec![];
        for _ in 0..200 {
            nodes.push(
                tree.append(tree.root(), Style::default(), 0x112233ff)
                    .unwrap(),
            );
        }
        let frame = tree.frame([100.0; 2]).unwrap();
        (tree, nodes, frame)
    }
    #[test]
    fn reuse_is_page_owned_scale_specific_and_failed_checks_do_not_poison_cache() {
        let (mut tree, nodes, first) = fixture();
        let mut cache = ValidationCache::default();
        assert!(cache.check(&first, [100; 2], 1.0, true).unwrap().full_scan);
        let mut tx = Transaction::default();
        tx.color(nodes[70], 0xaabbccff);
        tree.commit(
            tx,
            BackendCapabilities {
                solid_color_patch: true,
            },
        )
        .unwrap();
        let frame = tree.frame([100.0; 2]).unwrap();
        let stats = cache.check(&frame, [100; 2], 1.0, true).unwrap();
        assert_eq!(
            (
                stats.validated_pages,
                stats.reused_pages,
                stats.validated_items
            ),
            (1, 3, 64)
        );
        assert!(!stats.full_scan);
        // Cache holds old immutable pages even after the producer drops its copy.
        let mut malformed = frame.clone();
        Arc::make_mut(&mut malformed.items)[3].bounds.x = f32::NAN;
        assert_eq!(
            cache.check(&malformed, [100; 2], 1.0, true),
            Err(Error::InvalidFrame)
        );
        assert_eq!(
            cache
                .check(&frame, [100; 2], 1.0, true)
                .unwrap()
                .validated_items,
            0
        );
        let mut malformed = frame.clone();
        Arc::make_mut(&mut malformed.updates)[0].shadows = Arc::from([BoxShadow {
            color: 0x000000ff,
            blur_radius: 2.0,
            ..Default::default()
        }]);
        assert_eq!(
            cache.check(&malformed, [100; 2], 1.0, true),
            Err(Error::InvalidFrame)
        );
        let mut malformed = frame.clone();
        Arc::make_mut(&mut malformed.items)[2].shadows = Arc::from([BoxShadow {
            blur_radius: f32::NAN,
            ..Default::default()
        }]);
        assert_eq!(
            cache.check(&malformed, [100; 2], 1.0, true),
            Err(Error::InvalidFrame)
        );
        assert_eq!(
            cache
                .check(&frame, [100; 2], 1.0, true)
                .unwrap()
                .validated_items,
            0
        );
        let mut malformed = frame.clone();
        Arc::make_mut(&mut malformed.updates)[0].properties.effect = Some(tree.root());
        assert_eq!(
            cache.check(&malformed, [100; 2], 1.0, true),
            Err(Error::InvalidFrame)
        );
        let mut malformed = frame.clone();
        Arc::make_mut(&mut malformed.items)[2].node = nodes[0];
        assert_eq!(
            cache.check(&malformed, [100; 2], 1.0, true),
            Err(Error::InvalidFrame)
        );
        let mut malformed = frame.clone();
        Arc::make_mut(&mut malformed.items)[2].properties.effect = Some(tree.root());
        assert_eq!(
            cache.check(&malformed, [100; 2], 1.0, true),
            Err(Error::InvalidFrame)
        );
        let mut malformed = frame.clone();
        Arc::make_mut(&mut malformed.properties).effects.insert(
            tree.root(),
            PropertyNode {
                parent: Some(tree.root()),
                value: 0.5,
            },
        );
        assert_eq!(
            cache.check(&malformed, [100; 2], 1.0, true),
            Err(Error::InvalidFrame)
        );
        assert!(cache.check(&frame, [100; 2], 2.0, true).unwrap().full_scan);
        assert!(cache.check(&frame, [100; 2], 2.0, false).unwrap().full_scan);
        assert_eq!(
            cache.check(&frame, [100; 2], f32::NAN, true),
            Err(Error::InvalidFrame)
        );
    }

    #[test]
    fn reorder_generations_new_tree_and_effect_removal_revalidate_required_state() {
        let (mut tree, nodes, first) = fixture();
        let mut cache = ValidationCache::default();
        cache.check(&first, [100; 2], 1.0, true).unwrap();
        let mut tx = Transaction::default();
        tx.opacity(nodes[70], 0.5);
        tree.commit(tx, BackendCapabilities::default()).unwrap();
        let effect = tree.frame([100.0; 2]).unwrap();
        let stats = cache.check(&effect, [100; 2], 1.0, true).unwrap();
        assert_eq!(stats.validated_items, 64);
        assert_eq!(stats.effect_reference_items, 201);
        let mut malformed = effect.clone();
        Arc::make_mut(&mut malformed.properties).effects.clear();
        assert_eq!(
            cache.check(&malformed, [100; 2], 1.0, true),
            Err(Error::InvalidFrame)
        );
        let mut tx = Transaction::default();
        tx.reorder(tree.root(), nodes.iter().rev().copied().collect());
        tree.commit(tx, BackendCapabilities::default()).unwrap();
        let reordered = tree.frame([100.0; 2]).unwrap();
        assert!(
            cache
                .check(&reordered, [100; 2], 1.0, true)
                .unwrap()
                .full_scan
        );
        tree.remove(nodes[0]).unwrap();
        let replacement = tree.append(tree.root(), Style::default(), 0).unwrap();
        assert_ne!(replacement, nodes[0]);
        let replaced = tree.frame([100.0; 2]).unwrap();
        assert!(
            cache
                .check(&replaced, [100; 2], 1.0, true)
                .unwrap()
                .full_scan
        );
        let (_, _, other) = fixture();
        assert!(cache.check(&other, [100; 2], 1.0, true).unwrap().full_scan);
    }

    #[test]
    fn independent_property_snapshots_reuse_only_owned_unchanged_effect_roots() {
        let (mut tree, nodes, _) = fixture();
        let mut tx = Transaction::default();
        tx.opacity(nodes[10], 0.5);
        tree.commit(tx, BackendCapabilities::default()).unwrap();
        let before = tree.frame([100.0; 2]).unwrap();
        let mut cache = ValidationCache::default();
        cache.check(&before, [100; 2], 1.0, true).unwrap();
        let mut tx = Transaction::default();
        tx.translation(nodes[70], [1.0, 1.0]);
        tree.commit(tx, BackendCapabilities::default()).unwrap();
        let moved = tree.frame([100.0; 2]).unwrap();
        assert!(!Arc::ptr_eq(&before.properties, &moved.properties));
        let stats = cache.check(&moved, [100; 2], 1.0, true).unwrap();
        assert_eq!(stats.effect_reference_items, 64);
        let mut malformed = moved.clone();
        Arc::make_mut(&mut malformed.properties)
            .effects
            .remove(&nodes[10]);
        assert_eq!(
            cache.check(&malformed, [100; 2], 1.0, true),
            Err(Error::InvalidFrame)
        );
        assert_eq!(
            cache
                .check(&moved, [100; 2], 1.0, true)
                .unwrap()
                .effect_reference_items,
            0
        );
    }
}
