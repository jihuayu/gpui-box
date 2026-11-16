use super::*;
use crate::retained::{BackendCapabilities, FrameBuildPolicy, Transaction, UiTree};

fn space() -> LayoutConstraints {
    LayoutConstraints::new(AvailableSpace::Unbounded, AvailableSpace::Unbounded)
}

fn fixture(direction: FlexDirection) -> (UiTree, crate::retained::NodeId) {
    let mut tree = UiTree::new_layout(
        LayoutStyle::row()
            .flex_direction(direction)
            .align_items(AlignItems::Start)
            .size(Dimension::Percent(1.0), Dimension::Percent(1.0)),
        0,
    )
    .unwrap();
    let child = tree
        .append_layout(
            tree.root(),
            LayoutStyle::row()
                .size(Dimension::Px(80.0), Dimension::Px(80.0))
                .min_size(Dimension::Px(0.0), Dimension::Px(0.0))
                .flex_basis(Dimension::Px(20.0))
                .flex_grow(1.0),
            0xff0000ff,
        )
        .unwrap();
    (tree, child)
}

#[test]
fn normal_unbounded_flex_uses_basis_instead_of_intrinsic_contribution() {
    for direction in [FlexDirection::Row, FlexDirection::Column] {
        let (mut tree, child) = fixture(direction);
        let normal = tree.frame_in_viewport(space(), [100.0, 100.0]).unwrap();
        let expected = if direction == FlexDirection::Row {
            [20.0, 80.0]
        } else {
            [80.0, 20.0]
        };
        assert_eq!(tree.layout_result(tree.root()).unwrap().size, expected);
        assert_eq!(tree.layout_result(child).unwrap().size, expected);
        assert!(
            normal
                .items
                .iter()
                .all(|item| item.bounds.width.is_finite() && item.bounds.height.is_finite())
        );
        let intrinsic = tree
            .measure_with_constraints(LayoutConstraints::new(
                AvailableSpace::MaxContent,
                AvailableSpace::MaxContent,
            ))
            .unwrap();
        if direction == FlexDirection::Row {
            assert_eq!(intrinsic.size, [80.0, 80.0]);
        }
        assert_eq!(tree.layout_result(child).unwrap().size, expected);
        assert_eq!(
            tree.frame_in_viewport(space(), [100.0, 100.0])
                .unwrap()
                .layout_runs,
            0
        );
    }
}

#[test]
fn finite_present_surface_never_becomes_an_unbounded_percentage_basis() {
    let mut tree =
        UiTree::new_layout(LayoutStyle::column().align_items(AlignItems::Start), 0).unwrap();
    let child = tree
        .append_layout(
            tree.root(),
            LayoutStyle::column()
                .size(Dimension::Percent(0.5), Dimension::Percent(0.5))
                .flex_shrink(0.0),
            0xff0000ff,
        )
        .unwrap();
    tree.append_layout(
        child,
        LayoutStyle::row().size(Dimension::Px(30.0), Dimension::Px(20.0)),
        1,
    )
    .unwrap();
    tree.frame_in_viewport(space(), [200.0, 100.0]).unwrap();
    assert_eq!(tree.layout_result(child).unwrap().size, [30.0, 20.0]);
    let before = tree.published_frame().unwrap().clone();
    assert_eq!(
        tree.frame_in_viewport(space(), [400.0, 200.0])
            .unwrap()
            .layout_runs,
        0
    );
    assert_eq!(tree.layout_result(child).unwrap().size, [30.0, 20.0]);
    assert_eq!(before.items[0].spatial.root_clip.unwrap().width, 200.0);
}

#[test]
fn unbounded_layout_and_viewport_changes_invalidate_independently() {
    let (mut tree, child) = fixture(FlexDirection::Row);
    tree.frame([100.0, 100.0]).unwrap();
    assert_eq!(tree.layout_result(child).unwrap().size[0], 100.0);
    let free = tree
        .local_frame_in_viewport(space(), [100.0, 100.0])
        .unwrap();
    assert_eq!(free.resolve_geometry(child).unwrap().bounds.width, 20.0);
    let mut tx = Transaction::default();
    tx.translation(child, [10.0, 0.0]);
    tree.commit(tx, BackendCapabilities::default()).unwrap();
    let moved = tree
        .local_frame_in_viewport(space(), [100.0, 100.0])
        .unwrap();
    assert!(!moved.stats().full_publication);
    assert_eq!(moved.resolve_geometry(child).unwrap().bounds.x, 10.0);
    assert_eq!(free.resolve_geometry(child).unwrap().bounds.x, 0.0);
    let finite = tree.frame([100.0, 100.0]).unwrap();
    assert_eq!(finite.layout_runs, 1);
    assert_eq!(tree.layout_result(child).unwrap().size[0], 100.0);
}

#[test]
fn unbounded_rendering_keeps_finite_clip_and_full_selective_geometry_agreement() {
    let (mut fast, child) = fixture(FlexDirection::Row);
    let (mut full, other) = fixture(FlexDirection::Row);
    full.set_frame_build_policy(FrameBuildPolicy::Full);
    for width in [10.0, 50.0, 0.0, 100.0] {
        let a = fast.frame_in_viewport(space(), [width, 100.0]).unwrap();
        let b = full.frame_in_viewport(space(), [width, 100.0]).unwrap();
        assert_eq!(
            a.items
                .iter()
                .map(|i| (i.bounds, i.clip))
                .collect::<Vec<_>>(),
            b.items
                .iter()
                .map(|i| (i.bounds, i.clip))
                .collect::<Vec<_>>()
        );
        assert_eq!(
            fast.layout_result(child).unwrap().size,
            full.layout_result(other).unwrap().size
        );
        if width == 10.0 {
            assert_eq!(a.hit_test(5.0, 5.0), Some(child));
            assert_eq!(a.hit_test(15.0, 5.0), None);
        }
    }
    let before = fast.published_frame().unwrap().revision;
    assert!(matches!(
        fast.frame_in_viewport(space(), [f32::NAN, 100.0]),
        Err(Error::InvalidViewport)
    ));
    assert!(matches!(
        fast.frame_in_viewport(
            LayoutConstraints::viewport(f32::INFINITY, 100.0),
            [100.0, 100.0]
        ),
        Err(Error::InvalidViewport)
    ));
    assert_eq!(fast.published_frame().unwrap().revision, before);
}

#[test]
fn real_maximum_bounds_unbounded_wrap_and_minimum_distributes_finite_free_space() {
    for direction in [FlexDirection::Row, FlexDirection::Column] {
        let row = direction == FlexDirection::Row;
        let mut root = LayoutStyle::row()
            .flex_direction(direction)
            .flex_wrap(FlexWrap::Wrap)
            .align_items(AlignItems::Start)
            .align_content(AlignContent::Start);
        if row {
            root.max_size.width = Dimension::Px(50.0);
        } else {
            root.max_size.height = Dimension::Px(50.0);
        }
        let mut tree = UiTree::new_layout(root.clone(), 0).unwrap();
        let children = [30.0, 40.0].map(|main| {
            tree.append_layout(
                tree.root(),
                LayoutStyle::row()
                    .size(
                        Dimension::Px(if row { main } else { 10.0 }),
                        Dimension::Px(if row { 10.0 } else { main }),
                    )
                    .flex_shrink(0.0),
                0xff0000ff,
            )
            .unwrap()
        });
        tree.frame_in_viewport(space(), [100.0, 100.0]).unwrap();
        assert_eq!(
            tree.layout_result(tree.root()).unwrap().size,
            if row { [40.0, 20.0] } else { [20.0, 40.0] }
        );
        assert_eq!(
            tree.layout_result(children[1]).unwrap().location,
            if row { [0.0, 10.0] } else { [10.0, 0.0] }
        );
        if row {
            root.min_size.width = Dimension::Px(80.0);
        } else {
            root.min_size.height = Dimension::Px(80.0);
        }
        let mut tx = Transaction::default();
        tx.layout_style(tree.root(), root);
        tree.commit(tx, BackendCapabilities::default()).unwrap();
        tree.frame_in_viewport(space(), [100.0, 100.0]).unwrap();
        assert_eq!(
            tree.layout_result(tree.root()).unwrap().size,
            if row { [80.0, 10.0] } else { [10.0, 80.0] }
        );
        let (mut growing, child) = fixture(direction);
        let mut root = LayoutStyle::row()
            .flex_direction(direction)
            .align_items(AlignItems::Start);
        if row {
            root.min_size.width = Dimension::Px(50.0);
        } else {
            root.min_size.height = Dimension::Px(50.0);
        }
        let mut tx = Transaction::default();
        tx.layout_style(growing.root(), root);
        growing.commit(tx, BackendCapabilities::default()).unwrap();
        growing.frame_in_viewport(space(), [100.0, 100.0]).unwrap();
        assert_eq!(
            growing.layout_result(child).unwrap().size,
            if row { [50.0, 80.0] } else { [80.0, 50.0] }
        );
    }
}

#[test]
fn cpu_pixels_use_the_same_finite_clip_as_unbounded_layout_hit_testing() {
    use crate::retained::{pacing, software};
    let (mut tree, child) = fixture(FlexDirection::Row);
    let packet = tree.frame_in_viewport(space(), [10.0, 100.0]).unwrap();
    let mut job = software::SoftwareJob::new(
        packet.clone(),
        software::SoftwareOptions {
            size: [100, 100],
            scale: 1.0,
            background: 0,
            effects: pacing::SoftwareEffects::Exact,
            max_bytes: 4 * 1024 * 1024,
        },
    )
    .unwrap();
    while !job.step(4096).unwrap() {}
    let rendered = job.into_frame().unwrap();
    let pixel = |x: usize, y: usize| &rendered.pixels()[(y * 100 + x) * 4..(y * 100 + x) * 4 + 4];
    assert_eq!(pixel(5, 5), &[255, 0, 0, 255]);
    assert_eq!(pixel(15, 5), &[0, 0, 0, 0]);
    assert_eq!(rendered.hit_test([5.0, 5.0]), Some(child));
    assert_eq!(rendered.hit_test([15.0, 5.0]), None);
}

#[test]
fn logical_reactive_updates_use_unbounded_frames_and_keep_snapshots_owned() {
    use crate::retained::{logical::LogicalUi, reactive::Property};
    let (tree, child) = fixture(FlexDirection::Row);
    let mut ui = LogicalUi::from_tree(tree, BackendCapabilities::default()).unwrap();
    let scope = ui.root_scope();
    let source = ui.state(scope, 0, || 0xff0000ffu32).unwrap();
    ui.bind(source, Property::color(child)).unwrap();
    let before = ui.frame_in_viewport(space(), [100.0, 100.0]).unwrap();
    ui.transaction(|tx| tx.set(source, 0x00ff00ff)).unwrap();
    let after = ui.local_frame_in_viewport(space(), [100.0, 100.0]).unwrap();
    assert_eq!(
        after.resolve_geometry(child).unwrap().fill_color,
        0x00ff00ff
    );
    assert_eq!(before.items[1].fill_color, 0xff0000ff);
    assert_eq!(ui.layout_result(child).unwrap().size, [20.0, 80.0]);
    assert!(
        !ui.transaction(|tx| tx.set(source, 0x00ff00ff))
            .unwrap()
            .commit
            .request_frame
    );
}

#[test]
fn mixed_bounded_inline_and_unbounded_block_keeps_flex_percent_basis_indefinite() {
    let mut tree = UiTree::new_layout(
        LayoutStyle::column()
            .size(Dimension::Percent(1.0), Dimension::Auto)
            .align_items(AlignItems::Start)
            .padding(Edges::all(Length::Px(2.0)))
            .row_gap(Length::Px(3.0)),
        0,
    )
    .unwrap();
    let first = tree
        .append_layout(
            tree.root(),
            LayoutStyle::row()
                .size(Dimension::Percent(0.5), Dimension::Px(20.0))
                .flex_basis(Dimension::Percent(0.5))
                .flex_grow(1.0)
                .flex_shrink(0.0),
            0xff0000ff,
        )
        .unwrap();
    let second = tree
        .append_layout(
            tree.root(),
            LayoutStyle::row()
                .size(Dimension::Px(30.0), Dimension::Px(10.0))
                .flex_shrink(0.0),
            0x00ff00ff,
        )
        .unwrap();
    // An indefinite percentage basis falls back to content, so provide actual
    // owned content instead of assuming the preferred height becomes basis.
    let mut tx = Transaction::default();
    tx.measure(
        first,
        Some(crate::retained::MeasureSnapshot::Fixed(
            crate::retained::CustomMeasureSnapshot {
                width: 10.0,
                height: 20.0,
                baseline: None,
            },
        )),
    );
    tree.commit(tx, BackendCapabilities::default()).unwrap();
    let constraints =
        LayoutConstraints::new(AvailableSpace::Definite(100.0), AvailableSpace::Unbounded);
    tree.frame_in_viewport(constraints, [100.0, 15.0]).unwrap();
    assert_eq!(tree.layout_result(tree.root()).unwrap().size, [100.0, 37.0]);
    assert_eq!(tree.layout_result(first).unwrap().size, [48.0, 20.0]);
    assert_eq!(tree.layout_result(second).unwrap().location, [2.0, 25.0]);
    assert_eq!(tree.published_frame().unwrap().hit_test(3.0, 26.0), None);
}

#[test]
fn intrinsic_keywords_and_containment_override_natural_unbounded_sizing_explicitly() {
    for kind in [
        ContainerType::Normal,
        ContainerType::InlineSize,
        ContainerType::Size,
    ] {
        let (mut tree, child) = fixture(FlexDirection::Row);
        let style = LayoutStyle::row()
            .container_type(kind)
            .align_items(AlignItems::Start);
        let mut tx = Transaction::default();
        tx.layout_style(tree.root(), style.clone());
        tree.commit(tx, BackendCapabilities::default()).unwrap();
        tree.frame_in_viewport(space(), [100.0, 100.0]).unwrap();
        assert_eq!(
            tree.layout_result(tree.root()).unwrap().size,
            match kind {
                ContainerType::Normal => [20.0, 80.0],
                ContainerType::InlineSize => [0.0, 80.0],
                ContainerType::Size => [0.0, 0.0],
            }
        );
        if kind == ContainerType::Normal {
            let mut tx = Transaction::default();
            tx.layout_style(
                tree.root(),
                style.size(Dimension::MaxContent, Dimension::Auto),
            );
            tree.commit(tx, BackendCapabilities::default()).unwrap();
            tree.frame_in_viewport(space(), [100.0, 100.0]).unwrap();
            assert_eq!(tree.layout_result(child).unwrap().size, [80.0, 80.0]);
        }
    }
}

#[test]
fn unbounded_multichild_flex_accounts_for_margins_gaps_limits_and_nested_allocation() {
    for direction in [FlexDirection::Row, FlexDirection::Column] {
        let row = direction == FlexDirection::Row;
        let mut tree = UiTree::new_layout(
            LayoutStyle::row()
                .flex_direction(direction)
                .align_items(AlignItems::Start)
                .padding(Edges::all(Length::Px(2.0)))
                .main_cross_gap(Length::Px(3.0), Length::Px(0.0)),
            0,
        )
        .unwrap();
        let mut style = LayoutStyle::row()
            .flex_direction(direction)
            .align_items(AlignItems::Start)
            .size(Dimension::Px(80.0), Dimension::Px(80.0))
            .min_size(Dimension::Px(0.0), Dimension::Px(0.0))
            .margin(Edges::all(Length::Px(1.0)))
            .flex_basis(Dimension::Px(20.0))
            .flex_grow(1.0);
        if row {
            style.min_size.width = Dimension::Px(25.0);
            style.max_size.width = Dimension::Px(15.0);
        } else {
            style.min_size.height = Dimension::Px(25.0);
            style.max_size.height = Dimension::Px(15.0);
        }
        let first = tree.append_layout(tree.root(), style.clone(), 1).unwrap();
        if row {
            style.min_size.width = Dimension::Px(0.0);
        } else {
            style.min_size.height = Dimension::Px(0.0);
        }
        let second = tree.append_layout(tree.root(), style, 2).unwrap();
        let nested = tree
            .append_layout(
                first,
                LayoutStyle::row()
                    .size(Dimension::Px(4.0), Dimension::Px(4.0))
                    .min_size(Dimension::Px(0.0), Dimension::Px(0.0))
                    .flex_grow(1.0),
                3,
            )
            .unwrap();
        let frame = tree.frame_in_viewport(space(), [100.0, 100.0]).unwrap();
        assert_eq!(
            tree.layout_result(tree.root()).unwrap().size,
            if row { [51.0, 86.0] } else { [86.0, 51.0] }
        );
        assert_eq!(tree.layout_result(first).unwrap().location, [3.0, 3.0]);
        assert_eq!(
            tree.layout_result(second).unwrap().location,
            if row { [33.0, 3.0] } else { [3.0, 33.0] }
        );
        assert_eq!(
            tree.layout_result(nested).unwrap().size,
            if row { [25.0, 4.0] } else { [4.0, 25.0] }
        );
        assert!(
            frame
                .items
                .iter()
                .all(|item| item.bounds.width.is_finite() && item.bounds.height.is_finite())
        );
    }
}

#[test]
fn out_of_flow_percentages_use_layout_containing_block_and_ignore_paint_viewport() {
    for position in [taffy::Position::Absolute, taffy::Position::Fixed] {
        let mut tree = UiTree::new_layout(
            LayoutStyle::row().size(Dimension::Px(40.0), Dimension::Px(20.0)),
            0,
        )
        .unwrap();
        let mut style = LayoutStyle::row().size(Dimension::Percent(0.5), Dimension::Percent(0.5));
        style.inset = Edges::all(Length::Px(0.0));
        let mut lowered = style.to_taffy().unwrap();
        // Fixed remains a compatibility-only style, but it shares the kernel's
        // initial containing-block behavior with the public Absolute style.
        lowered.position = position;
        let child = tree.append(tree.root(), lowered, 1).unwrap();
        for viewport in [[100.0, 100.0], [200.0, 200.0]] {
            tree.frame_in_viewport(space(), viewport).unwrap();
            assert_eq!(tree.layout_result(child).unwrap().size, [20.0, 10.0]);
        }
        tree.frame_in_viewport(LayoutConstraints::viewport(80.0, 60.0), [200.0, 200.0])
            .unwrap();
        // No nearer positioned ancestor: definite layout input establishes the
        // initial containing block, independently of root and paint sizes.
        assert_eq!(tree.layout_result(child).unwrap().size, [40.0, 30.0]);
    }
}

#[test]
fn intrinsic_render_inputs_and_hidden_root_viewport_changes_preserve_publication() {
    let (mut tree, child) = fixture(FlexDirection::Row);
    let intrinsic = LayoutConstraints::new(AvailableSpace::MaxContent, AvailableSpace::MaxContent);
    let before = tree.frame_in_viewport(intrinsic, [10.0, 10.0]).unwrap();
    assert_eq!(tree.layout_result(child).unwrap().size, [80.0, 80.0]);
    let after = tree.frame_in_viewport(space(), [10.0, 10.0]).unwrap();
    assert_eq!(after.layout_runs, 1);
    assert_eq!(tree.layout_result(child).unwrap().size, [20.0, 80.0]);
    assert_eq!(before.items[1].bounds.width, 80.0);
    let mut tx = Transaction::default();
    tx.layout_style(tree.root(), LayoutStyle::row().display(LayoutDisplay::None));
    tree.commit(tx, BackendCapabilities::default()).unwrap();
    assert!(
        tree.frame_in_viewport(space(), [10.0, 10.0])
            .unwrap()
            .items
            .is_empty()
    );
    let resized = tree.frame_in_viewport(space(), [0.0, 0.0]).unwrap();
    assert_eq!(resized.layout_runs, 0);
    assert!(resized.damage.full);
    assert!(resized.items.is_empty());
    let mut tx = Transaction::default();
    tx.layout_style(
        tree.root(),
        LayoutStyle::row().align_items(AlignItems::Start),
    );
    tree.commit(tx, BackendCapabilities::default()).unwrap();
    let restored = tree.frame_in_viewport(space(), [10.0, 10.0]).unwrap();
    assert_eq!(restored.layout_runs, 1);
    assert_eq!(restored.hit_test(5.0, 5.0), Some(child));
    assert!(restored.damage.full);
}
