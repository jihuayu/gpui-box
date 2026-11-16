use super::*;
use crate::retained::{BackendCapabilities, FrameBuildPolicy, Transaction, UiTree};

fn constraints(bounds: BoxConstraints) -> LayoutConstraints {
    LayoutConstraints::new(AvailableSpace::Unbounded, AvailableSpace::Unbounded).with_bounds(bounds)
}

fn fixed(w: f32, h: f32) -> LayoutStyle {
    LayoutStyle::row()
        .size(Dimension::Px(w), Dimension::Px(h))
        .flex_shrink(0.0)
}

#[test]
fn hard_bounds_override_authored_limits_and_padding_for_leaf_and_flex_roots() {
    for has_children in [false, true] {
        for (bounds, expected) in [
            (BoxConstraints::tight([20.0, 15.0]), [20.0, 15.0]),
            (BoxConstraints::loose([50.0, 40.0]), [50.0, 40.0]),
            (
                BoxConstraints::new([120.0, 110.0], [150.0, 130.0]),
                [120.0, 110.0],
            ),
        ] {
            let style = fixed(80.0, 90.0)
                .min_size(Dimension::Px(100.0), Dimension::Px(100.0))
                .max_size(Dimension::Px(60.0), Dimension::Px(60.0))
                .padding(Edges::all(Length::Px(30.0)));
            let mut tree = UiTree::new_layout(style.clone(), 0xff0000ff).unwrap();
            if has_children {
                tree.append_layout(tree.root(), fixed(5.0, 5.0), 1).unwrap();
            }
            let result = tree
                .frame_in_viewport(constraints(bounds), [200.0, 200.0])
                .unwrap();
            assert_eq!(
                tree.layout_result(tree.root()).unwrap().size,
                expected,
                "{has_children}/{bounds:?}"
            );
            assert_eq!(tree.style(tree.root()).unwrap(), &style.to_taffy().unwrap());
            assert_eq!(result.items[0].bounds.width, expected[0]);
            assert!(
                result
                    .items
                    .iter()
                    .all(|item| item.bounds.width.is_finite())
            );
        }
    }
}

#[test]
fn loose_bounds_participate_in_real_flex_wrap_and_minimum_free_space() {
    for direction in [FlexDirection::Row, FlexDirection::Column] {
        let mut tree = UiTree::new_layout(
            LayoutStyle::row()
                .flex_direction(direction)
                .flex_wrap(FlexWrap::Wrap)
                .align_items(AlignItems::Start),
            0,
        )
        .unwrap();
        for _ in 0..3 {
            tree.append_layout(tree.root(), fixed(30.0, 30.0), 1)
                .unwrap();
        }
        let maximum = if direction == FlexDirection::Row {
            [60.0, 200.0]
        } else {
            [200.0, 60.0]
        };
        tree.frame_in_viewport(constraints(BoxConstraints::loose(maximum)), [200.0, 200.0])
            .unwrap();
        assert_eq!(tree.layout_result(tree.root()).unwrap().size, [60.0, 60.0]);
        let children = &tree.node(tree.root()).unwrap().children;
        let third = tree.layout_result(children[2]).unwrap();
        assert_eq!(
            third.location,
            if direction == FlexDirection::Row {
                [0.0, 30.0]
            } else {
                [30.0, 0.0]
            }
        );
    }
    let mut tree = UiTree::new_layout(LayoutStyle::row(), 0).unwrap();
    let child = tree
        .append_layout(tree.root(), fixed(10.0, 10.0).flex_grow(1.0), 1)
        .unwrap();
    tree.frame_in_viewport(
        constraints(BoxConstraints::new([80.0, 0.0], [f32::INFINITY; 2])),
        [100.0; 2],
    )
    .unwrap();
    assert_eq!(tree.layout_result(child).unwrap().size[0], 80.0);

    let mut tree = UiTree::new_layout(
        LayoutStyle::row()
            .flex_wrap(FlexWrap::Wrap)
            .align_items(AlignItems::Start),
        0,
    )
    .unwrap();
    for _ in 0..3 {
        tree.append_layout(tree.root(), fixed(30.0, 30.0), 1)
            .unwrap();
    }
    tree.frame_in_viewport(
        LayoutConstraints::viewport(20.0, 200.0)
            .with_bounds(BoxConstraints::new([80.0, 0.0], [100.0, 200.0])),
        [200.0; 2],
    )
    .unwrap();
    assert_eq!(tree.layout_result(tree.root()).unwrap().size, [80.0, 60.0]);
}

#[test]
fn bounds_do_not_leak_to_children_and_tight_height_is_a_percentage_basis() {
    let mut tree =
        UiTree::new_layout(LayoutStyle::column().align_items(AlignItems::Start), 0).unwrap();
    let child = tree
        .append_layout(
            tree.root(),
            LayoutStyle::row().size(Dimension::Percent(0.5), Dimension::Percent(0.5)),
            1,
        )
        .unwrap();
    tree.frame_in_viewport(constraints(BoxConstraints::tight([80.0, 60.0])), [100.0; 2])
        .unwrap();
    assert_eq!(tree.layout_result(child).unwrap().size, [40.0, 30.0]);
    tree.frame_in_viewport(constraints(BoxConstraints::loose([80.0, 60.0])), [100.0; 2])
        .unwrap();
    assert_eq!(tree.layout_result(child).unwrap().size, [0.0, 0.0]);
}

#[test]
fn bounds_only_changes_invalidate_layout_preserve_old_frames_and_reuse_idle() {
    let mut fast = UiTree::new_layout(fixed(150.0, 20.0), 0xff0000ff).unwrap();
    let mut full = UiTree::new_layout(fixed(150.0, 20.0), 0xff0000ff).unwrap();
    full.frame_build_policy = FrameBuildPolicy::Full;
    let mut old = None;
    for width in [80.0, 40.0, 120.0, 40.0] {
        let input = constraints(BoxConstraints::loose([width, 60.0]));
        let a = fast.local_frame_in_viewport(input, [160.0, 80.0]).unwrap();
        let b = full.frame_in_viewport(input, [160.0, 80.0]).unwrap();
        assert_eq!(a.resolve_geometry(fast.root()).unwrap().bounds.width, width);
        let ar = a.resolve();
        assert_eq!(ar.items[0].bounds, b.items[0].bounds);
        fn pixels(packet: crate::retained::FramePacket) -> Vec<u8> {
            use crate::retained::{pacing, software};
            let mut job = software::SoftwareJob::new(
                packet,
                software::SoftwareOptions {
                    size: [160, 80],
                    scale: 1.0,
                    background: 0,
                    effects: pacing::SoftwareEffects::Exact,
                    max_bytes: 4 * 1024 * 1024,
                },
            )
            .unwrap();
            while !job.step(4096).unwrap() {}
            job.into_frame().unwrap().pixels().to_vec()
        }
        let actual = pixels(ar);
        assert_eq!(actual, pixels(b));
        assert_eq!(
            &actual[(5 * 160 + 5) * 4..(5 * 160 + 5) * 4 + 4],
            &[255, 0, 0, 255]
        );
        let outside = (5 * 160 + width as usize + 1) * 4;
        assert_eq!(&actual[outside..outside + 4], &[0, 0, 0, 0]);
        assert_eq!(a.hit_test(5.0, 5.0), Some(fast.root()));
        assert_eq!(a.hit_test(width + 1.0, 5.0), None);
        assert_eq!(
            fast.frame_in_viewport(input, [160.0, 80.0])
                .unwrap()
                .layout_runs,
            0
        );
        if old.is_none() {
            old = Some(a);
        }
    }
    assert_eq!(
        old.unwrap()
            .resolve_geometry(fast.root())
            .unwrap()
            .bounds
            .width,
        80.0
    );
    let mut tx = Transaction::default();
    tx.color(fast.root(), 0x00ff00ff);
    fast.commit(tx, BackendCapabilities::default()).unwrap();
    let input = constraints(BoxConstraints::loose([40.0, 60.0]));
    assert_eq!(
        fast.local_frame_in_viewport(input, [160.0, 80.0])
            .unwrap()
            .resolve()
            .layout_runs,
        0
    );
}

#[test]
fn subtree_measure_bounds_leave_published_owner_and_styles_unchanged() {
    let mut tree = UiTree::new_layout(fixed(200.0, 100.0), 0).unwrap();
    let child = tree
        .append_layout(tree.root(), fixed(100.0, 40.0), 1)
        .unwrap();
    let before = tree.frame([200.0, 100.0]).unwrap();
    let result = tree
        .measure_node_with_constraints(child, constraints(BoxConstraints::tight([25.0, 15.0])))
        .unwrap();
    assert_eq!(result.size, [25.0, 15.0]);
    assert_eq!(tree.layout_result(child).unwrap().size, [100.0, 40.0]);
    assert_eq!(
        tree.frame([200.0, 100.0]).unwrap().revision,
        before.revision
    );
    assert_eq!(tree.frame([200.0, 100.0]).unwrap().layout_runs, 0);
}

#[test]
fn invalid_bounds_reject_before_owner_changes_and_hidden_root_ignores_minimum() {
    let mut tree = UiTree::new_layout(fixed(50.0, 30.0), 0).unwrap();
    let before = tree.frame([100.0; 2]).unwrap();
    for bounds in [
        BoxConstraints::new([10.0, 0.0], [5.0, 20.0]),
        BoxConstraints::tight([f32::INFINITY, 20.0]),
        BoxConstraints::loose([f32::NAN, 20.0]),
        BoxConstraints::new([-1.0, 0.0], [20.0; 2]),
    ] {
        assert_eq!(
            tree.frame_in_viewport(constraints(bounds), [100.0; 2])
                .err(),
            Some(Error::InvalidConstraints)
        );
        assert_eq!(tree.published_frame().unwrap().revision, before.revision);
    }
    let mut tx = Transaction::default();
    tx.layout_style(tree.root(), fixed(50.0, 30.0).display(LayoutDisplay::None));
    tree.commit(tx, BackendCapabilities::default()).unwrap();
    tree.frame_in_viewport(constraints(BoxConstraints::tight([80.0, 60.0])), [100.0; 2])
        .unwrap();
    assert_eq!(tree.layout_result(tree.root()).unwrap().size, [0.0; 2]);
    let mut tx = Transaction::default();
    let mut hidden = fixed(50.0, 30.0).display(LayoutDisplay::None);
    hidden.position = LayoutPosition::Absolute;
    tx.layout_style(tree.root(), hidden);
    tree.commit(tx, BackendCapabilities::default()).unwrap();
    tree.frame_in_viewport(
        LayoutConstraints::viewport(100.0, 100.0).with_bounds(BoxConstraints::tight([80.0, 60.0])),
        [100.0; 2],
    )
    .unwrap();
    assert_eq!(tree.layout_result(tree.root()).unwrap().size, [0.0; 2]);
}

#[test]
fn responsive_previews_use_parent_bounds_and_keep_environment_breakpoints_independent() {
    use crate::retained::{
        logical::LogicalUi,
        responsive::{LayoutBreakpointStyles, LayoutEnvironment, LayoutResponsiveBindings},
    };
    let base = LayoutStyle::column().fixed_inline_size(100.0);
    let mut ui = LogicalUi::new_layout(base.clone(), 0, BackendCapabilities::default()).unwrap();
    let scope = ui.root_scope();
    let root = ui.root_render();
    let leaf_style = fixed(10.0, 10.0);
    let leaf = ui
        .append_render_layout(scope, root, leaf_style.clone(), 0xff0000ff)
        .unwrap();
    let host = ui.state(scope, 0, || base.clone()).unwrap();
    let child = ui.state(scope, 1, || leaf_style.clone()).unwrap();
    let mut responsive = LayoutResponsiveBindings::new();
    responsive
        .bind(
            &mut ui,
            host,
            root,
            LayoutBreakpointStyles::new(
                base,
                vec![(300.0, LayoutStyle::column().fixed_inline_size(200.0))],
            )
            .unwrap(),
        )
        .unwrap();
    responsive
        .bind_container(
            &mut ui,
            child,
            leaf,
            root,
            LayoutBreakpointStyles::new(leaf_style, vec![(50.0, fixed(20.0, 20.0))]).unwrap(),
        )
        .unwrap();
    let environment = LayoutEnvironment::new([500.0, 400.0]);
    for (width, expected) in [(40.0, 10.0), (80.0, 20.0), (40.0, 10.0)] {
        let result = responsive
            .local_frame_in_viewport(
                &mut ui,
                &environment,
                constraints(BoxConstraints::new([width, 0.0], [width, f32::INFINITY])),
                [100.0; 2],
            )
            .unwrap();
        assert_eq!(ui.read(host).unwrap().size.width, Dimension::Px(200.0));
        assert_eq!(ui.layout_result(root).unwrap().size[0], width);
        assert_eq!(
            result.frame.resolve_geometry(leaf).unwrap().bounds.width,
            expected
        );
    }
}

#[test]
fn hard_parent_size_overrides_aspect_ratio_in_both_axes() {
    for children in [false, true] {
        let mut style = fixed(100.0, 80.0);
        style.aspect_ratio = Some(2.0);
        let mut tree = UiTree::new_layout(style, 0).unwrap();
        if children {
            tree.append_layout(tree.root(), fixed(5.0, 5.0), 1).unwrap();
        }
        tree.frame_in_viewport(constraints(BoxConstraints::tight([20.0, 15.0])), [100.0; 2])
            .unwrap();
        assert_eq!(tree.layout_result(tree.root()).unwrap().size, [20.0, 15.0]);
    }
}
