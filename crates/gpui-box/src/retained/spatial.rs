//! Local placement survives inherited world-position and clip changes.
use super::*;

#[derive(Clone, Debug, PartialEq)]
pub struct SpatialState {
    pub parent: Option<NodeId>,
    pub layout_origin: [f32; 2],
    pub translation: [f32; 2],
    /// Clamped content scroll; affects children, not this node's own paint.
    pub scroll: [f32; 2],
    /// Clip in this node's local border coordinates.
    pub clip: Option<Rect>,
    /// The producer viewport, carried only by the root.
    pub root_clip: Option<Rect>,
}

#[cfg(all(feature = "retained-wgpu", not(target_family = "wasm")))]
impl SpatialState {
    pub(super) fn resolve(&self, parent: Option<&PaintItem>) -> ([f32; 2], Option<Rect>) {
        let origin = parent.map_or([0.0; 2], |parent| {
            [
                parent.bounds.x - parent.spatial.scroll[0],
                parent.bounds.y - parent.spatial.scroll[1],
            ]
        });
        let origin = [
            origin[0] + self.layout_origin[0] + self.translation[0],
            origin[1] + self.layout_origin[1] + self.translation[1],
        ];
        let inherited = parent.map_or(self.root_clip, |parent| parent.clip);
        let own = self.clip.map(|clip| clip.translated(origin));
        let clip = match (inherited, own) {
            (Some(a), Some(b)) => Some(a.intersect(b)),
            (a, b) => a.or(b),
        };
        (origin, clip)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_publication_includes_shadow_only_changes_in_authoritative_deltas() {
        let mut tree = UiTree::new(Style::default(), 0).unwrap();
        tree.set_frame_build_policy(FrameBuildPolicy::Full);
        let node = tree
            .append(tree.root(), Style::default(), 0xff0000ff)
            .unwrap();
        let first = tree.frame([100.0; 2]).unwrap();
        let mut tx = Transaction::default();
        tx.shadows(
            node,
            vec![BoxShadow {
                color: 0x112233ff,
                blur_radius: 2.0,
                ..Default::default()
            }],
        );
        tree.commit(tx, BackendCapabilities::default()).unwrap();
        let next = tree.frame([100.0; 2]).unwrap();
        let old = first.items.iter().find(|i| i.node == node).unwrap();
        let changed = next.items.iter().find(|i| i.node == node).unwrap();
        assert_eq!(old.commands, changed.commands);
        assert!(old.shadows.is_empty());
        assert_eq!(changed.shadows.len(), 1);
        assert!(
            next.updates
                .iter()
                .any(|i| i.node == node && i.shadows == changed.shadows)
        );
        assert_eq!(next.layout_runs, 0);
    }

    #[test]
    fn cancelling_inherited_motion_still_publishes_changed_local_placement() {
        let style = Style {
            size: Size {
                width: length(100.0),
                height: length(100.0),
            },
            ..Default::default()
        };
        let mut tree = UiTree::new(style.clone(), 0).unwrap();
        let parent = tree.append(tree.root(), style.clone(), 0).unwrap();
        let child = tree.append(parent, style, 0xff0000ff).unwrap();
        let first = tree.frame([200.0; 2]).unwrap();
        let old = first.items.iter().find(|item| item.node == child).unwrap();
        let mut tx = Transaction::default();
        tx.translation(parent, [8.0, 4.0]);
        tx.translation(child, [-8.0, -4.0]);
        tree.commit(tx, BackendCapabilities::default()).unwrap();
        let next = tree.frame([200.0; 2]).unwrap();
        let item = next.items.iter().find(|item| item.node == child).unwrap();
        assert_eq!(item.bounds, old.bounds);
        assert_ne!(item.spatial, old.spatial);
        assert!(Arc::ptr_eq(&item.commands, &old.commands));
        assert!(
            next.updates
                .iter()
                .any(|value| value.node == child && value.spatial == item.spatial)
        );
        assert_eq!(next.layout_runs, 0);
        assert_eq!(old.spatial.translation, [0.0; 2]);
    }
}
