//! Shared sparse visual-property chains and conservative frame damage.
use super::*;

impl Rect {
    pub fn intersect(self, other: Self) -> Self {
        let x = self.x.max(other.x);
        let y = self.y.max(other.y);
        Self {
            x,
            y,
            width: (self.x + self.width).min(other.x + other.width).max(x) - x,
            height: (self.y + self.height).min(other.y + other.height).max(y) - y,
        }
    }
    pub fn is_empty(self) -> bool {
        self.width <= 0.0 || self.height <= 0.0
    }
    pub fn union(self, other: Self) -> Self {
        if self.is_empty() {
            return other;
        }
        if other.is_empty() {
            return self;
        }
        let x = self.x.min(other.x);
        let y = self.y.min(other.y);
        Self {
            x,
            y,
            width: (self.x + self.width).max(other.x + other.width) - x,
            height: (self.y + self.height).max(other.y + other.height) - y,
        }
    }
    pub fn translated(self, origin: [f32; 2]) -> Self {
        Self {
            x: self.x + origin[0],
            y: self.y + origin[1],
            ..self
        }
    }
    pub fn valid(self) -> bool {
        [
            self.x,
            self.y,
            self.width,
            self.height,
            self.x + self.width,
            self.y + self.height,
        ]
        .iter()
        .all(|v| v.is_finite())
            && self.width >= 0.0
            && self.height >= 0.0
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PropertyState {
    pub transform: Option<NodeId>,
    pub clip: Option<NodeId>,
    pub scroll: Option<NodeId>,
}

#[derive(Clone, Debug)]
pub struct PropertyNode<T> {
    pub parent: Option<NodeId>,
    pub value: T,
}

#[derive(Clone, Debug, Default)]
pub struct PropertyTrees {
    pub transforms: BTreeMap<NodeId, PropertyNode<[f32; 2]>>,
    pub clips: BTreeMap<NodeId, PropertyNode<Rect>>,
    pub scrolls: BTreeMap<NodeId, PropertyNode<[f32; 2]>>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Damage {
    pub full: bool,
    pub bounds: Option<Rect>,
}

impl PaintItem {
    pub fn visible_bounds(&self) -> Rect {
        self.clip
            .map_or(self.bounds, |clip| self.bounds.intersect(clip))
    }
    pub fn ink_bounds(&self) -> Option<Rect> {
        let bounds = self.visible_bounds();
        (!bounds.is_empty()
            && self.commands.iter().any(|command| match command {
                DisplayCommand::Fill(color) => color & 255 != 0,
            }))
        .then_some(bounds)
    }
}

pub(super) fn damage(
    previous: Option<&FramePacket>,
    items: &[PaintItem],
    viewport_changed: bool,
    order_changed: bool,
) -> Damage {
    let Some(previous) = previous else {
        return Damage {
            full: true,
            bounds: None,
        };
    };
    if viewport_changed || order_changed {
        return Damage {
            full: true,
            bounds: None,
        };
    }
    let old: BTreeMap<_, _> = previous
        .items
        .iter()
        .map(|item| (item.node, item))
        .collect();
    let new: BTreeMap<_, _> = items.iter().map(|item| (item.node, item)).collect();
    let mut bounds: Option<Rect> = None;
    for item in previous.items.iter().chain(items.iter()) {
        if old
            .get(&item.node)
            .zip(new.get(&item.node))
            .is_some_and(|(a, b)| {
                a.bounds == b.bounds && a.clip == b.clip && a.commands == b.commands
            })
        {
            continue;
        }
        if let Some(ink) = item.ink_bounds() {
            bounds = Some(bounds.map_or(ink, |old| old.union(ink)));
        }
    }
    Damage {
        full: false,
        bounds,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> (UiTree, NodeId, NodeId) {
        let mut tree = UiTree::new(
            Style {
                size: Size {
                    width: length(80.0),
                    height: length(40.0),
                },
                ..Default::default()
            },
            0x333333ff,
        )
        .unwrap();
        let style = Style {
            size: Size {
                width: length(64.0),
                height: length(32.0),
            },
            flex_shrink: 0.0,
            ..Default::default()
        };
        let a = tree.append(tree.root(), style.clone(), 0xff0000ff).unwrap();
        let b = tree.append(tree.root(), style, 0x0000ffff).unwrap();
        (tree, a, b)
    }
    #[test]
    fn scroll_clamps_to_layout_extent_keeps_background_and_shares_clip_state() {
        let (mut tree, a, b) = fixture();
        let before = tree.frame([160.0, 80.0]).unwrap();
        let root = tree.root();
        let mut tx = Transaction::default();
        tx.clip(
            root,
            Some(Rect {
                x: 0.0,
                y: 0.0,
                width: 80.0,
                height: 40.0,
            }),
        );
        tx.scroll(root, [10_000.0, 0.0]);
        tree.commit(tx, BackendCapabilities::default()).unwrap();
        let frame = tree.frame([160.0, 80.0]).unwrap();
        assert_eq!((frame.layout_runs, frame.recorded_chunks), (0, 0));
        assert_eq!(frame.items[0].bounds, before.items[0].bounds);
        assert_eq!(frame.properties.scrolls[&root].value, [48.0, 0.0]);
        assert_eq!(frame.items[0].properties.scroll, None);
        for item in &frame.items[1..] {
            assert_eq!(item.properties.clip, Some(root));
            assert_eq!(item.properties.scroll, Some(root));
            assert!(Arc::ptr_eq(
                &item.commands,
                &before
                    .items
                    .iter()
                    .find(|old| old.node == item.node)
                    .unwrap()
                    .commands
            ));
        }
        assert_eq!(frame.hit_test(5.0, 5.0), Some(a));
        assert_eq!(frame.hit_test(25.0, 5.0), Some(b));
        assert_eq!(frame.hit_test(90.0, 5.0), None);
    }
    #[test]
    fn clip_changes_damage_old_and_new_ink_and_preserve_chunk_identity() {
        let (mut tree, a, _) = fixture();
        let before = tree.frame([160.0, 80.0]).unwrap();
        let mut tx = Transaction::default();
        tx.clip(
            a,
            Some(Rect {
                x: 10.0,
                y: 8.0,
                width: 20.0,
                height: 16.0,
            }),
        );
        tree.commit(tx, BackendCapabilities::default()).unwrap();
        let frame = tree.frame([160.0, 80.0]).unwrap();
        assert_eq!((frame.layout_runs, frame.recorded_chunks), (0, 0));
        assert!(Arc::ptr_eq(
            &frame.items[1].commands,
            &before.items[1].commands
        ));
        assert_eq!(frame.damage.bounds, Some(before.items[1].bounds));
        assert_eq!(frame.hit_test(2.0, 2.0), Some(tree.root()));
        assert_eq!(frame.hit_test(15.0, 12.0), Some(a));
        let mut tx = Transaction::default();
        tx.clip(
            a,
            Some(Rect {
                x: 10.0,
                y: 8.0,
                width: 20.0,
                height: 16.0,
            }),
        );
        assert!(
            !tree
                .commit(tx, BackendCapabilities::default())
                .unwrap()
                .request_frame
        );
    }
    #[test]
    fn movement_damage_contains_both_positions_and_invalid_batch_is_atomic() {
        let (mut tree, a, _) = fixture();
        tree.frame([160.0, 80.0]).unwrap();
        let mut tx = Transaction::default();
        tx.translation(a, [8.0, 10.0]);
        tree.commit(tx, BackendCapabilities::default()).unwrap();
        let frame = tree.frame([160.0, 80.0]).unwrap();
        assert_eq!(
            frame.damage,
            Damage {
                full: false,
                bounds: Some(Rect {
                    x: 0.0,
                    y: 0.0,
                    width: 72.0,
                    height: 42.0
                })
            }
        );
        let mut tx = Transaction::default();
        tx.color(a, 0);
        tx.clip(
            a,
            Some(Rect {
                width: -1.0,
                ..Default::default()
            }),
        );
        assert_eq!(
            tree.commit(tx, BackendCapabilities::default()).unwrap_err(),
            Error::InvalidClip
        );
        let mut tx = Transaction::default();
        tx.scroll(a, [f32::INFINITY, 0.0]);
        assert_eq!(
            tree.commit(tx, BackendCapabilities::default()).unwrap_err(),
            Error::InvalidScroll
        );
        let unchanged = tree.frame([160.0, 80.0]).unwrap();
        assert_eq!(unchanged.revision, frame.revision);
    }
}
