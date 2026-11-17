use super::*;
use crate::style_helpers::TaffyMaxContent;
use crate::{RequestedAxis, Style, TaffyTree};
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};

#[derive(Debug)]
struct Demand(Arc<AtomicUsize>);
impl CustomLayout for Demand {
    fn measure(
        &self,
        input: CustomLayoutInput,
        _: &mut dyn CustomLayoutChildren,
    ) -> Result<Option<CustomMeasureOutput>, TaffyError> {
        self.0.fetch_add(1, Ordering::Relaxed);
        Ok(Some(CustomMeasureOutput {
            size: Size {
                width: input.known_size.width.unwrap_or(80.0),
                height: input.known_size.height.unwrap_or(
                    if input.requested_axis == RequestedAxis::Horizontal {
                        0.0
                    } else {
                        40.0
                    },
                ),
            },
            baselines: Baselines::NONE,
        }))
    }
    fn layout(
        &self,
        input: CustomLayoutInput,
        children: &mut dyn CustomLayoutChildren,
    ) -> Result<CustomLayoutOutput, TaffyError> {
        assert_eq!(input.requested_axis, RequestedAxis::Both);
        let output = self.measure(input, children)?.unwrap();
        Ok(CustomLayoutOutput {
            size: output.size,
            baselines: output.baselines,
            placements: vec![],
        })
    }
}

#[test]
fn scalar_and_complete_results_never_alias_in_exact_cache() {
    let mut tree = TaffyTree::<()>::new();
    let root = tree.new_leaf(Style::DEFAULT).unwrap();
    let calls = Arc::new(AtomicUsize::new(0));
    tree.set_custom_layout(root, Some(Arc::new(Demand(calls.clone()))))
        .unwrap();
    let input = LayoutInput {
        available_space: Size::MAX_CONTENT,
        axis: RequestedAxis::Horizontal,
        ..LayoutInput::HIDDEN
    };
    let leaf = |_: LayoutInput, _: NodeId, _: Option<&mut ()>, _: &Style| LayoutOutput::HIDDEN;
    let width = tree.measure_node_with_input(root, input, leaf).unwrap();
    assert_eq!(
        width.size,
        Size {
            width: 80.0,
            height: 0.0
        }
    );
    assert_eq!(calls.load(Ordering::Relaxed), 1);
    tree.measure_node_with_input(root, input, leaf).unwrap();
    assert_eq!(calls.load(Ordering::Relaxed), 1);
    let complete = tree
        .measure_node_with_input(
            root,
            LayoutInput {
                axis: RequestedAxis::Both,
                ..input
            },
            leaf,
        )
        .unwrap();
    assert_eq!(
        complete.size,
        Size {
            width: 80.0,
            height: 40.0
        }
    );
    let before = calls.load(Ordering::Relaxed);
    let again = tree.measure_node_with_input(root, input, leaf).unwrap();
    assert_eq!(again.size, width.size);
    assert_eq!(calls.load(Ordering::Relaxed), before);
    let baseline = tree
        .measure_node_baselines_with_input(root, input, leaf)
        .unwrap();
    assert_eq!(baseline.size, complete.size);
}
