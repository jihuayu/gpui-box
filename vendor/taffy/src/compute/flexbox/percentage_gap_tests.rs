use super::*;
use crate::{FlexWrap, Style, TaffyTree};

#[test]
fn fit_content_inline_gap_recollects_lines_after_used_width_without_resizing() {
    for nested in [false, true] {
        for available_width in [300.0, 70.0] {
            let mut tree = TaffyTree::<()>::new();
            tree.disable_rounding();
            let host_style = Style {
                flex_wrap: FlexWrap::Wrap,
                align_items: AlignItems::START,
                align_self: Some(AlignItems::START),
                align_content: AlignContent::START,
                ..Style::DEFAULT
            };
            let host = tree.new_leaf(host_style.clone()).unwrap();
            let mut children = vec![];
            for _ in 0..3 {
                let child = tree
                    .new_leaf(Style {
                        size: Size::from_lengths(30.0, 20.0),
                        flex_shrink: 0.0,
                        ..Style::DEFAULT
                    })
                    .unwrap();
                tree.add_child(host, child).unwrap();
                children.push(child);
            }
            let root = if nested {
                tree.new_with_children(
                    Style {
                        size: Size::from_lengths(available_width, 200.0),
                        align_items: AlignItems::START,
                        ..Style::DEFAULT
                    },
                    &[host],
                )
                .unwrap()
            } else {
                host
            };
            for gap in [0.0, 0.1, 0.0] {
                let mut style = host_style.clone();
                style.gap.width = crate::LengthPercentage::percent(gap);
                tree.set_style(host, style).unwrap();
                tree.compute_layout(
                    root,
                    Size {
                        width: AvailableSpace::Definite(available_width),
                        height: AvailableSpace::Definite(200.0),
                    },
                )
                .unwrap();
                let width = available_width.min(90.0);
                let wraps = gap > 0.0 || available_width < 90.0;
                assert_eq!(
                    tree.layout(host).unwrap().size,
                    Size {
                        width,
                        height: if wraps { 40.0 } else { 20.0 }
                    },
                    "{nested}/{available_width}/{gap}"
                );
                for (i, child) in children.iter().enumerate() {
                    let index = if wraps && i == 2 { 0 } else { i };
                    assert_eq!(
                        tree.layout(*child).unwrap().location,
                        Point {
                            x: index as f32 * (30.0 + gap * width),
                            y: if wraps && i == 2 { 20.0 } else { 0.0 },
                        },
                        "{nested}/{available_width}/{gap}/{i}"
                    );
                }
            }
        }
    }
}

#[test]
fn cyclic_block_gap_stays_zero_across_root_nested_and_style_cache_updates() {
    for nested in [false, true] {
        for keyword in [Dimension::auto(), Dimension::max_content()] {
            let mut tree = TaffyTree::<()>::new();
            tree.disable_rounding();
            let host_style = Style {
                flex_direction: FlexDirection::Column,
                size: Size {
                    width: Dimension::length(100.0),
                    height: keyword,
                },
                gap: Size {
                    width: crate::LengthPercentage::length(7.0),
                    height: crate::LengthPercentage::percent(0.1),
                },
                align_items: AlignItems::START,
                align_self: Some(AlignItems::START),
                ..Style::DEFAULT
            };
            let host = tree.new_leaf(host_style.clone()).unwrap();
            let mut children = vec![];
            for _ in 0..3 {
                let child = tree
                    .new_leaf(Style {
                        size: Size::from_lengths(30.0, 20.0),
                        flex_shrink: 0.0,
                        ..Style::DEFAULT
                    })
                    .unwrap();
                tree.add_child(host, child).unwrap();
                children.push(child);
            }
            let root = if nested {
                tree.new_with_children(
                    Style {
                        size: Size::from_lengths(200.0, 160.0),
                        align_items: AlignItems::START,
                        ..Style::DEFAULT
                    },
                    &[host],
                )
                .unwrap()
            } else {
                host
            };
            for definite in [false, true, false] {
                let mut style = host_style.clone();
                if definite {
                    style.size.height = Dimension::length(80.0);
                }
                tree.set_style(host, style).unwrap();
                tree.compute_layout(
                    root,
                    Size {
                        width: AvailableSpace::Definite(300.0),
                        height: AvailableSpace::Definite(200.0),
                    },
                )
                .unwrap();
                assert_eq!(
                    tree.layout(host).unwrap().size.height,
                    if definite { 80.0 } else { 60.0 },
                    "{nested}/{keyword:?}/{definite}"
                );
                for (i, child) in children.iter().enumerate() {
                    assert_eq!(
                        tree.layout(*child).unwrap().location.y,
                        i as f32 * if definite { 28.0 } else { 20.0 }
                    );
                }
            }
        }
    }
}
