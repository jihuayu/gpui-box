//! Native completion wake/retirement without submit, readback or UI polling.
use super::*;
use gpui_box::retained::wgpu::CompletionReceiver;
use std::{
    future::Future,
    sync::Arc,
    task::{Context, Poll, Wake, Waker},
    time::{Duration, Instant},
};
struct ThreadWake(std::thread::Thread);
impl Wake for ThreadWake {
    fn wake(self: Arc<Self>) {
        self.0.unpark();
    }
}
fn wait(receiver: &mut CompletionReceiver) -> Result<Option<u64>, Box<dyn Error>> {
    let waker = Waker::from(Arc::new(ThreadWake(std::thread::current())));
    let mut future = std::pin::pin!(receiver.ready());
    let deadline = Instant::now() + Duration::from_secs(35);
    loop {
        if let Poll::Ready(result) = future.as_mut().poll(&mut Context::from_waker(&waker)) {
            return result.map_err(Into::into);
        }
        let remaining = deadline.saturating_duration_since(Instant::now());
        assert!(!remaining.is_zero(), "GPU completion wake timed out");
        std::thread::park_timeout(remaining);
    }
}
pub(super) fn collect_through(
    renderer: &mut WgpuFrameRenderer,
    receiver: &mut CompletionReceiver,
    serial: u64,
) -> Result<(), Box<dyn Error>> {
    loop {
        let event = wait(receiver)?.expect("live completion subscription");
        assert!(renderer.lifecycle().completed >= event);
        renderer.poll()?;
        if event >= serial {
            break;
        }
    }
    assert_eq!(renderer.lifecycle().gpu_leases, 0);
    assert_eq!(renderer.lifecycle().frame_leases, 0);
    Ok(())
}
fn fixture(count: usize) -> Result<UiTree, Box<dyn Error>> {
    let mut tree = UiTree::new(
        Style {
            size: taffy::Size {
                width: length(256.0),
                height: length(128.0),
            },
            ..Default::default()
        },
        0x112233ff,
    )?;
    for _ in 0..count {
        tree.append(
            tree.root(),
            Style {
                size: taffy::Size {
                    width: length(2.0),
                    height: length(2.0),
                },
                ..Default::default()
            },
            0x4488ffff,
        )?;
    }
    Ok(tree)
}
pub(super) fn run(
    output: &std::path::Path,
    results: &mut Vec<serde_json::Value>,
) -> Result<(), Box<dyn Error>> {
    let mut renderer = WgpuFrameRenderer::new(3)?;
    let mut receiver = renderer.open_completions()?;
    assert!(renderer.open_completions().is_err());
    assert_eq!(receiver.stats().waits, 0);
    let mut tree = fixture(8)?;
    let image_node = tree.append(tree.root(), Style::default(), 0)?;
    let mut tx = Transaction::default();
    let resource = ImageResource::rgba8([2, 2], vec![255; 16])?;
    let weak_pixels = Arc::downgrade(resource.pixels());
    tx.image(image_node, Some(resource));
    tree.commit(tx, BackendCapabilities::default())?;
    let old = tree.frame([256.0, 128.0])?;
    let weak = Arc::downgrade(&old.items);
    renderer.submit(&old, [256, 128], 1.0)?;
    let mut tx = Transaction::default();
    tx.image(image_node, None);
    tx.color(tree.root(), 0x334455ff);
    tree.commit(
        tx,
        BackendCapabilities {
            solid_color_patch: true,
        },
    )?;
    let frame = tree.frame([256.0, 128.0])?;
    let last = renderer
        .submit(&frame, [256, 128], 1.0)?
        .unwrap()
        .gpu
        .serial;
    drop(old);
    collect_through(&mut renderer, &mut receiver, last)?;
    assert!(
        weak.upgrade().is_none(),
        "completion must release old packet without another frame"
    );
    assert!(
        weak_pixels.upgrade().is_none(),
        "retired image input must leave all CPU caches"
    );
    compare(
        &mut renderer,
        &frame,
        [256, 128],
        1.0,
        "completion-idle-retirement",
        output,
        results,
    )?;
    let stats = receiver.stats();
    assert_eq!(stats.scheduled, 2);
    results.last_mut().unwrap()["completion_jobs"] = stats.scheduled.into();
    results.last_mut().unwrap()["owner_waits"] = stats.delivered.into();
    results.last_mut().unwrap()["retired_without_new_frame"] = true.into();
    results.last_mut().unwrap()["retired_image_cpu_bytes"] = 16.into();

    // A consumer can attach after submission, and cancellation can happen with
    // a wait already in progress. The replacement covers the outstanding tail.
    drop(receiver);
    let mut tx = Transaction::default();
    tx.color(tree.root(), 0x556677ff);
    tree.commit(
        tx,
        BackendCapabilities {
            solid_color_patch: true,
        },
    )?;
    let frame = tree.frame([256.0, 128.0])?;
    let serial = renderer
        .submit(&frame, [256, 128], 1.0)?
        .unwrap()
        .gpu
        .serial;
    let receiver = renderer.open_completions()?;
    drop(receiver);
    let mut receiver = renderer.open_completions()?;
    collect_through(&mut renderer, &mut receiver, serial)?;
    compare(
        &mut renderer,
        &frame,
        [256, 128],
        1.0,
        "completion-late-resubscribe",
        output,
        results,
    )?;

    // Reset closes the old epoch. A new subscription covers a full resync.
    renderer.reset()?;
    assert_eq!(wait(&mut receiver)?, None);
    let mut receiver = renderer.open_completions()?;
    let submitted = renderer.submit(&frame, [256, 128], 1.0)?.unwrap();
    assert!(submitted.full_resync);
    collect_through(&mut renderer, &mut receiver, submitted.gpu.serial)?;
    compare(
        &mut renderer,
        &frame,
        [256, 128],
        1.0,
        "completion-reset-resubscribe",
        output,
        results,
    )?;
    drop(receiver);
    assert!(renderer.completion_stats().unwrap().closed);
    let mut receiver = renderer.open_completions()?;
    let mut tx = Transaction::default();
    tx.color(tree.root(), 0x778899ff);
    tree.commit(
        tx,
        BackendCapabilities {
            solid_color_patch: true,
        },
    )?;
    let frame = tree.frame([256.0, 128.0])?;
    renderer.submit(&frame, [256, 128], 1.0)?;
    drop(renderer);
    assert_eq!(wait(&mut receiver)?, None);
    Ok(())
}

pub(super) fn benchmark() -> Result<serde_json::Value, Box<dyn Error>> {
    let mut cases = vec![];
    for count in [1, 1000] {
        let mut tree = fixture(count)?;
        let mut renderer = WgpuFrameRenderer::new(3)?;
        let mut modes = [vec![], vec![]];
        let mut rounds = vec![];
        let mut stats = vec![];
        for round in 0..3 {
            for step in 0..2 {
                let mode = (round + step) % 2;
                let mut receiver = if mode == 1 {
                    Some(renderer.open_completions()?)
                } else {
                    None
                };
                let mut samples = vec![];
                for iteration in 0..68 {
                    let mut tx = Transaction::default();
                    tx.color(
                        tree.root(),
                        if iteration % 2 == 0 {
                            0x112233ff
                        } else {
                            0x334455ff
                        },
                    );
                    tree.commit(
                        tx,
                        BackendCapabilities {
                            solid_color_patch: true,
                        },
                    )?;
                    let frame = tree.frame([256.0, 128.0])?;
                    let start = Instant::now();
                    let serial = renderer
                        .submit(&frame, [256, 128], 1.0)?
                        .unwrap()
                        .gpu
                        .serial;
                    if let Some(receiver) = &mut receiver {
                        collect_through(&mut renderer, receiver, serial)?;
                    } else {
                        renderer.finish()?;
                    }
                    let us = start.elapsed().as_secs_f64() * 1e6;
                    if iteration >= 8 {
                        samples.push(us);
                    }
                }
                modes[mode].extend_from_slice(&samples);
                rounds.push(serde_json::json!({"round":round,"mode":mode,"samples_us":samples}));
                if let Some(receiver) = &receiver {
                    stats.push(serde_json::json!({"scheduled":receiver.stats().scheduled,"waits":receiver.stats().waits,"delivered":receiver.stats().delivered}));
                }
            }
        }
        let distribution = |samples: &mut Vec<f64>| {
            samples.sort_by(f64::total_cmp);
            serde_json::json!({"median_us":samples[samples.len()/2],"p95_us":samples[samples.len()*95/100],"samples_us":samples})
        };
        cases.push(serde_json::json!({"children":count,"blocking_owner":distribution(&mut modes[0]),"completion_wake":distribution(&mut modes[1]),"rounds":rounds,"completion_counts":stats}));
    }
    Ok(
        serde_json::json!({"scope":"Completed-work wall time including submit, driver wait and owner collection; 3 alternating rounds, 8 warmups + 60 measured samples/mode/round; no readback or presentation. Scheduling overhead is included, not an animation latency or energy measurement.","cases":cases}),
    )
}
