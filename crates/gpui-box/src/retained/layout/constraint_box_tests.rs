use super::*;
use crate::retained::{
    BackendCapabilities, CustomMeasureSnapshot, Error, FrameBuildPolicy, MeasureSnapshot,
    Transaction, UiTree,
};

fn fixed(w: f32, h: f32) -> LayoutStyle {
    LayoutStyle::row()
        .size(Dimension::Px(w), Dimension::Px(h))
        .flex_shrink(0.0)
}
fn unbounded() -> LayoutConstraints {
    LayoutConstraints::new(AvailableSpace::Unbounded, AvailableSpace::Unbounded)
}
fn box_style(width: ConstraintFit, height: ConstraintFit) -> LayoutStyle {
    LayoutStyle::constraint_box(width, height)
}

#[test]
fn nested_loose_and_tight_boxes_deflate_padding_and_enforce_child_bounds() {
    for fit in [ConstraintFit::Loose, ConstraintFit::Tight] {
        let mut tree = UiTree::new_layout(box_style(fit, ConstraintFit::Loose), 0).unwrap();
        let inner = tree
            .append_layout(
                tree.root(),
                box_style(ConstraintFit::Tight, ConstraintFit::Loose)
                    .padding(Edges::all(Length::Px(10.0)))
                    .size(Dimension::Px(80.0), Dimension::Auto),
                0,
            )
            .unwrap();
        let flex = tree
            .append_layout(
                inner,
                LayoutStyle::row()
                    .flex_wrap(FlexWrap::Wrap)
                    .align_items(AlignItems::Start),
                0,
            )
            .unwrap();
        let mut children = vec![];
        for _ in 0..3 {
            children.push(
                tree.append_layout(flex, fixed(30.0, 20.0), 0xff0000ff)
                    .unwrap(),
            );
        }
        tree.frame_in_viewport(
            unbounded().with_bounds(BoxConstraints::tight([100.0, 100.0])),
            [150.0; 2],
        )
        .unwrap();
        let width = if fit == ConstraintFit::Loose {
            80.0
        } else {
            100.0
        };
        assert_eq!(tree.layout_result(inner).unwrap().size, [width, 60.0]);
        assert_eq!(tree.layout_result(flex).unwrap().size, [width - 20.0, 40.0]);
        assert_eq!(tree.layout_result(flex).unwrap().location, [10.0, 10.0]);
        assert_eq!(
            tree.layout_result(children[2]).unwrap().location,
            [0.0, 20.0]
        );
    }
}

#[test]
fn tight_unbounded_does_not_expand_to_infinity_and_empty_boxes_are_finite() {
    for fit in [ConstraintFit::Loose, ConstraintFit::Tight] {
        let mut tree =
            UiTree::new_layout(box_style(fit, fit).padding(Edges::all(Length::Px(3.0))), 0)
                .unwrap();
        tree.frame_in_viewport(unbounded(), [100.0; 2]).unwrap();
        assert_eq!(tree.layout_result(tree.root()).unwrap().size, [6.0; 2]);
        let child = tree
            .append_layout(tree.root(), fixed(20.0, 30.0), 1)
            .unwrap();
        tree.frame_in_viewport(unbounded(), [100.0; 2]).unwrap();
        assert_eq!(tree.layout_result(tree.root()).unwrap().size, [26.0, 36.0]);
        assert_eq!(tree.layout_result(child).unwrap().size, [20.0, 30.0]);
    }
}

#[test]
fn extreme_padding_hard_max_and_child_style_limits_cannot_escape_allocation() {
    let mut tree = UiTree::new_layout(
        box_style(ConstraintFit::Tight, ConstraintFit::Loose).padding(Edges::all(Length::Px(30.0))),
        0,
    )
    .unwrap();
    let child = tree
        .append_layout(
            tree.root(),
            fixed(200.0, 200.0).min_size(Dimension::Px(300.0), Dimension::Px(300.0)),
            1,
        )
        .unwrap();
    tree.frame_in_viewport(
        unbounded().with_bounds(BoxConstraints::tight([20.0, 15.0])),
        [100.0; 2],
    )
    .unwrap();
    assert_eq!(tree.layout_result(tree.root()).unwrap().size, [20.0, 15.0]);
    assert_eq!(tree.layout_result(child).unwrap().size, [0.0; 2]);
}

#[test]
fn boundary_inside_flex_respects_sibling_allocation_and_publishes_baseline() {
    let mut tree =
        UiTree::new_layout(fixed(100.0, 40.0).align_items(AlignItems::Baseline), 0).unwrap();
    let host = tree
        .append_layout(
            tree.root(),
            box_style(ConstraintFit::Tight, ConstraintFit::Loose)
                .size(Dimension::Px(0.0), Dimension::Auto)
                .min_size(Dimension::Px(0.0), Dimension::Auto)
                .flex_grow(1.0)
                .padding(Edges::all(Length::Px(2.0))),
            0,
        )
        .unwrap();
    let leaf = tree.append_layout(host, LayoutStyle::row(), 1).unwrap();
    let sibling = tree
        .append_layout(tree.root(), fixed(30.0, 15.0), 2)
        .unwrap();
    let mut tx = Transaction::default();
    tx.measure(
        leaf,
        Some(MeasureSnapshot::Fixed(CustomMeasureSnapshot {
            width: 200.0,
            height: 20.0,
            baseline: Some(12.0),
        })),
    );
    tree.commit(tx, BackendCapabilities::default()).unwrap();
    tree.frame([100.0, 40.0]).unwrap();
    assert_eq!(tree.layout_result(host).unwrap().size, [70.0, 24.0]);
    assert_eq!(tree.layout_result(leaf).unwrap().size, [66.0, 20.0]);
    assert_eq!(tree.layout_result(host).unwrap().baseline, Some(14.0));
    assert_eq!(tree.layout_result(sibling).unwrap().location[0], 70.0);
}

#[test]
fn invalid_arity_measure_and_oof_edits_reject_atomically() {
    let mut tree =
        UiTree::new_layout(box_style(ConstraintFit::Loose, ConstraintFit::Loose), 0).unwrap();
    let root = tree.root();
    let child = tree.append_layout(root, fixed(20.0, 20.0), 1).unwrap();
    let old = tree.frame([100.0; 2]).unwrap();
    assert!(matches!(
        tree.append_layout(root, fixed(10.0, 10.0), 1),
        Err(Error::InvalidConstraintBox)
    ));
    let mut tx = Transaction::default();
    let mut absolute = fixed(20.0, 20.0);
    absolute.position = LayoutPosition::Absolute;
    tx.layout_style(child, absolute);
    tx.color(child, 0xff0000ff);
    assert!(matches!(
        tree.commit(tx, BackendCapabilities::default()),
        Err(Error::InvalidConstraintBox)
    ));
    assert_eq!(tree.revision, old.revision);
    let failed = tree.edit(BackendCapabilities::default(), |draft| {
        draft.append_layout(root, fixed(10.0, 10.0), 1)?;
        Ok(())
    });
    assert!(matches!(failed, Err(Error::InvalidConstraintBox)));
    assert_eq!(tree.revision, old.revision);
    tree.remove(child).unwrap();
    let mut tx = Transaction::default();
    tx.measure(
        root,
        Some(MeasureSnapshot::Fixed(CustomMeasureSnapshot {
            width: 1.0,
            height: 1.0,
            baseline: None,
        })),
    );
    assert!(matches!(
        tree.commit(tx, BackendCapabilities::default()),
        Err(Error::InvalidConstraintBox)
    ));
}

#[test]
fn protocol_can_change_with_topology_in_one_validated_edit() {
    let mut tree = UiTree::new_layout(fixed(100.0, 50.0), 0).unwrap();
    let root = tree.root();
    let a = tree.append_layout(root, fixed(20.0, 20.0), 1).unwrap();
    let b = tree.append_layout(root, fixed(20.0, 20.0), 2).unwrap();
    tree.frame([100.0, 50.0]).unwrap();
    tree.edit(BackendCapabilities::default(), |draft| {
        draft.remove(b)?;
        draft.properties().layout_style(
            root,
            box_style(ConstraintFit::Tight, ConstraintFit::Loose)
                .size(Dimension::Px(100.0), Dimension::Px(50.0)),
        );
        Ok(())
    })
    .unwrap();
    tree.frame([100.0, 50.0]).unwrap();
    assert_eq!(tree.layout_result(a).unwrap().size, [100.0, 20.0]);
    assert!(tree.layout_result(b).is_err());
}

#[test]
fn local_and_full_output_match_cpu_pixels_hit_testing_old_frames_and_measure_only() {
    fn fixture() -> (UiTree, crate::retained::NodeId) {
        let mut tree = UiTree::new_layout(
            box_style(ConstraintFit::Tight, ConstraintFit::Loose)
                .padding(Edges::all(Length::Px(5.0))),
            0,
        )
        .unwrap();
        let child = tree
            .append_layout(tree.root(), fixed(300.0, 20.0), 0xff0000ff)
            .unwrap();
        (tree, child)
    }
    fn pixels(frame: crate::retained::FramePacket) -> Vec<u8> {
        use crate::retained::{pacing, software};
        let mut job = software::SoftwareJob::new(
            frame,
            software::SoftwareOptions {
                size: [160, 100],
                scale: 1.0,
                background: 0,
                effects: pacing::SoftwareEffects::Exact,
                max_bytes: 1024 * 1024,
            },
        )
        .unwrap();
        while !job.step(4096).unwrap() {}
        job.into_frame().unwrap().pixels().to_vec()
    }
    let (mut fast, leaf) = fixture();
    let (mut full, _) = fixture();
    full.frame_build_policy = FrameBuildPolicy::Full;
    let mut old = None;
    for width in [80.0, 40.0, 110.0, 40.0] {
        let constraints = unbounded().with_bounds(BoxConstraints::tight([width, 70.0]));
        let local = fast
            .local_frame_in_viewport(constraints, [160.0, 100.0])
            .unwrap();
        let reference = full.frame_in_viewport(constraints, [160.0, 100.0]).unwrap();
        let actual = pixels(local.resolve());
        assert_eq!(actual, pixels(reference));
        assert_eq!(
            &actual[(10 * 160 + 10) * 4..(10 * 160 + 10) * 4 + 4],
            &[255, 0, 0, 255]
        );
        assert_eq!(
            &actual[(10 * 160 + width as usize) * 4..(10 * 160 + width as usize) * 4 + 4],
            &[0, 0, 0, 0]
        );
        assert_eq!(local.hit_test(10.0, 10.0), Some(leaf));
        assert_ne!(local.hit_test(width - 2.0, 10.0), Some(leaf));
        assert_eq!(
            fast.frame_in_viewport(constraints, [160.0, 100.0])
                .unwrap()
                .layout_runs,
            0
        );
        let before = fast.layout_result(leaf).unwrap();
        let measured = fast
            .measure_with_constraints(unbounded().with_bounds(BoxConstraints::tight([30.0, 40.0])))
            .unwrap();
        assert_eq!(measured.size, [30.0, 40.0]);
        assert_eq!(fast.layout_result(leaf).unwrap(), before);
        if old.is_none() {
            old = Some(local);
        }
    }
    assert_eq!(
        old.unwrap().resolve_geometry(leaf).unwrap().bounds.width,
        70.0
    );
}

#[test]
fn percentage_descendants_rtl_hidden_and_nested_absolute_outputs_use_the_chosen_box() {
    let mut style = box_style(ConstraintFit::Tight, ConstraintFit::Tight)
        .size(Dimension::Px(100.0), Dimension::Px(60.0))
        .padding(Edges::all(Length::Px(10.0)));
    style.position = LayoutPosition::Relative;
    let mut tree = UiTree::new_layout(style.clone(), 0).unwrap();
    let child = tree
        .append_layout(tree.root(), LayoutStyle::row(), 0)
        .unwrap();
    let percent = tree
        .append_layout(
            child,
            LayoutStyle::row().size(Dimension::Percent(0.5), Dimension::Percent(0.5)),
            1,
        )
        .unwrap();
    let mut absolute_style = fixed(10.0, 10.0);
    absolute_style.position = LayoutPosition::Absolute;
    absolute_style.inset = Edges {
        left: Length::Px(25.0),
        top: Length::Px(25.0),
        right: Length::Auto,
        bottom: Length::Auto,
    };
    let absolute = tree.append_layout(child, absolute_style, 2).unwrap();
    tree.frame([150.0; 2]).unwrap();
    assert_eq!(tree.layout_result(child).unwrap().size, [80.0, 40.0]);
    assert_eq!(tree.layout_result(percent).unwrap().size, [40.0, 20.0]);
    assert_eq!(tree.layout_result(absolute).unwrap().location, [15.0, 15.0]);
    let mut tx = Transaction::default();
    tx.layout_style(tree.root(), style.clone().direction(LayoutDirection::Rtl));
    tree.commit(tx, BackendCapabilities::default()).unwrap();
    tree.frame([150.0; 2]).unwrap();
    assert_eq!(tree.layout_result(child).unwrap().location, [10.0, 10.0]);
    let mut tx = Transaction::default();
    tx.layout_style(
        child,
        LayoutStyle::row()
            .display(LayoutDisplay::None)
            .padding(Edges::all(Length::Px(3.0))),
    );
    tree.commit(tx, BackendCapabilities::default()).unwrap();
    let hidden = tree.frame([150.0; 2]).unwrap();
    assert_eq!(hidden.items.len(), 1);
    assert_eq!(tree.layout_result(child).unwrap().size, [0.0; 2]);
    assert_eq!(tree.layout_result(child).unwrap().padding, [0.0; 4]);
    assert_eq!(tree.layout_result(percent).unwrap().size, [0.0; 2]);
}

#[test]
fn loose_child_keyword_sizes_use_intrinsic_queries_and_preserve_parent_style() {
    for (dimension, expected) in [
        (Dimension::MinContent, 20.0),
        (Dimension::MaxContent, 60.0),
        (Dimension::FitContentPx(35.0), 35.0),
    ] {
        let mut tree = UiTree::new_layout(
            box_style(ConstraintFit::Loose, ConstraintFit::Loose)
                .size(Dimension::Px(100.0), Dimension::Px(80.0)),
            0,
        )
        .unwrap();
        let child = tree
            .append_layout(
                tree.root(),
                LayoutStyle::row()
                    .flex_wrap(FlexWrap::Wrap)
                    .size(dimension, Dimension::Auto),
                0,
            )
            .unwrap();
        for _ in 0..3 {
            tree.append_layout(child, fixed(20.0, 10.0), 1).unwrap();
        }
        tree.frame([150.0; 2]).unwrap();
        assert_eq!(
            tree.layout_result(child).unwrap().size[0],
            expected,
            "{dimension:?}"
        );
        assert_eq!(
            tree.layout_result(child).unwrap().size[1],
            if expected == 60.0 { 10.0 } else { 30.0 }
        );
    }
}

#[test]
fn logical_typed_binding_switches_parent_protocol_atomically_and_query_proofs_stay_safe() {
    use crate::retained::{logical::LogicalUi, reactive::Property, responsive::*};
    let mut ui =
        LogicalUi::new_layout(fixed(200.0, 100.0), 0, BackendCapabilities::default()).unwrap();
    let scope = ui.root_scope();
    let root = ui.root_render();
    let (host, child) = ui
        .edit(
            |_| Ok(()),
            |_, draft| {
                let host = draft.append_layout(
                    scope,
                    root,
                    box_style(ConstraintFit::Loose, ConstraintFit::Loose).fixed_inline_size(80.0),
                    0,
                )?;
                let child = draft.append_layout(scope, host, fixed(20.0, 15.0), 0xff0000ff)?;
                Ok((host, child))
            },
        )
        .unwrap()
        .result
        .value;
    let style = ui
        .state(scope, 0, || {
            box_style(ConstraintFit::Loose, ConstraintFit::Loose).fixed_inline_size(80.0)
        })
        .unwrap();
    ui.bind(style, Property::layout_style(host)).unwrap();
    let before = ui.frame([200.0, 100.0]).unwrap();
    assert_eq!(ui.layout_result(child).unwrap().size, [20.0, 15.0]);
    ui.transaction(|tx| {
        tx.set(
            style,
            box_style(ConstraintFit::Tight, ConstraintFit::Loose).fixed_inline_size(80.0),
        )
    })
    .unwrap();
    let next = ui.local_frame([200.0, 100.0]).unwrap();
    assert_eq!(next.resolve_geometry(child).unwrap().bounds.width, 80.0);
    assert_eq!(
        before
            .items
            .iter()
            .find(|i| i.node == child)
            .unwrap()
            .bounds
            .width,
        20.0
    );
    let source = ui.state(scope, 1, LayoutStyle::row).unwrap();
    let mut queries = LayoutResponsiveBindings::new();
    queries
        .bind_container(
            &mut ui,
            source,
            child,
            host,
            LayoutBreakpointStyles::new(fixed(10.0, 10.0), vec![(70.0, fixed(30.0, 10.0))])
                .unwrap(),
        )
        .unwrap();
    queries
        .frame(&mut ui, &LayoutEnvironment::new([200.0, 100.0]))
        .unwrap();
    assert_eq!(ui.read(source).unwrap().size.width, Dimension::Px(30.0));
}

#[test]
fn minimum_selected_inline_size_remeasures_percentage_child_before_auto_height() {
    let mut tree = UiTree::new_layout(
        box_style(ConstraintFit::Loose, ConstraintFit::Loose)
            .min_size(Dimension::Px(80.0), Dimension::Auto)
            .padding(Edges::all(Length::Px(5.0))),
        0,
    )
    .unwrap();
    let mut style = LayoutStyle::row().size(Dimension::Percent(1.0), Dimension::Auto);
    style.aspect_ratio = Some(2.0);
    let child = tree.append_layout(tree.root(), style, 1).unwrap();
    tree.frame_in_viewport(unbounded(), [200.0; 2]).unwrap();
    assert_eq!(tree.layout_result(tree.root()).unwrap().size, [80.0, 45.0]);
    assert_eq!(tree.layout_result(child).unwrap().size, [70.0, 35.0]);
}

#[test]
fn explicit_child_height_allocation_and_percentage_basis_remain_distinct() {
    for fit in [ConstraintFit::Loose, ConstraintFit::Tight] {
        let mut tree = UiTree::new_layout(LayoutStyle::column(), 0).unwrap();
        let host = tree
            .append_layout(
                tree.root(),
                box_style(ConstraintFit::Tight, fit).size(Dimension::Px(80.0), Dimension::Px(40.0)),
                0,
            )
            .unwrap();
        let child = tree
            .append_layout(
                host,
                LayoutStyle::row().size(Dimension::Px(10.0), Dimension::Percent(0.5)),
                1,
            )
            .unwrap();
        tree.frame_in_viewport(unbounded(), [150.0; 2]).unwrap();
        assert_eq!(tree.layout_result(host).unwrap().size, [80.0, 40.0]);
        assert_eq!(
            tree.layout_result(child).unwrap().size,
            [
                80.0,
                if fit == ConstraintFit::Tight {
                    40.0
                } else {
                    20.0
                }
            ]
        );
    }
}

#[test]
fn query_proof_does_not_mistake_a_box_boundary_for_a_stretched_column() {
    use crate::retained::{logical::LogicalUi, responsive::*};
    let mut ui =
        LogicalUi::new_layout(fixed(200.0, 100.0), 0, BackendCapabilities::default()).unwrap();
    let scope = ui.root_scope();
    let boundary = ui
        .append_render_layout(
            scope,
            ui.root_render(),
            box_style(ConstraintFit::Loose, ConstraintFit::Loose)
                .flex_direction(FlexDirection::Column)
                .size(Dimension::Px(100.0), Dimension::Px(80.0)),
            0,
        )
        .unwrap();
    let host = ui
        .append_render_layout(
            scope,
            boundary,
            LayoutStyle::column().container_type(ContainerType::InlineSize),
            0,
        )
        .unwrap();
    let child = ui
        .append_render_layout(scope, host, fixed(10.0, 10.0), 1)
        .unwrap();
    let source = ui.state(scope, 0, LayoutStyle::row).unwrap();
    let mut responsive = LayoutResponsiveBindings::new();
    assert_eq!(
        responsive.bind_container(
            &mut ui,
            source,
            child,
            host,
            LayoutBreakpointStyles::new(fixed(10.0, 10.0), vec![]).unwrap()
        ),
        Err(Error::UnsafeContainerQuery)
    );
    assert!(responsive.is_empty());
    assert_eq!(ui.reactive().binding_count(), 0);
}
