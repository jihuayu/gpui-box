use super::*;
use crate::retained::{BackendCapabilities, Transaction, UiTree};

fn fixed(width: f32, height: f32) -> LayoutStyle {
    LayoutStyle::row()
        .size(Dimension::Px(width), Dimension::Px(height))
        .flex_shrink(0.0)
}

#[test]
fn physical_gap_axes_are_independent_of_flex_direction() {
    for direction in [
        FlexDirection::Row,
        FlexDirection::RowReverse,
        FlexDirection::Column,
        FlexDirection::ColumnReverse,
    ] {
        let style = fixed(100.0, 100.0)
            .flex_direction(direction)
            .gap(Length::Px(7.0), Length::Px(3.0));
        let lowered = style.to_taffy().unwrap();
        assert_eq!(lowered.gap.width, taffy::LengthPercentage::length(3.0));
        assert_eq!(lowered.gap.height, taffy::LengthPercentage::length(7.0));
        let mut tree = UiTree::new_layout(style, 0).unwrap();
        let a = tree
            .append_layout(tree.root(), fixed(10.0, 10.0), 1)
            .unwrap();
        let b = tree
            .append_layout(tree.root(), fixed(10.0, 10.0), 2)
            .unwrap();
        tree.frame([100.0, 100.0]).unwrap();
        let a = tree.layout_result(a).unwrap();
        let b = tree.layout_result(b).unwrap();
        match direction {
            FlexDirection::Row | FlexDirection::RowReverse => {
                assert_eq!((a.location[0] - b.location[0]).abs(), 13.0);
            }
            FlexDirection::Column | FlexDirection::ColumnReverse => {
                assert_eq!((a.location[1] - b.location[1]).abs(), 17.0);
            }
        }
    }
    let a = LayoutStyle::column()
        .row_gap(Length::Px(7.0))
        .column_gap(Length::Px(3.0));
    assert_eq!(
        a.to_taffy().unwrap().gap,
        LayoutStyle::row()
            .gap(Length::Px(7.0), Length::Px(3.0))
            .to_taffy()
            .unwrap()
            .gap
    );
}

fn wrapping_tree(width: Dimension, nested: bool) -> (UiTree, crate::retained::NodeId) {
    let container = LayoutStyle::row()
        .size(width, Dimension::Auto)
        .flex_shrink(0.0)
        .flex_wrap(FlexWrap::Wrap)
        .align_items(AlignItems::Start)
        .align_self(AlignSelf::Start);
    let mut tree = UiTree::new_layout(
        if nested {
            fixed(100.0, 100.0)
        } else {
            container.clone()
        },
        0,
    )
    .unwrap();
    let host = if nested {
        tree.append_layout(tree.root(), container, 0).unwrap()
    } else {
        tree.root()
    };
    for width in [30.0, 40.0] {
        tree.append_layout(host, fixed(width, 10.0), 1).unwrap();
    }
    (tree, host)
}

#[test]
fn intrinsic_width_keywords_drive_root_and_nested_wrapping() {
    for nested in [true, false] {
        for (dimension, width, height) in [
            (Dimension::MinContent, 40.0, 20.0),
            (Dimension::MaxContent, 70.0, 10.0),
            (Dimension::FitContentPx(50.0), 50.0, 20.0),
            (Dimension::FitContentPx(20.0), 40.0, 20.0),
            (Dimension::FitContentPx(200.0), 70.0, 10.0),
            (Dimension::FitContentPercent(0.5), 50.0, 20.0),
            (Dimension::FitContent, 70.0, 10.0),
            (Dimension::Stretch, 100.0, 10.0),
        ] {
            let (mut tree, host) = wrapping_tree(dimension, nested);
            tree.frame([100.0, 100.0]).unwrap();
            assert_eq!(
                tree.layout_result(host).unwrap().size,
                [width, height],
                "{dimension:?}, nested={nested}"
            );
            let children = &tree.node(host).unwrap().children;
            let second = tree.layout_result(children[1]).unwrap();
            assert_eq!(
                second.location,
                if height == 20.0 {
                    [0.0, 10.0]
                } else {
                    [30.0, 0.0]
                },
                "children of {dimension:?}, nested={nested}"
            );
        }
    }
}

#[test]
fn content_flex_basis_ignores_preferred_main_size_and_styles_update_normally() {
    let mut tree = UiTree::new_layout(fixed(200.0, 60.0), 0).unwrap();
    let style = fixed(90.0, 30.0).flex_basis(Dimension::Content);
    let host = tree.append_layout(tree.root(), style.clone(), 1).unwrap();
    tree.append_layout(host, fixed(30.0, 10.0), 2).unwrap();
    tree.frame([200.0, 60.0]).unwrap();
    assert_eq!(tree.layout_result(host).unwrap().size[0], 30.0);
    let mut tx = Transaction::default();
    tx.layout_style(host, style.clone().flex_basis(Dimension::Auto));
    tree.commit(tx, BackendCapabilities::default()).unwrap();
    let after = tree.frame([200.0, 60.0]).unwrap();
    assert_eq!(tree.layout_result(host).unwrap().size[0], 90.0);
    assert_eq!(after.recorded_chunks, 0);
    let mut tx = Transaction::default();
    tx.layout_style(host, style.flex_basis(Dimension::Auto));
    assert!(
        !tree
            .commit(tx, BackendCapabilities::default())
            .unwrap()
            .request_frame
    );
}

#[test]
fn root_intrinsic_sizing_preserves_other_axis_min_max_padding_and_queries() {
    let (mut tree, root) = wrapping_tree(Dimension::MaxContent, false);
    let style = LayoutStyle::row()
        .size(Dimension::MaxContent, Dimension::Px(32.0))
        .max_size(Length::Px(55.0), Length::Auto)
        .padding(Edges::all(Length::Px(2.0)))
        .flex_wrap(FlexWrap::Wrap);
    let mut tx = Transaction::default();
    tx.layout_style(root, style);
    tree.commit(tx, BackendCapabilities::default()).unwrap();
    let before = tree.frame([100.0, 100.0]).unwrap();
    let result = tree.layout_result(root).unwrap();
    assert_eq!(result.size, [55.0, 32.0]);
    let measured = tree
        .measure_with_constraints(LayoutConstraints::viewport(120.0, 100.0))
        .unwrap();
    assert_eq!(measured.size, result.size);
    assert_eq!(tree.layout_result(root).unwrap(), result);
    assert_eq!(tree.published_frame().unwrap().revision, before.revision);
    let unchanged = tree.frame([100.0, 100.0]).unwrap();
    assert_eq!(unchanged.layout_runs, 0);
}

#[test]
fn fit_content_limits_validate_and_reject_atomically_even_when_overwritten() {
    for invalid in [
        Dimension::FitContentPx(-1.0),
        Dimension::FitContentPercent(f32::NAN),
        Dimension::FitContentPx(f32::INFINITY),
        Dimension::Content,
    ] {
        let mut tree = UiTree::new_layout(fixed(100.0, 100.0), 0).unwrap();
        let before = tree.frame([100.0, 100.0]).unwrap();
        let mut tx = Transaction::default();
        tx.color(tree.root(), 0xffffffff);
        tx.layout_style(
            tree.root(),
            LayoutStyle::row().size(invalid, Dimension::Auto),
        );
        tx.layout_style(tree.root(), fixed(100.0, 100.0));
        assert!(matches!(
            tree.commit(tx, BackendCapabilities::default()),
            Err(Error::InvalidStyle)
        ));
        assert_eq!(tree.published_frame().unwrap().revision, before.revision);
        assert_eq!(tree.frame([100.0, 100.0]).unwrap().items[0].fill_color, 0);
    }
    assert!(
        LayoutStyle::row()
            .flex_basis(Dimension::Content)
            .validate()
            .is_ok()
    );
}

#[test]
fn intrinsic_height_column_layout_applies_min_over_max_and_final_wrapping() {
    for nested in [false, true] {
        let style = LayoutStyle::column()
            .size(Dimension::Px(50.0), Dimension::MaxContent)
            .min_size(Length::Auto, Length::Px(45.0))
            .max_size(Length::Auto, Length::Px(40.0))
            .flex_wrap(FlexWrap::Wrap)
            .align_self(AlignSelf::Start)
            .align_items(AlignItems::Start);
        let mut tree = UiTree::new_layout(
            if nested {
                fixed(100.0, 100.0)
            } else {
                style.clone()
            },
            0,
        )
        .unwrap();
        let host = if nested {
            tree.append_layout(tree.root(), style, 0).unwrap()
        } else {
            tree.root()
        };
        tree.append_layout(host, fixed(10.0, 30.0), 0).unwrap();
        let second = tree.append_layout(host, fixed(10.0, 40.0), 1).unwrap();
        tree.frame([100.0, 100.0]).unwrap();
        assert_eq!(
            tree.layout_result(host).unwrap().size,
            [50.0, 45.0],
            "nested={nested}"
        );
        assert_eq!(
            tree.layout_result(second).unwrap().location,
            [25.0, 0.0],
            "nested={nested}"
        );
    }
}

#[test]
fn root_intrinsic_leaf_content_box_measure_and_hidden_restore() {
    use crate::retained::{CustomMeasureSnapshot, LayoutDisplay, MeasureSnapshot};
    let mut style = LayoutStyle::row()
        .size(Dimension::MaxContent, Dimension::Px(20.0))
        .padding(Edges::all(Length::Px(2.0)));
    style.box_sizing = BoxSizing::ContentBox;
    let mut tree = UiTree::new_layout(style.clone(), 0).unwrap();
    let mut tx = Transaction::default();
    tx.measure(
        tree.root(),
        Some(MeasureSnapshot::Fixed(CustomMeasureSnapshot {
            width: 40.0,
            height: 10.0,
            baseline: Some(7.0),
        })),
    );
    tree.commit(tx, BackendCapabilities::default()).unwrap();
    tree.frame([100.0, 100.0]).unwrap();
    assert_eq!(tree.layout_result(tree.root()).unwrap().size, [44.0, 24.0]);
    let mut tx = Transaction::default();
    tx.layout_style(tree.root(), style.clone().display(LayoutDisplay::None));
    tree.commit(tx, BackendCapabilities::default()).unwrap();
    tree.frame([100.0, 100.0]).unwrap();
    assert_eq!(tree.layout_result(tree.root()).unwrap().size, [0.0, 0.0]);
    let mut tx = Transaction::default();
    tx.layout_style(tree.root(), style);
    tree.commit(tx, BackendCapabilities::default()).unwrap();
    tree.frame([100.0, 100.0]).unwrap();
    assert_eq!(tree.layout_result(tree.root()).unwrap().size, [44.0, 24.0]);
}

#[test]
fn intrinsic_size_with_definite_other_axis_transfers_aspect_ratio() {
    for nested in [false, true] {
        let mut style = LayoutStyle::row()
            .size(Dimension::MaxContent, Dimension::Px(20.0))
            .flex_wrap(FlexWrap::Wrap)
            .align_items(AlignItems::Start)
            .align_self(AlignSelf::Start)
            .flex_shrink(0.0);
        style.aspect_ratio = Some(2.0);
        let mut tree = UiTree::new_layout(
            if nested {
                fixed(100.0, 100.0)
            } else {
                style.clone()
            },
            0,
        )
        .unwrap();
        let host = if nested {
            tree.append_layout(tree.root(), style, 0).unwrap()
        } else {
            tree.root()
        };
        tree.append_layout(host, fixed(30.0, 10.0), 0).unwrap();
        let second = tree.append_layout(host, fixed(40.0, 10.0), 1).unwrap();
        tree.frame([100.0, 100.0]).unwrap();
        assert_eq!(tree.layout_result(host).unwrap().size, [40.0, 20.0]);
        assert_eq!(tree.layout_result(second).unwrap().location, [0.0, 10.0]);
    }
}

#[test]
fn content_derived_height_stays_indefinite_for_descendant_percentages() {
    for dimension in [
        Dimension::MaxContent,
        Dimension::MinContent,
        Dimension::FitContent,
        Dimension::Px(80.0),
    ] {
        for nested in [false, true] {
            let style = LayoutStyle::column()
                .size(Dimension::Px(80.0), dimension)
                .align_self(AlignSelf::Start)
                .flex_shrink(0.0);
            let mut tree = UiTree::new_layout(
                if nested {
                    fixed(100.0, 100.0)
                } else {
                    style.clone()
                },
                0,
            )
            .unwrap();
            let host = if nested {
                tree.append_layout(tree.root(), style, 0).unwrap()
            } else {
                tree.root()
            };
            let child = tree
                .append_layout(
                    host,
                    LayoutStyle::column()
                        .size(Dimension::Auto, Dimension::Percent(0.5))
                        .flex_shrink(0.0),
                    0,
                )
                .unwrap();
            tree.append_layout(child, fixed(10.0, 30.0), 1).unwrap();
            tree.frame([100.0, 100.0]).unwrap();
            let definite = dimension == Dimension::Px(80.0);
            assert_eq!(
                tree.layout_result(host).unwrap().size[1],
                if definite { 80.0 } else { 30.0 },
                "{dimension:?}, nested={nested}"
            );
            assert_eq!(
                tree.layout_result(child).unwrap().size[1],
                if definite { 40.0 } else { 30.0 },
                "{dimension:?}, nested={nested}"
            );
        }
    }
}

#[test]
fn intrinsic_used_width_resolves_descendant_percentages_in_final_layout() {
    for dimension in [
        Dimension::MinContent,
        Dimension::MaxContent,
        Dimension::FitContent,
    ] {
        let mut tree =
            UiTree::new_layout(LayoutStyle::row().size(dimension, Dimension::Px(40.0)), 0).unwrap();
        let child = tree
            .append_layout(
                tree.root(),
                LayoutStyle::row()
                    .size(Dimension::Percent(0.5), Dimension::Auto)
                    .flex_shrink(0.0),
                0,
            )
            .unwrap();
        tree.append_layout(child, fixed(30.0, 10.0), 1).unwrap();
        tree.frame([100.0, 100.0]).unwrap();
        assert_eq!(
            tree.layout_result(tree.root()).unwrap().size[0],
            30.0,
            "{dimension:?}"
        );
        assert_eq!(
            tree.layout_result(child).unwrap().size[0],
            15.0,
            "{dimension:?}"
        );
    }
}
