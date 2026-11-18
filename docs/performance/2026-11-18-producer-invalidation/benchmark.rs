//! Same public-API harness for baseline and indexed producer invalidation.
use gpui_box::retained::{logical::*, *};
use std::{hint::black_box, time::Instant};

fn main() {
    let samples = std::env::var("GPUI_PLAN_SAMPLES")
        .unwrap_or_else(|_| "100".into())
        .parse::<usize>()
        .unwrap();
    println!("tree,operation,sample,nanoseconds");
    for producers in [8usize, 32] {
        for operation in ["unread_source", "equal_source", "observed_source"] {
            let mut ui = LogicalUi::new_layout(
                LayoutStyle::column().size(LayoutDimension::Percent(1.0), LayoutDimension::Auto),
                0,
                BackendCapabilities::default(),
            )
            .unwrap();
            let root = ui.root_render();
            let scope = ui.root_scope();
            let read = ui.state(scope, 0, || 0u32).unwrap();
            let unread = ui.state(scope, 1, || 0u32).unwrap();
            let mut hosts = vec![];
            for _ in 0..producers {
                let host = ui
                    .append_render_layout(scope, root, LayoutStyle::column(), 0)
                    .unwrap();
                for _ in 0..4 {
                    ui.append_render_layout(
                        scope,
                        host,
                        LayoutStyle::row()
                            .size(LayoutDimension::Px(12.0), LayoutDimension::Px(12.0)),
                        0x3388aaff,
                    )
                    .unwrap();
                }
                hosts.push(host);
            }
            let constraints = LayoutConstraints::new(
                LayoutAvailableSpace::Definite(240.0),
                LayoutAvailableSpace::Unbounded,
            );
            ui.edit_local_frame(
                constraints,
                [240.0, 600.0],
                |_| Ok(()),
                |(), edit| {
                    for (index, host) in hosts.iter().copied().enumerate() {
                        edit.layout_producer(
                            scope,
                            index as u64,
                            host,
                            LayoutBuildKind::InlineAllocation,
                            &[],
                            LayoutProducer::new(move |_, edit| {
                                black_box(*edit.read(read)?);
                                Ok(())
                            }),
                        )?;
                    }
                    Ok(())
                },
            )
            .unwrap();
            let revision = ui.published_local_frame().unwrap().revision();
            for sample in 0..samples + 16 {
                let started = Instant::now();
                let changed = if operation == "unread_source" {
                    unread
                } else {
                    read
                };
                ui.transaction(|tx| {
                    tx.set(
                        changed,
                        if operation == "equal_source" {
                            0
                        } else {
                            (sample % 2 + 1) as u32
                        },
                    )
                })
                .unwrap();
                assert_eq!(ui.published_local_frame().unwrap().revision(), revision);
                black_box(ui.published_local_frame().unwrap());
                let nanos = started.elapsed().as_nanos();
                if sample >= 16 {
                    println!("producers{producers},{operation},{},{nanos}", sample - 16);
                }
            }
        }
    }
}
