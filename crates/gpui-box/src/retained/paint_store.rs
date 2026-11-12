//! Copy-on-write paint pages. Node indices remain stable until layout/order changes.
use super::*;
use std::ops::{Index, IndexMut};
use std::sync::OnceLock;
mod candidates;
mod hit_index;
mod updates;
pub use hit_index::HitTestStats;
pub use updates::PaintUpdates;

const PAGE_ITEMS: usize = 64;

/// Full mode provides a correctness/performance control using the same owning
/// tree and commands. It changes extraction work, not rendered behavior.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FrameBuildPolicy {
    #[default]
    Selective,
    /// Original candidate map and old-page/delta copying, for measured controls.
    SelectiveCopy,
    /// Retain complete subtree extraction as a same-runtime performance control.
    SelectiveReextract,
    Full,
}

/// Ordered immutable frame storage. Updating a page preserves all other page Arcs
/// and old packets. The small page directory is copied on each changed snapshot.
#[derive(Clone, Debug, Default)]
pub struct PaintItems {
    pages: Vec<Arc<Vec<PaintItem>>>,
    len: usize,
    hit_index: OnceLock<Arc<hit_index::HitNode>>,
}
impl From<Vec<PaintItem>> for PaintItems {
    fn from(items: Vec<PaintItem>) -> Self {
        let len = items.len();
        let mut pages = vec![];
        let mut iter = items.into_iter();
        loop {
            let page: Vec<_> = iter.by_ref().take(PAGE_ITEMS).collect();
            if page.is_empty() {
                break;
            }
            pages.push(Arc::new(page));
        }
        Self {
            pages,
            len,
            hit_index: OnceLock::new(),
        }
    }
}
impl PaintItems {
    pub fn len(&self) -> usize {
        self.len
    }
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }
    pub fn get(&self, index: usize) -> Option<&PaintItem> {
        self.pages.get(index / PAGE_ITEMS)?.get(index % PAGE_ITEMS)
    }
    pub fn iter(&self) -> impl DoubleEndedIterator<Item = &PaintItem> {
        self.pages.iter().flat_map(|page| page.iter())
    }
    pub fn shared_pages(&self, other: &Self) -> usize {
        self.pages
            .iter()
            .zip(&other.pages)
            .filter(|(a, b)| Arc::ptr_eq(a, b))
            .count()
    }
    pub fn page_count(&self) -> usize {
        self.pages.len()
    }
    #[cfg(all(feature = "retained-wgpu", not(target_family = "wasm")))]
    pub(super) fn pages(&self) -> &[Arc<Vec<PaintItem>>] {
        &self.pages
    }
    pub(super) fn patch(
        &self,
        updates: &[(usize, PaintItem)],
        stats: &mut FrameBuildStats,
    ) -> Self {
        let mut result = self.clone();
        result.hit_index = OnceLock::new();
        let mut dirty_pages = vec![];
        for (index, item) in updates {
            if dirty_pages.last() != Some(&(index / PAGE_ITEMS)) {
                dirty_pages.push(index / PAGE_ITEMS);
            }
            let page = &mut result.pages[index / PAGE_ITEMS];
            if Arc::strong_count(page) > 1 {
                stats.copied_pages += 1;
                stats.copied_items += page.len();
            }
            Arc::make_mut(page)[index % PAGE_ITEMS] = item.clone();
        }
        if let Some(index) = self.hit_index.get() {
            // Pages are ordered; internal indexed publication supplies sorted patches.
            dirty_pages.sort_unstable();
            dirty_pages.dedup();
            let patched = hit_index::HitNode::patch(index, &result.pages, &dirty_pages);
            let _ = result.hit_index.set(patched);
        }
        result
    }
}
impl PaintItems {
    fn patch_owned(&self, updates: Vec<(usize, PaintItem)>, stats: &mut FrameBuildStats) -> Self {
        debug_assert!(updates.windows(2).all(|p| p[0].0 < p[1].0));
        let mut result = self.clone();
        result.hit_index = OnceLock::new();
        let mut updates = updates.into_iter();
        let mut dirty_pages = vec![];
        while let Some((index, _)) = updates.as_slice().first() {
            let page = *index / PAGE_ITEMS;
            let count = updates
                .as_slice()
                .partition_point(|(i, _)| *i / PAGE_ITEMS == page);
            dirty_pages.push(page);
            stats.moved_items += count;
            if count == result.pages[page].len() {
                // Every element is supplied exactly once; no old payload is copied.
                result.pages[page] =
                    Arc::new(updates.by_ref().take(count).map(|(_, item)| item).collect());
                stats.replaced_pages += 1;
            } else {
                let values = &mut result.pages[page];
                stats.copied_pages += 1;
                stats.copied_items += values.len();
                let values = Arc::make_mut(values);
                for (index, item) in updates.by_ref().take(count) {
                    values[index % PAGE_ITEMS] = item;
                }
            }
        }
        if let Some(index) = self.hit_index.get() {
            let _ = result.hit_index.set(hit_index::HitNode::patch(
                index,
                &result.pages,
                &dirty_pages,
            ));
        }
        result
    }
}
impl Index<usize> for PaintItems {
    type Output = PaintItem;
    fn index(&self, index: usize) -> &Self::Output {
        self.get(index).expect("paint item index")
    }
}
impl IndexMut<usize> for PaintItems {
    fn index_mut(&mut self, index: usize) -> &mut Self::Output {
        self.hit_index.take();
        &mut Arc::make_mut(&mut self.pages[index / PAGE_ITEMS])[index % PAGE_ITEMS]
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FrameBuildStats {
    /// Render nodes extracted, excluding copy-on-write page item clones.
    pub extracted_nodes: usize,
    pub copied_pages: usize,
    pub copied_items: usize,
    /// Complete new pages moved directly from extraction, without cloning old items.
    pub replaced_pages: usize,
    pub moved_items: usize,
    /// Payload copies into independently owned delta lists (view deltas copy none).
    pub delta_cloned_items: usize,
    pub full_rebuild: bool,
}

impl UiTree {
    pub fn set_frame_build_policy(&mut self, policy: FrameBuildPolicy) {
        self.frame_build_policy = policy;
    }
    /// Material changes extract individual visible nodes. Visual property
    /// changes extract disjoint affected subtrees from persistent layout.
    pub(super) fn patch_paint_frame(
        &mut self,
        layout_runs: u64,
        layout_stats: LayoutStats,
    ) -> Result<FramePacket, Error> {
        let before = self.frame.as_ref().expect("published geometry").clone();
        let copying = self.frame_build_policy == FrameBuildPolicy::SelectiveCopy;
        let mut update_indices = vec![];
        let mut updates = vec![];
        let mut damage: Option<Rect> = None;
        let mut stats = FrameBuildStats::default();
        let mut recorded_chunks = 0;
        let mut ranges = self
            .property_dirty
            .iter()
            .filter_map(|id| {
                Some((
                    *self.paint_indices.get(id)?,
                    *self.paint_subtree_ends.get(id)?,
                    *id,
                ))
            })
            .collect::<Vec<_>>();
        ranges.sort_by_key(|(start, _, _)| *start);
        // Visible preorder makes descendants contiguous. Keep only the outer
        // dirty roots so simultaneous ancestor/descendant writes extract once.
        let mut disjoint = vec![];
        let mut covered_until = 0;
        for range in ranges {
            if range.0 >= covered_until {
                covered_until = range.1;
                disjoint.push(range);
            }
        }
        let mut candidates =
            candidates::Candidates::new(copying, disjoint.iter().map(|(a, b, _)| *a..*b).collect());
        let mut properties = if disjoint.is_empty() && self.geometry_dirty.is_empty() {
            before.properties.clone()
        } else {
            Arc::new((*before.properties).clone())
        };
        let mut property_edits = properties::PropertyEdits::default();
        for &(start, end, id) in &disjoint {
            let (origin, clip, state) = self.subtree_context(id, &before)?;
            let props = Arc::get_mut(&mut properties).expect("private property snapshot");
            let extracted = if layout_runs == 0
                && self.paint_dirty.is_empty()
                && self.frame_build_policy != FrameBuildPolicy::SelectiveReextract
                && !copying
            {
                self.resolve_property_subtree(
                    &before,
                    start..end,
                    origin,
                    clip,
                    state,
                    &mut property_edits,
                )?
            } else {
                self.extract_subtree(id, origin, clip, state, props, &mut recorded_chunks)?
            };
            debug_assert_eq!(extracted.len(), end - start);
            stats.extracted_nodes += extracted.len();
            for (index, item) in (start..end).zip(extracted) {
                debug_assert_eq!(item.node, before.items[index].node);
                candidates.insert(index, item, false);
            }
        }
        if let Some(props) = Arc::get_mut(&mut properties) {
            property_edits.apply(props);
        }
        for id in self.geometry_dirty.iter().copied().collect::<Vec<_>>() {
            let Some(index) = self.paint_indices.get(&id).copied() else {
                continue;
            };
            if candidates.contains(index) {
                continue;
            }
            let context = self.subtree_context(id, &before)?;
            let props = Arc::get_mut(&mut properties).expect("private geometry property snapshot");
            let item = self.extract_node(id, context, props, &mut recorded_chunks)?;
            stats.extracted_nodes += 1;
            candidates.insert(index, item, true);
        }
        for id in self.paint_dirty.iter().copied().collect::<Vec<_>>() {
            let Some(index) = self.paint_indices.get(&id).copied() else {
                continue;
            };
            if candidates.contains(index) {
                continue;
            }
            stats.extracted_nodes += 1;
            let mut item = before.items[index].clone();
            let node = self.node_mut(id);
            item.commands = node
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
            item.fill_color = node.color;
            item.shadows = node.shadows.clone();
            item.image = node.image.clone();
            #[cfg(all(feature = "retained-text", not(target_family = "wasm")))]
            {
                item.text = node.text.clone();
                item.glyphs = node
                    .text
                    .as_ref()
                    .map(|text| text.resource.chunk(item.content_bounds.width))
                    .transpose()?;
            }
            // paint_dirty is already unique; only geometry points need guards
            // against the subsequent paint pass.
            candidates.insert(index, item, false);
        }
        if properties
            .transforms
            .shares_root(&before.properties.transforms)
            && properties.clips.shares_root(&before.properties.clips)
            && properties.scrolls.shares_root(&before.properties.scrolls)
            && properties.effects.shares_root(&before.properties.effects)
        {
            properties = before.properties.clone();
        }
        let mut patches = vec![];
        let same_effect_tree = properties.effects.shares_root(&before.properties.effects);
        for (index, item) in candidates.into_sorted() {
            let old = &before.items[index];
            let paint_changed = old.bounds != item.bounds
                || old.content_bounds != item.content_bounds
                || old.content_offsets != item.content_offsets
                || old.clip != item.clip
                || old.commands != item.commands
                || old.fill_color != item.fill_color
                || old.shadows != item.shadows
                || old.image != item.image
                || !old.same_text(&item);
            let effects_changed = !same_effect_tree
                && !properties::same_effects(
                    old.properties.effect,
                    item.properties.effect,
                    &before.properties,
                    &properties,
                );
            if paint_changed || effects_changed {
                for ink in [old.ink_bounds(), item.ink_bounds()].into_iter().flatten() {
                    damage = Some(damage.map_or(ink, |previous| previous.union(ink)));
                }
            }
            let spatial_changed = old.spatial != item.spatial;
            if paint_changed || spatial_changed {
                update_indices.push(index);
                if copying {
                    updates.push(item.clone());
                    stats.delta_cloned_items += 1;
                }
            }
            if paint_changed || spatial_changed || old.properties != item.properties {
                patches.push((index, item));
            }
        }
        let items = if patches.is_empty() {
            before.items.clone()
        } else {
            Arc::new(if copying {
                before.items.patch(&patches, &mut stats)
            } else {
                before.items.patch_owned(patches, &mut stats)
            })
        };
        let mut frame = FramePacket {
            tree: self.identity,
            base_revision: Some(before.revision),
            revision: self.revision,
            items: items.clone(),
            layout_runs,
            recorded_chunks,
            layout_stats,
            build_stats: stats,
            updates: if copying {
                Arc::new(updates.into())
            } else {
                PaintUpdates::indexed(items.clone(), update_indices)
            },
            removed: Arc::from([]),
            order: None,
            properties,
            damage: Damage {
                full: false,
                bounds: damage,
            },
        };
        frame.seal_updates(Some(&before));
        self.paint_dirty.clear();
        self.property_dirty.clear();
        self.local_dirty.clear();
        self.geometry_dirty.clear();
        self.dirty = false;
        self.frame = Some(frame.clone());
        Ok(frame)
    }

    fn subtree_context(
        &self,
        id: NodeId,
        before: &FramePacket,
    ) -> Result<([f32; 2], Option<Rect>, PropertyState), Error> {
        let Some(parent) = self.node(id)?.parent else {
            let viewport = self.viewport.expect("published viewport");
            return Ok((
                [0.0; 2],
                Some(Rect {
                    x: 0.0,
                    y: 0.0,
                    width: viewport[0],
                    height: viewport[1],
                }),
                PropertyState::default(),
            ));
        };
        let item = &before.items[self.paint_indices[&parent]];
        let node = self.node(parent)?;
        let layout = self.layout.layout(node.layout)?;
        let mut state = item.properties;
        if node.scroll != [0.0; 2] {
            state.scroll = Some(parent);
        }
        Ok((
            [
                item.bounds.x - node.scroll[0].min(layout.scroll_width()),
                item.bounds.y - node.scroll[1].min(layout.scroll_height()),
            ],
            item.clip,
            state,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn owned_pages_copy_control_and_full_publication_preserve_snapshots_hits_and_deltas() {
        let (mut owned, a) = fixture();
        let (mut copying, b) = fixture();
        let (mut full, c) = fixture();
        copying.set_frame_build_policy(FrameBuildPolicy::SelectiveCopy);
        full.set_frame_build_policy(FrameBuildPolicy::Full);
        let image = ImageResource::rgba8([1, 1], vec![255, 128, 0, 255]).unwrap();
        let mut trees = [(&mut owned, a), (&mut copying, b), (&mut full, c)];
        let mut initial = vec![];
        for (tree, _) in &mut trees {
            let first = tree.frame([1000.0; 2]).unwrap();
            first.hit_test(5.0, 5.0);
            initial.push(first);
        }
        let mut saved = vec![];
        for round in 0..3 {
            let mut frames = vec![];
            for (tree, nodes) in &mut trees {
                let mut tx = Transaction::default();
                match round {
                    0 => {
                        tx.translation(tree.root(), [8.25, 3.5]);
                        tx.image(nodes[80], Some(image.clone()));
                        tx.shadows(
                            nodes[81],
                            vec![BoxShadow {
                                color: 0x55cc99aa,
                                blur_radius: 2.0,
                                ..Default::default()
                            }],
                        );
                    }
                    1 => {
                        let mut style = tree.style(nodes[70]).unwrap().clone();
                        style.size.width = length(30.0);
                        tx.style(nodes[70], style);
                        tx.translation(nodes[0], [2.5, -1.0]);
                        tx.clip(
                            nodes[128],
                            Some(Rect {
                                x: 1.0,
                                y: 1.0,
                                width: 8.0,
                                height: 8.0,
                            }),
                        );
                        tx.color(nodes[129], 0x884422ff);
                    }
                    _ => {
                        for node in &nodes[62..130] {
                            tx.color(*node, 0x00ff0080);
                        }
                    }
                }
                tree.commit(
                    tx,
                    BackendCapabilities {
                        solid_color_patch: true,
                    },
                )
                .unwrap();
                frames.push(tree.frame([1000.0; 2]).unwrap());
            }
            for control in &frames[1..] {
                equivalent(&frames[0], control);
                assert_eq!(
                    frames[0]
                        .updates
                        .iter()
                        .map(|i| i.node.slot)
                        .collect::<Vec<_>>(),
                    control
                        .updates
                        .iter()
                        .map(|i| i.node.slot)
                        .collect::<Vec<_>>()
                );
                for (item, other) in frames[0].items.iter().zip(control.items.iter()) {
                    assert_eq!(item.shadows, other.shadows);
                    assert_eq!(item.image, other.image);
                    assert_eq!(
                        (
                            item.spatial.layout_origin,
                            item.spatial.translation,
                            item.spatial.scroll,
                            item.spatial.clip
                        ),
                        (
                            other.spatial.layout_origin,
                            other.spatial.translation,
                            other.spatial.scroll,
                            other.spatial.clip
                        )
                    );
                }
                for point in [[5.0, 5.0], [15.0, 12.0], [500.5, 30.5], [999.0, 999.0]] {
                    assert_eq!(
                        frames[0].hit_test(point[0], point[1]).map(|n| n.slot),
                        control.hit_test(point[0], point[1]).map(|n| n.slot)
                    );
                }
            }
            assert_eq!(frames[0].build_stats.delta_cloned_items, 0);
            for item in frames[0].updates.iter() {
                let authoritative = frames[0]
                    .items
                    .iter()
                    .find(|i| i.node == item.node)
                    .unwrap();
                assert!(std::ptr::eq(item, authoritative));
            }
            if round == 0 {
                assert_eq!(
                    (
                        frames[0].build_stats.copied_items,
                        frames[0].build_stats.replaced_pages,
                        frames[0].build_stats.moved_items
                    ),
                    (0, 4, 201)
                );
                assert_eq!(
                    (
                        frames[1].build_stats.copied_items,
                        frames[1].build_stats.delta_cloned_items
                    ),
                    (201, 201)
                );
                assert_eq!(frames[0].hit_test_with_stats(15.0, 12.0).1.built_items, 0);
                saved = frames;
            }
        }
        drop(trees);
        drop((owned, copying, full));
        assert_eq!(saved[0].updates[0].bounds.x, 8.25);
        assert_eq!(initial[0].items[0].bounds.x, 0.0);
        let mut changed = saved[0].clone();
        Arc::make_mut(&mut changed.updates)[0].fill_color = 0x112233ff;
        assert_ne!(changed.updates[0].fill_color, changed.items[0].fill_color);
        assert_eq!(saved[0].items[81].image, Some(image));
    }
    fn fixture() -> (UiTree, Vec<NodeId>) {
        let mut tree = UiTree::new(
            Style {
                size: Size {
                    width: length(1000.0),
                    height: length(1000.0),
                },
                flex_wrap: taffy::FlexWrap::Wrap,
                ..Default::default()
            },
            0,
        )
        .unwrap();
        let mut nodes = vec![];
        for _ in 0..200 {
            nodes.push(
                tree.append(
                    tree.root(),
                    Style {
                        size: Size {
                            width: length(20.0),
                            height: length(20.0),
                        },
                        flex_shrink: 0.0,
                        ..Default::default()
                    },
                    0xff0000ff,
                )
                .unwrap(),
            );
        }
        (tree, nodes)
    }
    fn equivalent(a: &FramePacket, b: &FramePacket) {
        assert_eq!(a.items.len(), b.items.len());
        for (a, b) in a.items.iter().zip(b.items.iter()) {
            assert_eq!(
                (a.bounds, a.content_bounds, a.clip, a.fill_color),
                (b.bounds, b.content_bounds, b.clip, b.fill_color)
            );
            assert_eq!(a.commands, b.commands);
        }
        assert_eq!(a.damage, b.damage);
        assert_eq!(a.updates.len(), b.updates.len());
    }
    #[test]
    fn selective_pages_match_full_extraction_and_preserve_old_packets() {
        let (mut fast, nodes) = fixture();
        let (mut full, other) = fixture();
        full.set_frame_build_policy(FrameBuildPolicy::Full);
        let original = fast.frame([1000.0; 2]).unwrap();
        full.frame([1000.0; 2]).unwrap();
        let before = original.clone();
        let mut tx = Transaction::default();
        tx.color(nodes[70], 0x00ff0080);
        fast.commit(
            tx,
            BackendCapabilities {
                solid_color_patch: true,
            },
        )
        .unwrap();
        let mut tx = Transaction::default();
        tx.color(other[70], 0x00ff0080);
        full.commit(
            tx,
            BackendCapabilities {
                solid_color_patch: true,
            },
        )
        .unwrap();
        let after = fast.frame([1000.0; 2]).unwrap();
        let control = full.frame([1000.0; 2]).unwrap();
        equivalent(&after, &control);
        assert_eq!(
            after.build_stats,
            FrameBuildStats {
                extracted_nodes: 1,
                copied_pages: 1,
                copied_items: 64,
                full_rebuild: false,
                moved_items: 1,
                ..Default::default()
            }
        );
        assert_eq!(before.items.shared_pages(&after.items), 3);
        assert_eq!(before.items[71].fill_color, 0xff0000ff);
        assert_eq!(after.items[71].fill_color, 0x00ff0080);
        assert!(Arc::ptr_eq(&before.properties, &after.properties));
        assert!(Arc::ptr_eq(
            &before.items[71].commands,
            &after.items[71].commands
        ));
        // Separate commits before publication coalesce into one indexed extraction.
        for round in 0..16 {
            for slot in [0, 1, 62, 63, 64, 65, 128, 199] {
                let color = if round % 2 == 0 { 0x4455ffaa } else { 0 };
                for (tree, id) in [(&mut fast, nodes[slot]), (&mut full, other[slot])] {
                    let mut tx = Transaction::default();
                    tx.color(id, color);
                    tx.decoration(
                        id,
                        QuadDecoration {
                            radius: (round % 4) as f32,
                            border_width: 1.0,
                            border_color: 0xccccccff,
                        },
                    );
                    tree.commit(tx, BackendCapabilities::default()).unwrap();
                }
            }
            let a = fast.frame([1000.0; 2]).unwrap();
            let b = full.frame([1000.0; 2]).unwrap();
            equivalent(&a, &b);
            assert!(!a.build_stats.full_rebuild);
            assert!(a.build_stats.extracted_nodes <= 8);
            assert_eq!(
                fast.frame([1000.0; 2]).unwrap().build_stats,
                FrameBuildStats::default()
            );
        }
        assert_eq!(original.items[1].fill_color, 0xff0000ff);
    }
    #[test]
    fn hidden_material_changes_and_geometry_changes_use_correct_publication_paths() {
        let (mut tree, nodes) = fixture();
        tree.frame([1000.0; 2]).unwrap();
        let style = tree.style(nodes[5]).unwrap().clone();
        let mut hidden = style.clone();
        hidden.display = taffy::Display::None;
        let mut tx = Transaction::default();
        tx.style(nodes[5], hidden);
        tree.commit(tx, BackendCapabilities::default()).unwrap();
        let before = tree.frame([1000.0; 2]).unwrap();
        assert!(before.build_stats.full_rebuild);
        let mut tx = Transaction::default();
        tx.color(nodes[5], 0x4455ffff);
        tree.commit(tx, BackendCapabilities::default()).unwrap();
        let after = tree.frame([1000.0; 2]).unwrap();
        assert_eq!(after.build_stats.extracted_nodes, 0);
        assert!(after.updates.is_empty());
        assert!(Arc::ptr_eq(&before.items, &after.items));
        let mut tx = Transaction::default();
        tx.style(nodes[5], style);
        tree.commit(tx, BackendCapabilities::default()).unwrap();
        let restored = tree.frame([1000.0; 2]).unwrap();
        assert!(restored.build_stats.full_rebuild);
        assert_eq!(restored.items[6].fill_color, 0x4455ffff);
        let mut tx = Transaction::default();
        tx.translation(nodes[0], [1.0, 1.0]);
        tree.commit(tx, BackendCapabilities::default()).unwrap();
        let moved = tree.frame([1000.0; 2]).unwrap();
        assert!(!moved.build_stats.full_rebuild);
        assert_eq!(moved.build_stats.extracted_nodes, 1);
        let mut tx = Transaction::default();
        tx.color(nodes[0], 0x11ff11ff);
        tree.commit(tx, BackendCapabilities::default()).unwrap();
        assert_eq!(
            tree.frame([1000.0; 2]).unwrap().build_stats.extracted_nodes,
            1
        );
        tree.remove(nodes[0]).unwrap();
        assert!(tree.frame([1000.0; 2]).unwrap().build_stats.full_rebuild);
    }

    #[test]
    fn reordered_generation_reuse_updates_current_indices_and_old_page_lifetimes() {
        let (mut tree, nodes) = fixture();
        let original = tree.frame([1000.0; 2]).unwrap();
        let mut order = nodes.clone();
        order.reverse();
        let mut tx = Transaction::default();
        tx.reorder(tree.root(), order);
        tree.commit(tx, BackendCapabilities::default()).unwrap();
        let reordered = tree.frame([1000.0; 2]).unwrap();
        assert_eq!(reordered.items[1].node, nodes[199]);
        let mut tx = Transaction::default();
        tx.color(nodes[199], 0xaabbccff);
        tree.commit(
            tx,
            BackendCapabilities {
                solid_color_patch: true,
            },
        )
        .unwrap();
        let colored = tree.frame([1000.0; 2]).unwrap();
        assert_eq!(colored.items[1].fill_color, 0xaabbccff);
        assert_eq!(colored.build_stats.extracted_nodes, 1);
        tree.remove(nodes[199]).unwrap();
        let replacement = tree
            .append(tree.root(), Style::default(), 0x112233ff)
            .unwrap();
        assert_ne!(replacement, nodes[199]);
        tree.frame([1000.0; 2]).unwrap();
        let mut tx = Transaction::default();
        tx.color(replacement, 0x556677ff);
        tree.commit(
            tx,
            BackendCapabilities {
                solid_color_patch: true,
            },
        )
        .unwrap();
        let current = tree.frame([1000.0; 2]).unwrap();
        assert_eq!(
            current
                .items
                .iter()
                .find(|item| item.node == replacement)
                .unwrap()
                .fill_color,
            0x556677ff
        );
        assert!(current.items.iter().all(|item| item.node != nodes[199]));
        assert_eq!(original.items[200].fill_color, 0xff0000ff);
        let weak = Arc::downgrade(&colored.items.pages[0]);
        drop(colored);
        drop(reordered);
        assert!(
            weak.upgrade().is_none(),
            "retired snapshot pages release without retaining tree history"
        );
    }
}
