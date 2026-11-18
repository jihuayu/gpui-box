//! Same solver/inputs, whole private clone versus cold compact subtree union.
use serde_json::json;
use std::{hint::black_box, time::Instant};
use taffy::{LayoutInput, LayoutOutput, RunMode, prelude::*};
fn leaf(
    input: LayoutInput,
    _: taffy::NodeId,
    context: Option<&mut f32>,
    style: &Style,
) -> LayoutOutput {
    let h = context.copied().unwrap_or(0.0);
    taffy::compute_leaf_layout(
        input,
        style,
        |_, _| 0.0,
        |_, _| Size {
            width: 40.0,
            height: h,
        },
    )
}
fn query(tree: &mut TaffyTree<f32>, root: NodeId) -> LayoutOutput {
    tree.measure_node_with_input(
        root,
        LayoutInput {
            run_mode: RunMode::ComputeSize,
            available_space: Size::MAX_CONTENT,
            ..LayoutInput::HIDDEN
        },
        leaf,
    )
    .unwrap()
}
fn summary(mut samples: Vec<f64>) -> serde_json::Value {
    samples.sort_by(f64::total_cmp);
    json!({"p50_us":samples[samples.len()/2],"p95_us":samples[samples.len()*95/100]})
}
fn main() {
    let mut results = vec![];
    for count in [1_000, 10_000, 30_000] {
        for selected in [8, count] {
            let mut tree = TaffyTree::<f32>::new();
            tree.disable_rounding();
            let leaves = (0..count)
                .map(|i| {
                    tree.new_leaf_with_context(
                        Style {
                            flex_shrink: 0.0,
                            ..Style::DEFAULT
                        },
                        20.0 + (i % 3) as f32,
                    )
                    .unwrap()
                })
                .collect::<Vec<_>>();
            let root = tree
                .new_with_children(
                    Style {
                        flex_direction: FlexDirection::Column,
                        ..Style::DEFAULT
                    },
                    &leaves,
                )
                .unwrap();
            tree.compute_layout_with_measure(root, Size::MAX_CONTENT, leaf)
                .unwrap();
            let targets = if selected == count {
                vec![root]
            } else {
                leaves[count / 2..count / 2 + selected].to_vec()
            };
            let (compact, mapped) = tree.clone_subtrees_for_measurement(&targets).unwrap();
            let copied = compact.total_node_count();
            drop(compact);
            let mut old = tree.clone();
            let expected = targets
                .iter()
                .map(|id| query(&mut old, *id))
                .collect::<Vec<_>>();
            drop(old);
            let mut a = vec![];
            let mut b = vec![];
            for sample in 0..22 {
                let full = || {
                    let start = Instant::now();
                    let mut private = tree.clone();
                    let output = targets
                        .iter()
                        .map(|id| query(&mut private, *id))
                        .collect::<Vec<_>>();
                    black_box(&output);
                    drop(private);
                    let elapsed = start.elapsed().as_secs_f64() * 1e6;
                    assert_eq!(output, expected);
                    elapsed
                };
                let sparse = || {
                    let start = Instant::now();
                    let (mut private, mapped) =
                        tree.clone_subtrees_for_measurement(&targets).unwrap();
                    let output = mapped
                        .iter()
                        .map(|id| query(&mut private, *id))
                        .collect::<Vec<_>>();
                    black_box(&output);
                    drop(private);
                    let elapsed = start.elapsed().as_secs_f64() * 1e6;
                    assert_eq!(output, expected);
                    elapsed
                };
                let (x, y) = if sample % 2 == 0 {
                    (full(), sparse())
                } else {
                    let y = sparse();
                    (full(), y)
                };
                if sample >= 2 {
                    a.push(x);
                    b.push(y);
                }
            }
            black_box(mapped);
            results.push(json!({"live_nodes":tree.total_node_count(),"query_roots":targets.len(),"copied_nodes":copied,"whole_clone":summary(a),"compact_union":summary(b)}));
        }
    }
    println!("{}",serde_json::to_string_pretty(&json!({"scope":"private direct-child measurement, same Taffy solver; warm whole-clone caches vs cold compact union, includes copying/query/drop, excludes UI and rendering","results":results})).unwrap());
}
