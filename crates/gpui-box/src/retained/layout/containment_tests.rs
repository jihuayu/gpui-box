use super::*;
use crate::retained::{
    BackendCapabilities, CustomMeasureSnapshot, MeasureSnapshot, Transaction, UiTree,
};

fn fixed(width: f32, height: f32) -> LayoutStyle {
    LayoutStyle::row()
        .size(Dimension::Px(width), Dimension::Px(height))
        .flex_shrink(0.0)
}

#[test]
fn containment_preserves_final_children_and_isolates_only_declared_axes() {
    for kind in [
        ContainerType::Normal,
        ContainerType::InlineSize,
        ContainerType::Size,
    ] {
        for direction in [FlexDirection::Row, FlexDirection::Column] {
            let root_style = fixed(300.0, 200.0).flex_direction(direction);
            let mut tree = UiTree::new_layout(root_style, 0).unwrap();
            let host = tree
                .append_layout(
                    tree.root(),
                    LayoutStyle::column()
                        .container_type(kind)
                        .padding(Edges::all(Length::Px(2.0))),
                    0,
                )
                .unwrap();
            let child = tree.append_layout(host, fixed(1000.0, 500.0), 1).unwrap();
            let frame = tree.frame([300.0, 200.0]).unwrap();
            let expected = match (kind, direction) {
                (ContainerType::Normal, FlexDirection::Row) => [1004.0, 200.0],
                (ContainerType::Normal | ContainerType::InlineSize, FlexDirection::Column) => {
                    [300.0, 504.0]
                }
                (ContainerType::InlineSize | ContainerType::Size, FlexDirection::Row) => {
                    [4.0, 200.0]
                }
                (ContainerType::Size, FlexDirection::Column) => [300.0, 4.0],
                _ => unreachable!(),
            };
            assert_eq!(
                tree.layout_result(host).unwrap().size,
                expected,
                "{kind:?}/{direction:?}"
            );
            assert_eq!(tree.layout_result(child).unwrap().size, [1000.0, 500.0]);
            assert!(frame.items.iter().any(|item| item.node == child));
            assert!(tree.layout_result(host).unwrap().scrollable_overflow[1] >= 1002.0);
            if kind != ContainerType::Normal {
                assert_eq!(tree.layout_result(host).unwrap().baseline, None);
            }
        }
    }
}

#[test]
fn size_contained_leaf_still_measures_and_renders_content_without_sizing_from_it() {
    let mut tree = UiTree::new_layout(
        LayoutStyle::row()
            .container_type(ContainerType::Size)
            .padding(Edges::all(Length::Px(2.0))),
        0,
    )
    .unwrap();
    let mut tx = Transaction::default();
    tx.measure(
        tree.root(),
        Some(MeasureSnapshot::Fixed(CustomMeasureSnapshot {
            width: 80.0,
            height: 30.0,
            baseline: Some(20.0),
        })),
    );
    tree.commit(tx, BackendCapabilities::default()).unwrap();
    let frame = tree.frame([300.0, 200.0]).unwrap();
    assert_eq!(tree.layout_result(tree.root()).unwrap().size, [4.0, 4.0]);
    assert!(frame.layout_stats.measure_calls > 0);
    assert!(tree.layout_result(tree.root()).unwrap().scrollable_overflow[1] >= 82.0);
    assert_eq!(tree.layout_result(tree.root()).unwrap().baseline, None);
}

#[test]
fn size_containment_dynamic_style_and_min_max_keep_layout_transaction_contract() {
    let normal = LayoutStyle::column().align_self(AlignSelf::Start);
    let mut tree = UiTree::new_layout(fixed(300.0, 200.0), 0).unwrap();
    let host = tree.append_layout(tree.root(), normal.clone(), 0).unwrap();
    tree.append_layout(host, fixed(100.0, 30.0), 1).unwrap();
    let before = tree.frame([300.0, 200.0]).unwrap();
    let mut tx = Transaction::default();
    tx.layout_style(
        host,
        normal
            .clone()
            .container_type(ContainerType::Size)
            .min_size(Length::Px(60.0), Length::Px(20.0)),
    );
    tree.commit(tx, BackendCapabilities::default()).unwrap();
    let after = tree.frame([300.0, 200.0]).unwrap();
    assert_eq!(tree.layout_result(host).unwrap().size, [60.0, 20.0]);
    assert_eq!(
        before
            .items
            .iter()
            .find(|item| item.node == host)
            .unwrap()
            .bounds
            .width,
        100.0
    );
    assert_eq!(after.recorded_chunks, 0);
    let result = tree.layout_result(host).unwrap();
    tree.measure_node_with_constraints(host, LayoutConstraints::viewport(120.0, 100.0))
        .unwrap();
    assert_eq!(tree.layout_result(host).unwrap(), result);
    let mut tx = Transaction::default();
    tx.layout_style(host, normal);
    tree.commit(tx, BackendCapabilities::default()).unwrap();
    tree.frame([300.0, 200.0]).unwrap();
    assert_eq!(tree.layout_result(host).unwrap().size, [100.0, 30.0]);
}

#[test]
fn contained_images_suppress_natural_ratio_but_preserve_authored_ratio() {
    use crate::retained::ImageIntrinsicSize;
    for (kind, expected_height) in [
        (ContainerType::Normal, 50.0),
        (ContainerType::InlineSize, 40.0),
        (ContainerType::Size, 0.0),
    ] {
        for authored in [false, true] {
            let mut style = LayoutStyle::row()
                .size(Dimension::Px(100.0), Dimension::Auto)
                .container_type(kind);
            style.aspect_ratio = authored.then_some(2.0);
            let mut tree = UiTree::new_layout(style, 0).unwrap();
            let mut tx = Transaction::default();
            tx.measure(
                tree.root(),
                Some(MeasureSnapshot::Image(ImageIntrinsicSize {
                    width: 80.0,
                    height: 40.0,
                })),
            );
            tree.commit(tx, BackendCapabilities::default()).unwrap();
            tree.frame([300.0, 200.0]).unwrap();
            assert_eq!(
                tree.layout_result(tree.root()).unwrap().size,
                [100.0, if authored { 50.0 } else { expected_height }],
                "{kind:?}/{authored}"
            );
        }
    }
}
