//! CPU enqueue/commit comparison. OS thread scheduling is measured separately.
use gpui_box::retained::{
    reactive::{Property, ReactiveUi, Signal, UpdateReceiver, UpdateSender},
    *,
};
use serde_json::{Value, json};
use std::{error::Error as StdError, fs, path::PathBuf, time::Instant};

struct Fixture {
    ui: ReactiveUi,
    inputs: Vec<Signal<u32>>,
    receiver: UpdateReceiver,
    senders: Vec<UpdateSender<u32>>,
}
impl Fixture {
    fn new(sources: usize) -> Result<Self, Error> {
        let mut ui = ReactiveUi::new(
            UiTree::new(Style::default(), 0)?,
            BackendCapabilities {
                solid_color_patch: true,
            },
        )?;
        let mut inputs = vec![];
        for _ in 0..sources {
            let input = ui.signal(0u32);
            let color = ui.computed(&[input.dependency()], move |i| {
                Ok(*i.read(input)? << 8 | 255)
            })?;
            let node = ui.append(
                ui.root(),
                Style {
                    size: taffy::Size {
                        width: length(4.0),
                        height: length(4.0),
                    },
                    flex_shrink: 0.0,
                    ..Default::default()
                },
                0,
            )?;
            ui.bind(color, Property::color(node))?;
            inputs.push(input);
        }
        ui.frame([800.0, 600.0])?;
        let receiver = ui.open_updates(sources)?;
        let senders = inputs
            .iter()
            .map(|input| ui.update_sender(&receiver, *input))
            .collect::<Result<_, _>>()?;
        Ok(Self {
            ui,
            inputs,
            receiver,
            senders,
        })
    }
    fn update(&mut self, mode: usize, messages: usize, high: bool) -> Result<(), Error> {
        let initial = if high { 100_000 } else { 0 };
        match mode {
            0 => {
                for message in 0..messages {
                    self.ui.transaction(|tx| {
                        tx.set(
                            self.inputs[message % self.inputs.len()],
                            initial + message as u32,
                        )?;
                        Ok(())
                    })?;
                }
            }
            1 => {
                self.ui.transaction(|tx| {
                    for message in 0..messages {
                        tx.set(
                            self.inputs[message % self.inputs.len()],
                            initial + message as u32,
                        )?;
                    }
                    Ok(())
                })?;
            }
            _ => {
                for message in 0..messages {
                    self.senders[message % self.senders.len()].send(initial + message as u32)?;
                }
                let batch = self.ui.apply_updates(&mut self.receiver)?;
                assert_eq!(batch.applied, self.inputs.len());
            }
        }
        Ok(())
    }
}
fn summary(mut samples: Vec<f64>) -> Value {
    samples.sort_by(f64::total_cmp);
    json!({"median_us":samples[samples.len()/2],"p95_us":samples[samples.len()*95/100],"samples_us":samples})
}
fn main() -> Result<(), Box<dyn StdError>> {
    let output = PathBuf::from(
        std::env::args()
            .nth(1)
            .unwrap_or_else(|| "target/delivery-runtime-perf".into()),
    );
    fs::create_dir_all(&output)?;
    let mut results = vec![];
    for (sources, messages) in [(1, 1000), (1000, 1000), (1000, 10000)] {
        let mut fixtures = [
            Fixture::new(sources)?,
            Fixture::new(sources)?,
            Fixture::new(sources)?,
        ];
        let mut samples: [Vec<f64>; 3] = Default::default();
        let mut rounds = vec![];
        let mut counts = vec![];
        for round in 0..3 {
            for offset in 0..3 {
                let mode = (round + offset) % 3;
                let fixture = &mut fixtures[mode];
                let before = fixture.ui.stats();
                let mut round_samples = vec![];
                for sample in 0..48 {
                    let start = Instant::now();
                    fixture.update(mode, messages, sample % 2 == 0)?;
                    if sample >= 8 {
                        let elapsed = start.elapsed().as_secs_f64() * 1e6;
                        samples[mode].push(elapsed);
                        round_samples.push(elapsed);
                    }
                }
                let after = fixture.ui.stats();
                assert_eq!(
                    after.transactions - before.transactions,
                    if mode == 0 { 48 * messages as u64 } else { 48 }
                );
                assert_eq!(
                    after.computed_evaluations - before.computed_evaluations,
                    if mode == 0 {
                        48 * messages as u64
                    } else {
                        48 * sources as u64
                    }
                );
                counts.push(json!({"round":round,"mode":mode,"transactions":after.transactions-before.transactions,"computed_evaluations":after.computed_evaluations-before.computed_evaluations,"frame_intents":after.frame_requests-before.frame_requests,"includes_warmup":true}));
                rounds.push(json!({"round":round,"mode":mode,"timing":summary(round_samples)}));
            }
            let frames: Vec<_> = fixtures
                .iter_mut()
                .map(|fixture| fixture.ui.frame([800.0, 600.0]).unwrap())
                .collect();
            for other in &frames[1..] {
                for (a, b) in frames[0].items.iter().zip(other.items.iter()) {
                    assert_eq!(
                        (a.bounds, a.fill_color, &a.commands),
                        (b.bounds, b.fill_color, &b.commands)
                    );
                }
            }
        }
        results.push(json!({"sources":sources,"messages_per_batch":messages,"per_message":summary(samples[0].clone()),"explicit_transaction":summary(samples[1].clone()),"inbox":summary(samples[2].clone()),"rounds":rounds,"counts":counts,"final_geometry_material_commands_equal":true}));
    }
    fs::write(
        output.join("delivery-benchmark.json"),
        serde_json::to_vec_pretty(
            &json!({"scope":"CPU source delivery plus computed/sink/tree commits. Inbox includes every mutex enqueue and one owner drain, on the same benchmark thread; excludes OS scheduling, extraction, GPU and presentation. Three rotating rounds, eight warmups plus forty measured batches per mode/round. Per-message mode commits every input, explicit transaction stages the same messages, inbox coalesces them before one transaction.","results":results}),
        )?,
    )?;
    println!("Delivery benchmark passed; output={}", output.display());
    Ok(())
}
