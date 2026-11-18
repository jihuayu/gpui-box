//! Identical public-API control: repeated ordinary nested Flex construction.
use gpui_box::retained::{logical::*, *};
use std::{hint::black_box, time::Instant};

fn main() {
    let samples = std::env::var("GPUI_PLAN_SAMPLES")
        .ok()
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or(100);
    println!("tree,operation,sample,nanoseconds");
    for count in [64, 512] {
        for operation in ["unchanged_plan", "leaf_color"] {
            let mut ui = LogicalUi::new_layout(
                LayoutStyle::column()
                    .size(LayoutDimension::Percent(1.0), LayoutDimension::Auto),
                0,
                BackendCapabilities::default(),
            )
            .unwrap();
            let root = ui.root_render();
            let scope = ui.root_scope();
            let outer = ui.append_render_layout(scope, root, LayoutStyle::column(), 0).unwrap();
            let inner = ui.append_render_layout(scope, outer, LayoutStyle::column(), 0).unwrap();
            let mut leaf = None;
            for _ in 0..count {
                leaf = Some(ui.append_render_layout(
                    scope, inner,
                    LayoutStyle::row().size(LayoutDimension::Px(80.0), LayoutDimension::Px(4.0)),
                    0x40aaffff,
                ).unwrap());
            }
            let leaf = leaf.unwrap();
            let constraints = LayoutConstraints::new(
                LayoutAvailableSpace::Definite(240.0), LayoutAvailableSpace::Unbounded,
            );
            let mut previous = ui.local_frame_in_viewport(constraints, [240.0, 600.0]).unwrap();
            let revision = previous.revision();
            for sample in 0..(samples + 16) {
                let started = Instant::now();
                let (_, frame) = ui.edit_local_frame(
                    constraints, [240.0, 600.0], |_| Ok(()), |(), edit| {
                        let mut plan = LayoutBuildPlan::new();
                        plan.add(LayoutBuildTaskId(1), inner, LayoutBuildKind::InlineAllocation, &[], |_, _, _| Ok(()))?;
                        plan.add(LayoutBuildTaskId(90), outer, LayoutBuildKind::InlineAllocation, &[], |_, _, _| Ok(()))?;
                        edit.build_layout_plan(plan)?;
                        if operation == "leaf_color" {
                            edit.properties().color(leaf, if sample % 2 == 0 { 0xff0000ff } else { 0x0000ffff });
                        }
                        Ok(())
                    },
                ).unwrap();
                assert_eq!(frame.len(), count + 3);
                if operation == "unchanged_plan" { assert_eq!(frame.revision(), revision); }
                black_box(frame.stats());
                previous = black_box(frame);
                let elapsed = started.elapsed().as_nanos();
                if sample >= 16 {
                    println!("flex{count},{operation},{},{elapsed}", sample-16);
                }
            }
            black_box(previous);
        }
    }
}
