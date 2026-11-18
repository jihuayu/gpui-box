use super::*;
fn fixed(w: f32, h: f32) -> LayoutStyle {
    LayoutStyle::column()
        .size(LayoutDimension::Px(w), LayoutDimension::Px(h))
        .flex_shrink(0.0)
}
fn box_style(w: f32, h: f32, clipped: bool) -> LayoutStyle {
    let mode = if clipped {
        LayoutOverflow::Hidden
    } else {
        LayoutOverflow::Visible
    };
    let mut style = fixed(w, h)
        .padding(LayoutEdges::all(LayoutLength::Px(2.0)))
        .overflow(mode, mode);
    style.border = LayoutEdges::all(LayoutLength::Px(3.0));
    style
}
fn pixels(frame: FramePacket) -> Vec<u8> {
    let mut job = software::SoftwareJob::new(
        frame,
        software::SoftwareOptions {
            size: [120, 100],
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
#[test]
fn overflow_own_noto_glyphs_clip_to_viewport_and_preserve_border() {
    run(false);
}
#[cfg(feature = "retained-wgpu")]
#[test]
#[ignore = "requires WGPU adapter; run explicitly"]
fn hardware_overflow_own_noto_glyphs_match_explicit_reference() {
    run(true);
}
fn run(hardware: bool) {
    let system = text_system();
    let paragraph = shape(
        &system,
        "one two three four five six 中文段落 repeat one two three four five",
    );
    let mut raster = GlyphRasterizer::new(system.clone());
    let resource = raster.prepare(paragraph, 1.0).unwrap();
    let mut tree = UiTree::new_layout(fixed(120.0, 100.0), 0x222222ff).unwrap();
    let host = tree
        .append_layout(tree.root(), box_style(80.0, 30.0, true), 0x0000ffff)
        .unwrap();
    let paint = TextPaint {
        resource,
        color: 0x00ffffff,
    };
    let mut tx = Transaction::default();
    tx.text(host, Some(paint.clone()));
    tx.decoration(
        host,
        QuadDecoration {
            border_width: 3.0,
            border_color: 0xff0000ff,
            ..Default::default()
        },
    );
    tx.translation(host, [8.0, 6.0]);
    tx.opacity(host, 0.8);
    tree.commit(tx, BackendCapabilities::default()).unwrap();
    #[cfg(feature = "retained-wgpu")]
    let mut gpu = hardware.then(|| wgpu::WgpuFrameRenderer::new(2).unwrap());
    #[cfg(not(feature = "retained-wgpu"))]
    assert!(!hardware);
    for (w, h, clipped, moved, snap) in [
        (80.0, 30.0, true, [8.0, 6.0], PixelSnap::None),
        (80.0, 30.0, true, [12.0, 10.0], PixelSnap::None),
        (65.0, 50.0, true, [12.0, 10.0], PixelSnap::None),
        (65.0, 50.0, false, [12.0, 10.0], PixelSnap::None),
        (
            80.0,
            30.0,
            true,
            [8.25, 6.25],
            PixelSnap::DevicePixels { scale: 1.0 },
        ),
    ] {
        let mut tx = Transaction::default();
        tx.layout_style(host, box_style(w, h, clipped));
        tx.translation(host, moved);
        tree.commit(tx, BackendCapabilities::default()).unwrap();
        tree.set_pixel_snap(snap).unwrap();
        let frame = tree.local_frame([120.0, 100.0]).unwrap();
        // Explicit reference separates the ordinary host border from a child
        // text payload with a numeric padding-box clip. It never sets overflow.
        let mut reference = UiTree::new_layout(fixed(120.0, 100.0), 0x222222ff).unwrap();
        reference.set_frame_build_policy(FrameBuildPolicy::Full);
        let background = reference
            .append_layout(reference.root(), box_style(w, h, false), 0x0000ffff)
            .unwrap();
        let text = reference
            .append_layout(background, fixed(w - 10.0, h - 10.0), 0)
            .unwrap();
        let mut tx = Transaction::default();
        tx.text(text, Some(paint.clone()));
        tx.translation(background, moved);
        tx.opacity(background, 0.8);
        tx.decoration(
            background,
            QuadDecoration {
                border_width: 3.0,
                border_color: 0xff0000ff,
                ..Default::default()
            },
        );
        if clipped {
            tx.clip(
                text,
                Some(Rect {
                    x: -2.0,
                    y: -2.0,
                    width: w - 6.0,
                    height: h - 6.0,
                }),
            );
        }
        reference
            .commit(tx, BackendCapabilities::default())
            .unwrap();
        reference.set_pixel_snap(snap).unwrap();
        let full = reference.frame([120.0, 100.0]).unwrap();
        let resolved = frame.resolve();
        assert_eq!(
            resolved.items[1].glyphs.as_ref().unwrap().glyphs.len(),
            full.items[2].glyphs.as_ref().unwrap().glyphs.len()
        );
        assert_eq!(pixels(resolved), pixels(full.clone()), "{w}/{h}/{clipped}");
        #[cfg(feature = "retained-wgpu")]
        if let Some(gpu) = &mut gpu {
            assert!(gpu.submit_local(&frame, [120, 100], 1.0).unwrap().is_some());
            assert_eq!(
                gpu.image().unwrap().as_raw(),
                gpu.reference_image(&full, [120, 100], 1.0)
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
