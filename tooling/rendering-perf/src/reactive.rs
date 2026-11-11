//! CPU source/sink update benchmark. Frame extraction and GPU work are excluded.
use gpui_box::retained::{
    reactive::{Property, ReactiveUi, Signal},
    *,
};
use serde_json::{Value, json};
use std::{error::Error as StdError, fs, hint::black_box, path::PathBuf, time::Instant};

fn summary(mut samples: Vec<f64>) -> Value {
    samples.sort_by(f64::total_cmp);
    json!({"median_us": samples[samples.len() / 2], "p95_us": samples[samples.len() * 95 / 100], "samples_us": samples})
}

fn fixture(count: usize) -> Result<(UiTree, Vec<NodeId>), Error> {
    let mut tree = UiTree::new(
        Style {
            flex_wrap: taffy::FlexWrap::Wrap,
            ..Default::default()
        },
        0,
    )?;
    let style = Style {
        size: taffy::Size {
            width: length(4.0),
            height: length(4.0),
        },
        flex_shrink: 0.0,
        ..Default::default()
    };
    let nodes = (0..count)
        .map(|_| tree.append(tree.root(), style.clone(), 0x245fc7ff))
        .collect::<Result<_, _>>()?;
    tree.frame([800.0, 600.0])?;
    Ok((tree, nodes))
}

fn main() -> Result<(), Box<dyn StdError>> {
    let output = PathBuf::from(
        std::env::args()
            .nth(1)
            .unwrap_or_else(|| "target/reactive-runtime-perf".into()),
    );
    fs::create_dir_all(&output)?;
    let caps = BackendCapabilities {
        solid_color_patch: true,
    };
    let mut equality = vec![];
    for count in [300, 3_000, 30_000] {
        let (tree, nodes) = fixture(count)?;
        let mut ui = ReactiveUi::new(tree, caps)?;
        let signal = ui.signal(0x245fc7ff);
        for node in &nodes {
            ui.bind(signal, Property::color(*node))?;
        }
        let before = ui.frame([800.0, 600.0])?;
        let mut cases = vec![];
        for restore in [false, true] {
            let stats = ui.stats();
            let mut samples = vec![];
            for _ in 0..101 {
                let start = Instant::now();
                for _ in 0..1000 {
                    let commit = ui.transaction(|tx| {
                        if restore {
                            tx.set(signal, 0xffffffff)?;
                        }
                        tx.set(signal, black_box(0x245fc7ff))?;
                        Ok(())
                    })?;
                    black_box(commit);
                }
                samples.push(start.elapsed().as_secs_f64() * 1e6 / 1000.0);
            }
            let after = ui.stats();
            assert_eq!(after.sink_writes, stats.sink_writes);
            assert_eq!(after.frame_requests, stats.frame_requests);
            cases.push(json!({"case": if restore {"write-restore"} else {"source-equal"}, "cost_per_transaction": summary(samples),
                "transactions": after.transactions - stats.transactions, "sink_dispatches":0, "frame_requests":0}));
        }
        let after = ui.frame([800.0, 600.0])?;
        assert_eq!(before.revision, after.revision);
        assert!(std::sync::Arc::ptr_eq(&before.items, &after.items));
        equality.push(json!({"subscribers":count, "cases":cases, "same_frame_storage":true}));
    }
    let mut batches = vec![];
    for count in [1000, 10_000] {
        let (mut direct, nodes) = fixture(count)?;
        let (tree, targets) = fixture(count)?;
        let mut ui = ReactiveUi::new(tree, caps)?;
        let signals: Vec<Signal<u32>> = targets
            .iter()
            .map(|node| {
                let signal = ui.signal(0x245fc7ff);
                ui.bind(signal, Property::color(*node)).unwrap();
                signal
            })
            .collect();
        let mut samples: [Vec<f64>; 3] = Default::default();
        let mut requests = [0u64; 3];
        let mut alternate = [false; 3];
        for round in 0..3 {
            for offset in 0..3 {
                let mode = (round + offset) % 3;
                for sample in 0..48 {
                    alternate[mode] = !alternate[mode];
                    let color = if alternate[mode] {
                        0x33cc88ff
                    } else {
                        0x245fc7ff
                    };
                    let start = Instant::now();
                    match mode {
                        0 => {
                            for signal in &signals {
                                let commit = ui.transaction(|tx| {
                                    tx.set(*signal, color)?;
                                    Ok(())
                                })?;
                                if sample >= 8 {
                                    requests[mode] += u64::from(commit.commit.request_frame);
                                }
                            }
                        }
                        1 => {
                            let commit = ui.transaction(|tx| {
                                for signal in &signals {
                                    tx.set(*signal, color)?;
                                }
                                Ok(())
                            })?;
                            if sample >= 8 {
                                requests[mode] += u64::from(commit.commit.request_frame);
                            }
                        }
                        _ => {
                            let mut tx = Transaction::default();
                            for node in &nodes {
                                tx.color(*node, color);
                            }
                            let commit = direct.commit(tx, caps)?;
                            if sample >= 8 {
                                requests[mode] += u64::from(commit.request_frame);
                            }
                        }
                    }
                    if sample >= 8 {
                        samples[mode].push(start.elapsed().as_secs_f64() * 1e6);
                    }
                }
            }
        }
        assert_eq!(requests, [count as u64 * 120, 120, 120]);
        let actual = ui.frame([800.0, 600.0])?;
        let reference = direct.frame([800.0, 600.0])?;
        assert_eq!((actual.layout_runs, reference.layout_runs), (0, 0));
        for (a, b) in actual.items.iter().zip(reference.items.iter()) {
            assert_eq!((a.bounds, &a.commands), (b.bounds, &b.commands));
            assert_eq!(a.fill_color, b.fill_color);
        }
        let names = [
            "one-transaction-per-signal",
            "one-transaction-all-signals",
            "typed-batch",
        ];
        batches.push(json!({"nodes":count, "geometry_and_commands_equal":true,
            "measurements": (0..3).map(|mode| json!({"mode":names[mode], "commit_cost":summary(samples[mode].clone()),
                "render_commit_requests":requests[mode], "measured_updates":120})).collect::<Vec<_>>()}));
    }
    let report = json!({"scope":"CPU source writes, sink dispatch, coalescing and tree commit; excludes frame extraction, GPU and presentation; equality costs are 1000-transaction batch averages",
        "equality":equality, "batches":batches});
    fs::write(
        output.join("reactive-benchmark.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    println!("Reactive benchmark passed; output={}", output.display());
    Ok(())
}
