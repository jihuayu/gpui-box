//! Consumer-owned segment index over immutable render preorder. Tight layer
//! queries include all nested descendants. No caller-supplied dirty authority.
use super::*;
#[derive(Default)]
pub(in crate::retained) struct LocalInk {
    base: usize,
    tree: Vec<Option<Rect>>,
    raw: Vec<Option<Rect>>,
    exact: Vec<bool>,
    lazy: Vec<[f32; 2]>,
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
    fn lazy_integer_motion_matches_full_then_invalidates_for_clip_scroll_and_child_edit() {
        let style = |x, y, w, h| Style {
            position: taffy::Position::Absolute,
            inset: taffy::Rect {
                left: length(x),
                top: length(y),
                right: taffy::LengthPercentageAuto::auto(),
                bottom: taffy::LengthPercentageAuto::auto(),
            },
            size: taffy::Size {
                width: length(w),
                height: length(h),
            },
            ..Default::default()
        };
        let mut tree = UiTree::new(style(0.0, 0.0, 300.0, 200.0), 0).unwrap();
        let group = tree
            .append(tree.root(), style(40.0, 40.0, 80.0, 60.0), 0)
            .unwrap();
        let child = tree
            .append(group, style(4.0, 4.0, 40.0, 30.0), 0xff0000ff)
            .unwrap();
        let mut old = tree.local_frame([300.0, 200.0]).unwrap();
        let (mut index, _) = LocalInk::rebuild(&old);
        for step in 0..40 {
            let mut tx = Transaction::default();
            tx.translation(group, [if step % 2 == 0 { 8.0 } else { -8.0 }, 0.0]);
            tree.commit(tx, BackendCapabilities::default()).unwrap();
            let frame = tree.local_frame([300.0, 200.0]).unwrap();
            assert_eq!(index.update_motion(&frame, &old, true), (0, 2));
            check(&index, &frame);
            old = frame;
        }
        // Lazy parent shifts must be pushed correctly before a child-only edit.
        for step in 0..5 {
            let mut tx = Transaction::default();
            match step {
                0 => tx.translation(child, [0.5, 0.0]),
                1 => tx.translation(group, [16.0, 0.0]),
                2 => tx.clip(
                    group,
                    Some(Rect {
                        x: 0.0,
                        y: 0.0,
                        width: 12.0,
                        height: 10.0,
                    }),
                ),
                3 => tx.scroll(group, [1.0, 0.0]),
                _ => tx.translation(group, [-80.0, 0.0]),
            }
            tree.commit(tx, BackendCapabilities::default()).unwrap();
            let frame = tree.local_frame([300.0, 200.0]).unwrap();
            let stats = index.update_motion(&frame, &old, true);
            assert_eq!(stats.1, 0);
            check(&index, &frame);
            old = frame;
        }
    }
    #[test]
    fn mixed_nested_motion_matches_full_across_long_lazy_history_and_empty_clips() {
        let style = |x, y, w, h| Style {
            position: taffy::Position::Absolute,
            inset: taffy::Rect {
                left: length(x),
                top: length(y),
                right: taffy::LengthPercentageAuto::auto(),
                bottom: taffy::LengthPercentageAuto::auto(),
            },
            size: taffy::Size {
                width: length(w),
                height: length(h),
            },
            ..Default::default()
        };
        let mut tree = UiTree::new(style(0.0, 0.0, 512.0, 512.0), 0).unwrap();
        // Fractional external ancestry cancels before the eligible subtree.
        let outer = tree
            .append(tree.root(), style(0.5, 0.5, 400.0, 400.0), 0)
            .unwrap();
        let group = tree
            .append(outer, style(39.5, 39.5, 240.0, 240.0), 0)
            .unwrap();
        let mut nodes = vec![group];
        for n in 0..12 {
            let parent = if n % 3 == 0 {
                group
            } else {
                *nodes.last().unwrap()
            };
            nodes.push(
                tree.append(parent, style(4.0, 3.0, 32.0, 24.0), 0x4488ccff)
                    .unwrap(),
            );
        }
        let mut old = tree.local_frame([512.0, 512.0]).unwrap();
        let (mut index, _) = LocalInk::rebuild(&old);
        let mut random = 0x4e554c4cu32;
        let mut shifted = 0;
        let mut visited = 0;
        for step in 0..512 {
            random = random.wrapping_mul(1664525).wrapping_add(1013904223);
            let node = nodes[(random as usize >> 8) % nodes.len()];
            let mut tx = Transaction::default();
            let d = ((random >> 16) % 17) as f32 - 8.0;
            tx.translation(node, [d, -d]);
            match step % 16 {
                0 => tx.clip(
                    node,
                    Some(Rect {
                        x: 0.0,
                        y: 0.0,
                        width: 0.0,
                        height: 10.0,
                    }),
                ),
                1 => tx.clip(
                    node,
                    Some(Rect {
                        x: -2.0,
                        y: 1.0,
                        width: 17.0,
                        height: 13.0,
                    }),
                ),
                2 => tx.clip(node, None),
                3 => tx.scroll(node, [2.0, 1.0]),
                4 => tx.scroll(node, [0.0, 0.0]),
                5 => tx.translation(node, [d + 0.25, -d]),
                // Two nested/disjoint dirty roots must never hide a child edit.
                6 => tx.translation(group, [d, d]),
                _ => {}
            }
            tree.commit(tx, BackendCapabilities::default()).unwrap();
            let frame = tree.local_frame([512.0, 512.0]).unwrap();
            let stats = index.update_motion(&frame, &old, true);
            visited += stats.0;
            shifted += stats.1;
            check(&index, &frame);
            old = frame;
        }
        assert!(shifted > 0 && visited > 0);
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
            raw: vec![None; base * 2],
            exact: vec![true; base * 2],
            lazy: vec![[0.0; 2]; base * 2],
        };
        frame.visit_geometry(0..frame.len(), |i, item| result.set_leaf(i, item));
        for i in (1..base).rev() {
            result.pull(i);
        }
        (result, frame.len())
    }
    fn record(item: &PaintItem) -> (Option<Rect>, Option<Rect>, bool) {
        let ink = item.ink_bounds();
        let mut raw = item.clone();
        raw.clip = None;
        // Invisible ancestors still participate in child origin arithmetic.
        let raw = Some(
            raw.ink_bounds()
                .map_or(item.bounds, |r| r.union(item.bounds)),
        );
        let int = |v: f32| v.is_finite() && v.fract() == 0.0 && v.abs() <= 65536.0;
        let rect = |r: Rect| {
            [r.x, r.y, r.width, r.height, r.x + r.width, r.y + r.height]
                .into_iter()
                .all(int)
        };
        let spatial = &item.spatial;
        let exact = spatial
            .layout_origin
            .iter()
            .chain(&spatial.translation)
            .chain(&spatial.scroll)
            .copied()
            .all(int)
            && spatial.clip.is_none_or(rect)
            && item.clip.is_none_or(rect)
            && rect(item.bounds)
            && raw.is_none_or(rect)
            && item.image.is_none()
            && item.shadows.iter().all(|s| {
                s.offset
                    .into_iter()
                    .chain([s.blur_radius, s.spread_radius])
                    .all(int)
            });
        #[cfg(all(feature = "retained-text", not(target_family = "wasm")))]
        let exact = exact && item.text.is_none() && item.glyphs.is_none();
        (ink, raw, exact)
    }
    fn set_leaf(&mut self, i: usize, item: &PaintItem) {
        self.set_record(i, Self::record(item));
    }
    fn set_record(&mut self, i: usize, value: (Option<Rect>, Option<Rect>, bool)) {
        let slot = self.base + i;
        self.tree[slot] = value.0;
        self.raw[slot] = value.1;
        self.exact[slot] = value.2;
        self.lazy[slot] = [0.0; 2];
    }
    fn pull(&mut self, i: usize) {
        self.tree[i] = union(self.tree[i * 2], self.tree[i * 2 + 1]);
        self.raw[i] = union(self.raw[i * 2], self.raw[i * 2 + 1]);
        self.exact[i] = self.exact[i * 2] && self.exact[i * 2 + 1];
    }
    fn shift(&mut self, i: usize, d: [f32; 2]) {
        self.tree[i] = self.tree[i].map(|r| r.translated(d));
        self.raw[i] = self.raw[i].map(|r| r.translated(d));
        self.lazy[i][0] += d[0];
        self.lazy[i][1] += d[1];
    }
    fn push(&mut self, i: usize) {
        if i < self.base && self.lazy[i] != [0.0; 2] {
            let d = self.lazy[i];
            self.shift(i * 2, d);
            self.shift(i * 2 + 1, d);
            self.lazy[i] = [0.0; 2];
        }
    }
    fn move_range(
        &mut self,
        i: usize,
        start: usize,
        end: usize,
        range: &std::ops::Range<usize>,
        delta: [f32; 2],
    ) {
        if range.start >= end || range.end <= start {
            return;
        }
        if range.start <= start && end <= range.end {
            self.shift(i, delta);
            return;
        }
        self.push(i);
        let mid = (start + end) / 2;
        self.move_range(i * 2, start, mid, range, delta);
        self.move_range(i * 2 + 1, mid, end, range, delta);
        self.pull(i);
    }
    fn write_range(
        &mut self,
        i: usize,
        start: usize,
        end: usize,
        range: &std::ops::Range<usize>,
        values: &[(Option<Rect>, Option<Rect>, bool)],
    ) {
        if range.start >= end || range.end <= start {
            return;
        }
        if end - start == 1 {
            self.set_record(start, values[start - range.start]);
            return;
        }
        self.push(i);
        let mid = (start + end) / 2;
        self.write_range(i * 2, start, mid, range, values);
        self.write_range(i * 2 + 1, mid, end, range, values);
        self.pull(i);
    }
    #[cfg(test)]
    pub fn update(&mut self, frame: &local::LocalFrame) -> usize {
        self.update_impl(frame, None).0
    }
    pub fn update_motion(
        &mut self,
        frame: &local::LocalFrame,
        old: &local::LocalFrame,
        enabled: bool,
    ) -> (usize, usize) {
        self.update_impl(frame, enabled.then_some(old))
    }
    fn update_impl(
        &mut self,
        frame: &local::LocalFrame,
        old: Option<&local::LocalFrame>,
    ) -> (usize, usize) {
        let mut visited = 0;
        let mut shifted = 0;
        for range in frame.changed_subtrees() {
            let motion = old.and_then(|old| frame.ink_translation(old, range.clone()));
            if let Some((delta, clip)) = motion {
                let (raw, exact) = self.query_meta(1, 0, self.base, &range, [0.0; 2]);
                let eligible = exact
                    && raw.is_some_and(|raw| {
                        let moved = raw.translated(delta);
                        let valid = |r: Rect| {
                            [r.x, r.y, r.x + r.width, r.y + r.height]
                                .into_iter()
                                .all(|v| v.abs() <= 65536.0 && v.fract() == 0.0)
                        };
                        let contains = |r: Rect, c: Rect| {
                            r.x >= c.x
                                && r.y >= c.y
                                && r.x + r.width <= c.x + c.width
                                && r.y + r.height <= c.y + c.height
                        };
                        valid(raw)
                            && valid(moved)
                            && clip.is_none_or(|clip| contains(raw, clip) && contains(moved, clip))
                    });
                if eligible {
                    self.move_range(1, 0, self.base, &range, delta);
                    shifted += range.len();
                    continue;
                }
            }
            visited += range.len();
            let mut values = Vec::with_capacity(range.len());
            frame.visit_geometry(range.clone(), |_, item| values.push(Self::record(item)));
            self.write_range(1, 0, self.base, &range, &values);
        }
        (visited, shifted)
    }
    fn query_meta(
        &self,
        i: usize,
        start: usize,
        end: usize,
        range: &std::ops::Range<usize>,
        offset: [f32; 2],
    ) -> (Option<Rect>, bool) {
        if range.start >= end || range.end <= start {
            return (None, true);
        }
        if range.start <= start && end <= range.end {
            return (self.raw[i].map(|r| r.translated(offset)), self.exact[i]);
        }
        let offset = [offset[0] + self.lazy[i][0], offset[1] + self.lazy[i][1]];
        let mid = (start + end) / 2;
        let (a, x) = self.query_meta(i * 2, start, mid, range, offset);
        let (b, y) = self.query_meta(i * 2 + 1, mid, end, range, offset);
        (union(a, b), x && y)
    }
    pub fn query(&self, range: std::ops::Range<usize>) -> Option<Rect> {
        self.query_at(1, 0, self.base, &range, [0.0; 2])
    }
    fn query_at(
        &self,
        i: usize,
        start: usize,
        end: usize,
        range: &std::ops::Range<usize>,
        offset: [f32; 2],
    ) -> Option<Rect> {
        if range.start >= end || range.end <= start {
            return None;
        }
        if range.start <= start && end <= range.end {
            return self.tree[i].map(|r| r.translated(offset));
        }
        let offset = [offset[0] + self.lazy[i][0], offset[1] + self.lazy[i][1]];
        let mid = (start + end) / 2;
        union(
            self.query_at(i * 2, start, mid, range, offset),
            self.query_at(i * 2 + 1, mid, end, range, offset),
        )
    }
}
