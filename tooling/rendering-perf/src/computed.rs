//! Paired CPU transaction comparison: dependency routing versus eager derived
//! recomputation, with identical typed sinks and persistent tree commits.
use gpui_box::retained::{
    reactive::{Property, ReactiveUi, Signal},
    *,
};
use serde_json::{Value, json};
use std::{error::Error as StdError, fs, hint::black_box, path::PathBuf, time::Instant};

struct Fixture {
    ui: ReactiveUi,
    inputs: Vec<Signal<u32>>,
    buckets: Vec<Signal<u32>>,
    colors: Vec<Signal<u32>>,
    graph: bool,
}
fn color(bucket: u32, lane: u32) -> u32 {
    let channel = (bucket * 37 + lane * 17) % 256;
    channel << 24 | (255 - channel) << 16 | (lane * 19) << 8 | 255
}
impl Fixture {
    fn new(count: usize, graph: bool) -> Result<Self, Error> {
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
            .map(|_| tree.append(tree.root(), style.clone(), 0))
            .collect::<Result<Vec<_>, _>>()?;
        let mut ui = ReactiveUi::new(
            tree,
            BackendCapabilities {
                solid_color_patch: true,
            },
        )?;
        let mut inputs = vec![];
        let mut buckets = vec![];
        let mut colors = vec![];
        for group in 0..count / 10 {
            let input = ui.signal(0u32);
            let bucket = if graph {
                ui.computed(&[input.dependency()], move |i| Ok(*i.read(input)? / 4))?
            } else {
                ui.signal(0u32)
            };
            inputs.push(input);
            buckets.push(bucket);
            for lane in 0..10 {
                let signal = if graph {
                    ui.computed(&[bucket.dependency()], move |i| {
                        Ok(color(*i.read(bucket)?, lane))
                    })?
                } else {
                    ui.signal(color(0, lane))
                };
                ui.bind(signal, Property::color(nodes[group * 10 + lane as usize]))?;
                colors.push(signal);
            }
        }
        ui.frame([800.0, 600.0])?;
        Ok(Self {
            ui,
            inputs,
            buckets,
            colors,
            graph,
        })
    }
    fn update(&mut self, all: bool, value: u32) -> Result<(u64, usize), Error> {
        let mut eager_evaluations = 0;
        let commit = self.ui.transaction(|tx| {
            let mut changed = false;
            for input in self
                .inputs
                .iter()
                .take(if all { self.inputs.len() } else { 1 })
            {
                changed |= tx.set(*input, black_box(value))?;
            }
            if !self.graph && changed {
                for (group, (input, bucket)) in self.inputs.iter().zip(&self.buckets).enumerate() {
                    let next = *tx.read(*input)? / 4;
                    tx.set(*bucket, next)?;
                    eager_evaluations += 1;
                    for lane in 0..10 {
                        tx.set(self.colors[group * 10 + lane as usize], color(next, lane))?;
                        eager_evaluations += 1;
                    }
                }
            }
            Ok(())
        })?;
        Ok((
            if self.graph {
                commit.computed_evaluations
            } else {
                eager_evaluations
            },
            commit.sink_writes,
        ))
    }
}
fn summary(mut samples: Vec<f64>) -> Value {
    samples.sort_by(f64::total_cmp);
    json!({"median_us":samples[samples.len()/2], "p95_us":samples[samples.len()*95/100], "samples_us":samples})
}
fn main() -> Result<(), Box<dyn StdError>> {
    let output = PathBuf::from(
        std::env::args()
            .nth(1)
            .unwrap_or_else(|| "target/computed-runtime-perf".into()),
    );
    fs::create_dir_all(&output)?;
    let mut results = vec![];
    for count in [1_000, 10_000] {
        for workload in ["source-equal", "bucket-equal", "one-group", "all-groups"] {
            // Batch short events so timer quantization is not presented as a
            // sub-microsecond single-event measurement.
            let events_per_sample = match workload {
                "source-equal" => 1000,
                "bucket-equal" => 100,
                "one-group" => 10,
                _ => 1,
            };
            let mut fixtures = [Fixture::new(count, true)?, Fixture::new(count, false)?];
            let mut samples: [Vec<f64>; 2] = Default::default();
            let mut counts: [Vec<(u64, usize)>; 2] = Default::default();
            let mut rounds = vec![];
            for round in 0..3 {
                for offset in 0..2 {
                    let mode = (round + offset) % 2;
                    let fixture = &mut fixtures[mode];
                    let mut round_samples = vec![];
                    for sample in 0..72 {
                        let all = workload == "all-groups";
                        let start = Instant::now();
                        let mut counters = (0, 0);
                        for event in 0..events_per_sample {
                            let sequence = sample * events_per_sample + event;
                            let value = match workload {
                                "source-equal" => 0,
                                "bucket-equal" => sequence % 2,
                                _ => (sequence % 2) * 4,
                            };
                            let event_counts = fixture.update(all, value)?;
                            counters.0 += event_counts.0;
                            counters.1 += event_counts.1;
                        }
                        let elapsed =
                            start.elapsed().as_secs_f64() * 1e6 / events_per_sample as f64;
                        if sample >= 12 {
                            round_samples.push(elapsed);
                            samples[mode].push(elapsed);
                            counts[mode].push(counters);
                        }
                    }
                    rounds.push(json!({"round":round,"mode":mode,"timing":summary(round_samples)}));
                }
                let a = fixtures[0].ui.frame([800.0, 600.0])?;
                let b = fixtures[1].ui.frame([800.0, 600.0])?;
                assert_eq!((a.layout_runs, b.layout_runs), (0, 0));
                for (a, b) in a.items.iter().zip(b.items.iter()) {
                    assert_eq!(
                        (a.bounds, a.fill_color, &a.commands),
                        (b.bounds, b.fill_color, &b.commands)
                    );
                }
            }
            let graph_evaluations = match workload {
                "source-equal" => 0,
                "bucket-equal" => 1,
                "one-group" => 11,
                _ => count as u64 / 10 * 11,
            };
            let eager_evaluations = if workload == "source-equal" {
                0
            } else {
                count as u64 / 10 * 11
            };
            let sinks = match workload {
                "one-group" => 10,
                "all-groups" => count,
                _ => 0,
            };
            for (mode, expected) in [graph_evaluations, eager_evaluations].iter().enumerate() {
                assert!(counts[mode].iter().all(|actual| *actual
                    == (
                        *expected * events_per_sample as u64,
                        sinks * events_per_sample as usize
                    )));
            }
            results.push(json!({"render_nodes":count,"workload":workload,"graph":summary(samples[0].clone()),"eager":summary(samples[1].clone()),
                "events_per_sample":events_per_sample,"graph_evaluations_per_event":graph_evaluations,"eager_evaluations_per_event":eager_evaluations,"sinks_per_event":sinks,"rounds":rounds,"final_geometry_material_commands_equal":true}));
        }
    }
    fs::write(
        output.join("computed-benchmark.json"),
        serde_json::to_vec_pretty(
            &json!({"scope":"CPU transaction only: typed source writes, derived values, equality, sinks and tree commit. Excludes frame extraction, layout, GPU, notify and presentation. 3 rotating rounds, 12 warmup samples and 60 measured samples per mode/round. Samples average the recorded events_per_sample. Eager mode skips source-equal events but recomputes every derived value after a changed input.","results":results}),
        )?,
    )?;
    println!("Computed benchmark passed; output={}", output.display());
    Ok(())
}
