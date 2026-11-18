use super::*;
use crate::style_helpers::TaffyMaxContent;
use crate::{Dimension, FlexDirection};
use std::sync::atomic::{AtomicUsize, Ordering};

#[derive(Debug)]
struct Context {
    value: f32,
    clones: Arc<AtomicUsize>,
}
impl Clone for Context {
    fn clone(&self) -> Self {
        self.clones.fetch_add(1, Ordering::Relaxed);
        Self {
            value: self.value,
            clones: self.clones.clone(),
        }
    }
}
fn measure(tree: &mut TaffyTree<Context>, node: NodeId, input: LayoutInput) -> LayoutOutput {
    tree.measure_node_with_input(node, input, |input, _, context, style| {
        let height = context.map_or(0.0, |c| c.value);
        compute_leaf_layout(
            input,
            style,
            |_, _| 0.0,
            |_, _| Size {
                width: 30.0,
                height,
            },
        )
    })
    .unwrap()
}
fn input() -> LayoutInput {
    LayoutInput {
        available_space: Size::MAX_CONTENT,
        ..LayoutInput::HIDDEN
    }
}
#[test]
fn copied_subtree_union_clones_only_selected_contexts_shares_styles_and_starts_cold() {
    let mut tree = TaffyTree::new();
    let clones = Arc::new(AtomicUsize::new(0));
    let mut children = vec![];
    for i in 0..1000 {
        children.push(
            tree.new_leaf_with_context(
                Style::DEFAULT,
                Context {
                    value: i as f32 + 10.0,
                    clones: clones.clone(),
                },
            )
            .unwrap(),
        );
    }
    let a = tree
        .new_with_children(
            Style {
                flex_direction: FlexDirection::Column,
                ..Style::DEFAULT
            },
            &children[..3],
        )
        .unwrap();
    let root = tree.new_with_children(Style::DEFAULT, &[a]).unwrap();
    tree.set_exact_measure(children[0], true).unwrap();
    tree.set_parent_data(children[0], Some(crate::tree::CustomParentData::new(8u32)))
        .unwrap();
    tree.compute_layout_with_measure(root, Size::MAX_CONTENT, |input, _, c, style| {
        let height = c.map_or(0.0, |c| c.value);
        compute_leaf_layout(
            input,
            style,
            |_, _| 0.0,
            |_, _| Size {
                width: 30.0,
                height,
            },
        )
    })
    .unwrap();
    let before = tree.nodes[children[0].into()].clone();
    let (mut copied, roots) = tree
        .clone_subtrees_for_measurement(&[children[0], a, children[0]])
        .unwrap();
    assert_eq!(copied.total_node_count(), 4);
    assert_eq!(clones.load(Ordering::Relaxed), 3);
    assert_eq!(roots[0], roots[2]);
    assert!(Arc::ptr_eq(
        &tree.nodes[a.into()].style,
        &copied.nodes[roots[1].into()].style
    ));
    assert_eq!(copied.parent(roots[1]), None);
    assert_eq!(copied.parent(roots[0]), Some(roots[1]));
    assert_eq!(
        copied.parent_data[roots[0].into()].downcast_ref::<u32>(),
        Some(&8)
    );
    assert!(copied.exact_measure_nodes.contains_key(roots[0].into()));
    assert!(copied.exact_measure_caches.is_empty());
    assert_eq!(
        copied.nodes[roots[0].into()].unrounded_layout,
        Layout::new()
    );
    let mut full = tree.clone();
    assert_eq!(
        measure(&mut copied, roots[1], input()),
        measure(&mut full, a, input())
    );
    // Raw callbacks are allowed to mutate private contexts, never originals.
    copied.get_node_context_mut(roots[0]).unwrap().value = 90.0;
    assert_eq!(tree.get_node_context(children[0]).unwrap().value, 10.0);
    assert_eq!(tree.nodes[children[0].into()], before);
}
#[test]
fn invalid_and_detached_nodes_empty_batch_and_generation_reuse_are_isolated() {
    let mut tree: TaffyTree = TaffyTree::new();
    let dead = tree.new_leaf(Style::DEFAULT).unwrap();
    tree.remove(dead).unwrap();
    let live = tree
        .new_leaf(Style {
            size: Size {
                width: Dimension::length(31.0),
                height: Dimension::length(12.0),
            },
            ..Style::DEFAULT
        })
        .unwrap();
    assert!(tree.clone_subtrees_for_measurement(&[live, dead]).is_err());
    let (empty, roots) = tree.clone_subtrees_for_measurement(&[]).unwrap();
    assert_eq!(empty.total_node_count(), 0);
    assert!(roots.is_empty());
    let (mut copied, roots) = tree.clone_subtrees_for_measurement(&[live]).unwrap();
    copied.compute_layout(roots[0], Size::MAX_CONTENT).unwrap();
    assert_eq!(
        copied.layout(roots[0]).unwrap().size,
        Size {
            width: 31.0,
            height: 12.0
        }
    );
    assert_eq!(tree.layout(live).unwrap().size, Size::ZERO);
}

#[derive(Debug)]
struct Parent;
impl crate::tree::CustomLayout for Parent {
    fn measure(
        &self,
        input: crate::tree::CustomLayoutInput,
        children: &mut dyn crate::tree::CustomLayoutChildren,
    ) -> TaffyResult<Option<crate::tree::CustomMeasureOutput>> {
        let child = children.measure(
            0,
            LayoutInput {
                available_space: input.available,
                ..LayoutInput::HIDDEN
            },
        )?;
        Ok(Some(crate::tree::CustomMeasureOutput {
            size: child.size,
            baselines: child.baselines,
        }))
    }
    fn layout(
        &self,
        _: crate::tree::CustomLayoutInput,
        _: &mut dyn crate::tree::CustomLayoutChildren,
    ) -> TaffyResult<crate::tree::CustomLayoutOutput> {
        Err(TaffyError::InvalidCustomLayout)
    }
}
#[test]
fn custom_delegate_constraint_protocol_and_tracking_capabilities_survive_private_remap() {
    let mut tree: TaffyTree = TaffyTree::new();
    let child = tree
        .new_leaf(Style {
            size: Size {
                width: Dimension::length(20.0),
                height: Dimension::length(11.0),
            },
            ..Style::DEFAULT
        })
        .unwrap();
    let custom = tree.new_with_children(Style::DEFAULT, &[child]).unwrap();
    tree.set_custom_layout(custom, Some(Arc::new(Parent)))
        .unwrap();
    tree.set_track_layout_input(child, true).unwrap();
    let (mut query, roots) = tree.clone_subtrees_for_measurement(&[custom]).unwrap();
    let new_child = query.child_at_index(roots[0], 0).unwrap();
    assert!(Arc::ptr_eq(
        &tree.custom_layouts[custom.into()],
        &query.custom_layouts[roots[0].into()]
    ));
    assert!(query.tracked_layout_nodes.contains_key(new_child.into()));
    assert!(query.tracked_layout_inputs.is_empty());
    let output = query
        .measure_node_with_input(roots[0], input(), |input, _, _, style| {
            compute_leaf_layout(input, style, |_, _| 0.0, |_, _| Size::ZERO)
        })
        .unwrap();
    assert_eq!(
        output.size,
        Size {
            width: 20.0,
            height: 11.0
        }
    );
    // Invalid raw draft is preserved as invalid, not silently normalized.
    let other = tree.new_leaf(Style::DEFAULT).unwrap();
    let bad = tree
        .new_with_children(
            Style {
                display: Display::ConstraintBox {
                    width: crate::ConstraintFit::Tight,
                    height: crate::ConstraintFit::Loose,
                },
                ..Style::DEFAULT
            },
            &[custom, other],
        )
        .unwrap();
    let (mut query, roots) = tree.clone_subtrees_for_measurement(&[bad]).unwrap();
    assert!(matches!(
        query.measure_node_with_input(roots[0], input(), |_, _, _, _| LayoutOutput::HIDDEN),
        Err(TaffyError::InvalidConstraintBox)
    ));
}
