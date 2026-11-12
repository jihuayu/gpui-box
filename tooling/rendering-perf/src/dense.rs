//! Paired publication controls, then absolute-deadline sustained cadence.
use gpui_box::retained::{
    reactive::{Property, ReactiveUi},
    wgpu::WgpuFrameRenderer,
    *,
};
use serde_json::{Value, json};
use std::{
    error::Error,
    future::Future,
    path::PathBuf,
    sync::Arc,
    task::{Context, Poll, Wake, Waker},
    time::{Duration, Instant},
};

#[derive(Clone, Copy, PartialEq)]
struct VisualSample {
    tick: u64,
    scheduled: Instant,
    generated: Instant,
}
struct OwnerWake(std::thread::Thread);
impl Wake for OwnerWake {
    fn wake(self: Arc<Self>) {
        self.0.unpark();
    }
    fn wake_by_ref(self: &Arc<Self>) {
        self.0.unpark();
    }
}
fn wait_ready(
    future: impl Future<Output = Result<bool, gpui_box::retained::Error>>,
) -> Result<bool, gpui_box::retained::Error> {
    let waker = Waker::from(Arc::new(OwnerWake(std::thread::current())));
    let mut context = Context::from_waker(&waker);
    let mut future = std::pin::pin!(future);
    loop {
        if let Poll::Ready(value) = future.as_mut().poll(&mut context) {
            return value;
        }
        std::thread::park();
    }
}

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
fn fixture(count: usize, effects: bool) -> Result<(UiTree, NodeId, Vec<NodeId>), Box<dyn Error>> {
    let mut tree = UiTree::new(absolute(0.0, 0.0, 800.0, 600.0), 0x171d28ff)?;
    let parent = tree.append(tree.root(), absolute(0.0, 0.0, 800.0, 600.0), 0)?;
    let mut nodes = vec![];
    for group in 0..10 {
        let host = tree.append(parent, absolute(0.0, group as f32 * 60.0, 800.0, 60.0), 0)?;
        let mut tx = Transaction::default();
        if effects {
            tx.opacity(host, 0.85);
            tx.shadows(
                host,
                vec![BoxShadow {
                    color: 0x00000088,
                    blur_radius: 8.0,
                    offset: [2.0, 3.0],
                    ..Default::default()
                }],
            );
        }
        tree.commit(
            tx,
            BackendCapabilities {
                solid_color_patch: true,
            },
        )?;
        for i in 0..count / 10 {
            nodes.push(tree.append(
                host,
                absolute((i % 100) as f32 * 8.0, (i / 100) as f32 * 6.0, 4.0, 3.0),
                0x4488ccff,
            )?);
        }
    }
    Ok((tree, parent, nodes))
}
fn update(
    tree: &mut UiTree,
    parent: NodeId,
    nodes: &[NodeId],
    step: usize,
    batch: bool,
) -> Result<(), gpui_box::retained::Error> {
    let mut tx = Transaction::default();
    let shift = if step.is_multiple_of(2) {
        [1.25, 2.5]
    } else {
        [3.25, 4.5]
    };
    if batch {
        for node in nodes {
            tx.translation(*node, shift);
        }
    } else {
        tx.translation(parent, shift);
    }
    tree.commit(
        tx,
        BackendCapabilities {
            solid_color_patch: true,
        },
    )?;
    Ok(())
}
fn distribution(values: impl Iterator<Item = f64>) -> Value {
    let mut values: Vec<_> = values.collect();
    if values.is_empty() {
        return json!({"samples":0});
    }
    values.sort_by(f64::total_cmp);
    let percentile = |p: f64| {
        values[((values.len() as f64 * p).ceil() as usize)
            .saturating_sub(1)
            .min(values.len() - 1)]
    };
    json!({"samples":values.len(), "p50_us":percentile(0.5), "p95_us":percentile(0.95),
        "p99_us":percentile(0.99), "max_us":values.last().unwrap()})
}
// Keep the resolved control's stack-owned packet and allocation cost intact.
#[allow(clippy::large_enum_variant)]
enum OutputFrame {
    Resolved(FramePacket),
    Local(local::LocalFrame),
}
fn output_frame(
    ui: &mut ReactiveUi,
    local: bool,
) -> Result<OutputFrame, gpui_box::retained::Error> {
    Ok(if local {
        OutputFrame::Local(ui.local_frame([800.0, 600.0])?)
    } else {
        OutputFrame::Resolved(ui.frame([800.0, 600.0])?)
    })
}
fn submit_output(
    gpu: &mut WgpuFrameRenderer,
    frame: &OutputFrame,
) -> Result<Option<wgpu::FrameSubmission>, gpui_box::retained::Error> {
    match frame {
        OutputFrame::Local(frame) => gpu.submit_local(frame, [800, 600], 1.0),
        OutputFrame::Resolved(frame) => gpu.submit(frame, [800, 600], 1.0),
    }
}
fn cadence(
    count: usize,
    effects: bool,
    batch: bool,
    hz: u32,
    seconds: f64,
    local: bool,
) -> Result<Value, Box<dyn Error>> {
    let (tree, parent, nodes) = fixture(count, effects)?;
    let mut ui = ReactiveUi::new(
        tree,
        BackendCapabilities {
            solid_color_patch: true,
        },
    )?;
    let empty = Instant::now();
    let signal = ui.signal(VisualSample {
        tick: 0,
        scheduled: empty,
        generated: empty,
    });
    let placement = ui.computed(&[signal.dependency()], move |inputs| {
        let sample = inputs.read(signal)?;
        let step = (sample.tick % 31) as f32 * 0.125;
        Ok([1.25 + step, 2.5 + step])
    })?;
    for node in if batch { nodes } else { vec![parent] } {
        ui.bind(placement, Property::translation(node))?;
    }
    let mut receiver = ui.open_updates(1)?;
    let sender = ui.update_sender(&receiver, signal)?;
    let mut gpu = WgpuFrameRenderer::new(2)?;
    let adapter = format!("{:?}", gpu.adapter_info());
    submit_output(&mut gpu, &output_frame(&mut ui, local)?)?;
    gpu.finish()?;
    let queue = frame_queue::FrameQueue::new();
    let local_queue = frame_queue::LocalFrameQueue::new();
    let rate = pacing::FrameRate::hz(hz).unwrap();
    let period = rate.budget();
    let start = Instant::now() + Duration::from_millis(20);
    let slots = (seconds * f64::from(hz)) as usize;
    // Independent OS producer includes scheduling and ingress contention, and
    // continues producing while the owner publishes/submits/waits for the GPU.
    let producer = std::thread::spawn(move || -> Result<u64, gpui_box::retained::Error> {
        let mut clock = pacing::FrameClock::new(start, rate);
        let mut skipped = 0;
        loop {
            if let Some(wait) = clock
                .next_sample_at()
                .unwrap()
                .checked_duration_since(Instant::now())
            {
                std::thread::sleep(wait);
            }
            let generated = Instant::now();
            let tick = clock.sample(generated).unwrap();
            let chosen = tick.index.min(slots as u64 - 1);
            skipped += tick.superseded_samples.min(chosen);
            let scheduled = start + Duration::from_secs_f64(chosen as f64 / f64::from(hz));
            sender.send(VisualSample {
                tick: chosen,
                scheduled,
                generated,
            })?;
            if chosen + 1 >= slots as u64 {
                break;
            }
        }
        Ok(skipped)
    });
    let mut next = 0;
    let mut rows = vec![];
    let mut skipped = 0;
    let mut missed = 0;
    let mut consecutive = 0;
    let mut longest = 0;
    while next < slots {
        assert!(wait_ready(receiver.ready())?);
        let entered = Instant::now();
        let applied = ui.apply_updates(&mut receiver)?;
        let committed = Instant::now();
        assert_eq!(applied.applied, 1);
        let sample = *ui.read(signal)?;
        let chosen = sample.tick as usize;
        let superseded = chosen - next;
        skipped += superseded;
        consecutive += superseded;
        longest = longest.max(consecutive);
        let frame = output_frame(&mut ui, local)?;
        // Skipping an even number of alternating samples can produce equal
        // state. The mailbox correctly suppresses that revision; retain the
        // last complete snapshot instead of assuming every tick changes it.
        let frame = match frame {
            OutputFrame::Resolved(frame) => {
                queue.publish(frame.clone())?;
                OutputFrame::Resolved(queue.take_latest()?.unwrap_or(frame))
            }
            OutputFrame::Local(frame) => {
                local_queue.publish(frame.clone())?;
                OutputFrame::Local(local_queue.take_latest()?.unwrap_or(frame))
            }
        };
        let published = Instant::now();
        let submission = submit_output(&mut gpu, &frame)?;
        let submitted = Instant::now();
        // This is a conservative synchronous completion scope. Native present
        // has a separate receipt; this value must never be called photon latency.
        gpu.finish()?;
        let finished = Instant::now();
        let intended = sample.scheduled;
        let late = finished > intended + period;
        if late {
            missed += 1;
            consecutive += 1;
            longest = longest.max(consecutive);
        } else {
            consecutive = 0;
        }
        rows.push(
            json!({"tick":chosen,"source_us":(intended-start).as_secs_f64()*1e6,
            "producer_delay_us":(sample.generated-intended).as_secs_f64()*1e6,
            "source_to_owner_us":entered.saturating_duration_since(intended).as_secs_f64()*1e6,
            "ingress_to_gpu_complete_us":(finished-sample.generated).as_secs_f64()*1e6,
            "commit_us":(committed-entered).as_secs_f64()*1e6,
            "publication_queue_us":(published-committed).as_secs_f64()*1e6,
            "submit_us":(submitted-published).as_secs_f64()*1e6,
            "source_to_gpu_complete_us":(finished-intended).as_secs_f64()*1e6,
            "deadline_missed":late,"gpu_submitted":submission.is_some(),"superseded_ticks":superseded}),
        );
        next = chosen + 1;
    }
    let producer_skipped = producer.join().expect("visual producer panicked")?;
    assert_eq!(rows.len() + skipped, slots);
    let frame = ui.frame([800.0, 600.0])?;
    assert_eq!(
        gpu.image()?.as_raw(),
        gpu.reference_image(&frame, [800, 600], 1.0)?.as_raw()
    );
    let phases: serde_json::Map<String, Value> = [
        "producer_delay_us",
        "source_to_owner_us",
        "commit_us",
        "publication_queue_us",
        "submit_us",
        "ingress_to_gpu_complete_us",
    ]
    .into_iter()
    .map(|phase| {
        (
            phase.to_string(),
            distribution(rows.iter().map(|r| r[phase].as_f64().unwrap())),
        )
    })
    .collect();
    Ok(
        json!({"nodes":count,"effects":effects,"batch":batch,"local_snapshot":local,"target_hz":hz,"duration_seconds":seconds,
        "adapter":adapter,"expected_ticks":slots,"consumed_ticks":rows.len(),"superseded_ticks":skipped,
        "gpu_submissions":rows.iter().filter(|r| r["gpu_submitted"]==true).count(),
        "producer_superseded_ticks":producer_skipped,
        "update_queue":format!("{:?}",receiver.stats()?),
        "unsatisfied_ticks":skipped+missed,
        "deadline_misses":missed,"longest_consecutive_misses":longest,
        "budget_us":period.as_secs_f64()*1e6,"pixel_equal":true,
        "phases":phases,
        "source_to_gpu_complete":distribution(rows.iter().map(|r|r["source_to_gpu_complete_us"].as_f64().unwrap())),
        "raw":rows}),
    )
}
fn main() -> Result<(), Box<dyn Error>> {
    let output = PathBuf::from(
        std::env::args()
            .nth(1)
            .unwrap_or_else(|| "target/dense-perf".into()),
    );
    std::fs::create_dir_all(&output)?;
    let quick = std::env::args().any(|v| v == "--quick");
    let cadence_only = std::env::args().any(|v| v == "--cadence-only");
    let paired_only = std::env::args().any(|v| v == "--paired-only");
    let local = std::env::args().any(|v| v == "--local");
    let count = 10000;
    let mut cases = vec![];
    for effects in if cadence_only {
        vec![]
    } else {
        vec![false, true]
    } {
        for batch in [false, true] {
            let mut controls = vec![];
            for reextract in [false, true] {
                let (mut tree, parent, nodes) = fixture(count, effects)?;
                if reextract {
                    tree.set_frame_build_policy(FrameBuildPolicy::SelectiveReextract);
                }
                let mut gpu = WgpuFrameRenderer::new(3)?;
                let frame = tree.frame([800.0, 600.0])?;
                gpu.submit(&frame, [800, 600], 1.0)?;
                gpu.finish()?;
                controls.push((tree, parent, nodes, gpu));
            }
            let mut samples: [Vec<[f64; 4]>; 2] = Default::default();
            let mut work = vec![Value::Null; 2];
            for round in 0..3 {
                for offset in 0..2 {
                    let mode = (round + offset) % 2;
                    let (tree, parent, nodes, gpu) = &mut controls[mode];
                    for step in 0..if quick { 12 } else { 46 } {
                        let start = Instant::now();
                        update(tree, *parent, nodes, step, batch)?;
                        let committed = Instant::now();
                        let local_frame = if local && mode == 0 {
                            Some(tree.local_frame([800.0, 600.0])?)
                        } else {
                            None
                        };
                        let frame = if local_frame.is_none() {
                            Some(tree.frame([800.0, 600.0])?)
                        } else {
                            None
                        };
                        let published = Instant::now();
                        let submit = if let Some(frame) = &local_frame {
                            gpu.submit_local(frame, [800, 600], 1.0)?
                        } else {
                            gpu.submit(frame.as_ref().unwrap(), [800, 600], 1.0)?
                        }
                        .unwrap();
                        let submitted = Instant::now();
                        gpu.finish()?;
                        let complete = Instant::now();
                        if step >= 6 {
                            samples[mode].push([
                                (committed - start).as_secs_f64() * 1e6,
                                (published - committed).as_secs_f64() * 1e6,
                                (submitted - published).as_secs_f64() * 1e6,
                                (complete - start).as_secs_f64() * 1e6,
                            ]);
                            work[mode] = json!({"extracted":frame.as_ref().map_or(0,|f|f.build_stats.extracted_nodes),
                                "local_changed":local_frame.as_ref().map(|f|f.stats().changed_nodes),
                                "quad_bytes":submit.gpu.uploaded_bytes,"spatial_bytes":submit.gpu.spatial_upload_bytes,
                                "draws":submit.gpu.draw_calls,"layers_rasterized":submit.gpu.rasterized_layers,
                                "layers_reused":submit.gpu.reused_layers});
                        }
                    }
                    let frame = tree.frame([800.0, 600.0])?;
                    assert_eq!(
                        gpu.image()?.as_raw(),
                        gpu.reference_image(&frame, [800, 600], 1.0)?.as_raw()
                    );
                }
            }
            let modes: Vec<_> = (0..2)
                .map(|mode| {
                    json!({"reextract":mode==1,"work":work[mode],
                "commit":distribution(samples[mode].iter().map(|s|s[0])),
                "publication":distribution(samples[mode].iter().map(|s|s[1])),
                "submission":distribution(samples[mode].iter().map(|s|s[2])),
                "completed":distribution(samples[mode].iter().map(|s|s[3])),"raw_us":samples[mode]})
                })
                .collect();
            let case = json!({"effects":effects,"batch":batch,"nodes":count,"modes":modes,"pixel_equal":true});
            println!(
                "effects={effects} batch={batch}: {}",
                case["modes"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|m| format!("{}", m["completed"]))
                    .collect::<Vec<_>>()
                    .join(" / ")
            );
            cases.push(case);
        }
    }
    std::fs::write(
        output.join("paired.json"),
        serde_json::to_vec_pretty(&json!({"cases":cases,
        "scope":"owner transaction/publication/submission/GPU completion; same runtime/shaders, complete reextraction control; local mode uses full-viewport isolated layers while resolved control uses tight bounds; native presentation excluded",
        "quick":quick,"local_snapshot":local}))?,
    )?;
    let mut sustained = vec![];
    for (nodes, batch) in if paired_only {
        vec![]
    } else {
        vec![(1000, false), (10000, false), (10000, true)]
    } {
        for effects in [false, true] {
            for hz in [60, 120] {
                let result = cadence(
                    nodes,
                    effects,
                    batch,
                    hz,
                    if quick { 1.0 } else { 10.0 },
                    local,
                )?;
                println!(
                    "cadence nodes={nodes} effects={effects} batch={batch} hz={hz}: {}, skipped={}, misses={}",
                    result["source_to_gpu_complete"],
                    result["superseded_ticks"],
                    result["deadline_misses"]
                );
                sustained.push(result);
            }
        }
    }
    std::fs::write(
        output.join("cadence.json"),
        serde_json::to_vec_pretty(&json!({
        "scope":"absolute-clock OS worker -> bounded UpdateSender -> owner executor wake -> computed/typed binding/transaction -> local or resolved frame queue -> GPU submit/completion; local mode uses full-viewport isolated layers; no native present or physical input; source and owner skips reported; sleep scheduler delay included",
        "quick":quick,"local_snapshot":local,"max_in_flight":2,"synchronous_completion_per_frame":true,"cases":sustained}))?,
    )?;
    Ok(())
}
