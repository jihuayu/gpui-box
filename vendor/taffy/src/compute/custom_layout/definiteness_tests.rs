use super::*;
use crate::style_helpers::TaffyMaxContent;
use crate::tree::CustomPlacement;
use crate::{Dimension, Style, TaffyTree};
use std::sync::Arc;
#[derive(Debug)]
struct Provenance;
impl CustomLayout for Provenance {
    fn layout(
        &self,
        input: CustomLayoutInput,
        children: &mut dyn CustomLayoutChildren,
    ) -> Result<CustomLayoutOutput, TaffyError> {
        assert_eq!(
            input.known_size_is_definite,
            input
                .known_size
                .zip_map(input.percentage_basis, |known, basis| known.is_none()
                    || basis.is_some())
        );
        let slot = LayoutInput {
            available_space: Size::MAX_CONTENT,
            parent_size: input.percentage_basis,
            ..LayoutInput::HIDDEN
        };
        let child = children.measure(0, slot)?;
        Ok(CustomLayoutOutput {
            size: input.known_size.unwrap_or(child.size),
            baselines: Baselines {
                first: Some(child.size.width),
                last: Some(child.size.height),
            },
            placements: vec![CustomPlacement {
                child: 0,
                input: slot,
                origin: Point::ZERO,
            }],
        })
    }
}

#[test]
fn custom_final_bases_and_exact_queries_keep_equal_sizes_with_distinct_definiteness() {
    let mut tree = TaffyTree::<()>::new();
    tree.disable_rounding();
    let child = tree
        .new_leaf(Style {
            size: Size {
                width: Dimension::percent(0.5),
                height: Dimension::percent(0.5),
            },
            ..Style::DEFAULT
        })
        .unwrap();
    let root = tree.new_with_children(Style::DEFAULT, &[child]).unwrap();
    tree.set_custom_layout(root, Some(Arc::new(Provenance)))
        .unwrap();
    for definite in [
        Size {
            width: false,
            height: false,
        },
        Size {
            width: true,
            height: true,
        },
        Size {
            width: false,
            height: true,
        },
        Size {
            width: true,
            height: false,
        },
        Size {
            width: false,
            height: false,
        },
    ] {
        for complete in [false, true] {
            let input = LayoutInput {
                known_dimensions: Size {
                    width: Some(80.0),
                    height: Some(60.0),
                },
                known_dimensions_are_definite: definite,
                available_space: Size::MAX_CONTENT,
                ..LayoutInput::HIDDEN
            };
            let measure = |input: LayoutInput, _: NodeId, _: Option<&mut ()>, style: &Style| {
                super::super::compute_leaf_layout(
                    input,
                    style,
                    |_, _| 0.0,
                    |known, _| {
                        known.unwrap_or(Size {
                            width: 10.0,
                            height: 12.0,
                        })
                    },
                )
            };
            let output = if complete {
                tree.measure_node_baselines_with_input(root, input, measure)
                    .unwrap()
            } else {
                tree.measure_node_with_input(root, input, measure).unwrap()
            };
            assert_eq!(
                output.size,
                Size {
                    width: 80.0,
                    height: 60.0
                }
            );
            assert_eq!(
                output.baselines,
                Baselines {
                    first: Some(if definite.width { 40.0 } else { 10.0 }),
                    last: Some(if definite.height { 30.0 } else { 12.0 }),
                }
            );
        }
    }
}
