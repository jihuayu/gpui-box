use crate::style_helpers::TaffyMaxContent;
use crate::{AvailableSpace, BoxSizing, Display, Size, SizeBounds, Style, TaffyTree};

fn layout(tree: &mut TaffyTree, root: crate::NodeId, bounds: SizeBounds) {
    tree.compute_layout_with_bounds_and_measure_and_observer(
        root,
        Size { width: AvailableSpace::Unbounded, height: AvailableSpace::Unbounded },
        bounds,
        |input, _, _, style| crate::compute_leaf_layout(input, style, |_, _| 0.0, |_, _| Size::ZERO),
        |_, _, _| {},
    )
    .unwrap();
}

#[test]
fn host_bounds_apply_to_actual_flex_grid_and_block_algorithms() {
    let mut displays = Vec::new();
    #[cfg(feature = "flexbox")]
    displays.push(Display::Flex);
    #[cfg(feature = "grid")]
    displays.push(Display::Grid);
    #[cfg(feature = "block_layout")]
    displays.push(Display::Block);
    for display in displays {
        for children in [false, true] {
            for sizing in [BoxSizing::BorderBox, BoxSizing::ContentBox] {
                let mut tree = TaffyTree::new();
                tree.disable_rounding();
                let child = tree
                    .new_leaf(Style {
                        size: Size::from_lengths(5.0, 5.0),
                        flex_shrink: 0.0,
                        align_self: Some(crate::AlignSelf::START),
                        ..Default::default()
                    })
                    .unwrap();
                let style = Style {
                    display,
                    box_sizing: sizing,
                    size: Size::from_lengths(100.0, 90.0),
                    min_size: Size::from_lengths(120.0, 110.0),
                    padding: crate::Rect {
                        left: crate::LengthPercentage::length(30.0),
                        right: crate::LengthPercentage::length(30.0),
                        top: crate::LengthPercentage::length(30.0),
                        bottom: crate::LengthPercentage::length(30.0),
                    },
                    ..Default::default()
                };
                let root = tree
                    .new_with_children(style.clone(), if children { core::slice::from_ref(&child) } else { &[] })
                    .unwrap();
                for size in [Size { width: 20.0, height: 15.0 }, Size { width: 80.0, height: 70.0 }] {
                    layout(&mut tree, root, SizeBounds { min: size, max: size });
                    assert_eq!(tree.layout(root).unwrap().size, size, "{display:?}/{children}/{sizing:?}");
                    assert_eq!(tree.style(root).unwrap(), &style);
                    if children {
                        assert_eq!(tree.layout(child).unwrap().size, Size { width: 5.0, height: 5.0 });
                    }
                }
                tree.compute_layout(root, Size::MAX_CONTENT).unwrap();
                assert!(tree.layout(root).unwrap().size.width >= 120.0);
            }
        }
    }
}

#[test]
fn out_of_flow_root_bounds_override_style_and_preserve_child_percentage_basis() {
    let mut tree: TaffyTree = TaffyTree::new();
    tree.disable_rounding();
    let child = tree.new_leaf(Style { size: Size::from_percent(0.5, 0.5), ..Default::default() }).unwrap();
    let root = tree
        .new_with_children(
            Style {
                position: crate::Position::Absolute,
                size: Size::from_lengths(100.0, 90.0),
                min_size: Size::from_lengths(120.0, 110.0),
                ..Default::default()
            },
            &[child],
        )
        .unwrap();
    tree.compute_layout_with_bounds_and_measure_and_observer(
        root,
        Size { width: AvailableSpace::Definite(200.0), height: AvailableSpace::Definite(150.0) },
        SizeBounds { min: Size { width: 80.0, height: 60.0 }, max: Size { width: 80.0, height: 60.0 } },
        |inputs, _, _, style| crate::compute_leaf_layout(inputs, style, |_, _| 0.0, |_, _| Size::ZERO),
        |_, _, _| {},
    )
    .unwrap();
    assert_eq!(tree.layout(root).unwrap().size, Size { width: 80.0, height: 60.0 });
    assert_eq!(tree.layout(child).unwrap().size, Size { width: 40.0, height: 30.0 });
}

#[test]
fn invalid_bounds_reject_without_layout_writes() {
    let mut tree: TaffyTree = TaffyTree::new();
    let root = tree.new_leaf(Style::default()).unwrap();
    let mut writes = 0;
    let result = tree.compute_layout_with_bounds_and_measure_and_observer(
        root,
        Size::MAX_CONTENT,
        SizeBounds { min: Size { width: 20.0, height: 0.0 }, max: Size { width: 10.0, height: 100.0 } },
        |input, _, _, style| crate::compute_leaf_layout(input, style, |_, _| 0.0, |_, _| Size::ZERO),
        |_, _, _| writes += 1,
    );
    assert_eq!(result, Err(crate::TaffyError::InvalidSizeBounds));
    assert_eq!(writes, 0);
    assert_eq!(tree.layout(root).unwrap().size, Size::ZERO);
}
