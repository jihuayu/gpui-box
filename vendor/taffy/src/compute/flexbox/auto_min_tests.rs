use super::*;
use crate::{Style, TaffyTree};

#[test]
fn automatic_minimum_retains_size_provenance_replaced_kind_and_ratio_box() {
    for replaced in [false, true] {
        for content_box in [false, true] {
            let mut tree = TaffyTree::<()>::new();
            tree.disable_rounding();
            let root = tree
                .new_leaf(Style {
                    size: Size::from_lengths(30.0, 240.0),
                    align_items: AlignItems::START,
                    ..Style::DEFAULT
                })
                .unwrap();
            let inset = if content_box { 7.0 } else { 0.0 };
            let mut child_style = Style {
                aspect_ratio: Some(2.0),
                item_is_replaced: replaced,
                box_sizing: if content_box {
                    BoxSizing::ContentBox
                } else {
                    BoxSizing::BorderBox
                },
                padding: Rect {
                    left: crate::LengthPercentage::length(inset),
                    right: crate::LengthPercentage::length(inset),
                    top: crate::LengthPercentage::length(inset),
                    bottom: crate::LengthPercentage::length(inset),
                },
                ..Style::DEFAULT
            };
            let child = tree.new_leaf(child_style.clone()).unwrap();
            if !replaced {
                let content = tree
                    .new_leaf(Style {
                        size: Size::from_lengths(100.0, 50.0),
                        flex_shrink: 0.0,
                        ..Style::DEFAULT
                    })
                    .unwrap();
                tree.add_child(child, content).unwrap();
            }
            tree.add_child(root, child).unwrap();
            for height in [20.0, 80.0, 20.0] {
                child_style.size.height = Dimension::length(height);
                tree.set_style(child, child_style.clone()).unwrap();
                tree.compute_layout_with_measure(root, Size::MAX_CONTENT, |input, _, _, style| {
                    crate::compute_leaf_layout(
                        input,
                        style,
                        |_, _| 0.0,
                        |known, _| Size {
                            width: known
                                .width
                                .unwrap_or_else(|| known.height.map(|v| v * 2.0).unwrap_or(100.0)),
                            height: known
                                .height
                                .unwrap_or_else(|| known.width.map(|v| v / 2.0).unwrap_or(50.0)),
                        },
                    )
                })
                .unwrap();
                let content_min = if replaced { 0.0 } else { 100.0 };
                assert_eq!(
                    tree.layout(child).unwrap().size,
                    Size {
                        width: (height * 2.0).max(content_min) + inset * 2.0,
                        height: height + inset * 2.0,
                    },
                    "replaced={replaced} content_box={content_box}"
                );
            }
        }
    }
}
