//! Intrinsic update CPU benchmark, including frame extraction but no GPU work.
use gpui_box::retained::*;
use serde_json::{Value, json};
use std::{error::Error as StdError, fs, hint::black_box, path::PathBuf, time::Instant};

fn measure(height: f32) -> Option<MeasureSnapshot> {
    Some(MeasureSnapshot::Fixed(CustomMeasureSnapshot {
        width: 24.0,
        height,
        baseline: Some(6.0),
    }))
}

fn fixture(count: usize, height: f32) -> Result<(UiTree, Vec<NodeId>), Error> {
    let mut tree = UiTree::new(
        Style {
            flex_direction: taffy::FlexDirection::Column,
            align_items: taffy::AlignItems::START,
            size: taffy::Size {
                width: percent(1.0),
                height: percent(1.0),
            },
            ..Default::default()
        },
        0,
    )?;
    let mut nodes = Vec::with_capacity(count);
    let mut tx = Transaction::default();
    for i in 0..count {
        let node = tree.append(
            tree.root(),
            Style {
                flex_shrink: 0.0,
                ..Default::default()
            },
            0x4488ffff,
        )?;
        tx.measure(node, measure(if i == count / 2 { height } else { 8.0 }));
        nodes.push(node);
    }
    tree.commit(tx, BackendCapabilities::default())?;
    Ok((tree, nodes))
}

fn summary(samples: &[f64]) -> Value {
    let mut sorted = samples.to_vec();
    sorted.sort_by(f64::total_cmp);
    json!({"median_us":sorted[sorted.len()/2],"p95_us":sorted[sorted.len()*95/100],"p99_us":sorted[sorted.len()*99/100],"samples_us":samples})
}

fn main() -> Result<(), Box<dyn StdError>> {
    let output = PathBuf::from(
        std::env::args()
            .nth(1)
            .unwrap_or_else(|| "target/measure-runtime-perf".into()),
    );
    fs::create_dir_all(&output)?;
    let mut results = vec![];
    for count in [300, 3_000, 30_000] {
        let (mut retained, nodes) = fixture(count, 8.0)?;
        retained.frame([800.0, 600.0])?;
        let mut commit_samples = vec![];
        let mut frame_samples = vec![];
        let mut retained_samples = vec![];
        let mut fresh_samples = vec![];
        let mut retained_measures = vec![];
        let mut fresh_measures = vec![];
        let mut retained_leaf_layouts = vec![];
        let mut fresh_leaf_layouts = vec![];
        for round in 0..3 {
            // Rotate execution order to reduce fixed order bias. Every measured
            // retained step changes the source, whichever mode runs first.
            for sample in 0..28 {
                let height = if sample % 2 == 0 { 16.0 } else { 8.0 };
                let mut frames: [Option<FramePacket>; 2] = [None, None];
                for offset in 0..2 {
                    let mode = (round + offset) % 2;
                    let start = Instant::now();
                    let frame = if mode == 0 {
                        let mut tx = Transaction::default();
                        tx.measure(nodes[count / 2], measure(black_box(height)));
                        retained.commit(tx, BackendCapabilities::default())?;
                        let commit_us = start.elapsed().as_secs_f64() * 1e6;
                        let frame_start = Instant::now();
                        let frame = retained.frame([800.0, 600.0])?;
                        let frame_us = frame_start.elapsed().as_secs_f64() * 1e6;
                        if sample >= 8 {
                            commit_samples.push(commit_us);
                            frame_samples.push(frame_us);
                            retained_samples.push(start.elapsed().as_secs_f64() * 1e6);
                            retained_measures.push(frame.layout_stats.measure_calls);
                            retained_leaf_layouts.push(frame.layout_stats.leaf_layouts);
                        }
                        frame
                    } else {
                        let (mut tree, _) = fixture(count, height)?;
                        let frame = tree.frame([800.0, 600.0])?;
                        if sample >= 8 {
                            fresh_samples.push(start.elapsed().as_secs_f64() * 1e6);
                            fresh_measures.push(frame.layout_stats.measure_calls);
                            fresh_leaf_layouts.push(frame.layout_stats.leaf_layouts);
                        }
                        // Fresh reference timing includes construction, initial
                        // layout and extraction. Destruction is excluded.
                        frame
                    };
                    black_box(&frame);
                    frames[mode] = Some(frame);
                }
                let [a, b] = frames.map(Option::unwrap);
                for (a, b) in a.items.iter().zip(b.items.iter()) {
                    assert_eq!(a.bounds, b.bounds);
                    assert_eq!(a.clip, b.clip);
                    assert_eq!(a.commands, b.commands);
                }
                assert_eq!(a.items.len(), b.items.len());
                assert_eq!(a.recorded_chunks, 0);
            }
        }
        let retained_summary = summary(&retained_samples);
        let fresh_summary = summary(&fresh_samples);
        let row = json!({"nodes":count,"retained_commit":summary(&commit_samples),"retained_frame":summary(&frame_samples),
            "retained_total":retained_summary,"fresh_rebuild_total":fresh_summary,
            "retained_measure_calls":retained_measures,"fresh_measure_calls":fresh_measures,
            "retained_leaf_layouts":retained_leaf_layouts,"fresh_leaf_layouts":fresh_leaf_layouts,
            "geometry_equal":true,"retained_recorded_chunks":0});
        println!(
            "{count} nodes: retained {:.3} ms, fresh rebuild {:.3} ms; measure callbacks {} / {}",
            row["retained_total"]["median_us"].as_f64().unwrap() / 1000.0,
            row["fresh_rebuild_total"]["median_us"].as_f64().unwrap() / 1000.0,
            row["retained_measure_calls"][0],
            row["fresh_measure_calls"][0]
        );
        results.push(row);
    }
    fs::write(
        output.join("measure-benchmark.json"),
        serde_json::to_vec_pretty(&json!({
            "scope":"CPU only. One fixed intrinsic size update vs new retained tree construction + initial layout + frame extraction. GPU, window present and text shaping excluded. Fresh tree destruction excluded.",
            "rounds":3,"warmup_updates_per_round":8,"measured_updates_per_round":20,"results":results
        }))?,
    )?;
    Ok(())
}
