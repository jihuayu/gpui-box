//! Persistent outer/inset shadows, full GPUI oracle and typed transport control.
use super::*;
use std::time::Instant;
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
fn shadows() -> Vec<BoxShadow> {
    vec![
        BoxShadow {
            color: 0x55cc99aa,
            offset: [5.0, 4.0],
            blur_radius: 3.0,
            spread_radius: 1.5,
            inset: false,
        },
        BoxShadow {
            color: 0x00000088,
            offset: [2.0, 1.0],
            blur_radius: 1.0,
            spread_radius: 3.0,
            inset: true,
        },
    ]
}
pub(super) fn run(
    renderer: &mut WgpuFrameRenderer,
    output: &std::path::Path,
    results: &mut Vec<serde_json::Value>,
) -> Result<(), Box<dyn Error>> {
    renderer.set_policy(CompositorPolicy::Damage)?;
    for scale in [1.0, 1.25, 1.5] {
        let (context, mut raster) = retained_text::context()?;
        let text = retained_text::text(&context, &mut raster, "Shadow 中文", scale)?;
        let mut tree = UiTree::new(absolute(0.0, 0.0, 192.0, 144.0), 0x171d28ff)?;
        let group = tree.append(tree.root(), absolute(12.0, 12.0, 160.0, 120.0), 0)?;
        let node = tree.append(group, absolute(12.0, 12.0, 64.0, 40.0), 0x4488ccff)?;
        let image = tree.append(group, absolute(88.0, 20.0, 24.0, 24.0), 0)?;
        let label = tree.append(group, absolute(16.0, 76.0, 124.0, 32.0), 0)?;
        let mut tx = Transaction::default();
        tx.shadows(node, shadows());
        tx.decoration(
            node,
            QuadDecoration {
                radius: 8.0,
                border_width: 2.0,
                border_color: 0xff8844ff,
            },
        );
        tx.image(
            image,
            Some(ImageResource::rgba8([1, 1], vec![255, 128, 64, 128])?),
        );
        tx.text(
            label,
            Some(text::TextPaint {
                resource: text,
                color: 0xffdd88ff,
            }),
        );
        tree.commit(
            tx,
            BackendCapabilities {
                solid_color_patch: true,
            },
        )?;
        let surface = [(192.0 * scale) as u32, (144.0 * scale) as u32];
        let mut saved = None;
        for step in 0..12 {
            let mut tx = Transaction::default();
            match step {
                0 => {}
                1 => {
                    let mut values = shadows();
                    values[0].color = 0xee4455aa;
                    tx.shadows(node, values);
                }
                2 => tx.color(node, 0x9966ddff),
                3 => tx.translation(node, [2.5, 3.25]),
                4 => tx.clip(
                    group,
                    Some(Rect {
                        x: 0.0,
                        y: 0.0,
                        width: 92.0,
                        height: 65.0,
                    }),
                ),
                5 => tx.opacity(group, 0.6),
                6 => {
                    let mut values = shadows();
                    values[0].spread_radius = -3.0;
                    tx.shadows(node, values);
                }
                7 => {
                    let mut values = shadows();
                    values[0].inset = true;
                    values[1].inset = false;
                    tx.shadows(node, values);
                }
                8 => tx.shadows(
                    node,
                    (0..8)
                        .map(|i| BoxShadow {
                            offset: [i as f32 - 3.0, 2.0],
                            blur_radius: 1.0,
                            color: 0xff880022,
                            ..Default::default()
                        })
                        .collect(),
                ),
                9 => tx.shadows(node, vec![shadows()[0]]),
                10 => tx.shadows(node, vec![]),
                _ => {
                    tx.shadows(node, shadows());
                    tx.clip(group, None);
                }
            }
            tree.commit(
                tx,
                BackendCapabilities {
                    solid_color_patch: true,
                },
            )?;
            let frame = tree.frame([192.0, 144.0])?;
            let submit = renderer.submit(&frame, surface, scale)?.unwrap();
            if step == 1 {
                assert_eq!(
                    (
                        submit.gpu.uploaded_bytes,
                        submit.gpu.shadow_upload_bytes,
                        submit.gpu.shadow_upload_ranges
                    ),
                    (0, 112, 1)
                );
                assert!(!submit.composition.rebuilt);
            }
            if step == 2 {
                assert_eq!(
                    (submit.gpu.uploaded_bytes, submit.gpu.shadow_upload_bytes),
                    (160, 0)
                );
            }
            if step == 7 {
                assert!(submit.composition.rebuilt);
            }
            if step == 8 {
                assert!(submit.gpu.shadow_upload_bytes >= 8 * 112);
                saved = Some(frame.clone());
            }
            assert_eq!(
                saved
                    .as_ref()
                    .map(|f| f.items.iter().map(|item| item.shadows.len()).sum::<usize>())
                    .unwrap_or(8),
                8
            );
            compare(
                renderer,
                &frame,
                surface,
                scale,
                &format!("shadows-{scale}-{step}"),
                output,
                results,
            )?;
            let result = results.last_mut().unwrap();
            result["shadow_upload_bytes"] = submit.gpu.shadow_upload_bytes.into();
            result["shadow_upload_ranges"] = submit.gpu.shadow_upload_ranges.into();
            result["shadow_generation"] = submit.gpu.shadow_buffer_generation.into();
        }
        // Miss an intermediate packet: recover from the complete owned shadow snapshot.
        let mut tx = Transaction::default();
        tx.shadows(node, vec![shadows()[1]]);
        tree.commit(tx, BackendCapabilities::default())?;
        tree.frame([192.0, 144.0])?;
        let mut tx = Transaction::default();
        tx.translation(group, [7.0, 4.0]);
        tree.commit(tx, BackendCapabilities::default())?;
        let frame = tree.frame([192.0, 144.0])?;
        assert!(
            renderer
                .submit(&frame, surface, scale)?
                .unwrap()
                .full_resync
        );
        compare(
            renderer,
            &frame,
            surface,
            scale,
            &format!("shadows-skipped-{scale}"),
            output,
            results,
        )?;
        renderer.reset()?;
        renderer.submit(&frame, surface, scale)?;
        compare(
            renderer,
            &frame,
            surface,
            scale,
            &format!("shadows-reset-{scale}"),
            output,
            results,
        )?;
        tree.remove(node)?;
        let frame = tree.frame([192.0, 144.0])?;
        renderer.submit(&frame, surface, scale)?;
        compare(
            renderer,
            &frame,
            surface,
            scale,
            &format!("shadows-remove-{scale}"),
            output,
            results,
        )?;
        let fresh = tree.append(group, absolute(8.0, 4.0, 32.0, 24.0), 0)?;
        let mut tx = Transaction::default();
        tx.shadows(fresh, shadows());
        tree.commit(tx, BackendCapabilities::default())?;
        let frame = tree.frame([192.0, 144.0])?;
        renderer.submit(&frame, surface, scale)?;
        compare(
            renderer,
            &frame,
            surface,
            scale,
            &format!("shadows-generation-{scale}"),
            output,
            results,
        )?;
    }
    // Independent hard-shadow phase/coverage oracle.
    let mut tree = UiTree::new(absolute(0.0, 0.0, 128.0, 96.0), 0x000000ff)?;
    let node = tree.append(tree.root(), absolute(16.0, 16.0, 24.0, 16.0), 0x0000ffff)?;
    let mut tx = Transaction::default();
    tx.shadows(
        node,
        vec![
            BoxShadow {
                color: 0xff0000ff,
                offset: [8.0, 0.0],
                ..Default::default()
            },
            BoxShadow {
                color: 0x00ff00ff,
                spread_radius: 4.0,
                inset: true,
                ..Default::default()
            },
        ],
    );
    tree.commit(tx, BackendCapabilities::default())?;
    let frame = tree.frame([128.0, 96.0])?;
    renderer.submit(&frame, [128, 96], 1.0)?;
    let image = renderer.image()?;
    assert_eq!(image.get_pixel(17, 17).0, [0, 255, 0, 255]);
    assert_eq!(image.get_pixel(28, 24).0, [0, 0, 255, 255]);
    assert_eq!(image.get_pixel(45, 24).0, [255, 0, 0, 255]);
    compare(
        renderer,
        &frame,
        [128, 96],
        1.0,
        "shadows-analytical-phases",
        output,
        results,
    )?;
    results.last_mut().unwrap()["analytical_pixels"] = true.into();
    // The core body is clipped out, but its Gaussian tail still intersects the mask.
    let mut tx = Transaction::default();
    tx.color(node, 0);
    tx.shadows(
        node,
        vec![BoxShadow {
            color: 0xffffffff,
            blur_radius: 4.0,
            ..Default::default()
        }],
    );
    tx.clip(
        node,
        Some(Rect {
            x: -10.0,
            y: 0.0,
            width: 5.0,
            height: 16.0,
        }),
    );
    tree.commit(tx, BackendCapabilities::default())?;
    let frame = tree.frame([128.0, 96.0])?;
    renderer.submit(&frame, [128, 96], 1.0)?;
    assert!(renderer.image()?.get_pixel(10, 24).0[0] > 0);
    compare(
        renderer,
        &frame,
        [128, 96],
        1.0,
        "shadows-tail-only-clip",
        output,
        results,
    )?;
    let mut tx = Transaction::default();
    tx.clip(node, None);
    tx.color(node, 0x0000ffff);
    tx.shadows(
        node,
        vec![BoxShadow {
            color: 0x00ff00ff,
            spread_radius: 100.0,
            inset: true,
            ..Default::default()
        }],
    );
    tree.commit(tx, BackendCapabilities::default())?;
    let frame = tree.frame([128.0, 96.0])?;
    renderer.submit(&frame, [128, 96], 1.0)?;
    assert_eq!(renderer.image()?.get_pixel(28, 24).0, [0, 255, 0, 255]);
    compare(
        renderer,
        &frame,
        [128, 96],
        1.0,
        "shadows-collapsed-inset",
        output,
        results,
    )?;
    // Empty masks at a device-pixel center must not leave one-pixel lines.
    for kind in ["quad", "drop", "inset"] {
        for axis in ["width", "height"] {
            let mut tx = Transaction::default();
            tx.style(node, absolute(16.0, 16.0, 16.0, 16.0));
            tx.shadows(
                node,
                if kind == "quad" {
                    vec![]
                } else {
                    vec![BoxShadow {
                        color: 0x00ff00ff,
                        spread_radius: 100.0,
                        inset: kind == "inset",
                        ..Default::default()
                    }]
                },
            );
            tx.clip(
                node,
                Some(Rect {
                    x: if axis == "width" { 0.5 } else { 0.0 },
                    y: if axis == "height" { 0.5 } else { 0.0 },
                    width: if axis == "width" { 0.0 } else { 16.0 },
                    height: if axis == "height" { 0.0 } else { 16.0 },
                }),
            );
            tree.commit(tx, BackendCapabilities::default())?;
            let frame = tree.frame([128.0, 96.0])?;
            renderer.submit(&frame, [128, 96], 1.0)?;
            assert_eq!(
                renderer.image()?.get_pixel(16, 16).0,
                [0, 0, 0, 255],
                "empty fractional {kind} clip {axis} must not paint a line"
            );
            compare(
                renderer,
                &frame,
                [128, 96],
                1.0,
                &format!("shadows-empty-{kind}-clip-{axis}"),
                output,
                results,
            )?;
        }
    }
    // Adjacent typed records merge into one upload; growth retains old buffers.
    let mut bulk = UiTree::new(absolute(0.0, 0.0, 128.0, 128.0), 0x000000ff)?;
    let mut nodes = vec![];
    for index in 0..32 {
        nodes.push(bulk.append(
            bulk.root(),
            absolute(
                (index % 8) as f32 * 14.0,
                (index / 8) as f32 * 14.0,
                8.0,
                8.0,
            ),
            0,
        )?);
    }
    let mut tx = Transaction::default();
    for node in &nodes {
        tx.shadows(
            *node,
            vec![BoxShadow {
                color: 0x4488ccaa,
                blur_radius: 1.0,
                ..Default::default()
            }],
        );
    }
    bulk.commit(tx, BackendCapabilities::default())?;
    renderer.submit(&bulk.frame([128.0; 2])?, [128; 2], 1.0)?;
    let mut tx = Transaction::default();
    for node in &nodes {
        tx.shadows(
            *node,
            vec![BoxShadow {
                color: 0xee4455aa,
                blur_radius: 1.0,
                ..Default::default()
            }],
        );
    }
    bulk.commit(tx, BackendCapabilities::default())?;
    let frame = bulk.frame([128.0; 2])?;
    let submit = renderer.submit(&frame, [128; 2], 1.0)?.unwrap();
    assert_eq!(
        (
            submit.gpu.uploaded_bytes,
            submit.gpu.shadow_upload_bytes,
            submit.gpu.shadow_upload_ranges
        ),
        (0, 32 * 112, 1)
    );
    compare(
        renderer,
        &frame,
        [128; 2],
        1.0,
        "shadows-batch-one-range",
        output,
        results,
    )?;
    for index in 32..64 {
        let node = bulk.append(
            bulk.root(),
            absolute(
                (index % 8) as f32 * 14.0,
                (index / 8) as f32 * 14.0,
                8.0,
                8.0,
            ),
            0,
        )?;
        let mut tx = Transaction::default();
        tx.shadows(
            node,
            vec![BoxShadow {
                color: 0x44cc88aa,
                blur_radius: 1.0,
                ..Default::default()
            }],
        );
        bulk.commit(tx, BackendCapabilities::default())?;
    }
    let frame = bulk.frame([128.0; 2])?;
    renderer.submit(&frame, [128; 2], 1.0)?;
    compare(
        renderer,
        &frame,
        [128; 2],
        1.0,
        "shadows-batch-growth",
        output,
        results,
    )?;
    // Grow both typed buffers with an older submission still completion-held.
    let mut epochs = UiTree::new(absolute(0.0, 0.0, 512.0, 384.0), 0x000000ff)?;
    let mut gpu = WgpuFrameRenderer::new(3)?;
    for _ in 0..64 {
        let node = epochs.append(epochs.root(), absolute(16.0, 16.0, 480.0, 352.0), 0)?;
        let mut tx = Transaction::default();
        tx.shadows(
            node,
            vec![BoxShadow {
                color: 0x4488cc66,
                blur_radius: 4.0,
                ..Default::default()
            }],
        );
        epochs.commit(tx, BackendCapabilities::default())?;
    }
    let old = epochs.frame([512.0, 384.0])?;
    let first = gpu.submit(&old, [512, 384], 1.0)?.unwrap();
    for _ in 0..64 {
        let node = epochs.append(epochs.root(), absolute(16.0, 16.0, 480.0, 352.0), 0)?;
        let mut tx = Transaction::default();
        tx.shadows(
            node,
            vec![BoxShadow {
                color: 0x44cc8866,
                blur_radius: 4.0,
                ..Default::default()
            }],
        );
        epochs.commit(tx, BackendCapabilities::default())?;
    }
    let frame = epochs.frame([512.0, 384.0])?;
    let grown = gpu.submit(&frame, [512, 384], 1.0)?.unwrap();
    assert!(grown.gpu.shadow_buffer_generation > first.gpu.shadow_buffer_generation);
    assert!(grown.gpu.buffer_generation > first.gpu.buffer_generation);
    assert!(
        grown.gpu.in_flight > 1,
        "shadow growth workload must retain a prior submission lease"
    );
    assert_eq!(
        old.items
            .iter()
            .map(|item| item.shadows.len())
            .sum::<usize>(),
        64
    );
    compare(
        &mut gpu,
        &frame,
        [512, 384],
        1.0,
        "shadows-in-flight-buffer-growth",
        output,
        results,
    )?;
    let result = results.last_mut().unwrap();
    result["in_flight_at_growth"] = grown.gpu.in_flight.into();
    result["completed_at_growth"] = grown.gpu.completed.into();
    result["prior_serial"] = first.gpu.serial.into();
    result["shadow_generation_before"] = first.gpu.shadow_buffer_generation.into();
    result["shadow_generation_after"] = grown.gpu.shadow_buffer_generation.into();
    let mut backend = gpui_wgpu::RetainedQuadRenderer::new(3)?;
    let batch = |values| gpui_wgpu::QuadBatch {
        capacity: 0,
        writes: vec![],
        order: Some(vec![gpui_wgpu::QuadDraw::Shadows(0..1)]),
        layers: vec![],
        images: Default::default(),
        glyphs: Default::default(),
        shadows: gpui_wgpu::ShadowBatch {
            capacity: 1,
            writes: values,
        },
        cache_layers: true,
        size: gpui::size(gpui::DevicePixels(64), gpui::DevicePixels(64)),
        redraw: gpui_wgpu::QuadRedraw::Full,
    };
    backend.submit(batch(vec![gpui_wgpu::ShadowWrite {
        start: 0,
        values: vec![raw_shadow(0, 0xff0000ff)],
    }]))?;
    let before = backend.image()?;
    for kind in 0..5 {
        let mut shadow = raw_shadow(0, 0x00ff00ff);
        match kind {
            0 => shadow.blur_radius = gpui::ScaledPixels(-1.0),
            1 => shadow.bounds.size.width = gpui::ScaledPixels(-1.0),
            2 => shadow.color.a = f32::NAN,
            3 => shadow.corner_radii.top_left = gpui::ScaledPixels(f32::NAN),
            _ => shadow.blur_radius = gpui::ScaledPixels(f32::MAX / 4.0),
        }
        assert!(
            backend
                .submit(batch(vec![gpui_wgpu::ShadowWrite {
                    start: 0,
                    values: vec![shadow]
                }]))
                .is_err()
        );
        assert_eq!(backend.last_serial(), 1);
    }
    let mut invalid = batch(vec![]);
    invalid.order = Some(vec![gpui_wgpu::QuadDraw::Shadows(0..2)]);
    assert!(backend.submit(invalid).is_err());
    assert_eq!(backend.last_serial(), 1);
    let retry = backend.submit(batch(vec![]))?;
    assert_eq!((retry.serial, retry.shadow_upload_bytes), (2, 0));
    assert!(
        before.as_raw() == backend.image()?.as_raw(),
        "invalid shadow preflight must preserve old pixels and storage"
    );
    results.push(serde_json::json!({"case":"shadows-backend-preflight","malformed_rejected":6,"valid_retry_serial":2,"unchanged_pixels":true}));
    Ok(())
}
fn raw_shadow(index: usize, color: u32) -> gpui::Shadow {
    let bounds = gpui::Bounds {
        origin: gpui::point(
            gpui::ScaledPixels((index % 100) as f32 * 8.0),
            gpui::ScaledPixels((index / 100) as f32 * 6.0),
        ),
        size: gpui::size(gpui::ScaledPixels(2.0), gpui::ScaledPixels(2.0)),
    };
    gpui::Shadow {
        order: 0,
        blur_radius: gpui::ScaledPixels(1.0),
        bounds,
        corner_radii: gpui::Corners::all(gpui::ScaledPixels(0.0)),
        content_mask: gpui::ContentMask {
            bounds: gpui::Bounds {
                origin: Default::default(),
                size: gpui::size(gpui::ScaledPixels(800.0), gpui::ScaledPixels(600.0)),
            },
        },
        color: gpui::rgba(color).into(),
        element_bounds: bounds,
        element_corner_radii: gpui::Corners::all(gpui::ScaledPixels(0.0)),
        inset: 0,
        pad: 0,
    }
}
pub(super) fn benchmark() -> Result<serde_json::Value, Box<dyn Error>> {
    let mut cases = vec![];
    for count in [1000, 10000] {
        let mut backend = gpui_wgpu::RetainedQuadRenderer::new(1)?;
        let mut values = (0..count)
            .map(|i| raw_shadow(i, 0x4488ccaa))
            .collect::<Vec<_>>();
        let batch = |writes| gpui_wgpu::QuadBatch {
            capacity: 0,
            writes: vec![],
            order: Some(vec![gpui_wgpu::QuadDraw::Shadows(0..count as u32)]),
            layers: vec![],
            images: Default::default(),
            glyphs: Default::default(),
            shadows: gpui_wgpu::ShadowBatch {
                capacity: count as u32,
                writes,
            },
            cache_layers: true,
            size: gpui::size(gpui::DevicePixels(800), gpui::DevicePixels(600)),
            redraw: gpui_wgpu::QuadRedraw::Full,
        };
        backend.submit(batch(vec![gpui_wgpu::ShadowWrite {
            start: 0,
            values: values.clone(),
        }]))?;
        backend.finish()?;
        let mut cpu: [Vec<f64>; 2] = Default::default();
        let mut complete: [Vec<f64>; 2] = Default::default();
        let mut payload = [0; 2];
        for round in 0..3 {
            for mode in if round % 2 == 0 { [0, 1] } else { [1, 0] } {
                backend.set_full_instance_uploads_enabled(mode == 1);
                for step in 0..26 {
                    values[count / 2] = raw_shadow(
                        count / 2,
                        if step % 2 == 0 {
                            0xee4455aa
                        } else {
                            0x4488ccaa
                        },
                    );
                    let input = batch(vec![gpui_wgpu::ShadowWrite {
                        start: (count / 2) as u32,
                        values: vec![values[count / 2]],
                    }]);
                    let start = Instant::now();
                    let submit = backend.submit(input)?;
                    let us = start.elapsed().as_secs_f64() * 1e6;
                    backend.finish()?;
                    let done = start.elapsed().as_secs_f64() * 1e6;
                    if mode == 0 {
                        assert_eq!(
                            (submit.shadow_upload_bytes, submit.shadow_upload_ranges),
                            (112, 1)
                        );
                    } else {
                        assert_eq!(submit.shadow_upload_bytes, count as u64 * 112);
                    }
                    if step >= 6 {
                        cpu[mode].push(us);
                        complete[mode].push(done);
                        payload[mode] = submit.shadow_upload_bytes;
                    }
                }
                let actual = backend.image()?;
                let mut scene = gpui::Scene::default();
                for shadow in &values {
                    scene.insert_primitive(*shadow);
                }
                scene.finish();
                let reference = backend.reference_image(
                    &scene,
                    gpui::size(gpui::DevicePixels(800), gpui::DevicePixels(600)),
                )?;
                assert!(
                    actual.as_raw() == reference.as_raw(),
                    "shadow transport benchmark pixels"
                );
            }
        }
        let percentile = |samples: &[f64], p: f64| {
            let mut s = samples.to_vec();
            s.sort_by(f64::total_cmp);
            s[((s.len() - 1) as f64 * p).round() as usize]
        };
        let value = serde_json::json!({"count":count,"delta_shadow_bytes":payload[0],"full_shadow_bytes":payload[1],
            "delta_median_us":percentile(&complete[0],0.5),"full_median_us":percentile(&complete[1],0.5),
            "delta_p95_us":percentile(&complete[0],0.95),"full_p95_us":percentile(&complete[1],0.95),
            "delta_cpu_us":cpu[0],"full_cpu_us":cpu[1],"delta_completed_us":complete[0],"full_completed_us":complete[1],"pixel_equal":true});
        println!(
            "Shadow transport {count}: {} / {} us; {} / {} bytes",
            value["delta_median_us"], value["full_median_us"], payload[0], payload[1]
        );
        cases.push(value);
    }
    Ok(
        serde_json::json!({"scope":"backend typed shadow transport microbenchmark; same shader/draws/root redraw and persistent CPU mirror; one shadow color changes per frame; delta versus full declared instance-range upload, includes validation/submit and per-frame GPU completion; excludes UI/Taffy/paint recording, readback and native presentation; geometric spare slots and unused quad placeholder are excluded from full uploads", "rounds":3,"warmup":6,"samples_per_round":20,"max_in_flight":1,"cases":cases}),
    )
}
