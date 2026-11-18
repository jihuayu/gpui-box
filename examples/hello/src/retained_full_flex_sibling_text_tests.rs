use super::*;
use gpui_box::retained::logical::*;

fn root() -> LayoutStyle {
    LayoutStyle::column().size(LayoutDimension::Percent(1.0), LayoutDimension::Percent(1.0))
}
fn body() -> LayoutStyle {
    LayoutStyle::column()
        .flex_basis(LayoutDimension::Px(0.0))
        .flex_grow(1.0)
        .min_size(LayoutDimension::Px(0.0), LayoutDimension::Px(0.0))
        .overflow(LayoutOverflow::Hidden, LayoutOverflow::Hidden)
}
fn label() -> LayoutStyle {
    LayoutStyle::column()
        .min_size(LayoutDimension::Px(0.0), LayoutDimension::Auto)
        .flex_shrink(0.0)
}
fn fixed(w: f32, h: f32) -> LayoutStyle {
    LayoutStyle::column()
        .size(LayoutDimension::Px(w), LayoutDimension::Px(h))
        .flex_shrink(0.0)
}
fn pixels(packet: FramePacket) -> Vec<u8> {
    let mut job = software::SoftwareJob::new(
        packet,
        software::SoftwareOptions {
            size: [240, 240],
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
fn measured_sibling_noto_full_flex_uses_real_header_wrap_and_remaining_body_height() {
    run(false);
}
#[cfg(feature = "retained-wgpu")]
#[test]
#[ignore = "requires WGPU adapter; run explicitly"]
fn hardware_measured_sibling_noto_full_flex_matches_complete_reference() {
    run(true);
}
fn run(hardware: bool) {
    let system = text_system();
    let mut raster = GlyphRasterizer::new(system.clone());
    let long_paragraph = shape(
        &system,
        "one two three four five 中文段落 repeat one two three four",
    );
    let short_paragraph = shape(&system, "short title");
    let long = raster.prepare(long_paragraph.clone(), 1.0).unwrap();
    let short = raster.prepare(short_paragraph.clone(), 1.0).unwrap();
    let mut ui = LogicalUi::new_layout(root(), 0, BackendCapabilities::default()).unwrap();
    let owner = ui.root_scope();
    let header = ui
        .append_render_layout(
            owner,
            ui.root_render(),
            label().padding(LayoutEdges::all(LayoutLength::Px(3.0))),
            0x0000ffff,
        )
        .unwrap();
    let title = ui.append_render_layout(owner, header, label(), 0).unwrap();
    let host = ui
        .append_render_layout(owner, ui.root_render(), body(), 0)
        .unwrap();
    #[cfg(feature = "retained-wgpu")]
    let mut gpu = hardware.then(|| wgpu::WgpuFrameRenderer::new(2).unwrap());
    #[cfg(feature = "retained-wgpu")]
    if let Some(gpu) = &gpu {
        eprintln!("measured sibling Noto adapter: {:?}", gpu.adapter_info());
    }
    #[cfg(not(feature = "retained-wgpu"))]
    assert!(!hardware);
    let mut old = None;
    for (size, text, paragraph) in [
        ([100.0, 210.0], long.clone(), long_paragraph.clone()),
        ([180.0, 180.0], short.clone(), short_paragraph.clone()),
        ([140.0, 210.0], long.clone(), long_paragraph.clone()),
    ] {
        let constraints = LayoutConstraints::viewport(size[0], size[1]);
        let header_height = paragraph.layout(Some(size[0] - 6.0)).unwrap().size[1] + 6.0;
        let body_height = (size[1] - header_height).max(0.0);
        let (_, frame) = ui
            .edit_local_frame(
                constraints,
                [240.0; 2],
                |_| Ok(()),
                |(), edit| {
                    edit.properties().text(
                        title,
                        Some(TextPaint {
                            resource: text.clone(),
                            color: 0x00ffffff,
                        }),
                    );
                    edit.allocation_builder(host, |input, edit| {
                        assert_eq!(
                            input.known_size,
                            LayoutSize::new(Some(size[0]), Some(body_height))
                        );
                        let branch = edit.reconcile_keys(owner, &[u64::from(size[0] > 120.0)])?[0];
                        if edit.renders(branch)?.is_empty() {
                            edit.append_layout(branch, host, fixed(400.0, 300.0), 0xff0000ff)?;
                        }
                        // Same shaped measurement, different glyph color is paint-only.
                        edit.properties().text(
                            title,
                            Some(TextPaint {
                                resource: text.clone(),
                                color: 0xffcc00ff,
                            }),
                        );
                        Ok(())
                    })
                },
            )
            .unwrap();
        let mut reference = UiTree::new_layout(root(), 0).unwrap();
        reference.set_frame_build_policy(FrameBuildPolicy::Full);
        let header_ref = reference
            .append_layout(
                reference.root(),
                fixed(size[0], header_height).padding(LayoutEdges::all(LayoutLength::Px(3.0))),
                0x0000ffff,
            )
            .unwrap();
        let title_ref = reference.append_layout(header_ref, label(), 0).unwrap();
        let body_ref = reference
            .append_layout(
                reference.root(),
                fixed(size[0], body_height)
                    .overflow(LayoutOverflow::Hidden, LayoutOverflow::Hidden),
                0,
            )
            .unwrap();
        reference
            .append_layout(body_ref, fixed(400.0, 300.0), 0xff0000ff)
            .unwrap();
        let mut tx = Transaction::default();
        tx.text(
            title_ref,
            Some(TextPaint {
                resource: text.clone(),
                color: 0xffcc00ff,
            }),
        );
        reference
            .commit(tx, BackendCapabilities::default())
            .unwrap();
        let full = reference
            .frame_in_viewport(constraints, [240.0; 2])
            .unwrap();
        assert_eq!(
            ui.layout_result(title).unwrap(),
            reference.layout_result(title_ref).unwrap()
        );
        assert_eq!(ui.layout_result(host).unwrap().size, [size[0], body_height]);
        assert_eq!(pixels(frame.resolve()), pixels(full.clone()));
        #[cfg(feature = "retained-wgpu")]
        if let Some(gpu) = &mut gpu {
            gpu.submit_local(&frame, [240, 240], 1.0).unwrap();
            assert_eq!(
                gpu.image().unwrap().as_raw(),
                gpu.reference_image(&full, [240, 240], 1.0)
                    .unwrap()
                    .as_raw()
            );
        }
        let before = ui.published_local_frame().unwrap().clone();
        assert!(
            ui.edit_local_frame(
                constraints,
                [240.0; 2],
                |_| Ok(()),
                |(), edit| {
                    edit.allocation_builder(host, |_, edit| {
                        edit.properties().text(
                            title,
                            Some(TextPaint {
                                resource: if paragraph == long_paragraph {
                                    short.clone()
                                } else {
                                    long.clone()
                                },
                                color: 0xffcc00ff,
                            }),
                        );
                        Ok(())
                    })
                }
            )
            .is_err()
        );
        assert_eq!(
            before.revision(),
            ui.published_local_frame().unwrap().revision()
        );
        assert_eq!(
            pixels(before.resolve()),
            pixels(ui.published_local_frame().unwrap().resolve())
        );
        old.get_or_insert_with(|| (frame.clone(), pixels(frame.resolve())));
    }
    drop((
        ui,
        long,
        short,
        long_paragraph,
        short_paragraph,
        raster,
        system,
    ));
    let (frame, expected) = old.unwrap();
    assert_eq!(pixels(frame.resolve()), expected);
    #[cfg(feature = "retained-wgpu")]
    if let Some(gpu) = &mut gpu {
        gpu.finish().unwrap();
    }
}
