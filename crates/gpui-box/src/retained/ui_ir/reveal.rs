//! Post-layout nearest reveal. This runs only inside the ordinary private tree
//! candidate; authored offsets and its final frame are adopted together.
use super::*;

pub(super) struct RevealFrame<'a> {
    pub constraints: LayoutConstraints,
    pub viewport: [f32; 2],
    pub targets: &'a [UiNodeId],
    pub authored: &'a NodeMap,
}

#[derive(Default)]
pub(super) struct RevealResult {
    pub offsets: BTreeMap<UiNodeId, [f32; 2]>,
    pub commits: Vec<Commit>,
}

pub(super) fn apply(
    tree: &mut UiTree,
    mapping: &BTreeMap<UiNodeId, NodeId>,
    frame: &RevealFrame<'_>,
    capabilities: BackendCapabilities,
) -> Result<RevealResult, Error> {
    let external: BTreeMap<_, _> = mapping.iter().map(|(&a, &b)| (b, a)).collect();
    let targets: Vec<_> = frame.targets.iter().map(|id| mapping[id]).collect();
    let result = input::reveal::apply(
        tree,
        frame.constraints,
        frame.viewport,
        &targets,
        |node| {
            frame.authored[&external[&node]].styles.iter().any(|style| {
                style
                    .declarations()
                    .iter()
                    .any(|d| matches!(d, style::StyleDeclaration::Scroll(_)))
            })
        },
        capabilities,
    )?;
    Ok(RevealResult {
        offsets: result
            .offsets
            .into_iter()
            .map(|(node, offset)| (external[&node], offset))
            .collect(),
        commits: result.commits,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn snapped_publication_does_not_erase_offscreen_target_or_committed_scroll() {
        let fixed = |w, h| {
            LayoutStyle::column()
                .size(LayoutDimension::Px(w), LayoutDimension::Px(h))
                .flex_shrink(0.0)
        };
        let mut tree = UiTree::new_layout(
            fixed(100.0, 60.0).overflow(LayoutOverflow::Hidden, LayoutOverflow::Hidden),
            0,
        )
        .unwrap();
        tree.append_layout(tree.root(), fixed(100.0, 80.0), 0)
            .unwrap();
        let target = tree
            .append_layout(tree.root(), fixed(150.0, 30.0), 0xff0000ff)
            .unwrap();
        tree.set_pixel_snap(PixelSnap::DevicePixels { scale: 1.5 })
            .unwrap();
        let root_id = UiNodeId::new(0, 1);
        let target_id = UiNodeId::new(1, 1);
        let mapping = BTreeMap::from([(root_id, tree.root()), (target_id, target)]);
        let authored = BTreeMap::from([
            (root_id, Arc::new(UiNode::new(root_id, fixed(100.0, 60.0)))),
            (
                target_id,
                Arc::new(UiNode::new(target_id, fixed(150.0, 30.0))),
            ),
        ]);
        let frame = RevealFrame {
            constraints: LayoutConstraints::viewport(100.0, 60.0),
            viewport: [100.0, 60.0],
            targets: &[target_id],
            authored: &authored,
        };
        let result = apply(&mut tree, &mapping, &frame, BackendCapabilities::default()).unwrap();
        assert_eq!(result.offsets[&root_id], [0.0, 50.0]);
        assert_eq!(tree.node(tree.root()).unwrap().scroll, [0.0, 50.0]);
        let equal = apply(&mut tree, &mapping, &frame, BackendCapabilities::default()).unwrap();
        assert!(equal.offsets.is_empty());
    }
}
