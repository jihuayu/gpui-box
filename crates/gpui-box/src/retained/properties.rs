//! Shared sparse visual-property chains and conservative frame damage.
use super::*;
mod store;
pub use store::{PropertyIter, PropertyMap};

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
    pub effect: Option<NodeId>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PropertyNode<T> {
    pub parent: Option<NodeId>,
    pub value: T,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct PropertyTrees {
    pub transforms: PropertyMap<[f32; 2]>,
    pub clips: PropertyMap<Rect>,
    pub scrolls: PropertyMap<[f32; 2]>,
    pub effects: PropertyMap<f32>,
}

/// Private publication batch, shared across disjoint dirty subtrees. Dense
/// property animation must not repeatedly copy and rebalance persistent paths.
#[derive(Default)]
pub(super) struct PropertyEdits {
    pub transforms: Vec<(NodeId, Option<PropertyNode<[f32; 2]>>)>,
    pub clips: Vec<(NodeId, Option<PropertyNode<Rect>>)>,
    pub scrolls: Vec<(NodeId, Option<PropertyNode<[f32; 2]>>)>,
    pub effects: Vec<(NodeId, Option<PropertyNode<f32>>)>,
}
impl PropertyEdits {
    pub fn apply(self, trees: &mut PropertyTrees) {
        trees.transforms.apply_edits(self.transforms);
        trees.clips.apply_edits(self.clips);
        trees.scrolls.apply_edits(self.scrolls);
        trees.effects.apply_edits(self.effects);
    }
}

pub(super) trait PropertyOutput {
    fn record(
        &mut self,
        id: NodeId,
        transform: Option<PropertyNode<[f32; 2]>>,
        clip: Option<PropertyNode<Rect>>,
        scroll: Option<PropertyNode<[f32; 2]>>,
        effect: Option<PropertyNode<f32>>,
    );
}

impl PropertyOutput for PropertyTrees {
    fn record(
        &mut self,
        id: NodeId,
        transform: Option<PropertyNode<[f32; 2]>>,
        clip: Option<PropertyNode<Rect>>,
        scroll: Option<PropertyNode<[f32; 2]>>,
        effect: Option<PropertyNode<f32>>,
    ) {
        fn set<T: Clone + PartialEq>(
            map: &mut PropertyMap<T>,
            id: NodeId,
            value: Option<PropertyNode<T>>,
        ) {
            if let Some(value) = value {
                map.insert(id, value);
            } else {
                map.remove(&id);
            }
        }
        set(&mut self.transforms, id, transform);
        set(&mut self.clips, id, clip);
        set(&mut self.scrolls, id, scroll);
        set(&mut self.effects, id, effect);
    }
}

#[derive(Default)]
pub(super) struct PropertyBuilder {
    transforms: Vec<(NodeId, PropertyNode<[f32; 2]>)>,
    clips: Vec<(NodeId, PropertyNode<Rect>)>,
    scrolls: Vec<(NodeId, PropertyNode<[f32; 2]>)>,
    effects: Vec<(NodeId, PropertyNode<f32>)>,
}
impl PropertyOutput for PropertyBuilder {
    fn record(
        &mut self,
        id: NodeId,
        transform: Option<PropertyNode<[f32; 2]>>,
        clip: Option<PropertyNode<Rect>>,
        scroll: Option<PropertyNode<[f32; 2]>>,
        effect: Option<PropertyNode<f32>>,
    ) {
        if let Some(value) = transform {
            self.transforms.push((id, value));
        }
        if let Some(value) = clip {
            self.clips.push((id, value));
        }
        if let Some(value) = scroll {
            self.scrolls.push((id, value));
        }
        if let Some(value) = effect {
            self.effects.push((id, value));
        }
    }
}
impl PropertyBuilder {
    pub(super) fn finish(self) -> PropertyTrees {
        PropertyTrees {
            transforms: PropertyMap::from_entries(self.transforms),
            clips: PropertyMap::from_entries(self.clips),
            scrolls: PropertyMap::from_entries(self.scrolls),
            effects: PropertyMap::from_entries(self.effects),
        }
    }
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
        let mut quad = (!bounds.is_empty()
            && (self.image.is_some() || self.commands.iter().any(|command| self.visible(command))))
        .then_some(bounds);
        for shadow in self.shadows.iter().filter(|shadow| shadow.color & 255 != 0) {
            let ink = self.shadow_geometry(*shadow).ink;
            let ink = self.clip.map_or(ink, |clip| ink.intersect(clip));
            if !ink.is_empty() {
                quad = Some(quad.map_or(ink, |old| old.union(ink)));
            }
        }
        #[cfg(all(feature = "retained-text", not(target_family = "wasm")))]
        if self.text.as_ref().is_some_and(|text| text.color & 255 != 0)
            && let Some(ink) = self.glyphs.as_ref().and_then(|chunk| chunk.ink)
        {
            let ink = ink.translated([self.content_bounds.x, self.content_bounds.y]);
            let ink = self.clip.map_or(ink, |clip| ink.intersect(clip));
            if !ink.is_empty() {
                return Some(quad.map_or(ink, |quad| quad.union(ink)));
            }
        }
        quad
    }
    #[cfg(not(all(feature = "retained-text", not(target_family = "wasm"))))]
    pub(super) fn same_text(&self, _other: &Self) -> bool {
        true
    }
}

pub(super) fn damage(
    previous: Option<&FramePacket>,
    items: &[PaintItem],
    properties: &PropertyTrees,
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
                a.bounds == b.bounds
                    && a.clip == b.clip
                    && a.commands == b.commands
                    && a.fill_color == b.fill_color
                    && a.shadows == b.shadows
                    && a.image == b.image
                    && a.same_text(b)
                    && a.content_bounds == b.content_bounds
                    && same_effects(
                        a.properties.effect,
                        b.properties.effect,
                        &previous.properties,
                        properties,
                    )
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

pub(super) fn same_effects(
    mut a: Option<NodeId>,
    mut b: Option<NodeId>,
    old: &PropertyTrees,
    new: &PropertyTrees,
) -> bool {
    while let Some(id) = a {
        if b != a {
            return false;
        }
        let left = &old.effects[&id];
        let right = &new.effects[&id];
        if left.value != right.value {
            return false;
        }
        a = left.parent;
        b = right.parent;
    }
    b.is_none()
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
    fn group_opacity_reuses_commands_and_damages_its_descendants() {
        let (mut tree, a, _) = fixture();
        let before = tree.frame([160.0, 80.0]).unwrap();
        let mut tx = Transaction::default();
        tx.opacity(a, 0.5);
        tree.commit(tx, BackendCapabilities::default()).unwrap();
        let frame = tree.frame([160.0, 80.0]).unwrap();
        assert_eq!(
            (
                frame.layout_runs,
                frame.recorded_chunks,
                frame.updates.len()
            ),
            (0, 0, 0)
        );
        assert!(Arc::ptr_eq(
            &before.items[1].commands,
            &frame.items[1].commands
        ));
        assert_eq!(frame.items[1].properties.effect, Some(a));
        assert_eq!(frame.properties.effects[&a].value, 0.5);
        assert_eq!(frame.damage.bounds, Some(frame.items[1].bounds));
        let mut tx = Transaction::default();
        tx.color(a, 0);
        tx.opacity(a, f32::NAN);
        assert_eq!(
            tree.commit(tx, BackendCapabilities::default()).unwrap_err(),
            Error::InvalidOpacity
        );
        let mut tx = Transaction::default();
        tx.opacity(a, 0.0);
        tx.opacity(a, 0.5);
        assert!(
            !tree
                .commit(tx, BackendCapabilities::default())
                .unwrap()
                .request_frame
        );
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
        for item in frame.items.iter().skip(1) {
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
