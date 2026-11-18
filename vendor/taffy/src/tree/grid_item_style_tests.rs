use super::*;
use crate::style_helpers::TaffyGridLine;
use crate::{AlignSelf, AlignmentSafety, GridPlacement, Line};

#[test]
fn grid_item_view_preserves_numeric_named_and_self_alignment_inputs_across_clones() {
    let mut tree = TaffyTree::<()>::new();
    let style = Style {
        grid_column: Line {
            start: GridPlacement::from_line_index(-3),
            end: GridPlacement::Span(2),
        },
        grid_row: Line {
            start: GridPlacement::NamedLine("row".into(), -2),
            end: GridPlacement::NamedSpan("tail".into(), 3),
        },
        align_self: Some(AlignSelf::LAST_BASELINE),
        justify_self: Some(AlignSelf::END.with_safety(AlignmentSafety::Safe)),
        ..Style::DEFAULT
    };
    let node = tree.new_leaf(style.clone()).unwrap();
    let view = tree.as_layout_tree().get_custom_child_style(node).unwrap();
    assert_eq!(view.grid_column, style.grid_column);
    assert_eq!(view.grid_row, style.grid_row);
    assert_eq!(view.align_self, style.align_self);
    assert_eq!(view.justify_self, style.justify_self);
    let mut old = tree.clone();
    tree.set_style(node, Style::DEFAULT).unwrap();
    assert_eq!(
        old.as_layout_tree().get_custom_child_style(node).unwrap(),
        view
    );
    let current = tree.as_layout_tree().get_custom_child_style(node).unwrap();
    assert_eq!(current.grid_column, Line::default());
    assert_eq!(current.justify_self, None);
    drop(old);
    // The owned view survives the tree and retains authored names as well.
    tree.clear();
    assert_eq!(view.grid_row, style.grid_row);
}
