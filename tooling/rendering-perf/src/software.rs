//! CPU-only cooperative consumer experiment. Synthetic probe arrival is not an
//! OS input/visible-present measurement. Prepared packets exclude UI owner work.
use gpui_box::retained::{pacing::SoftwareEffects, software::*, *};
use serde_json::{Value, json};
use std::{
    error::Error,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

fn absolute(x: f32, y: f32, w: f32, h: f32) -> Style {
    Style {
        position: taffy::Position::Absolute,
        inset: taffy::Rect {
            left: length(x),
            top: length(y),
            right: taffy::LengthPercentageAuto::auto(),
            bottom: taffy::LengthPercentageAuto::auto(),
        },
        size: taffy::Size {
            width: length(w),
            height: length(h),
        },
        ..Default::default()
    }
}
fn packets(effects: bool) -> Result<Vec<FramePacket>, Box<dyn Error>> {
    let mut tree = UiTree::new(absolute(0.0, 0.0, 320.0, 180.0), 0x182030ff)?;
    let mut nodes = vec![];
    for i in 0..100 {
        nodes.push(tree.append(
            tree.root(),
            absolute((i % 10) as f32 * 31.0, (i / 10) as f32 * 17.0, 25.0, 15.0),
            0x4080c0ff,
        )?);
    }
    if effects {
        let mut tx = Transaction::default();
        for id in nodes.iter().step_by(10) {
            tx.opacity(*id, 0.7);
            tx.decoration(
                *id,
                QuadDecoration {
                    radius: 3.0,
                    border_width: 1.0,
                    border_color: 0xffffffff,
                },
            );
        }
        tree.commit(tx, BackendCapabilities::default())?;
    }
    let mut frames = vec![];
    for n in 0..8 {
        let mut tx = Transaction::default();
        for id in &nodes {
            tx.translation(*id, [(n % 4) as f32, 0.0]);
        }
        tree.commit(tx, BackendCapabilities::default())?;
        frames.push(tree.frame([320.0, 180.0])?);
    }
    Ok(frames)
}
struct Sample {
    tick: u64,
    at: Instant,
    frame: FramePacket,
}
#[derive(Default)]
struct Inbox {
    pending: Option<Sample>,
    probe: Option<(Instant, u64)>,
    produced: u64,
    coalesced: u64,
    probe_coalesced: u64,
    done: bool,
}
fn micros(d: Duration) -> f64 {
    d.as_secs_f64() * 1e6
}
fn dist(values: &[f64]) -> Value {
    if values.is_empty() {
        return json!({"count":0});
    }
    let mut v = values.to_vec();
    v.sort_by(f64::total_cmp);
    let p = |q: f64| v[((v.len() - 1) as f64 * q).ceil() as usize];
    json!({"count":v.len(),"p50_us":p(0.5),"p95_us":p(0.95),"p99_us":p(0.99),"max_us":v[v.len()-1]})
}
fn run(
    frames: &[FramePacket],
    hz: u32,
    quota: usize,
    seconds: f64,
) -> Result<Value, Box<dyn Error>> {
    let inbox = Arc::new(Mutex::new(Inbox::default()));
    let source = inbox.clone();
    let frames = frames.to_vec();
    let origin = Instant::now() + Duration::from_millis(20);
    let end = origin + Duration::from_secs_f64(seconds);
    let producer = std::thread::spawn(move || {
        let ticks = (seconds * hz as f64) as u64;
        for tick in 0..ticks {
            let at = origin + Duration::from_secs_f64(tick as f64 / hz as f64);
            if let Some(wait) = at.checked_duration_since(Instant::now()) {
                std::thread::sleep(wait);
            }
            let mut state = source.lock().unwrap();
            state.coalesced += u64::from(state.pending.is_some());
            state.probe_coalesced += u64::from(state.probe.is_some());
            // Keep oldest unhandled probe timestamp, so overload cannot erase
            // its tail latency while subsequent input arrivals are counted.
            if state.probe.is_none() {
                state.probe = Some((at, 1));
            } else {
                state.probe.as_mut().unwrap().1 += 1;
            }
            state.pending = Some(Sample {
                tick,
                at,
                frame: frames[tick as usize % frames.len()].clone(),
            });
            state.produced += 1;
        }
        source.lock().unwrap().done = true;
    });
    let options = SoftwareOptions {
        size: [320, 180],
        scale: 1.0,
        background: 0,
        effects: SoftwareEffects::Exact,
        max_bytes: 32 * 1024 * 1024,
    };
    let mut active: Option<(Sample, SoftwareJob)> = None;
    let mut shown: Option<SoftwareFrame> = None;
    let mut slices = vec![];
    let mut prepare = vec![];
    let mut retire = vec![];
    let mut probes = vec![];
    let mut finished = vec![];
    let mut raw = vec![];
    let mut ontime = 0;
    let mut satisfied = std::collections::BTreeSet::new();
    let mut probe_arrivals = 0;
    loop {
        {
            let mut state = inbox.lock().unwrap();
            if let Some((at, count)) = state.probe.take() {
                probes.push(micros(Instant::now().saturating_duration_since(at)));
                probe_arrivals += count;
            }
            if active.is_none() {
                if let Some(sample) = state.pending.take() {
                    drop(state);
                    let start = Instant::now();
                    let job = SoftwareJob::new(sample.frame.clone(), options)
                        .map_err(|e| format!("init {e:?}"))?;
                    prepare.push(micros(start.elapsed()));
                    active = Some((sample, job));
                } else if state.done {
                    break;
                }
            }
        }
        if let Some((sample, job)) = active.as_mut() {
            let start = Instant::now();
            let complete = job.step(quota).map_err(|e| format!("step {e:?}"))?;
            slices.push(micros(start.elapsed()));
            if complete {
                let at = Instant::now();
                let latency = micros(at.saturating_duration_since(sample.at));
                let deadline = sample.at + Duration::from_secs_f64(1.0 / hz as f64);
                let met = at <= deadline;
                if met {
                    ontime += 1;
                    satisfied.insert(sample.tick);
                }
                finished.push(latency);
                raw.push(json!({"tick":sample.tick,"completion_us":latency,"deadline_met":met,"work_units":job.stats().work_units}));
                let (_, job) = active.take().unwrap();
                let frame = job.into_frame().map_err(|e| format!("finish {e:?}"))?;
                let start = Instant::now();
                drop(shown.replace(frame));
                retire.push(micros(start.elapsed()));
            }
        } else {
            std::thread::sleep(Duration::from_micros(100));
        }
        // Finish active and latest pending after source ends; these drain times
        // remain in the raw result, and deadline accounting does not stop early.
    }
    producer.join().map_err(|_| "source panic")?;
    let state = inbox.lock().unwrap();
    let mut longest = 0;
    let mut burst = 0;
    for tick in 0..state.produced {
        if satisfied.contains(&tick) {
            burst = 0;
        } else {
            burst += 1;
            longest = longest.max(burst);
        }
    }
    Ok(
        json!({"hz":hz,"quota":quota,"duration_s":seconds,"elapsed_with_drain_s":Instant::now().saturating_duration_since(origin).as_secs_f64(),
        "source_ticks":state.produced,"coalesced_samples":state.coalesced,"consumed":finished.len(),"deadline_met":ontime,
        "unsatisfied_ticks":state.produced-ontime,"longest_unsatisfied_burst":longest,"probe_arrivals_handled":probe_arrivals,
        "probe_coalesced":state.probe_coalesced,"synthetic_probe_oldest_latency":dist(&probes),"slice":dist(&slices),"worker_init":dist(&prepare),"worker_retire":dist(&retire),
        "source_to_cpu_completion":dist(&finished),"completion_samples":raw,"slice_us":slices,"probe_oldest_us":probes,
        "source_stopped_at_s":end.saturating_duration_since(origin).as_secs_f64(),"native_present_measured":false,"owner_construction_measured":false}),
    )
}
fn main() -> Result<(), Box<dyn Error>> {
    let args = std::env::args().collect::<Vec<_>>();
    let output = args
        .get(1)
        .map(String::as_str)
        .unwrap_or("target/software-cadence");
    let seconds = args.get(2).map(|s| s.parse()).transpose()?.unwrap_or(5.0);
    let mut results = vec![];
    for effects in [false, true] {
        let frames = packets(effects)?;
        for hz in [60, 120] {
            // Reverse the paired order at 120 Hz to expose order bias.
            let quotas = if hz == 60 {
                [usize::MAX, 512]
            } else {
                [512, usize::MAX]
            };
            for quota in quotas {
                let mut result = run(&frames, hz, quota, seconds)?;
                result["effects"] = json!(effects);
                println!(
                    "effects={effects} hz={hz} quota={quota} completed={} probe_p99={}us",
                    result["consumed"], result["synthetic_probe_oldest_latency"]["p99_us"]
                );
                results.push(result);
            }
        }
    }
    std::fs::create_dir_all(output)?;
    std::fs::write(
        std::path::Path::new(output).join("cadence.json"),
        serde_json::to_vec_pretty(&results)?,
    )?;
    Ok(())
}
