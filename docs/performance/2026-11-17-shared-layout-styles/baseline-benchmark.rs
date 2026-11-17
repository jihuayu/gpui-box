use super::*;
use std::{hint::black_box, time::Instant};

fn scene(grid: bool) -> (UiTree, NodeId, NodeId) {
    let mut tree = UiTree::new_layout(
        LayoutStyle::column().size(LayoutDimension::Percent(1.0), LayoutDimension::Auto),
        0,
    )
    .unwrap();
    let (group, leaf) = tree
        .edit(BackendCapabilities::default(), |edit| {
            let mut first = None;
            for _ in 0..128 {
                let style = if grid {
                    LayoutStyle::grid(
                        GridStyle::default().columns(
                            (0..16)
                                .map(|_| GridTemplate::Track(GridTrack::px(12.0)))
                                .collect(),
                        ),
                    )
                } else {
                    LayoutStyle::row()
                };
                let group = edit.append_layout(edit.root(), style, 0)?;
                for _ in 0..16 {
                    let leaf = edit.append_layout(
                        group,
                        LayoutStyle::row().size(LayoutDimension::Px(8.0), LayoutDimension::Px(8.0)),
                        0xff0000ff,
                    )?;
                    first.get_or_insert((group, leaf));
                }
            }
            Ok(first.unwrap())
        })
        .unwrap()
        .value;
    tree.measure_with_constraints(bounds(512.0)).unwrap();
    (tree, group, leaf)
}

fn bounds(width: f32) -> LayoutConstraints {
    LayoutConstraints::new(
        LayoutAvailableSpace::Definite(width),
        LayoutAvailableSpace::Unbounded,
    )
}

fn sample(name: &str, samples: usize, mut run: impl FnMut(usize)) {
    for i in 0..12 {
        run(i);
    }
    let mut times = Vec::with_capacity(samples);
    for i in 0..samples {
        let start = Instant::now();
        run(i);
        times.push(start.elapsed().as_secs_f64() * 1e6);
    }
    times.sort_by(f64::total_cmp);
    println!(
        "STYLE_BENCH,{name},{samples},{:.3},{:.3},{:.3}",
        times[samples / 2],
        times[(samples * 95 / 100).min(samples - 1)],
        times[samples - 1],
    );
}

/// Run the exact same executable before/after changing snapshot storage. Each
/// timed operation includes destruction of its temporary draft/result. This is
/// owner work, without GPU, presentation or an application timing claim.
#[test]
#[ignore = "release-mode comparative timing; run explicitly with one test thread"]
fn style_snapshot_benchmark() {
    let samples = std::env::var("GPUI_STYLE_SAMPLES")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(100);
    assert!(samples >= 20);
    println!(
        "STYLE_SIZE,Style,{}\nSTYLE_SIZE,Node,{}\nSTYLE_SIZE,Slot,{}",
        std::mem::size_of::<Style>(),
        std::mem::size_of::<Node>(),
        std::mem::size_of::<Slot>(),
    );
    for grid in [false, true] {
        let name = if grid { "grid" } else { "flex" };
        let (mut tree, group, leaf) = scene(grid);
        sample(&format!("{name}_clone"), samples, |_| {
            black_box(tree.clone_for_layout_plan());
        });
        sample(&format!("{name}_dry_subtree"), samples, |i| {
            black_box(
                tree.measure_node_with_constraints(group, bounds(180.0 + (i % 2) as f32))
                    .unwrap(),
            );
        });
        sample(&format!("{name}_atomic_layout"), samples, |i| {
            black_box(
                tree.edit_validated(
                    BackendCapabilities::default(),
                    |edit| {
                        edit.properties().layout_style(
                            leaf,
                            LayoutStyle::row().size(
                                LayoutDimension::Px(8.0 + (i % 2) as f32),
                                LayoutDimension::Px(8.0),
                            ),
                        );
                        Ok(())
                    },
                    |draft| {
                        black_box(draft.measure_with_constraints(bounds(512.0))?);
                        Ok(())
                    },
                )
                .unwrap(),
            );
        });
        sample(&format!("{name}_relayout"), samples, |i| {
            black_box(
                tree.measure_with_constraints(bounds(500.0 + (i % 2) as f32))
                    .unwrap(),
            );
        });
        sample(&format!("{name}_create_layout"), samples, |_| {
            black_box(scene(grid));
        });
    }
}
