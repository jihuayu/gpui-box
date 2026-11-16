use super::*;
use crate::retained::{logical::LogicalUi, software::*};

fn fixed(w: f32, h: f32) -> LayoutStyle {
    LayoutStyle::row()
        .size(LayoutDimension::Px(w), LayoutDimension::Px(h))
        .flex_shrink(0.0)
}
fn image(frame: FramePacket, scale: f32) -> SoftwareFrame {
    image_on(frame, scale, 0)
}
fn image_on(frame: FramePacket, scale: f32, background: u32) -> SoftwareFrame {
    let mut job = SoftwareJob::new(
        frame,
        SoftwareOptions {
            size: [200, 200],
            scale,
            background,
            effects: pacing::SoftwareEffects::Exact,
            max_bytes: 1024 * 1024,
        },
    )
    .unwrap();
    while !job.step(4096).unwrap() {}
    job.into_frame().unwrap()
}

#[test]
fn fractional_flex_edges_snap_cumulatively_without_changing_logical_layout() {
    for scale in [1.0, 1.25, 1.5, 2.0] {
        let mut tree = UiTree::new_layout(fixed(100.0, 10.0), 0).unwrap();
        let mut children = vec![];
        for color in [0xff0000ff, 0x00ff00ff, 0x0000ffff] {
            children.push(
                tree.append_layout(
                    tree.root(),
                    LayoutStyle::row()
                        .size(LayoutDimension::Px(0.0), LayoutDimension::Px(10.0))
                        .flex_grow(1.0),
                    color,
                )
                .unwrap(),
            );
        }
        let first = tree.frame([100.0; 2]).unwrap();
        let logical: Vec<_> = children
            .iter()
            .map(|id| tree.layout_result(*id).unwrap())
            .collect();
        tree.set_pixel_snap(PixelSnap::DevicePixels { scale })
            .unwrap();
        let snapped = tree.frame([100.0; 2]).unwrap();
        assert_eq!(snapped.layout_runs, 0);
        assert_eq!(snapped.recorded_chunks, 0);
        assert_eq!(snapped.pixel_snap(), PixelSnap::DevicePixels { scale });
        let mut end = 0.0;
        for (i, id) in children.iter().enumerate() {
            let item = snapped.items.iter().find(|item| item.node == *id).unwrap();
            assert_eq!(item.bounds.x, end);
            let right = (((logical[i].location[0] + logical[i].size[0]) as f64 * f64::from(scale))
                .round()
                / f64::from(scale)) as f32;
            assert_eq!(item.bounds.x + item.bounds.width, right);
            end = right;
            assert_eq!(tree.layout_result(*id).unwrap(), logical[i]);
            assert_eq!(
                snapped.hit_test(item.bounds.x + item.bounds.width * 0.5, 1.0),
                Some(*id)
            );
        }
        assert_eq!(end, 100.0);
        let cpu = image(snapped.clone(), scale);
        for x in 0..(100.0 * scale) as usize {
            let p = (200 + x) * 4;
            assert_eq!(
                cpu.pixels()[p + 3],
                255,
                "gap at physical x={x}, scale={scale}"
            );
            assert!(cpu.hit_test([x as f32 + 0.5, 1.5]).is_some());
        }
        tree.set_pixel_snap(PixelSnap::None).unwrap();
        let restored = tree.frame([100.0; 2]).unwrap();
        assert_eq!(restored.layout_runs, 0);
        for (a, b) in first.items.iter().zip(restored.items.iter()) {
            assert_eq!(a.bounds, b.bounds);
        }
    }
}

#[test]
fn atomic_policy_source_and_frame_rollback_and_equal_policy_noop() {
    let mut ui =
        LogicalUi::new_layout(fixed(100.0, 100.0), 0, BackendCapabilities::default()).unwrap();
    let root = ui.root_render();
    let scope = ui.root_scope();
    let color = ui.state(scope, 1, || 0xff0000ffu32).unwrap();
    let child = ui
        .append_render_layout(scope, root, fixed(20.3, 10.2), 0)
        .unwrap();
    ui.bind(color, reactive::Property::color(child)).unwrap();
    let constraints = LayoutConstraints::viewport(100.0, 100.0);
    let (_, old) = ui
        .edit_local_frame(constraints, [100.0; 2], |_| Ok(()), |(), _| Ok(()))
        .unwrap();
    let before = ui.stats();
    let failure = ui.edit_local_frame(
        constraints,
        [100.0; 2],
        |tx| {
            tx.set(color, 0x00ff00ff)?;
            Ok(())
        },
        |(), edit| {
            edit.pixel_snap(PixelSnap::DevicePixels { scale: 2.0 })?;
            edit.properties().translation(root, [f32::MAX; 2]);
            edit.properties().translation(child, [f32::MAX; 2]);
            Ok(())
        },
    );
    assert!(matches!(failure, Err(Error::InvalidFrame)));
    assert_eq!(ui.pixel_snap(), PixelSnap::None);
    assert_eq!(ui.stats(), before);
    assert_eq!(*ui.read(color).unwrap(), 0xff0000ff);
    assert_eq!(
        ui.published_local_frame().unwrap().revision(),
        old.revision()
    );
    let (_, snapped) = ui
        .edit_local_frame(
            constraints,
            [100.0; 2],
            |_| Ok(()),
            |(), edit| edit.pixel_snap(PixelSnap::DevicePixels { scale: 2.0 }),
        )
        .unwrap();
    assert_eq!(snapped.pixel_snap(), PixelSnap::DevicePixels { scale: 2.0 });
    let (_, same) = ui
        .edit_local_frame(
            constraints,
            [100.0; 2],
            |_| Ok(()),
            |(), edit| edit.pixel_snap(PixelSnap::DevicePixels { scale: 2.0 }),
        )
        .unwrap();
    assert_eq!(snapped.revision(), same.revision());
    assert!(!same.stats().full_publication || same.stats().changed_nodes == 0);
    for scale in [0.0, -1.0, f32::NAN, f32::INFINITY, f32::from_bits(1)] {
        assert!(
            ui.edit(
                |_| Ok(()),
                |(), edit| edit.pixel_snap(PixelSnap::DevicePixels { scale })
            )
            .is_err()
        );
    }
    assert!(matches!(
        SoftwareJob::new(
            snapped.resolve(),
            SoftwareOptions {
                size: [200, 200],
                scale: 1.0,
                background: 0,
                effects: pacing::SoftwareEffects::Exact,
                max_bytes: 1024 * 1024,
            }
        ),
        Err(SoftwareError::InvalidFrame)
    ));
    assert_eq!(old.resolve_geometry(child).unwrap().bounds.width, 20.3);
}

#[test]
fn translated_scrolled_clipped_local_frames_match_fresh_full_projection() {
    let style = LayoutStyle::column().size(LayoutDimension::Px(70.3), LayoutDimension::Px(40.2));
    let mut tree = UiTree::new_layout(style.clone(), 0).unwrap();
    let host = tree
        .append_layout(
            tree.root(),
            fixed(40.3, 20.4).overflow(LayoutOverflow::Scroll, LayoutOverflow::Scroll),
            0,
        )
        .unwrap();
    let child = tree
        .append_layout(host, fixed(60.4, 30.3), 0xff0000ff)
        .unwrap();
    tree.set_pixel_snap(PixelSnap::DevicePixels { scale: 1.5 })
        .unwrap();
    let mut first = None;
    for dx in [0.2, 0.7, -0.3, 0.2] {
        let mut tx = Transaction::default();
        tx.translation(host, [dx, 0.3]);
        tx.clip(
            host,
            Some(Rect {
                x: 0.2,
                y: 0.3,
                width: 30.2,
                height: 15.6,
            }),
        );
        tx.scroll(host, [4.2, 3.1]);
        tree.commit(tx, BackendCapabilities::default()).unwrap();
        let local = tree.local_frame([100.0; 2]).unwrap();
        local.validate_geometry().unwrap();
        let actual = local.resolve();
        let mut full = UiTree::new_layout(style.clone(), 0).unwrap();
        full.set_frame_build_policy(FrameBuildPolicy::Full);
        let h = full
            .append_layout(
                full.root(),
                fixed(40.3, 20.4).overflow(LayoutOverflow::Scroll, LayoutOverflow::Scroll),
                0,
            )
            .unwrap();
        full.append_layout(h, fixed(60.4, 30.3), 0xff0000ff)
            .unwrap();
        let mut tx = Transaction::default();
        tx.translation(h, [dx, 0.3]);
        tx.scroll(h, [4.2, 3.1]);
        tx.clip(
            h,
            Some(Rect {
                x: 0.2,
                y: 0.3,
                width: 30.2,
                height: 15.6,
            }),
        );
        full.commit(tx, BackendCapabilities::default()).unwrap();
        full.set_pixel_snap(PixelSnap::DevicePixels { scale: 1.5 })
            .unwrap();
        let expected = full.frame([100.0; 2]).unwrap();
        assert_eq!(
            image(actual.clone(), 1.5).pixels(),
            image(expected, 1.5).pixels()
        );
        assert_eq!(
            local.resolve_geometry(child).unwrap().bounds,
            actual
                .items
                .iter()
                .find(|item| item.node == child)
                .unwrap()
                .bounds
        );
        if first.is_none() {
            first = Some((local, image(actual, 1.5).pixels().to_vec()));
        }
    }
    drop(tree);
    let (old, expected) = first.unwrap();
    assert_eq!(image(old.resolve(), 1.5).pixels(), expected);
}

#[test]
fn policy_edits_advance_once_and_exhaustion_preserves_owner() {
    let mut tree = UiTree::new_layout(fixed(20.3, 10.2), 0xff0000ff).unwrap();
    let root = tree.root();
    let old = tree.frame([100.0; 2]).unwrap();
    let commit = tree
        .edit(BackendCapabilities::default(), |edit| {
            edit.pixel_snap(PixelSnap::DevicePixels { scale: 2.0 })?;
            edit.properties().color(root, 0x00ff00ff);
            Ok(())
        })
        .unwrap();
    assert_eq!(commit.commit.revision, old.revision + 1);
    assert_eq!(tree.frame([100.0; 2]).unwrap().items[0].bounds.width, 20.5);
    tree.revision = u64::MAX;
    assert!(matches!(
        tree.edit(BackendCapabilities::default(), |edit| {
            edit.pixel_snap(PixelSnap::None)?;
            edit.properties().color(root, 0x0000ffff);
            Ok(())
        }),
        Err(Error::RevisionExhausted)
    ));
    assert_eq!(tree.pixel_snap(), PixelSnap::DevicePixels { scale: 2.0 });
    assert_eq!(tree.node(root).unwrap().color, 0x00ff00ff);
    assert!(matches!(
        tree.set_pixel_snap(PixelSnap::None),
        Err(Error::RevisionExhausted)
    ));
    assert!(
        !tree
            .set_pixel_snap(tree.pixel_snap())
            .unwrap()
            .request_frame
    );
}

#[test]
fn rounded_container_output_does_not_change_query_threshold_or_builder_input() {
    use crate::retained::responsive::*;
    let mut ui =
        LogicalUi::new_layout(fixed(100.0, 100.0), 0, BackendCapabilities::default()).unwrap();
    let scope = ui.root_scope();
    let host = ui
        .append_render_layout(
            scope,
            ui.root_render(),
            LayoutStyle::column().fixed_inline_size(10.3),
            0,
        )
        .unwrap();
    let boundary = LayoutStyle::constraint_box(ConstraintFit::Tight, ConstraintFit::Loose)
        .container_type(LayoutContainerType::InlineSize);
    let slot = ui
        .append_render_layout(scope, host, boundary.clone(), 0)
        .unwrap();
    let source = ui.state(scope, 1, || boundary.clone()).unwrap();
    let (_, frame) = ui
        .edit_local_frame(
            LayoutConstraints::viewport(100.0, 100.0),
            [100.0; 2],
            |_| Ok(()),
            |(), edit| {
                edit.pixel_snap(PixelSnap::DevicePixels { scale: 2.0 })?;
                edit.responsive_rule(
                    scope,
                    1,
                    source,
                    slot,
                    LayoutQuery::ContainerInline(host),
                    LayoutBreakpointStyles::new(
                        boundary.clone(),
                        vec![(
                            10.4,
                            boundary
                                .clone()
                                .padding(LayoutEdges::all(LayoutLength::Px(1.0))),
                        )],
                    )?,
                )?;
                edit.responsive_registered(&LayoutEnvironment::new([100.0; 2]))?;
                edit.inline_constraint_builder(slot, |input, edit| {
                    assert_eq!(input.max, 10.3);
                    edit.append_layout(scope, slot, fixed(20.0, 10.0), 0xff0000ff)
                })?;
                Ok(())
            },
        )
        .unwrap();
    assert_eq!(ui.read(source).unwrap(), &boundary);
    assert_eq!(ui.layout_result(host).unwrap().size[0], 10.3);
    assert_eq!(frame.resolve_geometry(host).unwrap().bounds.width, 10.5);
}

#[test]
fn fractional_viewport_and_root_own_clip_keep_damage_local_on_color_update() {
    let mut tree = UiTree::new_layout(fixed(95.3, 70.2), 0).unwrap();
    let root = tree.root();
    let child = tree
        .append_layout(root, fixed(10.3, 10.2), 0xff0000ff)
        .unwrap();
    let mut tx = Transaction::default();
    tx.translation(root, [0.3, -0.2]);
    tx.clip(
        root,
        Some(Rect {
            x: 0.2,
            y: 0.3,
            width: 80.2,
            height: 40.3,
        }),
    );
    tree.commit(tx, BackendCapabilities::default()).unwrap();
    tree.set_pixel_snap(PixelSnap::DevicePixels { scale: 1.5 })
        .unwrap();
    let first = tree.frame([99.3, 99.2]).unwrap();
    let root_item = &first.items[0];
    assert_eq!(
        root_item.spatial.root_clip,
        Some(Rect {
            x: 0.0,
            y: 0.0,
            width: 149.0 / 1.5,
            height: 149.0 / 1.5
        })
    );
    assert_eq!(root_item.properties.clip, Some(root));
    assert_ne!(root_item.clip, root_item.spatial.root_clip);
    let mut tx = Transaction::default();
    tx.color(child, 0x00ff00ff);
    tree.commit(tx, BackendCapabilities::default()).unwrap();
    let next = tree.local_frame([99.3, 99.2]).unwrap();
    next.validate_geometry().unwrap();
    let packet = tree.frame.as_ref().unwrap();
    assert!(!packet.damage.full);
    assert_eq!(packet.damage.bounds, Some(packet.items[1].visible_bounds()));
    assert_eq!(
        next.resolve_geometry(root).unwrap().properties,
        root_item.properties
    );
}

fn composite_fixture() -> (UiTree, [NodeId; 5]) {
    let mut tree = UiTree::new_layout(fixed(95.3, 70.2), 0).unwrap();
    let host = tree
        .append_layout(
            tree.root(),
            fixed(70.3, 40.2).overflow(LayoutOverflow::Scroll, LayoutOverflow::Scroll),
            0x330000ff,
        )
        .unwrap();
    let left = tree
        .append_layout(host, fixed(40.3, 30.4), 0x00ff00ff)
        .unwrap();
    let right = tree
        .append_layout(host, fixed(40.2, 30.4), 0x0000ffff)
        .unwrap();
    let leaf = tree.append_layout(left, fixed(14.3, 10.2), 0).unwrap();
    let mut absolute = fixed(10.3, 7.2);
    absolute.position = layout::LayoutPosition::Absolute;
    absolute.inset.left = LayoutLength::Px(12.3);
    absolute.inset.top = LayoutLength::Px(4.2);
    let absolute = tree.append_layout(host, absolute, 0xff00ffff).unwrap();
    let mut tx = Transaction::default();
    tx.clip(
        tree.root(),
        Some(Rect {
            x: 0.2,
            y: 0.3,
            width: 91.4,
            height: 65.3,
        }),
    );
    tx.image(
        leaf,
        Some(ImageResource::rgba8([2, 2], [255, 255, 0, 255].repeat(4)).unwrap()),
    );
    tree.commit(tx, BackendCapabilities::default()).unwrap();
    (tree, [host, left, right, leaf, absolute])
}

fn composite_update(tree: &mut UiTree, ids: [NodeId; 5], step: usize) {
    let [host, left, right, leaf, _] = ids;
    tree.edit(BackendCapabilities::default(), |edit| {
        edit.reparent(leaf, if step >= 3 { right } else { left })?;
        let tx = edit.properties();
        tx.translation(host, [if step % 2 == 0 { 0.2 } else { -0.7 }, 0.3]);
        tx.scroll(host, [4.2, 3.1]);
        tx.clip(
            host,
            Some(Rect {
                x: 0.3,
                y: 0.2,
                width: 60.2,
                height: 30.4,
            }),
        );
        tx.clip(
            left,
            Some(Rect {
                x: 1.2,
                y: 0.7,
                width: 30.3,
                height: 20.3,
            }),
        );
        tx.opacity(host, 0.6);
        tx.opacity(left, 0.8);
        tx.layout_style(
            leaf,
            fixed(14.3, 10.2).display(if step == 2 {
                layout::LayoutDisplay::None
            } else {
                layout::LayoutDisplay::Flex
            }),
        );
        Ok(())
    })
    .unwrap();
}

fn policies() -> [(PixelSnap, f32); 6] {
    [
        (PixelSnap::DevicePixels { scale: 1.25 }, 1.25),
        (PixelSnap::DevicePixels { scale: 1.5 }, 1.5),
        (PixelSnap::DevicePixels { scale: 2.0 }, 2.0),
        (PixelSnap::DevicePixels { scale: 2.0 }, 2.0),
        (PixelSnap::None, 2.0),
        (PixelSnap::DevicePixels { scale: 1.25 }, 1.25),
    ]
}

#[test]
fn nested_clips_effects_images_and_oof_survive_scale_hide_reparent_and_old_frames() {
    let (mut tree, ids) = composite_fixture();
    let mut first = None;
    for (step, (policy, scale)) in policies().into_iter().enumerate() {
        composite_update(&mut tree, ids, step);
        tree.set_pixel_snap(policy).unwrap();
        let local = tree.local_frame([100.0; 2]).unwrap();
        local.validate_geometry().unwrap();
        let actual = local.resolve();
        let (mut full, full_ids) = composite_fixture();
        full.set_frame_build_policy(FrameBuildPolicy::Full);
        composite_update(&mut full, full_ids, step);
        full.set_pixel_snap(policy).unwrap();
        let expected = full.frame([100.0; 2]).unwrap();
        assert_eq!(
            image(actual.clone(), scale).pixels(),
            image(expected, scale).pixels()
        );
        for item in actual.items.iter() {
            let resolved = local.resolve_geometry(item.node).unwrap();
            assert_eq!(resolved.bounds, item.bounds);
            assert_eq!(resolved.clip, item.clip);
            assert_eq!(resolved.properties, item.properties);
        }
        if first.is_none() {
            first = Some((local, image(actual, scale).pixels().to_vec()));
        }
    }
    drop(tree);
    let (old, expected) = first.unwrap();
    assert_eq!(image(old.resolve(), 1.25).pixels(), expected);
}

#[cfg(feature = "retained-wgpu")]
#[test]
#[ignore = "requires a WGPU adapter; run explicitly"]
fn hardware_pixel_snap_matches_full_reference_and_independent_integer_geometry() {
    let mut renderer = wgpu::WgpuFrameRenderer::new(2).unwrap();
    eprintln!("pixel snap adapter: {:?}", renderer.adapter_info());
    // Independent ordinary layout is the physical-edge oracle for this scene.
    let mut tree = UiTree::new_layout(fixed(10.3, 10.2), 0xff0000ff).unwrap();
    tree.set_pixel_snap(PixelSnap::DevicePixels { scale: 2.0 })
        .unwrap();
    let frame = tree.local_frame([100.0; 2]).unwrap();
    assert!(matches!(
        renderer.submit_local(&frame, [200; 2], 1.0),
        Err(Error::InvalidFrame)
    ));
    renderer
        .submit_local(&frame, [200; 2], 2.0)
        .unwrap()
        .unwrap();
    let actual = renderer.image().unwrap();
    let mut integer = UiTree::new_layout(fixed(10.5, 10.0), 0xff0000ff).unwrap();
    let expected = integer.frame([100.0; 2]).unwrap();
    assert_eq!(
        actual.as_raw(),
        renderer
            .reference_image(&expected, [200; 2], 2.0)
            .unwrap()
            .as_raw()
    );
    assert!(actual.as_raw() == image_on(frame.resolve(), 2.0, 0x000000ff).pixels());
    assert_eq!(frame.hit_test(10.4, 9.9), Some(tree.root()));
    assert_eq!(frame.hit_test(10.5, 9.9), None);
    // Invalid resolved submission must leave the valid local consumer retryable.
    assert!(matches!(
        renderer.submit(&frame.resolve(), [200; 2], 1.0),
        Err(Error::InvalidFrame)
    ));
    assert!(
        renderer
            .submit_local(&frame, [200; 2], 2.0)
            .unwrap()
            .is_none()
    );

    let (mut tree, ids) = composite_fixture();
    for (step, (policy, scale)) in policies().into_iter().enumerate() {
        composite_update(&mut tree, ids, step);
        tree.set_pixel_snap(policy).unwrap();
        let frame = tree.local_frame([100.0; 2]).unwrap();
        renderer
            .submit_local(&frame, [200; 2], scale)
            .unwrap()
            .unwrap();
        let actual = renderer.image().unwrap();
        let (mut full, full_ids) = composite_fixture();
        full.set_frame_build_policy(FrameBuildPolicy::Full);
        composite_update(&mut full, full_ids, step);
        full.set_pixel_snap(policy).unwrap();
        let expected = full.frame([100.0; 2]).unwrap();
        assert_eq!(
            actual.as_raw(),
            renderer
                .reference_image(&expected, [200; 2], scale)
                .unwrap()
                .as_raw(),
            "step={step}"
        );
        assert!(
            renderer
                .submit_local(&frame, [200; 2], scale)
                .unwrap()
                .is_none()
        );
        // Exercise the resolved consumer as well as the local entry point.
        renderer.submit(&frame.resolve(), [200; 2], scale).unwrap();
        assert_eq!(actual.as_raw(), renderer.image().unwrap().as_raw());
    }
    renderer.finish().unwrap();
}
