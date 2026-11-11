//! Copy-on-write paint pages. Node indices remain stable until layout/order changes.
use super::*;
use std::ops::{Index, IndexMut};

const PAGE_ITEMS: usize = 64;

/// Full mode provides a correctness/performance control using the same owning
/// tree and commands. It changes extraction work, not rendered behavior.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FrameBuildPolicy {
    #[default]
    Selective,
    Full,
}

/// Ordered immutable frame storage. Updating a page preserves all other page Arcs
/// and old packets. The small page directory is copied on each changed snapshot.
#[derive(Clone, Debug, Default)]
pub struct PaintItems {
    pages: Vec<Arc<Vec<PaintItem>>>,
    len: usize,
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
        Self { pages, len }
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
    pub(super) fn patch(
        &self,
        updates: &[(usize, PaintItem)],
        stats: &mut FrameBuildStats,
    ) -> Self {
        let mut result = self.clone();
        for (index, item) in updates {
            let page = &mut result.pages[index / PAGE_ITEMS];
            if Arc::strong_count(page) > 1 {
                stats.copied_pages += 1;
                stats.copied_items += page.len();
            }
            Arc::make_mut(page)[index % PAGE_ITEMS] = item.clone();
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
        &mut Arc::make_mut(&mut self.pages[index / PAGE_ITEMS])[index % PAGE_ITEMS]
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FrameBuildStats {
    /// Render nodes extracted, excluding copy-on-write page item clones.
    pub extracted_nodes: usize,
    pub copied_pages: usize,
    pub copied_items: usize,
    pub full_rebuild: bool,
}

impl UiTree {
    pub fn set_frame_build_policy(&mut self, policy: FrameBuildPolicy) {
        self.frame_build_policy = policy;
    }
    /// Material changes extract individual visible nodes. Visual property
    /// changes extract disjoint affected subtrees from persistent layout.
    pub(super) fn patch_paint_frame(&mut self) -> Result<FramePacket, Error> {
        let before = self.frame.as_ref().expect("published geometry").clone();
        let mut candidates = BTreeMap::new();
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
        let mut properties = if disjoint.is_empty() {
            before.properties.clone()
        } else {
            Arc::new((*before.properties).clone())
        };
        for &(start, end, id) in &disjoint {
            let (origin, clip, state) = self.subtree_context(id, &before)?;
            let props = Arc::get_mut(&mut properties).expect("private property snapshot");
            for index in start..end {
                let node = before.items[index].node;
                props.transforms.remove(&node);
                props.clips.remove(&node);
                props.scrolls.remove(&node);
                props.effects.remove(&node);
            }
            let extracted =
                self.extract_subtree(id, origin, clip, state, props, &mut recorded_chunks)?;
            debug_assert_eq!(extracted.len(), end - start);
            stats.extracted_nodes += extracted.len();
            for (index, item) in (start..end).zip(extracted) {
                debug_assert_eq!(item.node, before.items[index].node);
                candidates.insert(index, item);
            }
        }
        for id in self.paint_dirty.iter().copied().collect::<Vec<_>>() {
            let Some(index) = self.paint_indices.get(&id).copied() else {
                continue;
            };
            if candidates.contains_key(&index) {
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
            candidates.insert(index, item);
        }
        let mut patches = vec![];
        for (index, item) in candidates {
            let old = &before.items[index];
            let paint_changed = old.bounds != item.bounds
                || old.content_bounds != item.content_bounds
                || old.clip != item.clip
                || old.commands != item.commands
                || old.fill_color != item.fill_color
                || old.image != item.image
                || !old.same_text(&item);
            let effects_changed = !properties::same_effects(
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
            if paint_changed {
                updates.push(item.clone());
            }
            if paint_changed || old.properties != item.properties {
                patches.push((index, item));
            }
        }
        let items = if patches.is_empty() {
            before.items.clone()
        } else {
            Arc::new(before.items.patch(&patches, &mut stats))
        };
        let frame = FramePacket {
            tree: self.identity,
            base_revision: Some(before.revision),
            revision: self.revision,
            items,
            layout_runs: 0,
            recorded_chunks,
            layout_stats: LayoutStats::default(),
            build_stats: stats,
            updates: updates.into(),
            removed: Arc::from([]),
            order: None,
            properties,
            damage: Damage {
                full: false,
                bounds: damage,
            },
        };
        self.paint_dirty.clear();
        self.property_dirty.clear();
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
                full_rebuild: false
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
