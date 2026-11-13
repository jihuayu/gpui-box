//! Consumer-owned segment index over immutable render preorder. Tight layer
//! queries include all nested descendants. No caller-supplied dirty authority.
use super::*;
#[derive(Default)]
pub(in crate::retained) struct LocalInk {
    base: usize,
    tree: Vec<Option<Rect>>,
}

#[cfg(test)]
mod tests {
    use super::*;
    fn check(index: &LocalInk, frame: &local::LocalFrame) {
        let resolved = frame.resolve();
        for item in resolved.items.iter() {
            let range = frame.subtree(item.node).unwrap();
            let expected = resolved
                .items
                .iter()
                .skip(range.start)
                .take(range.len())
                .filter_map(PaintItem::ink_bounds)
                .fold(None, |a, b| union(a, Some(b)));
            assert_eq!(index.query(range), expected);
        }
    }
    #[test]
    fn subtree_index_matches_resolve_after_overlapping_updates_and_preserves_old_snapshot() {
        let style = |x, y| Style {
            position: taffy::Position::Absolute,
            inset: taffy::Rect {
                left: length(x),
                top: length(y),
                right: taffy::LengthPercentageAuto::auto(),
                bottom: taffy::LengthPercentageAuto::auto(),
            },
            size: taffy::Size {
                width: length(32.0),
                height: length(24.0),
            },
            ..Default::default()
        };
        let mut tree = UiTree::new(style(0.0, 0.0), 0x112233ff).unwrap();
        let a = tree
            .append(tree.root(), style(2.0, 3.0), 0xff000080)
            .unwrap();
        let b = tree.append(a, style(4.0, 5.0), 0x0000ffff).unwrap();
        let c = tree
            .append(tree.root(), style(12.0, 18.0), 0x00ff00ff)
            .unwrap();
        let mut tx = Transaction::default();
        tx.opacity(a, 0.5);
        tx.shadows(
            b,
            vec![BoxShadow {
                color: 0x00000088,
                blur_radius: 2.0,
                ..Default::default()
            }],
        );
        tree.commit(tx, BackendCapabilities::default()).unwrap();
        let original = tree.local_frame([80.0, 60.0]).unwrap();
        let (mut index, n) = LocalInk::rebuild(&original);
        assert_eq!(n, 4);
        check(&index, &original);
        let mut tx = Transaction::default();
        tx.translation(a, [8.0, 4.0]);
        tx.translation(b, [-2.0, 1.0]);
        tree.commit(tx, BackendCapabilities::default()).unwrap();
        let frame = tree.local_frame([80.0, 60.0]).unwrap();
        assert_eq!(index.update(&frame), 2);
        check(&index, &frame);
        let mut tx = Transaction::default();
        tx.clip(
            a,
            Some(Rect {
                x: 1.0,
                y: 1.0,
                width: 12.0,
                height: 10.0,
            }),
        );
        tx.translation(c, [-8.0, -4.0]);
        tree.commit(tx, BackendCapabilities::default()).unwrap();
        let next = tree.local_frame([80.0, 60.0]).unwrap();
        assert_eq!(index.update(&next), 3);
        check(&index, &next);
        let (old, _) = LocalInk::rebuild(&original);
        check(&old, &original);
        // The original immutable snapshot still has its original hit geometry.
        assert_eq!(original.resolve_geometry(a).unwrap().bounds.x, 2.0);
        assert_eq!(next.resolve_geometry(a).unwrap().bounds.x, 10.0);
    }
}
fn union(a: Option<Rect>, b: Option<Rect>) -> Option<Rect> {
    match (a, b) {
        (Some(a), Some(b)) => Some(a.union(b)),
        (a, b) => a.or(b),
    }
}
impl LocalInk {
    pub fn rebuild(frame: &local::LocalFrame) -> (Self, usize) {
        let base = frame.len().max(1).next_power_of_two();
        let mut result = Self {
            base,
            tree: vec![None; base * 2],
        };
        frame.visit_geometry(0..frame.len(), |i, item| {
            result.tree[base + i] = item.ink_bounds()
        });
        for i in (1..base).rev() {
            result.tree[i] = union(result.tree[i * 2], result.tree[i * 2 + 1]);
        }
        (result, frame.len())
    }
    pub fn update(&mut self, frame: &local::LocalFrame) -> usize {
        let mut count = 0;
        for range in frame.changed_subtrees() {
            count += range.len();
            frame.visit_geometry(range.clone(), |i, item| {
                self.tree[self.base + i] = item.ink_bounds()
            });
            // Rebuild each affected parent interval once at each index level.
            let mut left = (self.base + range.start) / 2;
            let mut right = (self.base + range.end - 1) / 2;
            while left > 0 {
                for i in left..=right {
                    self.tree[i] = union(self.tree[i * 2], self.tree[i * 2 + 1]);
                }
                left /= 2;
                right /= 2;
            }
        }
        count
    }
    pub fn query(&self, range: std::ops::Range<usize>) -> Option<Rect> {
        let (mut left, mut right) = (self.base + range.start, self.base + range.end);
        let mut result = None;
        while left < right {
            if left % 2 == 1 {
                result = union(result, self.tree[left]);
                left += 1;
            }
            if right % 2 == 1 {
                right -= 1;
                result = union(result, self.tree[right]);
            }
            left /= 2;
            right /= 2;
        }
        result
    }
}
