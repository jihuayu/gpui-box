//! Incremental image ownership, backend rejection/recovery and paired timing.
use super::*;
use gpui_wgpu::{ImageUpdates, QuadBatch, QuadDraw, QuadImage, QuadRedraw, RetainedQuadRenderer};
use std::{sync::Arc, time::Instant};

fn backend_checks() -> Result<(), Box<dyn Error>> {
    let a: Arc<[u8]> = vec![255, 0, 0, 255].into();
    let b: Arc<[u8]> = vec![0, 255, 0, 255].into();
    let draw = |id, resource, rgba: Arc<[u8]>, x| QuadImage {
        id: [77, id, 0],
        resource,
        rgba,
        size: [1, 1],
        bounds: [x, 0.0, 4.0, 4.0],
        clip: [0.0, 0.0, 32.0, 16.0],
    };
    let batch = |images, order| QuadBatch {
        capacity: 0,
        writes: vec![],
        order: Some(order),
        layers: vec![],
        images,
        glyphs: Default::default(),
        cache_layers: true,
        size: gpui::size(gpui::DevicePixels(32), gpui::DevicePixels(16)),
        redraw: QuadRedraw::Full,
    };
    let order = || vec![QuadDraw::Image([77, 0, 0]), QuadDraw::Image([77, 1, 0])];
    let mut renderer = RetainedQuadRenderer::new(3)?;
    let initial = renderer.submit(batch(
        ImageUpdates {
            reset: true,
            removed: vec![],
            images: vec![draw(0, 101, a.clone(), 0.0), draw(1, 102, b.clone(), 8.0)],
        },
        order(),
    ))?;
    assert_eq!((initial.image_uploads, initial.image_batch_updates), (2, 2));
    let swapped = renderer.submit(batch(
        ImageUpdates {
            reset: false,
            removed: vec![],
            images: vec![draw(0, 102, b.clone(), 0.0), draw(1, 101, a.clone(), 8.0)],
        },
        order(),
    ))?;
    assert_eq!((swapped.image_uploads, swapped.image_batch_updates), (0, 2));
    assert_eq!(renderer.image()?.get_pixel(1, 1).0, [0, 255, 0, 255]);
    assert_eq!(renderer.image()?.get_pixel(9, 1).0, [255, 0, 0, 255]);
    // Definitions remain immutable even if this transaction would remove all
    // previous owners. Validation rejects before any draw/reference mutation.
    for updates in [
        ImageUpdates {
            images: vec![draw(0, 101, vec![0; 4].into(), 0.0)],
            ..Default::default()
        },
        ImageUpdates {
            images: vec![draw(0, 102, b.clone(), 0.0), draw(0, 102, b.clone(), 0.0)],
            ..Default::default()
        },
        ImageUpdates {
            removed: vec![[77, 0, 0]],
            images: vec![draw(0, 102, b.clone(), 0.0)],
            ..Default::default()
        },
        ImageUpdates {
            removed: vec![[77, 0, 0], [77, 0, 0]],
            ..Default::default()
        },
        ImageUpdates {
            images: vec![draw(0, 102, b.clone(), f32::NAN)],
            ..Default::default()
        },
        ImageUpdates {
            images: vec![QuadImage {
                size: [2, 2],
                ..draw(0, 102, b.clone(), 0.0)
            }],
            ..Default::default()
        },
        ImageUpdates {
            reset: true,
            removed: vec![[77, 0, 0]],
            ..Default::default()
        },
    ] {
        assert!(renderer.submit(batch(updates, order())).is_err());
    }
    let shared = renderer.submit(batch(
        ImageUpdates {
            images: vec![draw(1, 102, b.clone(), 8.0)],
            ..Default::default()
        },
        order(),
    ))?;
    assert_eq!(shared.image_uploads, 0);
    let retained = renderer.submit(batch(
        ImageUpdates {
            removed: vec![[77, 0, 0]],
            ..Default::default()
        },
        vec![QuadDraw::Image([77, 1, 0])],
    ))?;
    assert_eq!(
        (
            retained.image_batch_updates,
            retained.image_uploads,
            retained.image_geometry_bytes
        ),
        (0, 0, 0)
    );
    assert_eq!(renderer.image()?.get_pixel(9, 1).0, [0, 255, 0, 255]);
    let mut resized = batch(ImageUpdates::default(), vec![QuadDraw::Image([77, 1, 0])]);
    resized.size = gpui::size(gpui::DevicePixels(64), gpui::DevicePixels(32));
    let resize = renderer.submit(resized)?;
    assert_eq!(
        (
            resize.image_batch_updates,
            resize.image_uploads,
            resize.image_geometry_bytes
        ),
        (0, 0, 48)
    );
    assert_eq!(renderer.image()?.get_pixel(9, 1).0, [0, 255, 0, 255]);
    // Reset drops the last owner; restoring that ID really uploads again.
    renderer.submit(batch(
        ImageUpdates {
            reset: true,
            ..Default::default()
        },
        vec![],
    ))?;
    let restore = renderer.submit(batch(
        ImageUpdates {
            images: vec![draw(1, 102, b, 8.0)],
            ..Default::default()
        },
        vec![QuadDraw::Image([77, 1, 0])],
    ))?;
    assert_eq!(restore.image_uploads, 1);
    renderer.finish()?;
    Ok(())
}

fn fixture(count: usize) -> Result<(UiTree, Vec<NodeId>, NodeId), Box<dyn Error>> {
    let mut tree = UiTree::new(
        Style {
            size: taffy::Size {
                width: length(800.0),
                height: length(600.0),
            },
            ..Default::default()
        },
        0x000000ff,
    )?;
    let resource = ImageResource::rgba8(
        [2, 2],
        vec![
            255, 0, 0, 255, 0, 255, 0, 128, 0, 0, 255, 255, 255, 255, 255, 255,
        ],
    )?;
    let mut nodes = vec![];
    let mut tx = Transaction::default();
    for i in 0..count {
        let node = tree.append(
            tree.root(),
            Style {
                position: taffy::Position::Absolute,
                inset: taffy::Rect {
                    left: length((i % 40) as f32 * 18.0),
                    top: length((i / 40) as f32 * 18.0),
                    right: taffy::LengthPercentageAuto::auto(),
                    bottom: taffy::LengthPercentageAuto::auto(),
                },
                size: taffy::Size {
                    width: length(12.0),
                    height: length(12.0),
                },
                ..Default::default()
            },
            0,
        )?;
        tx.image(node, Some(resource.clone()));
        nodes.push(node);
    }
    let background = tree.append(
        tree.root(),
        Style {
            position: taffy::Position::Absolute,
            size: taffy::Size {
                width: length(8.0),
                height: length(8.0),
            },
            ..Default::default()
        },
        0xff000080,
    )?;
    tree.commit(tx, BackendCapabilities::default())?;
    Ok((tree, nodes, background))
}

pub(super) fn run(
    renderer: &mut WgpuFrameRenderer,
    output: &std::path::Path,
    results: &mut Vec<serde_json::Value>,
) -> Result<(), Box<dyn Error>> {
    backend_checks()?;
    let (mut tree, nodes, background) = fixture(80)?;
    renderer.set_policy(CompositorPolicy::Damage)?;
    renderer.set_resource_deltas_enabled(true);
    let first = tree.frame([800.0, 600.0])?;
    renderer.submit(&first, [800, 600], 1.0)?;
    let mut tx = Transaction::default();
    tx.color(background, 0x00ff0080);
    tree.commit(
        tx,
        BackendCapabilities {
            solid_color_patch: true,
        },
    )?;
    let frame = tree.frame([800.0, 600.0])?;
    let submit = renderer.submit(&frame, [800, 600], 1.0)?.unwrap();
    assert_eq!(
        (
            submit.gpu.image_batch_updates,
            submit.gpu.image_uploads,
            submit.gpu.image_geometry_bytes
        ),
        (0, 0, 0)
    );
    compare(
        renderer,
        &frame,
        [800, 600],
        1.0,
        "image-delta-unrelated-color",
        output,
        results,
    )?;
    let mut tx = Transaction::default();
    tx.translation(nodes[10], [1.5, 0.5]);
    tree.commit(tx, BackendCapabilities::default())?;
    let frame = tree.frame([800.0, 600.0])?;
    let submit = renderer.submit(&frame, [800, 600], 1.0)?.unwrap();
    assert_eq!(
        (
            submit.gpu.image_batch_updates,
            submit.gpu.image_uploads,
            submit.gpu.image_geometry_bytes
        ),
        (1, 0, 48)
    );
    compare(
        renderer,
        &frame,
        [800, 600],
        1.0,
        "image-delta-one-move",
        output,
        results,
    )?;
    tree.remove(nodes[10])?;
    let frame = tree.frame([800.0, 600.0])?;
    let submit = renderer.submit(&frame, [800, 600], 1.0)?.unwrap();
    assert_eq!(submit.gpu.image_uploads, 0);
    compare(
        renderer,
        &frame,
        [800, 600],
        1.0,
        "image-delta-shared-removal",
        output,
        results,
    )?;
    let mut tx = Transaction::default();
    tx.translation(nodes[11], [2.0, 0.0]);
    tree.commit(tx, BackendCapabilities::default())?;
    tree.frame([800.0, 600.0])?;
    let mut tx = Transaction::default();
    tx.translation(nodes[12], [1.0, 0.0]);
    tree.commit(tx, BackendCapabilities::default())?;
    let frame = tree.frame([800.0, 600.0])?;
    let submit = renderer.submit(&frame, [800, 600], 1.0)?.unwrap();
    assert!(submit.full_resync);
    assert_eq!(
        (submit.gpu.image_batch_updates, submit.gpu.image_uploads),
        (79, 0)
    );
    compare(
        renderer,
        &frame,
        [800, 600],
        1.0,
        "image-delta-resync-reuse",
        output,
        results,
    )?;
    Ok(())
}

pub(super) fn benchmark() -> Result<serde_json::Value, Box<dyn Error>> {
    let mut cases = vec![];
    for count in [80, 800] {
        for kind in ["unrelated-color", "one-image-move"] {
            let (mut tree, nodes, background) = fixture(count)?;
            let mut renderer = WgpuFrameRenderer::new(1)?;
            renderer.set_policy(CompositorPolicy::Damage)?;
            renderer.submit(&tree.frame([800.0, 600.0])?, [800, 600], 1.0)?;
            renderer.finish()?;
            let mut cpu = [vec![], vec![]];
            let mut complete = [vec![], vec![]];
            let mut rounds = [vec![], vec![]];
            let mut baseline = None;
            for round in 0..3 {
                for mode in if round % 2 == 0 { [0, 1] } else { [1, 0] } {
                    renderer.set_resource_deltas_enabled(mode == 0);
                    let mut samples = vec![];
                    for i in 0..26 {
                        let mut tx = Transaction::default();
                        if kind == "unrelated-color" {
                            tx.color(background, if i % 2 == 0 { 0x00ff0080 } else { 0xff000080 });
                        } else {
                            tx.translation(nodes[count / 2], [((i + 1) % 2) as f32, 0.0]);
                        }
                        tree.commit(
                            tx,
                            BackendCapabilities {
                                solid_color_patch: true,
                            },
                        )?;
                        let frame = tree.frame([800.0, 600.0])?;
                        let start = Instant::now();
                        let submit = renderer.submit(&frame, [800, 600], 1.0)?.unwrap();
                        let us = start.elapsed().as_secs_f64() * 1e6;
                        renderer.finish()?;
                        let completed = start.elapsed().as_secs_f64() * 1e6;
                        if i >= 6 {
                            assert_eq!(
                                submit.gpu.image_batch_updates,
                                if mode == 1 {
                                    count
                                } else if kind == "unrelated-color" {
                                    0
                                } else {
                                    1
                                }
                            );
                            assert_eq!(submit.gpu.image_upload_bytes, 0);
                            assert_eq!(
                                submit.gpu.image_geometry_bytes,
                                if kind == "unrelated-color" { 0 } else { 48 }
                            );
                            cpu[mode].push(us);
                            complete[mode].push(completed);
                            samples.push(us);
                        }
                    }
                    samples.sort_by(f64::total_cmp);
                    rounds[mode].push(samples[samples.len() / 2]);
                    let image = renderer.image()?;
                    let frame = tree.published_frame().unwrap();
                    assert_eq!(
                        image.as_raw(),
                        renderer.reference_image(frame, [800, 600], 1.0)?.as_raw()
                    );
                    if let Some(old) = &baseline {
                        assert_eq!(image.as_raw(), old);
                    } else {
                        baseline = Some(image.into_raw());
                    }
                }
            }
            for mode in 0..2 {
                cpu[mode].sort_by(f64::total_cmp);
                complete[mode].sort_by(f64::total_cmp);
                cases.push(serde_json::json!({"image_draws":count,"case":kind,"policy":if mode==0 {"base-relative image updates"} else {"full image list reset"},"samples":cpu[mode].len(),"submit_median_us":cpu[mode][cpu[mode].len()/2],"submit_p95_us":cpu[mode][cpu[mode].len()*95/100],"submit_and_completion_median_us":complete[mode][complete[mode].len()/2],"image_batch_updates":if mode==1 {count} else if kind=="unrelated-color" {0} else {1},"image_upload_bytes":0,"geometry_bytes":if kind=="unrelated-color" {0} else {48},"round_submit_medians_us":rounds[mode],"submit_sorted_samples_us":cpu[mode],"completed_sorted_samples_us":complete[mode],"pixel_equal":true}));
            }
        }
    }
    Ok(
        serde_json::json!({"scope":"Same persistent tree and WGPU consumer, delta images vs full image list/reset with warm shared texture. 80/800 ordered image draws, one unrelated color or one image translation. Damage policy, 800x600 target, one frame in flight; 3 rotating rounds, 6 warmups +20 samples per mode/round. Timed submit includes remaining complete composition planning, live draw ID checks and completion leases. UI publication/setup/readback/presentation excluded. Pixels compare with fresh uncached full renderer outside timing.","cases":cases}),
    )
}
