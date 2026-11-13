//! Local snapshot route, independent complete extraction/reference pixels and
//! explicit skipped-base/device/consumer recovery for mixed resources/effects.
use super::*;

fn fixed(x: f32, y: f32, w: f32, h: f32) -> Style {
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
    let (text_system, mut raster) = retained_text::context()?;
    for scale in [1.0, 1.25, 1.5] {
        for shared in [false, true] {
            let mut tree = UiTree::new(fixed(0.0, 0.0, 160.0, 120.0), 0x102030ff)?;
            let parent = tree.append(tree.root(), fixed(8.0, 6.0, 140.0, 100.0), 0x304050ff)?;
            let child = tree.append(parent, fixed(2.0, 3.0, 80.0, 60.0), 0x4488cc88)?;
            let image = tree.append(child, fixed(5.0, 6.0, 20.0, 16.0), 0)?;
            let text = tree.append(child, fixed(4.0, 24.0, 72.0, 30.0), 0)?;
            let mut tx = Transaction::default();
            tx.opacity(parent, 0.85);
            tx.opacity(child, 0.65);
            tx.shadows(
                child,
                vec![BoxShadow {
                    color: 0x00000088,
                    blur_radius: 4.0,
                    offset: [2.0, 3.0],
                    ..Default::default()
                }],
            );
            tx.image(
                image,
                Some(ImageResource::rgba8(
                    [2, 2],
                    vec![
                        240, 80, 40, 255, 40, 220, 80, 128, 40, 80, 240, 200, 220, 220, 40, 255,
                    ],
                )?),
            );
            let text_resource = retained_text::text(&text_system, &mut raster, "局部 GPU", scale)?;
            tx.text(
                text,
                Some(gpui_box::retained::text::TextPaint {
                    resource: text_resource.clone(),
                    color: 0xffcc88ff,
                }),
            );
            tree.commit(tx, BackendCapabilities::default())?;
            let mut deep = parent;
            for _ in 0..38 {
                deep = tree.append(deep, fixed(0.125, 0.25, 4.0, 3.0), 0x6688aacc)?;
            }
            let mut gpu = WgpuFrameRenderer::new(3)?;
            gpu.set_shared_spatial_enabled(shared);
            gpu.set_local_tight_layer_bounds_enabled(std::env::args().any(|v|v=="--local-tight-layers"));
            let mut saved = vec![];
            for round in 0..14 {
                let mut tx = Transaction::default();
                if round != 11 {
                tx.translation(parent, [round as f32 * 0.375, round as f32 * 0.125]);
                tx.translation(child, [-round as f32 * 0.125, 0.0]);
                tx.clip(
                    parent,
                    (round % 3 != 0).then_some(Rect {
                        x: 1.0,
                        y: 2.0,
                        width: 120.0,
                        height: 90.0,
                    }),
                );
                tx.scroll(parent, if round % 2 == 0 { [0.0; 2] } else { [0.5, 1.0] });
                } else {
                    tx.color(child, 0xbb8844cc);
                }
                if round == 5 {
                    tx.opacity(child, 0.45);
                }
                if round == 3 || round == 4 {
                    tx.color(child, if round == 3 { 0xaa4488bb } else { 0x44aa88bb });
                }
                if round == 7 {
                    tx.color(child, 0x88cc4488);
                    tx.decoration(child, gpui_box::retained::QuadDecoration {
                        radius: 7.0, border_width: 2.0, border_color: 0xcc8844ff,
                    });
                    tx.shadows(child, vec![BoxShadow {
                        color: 0x3355aa99, blur_radius: 6.0, offset: [-3.0, 2.0], ..Default::default()
                    }]);
                    tx.text(text, Some(gpui_box::retained::text::TextPaint {
                        resource: text_resource.clone(),
                        color: 0x88ffccaa,
                    }));
                }
                if round == 10 {
                    tx.image(image, None);
                }
                if round == 8 {
                    tx.image(image, Some(ImageResource::rgba8([2, 2], [20, 220, 140, 180].repeat(4))?));
                }
                if round == 9 {
                    // Shadow insertion changes painter topology and must take
                    // complete recovery, even though owner material pages stay local.
                    tx.shadows(child, vec![BoxShadow { color: 0x00000088, blur_radius: 3.0, ..Default::default() },
                        BoxShadow { color: 0xff000088, inset: true, ..Default::default() }]);
                }
                tree.commit(tx, BackendCapabilities::default())?;
                let packet = tree.local_frame([160.0, 120.0])?;
                if round == 3 || round == 4 {
                    saved.push(packet);
                    continue;
                }
                let surface = [(160.0 * scale) as u32, (120.0 * scale) as u32];
                let submission = gpu.submit_local(&packet, surface, scale)?.unwrap();
                if round == 7 && shared {
                    assert!(!submission.full_resync, "material edits retain exact predecessor");
                    assert!(packet.stats().material_nodes >= 2);
                    assert!(submission.routing.prepared_quad_items > 0);
                    assert!(submission.routing.prepared_shadow_items > 0);
                }
                if round == 8 && shared { assert!(!submission.full_resync); }
                if round == 9 { assert!(submission.full_resync); }
                if round == 11 && shared {
                    assert!(!submission.full_resync);
                    assert_eq!(submission.gpu.spatial_upload_bytes, 0);
                    assert!(submission.gpu.uploaded_bytes > 0);
                }
                assert!(gpu.submit_local(&packet, surface, scale)?.is_none());
                if round == 6 {
                    gpu.reset()?;
                    assert!(
                        gpu.submit_local(&packet, surface, scale)?
                            .unwrap()
                            .full_resync
                    );
                }
                let resolved = packet.resolve();
                let actual = gpu.image()?;
                let expected = gpu.reference_image(&resolved, surface, scale)?;
                assert_eq!(
                    actual.as_raw(),
                    expected.as_raw(),
                    "local pixels: round={round} shared={shared} scale={scale}"
                );
                results.push(serde_json::json!({"case":format!("local-mixed-{scale}-{shared}-{round}"),"pixel_equal":true,"local_changed":packet.stats().changed_nodes,"material_changed":packet.stats().material_nodes,"full_resync":submission.full_resync,"spatial_bytes":submission.gpu.spatial_upload_bytes}));
                if round == 13 {
                    let explicit = tree.frame([160.0, 120.0])?;
                    // Same revision may change representation, never content.
                    let mut malformed = explicit.clone();
                    std::sync::Arc::make_mut(&mut malformed.items)[0].bounds.x += 1.0;
                    assert!(gpu.submit(&malformed, surface, scale).is_err());
                    assert!(gpu.submit(&explicit, surface, scale)?.is_none());
                }
            }
            assert!(
                gpu.submit_local(
                    &saved[0],
                    [(160.0 * scale) as u32, (120.0 * scale) as u32],
                    scale
                )
                .is_err()
            );
            let old = saved[0].resolve();
            drop(tree);
            let mut independent = WgpuFrameRenderer::new(1)?;
            independent.set_local_tight_layer_bounds_enabled(std::env::args().any(|v|v=="--local-tight-layers"));
            independent.submit_local(
                &saved[0],
                [(160.0 * scale) as u32, (120.0 * scale) as u32],
                scale,
            )?;
            assert_eq!(
                independent.image()?.as_raw(),
                independent
                    .reference_image(
                        &old,
                        [(160.0 * scale) as u32, (120.0 * scale) as u32],
                        scale
                    )?
                    .as_raw()
            );
        }
    }
    std::fs::write(
        output.join("local-validation-count.txt"),
        format!("{} mixed route comparisons\n", results.len()),
    )?;
    Ok(())
}
