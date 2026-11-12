//! Same immutable packets through cached and original consumer preparation.
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
        for shared in [true, false] {
            for scale in [1.0, 1.25, 1.5] {
                let (context, mut raster) = retained_text::context()?;
                let text = retained_text::text(&context, &mut raster, "Routing 中文", scale)?;
                let mut tree = UiTree::new(absolute(0.0, 0.0, 192.0, 144.0), 0x171d28ff)?;
                if copying {
                    tree.set_frame_build_policy(FrameBuildPolicy::SelectiveCopy);
                }
                let group = tree.append(tree.root(), absolute(8.0, 8.0, 168.0, 128.0), 0)?;
                let node = tree.append(group, absolute(8.0, 8.0, 64.0, 40.0), 0x4488ccff)?;
                let image = tree.append(group, absolute(92.0, 20.0, 24.0, 24.0), 0)?;
                let label = tree.append(group, absolute(16.0, 64.0, 124.0, 32.0), 0)?;
                let mut chain = vec![];
                let mut parent = group;
                for _ in 0..65 {
                    parent = tree.append(parent, absolute(0.0, 0.0, 128.0, 112.0), 0)?;
                    chain.push(parent);
                }
                tree.append(parent, absolute(4.0, 104.0, 16.0, 8.0), 0xff8844ff)?;
                let mut setup = Transaction::default();
                setup.image(
                    image,
                    Some(ImageResource::rgba8([1, 1], vec![255, 128, 0, 128])?),
                );
                setup.text(
                    label,
                    Some(text::TextPaint {
                        resource: text,
                        color: 0xffdd88ff,
                    }),
                );
                setup.decoration(
                    node,
                    QuadDecoration {
                        radius: 8.0,
                        border_width: 2.0,
                        border_color: 0xff8844ff,
                    },
                );
                setup.shadows(
                    node,
                    vec![BoxShadow {
                        color: 0x00000088,
                        offset: [2.0, 1.0],
                        blur_radius: 2.0,
                        ..Default::default()
                    }],
                );
                tree.commit(setup, BackendCapabilities::default())?;
                let mut renderers = [WgpuFrameRenderer::new(3)?, WgpuFrameRenderer::new(3)?];
                for (mode, gpu) in renderers.iter_mut().enumerate() {
                    gpu.set_shared_spatial_enabled(shared);
                    gpu.set_cached_primitive_routing_enabled(mode == 0);
                    gpu.set_policy(CompositorPolicy::Damage)?;
                }
                let size = [(192.0 * scale) as u32, (144.0 * scale) as u32];
                for step in 0..18 {
                    let mut tx = Transaction::default();
                    match step {
                        0 => {}
                        1 => tx.translation(group, [2.5, 3.25]),
                        2 => tx.color(node, 0xee4455ff),
                        3 => tx.clip(
                            group,
                            Some(Rect {
                                x: 1.5,
                                y: 2.25,
                                width: 94.0,
                                height: 96.0,
                            }),
                        ),
                        4 => tx.opacity(group, 0.6),
                        5 => tx.decoration(
                            node,
                            QuadDecoration {
                                radius: 12.0,
                                border_width: 1.5,
                                border_color: 0x88ff44ff,
                            },
                        ),
                        6 => tx.shadows(
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
                        ),
                        7 => tx.scroll(group, [4.25, 2.5]),
                        8 => tx.style(node, absolute(8.0, 8.0, 72.0, 46.0)),
                        9 => {
                            tx.image(
                                image,
                                Some(ImageResource::rgba8([1, 1], vec![0, 255, 128, 128])?),
                            );
                            tx.text(label, None);
                        }
                        10 => tx.shadows(node, vec![]),
                        11 => tx.color(node, 0x9988ffff),
                        12 => {
                            tree.edit(BackendCapabilities::default(), |edit| {
                                edit.reparent(chain[19], group)?;
                                Ok(())
                            })?;
                        }
                        13 => {
                            let mut skip = Transaction::default();
                            skip.translation(group, [5.0, 3.0]);
                            tree.commit(skip, BackendCapabilities::default())?;
                            tree.frame([192.0, 144.0])?;
                            tx.translation(group, [6.5, 4.25]);
                        }
                        14 => {
                            tree.remove(chain[0])?;
                            tree.append(group, absolute(120.0, 100.0, 16.0, 16.0), 0x8844ffff)?;
                        }
                        15 => {
                            for gpu in &mut renderers {
                                gpu.reset()?;
                            }
                            tx.clip(group, None);
                        }
                        16 => {
                            for (mode, gpu) in renderers.iter_mut().enumerate() {
                                gpu.set_cached_primitive_routing_enabled(mode != 0);
                            }
                            tx.translation(group, [-1.25, -2.5]);
                        }
                        _ => tx.color(node, 0xff8844ff),
                    }
                    tree.commit(
                        tx,
                        BackendCapabilities {
                            solid_color_patch: true,
                        },
                    )?;
                    let normal = tree.frame([192.0, 144.0])?;
                    let mut frame = normal.clone();
                    if step == 11 {
                        // A well-formed but incomplete public delta must not hide
                        // the new material or retain stale geometry in either mode.
                        frame.updates = Arc::new(Vec::new().into());
                        frame.damage = Damage::default();
                    }
                    if step == 14 {
                        // Missing removal metadata must retire stale draws from
                        // the authoritative live snapshot, including reused slots.
                        frame.removed = Arc::from([]);
                        frame.updates = Arc::new(Vec::new().into());
                        frame.damage = Damage::default();
                    }
                    for (mode, gpu) in renderers.iter_mut().enumerate() {
                        let submit = gpu.submit(&frame, size, scale)?.unwrap();
                        if step == 11 || step == 14 {
                            assert!(submit.full_resync && submit.routing.untrusted_full_resync);
                        }
                        if step == 13 || step == 15 || step == 16 {
                            assert!(submit.full_resync);
                        }
                        if step == 1 && mode == 0 && shared {
                            assert!(submit.routing.cached);
                            assert_eq!(submit.routing.prepared_quad_items, 0);
                            assert_eq!(submit.routing.prepared_shadow_items, 0);
                            assert_eq!(submit.routing.prepared_spatial_items, 3);
                        }
                        if step == 2 && mode == 0 {
                            assert_eq!(submit.routing.prepared_quad_items, 1);
                            assert_eq!(submit.routing.prepared_shadow_items, 0);
                        }
                        compare(
                            gpu,
                            &normal,
                            size,
                            scale,
                            &format!("routing-{copying}-{shared}-{scale}-{step}-{mode}"),
                            output,
                            results,
                        )?;
                        let value = results.last_mut().unwrap();
                        value["cached"] = submit.routing.cached.into();
                        value["prepared_quad_items"] = submit.routing.prepared_quad_items.into();
                        value["prepared_shadow_items"] =
                            submit.routing.prepared_shadow_items.into();
                        value["prepared_spatial_items"] =
                            submit.routing.prepared_spatial_items.into();
                        value["rebuilt_spatial_items"] =
                            submit.routing.rebuilt_spatial_items.into();
                        value["untrusted_full_resync"] =
                            submit.routing.untrusted_full_resync.into();
                        // New same-revision authoritative roots cannot pass the
                        // equal-frame fast path; a valid retry remains usable.
                        if step == 11 {
                            let mut changed = normal.clone();
                            Arc::make_mut(&mut changed.items)[0].fill_color = 0x123456ff;
                            changed.updates = Arc::new(Vec::new().into());
                            assert!(gpu.submit(&changed, size, scale).is_err());
                            assert!(gpu.submit(&normal, size, scale)?.is_none());
                            results.push(serde_json::json!({"case":format!("routing-equal-reject-{copying}-{shared}-{scale}-{mode}"),"same_revision_scene_rejected":true,"valid_equal_retry":true}));
                        }
                    }
                    assert_eq!(
                        renderers[0].image()?.as_raw(),
                        renderers[1].image()?.as_raw()
                    );
                }
                for (mode, gpu) in renderers.iter_mut().enumerate() {
                    gpu.reset()?;
                    assert_eq!(gpu.cached_primitive_routing_enabled(), mode != 0);
                }
            }
        }
    }
    Ok(())
}
