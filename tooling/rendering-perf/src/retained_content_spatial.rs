//! Shared image/glyph placement, exact content offsets and raw rejection/retry.
use super::*;
use gpui_wgpu::{
    GlyphBatch, GlyphGeometry, GlyphUpdates, ImageUpdates, InstanceWrite, QuadBatch, QuadDraw,
    QuadImage, QuadLayer, QuadRedraw, RetainedGlyph, SpatialBatch, SpatialContent, SpatialNode,
};
use std::{sync::Arc, time::Instant};
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
fn padded(x: f32, y: f32, w: f32, h: f32, border: f32, padding: f32) -> Style {
    Style {
        border: taffy::Rect {
            left: length(border),
            right: length(border),
            top: length(border),
            bottom: length(border),
        },
        padding: taffy::Rect {
            left: length(padding),
            right: length(padding),
            top: length(padding + 0.5),
            bottom: length(padding),
        },
        ..absolute(x, y, w, h)
    }
}
pub(super) fn run(
    output: &std::path::Path,
    results: &mut Vec<serde_json::Value>,
) -> Result<(), Box<dyn Error>> {
    retained_routing::run_content_controls(output, results)?;
    for shared in [true, false] {
        for scale in [1.0, 1.25, 1.5] {
            padding_and_toggle(shared, scale, output, results)?;
        }
    }
    backend_checks(results)?;
    retained_overflow(false, results)?;
    retained_overflow(true, results)?;
    negative_rounding(output, results)?;
    Ok(())
}

fn negative_rounding(
    output: &std::path::Path,
    results: &mut Vec<serde_json::Value>,
) -> Result<(), Box<dyn Error>> {
    for scale in [1.0, 1.25, 1.5] {
        let geometry = GlyphGeometry::new(vec![RetainedGlyph {
            resource: 905,
            size: [1, 1],
            mask: vec![255].into(),
            origin: [0.0; 2],
            offset: [2, 0],
        }])?;
        let mut node = anchor(-0.5 / scale);
        node.anchor_origin[1] = 8.0;
        let draws: Arc<[QuadDraw]> = Arc::from([QuadDraw::Glyphs([77, 2, 0])]);
        let mut gpu = gpui_wgpu::RetainedQuadRenderer::new(1)?;
        let mut input = batch(
            SpatialBatch {
                enabled: true,
                capacity: 1,
                scale,
                writes: vec![InstanceWrite {
                    start: 0,
                    values: vec![node],
                }],
            },
            Default::default(),
            GlyphUpdates {
                batches: vec![GlyphBatch {
                    id: [77, 2, 0],
                    clip: [0.0, 0.0, 64.0, 64.0],
                    color: 0xffffffff,
                    origin: [0.0; 2],
                    scale,
                    geometry: geometry.clone(),
                    spatial: Some(SpatialContent {
                        property: 1,
                        border: [0.0; 2],
                        padding: [0.0; 2],
                    }),
                }],
                ..Default::default()
            },
            draws.clone(),
        );
        gpu.submit(input)?;
        let shared = gpu.image()?;
        assert_eq!(shared.get_pixel(1, (8.0 * scale) as u32).0, [255; 4]);
        assert_eq!(shared.get_pixel(2, (8.0 * scale) as u32).0, [0, 0, 0, 255]);
        input = batch(
            SpatialBatch {
                enabled: true,
                capacity: 1,
                scale,
                writes: vec![],
            },
            Default::default(),
            GlyphUpdates {
                batches: vec![GlyphBatch {
                    id: [77, 2, 0],
                    clip: [0.0, 0.0, 64.0, 64.0],
                    color: 0xffffffff,
                    origin: [-0.5 / scale, 8.0],
                    scale,
                    geometry,
                    spatial: None,
                }],
                ..Default::default()
            },
            draws,
        );
        gpu.submit(input)?;
        assert_eq!(shared.as_raw(), gpu.image()?.as_raw());
        shared.save(output.join(format!("content-negative-rounding-{scale}.png")))?;
        results.push(serde_json::json!({"case":format!("content-negative-rounding-{scale}"),"pixel_equal":true,"analytical_negative_half_round":true,"reference":"absolute glyph placement plus CPU-expected bitmap pixel"}));
    }
    Ok(())
}
fn padding_and_toggle(
    shared: bool,
    scale: f32,
    output: &std::path::Path,
    results: &mut Vec<serde_json::Value>,
) -> Result<(), Box<dyn Error>> {
    let (context, mut raster) = retained_text::context()?;
    let text = retained_text::text(&context, &mut raster, "内容共享 中文", scale)?;
    let mut tree = UiTree::new(absolute(0.0, 0.0, 186.0, 144.0), 0x171d28ff)?;
    let group = tree.append(tree.root(), absolute(5.25, 3.75, 152.0, 120.0), 0)?;
    let image = tree.append(group, padded(8.5, 8.25, 32.0, 36.0, 1.5, 2.25), 0)?;
    let label = tree.append(group, padded(48.25, 8.25, 96.0, 52.0, 0.75, 2.25), 0)?;
    tree.append(group, absolute(240.0, 180.0, 8.0, 8.0), 0)?;
    let mut tx = Transaction::default();
    tx.image(
        image,
        Some(ImageResource::rgba8(
            [2, 2],
            vec![
                255, 0, 0, 255, 0, 255, 0, 128, 0, 0, 255, 255, 255, 255, 255, 128,
            ],
        )?),
    );
    tx.text(
        label,
        Some(text::TextPaint {
            resource: text,
            color: 0xffdd88ff,
        }),
    );
    tree.commit(tx, BackendCapabilities::default())?;
    let size = [(186.0 * scale) as u32, (144.0 * scale) as u32];
    let mut gpu = WgpuFrameRenderer::new(3)?;
    gpu.set_shared_content_enabled(shared);
    gpu.set_policy(CompositorPolicy::Damage)?;
    let mut old = None;
    for step in 0..10 {
        let mut tx = Transaction::default();
        match step {
            0 => {}
            1 => tx.translation(group, [0.5, -0.75]),
            2 => tx.clip(
                group,
                Some(Rect {
                    x: 1.25,
                    y: 2.5,
                    width: 142.75,
                    height: 90.25,
                }),
            ),
            3 => tx.scroll(group, [1.25, 2.5]),
            4 => {
                tx.style(image, padded(8.5, 8.25, 32.0, 36.0, 1.25, 2.5));
                tx.style(label, padded(48.25, 8.25, 96.0, 52.0, 1.0, 2.0));
            }
            5 => {
                tx.style(image, padded(8.5, 8.25, 38.25, 36.0, 1.25, 2.5));
                tx.style(label, padded(48.25, 8.25, 68.25, 52.0, 1.0, 2.0));
            }
            6 => tx.opacity(group, 0.45),
            7 => gpu.set_shared_content_enabled(!shared),
            8 => {
                gpu.set_shared_content_enabled(shared);
                tx.clip(group, None);
            }
            _ => {
                tx.image(image, None);
                tx.text(label, None);
            }
        }
        tree.commit(tx, BackendCapabilities::default())?;
        let frame = tree.frame([186.0, 144.0])?;
        if step == 0 {
            old = Some(frame.clone());
        }
        let submit = gpu.submit(&frame, size, scale)?.unwrap();
        if step == 3 {
            assert_eq!(
                frame
                    .items
                    .iter()
                    .find(|i| i.node == group)
                    .unwrap()
                    .spatial
                    .scroll,
                [1.25, 2.5]
            );
        }
        if shared && [1, 2, 3].contains(&step) {
            assert_eq!(
                (
                    submit.gpu.image_geometry_bytes,
                    submit.gpu.glyph_property_bytes,
                    submit.gpu.glyph_instance_bytes
                ),
                (0, 0, 0)
            );
            assert_eq!(
                (
                    submit.gpu.image_batch_updates,
                    submit.gpu.glyph_batch_updates
                ),
                (0, 0)
            );
        }
        if step == 7 {
            assert!(submit.full_resync);
            assert_eq!(
                (
                    submit.gpu.glyph_instance_bytes,
                    submit.gpu.glyph_atlas_bytes,
                    submit.gpu.image_upload_bytes
                ),
                (0, 0, 0)
            );
        }
        compare(
            &mut gpu,
            &frame,
            size,
            scale,
            &format!("content-padding-{shared}-{scale}-{step}"),
            output,
            results,
        )?;
        if step < 4 {
            let b = frame
                .items
                .iter()
                .find(|i| i.node == image)
                .unwrap()
                .content_bounds;
            let p = [
                ((b.x + b.width * 0.25) * scale) as u32,
                ((b.y + b.height * 0.25) * scale) as u32,
            ];
            assert_eq!(gpu.image()?.get_pixel(p[0], p[1]).0, [255, 0, 0, 255]);
            results.last_mut().unwrap()["analytical_image_texel"] = true.into();
        }
        results.last_mut().unwrap()["image_geometry_bytes"] =
            submit.gpu.image_geometry_bytes.into();
        results.last_mut().unwrap()["glyph_property_bytes"] =
            submit.gpu.glyph_property_bytes.into();
        if step == 6 {
            let larger = [size[0] + 8, size[1] + 8];
            let resized = gpu.submit(&frame, larger, scale)?.unwrap();
            assert_eq!(
                resized.gpu.image_geometry_bytes,
                if shared { 0 } else { 48 }
            );
            compare(
                &mut gpu,
                &frame,
                larger,
                scale,
                &format!("content-surface-only-{shared}-{scale}"),
                output,
                results,
            )?;
            let restored = gpu.submit(&frame, size, scale)?.unwrap();
            assert_eq!(
                restored.gpu.image_geometry_bytes,
                if shared { 0 } else { 48 }
            );
            compare(
                &mut gpu,
                &frame,
                size,
                scale,
                &format!("content-surface-restored-{shared}-{scale}"),
                output,
                results,
            )?;
        }
    }
    gpu.finish()?;
    assert_eq!(gpu.lifecycle().frame_leases, 0);
    assert_eq!(gpu.lifecycle().gpu_leases, 0);
    let old = old.unwrap();
    drop(tree);
    drop(raster);
    drop(context);
    let mut fresh = WgpuFrameRenderer::new(1)?;
    fresh.set_shared_content_enabled(shared);
    fresh.submit(&old, size, scale)?;
    compare(
        &mut fresh,
        &old,
        size,
        scale,
        &format!("content-old-owner-drop-{shared}-{scale}"),
        output,
        results,
    )?;
    Ok(())
}
fn anchor(x: f32) -> SpatialNode {
    SpatialNode {
        anchor_origin: [x, 8.0, 0.0, 0.0],
        anchor_clip: [0.0, 0.0, 64.0, 64.0],
        links: [u32::MAX, 0, 10, 0],
        ..Default::default()
    }
}
fn batch(
    spatial: SpatialBatch,
    images: ImageUpdates,
    glyphs: GlyphUpdates,
    draws: Arc<[QuadDraw]>,
) -> QuadBatch {
    QuadBatch {
        capacity: 0,
        writes: vec![],
        order: Some(vec![QuadDraw::Layer([77, 0, 0])]),
        layers: vec![QuadLayer {
            id: [77, 0, 0],
            opacity: 1.0,
            bounds: Some([0, 0, 64, 64]),
            draws,
        }],
        images,
        glyphs,
        shadows: Default::default(),
        spatial,
        cache_layers: true,
        size: gpui::size(gpui::DevicePixels(64), gpui::DevicePixels(64)),
        redraw: QuadRedraw::Full,
    }
}
fn space(writes: Vec<InstanceWrite<SpatialNode>>) -> SpatialBatch {
    SpatialBatch {
        enabled: true,
        capacity: 2,
        scale: 1.0,
        writes,
    }
}
fn backend_checks(results: &mut Vec<serde_json::Value>) -> Result<(), Box<dyn Error>> {
    let pixels: Arc<[u8]> = vec![255, 0, 0, 255].into();
    let image = || QuadImage {
        id: [77, 1, 0],
        resource: 901,
        size: [1, 1],
        rgba: pixels.clone(),
        bounds: [0.0, 0.0, 8.0, 8.0],
        clip: [0.0, 0.0, 64.0, 64.0],
        spatial: Some(SpatialContent {
            property: 2,
            border: [1.25, 0.75],
            padding: [2.0, 1.5],
        }),
    };
    let geometry = GlyphGeometry::new(vec![RetainedGlyph {
        resource: 902,
        size: [1, 1],
        mask: vec![255].into(),
        origin: [0.0; 2],
        offset: [0; 2],
    }])?;
    let glyph = || GlyphBatch {
        id: [77, 2, 0],
        clip: [0.0, 0.0, 64.0, 64.0],
        color: 0xffffffff,
        origin: [0.0; 2],
        scale: 1.0,
        geometry: geometry.clone(),
        spatial: Some(SpatialContent {
            property: 1,
            border: [20.0, 0.0],
            padding: [0.0; 2],
        }),
    };
    let draws: Arc<[QuadDraw]> =
        Arc::from([QuadDraw::Image([77, 1, 0]), QuadDraw::Glyphs([77, 2, 0])]);
    let mut gpu = gpui_wgpu::RetainedQuadRenderer::new(3)?;
    gpu.submit(batch(
        space(vec![InstanceWrite {
            start: 0,
            values: vec![
                anchor(8.0),
                SpatialNode {
                    links: [0, 1, 0, 0],
                    ..Default::default()
                },
            ],
        }]),
        ImageUpdates {
            images: vec![image()],
            ..Default::default()
        },
        GlyphUpdates {
            batches: vec![glyph()],
            ..Default::default()
        },
        draws.clone(),
    ))?;
    assert_eq!(gpu.image()?.get_pixel(12, 11).0, [255, 0, 0, 255]);
    assert_eq!(gpu.image()?.get_pixel(28, 8).0, [255; 4]);
    let moved = gpu.submit(batch(
        space(vec![InstanceWrite {
            start: 0,
            values: vec![anchor(9.0)],
        }]),
        Default::default(),
        Default::default(),
        draws.clone(),
    ))?;
    assert_eq!(
        (
            moved.image_geometry_bytes,
            moved.glyph_property_bytes,
            moved.glyph_instance_bytes
        ),
        (0, 0, 0)
    );
    assert_eq!(moved.rasterized_layers, 1);
    let before = gpu.image()?;
    assert_eq!(before.get_pixel(13, 11).0, [255, 0, 0, 255]);
    assert_eq!(before.get_pixel(29, 8).0, [255; 4]);
    for kind in 0..10 {
        let mut input = batch(
            space(vec![]),
            Default::default(),
            Default::default(),
            draws.clone(),
        );
        match kind {
            0 => {
                let mut value = image();
                value.spatial.as_mut().unwrap().property = 3;
                input.images.images.push(value);
            }
            1 => {
                let mut value = glyph();
                value.spatial.as_mut().unwrap().property = 3;
                input.glyphs.batches.push(value);
            }
            2 => {
                let mut value = image();
                value.spatial.as_mut().unwrap().padding[0] = f32::NAN;
                input.images.images.push(value);
            }
            3 => {
                let mut value = glyph();
                value.spatial.as_mut().unwrap().border[0] = f32::NAN;
                input.glyphs.batches.push(value);
            }
            4 => input.spatial = Default::default(),
            5 => input.spatial.capacity = 1,
            6 => {
                let mut value = glyph();
                value.scale = 2.0;
                input.glyphs.batches.push(value);
            }
            7 => {
                let mut value = image();
                value.spatial.as_mut().unwrap().border[0] = f32::MAX * 0.75;
                input.images.images.push(value);
                input.spatial.writes.push(InstanceWrite {
                    start: 0,
                    values: vec![anchor(f32::MAX * 0.75)],
                });
            }
            8 => {
                input.spatial.writes.push(InstanceWrite {
                    start: 0,
                    values: vec![anchor(20.0)],
                });
                input.layers[0].opacity = 2.0;
            }
            _ => {
                let mut value = image();
                value.spatial.as_mut().unwrap().property = 1;
                input.images.images.push(value);
                input.redraw = QuadRedraw::Damage {
                    x: 0,
                    y: 0,
                    width: 0,
                    height: 1,
                };
            }
        }
        assert!(gpu.submit(input).is_err(), "content malformed {kind}");
        assert_eq!(gpu.last_serial(), 2);
    }
    gpu.submit(batch(
        space(vec![]),
        Default::default(),
        Default::default(),
        draws.clone(),
    ))?;
    assert_eq!(before.as_raw(), gpu.image()?.as_raw());
    let mut absolute_image = image();
    absolute_image.spatial = None;
    absolute_image.bounds = [12.25, 10.25, 8.0, 8.0];
    let mut absolute_glyph = glyph();
    absolute_glyph.spatial = None;
    absolute_glyph.origin = [29.0, 8.0];
    let flip = gpu.submit(batch(
        Default::default(),
        ImageUpdates {
            images: vec![absolute_image],
            ..Default::default()
        },
        GlyphUpdates {
            batches: vec![absolute_glyph],
            ..Default::default()
        },
        draws.clone(),
    ))?;
    assert_eq!(flip.glyph_instance_bytes, 0);
    assert_eq!(before.as_raw(), gpu.image()?.as_raw());
    gpu.submit(batch(
        space(vec![]),
        ImageUpdates {
            images: vec![image()],
            ..Default::default()
        },
        GlyphUpdates {
            batches: vec![glyph()],
            ..Default::default()
        },
        draws,
    ))?;
    assert_eq!(before.as_raw(), gpu.image()?.as_raw());
    results.push(serde_json::json!({"case":"content-shared-raw-preflight","malformed_rejected":10,"zero_descriptor_ancestor_reraster":true,"analytical_pixels":true,"valid_retry":true,"atomic_mode_flip":true}));
    Ok(())
}
fn retained_overflow(
    glyph: bool,
    results: &mut Vec<serde_json::Value>,
) -> Result<(), Box<dyn Error>> {
    let mut gpu = gpui_wgpu::RetainedQuadRenderer::new(1)?;
    let mut images = ImageUpdates::default();
    let mut glyphs = GlyphUpdates::default();
    let draws: Arc<[QuadDraw]> = if glyph {
        glyphs.batches.push(GlyphBatch {
            id: [77, 2, 0],
            clip: [0.0, 0.0, 64.0, 64.0],
            color: 0xffffffff,
            origin: [0.0; 2],
            scale: 1.0,
            geometry: GlyphGeometry::new(vec![RetainedGlyph {
                resource: 903,
                size: [1, 1],
                mask: vec![255].into(),
                origin: [f32::MAX * 0.75, 0.0],
                offset: [0; 2],
            }])?,
            spatial: Some(SpatialContent {
                property: 1,
                border: [0.0; 2],
                padding: [0.0; 2],
            }),
        });
        Arc::from([QuadDraw::Glyphs([77, 2, 0])])
    } else {
        images.images.push(QuadImage {
            id: [77, 1, 0],
            resource: 904,
            size: [1, 1],
            rgba: vec![255; 4].into(),
            bounds: [0.0, 0.0, 1.0, 1.0],
            clip: [0.0, 0.0, 64.0, 64.0],
            spatial: Some(SpatialContent {
                property: 1,
                border: [f32::MAX * 0.75, 0.0],
                padding: [0.0; 2],
            }),
        });
        Arc::from([QuadDraw::Image([77, 1, 0])])
    };
    gpu.submit(batch(
        space(vec![InstanceWrite {
            start: 0,
            values: vec![
                anchor(0.0),
                SpatialNode {
                    links: [0, 1, 0, 0],
                    ..Default::default()
                },
            ],
        }]),
        images,
        glyphs,
        draws.clone(),
    ))?;
    assert!(
        gpu.submit(batch(
            space(vec![InstanceWrite {
                start: 0,
                values: vec![anchor(f32::MAX * 0.5)]
            }]),
            Default::default(),
            Default::default(),
            draws.clone()
        ))
        .is_err()
    );
    assert_eq!(gpu.last_serial(), 1);
    gpu.submit(batch(
        space(vec![]),
        Default::default(),
        Default::default(),
        draws,
    ))?;
    results.push(serde_json::json!({"case":format!("content-retained-overflow-{glyph}"),"unmentioned_shared_draw_checked":true,"overflow_rejected":true,"valid_retry":true}));
    Ok(())
}

pub(super) fn benchmark() -> Result<serde_json::Value, Box<dyn Error>> {
    let (context, mut raster) = retained_text::context()?;
    let text = retained_text::text(&context, &mut raster, "i", 1.0)?;
    let image = ImageResource::rgba8([1, 1], vec![255, 128, 0, 160])?;
    let mut cases = vec![];
    for count in [100, 1000] {
        for workload in ["ancestor_translation", "clip_toggle", "single_text_color"] {
            let mut controls = vec![];
            for shared in [true, false] {
                let mut tree = UiTree::new(absolute(0.0, 0.0, 800.0, 600.0), 0x171d28ff)?;
                let parent = tree.append(tree.root(), absolute(0.0, 0.0, 800.0, 600.0), 0)?;
                let mut target = parent;
                let mut tx = Transaction::default();
                for i in 0..count {
                    let x = (i % 50) as f32 * 16.0;
                    let y = (i / 50) as f32 * 24.0;
                    let sprite = tree.append(parent, absolute(x, y, 8.0, 10.0), 0)?;
                    tx.image(sprite, Some(image.clone()));
                    let label = tree.append(parent, absolute(x + 9.0, y, 6.0, 20.0), 0)?;
                    tx.text(
                        label,
                        Some(text::TextPaint {
                            resource: text.clone(),
                            color: 0xffdd88ff,
                        }),
                    );
                    if i == count / 2 {
                        target = label;
                    }
                }
                tree.commit(tx, BackendCapabilities::default())?;
                let mut gpu = WgpuFrameRenderer::new(1)?;
                gpu.set_shared_content_enabled(shared);
                gpu.submit(&tree.frame([800.0, 600.0])?, [800, 600], 1.0)?;
                gpu.finish()?;
                controls.push((tree, parent, target, gpu));
            }
            let mut phases: [Vec<[f64; 3]>; 2] = Default::default();
            let mut complete: [Vec<f64>; 2] = Default::default();
            let mut counters: [serde_json::Value; 2] = Default::default();
            for round in 0..3 {
                for mode in if round % 2 == 0 { [0, 1] } else { [1, 0] } {
                    let (tree, parent, target, gpu) = &mut controls[mode];
                    for step in 0..26 {
                        let start = Instant::now();
                        let mut tx = Transaction::default();
                        match workload {
                            "ancestor_translation" => tx.translation(
                                *parent,
                                if step % 2 == 0 {
                                    [1.25, 2.5]
                                } else {
                                    [3.25, 4.5]
                                },
                            ),
                            "clip_toggle" => tx.clip(
                                *parent,
                                Some(Rect {
                                    x: 0.5,
                                    y: 0.25,
                                    width: if step % 2 == 0 { 720.25 } else { 760.75 },
                                    height: 580.5,
                                }),
                            ),
                            _ => tx.text(
                                *target,
                                Some(text::TextPaint {
                                    resource: text.clone(),
                                    color: if step % 2 == 0 {
                                        0xee4455ff
                                    } else {
                                        0xffdd88ff
                                    },
                                }),
                            ),
                        }
                        tree.commit(
                            tx,
                            BackendCapabilities {
                                solid_color_patch: true,
                            },
                        )?;
                        let commit = start.elapsed().as_secs_f64() * 1e6;
                        let frame = tree.frame([800.0, 600.0])?;
                        let published = start.elapsed().as_secs_f64() * 1e6;
                        let submit = gpu.submit(&frame, [800, 600], 1.0)?.unwrap();
                        let submitted = start.elapsed().as_secs_f64() * 1e6;
                        gpu.finish()?;
                        let finished = start.elapsed().as_secs_f64() * 1e6;
                        let s = submit.gpu;
                        assert!(!submit.full_resync);
                        if workload != "single_text_color" {
                            assert_eq!(s.spatial_upload_bytes, 96);
                            assert_eq!(
                                s.image_geometry_bytes,
                                if mode == 0 { 0 } else { count as u64 * 48 }
                            );
                            assert_eq!(
                                s.glyph_property_bytes,
                                if mode == 0 { 0 } else { count as u64 * 32 }
                            );
                            assert_eq!(
                                (
                                    s.glyph_instance_bytes,
                                    s.glyph_atlas_bytes,
                                    s.image_upload_bytes
                                ),
                                (0, 0, 0)
                            );
                        } else {
                            assert_eq!(s.glyph_material_bytes, 16);
                            assert_eq!(
                                (
                                    s.glyph_instance_bytes,
                                    s.glyph_property_bytes,
                                    s.image_geometry_bytes
                                ),
                                (0, 0, 0)
                            );
                        }
                        if step >= 6 {
                            phases[mode].push([commit, published - commit, submitted - published]);
                            complete[mode].push(finished);
                            counters[mode] = serde_json::json!({"image_geometry_bytes":s.image_geometry_bytes,"glyph_property_bytes":s.glyph_property_bytes,"spatial_bytes":s.spatial_upload_bytes,"image_upload_bytes":s.image_upload_bytes,"glyph_instance_bytes":s.glyph_instance_bytes,"glyph_atlas_bytes":s.glyph_atlas_bytes,"glyph_material_bytes":s.glyph_material_bytes,"image_batch_updates":s.image_batch_updates,"glyph_batch_updates":s.glyph_batch_updates,"extracted_nodes":frame.build_stats.extracted_nodes,"draw_calls":s.draw_calls});
                        }
                    }
                    let frame = tree.frame([800.0, 600.0])?;
                    assert_eq!(
                        gpu.image()?.as_raw(),
                        gpu.reference_image(&frame, [800, 600], 1.0)?.as_raw()
                    );
                }
            }
            let percentile = |values: &[f64], p: f64| {
                let mut values = values.to_vec();
                values.sort_by(f64::total_cmp);
                values[((values.len() - 1) as f64 * p).round() as usize]
            };
            let result = serde_json::json!({"pairs":count,"workload":workload,"phase_columns":["commit_us","publication_us","submission_us"],"shared_phases_us":phases[0],"world_phases_us":phases[1],"shared_completed_us":complete[0],"world_completed_us":complete[1],"shared_counters":counters[0],"world_counters":counters[1],"shared_median_us":percentile(&complete[0],0.5),"world_median_us":percentile(&complete[1],0.5),"shared_p95_us":percentile(&complete[0],0.95),"world_p95_us":percentile(&complete[1],0.95),"pixel_equal":true});
            println!(
                "Content {workload} {count} pairs: {} / {} us",
                result["shared_median_us"], result["world_median_us"]
            );
            cases.push(result);
        }
    }
    Ok(
        serde_json::json!({"scope":"normal owner commit/publication/submission and per-frame GPU completion; shared image/glyph spatial references and local descriptor routing versus independent world-placement records; same shared Quad/Shadow tree, backend index, publisher, full redraw and content; setup/readback/native presentation excluded","rounds":3,"warmup":6,"samples_per_round":20,"max_in_flight":1,"cases":cases}),
    )
}
