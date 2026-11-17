use super::*;
use crate::{AvailableSpace, Dimension, FlexDirection, RequestedAxis, RunMode, Size, Style};

#[test]
fn incoming_tracking_survives_cache_and_ignores_dry_queries_then_retires() {
    let mut tree = TaffyTree::<()>::new();
    tree.disable_rounding();
    let host = tree.new_leaf(Style::DEFAULT).unwrap();
    let root = tree
        .new_with_children(
            Style {
                flex_direction: FlexDirection::Column,
                size: Size {
                    width: Dimension::length(100.0),
                    height: Dimension::auto(),
                },
                ..Style::DEFAULT
            },
            &[host],
        )
        .unwrap();
    tree.set_track_layout_input(host, true).unwrap();
    let available = Size {
        width: AvailableSpace::Definite(100.0),
        height: AvailableSpace::Unbounded,
    };
    tree.compute_layout(root, available).unwrap();
    let input = tree.tracked_layout_input(host).unwrap().unwrap();
    assert_eq!(input.run_mode, RunMode::PerformLayout);
    assert_eq!(input.axis, RequestedAxis::Both);
    assert_eq!(input.known_dimensions.width, Some(100.0));
    tree.compute_layout(root, available).unwrap();
    assert_eq!(tree.tracked_layout_input(host).unwrap(), Some(input));
    let query = LayoutInput {
        known_dimensions: Size {
            width: Some(40.0),
            height: None,
        },
        ..input
    };
    tree.measure_node_with_input(host, query, |input, _, _, style| {
        crate::compute_leaf_layout(input, style, |_, _| 0.0, |_, _| Size::ZERO)
    })
    .unwrap();
    tree.measure_node_baselines_with_input(host, query, |input, _, _, style| {
        crate::compute_leaf_layout(input, style, |_, _| 0.0, |_, _| Size::ZERO)
    })
    .unwrap();
    assert_eq!(tree.tracked_layout_input(host).unwrap(), Some(input));
    let mut clone = tree.clone();
    clone.clear_tracked_layout_inputs();
    assert_eq!(clone.tracked_layout_input(host).unwrap(), None);
    assert_eq!(tree.tracked_layout_input(host).unwrap(), Some(input));
    tree.set_style(
        root,
        Style {
            display: crate::Display::None,
            ..Style::DEFAULT
        },
    )
    .unwrap();
    tree.compute_layout(root, available).unwrap();
    assert_eq!(tree.tracked_layout_input(host).unwrap(), None);
    tree.set_style(
        root,
        Style {
            flex_direction: FlexDirection::Column,
            size: Size::from_lengths(80.0, 100.0),
            ..Style::DEFAULT
        },
    )
    .unwrap();
    tree.compute_layout(root, available).unwrap();
    assert_eq!(
        tree.tracked_layout_input(host)
            .unwrap()
            .unwrap()
            .known_dimensions
            .width,
        Some(80.0)
    );
    tree.remove(host).unwrap();
    let replacement = tree.new_leaf(Style::DEFAULT).unwrap();
    assert!(tree.tracked_layout_input(host).is_err());
    assert_eq!(tree.tracked_layout_input(replacement).unwrap(), None);
    tree.set_track_layout_input(replacement, true).unwrap();
    tree.clear();
    assert!(tree.tracked_layout_input(replacement).is_err());
}
