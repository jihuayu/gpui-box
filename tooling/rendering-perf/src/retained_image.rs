//! Immutable decoded resource reuse, pixel oracles and completed-work A/B.
use super::*;
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
fn resource(reverse: bool) -> ImageResource {
    let mut pixels = vec![
        255, 0, 0, 255, 0, 255, 0, 128, 0, 0, 255, 255, 255, 255, 255, 0,
    ];
    if reverse {
        pixels.rotate_left(4);
    }
    ImageResource::rgba8([2, 2], pixels).unwrap()
}
fn set(
    tree: &mut UiTree,
    node: NodeId,
    image: Option<ImageResource>,
) -> Result<(), Box<dyn Error>> {
    let mut tx = Transaction::default();
    tx.image(node, image);
    tree.commit(tx, BackendCapabilities::default())?;
    Ok(())
}
pub(super) fn run(
    renderer: &mut WgpuFrameRenderer,
    output: &std::path::Path,
    results: &mut Vec<serde_json::Value>,
) -> Result<(), Box<dyn Error>> {
    renderer.set_policy(CompositorPolicy::Damage)?;
    let mut tree = UiTree::new(absolute(0.0, 0.0, 128.0, 96.0), 0x000000ff)?;
    let node = tree.append(tree.root(), absolute(8.0, 8.0, 32.0, 32.0), 0)?;
    let other = tree.append(tree.root(), absolute(64.0, 8.0, 32.0, 32.0), 0)?;
    let a = resource(false);
    let b = resource(true);
    set(&mut tree, node, Some(a.clone()))?;
    set(&mut tree, other, Some(a.clone()))?;
    let mut frame = tree.frame([128.0, 96.0])?;
    let first = renderer.submit(&frame, [128, 96], 1.0)?.unwrap();
    assert_eq!(
        (first.gpu.image_upload_bytes, first.gpu.image_uploads),
        (16, 1)
    );
    let pixels = renderer.image()?;
    for (x, y, expected) in [
        (12, 12, [255, 0, 0, 255]),
        (28, 12, [0, 128, 0, 255]),
        (12, 28, [0, 0, 255, 255]),
        (28, 28, [0, 0, 0, 255]),
        (68, 12, [255, 0, 0, 255]),
    ] {
        let actual = pixels.get_pixel(x, y).0;
        assert!(
            actual.iter().zip(expected).all(|(a, b)| a.abs_diff(b) <= 1),
            "image oracle {x},{y}: {actual:?}"
        );
    }
    compare(
        renderer,
        &frame,
        [128, 96],
        1.0,
        "image-shared-rgba",
        output,
        results,
    )?;
    results.last_mut().unwrap()["texture_upload_bytes"] = first.gpu.image_upload_bytes.into();
    let mut tx = Transaction::default();
    tx.translation(node, [4.0, 0.0]);
    tree.commit(tx, BackendCapabilities::default())?;
    frame = tree.frame([128.0, 96.0])?;
    let moved = renderer.submit(&frame, [128, 96], 1.0)?.unwrap();
    assert_eq!(
        (moved.gpu.image_upload_bytes, moved.gpu.image_geometry_bytes),
        (0, 48)
    );
    assert!(moved.gpu.partial);
    compare(
        renderer,
        &frame,
        [128, 96],
        1.0,
        "image-transform-reuse",
        output,
        results,
    )?;
    let mut tx = Transaction::default();
    tx.clip(
        node,
        Some(Rect {
            x: 0.0,
            y: 0.0,
            width: 16.0,
            height: 32.0,
        }),
    );
    tree.commit(tx, BackendCapabilities::default())?;
    frame = tree.frame([128.0, 96.0])?;
    assert_eq!(
        renderer
            .submit(&frame, [128, 96], 1.0)?
            .unwrap()
            .gpu
            .image_upload_bytes,
        0
    );
    let pixels = renderer.image()?;
    assert_eq!(pixels.get_pixel(32, 12).0, [0, 0, 0, 255]);
    compare(
        renderer,
        &frame,
        [128, 96],
        1.0,
        "image-clip",
        output,
        results,
    )?;
    set(&mut tree, node, Some(b.clone()))?;
    frame = tree.frame([128.0, 96.0])?;
    assert_eq!((frame.layout_runs, frame.recorded_chunks), (0, 0));
    let replacement = renderer.submit(&frame, [128, 96], 1.0)?.unwrap();
    assert_eq!(
        (
            replacement.gpu.uploaded_bytes,
            replacement.gpu.image_upload_bytes
        ),
        (0, 16)
    );
    compare(
        renderer,
        &frame,
        [128, 96],
        1.0,
        "image-replace-same-size",
        output,
        results,
    )?;
    let mut tx = Transaction::default();
    tx.opacity(node, 0.5);
    tx.clip(node, None);
    tree.commit(tx, BackendCapabilities::default())?;
    frame = tree.frame([128.0, 96.0])?;
    assert_eq!(
        renderer
            .submit(&frame, [128, 96], 1.0)?
            .unwrap()
            .gpu
            .rasterized_layers,
        1
    );
    compare(
        renderer,
        &frame,
        [128, 96],
        1.0,
        "image-isolated",
        output,
        results,
    )?;
    let mut tx = Transaction::default();
    tx.opacity(node, 0.25);
    tree.commit(tx, BackendCapabilities::default())?;
    frame = tree.frame([128.0, 96.0])?;
    let alpha = renderer.submit(&frame, [128, 96], 1.0)?.unwrap();
    assert_eq!(
        (
            alpha.gpu.rasterized_layers,
            alpha.gpu.reused_layers,
            alpha.gpu.image_upload_bytes
        ),
        (0, 1, 0)
    );
    compare(
        renderer,
        &frame,
        [128, 96],
        1.0,
        "image-opacity-cache",
        output,
        results,
    )?;
    set(&mut tree, node, Some(a.clone()))?;
    frame = tree.frame([128.0, 96.0])?;
    let replacement = renderer.submit(&frame, [128, 96], 1.0)?.unwrap();
    assert_eq!(
        replacement.gpu.rasterized_layers, 1,
        "image identity invalidates cached content"
    );
    assert_eq!(
        replacement.gpu.image_upload_bytes, 0,
        "second node already retains A"
    );
    compare(
        renderer,
        &frame,
        [128, 96],
        1.0,
        "image-resource-invalidates-layer",
        output,
        results,
    )?;
    set(
        &mut tree,
        node,
        Some(ImageResource::rgba8([4, 2], vec![255; 32])?),
    )?;
    frame = tree.frame([128.0, 96.0])?;
    assert_eq!(frame.layout_runs, 1);
    renderer.submit(&frame, [128, 96], 1.0)?;
    compare(
        renderer,
        &frame,
        [128, 96],
        1.0,
        "image-intrinsic-change",
        output,
        results,
    )?;
    set(&mut tree, node, None)?;
    frame = tree.frame([128.0, 96.0])?;
    renderer.submit(&frame, [128, 96], 1.0)?;
    compare(
        renderer,
        &frame,
        [128, 96],
        1.0,
        "image-detach",
        output,
        results,
    )?;
    // Repeated replacements and immediate removals can overlap GPU completion.
    let final_resource = ImageResource::rgba8([2, 2], vec![255; 16])?;
    let weak = Arc::downgrade(final_resource.pixels());
    let mut final_resource = Some(final_resource);
    for i in 0..24 {
        set(
            &mut tree,
            node,
            Some(if i == 23 {
                final_resource.take().unwrap()
            } else {
                resource(i % 2 == 0)
            }),
        )?;
        frame = tree.frame([128.0, 96.0])?;
        assert!(
            renderer
                .submit(&frame, [128, 96], 1.0)?
                .unwrap()
                .gpu
                .in_flight
                <= 3
        );
    }
    assert!(weak.upgrade().is_some());
    compare(
        renderer,
        &frame,
        [128, 96],
        1.0,
        "image-in-flight-replacement",
        output,
        results,
    )?;
    set(&mut tree, node, None)?;
    // Skip this base before adding the image again.
    tree.frame([128.0, 96.0])?;
    set(&mut tree, node, Some(a.clone()))?;
    frame = tree.frame([128.0, 96.0])?;
    assert!(
        renderer
            .submit(&frame, [128, 96], 1.0)?
            .unwrap()
            .full_resync
    );
    compare(
        renderer,
        &frame,
        [128, 96],
        1.0,
        "image-skipped-base",
        output,
        results,
    )?;
    renderer.finish()?;
    assert!(
        weak.upgrade().is_none(),
        "finished packets and evicted textures release pixel owners"
    );
    renderer.reset()?;
    let reset = renderer.submit(&frame, [192, 144], 1.5)?.unwrap();
    assert_eq!(
        (reset.gpu.image_uploads, reset.gpu.image_upload_bytes),
        (1, 16)
    );
    compare(
        renderer,
        &frame,
        [192, 144],
        1.5,
        "image-device-epoch-scale",
        output,
        results,
    )?;
    // Content insets are independent of the background/border quad geometry.
    let mut style = tree.style(node)?.clone();
    style.padding = taffy::Rect {
        left: length(4.0),
        right: length(4.0),
        top: length(4.0),
        bottom: length(4.0),
    };
    let mut tx = Transaction::default();
    tx.style(node, style);
    tx.opacity(node, 1.0);
    tree.commit(tx, BackendCapabilities::default())?;
    frame = tree.frame([128.0, 96.0])?;
    renderer.submit(&frame, [128, 96], 1.0)?;
    let pixels = renderer.image()?;
    assert_eq!(pixels.get_pixel(13, 9).0, [0, 0, 0, 255]);
    assert_eq!(pixels.get_pixel(17, 13).0, [255, 0, 0, 255]);
    compare(
        renderer,
        &frame,
        [128, 96],
        1.0,
        "image-content-insets",
        output,
        results,
    )?;
    let cover = tree.append(tree.root(), absolute(16.0, 12.0, 8.0, 8.0), 0xdd88aaff)?;
    frame = tree.frame([128.0, 96.0])?;
    renderer.submit(&frame, [128, 96], 1.0)?;
    assert_eq!(
        renderer.image()?.get_pixel(18, 14).0,
        [221, 136, 170, 255],
        "later quad occludes an earlier image"
    );
    compare(
        renderer,
        &frame,
        [128, 96],
        1.0,
        "image-mixed-paint-order",
        output,
        results,
    )?;
    tree.remove(cover)?;
    let visible_style = tree.style(node)?.clone();
    let mut hidden_style = visible_style.clone();
    hidden_style.display = taffy::Display::None;
    let mut tx = Transaction::default();
    tx.style(node, hidden_style);
    tree.commit(tx, BackendCapabilities::default())?;
    frame = tree.frame([128.0, 96.0])?;
    renderer.submit(&frame, [128, 96], 1.0)?;
    assert_eq!(renderer.image()?.get_pixel(17, 13).0, [0, 0, 0, 255]);
    compare(
        renderer,
        &frame,
        [128, 96],
        1.0,
        "image-display-none",
        output,
        results,
    )?;
    let mut tx = Transaction::default();
    tx.style(node, visible_style);
    tree.commit(tx, BackendCapabilities::default())?;
    frame = tree.frame([128.0, 96.0])?;
    let restored = renderer.submit(&frame, [128, 96], 1.0)?.unwrap();
    assert_eq!(
        restored.gpu.image_upload_bytes, 0,
        "visible sibling retains the texture while node is hidden"
    );
    compare(
        renderer,
        &frame,
        [128, 96],
        1.0,
        "image-display-restore",
        output,
        results,
    )?;
    Ok(())
}

pub(super) fn benchmark() -> Result<serde_json::Value, Box<dyn Error>> {
    let mut cases = vec![];
    for edge in [256u32, 1024] {
        let pixels: Arc<[u8]> = vec![90u8; (edge * edge * 4) as usize].into();
        let a = ImageResource::rgba8([edge, edge], pixels.clone())?;
        let b = ImageResource::rgba8([edge, edge], pixels)?;
        let mut tree = UiTree::new(absolute(0.0, 0.0, 640.0, 360.0), 0x171d28ff)?;
        let node = tree.append(tree.root(), absolute(10.0, 10.0, 256.0, 256.0), 0)?;
        set(&mut tree, node, Some(a.clone()))?;
        let mut renderer = WgpuFrameRenderer::new(3)?;
        renderer.submit(&tree.frame([640.0, 360.0])?, [640, 360], 1.0)?;
        renderer.finish()?;
        let mut samples = [vec![], vec![]];
        let mut bytes = [vec![], vec![]];
        let mut round_medians = [vec![], vec![]];
        let mut baseline_pixels = None;
        for round in 0..3 {
            for mode in if round % 2 == 0 { [0, 1] } else { [1, 0] } {
                let mut round_samples = vec![];
                for i in 0..26 {
                    let mut tx = Transaction::default();
                    tx.translation(node, [(i % 2) as f32, 0.0]);
                    tx.image(
                        node,
                        Some(if mode == 1 && i % 2 == 0 {
                            b.clone()
                        } else {
                            a.clone()
                        }),
                    );
                    tree.commit(tx, BackendCapabilities::default())?;
                    let frame = tree.frame([640.0, 360.0])?;
                    let start = Instant::now();
                    let submission = renderer.submit(&frame, [640, 360], 1.0)?;
                    renderer.finish()?;
                    if i >= 6 {
                        let submission = submission.expect("measured frame must change");
                        let elapsed = start.elapsed().as_secs_f64() * 1e6;
                        samples[mode].push(elapsed);
                        round_samples.push(elapsed);
                        bytes[mode].push(submission.gpu.image_upload_bytes);
                    }
                }
                round_samples.sort_by(f64::total_cmp);
                round_medians[mode].push(round_samples[round_samples.len() / 2]);
                // Outside all timed samples; both modes end at the same offset.
                let actual = renderer.image()?;
                if let Some(reference) = &baseline_pixels {
                    assert_eq!(
                        actual.as_raw(),
                        reference,
                        "image reuse/reupload A/B pixels"
                    );
                } else {
                    baseline_pixels = Some(actual.into_raw());
                }
            }
        }
        for mode in 0..2 {
            samples[mode].sort_by(f64::total_cmp);
            assert!(bytes[mode].iter().all(|b| *b
                == if mode == 0 {
                    0
                } else {
                    u64::from(edge) * u64::from(edge) * 4
                }));
            cases.push(serde_json::json!({"texture_edge":edge,"mode":if mode==0 {"reuse immutable texture"} else {"replace/reupload identical decoded pixels"},"samples":samples[mode].len(),"median_us":samples[mode][samples[mode].len()/2],"p95_us":samples[mode][samples[mode].len()*95/100],"texture_bytes_per_frame":bytes[mode][0],"sorted_samples_us":samples[mode],"round_medians_us":round_medians[mode],"pixel_equal":true}));
        }
    }
    Ok(
        serde_json::json!({"scope":"FramePacket submit + GPU completion; full redraw both modes; decoded bytes and UI snapshot preparation excluded; same pixels and translated geometry; no readback/presentation; 3 rotating rounds, 6 warmups + 20 samples per mode per round","cases":cases}),
    )
}
