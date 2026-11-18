//! Shared post-layout nearest reveal over owner-local complete geometry.
use super::*;
use std::collections::BTreeMap;

#[derive(Default)]
pub(in crate::retained) struct RevealResult {
    pub offsets: BTreeMap<NodeId, [f32; 2]>,
    pub commits: Vec<Commit>,
}

pub(in crate::retained) fn apply(
    tree: &mut UiTree,
    constraints: LayoutConstraints,
    viewport: [f32; 2],
    targets: &[NodeId],
    controlled: impl Fn(NodeId) -> bool,
    capabilities: BackendCapabilities,
) -> Result<RevealResult, Error> {
    let mut offsets = BTreeMap::new();
    let mut commits = vec![];
    for &target in targets {
        tree.local_frame_in_viewport(constraints, viewport)?;
        let path = geometry(tree, target)?;
        let target = path.last().ok_or(Error::InvalidNode)?;
        let mut bounds = target.bounds;
        if let Some(clip) = target.clip {
            bounds = bounds.intersect(clip.translated([bounds.x, bounds.y]));
        }
        let mut tx = Transaction::default();
        for host in path.iter().rev().skip(1) {
            if bounds.is_empty() {
                break;
            }
            let node = tree.node(host.node)?;
            let owned = controlled(host.node);
            let maximum = [
                node.published_layout.scroll_width(),
                node.published_layout.scroll_height(),
            ];
            // Pixel-snapped output bakes scroll into placement, so its spatial
            // scroll can be zero. The owner remains the authoritative offset.
            let before = [
                node.scroll[0].min(maximum[0]),
                node.scroll[1].min(maximum[1]),
            ];
            let mut next = before;
            if let Some(view) = host.overflow {
                let mut viewport = view.bounds.translated([host.bounds.x, host.bounds.y]);
                if let Some(clip) = host.clip {
                    viewport = viewport.intersect(clip.translated([host.bounds.x, host.bounds.y]));
                }
                let modes = [node.style.overflow.x, node.style.overflow.y];
                for axis in 0..2 {
                    if owned
                        || !matches!(
                            modes[axis],
                            taffy::Overflow::Hidden | taffy::Overflow::Scroll
                        )
                    {
                        continue;
                    }
                    let (start, size, low, span) = if axis == 0 {
                        (bounds.x, bounds.width, viewport.x, viewport.width)
                    } else {
                        (bounds.y, bounds.height, viewport.y, viewport.height)
                    };
                    let delta = nearest(start, size, low, span);
                    next[axis] = (f64::from(before[axis]) + delta)
                        .clamp(0.0, f64::from(maximum[axis]))
                        as f32;
                    let used = next[axis] - before[axis];
                    if axis == 0 {
                        bounds.x -= used;
                    } else {
                        bounds.y -= used;
                    }
                }
                // Outer containers reveal the portion that can actually pass
                // this viewport. A fixed Clip/style-owned host cannot be
                // bypassed by scrolling an ancestor toward hidden content.
                if view.axes[0] {
                    let x = bounds.x.max(viewport.x);
                    bounds.width = (bounds.x + bounds.width)
                        .min(viewport.x + viewport.width)
                        .max(x)
                        - x;
                    bounds.x = x;
                }
                if view.axes[1] {
                    let y = bounds.y.max(viewport.y);
                    bounds.height = (bounds.y + bounds.height)
                        .min(viewport.y + viewport.height)
                        .max(y)
                        - y;
                    bounds.y = y;
                }
            }
            if next != before {
                tx.scroll(host.node, next);
                offsets.insert(host.node, next);
            }
            if let Some(clip) = host.clip {
                bounds = bounds.intersect(clip.translated([host.bounds.x, host.bounds.y]));
            }
        }
        if !tx.patches.is_empty() {
            let commit = tree.commit(tx, capabilities)?;
            tree.local_frame_in_viewport(constraints, viewport)?
                .validate_geometry()?;
            commits.push(commit);
        }
    }
    Ok(RevealResult { offsets, commits })
}

struct RevealGeometry {
    node: NodeId,
    bounds: Rect,
    clip: Option<Rect>,
    overflow: Option<OverflowClip>,
}

// Read the same owner-local logical placement/order as extraction, before
// device-pixel projection bakes inherited clipping into output. A projected
// zero-size clip must not erase an offscreen target from reveal planning.
fn geometry(tree: &UiTree, target: NodeId) -> Result<Vec<RevealGeometry>, Error> {
    let mut ids = vec![];
    let mut next = Some(target);
    while let Some(id) = next {
        ids.push(id);
        next = tree.node(id)?.parent;
    }
    let mut origin = [0.0; 2];
    let mut path = Vec::with_capacity(ids.len());
    for id in ids.into_iter().rev() {
        let node = tree.node(id)?;
        let layout = &node.published_layout;
        let x = origin[0] + node.layout_origin[0] + node.translation[0];
        let y = origin[1] + node.layout_origin[1] + node.translation[1];
        path.push(RevealGeometry {
            node: id,
            bounds: Rect {
                x,
                y,
                width: layout.size.width,
                height: layout.size.height,
            },
            clip: node.clip,
            overflow: OverflowClip::from_layout(&node.style, layout),
        });
        origin = [
            x - node.scroll[0].min(layout.scroll_width()),
            y - node.scroll[1].min(layout.scroll_height()),
        ];
    }
    Ok(path)
}

fn nearest(start: f32, size: f32, low: f32, span: f32) -> f64 {
    let (start, size, low, span) = (
        f64::from(start),
        f64::from(size),
        f64::from(low),
        f64::from(span),
    );
    let end = start + size;
    let high = low + span;
    if start >= low && end <= high || start <= low && end >= high {
        return 0.0;
    }
    let leading = start - low;
    let trailing = end - high;
    if leading.abs() < trailing.abs() {
        leading
    } else {
        trailing
    }
}
