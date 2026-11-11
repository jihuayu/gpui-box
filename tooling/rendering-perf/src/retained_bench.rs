//! Completed-work wall-clock A/B. No PNG encoding or texture readback is timed.
use gpui_box::retained::{
    wgpu::{CompositorPolicy, WgpuFrameRenderer},
    *,
};
use serde_json::{Value, json};
use std::{error::Error, time::Instant};

fn distribution(mut values: Vec<f64>) -> Value {
    values.sort_by(f64::total_cmp);
    json!({
        "median_us": values[values.len() / 2],
        "p95_us": values[(values.len() * 95 / 100).min(values.len() - 1)],
    })
}

pub(super) fn run() -> Result<Value, Box<dyn Error>> {
    let mut cases = vec![];
    for (name, surface, overdraw) in [
        ("grid-576-1080p", [1920, 1080], false),
        ("overdraw-32-1080p", [1920, 1080], true),
        ("overdraw-32-2160p", [3840, 2160], true),
    ] {
        let viewport = [surface[0] as f32, surface[1] as f32];
        let mut tree = UiTree::new(
            Style {
                size: taffy::Size {
                    width: percent(1.0),
                    height: percent(1.0),
                },
                flex_wrap: taffy::FlexWrap::Wrap,
                ..Default::default()
            },
            0x171d28ff,
        )?;
        let absolute = |x, y, width, height| Style {
            position: taffy::Position::Absolute,
            inset: taffy::Rect {
                left: length(x),
                top: length(y),
                right: taffy::LengthPercentageAuto::auto(),
                bottom: taffy::LengthPercentageAuto::auto(),
            },
            size: taffy::Size {
                width: length(width),
                height: length(height),
            },
            ..Default::default()
        };
        if overdraw {
            for _ in 0..32 {
                tree.append(
                    tree.root(),
                    absolute(0.0, 0.0, viewport[0], viewport[1]),
                    0x4488cc22,
                )?;
            }
        } else {
            for i in 0..576 {
                tree.append(
                    tree.root(),
                    absolute(
                        (i % 32) as f32 * viewport[0] / 32.0,
                        (i / 32) as f32 * viewport[1] / 18.0,
                        viewport[0] / 32.0,
                        viewport[1] / 18.0,
                    ),
                    0x245fc7ff,
                )?;
            }
        }
        let tile = tree.append(
            tree.root(),
            absolute(viewport[0] / 2.0, viewport[1] / 2.0, 24.0, 24.0),
            0xcc554480,
        )?;
        let mut renderer = WgpuFrameRenderer::new(3)?;
        let modes = [
            ("full", CompositorPolicy::Full),
            ("damage", CompositorPolicy::Damage),
            (
                "auto-35pct",
                CompositorPolicy::Auto {
                    max_damage_fraction: 0.35,
                },
            ),
        ];
        let mut cpu: [Vec<f64>; 3] = Default::default();
        let mut submit: [Vec<f64>; 3] = Default::default();
        let mut completed: [Vec<f64>; 3] = Default::default();
        let mut pixels = [0; 3];
        let mut draw_calls = [0; 3];
        const SAMPLES: usize = 40;
        const WARMUP: usize = 8;
        let mut color = false;
        for round in 0..3 {
            // Rotate the order to reduce a fixed ordering/clock bias.
            for offset in 0..3 {
                let mode = (round + offset) % 3;
                renderer.set_policy(modes[mode].1)?;
                renderer.submit(&tree.frame(viewport)?, surface, 1.0)?;
                renderer.finish()?;
                let mut batch_start = None;
                for sample in 0..WARMUP + SAMPLES {
                    if sample == WARMUP {
                        renderer.finish()?;
                        batch_start = Some(Instant::now());
                    }
                    let start = Instant::now();
                    color = !color;
                    let mut tx = Transaction::default();
                    tx.color(tile, if color { 0xeeaa4480 } else { 0xcc554480 });
                    tree.commit(
                        tx,
                        BackendCapabilities {
                            solid_color_patch: true,
                        },
                    )?;
                    let frame = tree.frame(viewport)?;
                    let prepared = Instant::now();
                    let submission = renderer
                        .submit(&frame, surface, 1.0)?
                        .expect("changed frame");
                    let submitted = Instant::now();
                    if sample >= WARMUP {
                        cpu[mode].push((prepared - start).as_secs_f64() * 1e6);
                        submit[mode].push((submitted - prepared).as_secs_f64() * 1e6);
                        pixels[mode] = submission.gpu.drawn_pixels;
                        draw_calls[mode] = submission.gpu.draw_calls;
                    }
                }
                renderer.finish()?;
                completed[mode].push(batch_start.expect("timed batch").elapsed().as_secs_f64());
                // Check the benchmark's final pixels outside the timed interval.
                let actual = renderer.image()?;
                let reference = renderer.reference_image(&tree.frame(viewport)?, surface, 1.0)?;
                assert!(
                    actual.as_raw() == reference.as_raw(),
                    "benchmark pixels in {name}/{}",
                    modes[mode].0
                );
            }
        }
        let full_mean = completed[0].iter().sum::<f64>() * 1e6 / (3 * SAMPLES) as f64;
        let measurements: Vec<_> = (0..3).map(|mode| {
            let mean = completed[mode].iter().sum::<f64>() * 1e6 / (3 * SAMPLES) as f64;
            json!({
                "policy": modes[mode].0,
                "completed_frame_mean_us": mean,
                "round_completed_frame_mean_us": completed[mode].iter().map(|seconds| seconds * 1e6 / SAMPLES as f64).collect::<Vec<_>>(),
                "speedup_vs_full": full_mean / mean,
                "commit_and_frame": distribution(cpu[mode].clone()),
                "submit_including_backpressure": distribution(submit[mode].clone()),
                "redraw_region_pixels": pixels[mode],
                "draw_calls": draw_calls[mode],
                "uploaded_bytes_per_change": 160,
                "pixel_equal": true,
            })
        }).collect();
        println!("{name}: {}", serde_json::to_string(&measurements)?);
        cases.push(json!({"case": name, "surface": surface, "measurements": measurements}));
    }
    Ok(json!({
        "scope": "completed-work wall clock: transaction + frame + submit + queue backpressure + final completion; no texture readback, no present, no GPU-only timestamp",
        "samples_per_policy": 120, "rounds": 3, "warmup_per_round": 8, "max_in_flight": 3,
        "cases": cases,
    }))
}
