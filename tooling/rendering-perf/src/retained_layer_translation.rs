use super::*;
use std::time::Instant;
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
pub(super) fn run(
    output: &std::path::Path,
    results: &mut Vec<serde_json::Value>,
) -> Result<(), Box<dyn Error>> {
    for shared in [false, true] {
        let mut tree = UiTree::new(absolute(0.0, 0.0, 320.0, 220.0), 0x171d28ff)?;
        let group = tree.append(tree.root(), absolute(48.0, 48.0, 180.0, 120.0), 0)?;
        let child = tree.append(group, absolute(8.0, 8.0, 100.0, 70.0), 0x8844cc80)?;
        let mut tx = Transaction::default();
        tx.opacity(group, 0.6);
        tx.decoration(
            child,
            QuadDecoration {
                radius: 12.0,
                border_width: 3.0,
                border_color: 0xff8844ff,
            },
        );
        tx.shadows(
            child,
            vec![BoxShadow {
                color: 0x00000088,
                offset: [2.0, 3.0],
                blur_radius: 4.0,
                ..Default::default()
            }],
        );
        tree.commit(tx, BackendCapabilities::default())?;
        let mut renderer = WgpuFrameRenderer::new(3)?;
        renderer.set_shared_spatial_enabled(shared);
        renderer.set_layer_translation_reuse_enabled(true);
        for (step, offset) in [
            [0.0, 0.0],
            [8.0, 8.0],
            [-8.0, -8.0],
            [0.5, 0.0],
            [8.5, 0.0],
            [0.0, 0.0],
        ]
        .into_iter()
        .enumerate()
        {
            let mut tx = Transaction::default();
            tx.translation(group, offset);
            tree.commit(tx, BackendCapabilities::default())?;
            let frame = tree.frame([320.0, 220.0])?;
            let submit = renderer.submit(&frame, [320, 220], 1.0)?.unwrap();
            if step == 1 || step == 2 {
                assert_eq!(submit.gpu.rasterized_layers, 0);
                assert_eq!(submit.gpu.reused_layers, 1);
            }
            if step == 3 || step == 4 {
                assert_eq!(submit.gpu.rasterized_layers, 1);
            }
            compare(
                &mut renderer,
                &frame,
                [320, 220],
                1.0,
                &format!("layer-translation-{shared}-{step}"),
                output,
                results,
            )?;
        }
        // Child-local movement, material, external clip, scale and source reset
        // must not inherit the previous whole-group translation eligibility.
        for step in 0..5 {
            let mut tx = Transaction::default();
            tx.translation(group, [16.0 + step as f32 * 8.0, 0.0]);
            match step {
                0 => tx.translation(child, [4.0, 0.0]),
                1 => tx.color(child, 0xff000080),
                2 => tx.clip(
                    tree.root(),
                    Some(Rect {
                        x: 0.0,
                        y: 0.0,
                        width: 100.0,
                        height: 220.0,
                    }),
                ),
                3 => tx.clip(tree.root(), None),
                _ => (),
            }
            tree.commit(tx, BackendCapabilities::default())?;
            if step == 4 {
                renderer.reset()?;
            }
            let frame = tree.frame([320.0, 220.0])?;
            let submit = renderer.submit(&frame, [320, 220], 1.0)?.unwrap();
            assert!(submit.gpu.rasterized_layers > 0);
            compare(
                &mut renderer,
                &frame,
                [320, 220],
                1.0,
                &format!("layer-translation-invalidate-{shared}-{step}"),
                output,
                results,
            )?;
        }
    }
    boundaries(output, results)?;
    // Queue-order updates of the same camera/texture must not mutate previously
    // submitted raster work. No readback/finish separates these submissions.
    {
        let mut tree = UiTree::new(absolute(0.0, 0.0, 128.0, 96.0), 0x171d28ff)?;
        let group = tree.append(tree.root(), absolute(24.0, 24.0, 40.0, 32.0), 0xff000080)?;
        let mut tx = Transaction::default();
        tx.opacity(group, 0.6);
        tree.commit(tx, BackendCapabilities::default())?;
        let mut renderer = WgpuFrameRenderer::new(3)?;
        renderer.set_layer_translation_reuse_enabled(true);
        renderer.submit(&tree.frame([128.0, 96.0])?, [128, 96], 1.0)?;
        let mut final_frame = None;
        for step in 0..12 {
            let mut tx = Transaction::default();
            tx.translation(group, [if step % 2 == 0 { 8.0 } else { 0.0 }, 0.0]);
            tree.commit(tx, BackendCapabilities::default())?;
            let frame = tree.frame([128.0, 96.0])?;
            let submit = renderer.submit(&frame, [128, 96], 1.0)?.unwrap();
            assert_eq!(submit.gpu.rasterized_layers, 0);
            assert_eq!(submit.gpu.layer_allocations, 0);
            final_frame = Some(frame);
        }
        compare(
            &mut renderer,
            &final_frame.unwrap(),
            [128, 96],
            1.0,
            "layer-translation-multiflight",
            output,
            results,
        )?;
    }
    // Heavy overdraw reveals raster savings; alternating order retains cold and
    // warm losses. This is a targeted worker benchmark, not 120 Hz acceptance.
    let mut tree = UiTree::new(absolute(0.0, 0.0, 1000.0, 760.0), 0x171d28ff)?;
    let group = tree.append(tree.root(), absolute(40.0, 40.0, 900.0, 650.0), 0)?;
    for _ in 0..128 {
        tree.append(group, absolute(0.0, 0.0, 900.0, 650.0), 0x4488cc40)?;
    }
    let mut tx = Transaction::default();
    tx.opacity(group, 0.6);
    tree.commit(tx, BackendCapabilities::default())?;
    let mut renderer = WgpuFrameRenderer::new(3)?;
    let mut captures = vec![];
    for round in 0..2 {
        for enabled in if round == 0 {
            [false, true]
        } else {
            [true, false]
        } {
            renderer.finish()?;
            renderer.set_layer_translation_reuse_enabled(enabled);
            renderer.submit(&tree.frame([1000.0, 760.0])?, [1000, 760], 1.0)?;
            renderer.finish()?;
            for step in 0..24 {
                let start = Instant::now();
                let mut tx = Transaction::default();
                tx.translation(group, [if step % 2 == 0 { 8.0 } else { 0.0 }, 0.0]);
                tree.commit(tx, BackendCapabilities::default())?;
                let frame = tree.frame([1000.0, 760.0])?;
                let submit = renderer.submit(&frame, [1000, 760], 1.0)?.unwrap();
                let submit_us = start.elapsed().as_secs_f64() * 1e6;
                renderer.finish()?;
                if step >= 4 {
                    captures.push(serde_json::json!({"round":round,"enabled":enabled,"step":step,"owner_submit_us":submit_us,"completed_us":start.elapsed().as_secs_f64()*1e6,"rasterized_layers":submit.gpu.rasterized_layers,"reused_layers":submit.gpu.reused_layers,"rasterized_pixels":submit.gpu.rasterized_layer_pixels}));
                }
            }
            let frame = tree.frame([1000.0, 760.0])?;
            compare(
                &mut renderer,
                &frame,
                [1000, 760],
                1.0,
                &format!("layer-translation-bench-{round}-{enabled}"),
                output,
                results,
            )?;
        }
    }
    fs::write(
        output.join("translation-benchmark.json"),
        serde_json::to_vec_pretty(&captures)?,
    )?;
    cadence(output, false)?;
    Ok(())
}

fn boundaries(
    output: &std::path::Path,
    results: &mut Vec<serde_json::Value>,
) -> Result<(), Box<dyn Error>> {
    for scenario in [
        "viewport",
        "fixed-clip",
        "nested",
        "image",
        "text",
        "scale",
        "opacity",
    ] {
        let mut tree = UiTree::new(absolute(0.0, 0.0, 128.0, 96.0), 0x171d28ff)?;
        let group = tree.append(tree.root(), absolute(24.0, 24.0, 64.0, 48.0), 0x8040cc80)?;
        let child = tree.append(group, absolute(4.0, 4.0, 40.0, 32.0), 0xff880080)?;
        let mut tx = Transaction::default();
        tx.opacity(group, 0.6);
        if scenario == "fixed-clip" {
            tx.clip(
                tree.root(),
                Some(Rect {
                    x: 0.0,
                    y: 0.0,
                    width: 60.0,
                    height: 96.0,
                }),
            );
        }
        if scenario == "nested" {
            tx.opacity(child, 0.7);
        }
        if scenario == "image" {
            tx.image(
                child,
                Some(ImageResource::rgba8([1, 1], vec![255, 255, 255, 128])?),
            );
        }
        let (context, mut raster) = retained_text::context()?;
        if scenario == "text" {
            tx.text(
                child,
                Some(text::TextPaint {
                    resource: retained_text::text(&context, &mut raster, "A", 1.0)?,
                    color: 0xffffffff,
                }),
            );
        }
        tree.commit(tx, BackendCapabilities::default())?;
        let mut renderer = WgpuFrameRenderer::new(3)?;
        renderer.set_layer_translation_reuse_enabled(true);
        let offsets = if scenario == "viewport" {
            [0.0, -32.0, -16.0, 48.0, 0.0]
        } else {
            [0.0, 8.0, -8.0, 16.0, 0.0]
        };
        for (step, x) in offsets.into_iter().enumerate() {
            let mut tx = Transaction::default();
            tx.translation(group, [x, 0.0]);
            if scenario == "opacity" {
                tx.opacity(group, 0.3 + step as f32 * 0.1);
            }
            tree.commit(tx, BackendCapabilities::default())?;
            let scale = if scenario == "scale" && step > 0 {
                1.5
            } else {
                1.0
            };
            let size = [(128.0 * scale) as u32, (96.0 * scale) as u32];
            let frame = tree.frame([128.0, 96.0])?;
            let submit = renderer.submit(&frame, size, scale)?.unwrap();
            if step > 0 && matches!(scenario, "nested" | "image" | "text" | "scale") {
                assert!(submit.gpu.rasterized_layers > 0);
            }
            if step > 0 && scenario == "opacity" {
                assert_eq!(submit.gpu.rasterized_layers, 0);
            }
            compare(
                &mut renderer,
                &frame,
                size,
                scale,
                &format!("layer-translation-boundary-{scenario}-{step}"),
                output,
                results,
            )?;
        }
    }
    Ok(())
}

pub(super) fn run_local(
    output: &std::path::Path,
    results: &mut Vec<serde_json::Value>,
) -> Result<(), Box<dyn Error>> {
    for tight in [false, true] {
        let mut tree = UiTree::new(absolute(0.0, 0.0, 320.0, 220.0), 0x171d28ff)?;
        let group = tree.append(tree.root(), absolute(48.0, 48.0, 100.0, 70.0), 0x8844cc80)?;
        let child = tree.append(group, absolute(8.0, 8.0, 40.0, 30.0), 0xff880080)?;
        let mut tx = Transaction::default();
        tx.opacity(group, 0.6);
        tx.decoration(
            child,
            QuadDecoration {
                radius: 6.0,
                border_width: 2.0,
                border_color: 0xffffffff,
            },
        );
        tree.commit(tx, BackendCapabilities::default())?;
        let mut renderer = WgpuFrameRenderer::new(3)?;
        renderer.set_tight_layer_bounds_enabled(tight);
        renderer.set_local_tight_layer_bounds_enabled(tight);
        renderer.set_layer_translation_reuse_enabled(true);
        let mut saved = vec![];
        for step in 0..12 {
            let mut tx = Transaction::default();
            tx.translation(
                group,
                match step {
                    0 => [0.0; 2],
                    1 => [8.0, 8.0],
                    2 => [-8.0, -8.0],
                    3 => [0.5, 0.0],
                    _ => [step as f32 * 2.0, 0.0],
                },
            );
            if step == 5 {
                tx.clip(
                    group,
                    Some(Rect {
                        x: 2.0,
                        y: 3.0,
                        width: 50.0,
                        height: 40.0,
                    }),
                );
            }
            if step == 6 {
                tx.scroll(group, [4.0, 2.0]);
            }
            if step == 7 {
                tx.opacity(group, 0.4);
            }
            if step == 8 {
                tx.color(child, 0x00ff0080);
            }
            tree.commit(tx, BackendCapabilities::default())?;
            let local = tree.local_frame([320.0, 220.0])?;
            let stats = renderer.submit_local(&local, [320, 220], 1.0)?.unwrap();
            if tight && matches!(step, 1 | 2) {
                assert_eq!(stats.gpu.rasterized_layers, 0);
                assert_eq!(stats.gpu.reused_layers, 1);
                assert_eq!(stats.composition.bounds_items, 2);
            }
            if !tight && matches!(step, 1 | 2) {
                assert_eq!(stats.gpu.rasterized_layers, 1);
            }
            let resolved = local.resolve();
            compare(
                &mut renderer,
                &resolved,
                [320, 220],
                1.0,
                &format!("local-layer-translation-{tight}-{step}"),
                output,
                results,
            )?;
            assert_eq!(local.hit_test(60.0, 60.0), resolved.hit_test(60.0, 60.0));
            saved.push(local);
        }
        renderer.reset()?;
        renderer.submit_local(saved.last().unwrap(), [320, 220], 1.0)?;
        compare(
            &mut renderer,
            &saved.last().unwrap().resolve(),
            [320, 220],
            1.0,
            &format!("local-layer-reset-{tight}"),
            output,
            results,
        )?;
        // Missing predecessor must rebuild index and recover all content.
        for i in 0..2 {
            let mut tx = Transaction::default();
            tx.translation(child, [i as f32 * 8.0, 0.0]);
            tree.commit(tx, BackendCapabilities::default())?;
            let local = tree.local_frame([320.0, 220.0])?;
            if i == 1 {
                renderer.submit_local(&local, [320, 220], 1.0)?;
                compare(
                    &mut renderer,
                    &local.resolve(),
                    [320, 220],
                    1.0,
                    &format!("local-layer-skipped-{tight}"),
                    output,
                    results,
                )?;
            }
        }
    }
    cadence(output, true)
}

fn cadence(output: &std::path::Path, local: bool) -> Result<(), Box<dyn Error>> {
    use std::{
        sync::{Arc, Mutex},
        time::Duration,
    };
    let mut captures = vec![];
    for hz in [60u32, 120] {
        for enabled in if hz == 60 {
            [false, true]
        } else {
            [true, false]
        } {
            let mut tree = UiTree::new(absolute(0.0, 0.0, 1000.0, 760.0), 0x171d28ff)?;
            let group = tree.append(tree.root(), absolute(40.0, 40.0, 900.0, 650.0), 0)?;
            for _ in 0..128 {
                tree.append(group, absolute(0.0, 0.0, 900.0, 650.0), 0x4488cc40)?;
            }
            let mut tx = Transaction::default();
            tx.opacity(group, 0.6);
            tree.commit(tx, BackendCapabilities::default())?;
            let mut renderer = WgpuFrameRenderer::new(3)?;
            renderer.set_local_tight_layer_bounds_enabled(local);
            renderer.set_layer_translation_reuse_enabled(enabled);
            if local {
                renderer.submit_local(&tree.local_frame([1000.0, 760.0])?, [1000, 760], 1.0)?;
            } else {
                renderer.submit(&tree.frame([1000.0, 760.0])?, [1000, 760], 1.0)?;
            }
            renderer.finish()?;
            let origin = Instant::now() + Duration::from_millis(20);
            let ticks = hz as u64 * 10;
            let pending = Arc::new(Mutex::new((None::<(u64, Instant)>, 0u64, false)));
            let source = pending.clone();
            let owner = std::thread::current();
            let worker = std::thread::spawn(move || {
                for tick in 0..ticks {
                    let at = origin + Duration::from_secs_f64(tick as f64 / hz as f64);
                    if let Some(wait) = at.checked_duration_since(Instant::now()) {
                        std::thread::sleep(wait);
                    }
                    let mut state = source.lock().unwrap();
                    if state.0.is_some() {
                        state.1 += 1;
                    }
                    state.0 = Some((tick, at));
                    drop(state);
                    owner.unpark();
                }
                source.lock().unwrap().2 = true;
                owner.unpark();
            });
            let mut samples = vec![];
            let mut ontime = std::collections::BTreeSet::new();
            loop {
                let mut state = pending.lock().unwrap();
                let sample = state.0.take();
                let done = state.2;
                drop(state);
                let Some((tick, at)) = sample else {
                    if done {
                        break;
                    }
                    std::thread::park_timeout(Duration::from_millis(10));
                    continue;
                };
                let entered = Instant::now();
                let mut tx = Transaction::default();
                tx.translation(group, [if tick % 2 == 0 { 8.0 } else { 0.0 }, 0.0]);
                tree.commit(tx, BackendCapabilities::default())?;
                let stats = if local {
                    renderer.submit_local(&tree.local_frame([1000.0, 760.0])?, [1000, 760], 1.0)?
                } else {
                    renderer.submit(&tree.frame([1000.0, 760.0])?, [1000, 760], 1.0)?
                };
                let submitted = Instant::now();
                renderer.finish()?;
                let completed = Instant::now();
                let met = completed <= at + Duration::from_secs_f64(1.0 / hz as f64);
                if met {
                    ontime.insert(tick);
                }
                samples.push(serde_json::json!({"tick":tick,"source_to_owner_us":entered.saturating_duration_since(at).as_secs_f64()*1e6,
                "owner_submit_us":submitted.duration_since(entered).as_secs_f64()*1e6,"completion_us":completed.saturating_duration_since(at).as_secs_f64()*1e6,
                "deadline_met":met,"bounds_items":stats.as_ref().map_or(0,|s|s.composition.bounds_items),"rasterized_layers":stats.as_ref().map_or(0,|s|s.gpu.rasterized_layers),"reused_layers":stats.as_ref().map_or(0,|s|s.gpu.reused_layers)}));
            }
            worker.join().map_err(|_| "source worker panic")?;
            let coalesced = pending.lock().unwrap().1;
            assert_eq!(samples.len() as u64 + coalesced, ticks);
            let mut burst = 0;
            let mut longest = 0;
            for tick in 0..ticks {
                if ontime.contains(&tick) {
                    burst = 0;
                } else {
                    burst += 1;
                    longest = longest.max(burst);
                }
            }
            println!(
                "translation cadence hz={hz} enabled={enabled} deadline={}/{} coalesced={coalesced}",
                ontime.len(),
                ticks
            );
            captures.push(serde_json::json!({"hz":hz,"enabled":enabled,"local_snapshot":local,"duration_s":10,"source_ticks":ticks,"coalesced":coalesced,"deadline_met":ontime.len(),"longest_unsatisfied_burst":longest,"samples":samples}));
        }
    }
    fs::write(
        output.join("translation-cadence.json"),
        serde_json::to_vec_pretty(&captures)?,
    )?;
    Ok(())
}
