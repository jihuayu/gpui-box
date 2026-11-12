//! Owned page/delta publication through normal renderer, hits and old snapshots.
use super::*;
use std::sync::Arc;
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
    for copying in [false, true] {
        for scale in [1.0, 1.25, 1.5] {
            let (context, mut raster) = retained_text::context()?;
            let text = retained_text::text(&context, &mut raster, "Pages 中文", scale)?;
            let mut tree = UiTree::new(absolute(0.0, 0.0, 192.0, 160.0), 0x171d28ff)?;
            tree.set_frame_build_policy(if copying {
                FrameBuildPolicy::SelectiveCopy
            } else {
                FrameBuildPolicy::Selective
            });
            let group = tree.append(tree.root(), absolute(12.0, 8.0, 168.0, 128.0), 0)?;
            let mut nodes = vec![];
            for index in 0..150 {
                nodes.push(tree.append(
                    group,
                    absolute(
                        (index % 15) as f32 * 9.0,
                        (index / 15) as f32 * 8.0,
                        6.0,
                        6.0,
                    ),
                    0x4488ccaa,
                )?);
            }
            let outside =
                tree.append(tree.root(), absolute(2.0, 140.0, 160.0, 12.0), 0x884422ff)?;
            let image = ImageResource::rgba8([1, 1], vec![255, 128, 0, 128])?;
            let mut tx = Transaction::default();
            tx.image(nodes[149], Some(image));
            tx.text(
                nodes[148],
                Some(text::TextPaint {
                    resource: text,
                    color: 0xffdd88ff,
                }),
            );
            tx.shadows(
                nodes[30],
                vec![BoxShadow {
                    color: 0x00000088,
                    offset: [2.0, 1.0],
                    blur_radius: 2.0,
                    ..Default::default()
                }],
            );
            tree.commit(tx, BackendCapabilities::default())?;
            let mut gpu = WgpuFrameRenderer::new(3)?;
            gpu.set_policy(CompositorPolicy::Damage)?;
            let size = [(192.0 * scale) as u32, (160.0 * scale) as u32];
            let mut saved = None;
            for step in 0..12 {
                let mut tx = Transaction::default();
                match step {
                    0 => {}
                    1 => tx.translation(group, [2.5, 3.25]),
                    2 => tx.color(nodes[70], 0xee4455ff),
                    3 => {
                        for node in &nodes[62..130] {
                            tx.color(*node, 0x44cc8888);
                        }
                    }
                    4 => {
                        tx.translation(group, [6.25, 5.5]);
                        tx.color(outside, 0x8844ffff);
                        tx.image(
                            nodes[149],
                            Some(ImageResource::rgba8([1, 1], vec![0, 255, 128, 128])?),
                        );
                    }
                    5 => tx.clip(
                        group,
                        Some(Rect {
                            x: 4.0,
                            y: 4.0,
                            width: 80.0,
                            height: 64.0,
                        }),
                    ),
                    6 => tx.opacity(group, 0.6),
                    7 => {
                        tx.style(nodes[11], absolute(99.0, 0.0, 9.0, 6.0));
                        tx.color(nodes[11], 0xff8844ff);
                        tx.style(outside, absolute(2.0, 140.0, 164.0, 12.0));
                        tx.translation(nodes[140], [-2.5, 1.0]);
                    }
                    8 => {
                        tx.clip(group, None);
                        tx.scroll(group, [2.0; 2]);
                    }
                    9 => {
                        let mut order = nodes.clone();
                        order.reverse();
                        tx.reorder(group, order);
                    }
                    10 => {
                        let mut skip = Transaction::default();
                        skip.color(nodes[0], 0x9988ffff);
                        tree.commit(skip, BackendCapabilities::default())?;
                        tree.frame([192.0, 160.0])?;
                        tx.translation(group, [3.25, -1.5]);
                    }
                    _ => {
                        tree.remove(nodes[30])?;
                        tree.append(group, absolute(20.0, 88.0, 24.0, 16.0), 0xff8844ff)?;
                    }
                }
                tree.commit(
                    tx,
                    BackendCapabilities {
                        solid_color_patch: true,
                    },
                )?;
                let frame = tree.frame([192.0, 160.0])?;
                if step == 0 {
                    saved = Some(frame.clone());
                    frame.hit_test(20.0, 20.0);
                }
                let submit = gpu.submit(&frame, size, scale)?.unwrap();
                if step == 10 {
                    assert!(submit.full_resync);
                }
                if copying {
                    assert_eq!(submit.validation.validated_delta_items, frame.updates.len());
                } else {
                    assert_eq!(submit.validation.shared_delta_items, frame.updates.len());
                    assert_eq!(frame.build_stats.delta_cloned_items, 0);
                }
                if step == 3 && !copying {
                    assert!(frame.build_stats.replaced_pages > 0);
                }
                let point = [20.0, 20.0];
                let expected = frame
                    .items
                    .iter()
                    .rev()
                    .find(|item| {
                        let b = item.visible_bounds();
                        point[0] >= b.x
                            && point[1] >= b.y
                            && point[0] < b.x + b.width
                            && point[1] < b.y + b.height
                    })
                    .map(|item| item.node);
                assert_eq!(frame.hit_test(point[0], point[1]), expected);
                compare(
                    &mut gpu,
                    &frame,
                    size,
                    scale,
                    &format!("publication-{copying}-{scale}-{step}"),
                    output,
                    results,
                )?;
                let value = results.last_mut().unwrap();
                value["copied_items"] = frame.build_stats.copied_items.into();
                value["replaced_pages"] = frame.build_stats.replaced_pages.into();
                value["moved_items"] = frame.build_stats.moved_items.into();
                value["delta_cloned_items"] = frame.build_stats.delta_cloned_items.into();
                value["validated_delta_items"] = submit.validation.validated_delta_items.into();
                value["shared_delta_items"] = submit.validation.shared_delta_items.into();
            }
            let old = saved.unwrap();
            drop(tree);
            assert_eq!(old.items[1].spatial.translation, [0.0; 2]);
            let mut fresh = WgpuFrameRenderer::new(1)?;
            fresh.submit(&old, size, scale)?;
            compare(
                &mut fresh,
                &old,
                size,
                scale,
                &format!("publication-old-{copying}-{scale}"),
                output,
                results,
            )?;
            let before = fresh.image()?;
            let mut malformed = old.clone();
            Arc::make_mut(&mut malformed.updates)[0].fill_color = 0xff0000ff;
            assert!(fresh.submit(&malformed, size, scale).is_err());
            assert!(fresh.submit(&old, size, scale)?.is_none());
            assert_eq!(before.as_raw(), fresh.image()?.as_raw());
            results.push(serde_json::json!({"case":format!("publication-mutation-{copying}-{scale}"),"mutated_delta_rejected":true,"valid_equal_retry":true,"unchanged_pixels":true}));
        }
    }
    Ok(())
}
