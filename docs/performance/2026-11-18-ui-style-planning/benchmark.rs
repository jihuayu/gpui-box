//! Identical public-API harness for f3f1587 and the incremental planner.
use gpui_box::retained::{responsive::LayoutEnvironment, style::*, ui_ir::*, *};
use std::{hint::black_box, time::Instant};

fn fixture(grid: bool) -> (UiDocument, UiResources, Vec<UiNodeId>) {
    let root = UiNodeId::new(0, 1);
    let mut nodes = vec![UiNode::new(
        root,
        LayoutStyle::column().size(LayoutDimension::Px(240.0), LayoutDimension::Auto),
    )];
    let mut leaves = vec![];
    let mut next = 1;
    let shared = |width: bool| {
        StyleRef::new(
            "leaf",
            vec![
                StyleDeclaration::Color(StyleRule::value(0xff0000ff).when(
                    if width {
                        StyleCondition {
                            min_width: Some(200.0),
                            ..Default::default()
                        }
                    } else {
                        StyleCondition {
                            hovered: Some(true),
                            ..Default::default()
                        }
                    },
                    0x0000ffff,
                )),
                StyleDeclaration::Padding(StyleRule::value(LayoutEdges::all(LayoutLength::Px(
                    1.0,
                )))),
                StyleDeclaration::Opacity(StyleRule::value(1.0)),
                StyleDeclaration::Translation(StyleRule::value([0.0; 2])),
                StyleDeclaration::Decoration(StyleRule::value(QuadDecoration::default())),
                StyleDeclaration::FlexGrow(StyleRule::value(0.0)),
            ],
        )
        .unwrap()
    };
    let hover = shared(false);
    let environment = shared(true);
    for group in 0..128 {
        let id = UiNodeId::new(next, 1);
        next += 1;
        nodes[0].children.push(id);
        let style = if grid {
            LayoutStyle::grid(GridStyle::default().columns(vec![GridTrack::px(12.0).into(); 16]))
        } else {
            LayoutStyle::row()
        };
        let mut parent = UiNode::new(id, style);
        for index in 0..16 {
            let id = UiNodeId::new(next, 1);
            next += 1;
            parent.children.push(id);
            let mut leaf = UiNode::new(
                id,
                LayoutStyle::row()
                    .size(LayoutDimension::Px(12.0), LayoutDimension::Px(8.0))
                    .min_size(LayoutDimension::Px(0.0), LayoutDimension::Auto),
            );
            leaf.styles = vec![if (group * 16 + index) % 16 == 0 {
                environment.clone()
            } else {
                hover.clone()
            }];
            leaves.push(id);
            nodes.push(leaf);
        }
        nodes.push(parent);
    }
    assert_eq!(nodes.len(), 2177);
    let mut snapshot = UiSnapshot::new(300, 1, root, nodes);
    snapshot.style_environment = LayoutEnvironment::new([120.0, 600.0]);
    let resources = UiResources::new();
    let (document, _) = UiDocument::from_snapshot(
        snapshot,
        &resources,
        UiLimits::default(),
        BackendCapabilities::default(),
        LayoutConstraints::viewport(240.0, 600.0),
        [240.0, 600.0],
    )
    .unwrap();
    (document, resources, leaves)
}
fn main() {
    let samples = std::env::var("GPUI_PLAN_SAMPLES")
        .ok()
        .map(|v| v.parse::<usize>().unwrap())
        .unwrap_or(100);
    let warmups = 12;
    println!("kind,tree,operation,index,nanoseconds");
    for grid in [false, true] {
        let tree = if grid { "grid" } else { "flex" };
        for operation in [
            "single_hover",
            "dense_hover",
            "sparse_environment",
            "metadata_only",
            "equal_styles",
        ] {
            let (mut document, resources, leaves) = fixture(grid);
            let equal = document.node(leaves[1]).unwrap().styles.clone();
            for index in 0..samples + warmups {
                let high = index % 2 == 0;
                let operations = match operation {
                    "single_hover" => vec![UiOperation::StyleState {
                        node: leaves[1],
                        value: StyleState {
                            hovered: high,
                            ..Default::default()
                        },
                    }],
                    "dense_hover" => leaves
                        .iter()
                        .map(|&node| UiOperation::StyleState {
                            node,
                            value: StyleState {
                                hovered: high,
                                ..Default::default()
                            },
                        })
                        .collect(),
                    "sparse_environment" => {
                        vec![UiOperation::StyleEnvironment(LayoutEnvironment::new([
                            if high { 240.0 } else { 120.0 },
                            600.0,
                        ]))]
                    }
                    "metadata_only" => {
                        let mut environment = LayoutEnvironment::new([120.0, 600.0]);
                        environment.device_scale = if high { 2.0 } else { 1.0 };
                        vec![UiOperation::StyleEnvironment(environment)]
                    }
                    "equal_styles" => vec![UiOperation::Styles {
                        node: leaves[1],
                        value: equal.clone(),
                    }],
                    _ => unreachable!(),
                };
                let batch = UiBatch {
                    schema: UI_SCHEMA_VERSION,
                    runtime: 300,
                    epoch: 1,
                    base_revision: document.revision(),
                    revision: document.revision() + 1,
                    sequence: document.next_sequence(),
                    operations,
                };
                let before = Instant::now();
                let output = document
                    .apply(
                        batch,
                        &resources,
                        BackendCapabilities::default(),
                        LayoutConstraints::viewport(240.0, 600.0),
                        [240.0, 600.0],
                    )
                    .unwrap();
                let request = output.commit.request_frame;
                black_box(&output);
                drop(output);
                let elapsed = before.elapsed().as_nanos();
                if matches!(operation, "metadata_only" | "equal_styles") {
                    assert!(!request);
                }
                if index >= warmups {
                    println!("sample,{tree},{operation},{},{elapsed}", index - warmups);
                }
            }
        }
    }
}
