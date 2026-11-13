//! Same pipelined workload with admission before/after replaceable owner work.
use super::*;
use gpui_box::retained::wgpu::{CompletionReceipt, CompletionReceiver};
use std::collections::VecDeque;

struct PendingFrame {
    serial: u64,
    sample: VisualSample,
    entered: Instant,
    committed: Instant,
    published: Instant,
    submitted: Instant,
    admission_wait: Duration,
    superseded: usize,
    gpu_submitted: bool,
    burst_epoch: u64,
    burst_epochs_crossed: u64,
    diagnostics: Value,
}
struct Observations {
    pending: VecDeque<PendingFrame>,
    rows: Vec<Value>,
    latest: Option<CompletionReceipt>,
    origin: Instant,
    budget: Duration,
}
impl Observations {
    fn complete(&mut self, receipt: CompletionReceipt) {
        if self.latest.is_none_or(|old| receipt.serial >= old.serial) {
            self.latest = Some(receipt);
        }
        let observed = Instant::now();
        while self
            .pending
            .front()
            .is_some_and(|f| f.serial <= receipt.serial)
        {
            let f = self.pending.pop_front().unwrap();
            // Equal-state samples may use already completed output. Their
            // work completes at owner submission/check, never before arrival.
            let finished = receipt.observed_at.max(f.submitted);
            self.rows.push(json!({
                "tick":f.sample.tick,"serial":f.serial,"covering_serial":receipt.serial,"diagnostics":f.diagnostics,
                "burst_epoch":f.burst_epoch,"burst_epochs_crossed":f.burst_epochs_crossed,
                "source_us":f.sample.scheduled.duration_since(self.origin).as_secs_f64()*1e6,
                "producer_delay_us":f.sample.generated.duration_since(f.sample.scheduled).as_secs_f64()*1e6,
                "source_to_owner_us":f.entered.saturating_duration_since(f.sample.scheduled).as_secs_f64()*1e6,
                "commit_us":f.committed.duration_since(f.entered).as_secs_f64()*1e6,
                "publication_us":f.published.duration_since(f.committed).as_secs_f64()*1e6,
                "submit_and_admission_us":f.submitted.duration_since(f.published).as_secs_f64()*1e6,
                "admission_wait_us":f.admission_wait.as_secs_f64()*1e6,
                "source_to_completion_upper_bound_us":finished.duration_since(f.sample.scheduled).as_secs_f64()*1e6,
                "completion_to_owner_observation_us":observed.saturating_duration_since(finished).as_secs_f64()*1e6,
                "source_to_owner_completion_observation_us":observed.max(f.submitted).duration_since(f.sample.scheduled).as_secs_f64()*1e6,
                "deadline_missed":finished > f.sample.scheduled+self.budget,
                "superseded_ticks":f.superseded,"gpu_submitted":f.gpu_submitted
            }));
        }
    }
    fn push(&mut self, frame: PendingFrame) {
        self.pending.push_back(frame);
        if let Some(receipt) = self.latest {
            self.complete(receipt);
        }
    }
}

// A source-only waiter can be woken by a completion yet immediately park again
// without observing it. Poll both channels on the single owner, retire leases,
// and then accept source work. No timer or additional completion worker.
enum OwnerEvent {
    Completion,
    Source,
}
fn wait_source(
    gpu: &mut WgpuFrameRenderer,
    completions: &mut CompletionReceiver,
    observations: &mut Observations,
    source: &mut gpui_box::retained::reactive::UpdateReceiver,
) -> Result<OwnerEvent, Box<dyn Error>> {
    let waker = Waker::from(Arc::new(OwnerWake(std::thread::current())));
    let mut context = Context::from_waker(&waker);
    let timeout = Instant::now() + Duration::from_secs(35);
    loop {
        if completion(gpu, completions, observations, false)? {
            return Ok(OwnerEvent::Completion);
        }
        {
            let mut ready = std::pin::pin!(source.ready());
            if let Poll::Ready(result) = ready.as_mut().poll(&mut context) {
                assert!(result?, "source closed before final tick");
                return Ok(OwnerEvent::Source);
            }
        }
        let remaining = timeout.saturating_duration_since(Instant::now());
        assert!(!remaining.is_zero(), "source/completion wait timed out");
        std::thread::park_timeout(remaining);
    }
}
fn completion(
    gpu: &mut WgpuFrameRenderer,
    receiver: &mut CompletionReceiver,
    observations: &mut Observations,
    block: bool,
) -> Result<bool, Box<dyn Error>> {
    let waker = Waker::from(Arc::new(OwnerWake(std::thread::current())));
    let mut context = Context::from_waker(&waker);
    let mut ready = std::pin::pin!(receiver.ready_receipt());
    let timeout = Instant::now() + Duration::from_secs(35);
    loop {
        match ready.as_mut().poll(&mut context) {
            Poll::Ready(result) => {
                let receipt = result?.ok_or("completion subscription closed")?;
                observations.complete(receipt);
                gpu.poll()?;
                return Ok(true);
            }
            Poll::Pending if !block => return Ok(false),
            Poll::Pending => {
                let remaining = timeout.saturating_duration_since(Instant::now());
                assert!(!remaining.is_zero(), "pipeline completion timed out");
                std::thread::park_timeout(remaining);
            }
        }
    }
}
fn capacity(
    gpu: &mut WgpuFrameRenderer,
    receiver: &mut CompletionReceiver,
    observations: &mut Observations,
) -> Result<(Duration, usize), Box<dyn Error>> {
    let start = Instant::now();
    let mut waits = 0;
    while !gpu.poll_frame_capacity()?.available() {
        completion(gpu, receiver, observations, true)?;
        waits += 1;
    }
    Ok((start.elapsed(), waits))
}

fn case(
    count: usize,
    batch: bool,
    effects: bool,
    hz: u32,
    limit: usize,
    before_work: bool,
    seconds: f64,
) -> Result<Value, Box<dyn Error>> {
    let burst = std::env::args().any(|v| v == "--pipeline-burst-only");
    let service = !std::env::args().any(|v| v == "--source-only-wait");
    let (tree, parent, nodes) = fixture(count, effects)?;
    let mut ui = ReactiveUi::new(
        tree,
        BackendCapabilities {
            solid_color_patch: true,
        },
    )?;
    let initial = Instant::now();
    let signal = ui.signal(VisualSample {
        tick: 0,
        scheduled: initial,
        generated: initial,
    });
    let placement = ui.computed(&[signal.dependency()], move |inputs| {
        let tick = inputs.read(signal)?.tick;
        if std::env::args().any(|v| v == "--integer-motion") {
            let value = if tick % 2 == 0 { 2.0 } else { -2.0 };
            return Ok([value, value]);
        }
        Ok([
            1.25 + (tick % 31) as f32 * 0.125,
            2.5 + (tick % 31) as f32 * 0.125,
        ])
    })?;
    for node in if batch { nodes.as_slice() } else { std::slice::from_ref(&parent) } {
        ui.bind(placement, Property::translation(*node))?;
    }
    if burst {
        let color = ui.computed(&[signal.dependency()], move |inputs| {
            Ok(dense_burst::color(dense_burst::epoch(inputs.read(signal)?.tick, hz)))
        })?;
        for node in &nodes { ui.bind(color, Property::color(*node))?; }
    }
    let mut receiver = ui.open_updates(1)?;
    let sender = ui.update_sender(&receiver, signal)?;
    let mut gpu = WgpuFrameRenderer::new(limit)?;
    let scale = if limit == 1 { 3.0 } else { 1.0 };
    let surface = if limit == 1 { [2400, 1800] } else { [800, 600] };
    let initial_frame = ui.local_frame([800.0, 600.0])?;
    gpu.submit_local(&initial_frame, surface, scale)?;
    gpu.finish()?;
    let mut completions = gpu.open_completions()?;
    let rate = pacing::FrameRate::hz(hz).unwrap();
    let origin = Instant::now() + Duration::from_millis(20);
    let slots = (seconds * f64::from(hz)) as usize;
    let producer = std::thread::spawn(move || -> Result<(), gpui_box::retained::Error> {
        let mut clock = pacing::FrameClock::new(origin, rate);
        loop {
            if let Some(wait) = clock
                .next_sample_at()
                .unwrap()
                .checked_duration_since(Instant::now())
            {
                std::thread::sleep(wait);
            }
            let generated = Instant::now();
            let tick = clock.sample(generated).unwrap().index.min(slots as u64 - 1);
            sender.send(VisualSample {
                tick,
                generated,
                scheduled: origin + Duration::from_secs_f64(tick as f64 / f64::from(hz)),
            })?;
            if tick + 1 == slots as u64 {
                break;
            }
        }
        Ok(())
    });
    let mut observations = Observations {
        pending: VecDeque::new(),
        rows: vec![],
        latest: Some(CompletionReceipt {
            serial: gpu.lifecycle().completed,
            observed_at: Instant::now(),
        }),
        origin,
        budget: rate.budget(),
    };
    let mut next = 0;
    let mut waits = 0;
    let mut maximum_pending = 0;
    let mut maximum_gpu = 0;
    let mut resource_waits = 0;
    let mut last_burst = 0;
    let mut full_resyncs = 0;
    while next < slots {
        while completion(&mut gpu, &mut completions, &mut observations, false)? {}
        if service {
            while matches!(
                wait_source(&mut gpu, &mut completions, &mut observations, &mut receiver)?,
                OwnerEvent::Completion
            ) {}
        } else {
            assert!(wait_ready(receiver.ready())?);
        }
        let (mut admission_wait, n) = if before_work {
            capacity(&mut gpu, &mut completions, &mut observations)?
        } else {
            (Duration::ZERO, 0)
        };
        waits += n;
        let entered = Instant::now();
        let applied = ui.apply_updates(&mut receiver)?;
        assert_eq!(applied.applied, 1);
        let committed = Instant::now();
        let sample = *ui.read(signal)?;
        let superseded = sample.tick as usize - next;
        let frame = ui.local_frame([800.0, 600.0])?;
        let published = Instant::now();
        if !before_work {
            let (elapsed, n) = capacity(&mut gpu, &mut completions, &mut observations)?;
            admission_wait += elapsed;
            waits += n;
        }
        let submission = gpu.submit_local(&frame, surface, scale)?;
        let submitted = Instant::now();
        #[cfg(feature = "retained-diagnostics")]
        let diagnostics = submission.as_ref().map_or(Value::Null, |s| json!({
            "local_us":s.local_phase_times.map(|phases| phases.map(|p| p.as_secs_f64()*1e6)),
            "preparation_us":s.local_preparation_times.map(|phases| phases.map(|p| p.as_secs_f64()*1e6)),
            "cleanup_us":s.local_cleanup_time.map(|p| p.as_secs_f64()*1e6),
            "material_sample_us":s.material_sample_times.map(|(phases, _)| phases.map(|p| p.as_secs_f64()*1e6)),
            "material_sample_count":s.material_sample_times.map(|(_, count)| count),
            "material_item_count":s.validation.validated_items,
            "backend_us":s.gpu.phase_times.map(|p| p.as_secs_f64()*1e6),
            "quad_bytes":s.gpu.uploaded_bytes,"quad_ranges":s.gpu.upload_ranges,
            "spatial_bytes":s.gpu.spatial_upload_bytes,
            "quad_references":s.gpu.spatial_work.quad_references,
            "rasterized_layers":s.gpu.rasterized_layers,"reused_layers":s.gpu.reused_layers
        }));
        #[cfg(not(feature = "retained-diagnostics"))]
        let diagnostics = Value::Null;
        if let Some(s) = &submission {
            maximum_gpu = maximum_gpu.max(s.gpu.in_flight);
            resource_waits += s.gpu.layer_budget_waits;
            full_resyncs += usize::from(s.full_resync);
        }
        let burst_epoch = if burst { dense_burst::epoch(sample.tick, hz) } else { 0 };
        observations.push(PendingFrame {
            serial: gpu.lifecycle().submitted,
            sample,
            entered,
            committed,
            published,
            submitted,
            admission_wait,
            superseded,
            gpu_submitted: submission.is_some(),
            burst_epoch,
            burst_epochs_crossed: burst_epoch - last_burst,
            diagnostics,
        });
        last_burst = burst_epoch;
        maximum_pending = maximum_pending.max(observations.pending.len());
        next = sample.tick as usize + 1;
    }
    producer.join().expect("pipeline producer panicked")?;
    while !observations.pending.is_empty() {
        completion(&mut gpu, &mut completions, &mut observations, true)?;
    }
    gpu.poll()?;
    assert_eq!(gpu.lifecycle().frame_leases, 0);
    assert_eq!(gpu.lifecycle().gpu_leases, 0);
    assert!(maximum_gpu <= limit);
    let frame = ui.frame([800.0, 600.0])?;
    assert_eq!(
        gpu.image()?.as_raw(),
        gpu.reference_image(&frame, surface, scale)?.as_raw()
    );
    let rows = &observations.rows;
    let mut miss_streak = 0;
    let mut longest = 0;
    let mut skipped = 0;
    let mut missed = 0;
    for row in rows {
        let superseded = row["superseded_ticks"].as_u64().unwrap();
        skipped += superseded;
        miss_streak += superseded;
        longest = longest.max(miss_streak);
        if row["deadline_missed"] == true {
            miss_streak += 1;
            missed += 1;
            longest = longest.max(miss_streak);
        } else {
            miss_streak = 0;
        }
    }
    assert_eq!(rows.len() as u64 + skipped, slots as u64);
    let phases: serde_json::Map<String, Value> = [
        "commit_us",
        "publication_us",
        "submit_and_admission_us",
        "admission_wait_us",
        "completion_to_owner_observation_us",
        "source_to_owner_completion_observation_us",
    ]
    .into_iter()
    .map(|key| {
        (
            key.to_string(),
            distribution(rows.iter().map(|r| r[key].as_f64().unwrap())),
        )
    })
    .collect();
    Ok(
        json!({"nodes":count,"batch":batch,"effects":effects,"burst":burst,"service_completions_while_waiting":service,"target_hz":hz,"max_in_flight":limit,
        "surface":surface,"scale":scale,"admission_before_work":before_work,"expected_ticks":slots,"consumed_ticks":rows.len(),
        "superseded_ticks":skipped,"deadline_misses_upper_bound":missed,"longest_consecutive_misses_upper_bound":longest,
        "unsatisfied_ticks_upper_bound":skipped+missed,"admission_waits":waits,"layer_budget_waits":resource_waits,
        "maximum_gpu_in_flight":maximum_gpu,"maximum_pending_receipts":maximum_pending,
        "dense_epochs_crossed":last_burst,"full_resyncs_after_initial":full_resyncs,
        "pixel_equal":true,"leases_retired":true,"phases":phases,
        "update_queue":format!("{:?}",receiver.stats()?),"completion_queue":format!("{:?}",completions.stats()),
        "source_to_completion_upper_bound":distribution(rows.iter().map(|r|r["source_to_completion_upper_bound_us"].as_f64().unwrap())),
        "raw":rows}),
    )
}

pub(super) fn run(output: &std::path::Path, quick: bool) -> Result<(), Box<dyn Error>> {
    let mut cases = vec![];
    let pressure = std::env::args().any(|v| v == "--pressure");
    let burst = std::env::args().any(|v| v == "--pipeline-burst-only");
    for (count, batch) in if pressure || burst {
        vec![(10000, false)]
    } else {
        vec![(1000, false), (10000, false), (10000, true)]
    } {
        for effects in if burst { vec![true] } else { vec![false, true] } {
            for hz in [60, 120] {
                for before in if std::env::args().any(|v| v == "--reverse") {
                    [true, false]
                } else {
                    [false, true]
                } {
                    let result = case(
                        count,
                        batch,
                        effects,
                        hz,
                        if pressure { 1 } else { 2 },
                        before,
                        if quick { 1.0 } else { 10.0 },
                    )?;
                    println!(
                        "pipeline nodes={count} batch={batch} effects={effects} hz={hz} before={before}: {} skips={} misses={}",
                        result["source_to_completion_upper_bound"],
                        result["superseded_ticks"],
                        result["deadline_misses_upper_bound"]
                    );
                    cases.push(result);
                }
            }
        }
    }
    std::fs::write(
        output.join("pipeline.json"),
        serde_json::to_vec_pretty(&json!({
        "scope":"independent source -> owner -> local snapshot -> two-in-flight GPU; coalesced native completion receipts are conservative upper bounds; no native present/display/input",
        "quick":quick,"cases":cases}))?,
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn owner_services_completion_without_another_source_update() -> Result<(), Box<dyn Error>> {
        let tree = UiTree::new(absolute(0.0, 0.0, 16.0, 16.0), 0x4488ccff)?;
        let mut ui = ReactiveUi::new(tree, BackendCapabilities { solid_color_patch: true })?;
        let mut source = ui.open_updates(1)?;
        let mut gpu = WgpuFrameRenderer::new(2)?;
        let mut completions = gpu.open_completions()?;
        let frame = ui.local_frame([16.0, 16.0])?;
        gpu.submit_local(&frame, [16, 16], 1.0)?;
        let serial = gpu.lifecycle().submitted;
        let mut observations = Observations {
            pending: VecDeque::new(), rows: vec![], latest: None,
            origin: Instant::now(), budget: Duration::from_millis(8),
        };
        // The source remains open and idle. A source-only waiter cannot return;
        // the owner must wake on completion and collect the submitted leases.
        assert!(matches!(wait_source(&mut gpu, &mut completions, &mut observations, &mut source)?, OwnerEvent::Completion));
        assert!(observations.latest.unwrap().serial >= serial);
        assert_eq!(gpu.lifecycle().frame_leases, 0);
        assert_eq!(gpu.lifecycle().gpu_leases, 0);
        let waker = Waker::from(Arc::new(OwnerWake(std::thread::current())));
        let mut context = Context::from_waker(&waker);
        assert!(std::pin::pin!(source.ready()).as_mut().poll(&mut context).is_pending());
        Ok(())
    }
}
