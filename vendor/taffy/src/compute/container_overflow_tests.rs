use crate::style_helpers::{TaffyAuto, TaffyMaxContent};
use crate::tree::{CustomLayout, CustomLayoutChildren, CustomLayoutInput, CustomLayoutOutput, CustomPlacement};
use crate::{
    Baselines, ConstraintFit, Contain, Dimension, Direction, Display, LayoutInput, NodeId, Overflow, Point, Rect,
    RunMode, Size, Style, TaffyError, TaffyTree,
};
use std::sync::Arc;

#[derive(Debug)]
struct OneChild {
    rtl: bool,
    offset: Point<f32>,
}
impl CustomLayout for OneChild {
    fn layout(
        &self,
        input: CustomLayoutInput,
        children: &mut dyn CustomLayoutChildren,
    ) -> Result<CustomLayoutOutput, TaffyError> {
        let slot = LayoutInput {
            available_space: Size::MAX_CONTENT,
            parent_size: input.percentage_basis,
            run_mode: RunMode::ComputeSize,
            ..LayoutInput::HIDDEN
        };
        let mut placements = vec![];
        let mut size = Size::ZERO;
        if !children.is_empty() {
            size = children.measure(0, slot)?.size;
            let x = if self.rtl { input.known_size.width.unwrap_or(size.width) - size.width } else { 0.0 };
            placements.push(CustomPlacement {
                child: 0,
                input: slot,
                origin: Point { x: x + self.offset.x, y: self.offset.y },
            });
        }
        Ok(CustomLayoutOutput { size: input.known_size.unwrap_or(size), baselines: Baselines::NONE, placements })
    }
}
fn inset(left: f32, right: f32, top: f32, bottom: f32) -> Rect<crate::LengthPercentage> {
    Rect {
        left: crate::LengthPercentage::length(left),
        right: crate::LengthPercentage::length(right),
        top: crate::LengthPercentage::length(top),
        bottom: crate::LengthPercentage::length(bottom),
    }
}
fn fixture(
    mode: u8,
    direction: Direction,
    overflow: Point<Overflow>,
    child_overflow: Point<Overflow>,
    contain: Contain,
    child_size: Size<f32>,
) -> (TaffyTree, NodeId, NodeId) {
    let mut tree = TaffyTree::new();
    tree.disable_rounding();
    let grandchild =
        tree.new_leaf(Style { size: Size::from_lengths(110.0, 100.0), flex_shrink: 0.0, ..Style::DEFAULT }).unwrap();
    // ConstraintBox caps its child; an independently authored ordinary Flex
    // reference uses that same resulting fixed size, without the box protocol.
    let child_size = if mode == 3 {
        Size {
            width: child_size.width.min(64.0 - if overflow.y == Overflow::Scroll { 6.0 } else { 0.0 }),
            height: child_size.height.min(41.0 - if overflow.x == Overflow::Scroll { 6.0 } else { 0.0 }),
        }
    } else {
        child_size
    };
    let child = tree
        .new_with_children(
            Style {
                size: child_size.map(Dimension::length),
                overflow: child_overflow,
                contain,
                align_items: crate::AlignItems::START,
                flex_shrink: 0.0,
                ..Style::DEFAULT
            },
            &[grandchild],
        )
        .unwrap();
    let root = tree
        .new_with_children(
            Style {
                display: if mode == 2 {
                    Display::ConstraintBox { width: ConstraintFit::Loose, height: ConstraintFit::Loose }
                } else {
                    Display::Flex
                },
                size: Size::from_lengths(80.0, 60.0),
                padding: inset(3.0, 7.0, 5.0, 11.0),
                border: inset(2.0, 4.0, 1.0, 2.0),
                scrollbar_width: 6.0,
                direction,
                overflow,
                flex_direction: crate::FlexDirection::Column,
                align_items: crate::AlignItems::START,
                ..Style::DEFAULT
            },
            &[child],
        )
        .unwrap();
    if mode == 1 {
        tree.set_custom_layout(root, Some(Arc::new(OneChild { rtl: direction.is_rtl(), offset: Point::ZERO })))
            .unwrap();
    }
    tree.compute_layout(root, Size::MAX_CONTENT).unwrap();
    (tree, root, child)
}

#[test]
fn owned_containers_match_flex_actual_child_overflow_padding_gutters_direction_and_containment() {
    let options = [
        Point { x: Overflow::Visible, y: Overflow::Visible },
        Point { x: Overflow::Clip, y: Overflow::Visible },
        Point { x: Overflow::Visible, y: Overflow::Hidden },
        Point { x: Overflow::Scroll, y: Overflow::Scroll },
    ];
    let mut cases = 0;
    for direction in [Direction::Ltr, Direction::Rtl] {
        for overflow in options {
            for child_overflow in options {
                for contain in [Contain::NONE, Contain::LAYOUT, Contain::PAINT] {
                    for child_size in [Size { width: 40.0, height: 20.0 }, Size { width: 120.0, height: 90.0 }] {
                        for mode in [1, 2] {
                            let (reference, reference_root, reference_child) = fixture(
                                if mode == 2 { 3 } else { 0 },
                                direction,
                                overflow,
                                child_overflow,
                                contain,
                                child_size,
                            );
                            let (tree, root, child) =
                                fixture(mode, direction, overflow, child_overflow, contain, child_size);
                            assert_eq!(
                                tree.layout(child).unwrap().size,
                                reference.layout(reference_child).unwrap().size
                            );
                            assert_eq!(
                                tree.layout(child).unwrap().location,
                                reference.layout(reference_child).unwrap().location
                            );
                            assert_eq!(
                                tree.layout(root).unwrap().scrollable_overflow_rect,
                                reference.layout(reference_root).unwrap().scrollable_overflow_rect,
                                "mode={mode}/{direction:?}/{overflow:?}/{child_overflow:?}/{contain:?}/{child_size:?}"
                            );
                            assert_eq!(
                                (tree.layout(root).unwrap().scroll_width(), tree.layout(root).unwrap().scroll_height()),
                                (
                                    reference.layout(reference_root).unwrap().scroll_width(),
                                    reference.layout(reference_root).unwrap().scroll_height()
                                )
                            );
                            cases += 1;
                        }
                    }
                }
            }
        }
    }
    assert_eq!(cases, 384);
}

#[test]
fn desired_host_size_never_becomes_scroll_content_and_negative_placements_obey_reachability() {
    for overflow in [Overflow::Visible, Overflow::Scroll] {
        let mut tree = TaffyTree::<()>::new();
        tree.disable_rounding();
        let root = tree
            .new_leaf(Style {
                size: Size::from_lengths(80.0, 60.0),
                padding: inset(3.0, 7.0, 5.0, 11.0),
                overflow: Point { x: overflow, y: overflow },
                ..Style::DEFAULT
            })
            .unwrap();
        tree.set_custom_layout(root, Some(Arc::new(OneChild { rtl: false, offset: Point::ZERO }))).unwrap();
        tree.compute_layout(root, Size::MAX_CONTENT).unwrap();
        assert_eq!(
            tree.layout(root).unwrap().scrollable_overflow_rect,
            if overflow == Overflow::Visible {
                Rect::ZERO
            } else {
                Rect { left: 0.0, right: 7.0, top: 0.0, bottom: 11.0 }
            }
        );
        let child = tree.new_leaf(Style { size: Size::from_lengths(10.0, 10.0), ..Style::DEFAULT }).unwrap();
        tree.add_child(root, child).unwrap();
        for (offset, expected_visible, expected_scroll) in [
            (
                Point { x: -30.0, y: 0.0 },
                Rect { left: -27.0, right: 0.0, top: 0.0, bottom: 15.0 },
                Rect { left: 0.0, right: 7.0, top: 0.0, bottom: 11.0 },
            ),
            (
                Point { x: -8.0, y: -8.0 },
                Rect { left: -5.0, right: 5.0, top: -3.0, bottom: 7.0 },
                Rect { left: -5.0, right: 12.0, top: -3.0, bottom: 18.0 },
            ),
        ] {
            tree.set_custom_layout(root, Some(Arc::new(OneChild { rtl: false, offset }))).unwrap();
            tree.compute_layout(root, Size::MAX_CONTENT).unwrap();
            assert_eq!(
                tree.layout(root).unwrap().scrollable_overflow_rect,
                if overflow == Overflow::Visible { expected_visible } else { expected_scroll }
            );
        }
    }
}

#[test]
fn constraint_and_custom_empty_hidden_and_restored_children_have_only_real_scroll_extent() {
    for mode in [1, 2] {
        for direction in [Direction::Ltr, Direction::Rtl] {
            let mut tree = TaffyTree::<()>::new();
            tree.disable_rounding();
            let root = tree
                .new_leaf(Style {
                    display: if mode == 2 {
                        Display::ConstraintBox { width: ConstraintFit::Loose, height: ConstraintFit::Loose }
                    } else {
                        Display::Flex
                    },
                    direction,
                    size: Size::from_lengths(80.0, 60.0),
                    padding: inset(3.0, 7.0, 5.0, 11.0),
                    overflow: Point { x: Overflow::Scroll, y: Overflow::Scroll },
                    ..Style::DEFAULT
                })
                .unwrap();
            if mode == 1 {
                tree.set_custom_layout(root, Some(Arc::new(OneChild { rtl: direction.is_rtl(), offset: Point::ZERO })))
                    .unwrap();
            }
            let expected =
                Rect { left: 0.0, right: if direction.is_rtl() { 3.0 } else { 7.0 }, top: 0.0, bottom: 11.0 };
            tree.compute_layout(root, Size::MAX_CONTENT).unwrap();
            assert_eq!(tree.layout(root).unwrap().scrollable_overflow_rect, expected);
            let child = tree
                .new_leaf(Style { display: Display::None, size: Size::from_lengths(30.0, 20.0), ..Style::DEFAULT })
                .unwrap();
            tree.add_child(root, child).unwrap();
            tree.compute_layout(root, Size::MAX_CONTENT).unwrap();
            assert_eq!(tree.layout(root).unwrap().scrollable_overflow_rect, expected);
            tree.set_style(child, Style { size: Size::from_lengths(30.0, 20.0), ..Style::DEFAULT }).unwrap();
            tree.compute_layout(root, Size::MAX_CONTENT).unwrap();
            assert_eq!(
                tree.layout(root).unwrap().scrollable_overflow_rect,
                Rect { left: 0.0, right: 40.0, top: 0.0, bottom: 36.0 }
            );
            let published = *tree.layout(root).unwrap();
            tree.compute_layout(root, Size::MAX_CONTENT).unwrap();
            assert_eq!(*tree.layout(root).unwrap(), published);
            tree.set_children(root, &[]).unwrap();
            tree.compute_layout(root, Size::MAX_CONTENT).unwrap();
            assert_eq!(tree.layout(root).unwrap().scrollable_overflow_rect, expected);
        }
    }
}

#[test]
fn real_out_of_flow_descendants_still_extend_owned_containers_after_inflow_reset() {
    for direction in [Direction::Ltr, Direction::Rtl] {
        let overflow = Point { x: Overflow::Scroll, y: Overflow::Scroll };
        let mut results = vec![];
        for mode in [0, 1, 2] {
            let (mut tree, root, child) = fixture(
                mode,
                direction,
                overflow,
                Point { x: Overflow::Visible, y: Overflow::Visible },
                Contain::NONE,
                Size { width: 40.0, height: 20.0 },
            );
            let mut style = tree.style(root).unwrap().clone();
            style.position = crate::Position::Relative;
            tree.set_style(root, style).unwrap();
            let absolute = tree
                .new_leaf(Style {
                    position: crate::Position::Absolute,
                    // Place beyond the reachable logical end in either
                    // direction. Physical left130 in RTL can instead be
                    // wholly before the scroll origin and correctly excluded.
                    inset: Rect {
                        left: if direction.is_rtl() {
                            crate::LengthPercentageAuto::AUTO
                        } else {
                            crate::LengthPercentageAuto::length(130.0)
                        },
                        right: if direction.is_rtl() {
                            crate::LengthPercentageAuto::length(130.0)
                        } else {
                            crate::LengthPercentageAuto::AUTO
                        },
                        top: crate::LengthPercentageAuto::length(120.0),
                        bottom: crate::LengthPercentageAuto::AUTO,
                    },
                    size: Size::from_lengths(30.0, 20.0),
                    ..Style::DEFAULT
                })
                .unwrap();
            tree.add_child(child, absolute).unwrap();
            tree.compute_layout(root, Size::MAX_CONTENT).unwrap();
            results
                .push((tree.layout(root).unwrap().scrollable_overflow_rect, tree.layout(absolute).unwrap().location));
        }
        assert_eq!(results[0], results[1], "custom/{direction:?}");
        assert_eq!(results[0], results[2], "constraint/{direction:?}");
        assert_eq!((results[0].0.right, results[0].0.bottom), (160.0, 140.0));
    }
}
