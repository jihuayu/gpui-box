//! Hierarchical placement pixels, bounded deep anchors and completed owner A/B.
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
    for shared in [true, false] {
        for scale in [1.0, 1.25, 1.5] {
            let (context, mut raster) = retained_text::context()?;
            let text = retained_text::text(&context, &mut raster, "Spatial 中文", scale)?;
            let mut tree = UiTree::new(absolute(0.0, 0.0, 192.0, 144.0), 0x171d28ff)?;
            let group = tree.append(tree.root(), absolute(12.0, 12.0, 150.0, 110.0), 0)?;
            let node = tree.append(group, absolute(8.0, 8.0, 64.0, 40.0), 0x4488ccff)?;
            let image = tree.append(group, absolute(92.0, 20.0, 24.0, 24.0), 0)?;
            let label = tree.append(group, absolute(16.0, 64.0, 124.0, 32.0), 0)?;
            tree.append(group, absolute(260.0, 140.0, 8.0, 8.0), 0x884422ff)?;
            let mut tx = Transaction::default();
            tx.decoration(
                node,
                QuadDecoration {
                    radius: 8.0,
                    border_width: 2.0,
                    border_color: 0xff8844ff,
                },
            );
            tx.shadows(
                node,
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
                ],
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
            tree.commit(tx, BackendCapabilities::default())?;
            let surface = [(192.0 * scale) as u32, (144.0 * scale) as u32];
            let mut gpu = WgpuFrameRenderer::new(3)?;
            gpu.set_shared_spatial_enabled(shared);
            gpu.set_policy(CompositorPolicy::Damage)?;
            let mut old = None;
            for step in 0..12 {
                let mut tx = Transaction::default();
                match step {
                    0 => {}
                    1 => tx.translation(group, [2.5, 3.25]),
                    2 => tx.clip(
                        group,
                        Some(Rect {
                            x: 1.5,
                            y: 2.25,
                            width: 94.0,
                            height: 76.0,
                        }),
                    ),
                    3 => tx.scroll(group, [12.5, 4.25]),
                    4 => tx.opacity(group, 0.6),
                    5 => tx.translation(group, [6.25, 7.5]),
                    6 => {
                        tx.translation(group, [8.25, 9.5]);
                        tx.translation(node, [-2.0, -2.0]);
                    }
                    7 => tx.clip(
                        group,
                        Some(Rect {
                            x: 0.5,
                            y: 0.0,
                            width: 0.0,
                            height: 100.0,
                        }),
                    ),
                    8 => tx.clip(group, None),
                    9 => tx.opacity(group, 0.35),
                    10 => tx.style(node, absolute(8.0, 8.0, 72.0, 46.0)),
                    _ => {
                        tx.scroll(group, [0.0; 2]);
                        tx.translation(group, [-1.25, -2.5]);
                    }
                }
                tree.commit(tx, BackendCapabilities::default())?;
                let frame = tree.frame([192.0, 144.0])?;
                let submit = gpu.submit(&frame, surface, scale)?.unwrap();
                if shared && [1, 2, 3, 5, 7, 8, 11].contains(&step) {
                    assert_eq!(
                        (submit.gpu.uploaded_bytes, submit.gpu.shadow_upload_bytes),
                        (0, 0),
                        "shared spatial step {step}"
                    );
                    assert!(submit.gpu.spatial_upload_bytes > 0);
                }
                if shared && [1, 2, 3, 5, 7, 8].contains(&step) {
                    assert_eq!(submit.gpu.spatial_upload_bytes, 96);
                }
                if step == 5 {
                    assert_eq!(submit.gpu.rasterized_layers, 1);
                }
                if step == 9 {
                    assert_eq!(submit.gpu.rasterized_layers, 0);
                    assert_eq!(submit.gpu.property_upload_bytes, 16);
                }
                compare(
                    &mut gpu,
                    &frame,
                    surface,
                    scale,
                    &format!("spatial-{shared}-{scale}-{step}"),
                    output,
                    results,
                )?;
                let result = results.last_mut().unwrap();
                result["quad_bytes"] = submit.gpu.uploaded_bytes.into();
                result["shadow_bytes"] = submit.gpu.shadow_upload_bytes.into();
                result["spatial_bytes"] = submit.gpu.spatial_upload_bytes.into();
                if step == 0 {
                    old = Some(frame.clone());
                }
            }
            assert_eq!(old.unwrap().items[1].spatial.translation, [0.0; 2]);
            // Skip a published base, then reset and flip the transport control.
            let mut tx = Transaction::default();
            tx.translation(group, [4.5, 2.25]);
            tree.commit(tx, BackendCapabilities::default())?;
            tree.frame([192.0, 144.0])?;
            let mut tx = Transaction::default();
            tx.translation(group, [5.5, 3.25]);
            tree.commit(tx, BackendCapabilities::default())?;
            let frame = tree.frame([192.0, 144.0])?;
            assert!(gpu.submit(&frame, surface, scale)?.unwrap().full_resync);
            compare(
                &mut gpu,
                &frame,
                surface,
                scale,
                &format!("spatial-skipped-{shared}-{scale}"),
                output,
                results,
            )?;
            gpu.reset()?;
            assert_eq!(gpu.shared_spatial_enabled(), shared);
            gpu.submit(&frame, surface, scale)?;
            compare(
                &mut gpu,
                &frame,
                surface,
                scale,
                &format!("spatial-reset-{shared}-{scale}"),
                output,
                results,
            )?;
            gpu.set_shared_spatial_enabled(!shared);
            gpu.submit(&frame, surface, scale)?;
            compare(
                &mut gpu,
                &frame,
                surface,
                scale,
                &format!("spatial-control-{shared}-{scale}"),
                output,
                results,
            )?;
            tree.remove(node)?;
            gpu.submit(&tree.frame([192.0, 144.0])?, surface, scale)?;
            let next = tree.append(group, absolute(8.0, 8.0, 32.0, 24.0), 0xff8822ff)?;
            assert_ne!(next, node);
            let mut stale = Transaction::default();
            stale.translation(node, [1.0; 2]);
            assert!(tree.commit(stale, BackendCapabilities::default()).is_err());
            let frame = tree.frame([192.0, 144.0])?;
            gpu.submit(&frame, surface, scale)?;
            compare(
                &mut gpu,
                &frame,
                surface,
                scale,
                &format!("spatial-reuse-{shared}-{scale}"),
                output,
                results,
            )?;
            // Restore the initial mode after retirement while disabled; queued
            // empty-anchor clears must survive those control frames.
            gpu.set_shared_spatial_enabled(shared);
            assert!(gpu.submit(&frame, surface, scale)?.is_some());
            compare(
                &mut gpu,
                &frame,
                surface,
                scale,
                &format!("spatial-reenabled-{shared}-{scale}"),
                output,
                results,
            )?;
        }
    }
    // Multiple bounded segments: a root move updates anchors, never local quads.
    let mut tree = UiTree::new(absolute(0.0, 0.0, 128.0, 96.0), 0x000000ff)?;
    let mut parent = tree.root();
    let mut chain = vec![];
    for _ in 0..70 {
        parent = tree.append(parent, absolute(0.0, 0.0, 100.0, 80.0), 0)?;
        chain.push(parent);
    }
    let leaf = tree.append(parent, absolute(16.0, 16.0, 24.0, 16.0), 0xff0000ff)?;
    let mut gpu = WgpuFrameRenderer::new(3)?;
    gpu.submit(&tree.frame([128.0, 96.0])?, [128, 96], 1.0)?;
    let mut tx = Transaction::default();
    tx.translation(tree.root(), [3.25, 4.5]);
    tree.commit(tx, BackendCapabilities::default())?;
    let frame = tree.frame([128.0, 96.0])?;
    let submit = gpu.submit(&frame, [128, 96], 1.0)?.unwrap();
    assert_eq!(
        (submit.gpu.uploaded_bytes, submit.gpu.spatial_upload_bytes),
        (0, 3 * 96)
    );
    assert_eq!(gpu.image()?.get_pixel(25, 25).0, [255, 0, 0, 255]);
    compare(
        &mut gpu,
        &frame,
        [128, 96],
        1.0,
        "spatial-deep-anchors",
        output,
        results,
    )?;
    results.last_mut().unwrap()["spatial_bytes"] = submit.gpu.spatial_upload_bytes.into();
    assert_eq!(frame.hit_test(25.0, 25.0), Some(leaf));
    let root = tree.root();
    tree.edit(BackendCapabilities::default(), |edit| {
        edit.reparent(chain[19], root)?;
        Ok(())
    })?;
    let moved = tree.frame([128.0, 96.0])?;
    assert_eq!(
        moved.items.iter().find(|i| i.node == leaf).unwrap().bounds,
        frame.items.iter().find(|i| i.node == leaf).unwrap().bounds
    );
    let submit = gpu.submit(&moved, [128, 96], 1.0)?.unwrap();
    assert!(!submit.full_resync);
    assert_eq!(submit.gpu.uploaded_bytes, 0);
    assert!(submit.gpu.spatial_upload_bytes > 0);
    compare(
        &mut gpu,
        &moved,
        [128, 96],
        1.0,
        "spatial-deep-reparent",
        output,
        results,
    )?;
    tree.remove(chain[0])?;
    gpu.submit(&tree.frame([128.0, 96.0])?, [128, 96], 1.0)?;
    gpu.finish()?;
    let mut host = tree.root();
    for _ in 0..20 {
        host = tree.append(host, absolute(1.0, 0.0, 64.0, 64.0), 0)?;
    }
    tree.append(host, absolute(4.0, 36.0, 16.0, 12.0), 0x0000ffff)?;
    let recycled = tree.frame([128.0, 96.0])?;
    gpu.submit(&recycled, [128, 96], 1.0)?;
    compare(
        &mut gpu,
        &recycled,
        [128, 96],
        1.0,
        "spatial-depth-reuse",
        output,
        results,
    )?;
    clip_centers(output, results)?;
    backend_preflight(results)?;
    Ok(())
}

fn clip_centers(
    output: &std::path::Path,
    results: &mut Vec<serde_json::Value>,
) -> Result<(), Box<dyn Error>> {
    for shared in [true, false] {
        for cropped in [true, false] {
            for shadow in [true, false] {
                let mut tree = UiTree::new(absolute(0.0, 0.0, 64.0, 64.0), 0x000000ff)?;
                let group = tree.append(tree.root(), absolute(8.0, 8.0, 32.0, 32.0), 0)?;
                let node = tree.append(
                    group,
                    absolute(0.0, 0.0, 24.0, 16.0),
                    if shadow { 0 } else { 0xff0000ff },
                )?;
                let mut tx = Transaction::default();
                tx.opacity(group, 0.6);
                tx.clip(
                    node,
                    Some(Rect {
                        x: 0.0,
                        y: 0.5,
                        width: 24.0,
                        height: 10.0,
                    }),
                );
                if shadow {
                    tx.shadows(
                        node,
                        vec![BoxShadow {
                            color: 0xff0000ff,
                            ..Default::default()
                        }],
                    );
                }
                tree.commit(tx, BackendCapabilities::default())?;
                let frame = tree.frame([64.0; 2])?;
                let mut gpu = WgpuFrameRenderer::new(1)?;
                gpu.set_shared_spatial_enabled(shared);
                gpu.set_tight_layer_bounds_enabled(cropped);
                gpu.submit(&frame, [64; 2], 1.0)?;
                let image = gpu.image()?;
                assert_eq!(
                    image.get_pixel(16, 8).0,
                    [153, 0, 0, 255],
                    "inclusive top center"
                );
                assert_eq!(
                    image.get_pixel(16, 18).0,
                    [153, 0, 0, 255],
                    "inclusive bottom center"
                );
                assert_eq!(image.get_pixel(16, 7).0, [0, 0, 0, 255]);
                assert_eq!(image.get_pixel(16, 19).0, [0, 0, 0, 255]);
                compare(
                    &mut gpu,
                    &frame,
                    [64; 2],
                    1.0,
                    &format!("spatial-clip-centers-{shared}-{cropped}-{shadow}"),
                    output,
                    results,
                )?;
                results.last_mut().unwrap()["analytical_pixels"] = true.into();
            }
        }
    }
    Ok(())
}

fn backend_preflight(results: &mut Vec<serde_json::Value>) -> Result<(), Box<dyn Error>> {
    let node = gpui_wgpu::SpatialNode {
        anchor_origin: [8.0, 8.0, 0.0, 0.0],
        anchor_clip: [0.0, 0.0, 64.0, 64.0],
        links: [u32::MAX, 0, 10, 0],
        ..Default::default()
    };
    let quad = gpui::Quad {
        order: 1,
        bounds: gpui::Bounds {
            origin: gpui::point(gpui::ScaledPixels(0.0), gpui::ScaledPixels(0.0)),
            size: gpui::size(gpui::ScaledPixels(16.0), gpui::ScaledPixels(16.0)),
        },
        background: gpui::rgba(0xff0000ff).into(),
        ..Default::default()
    };
    let batch = |nodes, quads| gpui_wgpu::QuadBatch {
        capacity: 1,
        writes: quads,
        order: Some(vec![gpui_wgpu::QuadDraw::Instances(0..1)]),
        layers: vec![],
        images: Default::default(),
        glyphs: Default::default(),
        shadows: Default::default(),
        spatial: gpui_wgpu::SpatialBatch {
            enabled: true,
            capacity: 1,
            scale: 1.0,
            writes: nodes,
        },
        cache_layers: true,
        size: gpui::size(gpui::DevicePixels(64), gpui::DevicePixels(64)),
        redraw: gpui_wgpu::QuadRedraw::Full,
    };
    let mut untouched = gpui_wgpu::RetainedQuadRenderer::new(1)?;
    untouched.submit(batch(
        vec![],
        vec![gpui_wgpu::QuadWrite {
            start: 0,
            values: vec![quad],
        }],
    ))?;
    assert_eq!(
        untouched.image()?.get_pixel(12, 12).0,
        [0, 0, 0, 255],
        "unwritten spatial slots must be empty anchors"
    );
    let mut gpu = gpui_wgpu::RetainedQuadRenderer::new(3)?;
    gpu.submit(batch(
        vec![gpui_wgpu::InstanceWrite {
            start: 0,
            values: vec![node],
        }],
        vec![gpui_wgpu::QuadWrite {
            start: 0,
            values: vec![quad],
        }],
    ))?;
    let before = gpu.image()?;
    assert_eq!(before.get_pixel(12, 12).0, [255, 0, 0, 255]);
    for kind in 0..7 {
        let mut invalid = node;
        match kind {
            0 => invalid.links = [9, 1, 0, 0],
            1 => invalid.links = [0, 1, 0, 0],
            2 => invalid.links[1] = 32,
            3 => invalid.clip[0] = f32::NAN,
            5 => invalid.anchor_origin[0] = f32::MAX,
            _ => {}
        }
        let mut input = batch(
            vec![gpui_wgpu::InstanceWrite {
                start: 0,
                values: vec![invalid],
            }],
            vec![],
        );
        if kind == 4 {
            input.writes.push(gpui_wgpu::QuadWrite {
                start: 0,
                values: vec![gpui::Quad { order: 2, ..quad }],
            });
        }
        if kind == 5 {
            input.spatial.scale = 2.0;
        }
        if kind == 6 {
            input.spatial.writes[0].start = 1;
        }
        assert!(gpu.submit(input).is_err(), "malformed spatial kind {kind}");
        assert_eq!(gpu.last_serial(), 1);
    }
    let retry = gpu.submit(batch(vec![], vec![]))?;
    assert_eq!(
        (
            retry.serial,
            retry.uploaded_bytes,
            retry.spatial_upload_bytes
        ),
        (2, 0, 0)
    );
    assert_eq!(before.as_raw(), gpu.image()?.as_raw());
    results.push(serde_json::json!({"case":"spatial-backend-preflight","malformed_rejected":7,"valid_retry_serial":2,"unchanged_pixels":true,"analytical_red_pixel":true,"unwritten_slots_empty":true}));
    Ok(())
}

pub(super) fn benchmark() -> Result<serde_json::Value, Box<dyn Error>> {
    let mut cases = vec![];
    for count in [1000, 10000] {
        let mut controls = vec![];
        for shared in [true, false] {
            let mut tree = UiTree::new(absolute(0.0, 0.0, 800.0, 600.0), 0x171d28ff)?;
            let parent = tree.append(tree.root(), absolute(0.0, 0.0, 800.0, 600.0), 0)?;
            for i in 0..count {
                tree.append(
                    parent,
                    absolute((i % 100) as f32 * 8.0, (i / 100) as f32 * 6.0, 4.0, 3.0),
                    0x4488ccff,
                )?;
            }
            let mut gpu = WgpuFrameRenderer::new(1)?;
            gpu.set_shared_spatial_enabled(shared);
            gpu.submit(&tree.frame([800.0, 600.0])?, [800, 600], 1.0)?;
            gpu.finish()?;
            controls.push((tree, parent, gpu));
        }
        let mut cpu: [Vec<f64>; 2] = Default::default();
        let mut completed: [Vec<f64>; 2] = Default::default();
        let mut bytes = [[0; 2]; 2];
        for round in 0..3 {
            for mode in if round % 2 == 0 { [0, 1] } else { [1, 0] } {
                let (tree, parent, gpu) = &mut controls[mode];
                for step in 0..26 {
                    let start = Instant::now();
                    let mut tx = Transaction::default();
                    tx.translation(
                        *parent,
                        if step % 2 == 0 {
                            [1.25, 2.5]
                        } else {
                            [3.25, 4.5]
                        },
                    );
                    tree.commit(tx, BackendCapabilities::default())?;
                    let frame = tree.frame([800.0, 600.0])?;
                    let submit = gpu.submit(&frame, [800, 600], 1.0)?.unwrap();
                    let cpu_us = start.elapsed().as_secs_f64() * 1e6;
                    gpu.finish()?;
                    let completed_us = start.elapsed().as_secs_f64() * 1e6;
                    if mode == 0 {
                        assert_eq!(
                            (submit.gpu.uploaded_bytes, submit.gpu.spatial_upload_bytes),
                            (0, 96)
                        );
                    } else {
                        assert_eq!(submit.gpu.uploaded_bytes, (count as u64 + 1) * 160);
                        assert_eq!(submit.gpu.spatial_upload_bytes, 0);
                    }
                    if step >= 6 {
                        cpu[mode].push(cpu_us);
                        completed[mode].push(completed_us);
                        bytes[mode] = [submit.gpu.uploaded_bytes, submit.gpu.spatial_upload_bytes];
                    }
                }
                let frame = tree.frame([800.0, 600.0])?;
                let actual = gpu.image()?;
                let reference = gpu.reference_image(&frame, [800, 600], 1.0)?;
                assert_eq!(
                    actual.as_raw(),
                    reference.as_raw(),
                    "spatial owner benchmark reference"
                );
            }
        }
        let percentile = |values: &[f64], p: f64| {
            let mut s = values.to_vec();
            s.sort_by(f64::total_cmp);
            s[((s.len() - 1) as f64 * p).round() as usize]
        };
        let value = serde_json::json!({"count":count,"shared_bytes":bytes[0],"world_bytes":bytes[1],"shared_cpu_us":cpu[0],"world_cpu_us":cpu[1],"shared_completed_us":completed[0],"world_completed_us":completed[1],"shared_median_us":percentile(&completed[0],0.5),"world_median_us":percentile(&completed[1],0.5),"shared_p95_us":percentile(&completed[0],0.95),"world_p95_us":percentile(&completed[1],0.95),"pixel_equal":true});
        println!(
            "Spatial owner {count}: {} / {} us",
            value["shared_median_us"], value["world_median_us"]
        );
        cases.push(value);
    }
    Ok(
        serde_json::json!({"scope":"owner parent translation through commit/publication/submission and per-frame GPU completion; shared hierarchy versus original world-instance geometry; same tree/local geometry/full root redraw; excludes setup/readback/native presentation; CPU subtree extraction and allocation scans remain included", "rounds":3,"warmup":6,"samples_per_round":20,"max_in_flight":1,"cases":cases}),
    )
}
