use super::*;
use crate::{AvailableSpace, Contain, Dimension, Display, FlexDirection, Size, Style, TaffyTree};

fn space(w: f32) -> Size<AvailableSpace> {
    Size {
        width: AvailableSpace::Definite(w),
        height: AvailableSpace::Definite(100.0),
    }
}
fn contained() -> Style {
    Style {
        contain: Contain::SIZE | Contain::LAYOUT,
        size: Size {
            width: Dimension::length(80.0),
            height: Dimension::length(40.0),
        },
        ..Style::DEFAULT
    }
}

#[test]
fn contained_final_inputs_survive_cache_probes_clones_and_retire_with_visibility_and_nodes() {
    let mut tree = TaffyTree::<()>::new();
    tree.disable_rounding();
    let child = tree.new_leaf(contained()).unwrap();
    let parent = tree
        .new_with_children(
            Style {
                flex_direction: FlexDirection::Column,
                ..Style::DEFAULT
            },
            &[child],
        )
        .unwrap();
    tree.compute_layout(parent, space(120.0)).unwrap();
    let input = tree.contained_layout_input(child).unwrap().unwrap();
    assert_eq!(
        input.known_dimensions,
        Size {
            width: Some(80.0),
            height: Some(40.0)
        }
    );
    tree.compute_layout(parent, space(120.0)).unwrap();
    assert_eq!(tree.contained_layout_input(child).unwrap(), Some(input));
    let old = tree.clone();
    let mut probe = input;
    probe.run_mode = RunMode::ComputeSize;
    probe.parent_size.height = Some(999.0);
    tree.as_layout_tree().compute_child_layout(child, probe);
    assert_eq!(tree.contained_layout_input(child).unwrap(), Some(input));
    tree.set_style(
        parent,
        Style {
            display: Display::None,
            ..Style::DEFAULT
        },
    )
    .unwrap();
    tree.compute_layout(parent, space(120.0)).unwrap();
    assert_eq!(tree.contained_layout_input(child).unwrap(), None);
    assert_eq!(old.contained_layout_input(child).unwrap(), Some(input));
    tree.set_style(parent, Style::DEFAULT).unwrap();
    tree.compute_layout(parent, space(120.0)).unwrap();
    assert!(tree.contained_layout_input(child).unwrap().is_some());
    tree.set_style(child, Style::DEFAULT).unwrap();
    assert_eq!(tree.contained_layout_input(child).unwrap(), None);
    tree.set_style(child, contained()).unwrap();
    tree.compute_layout(parent, space(120.0)).unwrap();
    tree.remove(child).unwrap();
    assert!(tree.contained_layout_input(child).is_err());
    let replacement = tree.new_leaf(contained()).unwrap();
    assert_eq!(tree.contained_layout_input(replacement).unwrap(), None);
    tree.compute_layout(replacement, space(120.0)).unwrap();
    tree.clear();
    assert!(tree.contained_layout_inputs.is_empty());
    assert!(tree.size_contained_nodes.is_empty());
    assert!(tree.exact_layout_inputs.is_empty());
}

#[test]
fn contained_final_record_tracks_soft_space_even_with_equal_known_used_size() {
    let mut tree = TaffyTree::<()>::new();
    tree.disable_rounding();
    let root = tree.new_leaf(contained()).unwrap();
    for width in [40.0, 70.0, 40.0] {
        let mut input = LayoutInput::HIDDEN;
        input.run_mode = RunMode::PerformLayout;
        input.known_dimensions = Size {
            width: Some(80.0),
            height: Some(40.0),
        };
        input.available_space = space(width);
        tree.as_layout_tree()
            .compute_custom_child_layout(root, input);
        let input = tree.contained_layout_input(root).unwrap().unwrap();
        assert_eq!(input.known_dimensions.width, Some(80.0));
        assert_eq!(input.available_space.width, AvailableSpace::Definite(width));
        assert_eq!(input.known_dimensions.height, Some(40.0));
    }
}
