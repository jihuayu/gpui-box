//! Local placement survives inherited world-position and clip changes.
use super::*;

/// Axis-selective content viewport in local border coordinates. Overflow clips
/// content and descendants, while the node's border/background/shadows retain
/// the ordinary explicit/inherited clip. Unrestricted axes use that finite clip.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct OverflowClip {
    pub bounds: Rect,
    pub axes: [bool; 2],
}
impl OverflowClip {
    pub(super) fn from_layout(style: &taffy::Style, layout: &taffy::Layout) -> Option<Self> {
        let axes = [
            style.overflow.x != taffy::Overflow::Visible,
            style.overflow.y != taffy::Overflow::Visible,
        ];
        axes.iter().any(|axis| *axis).then(|| Self {
            bounds: Rect {
                x: layout.border.left
                    + if style.direction == taffy::Direction::Rtl {
                        layout.scrollbar_size.width
                    } else {
                        0.0
                    },
                y: layout.border.top,
                width: (layout.size.width
                    - layout.border.left
                    - layout.border.right
                    - layout.scrollbar_size.width)
                    .max(0.0),
                height: (layout.size.height
                    - layout.border.top
                    - layout.border.bottom
                    - layout.scrollbar_size.height)
                    .max(0.0),
            },
            axes,
        })
    }

    // Producers always install a finite root viewport. Consumers reject a
    // missing finite clip when overflow is present; no artificial infinity is
    // needed to express a Visible axis.
    pub(super) fn apply(self, inherited: Option<Rect>, origin: [f32; 2]) -> Option<Rect> {
        inherited.map(|mut clip| {
            let own = self.bounds.translated(origin);
            if self.axes[0] {
                let x = clip.x.max(own.x);
                clip.width = (clip.x + clip.width).min(own.x + own.width).max(x) - x;
                clip.x = x;
            }
            if self.axes[1] {
                let y = clip.y.max(own.y);
                clip.height = (clip.y + clip.height).min(own.y + own.height).max(y) - y;
                clip.y = y;
            }
            clip
        })
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct SpatialState {
    pub parent: Option<NodeId>,
    pub layout_origin: [f32; 2],
    pub translation: [f32; 2],
    /// Clamped content scroll; affects children, not this node's own paint.
    pub scroll: [f32; 2],
    /// Clip in this node's local border coordinates.
    pub clip: Option<Rect>,
    pub overflow_clip: Option<OverflowClip>,
    /// The producer viewport, carried only by the root.
    pub root_clip: Option<Rect>,
}

/// Shared producer check for both resolved and local atomic publications.
/// Validate composed geometry even when an inherited clip hides overflow.
pub(super) fn validate_geometry(
    bounds: Rect,
    content_bounds: Rect,
    clip: Option<Rect>,
    spatial: &SpatialState,
) -> Result<(), Error> {
    let origin = [bounds.x, bounds.y];
    for rect in [
        Some(bounds),
        Some(content_bounds),
        clip,
        spatial.clip.map(|rect| rect.translated(origin)),
        spatial.root_clip,
        spatial
            .overflow_clip
            .map(|value| value.bounds.translated(origin)),
    ]
    .into_iter()
    .flatten()
    {
        if !rect.valid() {
            return Err(Error::InvalidFrame);
        }
    }
    if spatial.overflow_clip.is_some() && clip.is_none() {
        return Err(Error::InvalidFrame);
    }
    if spatial
        .layout_origin
        .iter()
        .chain(&spatial.translation)
        .chain(&spatial.scroll)
        .any(|value| !value.is_finite())
        || !(bounds.x - spatial.scroll[0]).is_finite()
        || !(bounds.y - spatial.scroll[1]).is_finite()
    {
        return Err(Error::InvalidFrame);
    }
    Ok(())
}

impl SpatialState {
    pub(super) fn content_clip(&self, origin: [f32; 2], clip: Option<Rect>) -> Option<Rect> {
        self.overflow_clip
            .map_or(clip, |value| value.apply(clip, origin))
    }
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
        let inherited = parent.map_or(self.root_clip, |parent| parent.content_clip);
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
