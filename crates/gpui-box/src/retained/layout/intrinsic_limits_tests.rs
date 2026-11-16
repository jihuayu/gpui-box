use super::*;
use crate::retained::{BackendCapabilities, FrameBuildPolicy, Transaction, UiTree};

fn fixed(w: f32, h: f32) -> LayoutStyle {
    LayoutStyle::row()
        .size(Dimension::Px(w), Dimension::Px(h))
        .flex_shrink(0.0)
}

fn fixture(
    style: LayoutStyle,
    nested: bool,
    direction: FlexDirection,
) -> (UiTree, crate::retained::NodeId) {
    let mut tree = UiTree::new_layout(
        if nested {
            fixed(100.0, 100.0).flex_direction(direction)
        } else {
            style.clone()
        },
        0,
    )
    .unwrap();
    let host = if nested {
        tree.append_layout(tree.root(), style, 0xff0000ff).unwrap()
    } else {
        tree.root()
    };
    for width in [30.0, 40.0] {
        tree.append_layout(host, fixed(width, 10.0), 0x00ff00ff)
            .unwrap();
    }
    (tree, host)
}

#[test]
fn intrinsic_min_max_width_controls_root_and_nested_wrap() {
    for nested in [false, true] {
        for direction in [FlexDirection::Row, FlexDirection::Column] {
            for (min, max, expected) in [
                (Dimension::MinContent, Dimension::Auto, [40.0, 20.0]),
                (Dimension::MaxContent, Dimension::Auto, [70.0, 10.0]),
                (Dimension::Auto, Dimension::MinContent, [40.0, 20.0]),
                (Dimension::Auto, Dimension::MaxContent, [70.0, 10.0]),
                (Dimension::MaxContent, Dimension::MinContent, [70.0, 10.0]),
            ] {
                let preferred = if min == Dimension::Auto { 100.0 } else { 10.0 };
                let style = LayoutStyle::row()
                    .size(Dimension::Px(preferred), Dimension::Auto)
                    .min_size(min, Dimension::Auto)
                    .max_size(max, Dimension::Auto)
                    .flex_wrap(FlexWrap::Wrap)
                    .align_items(AlignItems::Start)
                    .align_self(AlignSelf::Start);
                let (mut tree, host) = fixture(style, nested, direction);
                tree.frame([100.0, 100.0]).unwrap();
                assert_eq!(
                    tree.layout_result(host).unwrap().size,
                    expected,
                    "{nested}/{direction:?}/{min:?}/{max:?}"
                );
            }
        }
    }
}

#[test]
fn intrinsic_limits_participate_in_flex_shrink_and_cross_stretch() {
    for (min, max, expected) in [
        (Dimension::Auto, Dimension::Auto, 40.0),
        (Dimension::MaxContent, Dimension::Auto, 70.0),
        (Dimension::Px(0.0), Dimension::MaxContent, 40.0),
    ] {
        let style = LayoutStyle::row()
            .size(Dimension::Px(100.0), Dimension::Px(20.0))
            .min_size(min, Dimension::Auto)
            .max_size(max, Dimension::Auto)
            .flex_wrap(FlexWrap::Wrap);
        let (mut tree, host) = fixture(style, true, FlexDirection::Row);
        tree.append_layout(tree.root(), fixed(60.0, 10.0), 1)
            .unwrap();
        tree.frame([100.0, 100.0]).unwrap();
        assert_eq!(
            tree.layout_result(host).unwrap().size[0],
            expected,
            "{min:?}/{max:?}"
        );
    }
    let style = LayoutStyle::row()
        .max_size(Dimension::MinContent, Dimension::Auto)
        .flex_wrap(FlexWrap::Wrap);
    let (mut tree, host) = fixture(style, true, FlexDirection::Column);
    tree.frame([100.0, 100.0]).unwrap();
    assert_eq!(tree.layout_result(host).unwrap().size, [40.0, 20.0]);
}

#[test]
fn intrinsic_limits_keep_padding_border_box_and_containment() {
    for sizing in [BoxSizing::BorderBox, BoxSizing::ContentBox] {
        for kind in [
            ContainerType::Normal,
            ContainerType::InlineSize,
            ContainerType::Size,
        ] {
            let mut style = LayoutStyle::row()
                .size(Dimension::Px(100.0), Dimension::Auto)
                .max_size(Dimension::MinContent, Dimension::Auto)
                .padding(Edges::all(Length::Px(2.0)))
                .flex_wrap(FlexWrap::Wrap)
                .container_type(kind);
            style.box_sizing = sizing;
            let (mut tree, host) = fixture(style, false, FlexDirection::Row);
            tree.frame([100.0, 100.0]).unwrap();
            let expected = match kind {
                ContainerType::Normal => [44.0, 24.0],
                ContainerType::InlineSize => [4.0, 24.0],
                ContainerType::Size => [4.0, 4.0],
            };
            assert_eq!(
                tree.layout_result(host).unwrap().size,
                expected,
                "{sizing:?}/{kind:?}"
            );
        }
    }
}

#[test]
fn intrinsic_limit_changes_use_normal_publication_and_measure_isolation() {
    let style = LayoutStyle::row()
        .size(Dimension::Px(100.0), Dimension::Auto)
        .flex_wrap(FlexWrap::Wrap);
    let (mut fast, host) = fixture(style.clone(), true, FlexDirection::Column);
    let (mut full, other) = fixture(style.clone(), true, FlexDirection::Column);
    full.set_frame_build_policy(FrameBuildPolicy::Full);
    let before = fast.frame([100.0, 100.0]).unwrap();
    full.frame([100.0, 100.0]).unwrap();
    for max in [
        Dimension::MinContent,
        Dimension::MaxContent,
        Dimension::Auto,
    ] {
        for (tree, id) in [(&mut fast, host), (&mut full, other)] {
            let mut tx = Transaction::default();
            tx.layout_style(id, style.clone().max_size(max, Dimension::Auto));
            tree.commit(tx, BackendCapabilities::default()).unwrap();
        }
        let a = fast.frame([100.0, 100.0]).unwrap();
        let b = full.frame([100.0, 100.0]).unwrap();
        assert_eq!(
            a.items.iter().map(|x| x.bounds).collect::<Vec<_>>(),
            b.items.iter().map(|x| x.bounds).collect::<Vec<_>>()
        );
        assert_eq!(a.recorded_chunks, 0);
        let result = fast.layout_result(host).unwrap();
        fast.measure_node_with_constraints(
            host,
            LayoutConstraints::new(AvailableSpace::MinContent, AvailableSpace::MaxContent),
        )
        .unwrap();
        assert_eq!(fast.layout_result(host).unwrap(), result);
        assert_eq!(fast.frame([100.0, 100.0]).unwrap().layout_runs, 0);
    }
    assert_eq!(before.items[1].bounds.width, 100.0);
}

#[test]
fn intrinsic_limits_refresh_after_content_change_and_hidden_restore() {
    let style = LayoutStyle::row()
        .size(Dimension::Px(100.0), Dimension::Auto)
        .max_size(Dimension::MinContent, Dimension::Auto)
        .flex_wrap(FlexWrap::Wrap);
    let (mut tree, host) = fixture(style.clone(), true, FlexDirection::Column);
    let child = tree.node(host).unwrap().children[1];
    let before = tree.frame([100.0, 100.0]).unwrap();
    assert_eq!(tree.layout_result(host).unwrap().size, [40.0, 20.0]);
    let mut tx = Transaction::default();
    let mut hidden = style.clone();
    hidden.display = LayoutDisplay::None;
    tx.layout_style(host, hidden);
    tx.layout_style(child, fixed(60.0, 10.0));
    tree.commit(tx, BackendCapabilities::default()).unwrap();
    tree.frame([100.0, 100.0]).unwrap();
    let mut tx = Transaction::default();
    tx.layout_style(host, style);
    tree.commit(tx, BackendCapabilities::default()).unwrap();
    let restored = tree.frame([100.0, 100.0]).unwrap();
    assert_eq!(tree.layout_result(host).unwrap().size, [60.0, 20.0]);
    assert_eq!(before.items[1].bounds.width, 40.0);
    assert_eq!(restored.items[1].bounds.width, 60.0);
}

#[test]
fn intrinsic_limit_values_validate_before_any_transaction_publication() {
    for value in [
        Dimension::Content,
        Dimension::FitContentPx(-1.0),
        Dimension::FitContentPercent(f32::NAN),
    ] {
        let mut tree = UiTree::new_layout(fixed(100.0, 100.0), 0).unwrap();
        let before = tree.frame([100.0, 100.0]).unwrap();
        let mut tx = Transaction::default();
        tx.color(tree.root(), 1);
        tx.layout_style(
            tree.root(),
            LayoutStyle::row().min_size(value, Dimension::Auto),
        );
        tx.layout_style(tree.root(), fixed(100.0, 100.0));
        assert_eq!(
            tree.commit(tx, BackendCapabilities::default()).unwrap_err(),
            Error::InvalidStyle
        );
        assert_eq!(tree.published_frame().unwrap().revision, before.revision);
    }
    // Existing length builders remain source-compatible through Into.
    assert_eq!(
        LayoutStyle::row()
            .min_size(Length::Px(10.0), Length::Auto)
            .min_size
            .width,
        Dimension::Px(10.0)
    );
}

#[test]
fn measured_leaf_intrinsic_limits_follow_final_flex_width_and_preserve_baseline() {
    use crate::retained::{CustomMeasureSnapshot, MeasureSnapshot};
    for nested in [false, true] {
        let style = LayoutStyle::row()
            .size(Dimension::Px(5.0), Dimension::Px(5.0))
            .min_size(Dimension::MaxContent, Dimension::MaxContent)
            .padding(Edges::all(Length::Px(2.0)));
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
            tree.append_layout(tree.root(), style, 1).unwrap()
        } else {
            tree.root()
        };
        let mut tx = Transaction::default();
        tx.measure(
            host,
            Some(MeasureSnapshot::Fixed(CustomMeasureSnapshot {
                width: 80.0,
                height: 30.0,
                baseline: Some(20.0),
            })),
        );
        tree.commit(tx, BackendCapabilities::default()).unwrap();
        tree.frame([100.0, 100.0]).unwrap();
        assert_eq!(tree.layout_result(host).unwrap().size, [84.0, 34.0]);
        assert_eq!(tree.layout_result(host).unwrap().baseline, Some(22.0));
    }
    // A paragraph is needed to verify width-dependent intrinsic height after
    // flex shrink. The platform-backed version lives under retained-text.
}

#[test]
fn intrinsic_height_limits_drive_container_column_wrap() {
    for nested in [false, true] {
        let style = LayoutStyle::column()
            .size(Dimension::Px(50.0), Dimension::Px(5.0))
            .min_size(Dimension::Auto, Dimension::MaxContent)
            .flex_wrap(FlexWrap::Wrap)
            .align_self(AlignSelf::Start);
        let (mut tree, host) = fixture(style, nested, FlexDirection::Row);
        tree.frame([100.0, 100.0]).unwrap();
        assert_eq!(tree.layout_result(host).unwrap().size, [50.0, 20.0]);
        let children = &tree.node(host).unwrap().children;
        assert_eq!(
            tree.layout_result(children[1]).unwrap().location,
            [0.0, 10.0]
        );
    }
}

#[test]
fn intrinsic_limits_compose_with_preferred_keywords_and_aspect_ratio() {
    for nested in [false, true] {
        let style = LayoutStyle::row()
            .size(Dimension::MinContent, Dimension::Auto)
            .min_size(Dimension::MaxContent, Dimension::Auto)
            .flex_wrap(FlexWrap::Wrap)
            .align_self(AlignSelf::Start);
        let (mut tree, host) = fixture(style, nested, FlexDirection::Row);
        tree.frame([100.0, 100.0]).unwrap();
        assert_eq!(tree.layout_result(host).unwrap().size, [70.0, 10.0]);

        let mut style = LayoutStyle::row()
            .size(Dimension::Px(10.0), Dimension::Auto)
            .min_size(Dimension::MaxContent, Dimension::Auto)
            .align_self(AlignSelf::Start);
        style.aspect_ratio = Some(2.0);
        let (mut tree, host) = fixture(style.clone(), nested, FlexDirection::Row);
        tree.frame([100.0, 100.0]).unwrap();
        assert_eq!(
            tree.layout_result(host).unwrap().size,
            [70.0, 35.0],
            "{nested}"
        );
        style.min_size.height = Dimension::Px(60.0);
        let mut tx = Transaction::default();
        tx.layout_style(host, style);
        tree.commit(tx, BackendCapabilities::default()).unwrap();
        tree.frame([100.0, 100.0]).unwrap();
        assert_eq!(
            tree.layout_result(host).unwrap().size,
            [70.0, 60.0],
            "{nested}"
        );
    }
}

#[test]
fn intrinsic_min_height_does_not_make_auto_height_a_percentage_basis() {
    for nested in [false, true] {
        for height in [Dimension::Auto, Dimension::Px(5.0)] {
            for equal in [false, true] {
                let style = LayoutStyle::column()
                    .size(Dimension::Px(80.0), height)
                    .min_size(Dimension::Auto, Dimension::MaxContent)
                    .max_size(
                        Dimension::Auto,
                        if equal {
                            Dimension::MaxContent
                        } else {
                            Dimension::Auto
                        },
                    )
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
                assert_eq!(
                    tree.layout_result(host).unwrap().size[1],
                    30.0,
                    "{nested}/{height:?}/{equal}"
                );
                assert_eq!(
                    tree.layout_result(child).unwrap().size[1],
                    if height == Dimension::Auto {
                        30.0
                    } else {
                        15.0
                    },
                    "{nested}/{height:?}/{equal}"
                );
            }
        }
    }
}

#[test]
fn fit_content_limit_functions_and_stretch_use_available_space_minus_margins() {
    for (max, expected) in [
        (Dimension::FitContentPx(50.0), 50.0),
        (Dimension::FitContentPx(20.0), 40.0),
        (Dimension::FitContentPercent(0.5), 50.0),
        (Dimension::FitContent, 70.0),
        (Dimension::Stretch, 90.0),
    ] {
        let style = LayoutStyle::row()
            .size(Dimension::Px(200.0), Dimension::Auto)
            .max_size(max, Dimension::Auto)
            .margin(Edges {
                left: Length::Px(5.0),
                right: Length::Px(5.0),
                ..Edges::zero()
            })
            .flex_wrap(FlexWrap::Wrap)
            .align_self(AlignSelf::Start)
            .flex_shrink(0.0);
        for nested in [true, false] {
            let (mut tree, host) = fixture(style.clone(), nested, FlexDirection::Row);
            tree.frame([100.0, 100.0]).unwrap();
            assert_eq!(
                tree.layout_result(host).unwrap().size[0],
                expected,
                "{max:?}/{nested}"
            );
        }
    }
}
