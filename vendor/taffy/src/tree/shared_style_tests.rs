use super::*;
use crate::{Contain, Dimension, Size};

#[test]
fn shared_style_replacement_keeps_old_tree_and_caller_snapshot_alive() {
    let original = Arc::new(Style {
        size: Size {
            width: Dimension::length(40.0),
            height: Dimension::length(20.0),
        },
        ..Style::DEFAULT
    });
    let weak = Arc::downgrade(&original);
    let mut tree = TaffyTree::<()>::new();
    let leaf = tree.new_leaf_shared(original.clone()).unwrap();
    assert!(core::ptr::eq(tree.style(leaf).unwrap(), original.as_ref()));
    let space = Size {
        width: AvailableSpace::MaxContent,
        height: AvailableSpace::MaxContent,
    };
    tree.compute_layout(leaf, space).unwrap();
    let mut old = tree.clone();
    assert!(core::ptr::eq(
        tree.style(leaf).unwrap(),
        old.style(leaf).unwrap()
    ));
    tree.set_style(
        leaf,
        Style {
            size: Size {
                width: Dimension::length(60.0),
                height: Dimension::length(30.0),
            },
            ..Style::DEFAULT
        },
    )
    .unwrap();
    assert!(tree.dirty(leaf).unwrap());
    tree.compute_layout(leaf, space).unwrap();
    old.compute_layout(leaf, space).unwrap();
    assert_eq!(tree.layout(leaf).unwrap().size.width, 60.0);
    assert_eq!(old.layout(leaf).unwrap().size.width, 40.0);
    drop(original);
    assert!(weak.upgrade().is_some());
    old.clear();
    assert!(weak.upgrade().is_none());
}

#[test]
fn shared_setter_preserves_containment_visibility_and_dirty_indexes() {
    let mut tree = TaffyTree::<()>::new();
    let node = tree.new_leaf(Style::DEFAULT).unwrap();
    let shared = Arc::new(Style {
        contain: Contain::SIZE,
        size: Size {
            width: Dimension::length(40.0),
            height: Dimension::length(20.0),
        },
        ..Style::DEFAULT
    });
    tree.set_style_shared(node, shared.clone()).unwrap();
    assert!(tree.size_contained_nodes.contains_key(node.into()));
    let space = Size {
        width: AvailableSpace::MaxContent,
        height: AvailableSpace::MaxContent,
    };
    tree.compute_layout(node, space).unwrap();
    assert!(tree.contained_layout_input(node).unwrap().is_some());
    // Engine set_style has always invalidated even equal values. Framework P0
    // is checked before this boundary; the shared setter must keep that contract.
    tree.set_style_shared(node, shared).unwrap();
    assert!(tree.dirty(node).unwrap());
    tree.set_style_shared(
        node,
        Arc::new(Style {
            display: Display::None,
            ..Style::DEFAULT
        }),
    )
    .unwrap();
    assert!(tree.contained_layout_input(node).unwrap().is_none());
    assert!(!tree.size_contained_nodes.contains_key(node.into()));
    tree.compute_layout(node, space).unwrap();
    assert_eq!(tree.layout(node).unwrap().size, Size::ZERO);
}

#[test]
fn owned_context_and_parent_constructors_clone_to_shared_snapshots() {
    let mut tree = TaffyTree::new();
    let child = tree.new_leaf_with_context(Style::DEFAULT, 7_u32).unwrap();
    let parent = tree.new_with_children(Style::DEFAULT, &[child]).unwrap();
    let old = tree.clone();
    for node in [child, parent] {
        assert!(core::ptr::eq(
            tree.style(node).unwrap(),
            old.style(node).unwrap()
        ));
    }
    assert_eq!(old.get_node_context(child), Some(&7));
    let snapshot = tree.nodes[parent.into()].style.clone();
    tree.remove(parent).unwrap();
    assert_eq!(old.style(parent).unwrap(), snapshot.as_ref());
}
