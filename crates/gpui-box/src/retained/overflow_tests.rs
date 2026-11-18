use super::*;

fn fixed(w: f32, h: f32) -> LayoutStyle {
    LayoutStyle::column()
        .size(LayoutDimension::Px(w), LayoutDimension::Px(h))
        .flex_shrink(0.0)
}
fn viewport(x: LayoutOverflow, y: LayoutOverflow, w: f32, h: f32) -> LayoutStyle {
    let mut style = fixed(w, h)
        .overflow(x, y)
        .padding(LayoutEdges::all(LayoutLength::Px(2.0)));
    style.border = LayoutEdges::all(LayoutLength::Px(4.0));
    style
}
fn fixture(
    policy: FrameBuildPolicy,
    x: LayoutOverflow,
    y: LayoutOverflow,
) -> (UiTree, NodeId, NodeId) {
    let mut tree = UiTree::new_layout(fixed(180.0, 140.0), 0x222222ff).unwrap();
    tree.set_frame_build_policy(policy);
    let host = tree
        .append_layout(tree.root(), viewport(x, y, 60.0, 40.0), 0x0000ffff)
        .unwrap();
    let child = tree
        .append_layout(host, fixed(120.0, 100.0), 0xff0000ff)
        .unwrap();
    let mut tx = Transaction::default();
    tx.translation(host, [10.0, 10.0]);
    tree.commit(tx, BackendCapabilities::default()).unwrap();
    (tree, host, child)
}
fn pixels(frame: FramePacket) -> Vec<u8> {
    let mut job = software::SoftwareJob::new(
        frame,
        software::SoftwareOptions {
            size: [180, 140],
            scale: 1.0,
            background: 0,
            effects: pacing::SoftwareEffects::Exact,
            max_bytes: 2 * 1024 * 1024,
        },
    )
    .unwrap();
    while !job.step(4096).unwrap() {}
    job.into_frame().unwrap().pixels().to_vec()
}
fn item(frame: &FramePacket, id: NodeId) -> &PaintItem {
    frame.items.iter().find(|item| item.node == id).unwrap()
}

#[test]
fn axis_viewport_clips_descendant_paint_and_hits_but_preserves_own_border_and_shadow() {
    for mode in [
        LayoutOverflow::Clip,
        LayoutOverflow::Hidden,
        LayoutOverflow::Scroll,
    ] {
        for (x, y, expected) in [
            (
                LayoutOverflow::Visible,
                mode,
                Rect {
                    x: 0.0,
                    y: 14.0,
                    width: 180.0,
                    height: 32.0,
                },
            ),
            (
                mode,
                LayoutOverflow::Visible,
                Rect {
                    x: 14.0,
                    y: 0.0,
                    width: 52.0,
                    height: 140.0,
                },
            ),
            (
                mode,
                mode,
                Rect {
                    x: 14.0,
                    y: 14.0,
                    width: 52.0,
                    height: 32.0,
                },
            ),
        ] {
            let (mut tree, host, child) = fixture(FrameBuildPolicy::Selective, x, y);
            let mut tx = Transaction::default();
            tx.shadows(
                host,
                vec![BoxShadow {
                    color: 0x00ff00ff,
                    spread_radius: 3.0,
                    ..Default::default()
                }],
            );
            tree.commit(tx, BackendCapabilities::default()).unwrap();
            let frame = tree.local_frame([180.0, 140.0]).unwrap();
            let full = frame.resolve();
            let host_item = item(&full, host);
            assert_eq!(host_item.content_clip, Some(expected));
            assert_eq!(host_item.clip, full.items[0].clip);
            assert!(host_item.ink_bounds().unwrap().x < host_item.bounds.x);
            assert_eq!(item(&full, child).clip, Some(expected));
            for point in [[20.0, 20.0], [80.0, 20.0], [20.0, 55.0], [80.0, 55.0]] {
                let allowed = point[0] >= expected.x
                    && point[0] < expected.x + expected.width
                    && point[1] >= expected.y
                    && point[1] < expected.y + expected.height;
                assert_eq!(full.hit_test(point[0], point[1]) == Some(child), allowed);
                assert_eq!(
                    frame.hit_test(point[0], point[1]),
                    full.hit_test(point[0], point[1])
                );
            }
            // The border/padding area remains a host target, not clipped away.
            assert_eq!(frame.hit_test(11.0, 11.0), Some(host));
        }
    }
}

#[test]
fn resized_overflow_viewport_republishes_cached_descendants_and_old_snapshots_stay_immutable() {
    let (mut tree, host, child) = fixture(
        FrameBuildPolicy::Selective,
        LayoutOverflow::Hidden,
        LayoutOverflow::Hidden,
    );
    let old = tree.local_frame([180.0, 140.0]).unwrap();
    let before = old.resolve();
    let mut tx = Transaction::default();
    tx.layout_style(
        host,
        viewport(LayoutOverflow::Hidden, LayoutOverflow::Hidden, 90.0, 70.0),
    );
    tree.commit(tx, BackendCapabilities::default()).unwrap();
    let next = tree.local_frame([180.0, 140.0]).unwrap();
    let resolved = next.resolve();
    assert_eq!(item(&before, child).bounds, item(&resolved, child).bounds);
    assert_ne!(item(&before, child).clip, item(&resolved, child).clip);
    assert!(Arc::ptr_eq(
        &item(&before, child).commands,
        &item(&resolved, child).commands
    ));
    assert_ne!(old.hit_test(80.0, 55.0), Some(child));
    assert_eq!(next.hit_test(80.0, 55.0), Some(child));
    assert_eq!(pixels(old.resolve()), pixels(before));
    assert_eq!(resolved.recorded_chunks, 0);
}

#[test]
fn rtl_gutter_and_explicit_clip_compose_with_padding_viewport() {
    let (mut tree, host, child) = fixture(
        FrameBuildPolicy::Selective,
        LayoutOverflow::Scroll,
        LayoutOverflow::Scroll,
    );
    let mut style =
        viewport(LayoutOverflow::Scroll, LayoutOverflow::Scroll, 60.0, 40.0).scrollbar_width(5.0);
    let mut tx = Transaction::default();
    tx.layout_style(host, style.clone());
    tx.clip(
        host,
        Some(Rect {
            x: 2.0,
            y: 3.0,
            width: 55.0,
            height: 32.0,
        }),
    );
    tree.commit(tx, BackendCapabilities::default()).unwrap();
    let a = tree.frame([180.0, 140.0]).unwrap();
    assert_eq!(
        item(&a, host).content_clip,
        Some(Rect {
            x: 14.0,
            y: 14.0,
            width: 47.0,
            height: 27.0
        })
    );
    style.direction = LayoutDirection::Rtl;
    let mut tx = Transaction::default();
    tx.layout_style(host, style);
    tree.commit(tx, BackendCapabilities::default()).unwrap();
    let b = tree.frame([180.0, 140.0]).unwrap();
    assert_eq!(
        item(&b, host).content_clip,
        Some(Rect {
            x: 19.0,
            y: 14.0,
            width: 47.0,
            height: 27.0
        })
    );
    assert_eq!(item(&b, child).clip, item(&b, host).content_clip);
}

// Numeric independent control: only ordinary explicit child clip, no overflow
// mode or shared automatic-viewport construction. Host keeps its full own paint.
fn reference(
    scroll: [f32; 2],
    moved: [f32; 2],
    width: f32,
    height: f32,
    clipped: bool,
    snap: PixelSnap,
) -> FramePacket {
    let (mut tree, host, child) = fixture(
        FrameBuildPolicy::Full,
        LayoutOverflow::Visible,
        LayoutOverflow::Visible,
    );
    let mut tx = Transaction::default();
    tx.layout_style(
        host,
        viewport(
            LayoutOverflow::Visible,
            LayoutOverflow::Visible,
            width,
            height,
        ),
    );
    tx.translation(host, moved);
    tx.translation(child, [-scroll[0], -scroll[1]]);
    if clipped {
        // Child origin is border4 + padding2 - scroll, while viewport stays
        // fixed to the host padding box; express the difference explicitly.
        tx.clip(
            child,
            Some(Rect {
                x: scroll[0] - 2.0,
                y: scroll[1] - 2.0,
                width: width - 8.0,
                height: height - 8.0,
            }),
        );
    }
    tree.commit(tx, BackendCapabilities::default()).unwrap();
    tree.set_pixel_snap(snap).unwrap();
    tree.frame([180.0, 140.0]).unwrap()
}
#[test]
fn full_selective_local_and_snapped_overflow_match_independent_explicit_reference() {
    run(false);
}
#[cfg(feature = "retained-wgpu")]
#[test]
#[ignore = "requires WGPU adapter; run explicitly"]
fn hardware_overflow_viewports_match_independent_full_images() {
    run(true);
}
fn run(hardware: bool) {
    #[cfg(feature = "retained-wgpu")]
    let mut gpu = hardware.then(|| wgpu::WgpuFrameRenderer::new(2).unwrap());
    #[cfg(feature = "retained-wgpu")]
    if let Some(gpu) = &gpu {
        eprintln!("overflow viewport adapter: {:?}", gpu.adapter_info());
    }
    #[cfg(not(feature = "retained-wgpu"))]
    assert!(!hardware);
    for policy in [
        FrameBuildPolicy::Full,
        FrameBuildPolicy::Selective,
        FrameBuildPolicy::SelectiveReextract,
        FrameBuildPolicy::SelectiveCopy,
    ] {
        let (mut tree, host, child) =
            fixture(policy, LayoutOverflow::Hidden, LayoutOverflow::Hidden);
        for (scroll, moved, width, height, clipped, snap) in [
            ([0.0; 2], [10.0, 10.0], 60.0, 40.0, true, PixelSnap::None),
            (
                [15.0, 20.0],
                [10.0, 10.0],
                60.0,
                40.0,
                true,
                PixelSnap::None,
            ),
            ([20.0, 30.0], [12.0, 8.0], 60.0, 40.0, true, PixelSnap::None),
            ([20.0, 30.0], [12.0, 8.0], 90.0, 70.0, true, PixelSnap::None),
            ([0.0; 2], [12.0, 8.0], 90.0, 70.0, false, PixelSnap::None),
            (
                [15.0, 20.0],
                [10.25, 10.25],
                60.0,
                40.0,
                true,
                PixelSnap::DevicePixels { scale: 1.0 },
            ),
        ] {
            let mode = if clipped {
                LayoutOverflow::Hidden
            } else {
                LayoutOverflow::Visible
            };
            let mut tx = Transaction::default();
            tx.layout_style(host, viewport(mode, mode, width, height));
            tx.translation(host, moved);
            tx.scroll(host, scroll);
            tree.commit(tx, BackendCapabilities::default()).unwrap();
            tree.set_pixel_snap(snap).unwrap();
            let frame = tree.local_frame([180.0, 140.0]).unwrap();
            frame.validate_geometry().unwrap();
            let resolved = frame.resolve();
            let baseline = reference(scroll, moved, width, height, clipped, snap);
            assert_eq!(item(&resolved, child).bounds, baseline.items[2].bounds);
            assert_eq!(
                pixels(resolved.clone()),
                pixels(baseline.clone()),
                "{policy:?} {scroll:?} {width}"
            );
            assert!(
                resolved
                    .items
                    .iter()
                    .all(|item| item.content_clip.is_some())
            );
            #[cfg(feature = "retained-wgpu")]
            if let Some(gpu) = &mut gpu {
                assert!(gpu.submit_local(&frame, [180, 140], 1.0).unwrap().is_some());
                assert_eq!(
                    gpu.image().unwrap().as_raw(),
                    gpu.reference_image(&baseline, [180, 140], 1.0)
                        .unwrap()
                        .as_raw()
                );
            }
        }
    }
    #[cfg(feature = "retained-wgpu")]
    if let Some(gpu) = &mut gpu {
        gpu.finish().unwrap();
    }
}

#[test]
fn deep_overflow_ancestry_keeps_images_clipped_after_sparse_scroll() {
    deep(false);
}
#[cfg(feature = "retained-wgpu")]
#[test]
#[ignore = "requires WGPU adapter; run explicitly"]
fn hardware_deep_overflow_images_and_outer_shadows_match_explicit_reference() {
    deep(true);
}
fn deep(hardware: bool) {
    let image = ImageResource::rgba8([1, 1], vec![255, 255, 0, 255]).unwrap();
    let build = |automatic: bool| {
        let mut tree = UiTree::new_layout(fixed(180.0, 140.0), 0x222222ff).unwrap();
        let mode = if automatic {
            LayoutOverflow::Hidden
        } else {
            LayoutOverflow::Visible
        };
        let host = tree
            .append_layout(
                tree.root(),
                fixed(60.0, 40.0).overflow(mode, mode),
                0x0000ffff,
            )
            .unwrap();
        let mut leaf = host;
        for _ in 0..70 {
            leaf = tree.append_layout(leaf, fixed(120.0, 100.0), 0).unwrap();
        }
        let mut tx = Transaction::default();
        tx.image(leaf, Some(image.clone()));
        tx.translation(host, [10.0, 10.0]);
        tx.shadows(
            host,
            vec![BoxShadow {
                color: 0x00ff00ff,
                spread_radius: 3.0,
                ..Default::default()
            }],
        );
        tree.commit(tx, BackendCapabilities::default()).unwrap();
        (tree, host, leaf)
    };
    let (mut tree, host, leaf) = build(true);
    #[cfg(feature = "retained-wgpu")]
    let mut gpu = hardware.then(|| wgpu::WgpuFrameRenderer::new(2).unwrap());
    #[cfg(not(feature = "retained-wgpu"))]
    assert!(!hardware);
    for scroll in [[0.0, 0.0], [20.0, 15.0], [35.0, 40.0], [5.0, 10.0]] {
        let mut tx = Transaction::default();
        tx.scroll(host, scroll);
        tree.commit(tx, BackendCapabilities::default()).unwrap();
        let frame = tree.local_frame([180.0, 140.0]).unwrap();
        let (mut baseline, _, control_leaf) = build(false);
        let mut tx = Transaction::default();
        tx.translation(control_leaf, [-scroll[0], -scroll[1]]);
        tx.clip(
            control_leaf,
            Some(Rect {
                x: scroll[0],
                y: scroll[1],
                width: 60.0,
                height: 40.0,
            }),
        );
        baseline.commit(tx, BackendCapabilities::default()).unwrap();
        let control = baseline.frame([180.0, 140.0]).unwrap();
        let resolved = frame.resolve();
        assert_eq!(
            item(&resolved, leaf).bounds,
            item(&control, control_leaf).bounds
        );
        assert_eq!(
            item(&resolved, leaf).clip,
            item(&control, control_leaf).clip
        );
        assert_ne!(frame.hit_test(80.0, 20.0), Some(leaf));
        assert_eq!(frame.hit_test(20.0, 20.0), Some(leaf));
        #[cfg(feature = "retained-wgpu")]
        if let Some(gpu) = &mut gpu {
            assert!(gpu.submit_local(&frame, [180, 140], 1.0).unwrap().is_some());
            assert_eq!(
                gpu.image().unwrap().as_raw(),
                gpu.reference_image(&control, [180, 140], 1.0)
                    .unwrap()
                    .as_raw()
            );
        }
    }
    #[cfg(feature = "retained-wgpu")]
    if let Some(gpu) = &mut gpu {
        gpu.finish().unwrap();
    }
}

#[test]
fn mixed_overflow_axes_match_numeric_pixels_and_keep_visible_axis() {
    mixed(false);
}
#[cfg(feature = "retained-wgpu")]
#[test]
#[ignore = "requires WGPU adapter; run explicitly"]
fn hardware_mixed_overflow_axes_match_numeric_pixels() {
    mixed(true);
}
fn mixed(hardware: bool) {
    #[cfg(feature = "retained-wgpu")]
    let mut gpu = hardware.then(|| wgpu::WgpuFrameRenderer::new(2).unwrap());
    #[cfg(not(feature = "retained-wgpu"))]
    assert!(!hardware);
    for mode in [
        LayoutOverflow::Clip,
        LayoutOverflow::Hidden,
        LayoutOverflow::Scroll,
    ] {
        for axes in [[true, false], [false, true], [true, true]] {
            let (mut tree, _, _) = fixture(
                FrameBuildPolicy::Selective,
                if axes[0] {
                    mode
                } else {
                    LayoutOverflow::Visible
                },
                if axes[1] {
                    mode
                } else {
                    LayoutOverflow::Visible
                },
            );
            let frame = tree.local_frame([180.0, 140.0]).unwrap();
            let (mut baseline, _, child) = fixture(
                FrameBuildPolicy::Full,
                LayoutOverflow::Visible,
                LayoutOverflow::Visible,
            );
            let mut tx = Transaction::default();
            tx.clip(
                child,
                Some(Rect {
                    x: if axes[0] { -2.0 } else { -16.0 },
                    y: if axes[1] { -2.0 } else { -16.0 },
                    width: if axes[0] { 52.0 } else { 180.0 },
                    height: if axes[1] { 32.0 } else { 140.0 },
                }),
            );
            baseline.commit(tx, BackendCapabilities::default()).unwrap();
            let control = baseline.frame([180.0, 140.0]).unwrap();
            assert_eq!(pixels(frame.resolve()), pixels(control.clone()));
            #[cfg(feature = "retained-wgpu")]
            if let Some(gpu) = &mut gpu {
                assert!(gpu.submit_local(&frame, [180, 140], 1.0).unwrap().is_some());
                assert_eq!(
                    gpu.image().unwrap().as_raw(),
                    gpu.reference_image(&control, [180, 140], 1.0)
                        .unwrap()
                        .as_raw()
                );
            }
        }
    }
    #[cfg(feature = "retained-wgpu")]
    if let Some(gpu) = &mut gpu {
        gpu.finish().unwrap();
    }
}
