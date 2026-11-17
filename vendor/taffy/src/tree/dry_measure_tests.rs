use super::*;
use crate::style_helpers::TaffyMaxContent;
use crate::tree::{
    CustomLayout, CustomLayoutChildren, CustomLayoutInput, CustomLayoutOutput, CustomMeasureOutput, CustomPlacement,
};
use crate::{Baselines, Dimension, Point};
use std::sync::atomic::{AtomicUsize, Ordering};

#[derive(Debug)]
struct Dry {
    calls: Arc<AtomicUsize>,
    plans: Arc<AtomicUsize>,
    bad: bool,
}
impl CustomLayout for Dry {
    fn measure(
        &self,
        input: CustomLayoutInput,
        children: &mut dyn CustomLayoutChildren,
    ) -> TaffyResult<Option<CustomMeasureOutput>> {
        self.calls.fetch_add(1, Ordering::Relaxed);
        let mut slot = LayoutInput::HIDDEN;
        slot.available_space = input.available;
        slot.parent_size = input.percentage_basis;
        let child = children.measure(0, slot)?;
        Ok(Some(CustomMeasureOutput {
            size: input.known_size.unwrap_or(child.size),
            baselines: Baselines {
                first: Some(3.0),
                last: Some(7.0),
            },
        }))
    }
    fn layout(
        &self,
        input: CustomLayoutInput,
        children: &mut dyn CustomLayoutChildren,
    ) -> TaffyResult<CustomLayoutOutput> {
        self.plans.fetch_add(1, Ordering::Relaxed);
        let mut slot = LayoutInput::HIDDEN;
        slot.available_space = input.available;
        slot.parent_size = input.percentage_basis;
        let child = children.measure(0, slot)?;
        Ok(CustomLayoutOutput {
            size: input.known_size.unwrap_or(child.size),
            baselines: Baselines {
                first: Some(3.0),
                last: Some(if self.bad { 8.0 } else { 7.0 }),
            },
            placements: vec![CustomPlacement {
                child: 0,
                input: slot,
                origin: Point::ZERO,
            }],
        })
    }
}
fn fixture(bad: bool) -> (TaffyTree, NodeId, NodeId, Arc<AtomicUsize>, Arc<AtomicUsize>) {
    let mut tree = TaffyTree::new();
    tree.disable_rounding();
    let child = tree
        .new_leaf(Style {
            size: Size {
                width: Dimension::length(20.0),
                height: Dimension::length(10.0),
            },
            ..Style::DEFAULT
        })
        .unwrap();
    let root = tree.new_with_children(Style::DEFAULT, &[child]).unwrap();
    let calls = Arc::new(AtomicUsize::new(0));
    let plans = Arc::new(AtomicUsize::new(0));
    tree.set_custom_layout(
        root,
        Some(Arc::new(Dry {
            calls: calls.clone(),
            plans: plans.clone(),
            bad,
        })),
    )
    .unwrap();
    (tree, root, child, calls, plans)
}
fn measure(tree: &mut TaffyTree, node: NodeId, input: LayoutInput) -> TaffyResult<LayoutOutput> {
    tree.measure_node_with_input(node, input, |input, _, _, style| {
        compute_leaf_layout(input, style, |_, _| 0.0, |_, _| Size::ZERO)
    })
}
#[test]
fn dry_api_forces_compute_size_reuses_cache_and_preserves_published_layouts() {
    let (mut tree, root, child, calls, plans) = fixture(false);
    let input = LayoutInput {
        available_space: Size::MAX_CONTENT,
        run_mode: RunMode::PerformLayout,
        ..LayoutInput::HIDDEN
    };
    let output = measure(&mut tree, root, input).unwrap();
    assert_eq!(
        output.size,
        Size {
            width: 20.0,
            height: 10.0
        }
    );
    assert_eq!(
        output.baselines,
        Baselines {
            first: Some(3.0),
            last: Some(7.0)
        }
    );
    assert_eq!(plans.load(Ordering::Relaxed), 0);
    assert_eq!(tree.layout(child).unwrap().size, Size::ZERO);
    assert_eq!(tree.layout(root).unwrap().size, Size::ZERO);
    let before = calls.load(Ordering::Relaxed);
    assert_eq!(measure(&mut tree, root, input).unwrap(), output);
    assert_eq!(calls.load(Ordering::Relaxed), before);
    tree.compute_layout(root, Size::MAX_CONTENT).unwrap();
    assert_eq!(plans.load(Ordering::Relaxed), 1);
    let published = *tree.layout(child).unwrap();
    let mut other = input;
    other.parent_size.width = Some(200.0);
    measure(&mut tree, root, other).unwrap();
    assert!(calls.load(Ordering::Relaxed) > before);
    assert_eq!(*tree.layout(child).unwrap(), published);
    assert_eq!(plans.load(Ordering::Relaxed), 1);
    let mut bad = input;
    bad.available_space.width = AvailableSpace::Definite(f32::NAN);
    assert!(measure(&mut tree, root, bad).is_err());
    bad = input;
    bad.size_bounds.min.width = 2.0;
    bad.size_bounds.max.width = 1.0;
    assert_eq!(measure(&mut tree, root, bad), Err(TaffyError::InvalidSizeBounds));
    assert!(measure(&mut tree, NodeId::from(u64::MAX), input).is_err());
    tree.set_style(
        child,
        Style {
            size: Size {
                width: Dimension::length(30.0),
                height: Dimension::length(10.0),
            },
            ..Style::DEFAULT
        },
    )
    .unwrap();
    assert_eq!(measure(&mut tree, root, input).unwrap().size.width, 30.0);
}
#[test]
fn successful_dry_result_cannot_authorize_inconsistent_final_plan_or_cache_error() {
    let (mut tree, root, _, _, plans) = fixture(true);
    let input = LayoutInput {
        available_space: Size::MAX_CONTENT,
        ..LayoutInput::HIDDEN
    };
    assert!(measure(&mut tree, root, input).is_ok());
    assert_eq!(
        tree.compute_layout(root, Size::MAX_CONTENT),
        Err(TaffyError::InvalidCustomLayout)
    );
    let before = plans.load(Ordering::Relaxed);
    assert_eq!(
        tree.compute_layout(root, Size::MAX_CONTENT),
        Err(TaffyError::InvalidCustomLayout)
    );
    assert!(
        plans.load(Ordering::Relaxed) > before,
        "failed full plans are not cached as successful"
    );
}
