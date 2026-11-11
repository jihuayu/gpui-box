//! Persistent page BVH. It stores bounds and order, not resource-owning items.
use super::*;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct HitTestStats {
    pub visited_bounds: usize,
    pub tested_items: usize,
    /// Cold/lazily-invalidated index construction scans all item bounds once.
    pub built_items: usize,
}

#[derive(Debug)]
pub(super) struct HitNode {
    start: usize,
    end: usize,
    bounds: Option<Rect>,
    children: Option<(Arc<HitNode>, Arc<HitNode>)>,
}

fn contains(b: Rect, x: f32, y: f32) -> bool {
    x >= b.x && y >= b.y && x < b.x + b.width && y < b.y + b.height
}
fn union(a: Option<Rect>, b: Option<Rect>) -> Option<Rect> {
    match (a, b) {
        (Some(a), Some(b)) => Some(a.union(b)),
        (a, b) => a.or(b),
    }
}
fn page_bounds(items: &[PaintItem]) -> Option<Rect> {
    items
        .iter()
        .map(PaintItem::visible_bounds)
        .filter(|b| !b.is_empty())
        .fold(None, |bounds, b| union(bounds, Some(b)))
}

impl HitNode {
    fn build(pages: &[Arc<Vec<PaintItem>>], start: usize, end: usize) -> Arc<Self> {
        let children = (end - start > 1).then(|| {
            let mid = start + (end - start) / 2;
            (Self::build(pages, start, mid), Self::build(pages, mid, end))
        });
        let bounds = children.as_ref().map_or_else(
            || page_bounds(&pages[start]),
            |(a, b)| union(a.bounds, b.bounds),
        );
        Arc::new(Self {
            start,
            end,
            bounds,
            children,
        })
    }
    pub(super) fn patch(
        previous: &Arc<Self>,
        pages: &[Arc<Vec<PaintItem>>],
        dirty: &[usize],
    ) -> Arc<Self> {
        let first = dirty.partition_point(|page| *page < previous.start);
        if first == dirty.len() || dirty[first] >= previous.end {
            return previous.clone();
        }
        let children = previous
            .children
            .as_ref()
            .map(|(a, b)| (Self::patch(a, pages, dirty), Self::patch(b, pages, dirty)));
        let bounds = children.as_ref().map_or_else(
            || page_bounds(&pages[previous.start]),
            |(a, b)| union(a.bounds, b.bounds),
        );
        if bounds == previous.bounds
            && match (&children, &previous.children) {
                (None, None) => true,
                (Some((a, b)), Some((old_a, old_b))) => {
                    Arc::ptr_eq(a, old_a) && Arc::ptr_eq(b, old_b)
                }
                _ => false,
            }
        {
            return previous.clone();
        }
        Arc::new(Self {
            start: previous.start,
            end: previous.end,
            bounds,
            children,
        })
    }
    fn query(
        &self,
        pages: &[Arc<Vec<PaintItem>>],
        x: f32,
        y: f32,
        stats: &mut HitTestStats,
    ) -> Option<NodeId> {
        stats.visited_bounds += 1;
        if self.bounds.is_none_or(|bounds| !contains(bounds, x, y)) {
            return None;
        }
        if let Some((left, right)) = &self.children {
            return right
                .query(pages, x, y, stats)
                .or_else(|| left.query(pages, x, y, stats));
        }
        for item in pages[self.start].iter().rev() {
            stats.tested_items += 1;
            if contains(item.visible_bounds(), x, y) {
                return Some(item.node);
            }
        }
        None
    }
}

impl PaintItems {
    pub(in crate::retained) fn hit_test(&self, x: f32, y: f32) -> (Option<NodeId>, HitTestStats) {
        let mut stats = HitTestStats::default();
        if self.is_empty() || !x.is_finite() || !y.is_finite() {
            return (None, stats);
        }
        let index = self.hit_index.get_or_init(|| {
            stats.built_items = self.len;
            HitNode::build(&self.pages, 0, self.pages.len())
        });
        (index.query(&self.pages, x, y, &mut stats), stats)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn linear(items: &PaintItems, x: f32, y: f32) -> Option<NodeId> {
        items
            .iter()
            .rev()
            .find(|item| contains(item.visible_bounds(), x, y))
            .map(|item| item.node)
    }
    #[test]
    fn page_index_matches_topmost_clipped_geometry_and_persists_across_updates() {
        let mut tree = UiTree::new(
            Style {
                flex_wrap: taffy::FlexWrap::Wrap,
                size: Size {
                    width: length(1000.0),
                    height: length(1000.0),
                },
                ..Default::default()
            },
            0,
        )
        .unwrap();
        let mut nodes = vec![];
        for _ in 0..10000 {
            nodes.push(
                tree.append(
                    tree.root(),
                    Style {
                        size: Size {
                            width: length(10.0),
                            height: length(10.0),
                        },
                        flex_shrink: 0.0,
                        ..Default::default()
                    },
                    0xff0000ff,
                )
                .unwrap(),
            );
        }
        let original = tree.frame([1000.0; 2]).unwrap();
        let (_, cold) = original.hit_test_with_stats(5.0, 5.0);
        assert_eq!(cold.built_items, 10001);
        let old_index = original.items.hit_index.get().unwrap().clone();
        for round in 0..8 {
            let mut tx = Transaction::default();
            tx.translation(nodes[9970], [-700.0, -900.0 + (round as f32)]);
            tx.clip(
                nodes[9970],
                Some(Rect {
                    x: 1.0,
                    y: 1.0,
                    width: 6.0,
                    height: 6.0,
                }),
            );
            tx.color(nodes[70], round);
            tree.commit(
                tx,
                BackendCapabilities {
                    solid_color_patch: true,
                },
            )
            .unwrap();
            let frame = tree.frame([1000.0; 2]).unwrap();
            for (x, y) in [
                (5.0, 5.0),
                (2.0, 92.0),
                (1000.0, 20.0),
                (-1.0, 20.0),
                (999.0, 999.0),
                (550.5, 440.25),
            ] {
                let (actual, stats) = frame.hit_test_with_stats(x, y);
                assert_eq!(actual, linear(&frame.items, x, y));
                assert_eq!(stats.built_items, 0);
            }
        }
        assert!(Arc::ptr_eq(
            &old_index,
            original.items.hit_index.get().unwrap()
        ));
        assert_eq!(
            original.hit_test(2.0, 92.0),
            linear(&original.items, 2.0, 92.0)
        );
        let (_, warm) = original.hit_test_with_stats(5.0, 5.0);
        // The next page straddles two rows, so its union overlaps the first
        // page's area even when none of its individual items hit this point.
        assert!(
            warm.tested_items <= 128 && warm.visited_bounds < 32,
            "{warm:?}"
        );
        let mut tx = Transaction::default();
        tx.color(nodes[5], 0x00ff00ff);
        tree.commit(
            tx,
            BackendCapabilities {
                solid_color_patch: true,
            },
        )
        .unwrap();
        let before = tree
            .published_frame()
            .unwrap()
            .items
            .hit_index
            .get()
            .unwrap()
            .clone();
        let colored = tree.frame([1000.0; 2]).unwrap();
        assert!(Arc::ptr_eq(&before, colored.items.hit_index.get().unwrap()));
        // Public mutable COW access must invalidate the summary of that packet.
        let mut modified = colored.clone();
        let last = modified.items.len() - 1;
        Arc::make_mut(&mut modified.items)[last].bounds = Rect {
            x: 0.0,
            y: 0.0,
            width: 10.0,
            height: 10.0,
        };
        let (hit, stats) = modified.hit_test_with_stats(5.0, 5.0);
        assert_eq!(hit, Some(nodes[9999]));
        assert_eq!(stats.built_items, 10001);
        assert_ne!(colored.hit_test(5.0, 5.0), hit);
        assert_eq!(modified.hit_test(f32::NAN, 0.0), None);
    }

    #[test]
    fn order_visibility_clip_edges_and_slot_reuse_match_linear_hit_contract() {
        let mut tree = UiTree::new(Style::default(), 0).unwrap();
        let fixed = Style {
            size: Size {
                width: length(20.0),
                height: length(20.0),
            },
            position: taffy::Position::Absolute,
            ..Default::default()
        };
        let a = tree.append(tree.root(), fixed.clone(), 0).unwrap();
        let b = tree.append(tree.root(), fixed.clone(), 0).unwrap();
        let first = tree.frame([100.0; 2]).unwrap();
        assert_eq!(first.hit_test(5.0, 5.0), Some(b));
        assert_eq!(first.hit_test(20.0, 5.0), linear(&first.items, 20.0, 5.0));
        let mut tx = Transaction::default();
        tx.reorder(tree.root(), vec![b, a]);
        tx.clip(
            a,
            Some(Rect {
                x: 1.0,
                y: 1.0,
                width: 5.0,
                height: 5.0,
            }),
        );
        tree.commit(tx, BackendCapabilities::default()).unwrap();
        let reordered = tree.frame([100.0; 2]).unwrap();
        assert_eq!(reordered.hit_test(2.0, 2.0), Some(a));
        assert_eq!(reordered.hit_test(6.0, 2.0), Some(b));
        let mut tx = Transaction::default();
        let mut hidden = fixed.clone();
        hidden.display = taffy::Display::None;
        tx.style(a, hidden);
        tree.commit(tx, BackendCapabilities::default()).unwrap();
        let hidden = tree.frame([100.0; 2]).unwrap();
        assert_eq!(hidden.hit_test(2.0, 2.0), Some(b));
        tree.remove(a).unwrap();
        let replacement = tree.append(tree.root(), fixed, 0).unwrap();
        assert_ne!(replacement, a);
        let replaced = tree.frame([100.0; 2]).unwrap();
        assert_eq!(replaced.hit_test(2.0, 2.0), Some(replacement));
        assert_eq!(first.hit_test(2.0, 2.0), Some(b));
        assert_eq!(reordered.hit_test(2.0, 2.0), Some(a));
        assert_eq!(PaintItems::default().hit_test(0.0, 0.0).0, None);
    }
}
