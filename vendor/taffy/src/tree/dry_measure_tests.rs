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

fn baseline_leaf(input: LayoutInput, _: NodeId, _: Option<&mut ()>, style: &Style) -> LayoutOutput {
    let mut output = compute_leaf_layout(input, style, |_, _| 0.0, |_, _| Size { width: 20.0, height: 10.0 });
    output.baselines = Baselines { first: Some(3.0), last: Some(7.0) };
    output
}

#[test]
fn explicit_baseline_query_bypasses_size_caches_and_preserves_all_tree_state() {
    let mut cases = 0;
    for direction in [crate::Direction::Ltr, crate::Direction::Rtl] {
        for flex_direction in [
            crate::FlexDirection::Row,
            crate::FlexDirection::RowReverse,
            crate::FlexDirection::Column,
            crate::FlexDirection::ColumnReverse,
        ] {
            for flex_wrap in [crate::FlexWrap::NoWrap, crate::FlexWrap::Wrap, crate::FlexWrap::WrapReverse] {
                for align in [crate::AlignItems::START, crate::AlignItems::BASELINE, crate::AlignItems::LAST_BASELINE] {
                    let mut tree = TaffyTree::new();
                    tree.disable_rounding();
                    let child = tree
                        .new_leaf(Style {
                            size: Size { width: Dimension::length(20.0), height: Dimension::length(10.0) },
                            ..Style::DEFAULT
                        })
                        .unwrap();
                    let other = tree
                        .new_leaf(Style {
                            size: Size { width: Dimension::length(20.0), height: Dimension::length(20.0) },
                            ..Style::DEFAULT
                        })
                        .unwrap();
                    let hidden = tree.new_leaf(Style { display: Display::None, ..Style::DEFAULT }).unwrap();
                    let absolute = tree.new_leaf(Style {
                        position: crate::Position::Absolute,
                        size: Size { width: Dimension::length(200.0), height: Dimension::length(200.0) },
                        ..Style::DEFAULT
                    }).unwrap();
                    let root = tree
                        .new_with_children(
                            Style {
                                size: Size { width: Dimension::length(30.0), height: Dimension::length(50.0) },
                                direction,
                                flex_direction,
                                flex_wrap,
                                align_items: align,
                                padding: crate::Rect {
                                    left: crate::LengthPercentage::length(2.0),
                                    right: crate::LengthPercentage::length(2.0),
                                    top: crate::LengthPercentage::length(2.0),
                                    bottom: crate::LengthPercentage::length(2.0),
                                },
                                ..Style::DEFAULT
                            },
                            &[child, other, hidden, absolute],
                        )
                        .unwrap();
                    let input = LayoutInput { available_space: Size::MAX_CONTENT, ..LayoutInput::HIDDEN };
                    // Warm lossy size caches and publish actual geometry first.
                    tree.measure_node_with_input(root, input, baseline_leaf).unwrap();
                    tree.compute_layout_with_measure(root, Size::MAX_CONTENT, baseline_leaf).unwrap();
                    let before = format!("{tree:?}");
                    let query = tree.measure_node_baselines_with_input(root, input, baseline_leaf).unwrap();
                    assert_eq!(format!("{tree:?}"), before);
                    let repeated = tree.measure_node_baselines_with_input(root, input, baseline_leaf).unwrap();
                    assert_eq!(query, repeated);
                    assert_eq!(format!("{tree:?}"), before);
                    let mut reference = tree.clone();
                    reference.mark_dirty(root).unwrap();
                    let mut view = TaffyView {
                        taffy: &mut reference,
                        measure_function: baseline_leaf,
                        layout_observer: |_, _, _| {},
                        layout_error: None,
                        exact_measure: true,
                        scratch_baselines: false,
                    };
                    let full = view
                        .compute_custom_child_layout(root, LayoutInput { run_mode: RunMode::PerformLayout, ..input });
                    assert_eq!(
                        (query.size, query.baselines),
                        (full.size, full.baselines),
                        "{direction:?}/{flex_direction:?}/{flex_wrap:?}/{align:?}"
                    );
                    assert!(query.baselines.first.is_some());
                    assert!(query.baselines.last.is_some());
                    cases += 1;
                }
            }
        }
    }
    assert_eq!(cases, 72);
}

#[test]
fn explicit_baseline_query_returns_real_nested_metrics_without_authorizing_final_cache() {
    let mut tree = TaffyTree::new();
    tree.disable_rounding();
    let leaf = tree.new_leaf(Style::DEFAULT).unwrap();
    let root = tree
        .new_with_children(
            Style {
                size: Size { width: Dimension::length(50.0), height: Dimension::length(30.0) },
                align_items: crate::AlignItems::START,
                padding: crate::Rect {
                    left: crate::LengthPercentage::length(2.0),
                    right: crate::LengthPercentage::length(2.0),
                    top: crate::LengthPercentage::length(2.0),
                    bottom: crate::LengthPercentage::length(2.0),
                },
                ..Style::DEFAULT
            },
            &[leaf],
        )
        .unwrap();
    let input = LayoutInput { available_space: Size::MAX_CONTENT, ..LayoutInput::HIDDEN };
    let before = format!("{tree:?}");
    let output = tree.measure_node_baselines_with_input(root, input, baseline_leaf).unwrap();
    assert_eq!(output.size, Size { width: 50.0, height: 30.0 });
    assert_eq!(output.baselines, Baselines { first: Some(5.0), last: Some(9.0) });
    assert_eq!(format!("{tree:?}"), before);
    assert!(tree.dirty(root).unwrap());
    assert!(tree.dirty(leaf).unwrap());
    tree.compute_layout_with_measure(root, Size::MAX_CONTENT, baseline_leaf).unwrap();
    assert_eq!(tree.layout(leaf).unwrap().location.y, 2.0);
    assert_eq!(tree.layout(root).unwrap().baselines, output.baselines);
    tree.set_style(root, Style { display: Display::None, ..Style::DEFAULT }).unwrap();
    let before = format!("{tree:?}");
    assert_eq!(tree.measure_node_baselines_with_input(root, input, baseline_leaf).unwrap().baselines, Baselines::NONE);
    assert_eq!(format!("{tree:?}"), before, "hidden recursion must not clear published layouts/caches");
}

#[test]
fn explicit_baselines_preserve_protocol_observations_nested_errors_and_supported_container_metrics() {
    let (mut custom, root, child, _, plans) = fixture(false);
    custom.compute_layout_with_measure(root, Size::MAX_CONTENT, baseline_leaf).unwrap();
    let published = (*custom.layout(root).unwrap(), *custom.layout(child).unwrap());
    let observed = custom.custom_child_inputs.clone();
    let exact = custom.exact_layout_inputs.clone();
    let before = plans.load(Ordering::Relaxed);
    let input = LayoutInput { available_space: Size { width: AvailableSpace::Definite(80.0), height: AvailableSpace::Unbounded }, ..LayoutInput::HIDDEN };
    let query = custom.measure_node_baselines_with_input(root, input, baseline_leaf).unwrap();
    assert_eq!(query.baselines, Baselines { first: Some(3.0), last: Some(7.0) });
    assert!(plans.load(Ordering::Relaxed) > before, "baseline-complete custom queries execute full plans");
    assert_eq!((*custom.layout(root).unwrap(), *custom.layout(child).unwrap()), published);
    assert_eq!(custom.custom_child_inputs, observed);
    assert_eq!(custom.exact_layout_inputs, exact);
    let (mut bad, root, _, _, _) = fixture(true);
    assert_eq!(bad.measure_node_baselines_with_input(root, input, baseline_leaf), Err(TaffyError::InvalidCustomLayout));
    assert!(bad.dirty(root).unwrap());

    let mut displays = vec![Display::Flex, Display::ConstraintBox { width: crate::ConstraintFit::Loose, height: crate::ConstraintFit::Loose }];
    #[cfg(feature = "block_layout")]
    displays.extend([Display::Block, Display::FlowRoot]);
    #[cfg(feature = "grid")]
    displays.push(Display::Grid);
    for display in displays {
        let mut tree = TaffyTree::new();
        tree.disable_rounding();
        let child = tree.new_leaf(Style::DEFAULT).unwrap();
        let root = tree.new_with_children(Style {
            display,
            size: Size { width: Dimension::length(50.0), height: Dimension::length(30.0) },
            contain: crate::Contain::SIZE,
            align_items: crate::AlignItems::START,
            ..Style::DEFAULT
        }, &[child]).unwrap();
        tree.compute_layout_with_measure(root, Size::MAX_CONTENT, baseline_leaf).unwrap();
        let before = format!("{tree:?}");
        let query = tree.measure_node_baselines_with_input(root, input, baseline_leaf).unwrap();
        assert_eq!(format!("{tree:?}"), before, "{display:?}");
        assert_eq!(query.baselines.first, tree.layout(root).unwrap().baselines.first, "{display:?}");
        assert_eq!(query.baselines.last, tree.layout(root).unwrap().baselines.last, "{display:?}");
        assert_eq!(query.size, tree.layout(root).unwrap().size, "{display:?}");
        assert_eq!(tree.contained_layout_inputs.len(), 1);
    }
}
