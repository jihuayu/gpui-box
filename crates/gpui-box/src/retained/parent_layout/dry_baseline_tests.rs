use super::*;

#[derive(Debug)]
struct Mirror;
fn child_input(input: ParentLayoutInput) -> ChildLayoutInput {
    ChildLayoutInput {
        constraints: LayoutConstraints::new(input.available.width, LayoutAvailableSpace::Unbounded),
        percentage_basis: input.percentage_basis,
    }
}
impl ParentMeasure for Mirror {
    fn measure(
        &self,
        input: ParentLayoutInput,
        children: &mut dyn ParentLayoutChildren,
    ) -> Result<ParentMeasureOutput, Error> {
        let child = children.measure_baselines(0, child_input(input))?;
        Ok(ParentMeasureOutput {
            size: [
                input.known_size.width.unwrap_or(child.size[0]),
                input.known_size.height.unwrap_or(child.size[1]),
            ],
            baseline: child.baseline,
            last_baseline: child.last_baseline,
        })
    }
}
impl ParentLayout for Mirror {
    fn layout(
        &self,
        input: ParentLayoutInput,
        children: &mut dyn ParentLayoutChildren,
    ) -> Result<ParentLayoutOutput, Error> {
        let metrics = self.measure(input, children)?;
        Ok(ParentLayoutOutput {
            size: metrics.size,
            baseline: metrics.baseline,
            last_baseline: metrics.last_baseline,
            placements: vec![ParentPlacement {
                child: 0,
                input: child_input(input),
                origin: [0.0; 2],
            }],
        })
    }
}
fn fixture(custom: bool) -> (UiTree, NodeId, NodeId) {
    let mut tree = UiTree::new_layout(LayoutStyle::column(), 0).unwrap();
    let host = tree
        .append_layout(
            tree.root(),
            LayoutStyle::column()
                .size(LayoutDimension::Px(80.0), LayoutDimension::Auto)
                .align_items(LayoutAlignItems::Start)
                .padding(LayoutEdges::all(LayoutLength::Px(2.0))),
            0,
        )
        .unwrap();
    let row = tree
        .append_layout(
            host,
            LayoutStyle::row()
                .size(LayoutDimension::Px(50.0), LayoutDimension::Px(30.0))
                .align_items(LayoutAlignItems::Start)
                .padding(LayoutEdges::all(LayoutLength::Px(3.0))),
            0,
        )
        .unwrap();
    let leaf = tree
        .append_layout(
            row,
            LayoutStyle::row().size(LayoutDimension::Px(20.0), LayoutDimension::Px(10.0)),
            0xff0000ff,
        )
        .unwrap();
    tree.append_layout(
        tree.root(),
        LayoutStyle::row().size(LayoutDimension::Px(15.0), LayoutDimension::Px(8.0)),
        0x00ff00ff,
    )
    .unwrap();
    let mut tx = Transaction::default();
    tx.measure(
        leaf,
        Some(MeasureSnapshot::FixedBaselines(BaselineMeasureSnapshot {
            width: 20.0,
            height: 10.0,
            first_baseline: Some(3.0),
            last_baseline: Some(7.0),
        })),
    );
    if custom {
        tx.parent_layout(
            host,
            Some(ParentLayoutSnapshot::new(Mirror).with_measure(Mirror)),
        );
    }
    tree.commit(tx, BackendCapabilities::default()).unwrap();
    (tree, host, row)
}
fn input() -> ChildLayoutInput {
    ChildLayoutInput::new(LayoutConstraints::new(
        LayoutAvailableSpace::Definite(100.0),
        LayoutAvailableSpace::Unbounded,
    ))
}

#[test]
fn explicit_dry_baselines_match_nested_final_flex_and_keep_published_frames() {
    for custom in [false, true] {
        let (mut tree, host, row) = fixture(custom);
        let before = tree.dry_measure_node_baselines(host, input()).unwrap();
        assert_eq!(before.size, [80.0, 34.0]);
        assert_eq!(
            (before.baseline, before.last_baseline),
            (Some(8.0), Some(12.0))
        );
        let frame = tree.local_frame([120.0; 2]).unwrap();
        let published = tree.layout_result(host).unwrap();
        assert_eq!(published.size, before.size);
        assert_eq!(
            (published.baseline, published.last_baseline),
            (before.baseline, before.last_baseline)
        );
        assert_eq!(
            tree.dry_measure_node_baselines(host, input()).unwrap(),
            before
        );
        let row_query = tree.dry_measure_node_baselines(row, input()).unwrap();
        assert_eq!(
            (row_query.baseline, row_query.last_baseline),
            (Some(6.0), Some(10.0))
        );
        assert_eq!(tree.layout_result(host).unwrap(), published);
        assert_eq!(
            tree.local_frame([120.0; 2]).unwrap().resolve().layout_stats,
            LayoutStats::default()
        );
        let (mut reference, _, _) = fixture(false);
        reference.set_frame_build_policy(FrameBuildPolicy::Full);
        let expected = super::tests::pixels(reference.frame([120.0; 2]).unwrap());
        assert_eq!(super::tests::pixels(frame.resolve()), expected);
        drop(tree);
        assert_eq!(super::tests::pixels(frame.resolve()), expected);
    }
}

#[test]
fn explicit_baseline_queries_preserve_enforced_inline_and_full_assignments() {
    for full in [false, true] {
        let (mut tree, host, row) = fixture(true);
        let mut snapshot = ParentLayoutSnapshot::new(Mirror).with_measure(Mirror);
        snapshot = if full {
            snapshot.with_allocation_layout(Full)
        } else {
            snapshot.with_inline_layout(Inline)
        };
        let mut tx = Transaction::default();
        tx.parent_layout(host, Some(snapshot));
        tx.parent_data(row, Some(ParentData::new(40.0f32)));
        tree.commit(tx, BackendCapabilities::default()).unwrap();
        let result = tree.dry_measure_node(host, input()).unwrap();
        tree.frame([120.0; 2]).unwrap();
        assert_eq!(
            tree.layout_result(row).unwrap().size[0],
            if full { 40.0 } else { 30.0 }
        );
        let final_result = tree.layout_result(host).unwrap();
        assert_eq!(
            (final_result.baseline, final_result.last_baseline),
            (result.baseline, result.last_baseline)
        );
    }
}

#[test]
fn custom_parent_complete_metrics_drive_real_outer_first_and_last_alignment() {
    for align in [LayoutAlignItems::Baseline, LayoutAlignItems::LastBaseline] {
        let (mut tree, host, row) = fixture(true);
        let (mut full, full_host, _) = fixture(false);
        full.set_frame_build_policy(FrameBuildPolicy::Full);
        for owner in [&mut tree, &mut full] {
            let mut tx = Transaction::default();
            tx.layout_style(
                owner.root(),
                LayoutStyle::row()
                    .size(LayoutDimension::Px(120.0), LayoutDimension::Px(80.0))
                    .align_items(align),
            );
            owner.commit(tx, BackendCapabilities::default()).unwrap();
        }
        let leaf = tree.node(row).unwrap().children[0];
        let full_row = full.node(full_host).unwrap().children[0];
        let full_leaf = full.node(full_row).unwrap().children[0];
        for baseline in [3.0, 5.0, 3.0] {
            for (owner, leaf) in [(&mut tree, leaf), (&mut full, full_leaf)] {
                let mut tx = Transaction::default();
                tx.measure(
                    leaf,
                    Some(MeasureSnapshot::FixedBaselines(BaselineMeasureSnapshot {
                        width: 20.0,
                        height: 10.0,
                        first_baseline: Some(baseline),
                        last_baseline: Some(7.0),
                    })),
                );
                owner.commit(tx, BackendCapabilities::default()).unwrap();
            }
            let frame = tree.local_frame([120.0, 80.0]).unwrap();
            let expected = full.frame([120.0, 80.0]).unwrap();
            let actual = tree.layout_result(host).unwrap();
            let reference = full.layout_result(full_host).unwrap();
            assert_eq!(actual, reference);
            assert_eq!(
                super::tests::pixels(frame.resolve()),
                super::tests::pixels(expected)
            );
        }
    }
}

#[test]
fn explicit_baseline_query_invalid_input_and_containment_are_preserved() {
    let (mut tree, host, _) = fixture(true);
    let old = tree.local_frame([120.0; 2]).unwrap();
    let mut tx = Transaction::default();
    tx.layout_style(
        host,
        LayoutStyle::column()
            .size(LayoutDimension::Px(80.0), LayoutDimension::Px(34.0))
            .container_type(LayoutContainerType::Size),
    );
    tree.commit(tx, BackendCapabilities::default()).unwrap();
    let query = tree.dry_measure_node_baselines(host, input()).unwrap();
    assert_eq!((query.baseline, query.last_baseline), (None, None));
    assert_eq!(
        tree.published_local_frame().unwrap().revision(),
        old.revision()
    );
    let mut bad = input();
    bad.percentage_basis.width = Some(f32::NAN);
    assert!(tree.dry_measure_node_baselines(host, bad).is_err());
    assert!(
        tree.dry_measure_node_baselines(
            NodeId {
                tree: tree.root().tree,
                slot: usize::MAX,
                generation: 0
            },
            input()
        )
        .is_err()
    );
}

#[derive(Debug)]
struct Invalid;
impl ParentLayout for Invalid {
    fn layout(
        &self,
        _: ParentLayoutInput,
        _: &mut dyn ParentLayoutChildren,
    ) -> Result<ParentLayoutOutput, Error> {
        Err(Error::InvalidParentLayout)
    }
}
#[test]
fn nested_baseline_error_rejects_atomic_edit_and_keeps_old_frame() {
    use crate::retained::{logical::LogicalUi, reactive::Property};
    let mut ui =
        LogicalUi::new_layout(LayoutStyle::column(), 0, BackendCapabilities::default()).unwrap();
    let scope = ui.root_scope();
    let root = ui.root_render();
    let host = ui
        .append_render_layout(scope, root, LayoutStyle::column(), 0)
        .unwrap();
    let row = ui
        .append_render_layout(scope, host, LayoutStyle::row(), 0)
        .unwrap();
    ui.append_render_layout(
        scope,
        row,
        LayoutStyle::row().size(LayoutDimension::Px(20.0), LayoutDimension::Px(10.0)),
        1,
    )
    .unwrap();
    let source = ui
        .state(scope, 0, || {
            Some(ParentLayoutSnapshot::new(Mirror).with_measure(Mirror))
        })
        .unwrap();
    ui.bind(source, Property::parent_layout(row)).unwrap();
    ui.edit(
        |_| Ok(()),
        |(), edit| {
            edit.properties().parent_layout(
                host,
                Some(ParentLayoutSnapshot::new(Mirror).with_measure(Mirror)),
            );
            Ok(())
        },
    )
    .unwrap();
    let old = ui.local_frame([120.0; 2]).unwrap();
    let geometry = ui.layout_result(host).unwrap();
    assert!(
        ui.edit_local_frame(
            LayoutConstraints::viewport(120.0, 120.0),
            [120.0; 2],
            |tx| {
                tx.set(source, Some(ParentLayoutSnapshot::new(Invalid)))?;
                Ok(())
            },
            |(), _| Ok(())
        )
        .is_err()
    );
    assert_eq!(ui.layout_result(host).unwrap(), geometry);
    assert!(ui.read(source).unwrap().as_ref().unwrap().has_measure());
    assert_eq!(
        ui.published_local_frame().unwrap().revision(),
        old.revision()
    );
    assert!(ui.dry_measure_node_baselines(host, input()).is_ok());
}

#[cfg(feature = "retained-wgpu")]
#[test]
#[ignore = "requires a WGPU adapter; run explicitly"]
fn hardware_baseline_queries_and_nested_parent_match_ordinary_full_flex() {
    let (mut tree, host, row) = fixture(true);
    let (mut full, _, full_row) = fixture(false);
    full.set_frame_build_policy(FrameBuildPolicy::Full);
    let leaf = tree.node(row).unwrap().children[0];
    let reference_leaf = full.node(full_row).unwrap().children[0];
    let mut gpu = crate::retained::wgpu::WgpuFrameRenderer::new(2).unwrap();
    eprintln!("dry baseline adapter: {:?}", gpu.adapter_info());
    for baseline in [3.0, 5.0, 3.0] {
        for (owner, node) in [(&mut tree, leaf), (&mut full, reference_leaf)] {
            let mut tx = Transaction::default();
            tx.measure(
                node,
                Some(MeasureSnapshot::FixedBaselines(BaselineMeasureSnapshot {
                    width: 20.0,
                    height: 10.0,
                    first_baseline: Some(baseline),
                    last_baseline: Some(7.0),
                })),
            );
            owner.commit(tx, BackendCapabilities::default()).unwrap();
        }
        let query = tree.dry_measure_node_baselines(host, input()).unwrap();
        let frame = tree.local_frame([120.0; 2]).unwrap();
        assert_eq!(tree.layout_result(host).unwrap().baseline, query.baseline);
        gpu.submit_local(&frame, [120; 2], 1.0).unwrap().unwrap();
        assert_eq!(
            gpu.image().unwrap().as_raw(),
            gpu.reference_image(&full.frame([120.0; 2]).unwrap(), [120; 2], 1.0)
                .unwrap()
                .as_raw()
        );
    }
    gpu.finish().unwrap();
}

#[derive(Debug)]
struct Inline;
impl ParentInlineLayout for Inline {
    fn allocate(
        &self,
        _: ParentInlineInput,
        count: usize,
    ) -> Result<Vec<ChildInlineAllocation>, Error> {
        Ok(vec![
            ChildInlineAllocation {
                width: 30.0,
                percentage_basis: Some(80.0)
            };
            count
        ])
    }
}
#[derive(Debug)]
struct Full;
impl ParentAllocationLayout for Full {
    fn allocate_children(
        &self,
        _: ParentAllocationInput,
        children: &dyn ParentAllocationChildren,
    ) -> Result<Vec<ChildLayoutInput>, Error> {
        (0..children.len())
            .map(|child| {
                let data = children
                    .parent_data(child)?
                    .ok_or(Error::InvalidParentLayout)?;
                let width = *data.get::<f32>().ok_or(Error::InvalidParentLayout)?;
                Ok(ChildLayoutInput::new(
                    LayoutConstraints::new(
                        LayoutAvailableSpace::Definite(width),
                        LayoutAvailableSpace::Unbounded,
                    )
                    .with_bounds(BoxConstraints::new([width, 0.0], [width, f32::INFINITY])),
                ))
            })
            .collect()
    }
}
