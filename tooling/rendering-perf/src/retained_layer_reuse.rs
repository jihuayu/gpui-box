//! Queue-ordered same-extent texture/camera reuse and replacement control.
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
fn translate(tree: &mut UiTree, group: NodeId, step: usize) -> Result<(), Box<dyn Error>> {
    let mut tx = Transaction::default();
    tx.translation(
        group,
        if step.is_multiple_of(2) {
            [8.0, 8.0]
        } else {
            [0.0, 0.0]
        },
    );
    tree.commit(tx, BackendCapabilities::default())?;
    Ok(())
}
fn fixture(quads: usize) -> Result<(UiTree, NodeId), Box<dyn Error>> {
    let mut tree = UiTree::new(absolute(0.0, 0.0, 1400.0, 1000.0), 0x171d28ff)?;
    let group = tree.append(tree.root(), absolute(32.0, 32.0, 1200.0, 900.0), 0)?;
    for _ in 0..quads {
        tree.append(group, absolute(0.0, 0.0, 1200.0, 900.0), 0x4488cc40)?;
    }
    let mut tx = Transaction::default();
    tx.opacity(group, 0.6);
    tree.commit(tx, BackendCapabilities::default())?;
    Ok((tree, group))
}
pub(super) fn run(
    output: &std::path::Path,
    results: &mut Vec<serde_json::Value>,
) -> Result<(), Box<dyn Error>> {
    for scale in [1.0, 1.5] {
        let (context, mut raster) = retained_text::context()?;
        let resource = retained_text::text(&context, &mut raster, "Move 中文", scale)?;
        let mut tree = UiTree::new(absolute(0.0, 0.0, 320.0, 220.0), 0x171d28ff)?;
        let group = tree.append(tree.root(), absolute(30.0, 30.0, 180.0, 150.0), 0)?;
        let red = tree.append(group, absolute(0.0, 0.0, 120.0, 90.0), 0x8844cc80)?;
        let nested = tree.append(group, absolute(32.0, 18.0, 60.0, 48.0), 0)?;
        let image = tree.append(nested, absolute(0.0, 0.0, 60.0, 48.0), 0)?;
        let label = tree.append(group, absolute(0.0, 94.0, 160.0, 32.0), 0)?;
        let mut tx = Transaction::default();
        tx.opacity(group, 0.6);
        tx.opacity(nested, 0.7);
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
        tx.decoration(
            red,
            QuadDecoration {
                radius: 16.0,
                border_width: 3.0,
                border_color: 0xff8844ff,
            },
        );
        tree.commit(tx, BackendCapabilities::default())?;
        let surface = [(320.0 * scale) as u32, (220.0 * scale) as u32];
        let mut renderer = WgpuFrameRenderer::new(3)?;
        let first = renderer
            .submit(&tree.frame([320.0, 220.0])?, surface, scale)?
            .unwrap();
        let bytes = first.gpu.live_layer_bytes;
        renderer.set_layer_budget_bytes(bytes)?;
        renderer.submit(&tree.frame([320.0, 220.0])?, surface, scale)?;
        for step in 0..12 {
            translate(&mut tree, group, step)?;
            let frame = tree.frame([320.0, 220.0])?;
            let submit = renderer.submit(&frame, surface, scale)?.unwrap();
            assert_eq!(
                (
                    submit.gpu.layer_allocations,
                    submit.gpu.rebased_layer_textures,
                    submit.gpu.layer_target_bytes
                ),
                (0, 2, 96)
            );
            assert_eq!(submit.gpu.layer_budget_waits, 0);
            assert_eq!(submit.gpu.layer_memory.peak_resident_bytes, bytes);
            assert_eq!(submit.gpu.property_upload_bytes, 0);
            assert_eq!(submit.gpu.image_upload_bytes, 0);
            assert_eq!(submit.gpu.glyph_atlas_bytes, 0);
            if step == 0 || step == 11 {
                compare(
                    &mut renderer,
                    &frame,
                    surface,
                    scale,
                    &format!("layer-reuse-mixed-{scale}-{step}"),
                    output,
                    results,
                )?;
            }
        }
        // Extent changes still replace allocations; parent/child origins remain independent.
        let mut tx = Transaction::default();
        tx.clip(
            group,
            Some(Rect {
                x: 0.0,
                y: 0.0,
                width: 50.0,
                height: 50.0,
            }),
        );
        tree.commit(tx, BackendCapabilities::default())?;
        let frame = tree.frame([320.0, 220.0])?;
        let clipped = renderer.submit(&frame, surface, scale)?.unwrap();
        assert!(clipped.gpu.layer_allocations > 0);
        compare(
            &mut renderer,
            &frame,
            surface,
            scale,
            &format!("layer-reuse-extent-{scale}"),
            output,
            results,
        )?;
        renderer.set_layer_texture_reuse_enabled(false);
        renderer.reset()?;
        renderer.submit(&frame, surface, scale)?;
        translate(&mut tree, group, 0)?;
        let frame = tree.frame([320.0, 220.0])?;
        let forced = renderer.submit(&frame, surface, scale)?.unwrap();
        assert_eq!(
            (
                forced.gpu.layer_allocations,
                forced.gpu.rebased_layer_textures
            ),
            (2, 0)
        );
        compare(
            &mut renderer,
            &frame,
            surface,
            scale,
            &format!("layer-reuse-forced-reset-{scale}"),
            output,
            results,
        )?;
        renderer.set_layer_texture_reuse_enabled(true);
        renderer.reset()?;
        renderer.submit(&frame, surface, scale)?;
        translate(&mut tree, group, 1)?;
        let frame = tree.frame([320.0, 220.0])?;
        let reused = renderer.submit(&frame, surface, scale)?.unwrap();
        assert_eq!(
            (
                reused.gpu.layer_allocations,
                reused.gpu.rebased_layer_textures
            ),
            (0, 2)
        );
        compare(
            &mut renderer,
            &frame,
            surface,
            scale,
            &format!("layer-reuse-enabled-reset-{scale}"),
            output,
            results,
        )?;
    }
    // No instance/resource writes: target rebasing alone must invalidate the raster.
    let mut backend = gpui_wgpu::RetainedQuadRenderer::new(3)?;
    backend.set_layer_budget_bytes(16 * 16 * 4)?;
    let batch = |origin, write| gpui_wgpu::QuadBatch {
        capacity: 1,
        writes: if write {
            vec![gpui_wgpu::QuadWrite {
                start: 0,
                values: vec![gpui::Quad {
                    bounds: gpui::Bounds {
                        origin: gpui::point(gpui::ScaledPixels(4.0), gpui::ScaledPixels(4.0)),
                        size: gpui::size(gpui::ScaledPixels(16.0), gpui::ScaledPixels(16.0)),
                    },
                    content_mask: gpui::ContentMask {
                        bounds: gpui::Bounds {
                            origin: gpui::point(gpui::ScaledPixels(0.0), gpui::ScaledPixels(0.0)),
                            size: gpui::size(gpui::ScaledPixels(64.0), gpui::ScaledPixels(64.0)),
                        },
                    },
                    background: gpui::rgba(0xff0000ff).into(),
                    ..Default::default()
                }],
            }]
        } else {
            vec![]
        },
        order: Some(vec![gpui_wgpu::QuadDraw::Layer([1, 0, 1])]),
        layers: vec![gpui_wgpu::QuadLayer {
            id: [1, 0, 1],
            opacity: 0.5,
            bounds: Some([origin, origin, 16, 16]),
            draws: Arc::from([gpui_wgpu::QuadDraw::Instances(0..1)]),
        }],
        images: Default::default(),
        glyphs: Default::default(),
        shadows: Default::default(),
        spatial: Default::default(),
        cache_layers: true,
        size: gpui::size(gpui::DevicePixels(64), gpui::DevicePixels(64)),
        redraw: gpui_wgpu::QuadRedraw::Full,
    };
    backend.submit(batch(4, true))?;
    assert!(backend.image()?.get_pixel(8, 8).0[0].abs_diff(128) <= 1);
    let submit = backend.submit(batch(24, false))?;
    assert_eq!(
        (
            submit.layer_allocations,
            submit.rebased_layer_textures,
            submit.rasterized_layers
        ),
        (0, 1, 1)
    );
    let image = backend.image()?;
    assert_eq!(image.get_pixel(28, 28).0, [0, 0, 0, 255]);
    assert_eq!(image.get_pixel(8, 8).0, [0, 0, 0, 255]);
    for step in 0..24 {
        backend.submit(batch(if step % 2 == 0 { 4 } else { 24 }, false))?;
    }
    let image = backend.image()?;
    assert_eq!(image.get_pixel(28, 28).0, [0, 0, 0, 255]);
    backend.submit(batch(4, false))?;
    let image = backend.image()?;
    assert!(image.get_pixel(8, 8).0[0].abs_diff(128) <= 1);
    image.save(output.join("layer-reuse-raw-origin-only.png"))?;
    results.push(serde_json::json!({"case":"layer-reuse-raw-origin-only","analytical_pixels":true,"instance_writes_after_first":0,"peak_bytes":backend.layer_memory().peak_resident_bytes}));
    Ok(())
}
pub(super) fn benchmark() -> Result<serde_json::Value, Box<dyn Error>> {
    let mut cases = vec![];
    for quads in [4, 128] {
        for narrow in [false, true] {
            let (mut tree, group) = fixture(quads)?;
            let mut renderer = WgpuFrameRenderer::new(3)?;
            let bytes = 1202 * 902 * 4;
            renderer.set_layer_budget_bytes(if narrow { bytes } else { 256 * 1024 * 1024 })?;
            let mut rounds: [Vec<f64>; 2] = Default::default();
            let mut cpu: [Vec<f64>; 2] = Default::default();
            let mut allocated = [0; 2];
            let mut rebased = [0; 2];
            let mut waits = [0; 2];
            let mut max_resident = [0; 2];
            for round in 0..3 {
                for mode in if round % 2 == 0 { [0, 1] } else { [1, 0] } {
                    renderer.finish()?;
                    renderer.set_layer_texture_reuse_enabled(mode == 0);
                    renderer.submit(&tree.frame([1400.0, 1000.0])?, [1400, 1000], 1.0)?;
                    renderer.finish()?;
                    let mut start = Instant::now();
                    for step in 0..26 {
                        if step == 6 {
                            renderer.finish()?;
                            start = Instant::now();
                        }
                        let cpu_start = Instant::now();
                        translate(&mut tree, group, step)?;
                        let submit = renderer
                            .submit(&tree.frame([1400.0, 1000.0])?, [1400, 1000], 1.0)?
                            .unwrap();
                        let us = cpu_start.elapsed().as_secs_f64() * 1e6;
                        assert_eq!(submit.gpu.layer_allocations, usize::from(mode == 1));
                        assert_eq!(submit.gpu.rebased_layer_textures, usize::from(mode == 0));
                        assert_eq!(submit.gpu.layer_target_bytes, 48);
                        assert_eq!(submit.gpu.rasterized_layers, 1);
                        if mode == 0 {
                            assert_eq!(submit.gpu.layer_budget_waits, 0);
                        }
                        if step >= 6 {
                            cpu[mode].push(us);
                            allocated[mode] += submit.gpu.allocated_layer_bytes;
                            rebased[mode] += submit.gpu.rebased_layer_textures;
                            waits[mode] += submit.gpu.layer_budget_waits;
                            max_resident[mode] =
                                max_resident[mode].max(submit.gpu.layer_memory.resident_bytes);
                        }
                    }
                    renderer.finish()?;
                    rounds[mode].push(start.elapsed().as_secs_f64() * 1e6 / 20.0);
                    let actual = renderer.image()?;
                    let reference = renderer.reference_image(
                        &tree.frame([1400.0, 1000.0])?,
                        [1400, 1000],
                        1.0,
                    )?;
                    assert!(
                        actual.as_raw() == reference.as_raw(),
                        "layer texture reuse benchmark pixels"
                    );
                }
            }
            let value = serde_json::json!({"quads":quads,"budget_bytes":if narrow {bytes}else{256*1024*1024},
                "reuse_round_us":rounds[0],"replace_round_us":rounds[1],"reuse_cpu_samples_us":cpu[0],"replace_cpu_samples_us":cpu[1],
                "reuse_allocated_bytes":allocated[0],"replace_allocated_bytes":allocated[1],"rebased_textures":rebased[0],
                "reuse_budget_waits":waits[0],"replace_budget_waits":waits[1],
                "reuse_max_postsubmit_resident_bytes":max_resident[0],"replace_max_postsubmit_resident_bytes":max_resident[1],"pixel_equal":true});
            println!(
                "Texture reuse {quads} quads narrow={narrow}: {} / {} us; resident={} / {}",
                value["reuse_round_us"],
                value["replace_round_us"],
                value["reuse_max_postsubmit_resident_bytes"],
                value["replace_max_postsubmit_resident_bytes"]
            );
            cases.push(value);
        }
    }
    Ok(
        serde_json::json!({"scope":"same renderer and allocation cap; persistent same-extent texture/camera versus forced replacement; alternating origins; full root redraw and actual content raster in both modes; timed owner commit/publication/submission and final completion; excludes readback/native presentation","max_in_flight":3,"rounds":3,"warmup":6,"samples_per_round":20,"cases":cases}),
    )
}
