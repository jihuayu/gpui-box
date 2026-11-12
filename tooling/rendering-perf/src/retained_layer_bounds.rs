//! Bounded-layer correctness and same-renderer completed-work controls.
use super::*;
use std::{sync::Arc, time::Instant};

fn absolute(x: f32, y: f32, width: f32, height: f32) -> Style {
    Style {
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
    }
}
pub(super) fn run(
    renderer: &mut WgpuFrameRenderer,
    output: &std::path::Path,
    results: &mut Vec<serde_json::Value>,
) -> Result<(), Box<dyn Error>> {
    renderer.set_policy(CompositorPolicy::Damage)?;
    renderer.set_tight_layer_bounds_enabled(true);
    for scale in [1.0, 1.25, 1.5] {
        let (context, mut raster) = retained_text::context()?;
        let resource = retained_text::text(&context, &mut raster, "Crop 中文 overhang", scale)?;
        let mut tree = UiTree::new(absolute(0.0, 0.0, 320.0, 200.0), 0x171d28ff)?;
        let group = tree.append(tree.root(), absolute(41.25, 37.5, 180.0, 130.0), 0)?;
        let decorated = tree.append(group, absolute(-8.5, 2.25, 70.0, 52.0), 0x4488cc80)?;
        let image = tree.append(group, absolute(22.5, 12.25, 52.5, 40.5), 0)?;
        let nested = tree.append(group, absolute(2.75, 62.25, 100.0, 20.0), 0)?;
        // Deliberately smaller layout box: glyph ink can extend outside it.
        let label = tree.append(nested, absolute(1.25, 0.5, 70.0, 8.0), 0)?;
        let mut tx = Transaction::default();
        tx.opacity(group, 0.6);
        tx.opacity(nested, 0.7);
        tx.decoration(
            decorated,
            QuadDecoration {
                radius: 17.0,
                border_width: 3.0,
                border_color: 0xff8844ff,
            },
        );
        tx.image(
            image,
            Some(ImageResource::rgba8(
                [2, 2],
                vec![
                    255, 0, 0, 255, 0, 255, 0, 128, 0, 0, 255, 255, 255, 255, 255, 0,
                ],
            )?),
        );
        tx.text(
            label,
            Some(text::TextPaint {
                resource,
                color: 0xffdd88ff,
            }),
        );
        tree.commit(tx, BackendCapabilities::default())?;
        let surface = [(320.0 * scale) as u32, (200.0 * scale) as u32];
        for step in 0..10 {
            let mut tx = Transaction::default();
            match step {
                0 => {}
                1 => tx.clip(
                    group,
                    Some(Rect {
                        x: 3.5,
                        y: 7.25,
                        width: 100.0,
                        height: 75.5,
                    }),
                ),
                2 => tx.translation(group, [-62.5, -48.75]),
                3 => tx.translation(group, [500.0, 0.0]),
                4 => {
                    tx.translation(group, [0.0, 0.0]);
                    tx.clip(group, None);
                }
                5 => tx.opacity(group, 0.0),
                6 => {
                    tx.color(decorated, 0x88cc44aa);
                    tx.translation(nested, [15.5, 5.75]);
                }
                7 => tx.opacity(group, 0.6),
                8 => {
                    tx.image(image, None);
                    tx.text(label, None);
                    tx.color(decorated, 0);
                    tx.decoration(decorated, QuadDecoration::default());
                }
                _ => {
                    tx.color(decorated, 0xff8844ff);
                    tx.translation(group, [18.25, 4.5]);
                }
            }
            tree.commit(
                tx,
                BackendCapabilities {
                    solid_color_patch: true,
                },
            )?;
            let frame = tree.frame([320.0, 200.0])?;
            let submit = renderer.submit(&frame, surface, scale)?.unwrap();
            assert!(
                submit.gpu.live_layer_bytes < u64::from(surface[0]) * u64::from(surface[1]) * 8
            );
            if step == 3 || step == 8 {
                assert_eq!(submit.gpu.live_layer_bytes, 8);
            }
            compare(
                renderer,
                &frame,
                surface,
                scale,
                &format!("layer-bounds-mixed-{scale}-{step}"),
                output,
                results,
            )?;
            let result = results.last_mut().unwrap();
            result["live_layer_bytes"] = submit.gpu.live_layer_bytes.into();
            result["layer_allocations"] = submit.gpu.layer_allocations.into();
            result["bounds_items"] = submit.composition.bounds_items.into();
        }
        // Queue successive extent/origin replacements without intermediate readback.
        for step in 0..24 {
            let mut tx = Transaction::default();
            tx.translation(group, [step as f32 * 0.75, step as f32 * 0.5]);
            tree.commit(tx, BackendCapabilities::default())?;
            renderer.submit(&tree.frame([320.0, 200.0])?, surface, scale)?;
        }
        compare(
            renderer,
            &tree.frame([320.0, 200.0])?,
            surface,
            scale,
            &format!("layer-bounds-in-flight-{scale}"),
            output,
            results,
        )?;
        renderer.reset()?;
        renderer.submit(&tree.frame([320.0, 200.0])?, surface, scale)?;
        compare(
            renderer,
            &tree.frame([320.0, 200.0])?,
            surface,
            scale,
            &format!("layer-bounds-reset-{scale}"),
            output,
            results,
        )?;
    }
    // Public backend rejects malformed physical bounds before allocation.
    let mut backend = gpui_wgpu::RetainedQuadRenderer::new(1)?;
    let batch = |bounds| gpui_wgpu::QuadBatch {
        capacity: 0,
        writes: vec![],
        order: Some(vec![gpui_wgpu::QuadDraw::Layer([1, 0, 1])]),
        layers: vec![gpui_wgpu::QuadLayer {
            id: [1, 0, 1],
            opacity: 0.5,
            bounds: Some(bounds),
            draws: Arc::from([]),
        }],
        images: Default::default(),
        glyphs: Default::default(),
        shadows: Default::default(),
        spatial: Default::default(),
        cache_layers: true,
        size: gpui::size(gpui::DevicePixels(64), gpui::DevicePixels(64)),
        redraw: gpui_wgpu::QuadRedraw::Full,
    };
    for bounds in [[64, 0, 1, 1], [0, 64, 1, 1], [u32::MAX, 0, 2, 1]] {
        assert!(
            backend
                .submit(batch(bounds))
                .unwrap_err()
                .to_string()
                .contains("invalid isolated layer bounds")
        );
    }
    let mut oversized = batch([0, 0, 3840, 2160]);
    oversized.size = gpui::size(gpui::DevicePixels(3840), gpui::DevicePixels(2160));
    oversized.layers = (0..9)
        .map(|index| gpui_wgpu::QuadLayer {
            id: [1, index, 1],
            opacity: 0.5,
            bounds: Some([0, 0, 3840, 2160]),
            draws: Arc::from([]),
        })
        .collect();
    assert!(
        backend
            .submit(oversized)
            .unwrap_err()
            .to_string()
            .contains("256 MiB")
    );
    assert_eq!(backend.submit(batch([0; 4]))?.live_layer_bytes, 4);
    backend.finish()?;
    results.push(serde_json::json!({"case":"layer-bounds-malformed-preflight", "malformed_rejected":3,"tight_budget_rejected":true,"valid_retry":true}));
    Ok(())
}

pub(super) fn benchmark() -> Result<serde_json::Value, Box<dyn Error>> {
    let mut cases = vec![];
    for surface in [[1920, 1080], [3840, 2160]] {
        let viewport = [surface[0] as f32, surface[1] as f32];
        for kind in [
            "small-cached-opacity",
            "small-material-raster",
            "full-surface-control",
        ] {
            let mut tree = UiTree::new(absolute(0.0, 0.0, viewport[0], viewport[1]), 0x171d28ff)?;
            let mut groups = vec![];
            let mut leaves = vec![];
            for i in 0..if kind == "full-surface-control" { 1 } else { 8 } {
                let style = if kind == "full-surface-control" {
                    absolute(0.0, 0.0, viewport[0], viewport[1])
                } else {
                    absolute(80.0 + i as f32 * 150.0, 80.0, 96.0, 64.0)
                };
                let group = tree.append(tree.root(), style, 0)?;
                let mut tx = Transaction::default();
                tx.opacity(group, 0.6);
                tree.commit(tx, BackendCapabilities::default())?;
                for _ in 0..4 {
                    leaves.push(tree.append(
                        group,
                        if kind == "full-surface-control" {
                            absolute(0.0, 0.0, viewport[0], viewport[1])
                        } else {
                            absolute(0.0, 0.0, 96.0, 64.0)
                        },
                        0x4488cc80,
                    )?);
                }
                groups.push(group);
            }
            let mut renderer = WgpuFrameRenderer::new(1)?;
            renderer.set_policy(CompositorPolicy::Full)?;
            let mut samples: [Vec<f64>; 2] = Default::default();
            let mut cpu: [Vec<f64>; 2] = Default::default();
            let mut rounds: [Vec<Vec<f64>>; 2] = Default::default();
            let mut counters = [serde_json::Value::Null, serde_json::Value::Null];
            for round in 0..3 {
                for mode in if round % 2 == 0 { [0, 1] } else { [1, 0] } {
                    renderer.set_tight_layer_bounds_enabled(mode == 0);
                    renderer.submit(&tree.frame(viewport)?, surface, 1.0)?;
                    renderer.finish()?;
                    let mut current = vec![];
                    for step in 0..26 {
                        let mut tx = Transaction::default();
                        if kind == "small-cached-opacity" {
                            for group in &groups {
                                tx.opacity(*group, if step % 2 == 0 { 0.5 } else { 0.75 });
                            }
                        } else {
                            for leaf in &leaves {
                                tx.color(
                                    *leaf,
                                    if step % 2 == 0 {
                                        0x88cc4480
                                    } else {
                                        0x4488cc80
                                    },
                                );
                            }
                        }
                        let start = Instant::now();
                        tree.commit(
                            tx,
                            BackendCapabilities {
                                solid_color_patch: true,
                            },
                        )?;
                        let frame = tree.frame(viewport)?;
                        let submit = renderer.submit(&frame, surface, 1.0)?.unwrap();
                        let cpu_us = start.elapsed().as_secs_f64() * 1e6;
                        renderer.finish()?;
                        let complete_us = start.elapsed().as_secs_f64() * 1e6;
                        assert_eq!(submit.gpu.layer_allocations, 0);
                        assert_eq!(
                            submit.gpu.rasterized_layers,
                            if kind == "small-cached-opacity" {
                                0
                            } else {
                                groups.len()
                            }
                        );
                        if step >= 6 {
                            samples[mode].push(complete_us);
                            cpu[mode].push(cpu_us);
                            current.push(complete_us);
                        }
                        counters[mode] = serde_json::json!({"live_layer_bytes":submit.gpu.live_layer_bytes,
                            "rasterized_layer_pixels":submit.gpu.rasterized_layer_pixels,
                            "property_upload_bytes":submit.gpu.property_upload_bytes,"instance_upload_bytes":submit.gpu.uploaded_bytes,
                            "bounds_items":submit.composition.bounds_items});
                    }
                    rounds[mode].push(current);
                    let actual = renderer.image()?;
                    let reference =
                        renderer.reference_image(&tree.frame(viewport)?, surface, 1.0)?;
                    assert!(
                        actual.as_raw() == reference.as_raw(),
                        "bounded benchmark final pixels"
                    );
                }
            }
            let percentile = |values: &[f64], p: f64| {
                let mut v = values.to_vec();
                v.sort_by(f64::total_cmp);
                v[((v.len() - 1) as f64 * p).round() as usize]
            };
            let value = serde_json::json!({"surface":surface,"kind":kind,
                "tight_median_us":percentile(&samples[0],0.5),"full_median_us":percentile(&samples[1],0.5),
                "tight_p95_us":percentile(&samples[0],0.95),"full_p95_us":percentile(&samples[1],0.95),
                "tight_cpu_median_us":percentile(&cpu[0],0.5),"full_cpu_median_us":percentile(&cpu[1],0.5),
                "tight_rounds_us":rounds[0],"full_rounds_us":rounds[1],
                "tight_cpu_samples_us":cpu[0],"full_cpu_samples_us":cpu[1],
                "tight":counters[0],"full":counters[1],"pixel_equal":true});
            println!(
                "Layer bounds {surface:?} {kind}: {:.1} / {:.1} us",
                percentile(&samples[0], 0.5),
                percentile(&samples[1], 0.5)
            );
            cases.push(value);
        }
    }
    Ok(
        serde_json::json!({"scope":"same renderer; tight versus full-viewport layer textures; Full root redraw; owner commit/publication/submit plus per-frame GPU completion; excludes readback and native presentation; physical texture payload excludes driver allocation overhead and retired epochs", "max_in_flight":1,"rounds":3,"warmup_per_round":6,"samples_per_round":20,"cases":cases}),
    )
}
