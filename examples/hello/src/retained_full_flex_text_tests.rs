use super::*;
use gpui_box::retained::logical::*;

fn root() -> LayoutStyle {
    LayoutStyle::row().size(LayoutDimension::Percent(1.0), LayoutDimension::Percent(1.0))
}
fn host() -> LayoutStyle {
    LayoutStyle::column()
        .flex_basis(LayoutDimension::Px(0.0))
        .flex_grow(1.0)
        .min_size(LayoutDimension::Px(0.0), LayoutDimension::Px(0.0))
        .padding(LayoutEdges::all(LayoutLength::Px(3.0)))
        .overflow(LayoutOverflow::Hidden, LayoutOverflow::Hidden)
}
fn fixed(w: f32, h: f32) -> LayoutStyle {
    LayoutStyle::column()
        .size(LayoutDimension::Px(w), LayoutDimension::Px(h))
        .flex_shrink(0.0)
}
fn label() -> LayoutStyle {
    LayoutStyle::column()
        .min_size(LayoutDimension::Px(0.0), LayoutDimension::Auto)
        .flex_shrink(0.0)
}
fn pixels(packet: FramePacket) -> Vec<u8> {
    let mut job = software::SoftwareJob::new(
        packet,
        software::SoftwareOptions {
            size: [240, 180],
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
fn full_flex_two_axis_real_noto_wrap_clip_baselines_and_atomic_failure_match_full_reference() {
    run(false);
}
#[cfg(feature = "retained-wgpu")]
#[test]
#[ignore = "requires WGPU adapter; run explicitly"]
fn hardware_full_flex_two_axis_real_noto_matches_full_reference() {
    run(true);
}
fn run(hardware: bool) {
    let system = text_system();
    let mut raster = GlyphRasterizer::new(system.clone());
    let text = raster
        .prepare(
            shape(
                &system,
                "one two three four five 中文段落 repeat one two three four",
            ),
            1.0,
        )
        .unwrap();
    let mut ui = LogicalUi::new_layout(root(), 0, BackendCapabilities::default()).unwrap();
    let scope = ui.root_scope();
    let outer = ui
        .append_render_layout(scope, ui.root_render(), host(), 0x0000ffff)
        .unwrap();
    let sibling = ui
        .append_render_layout(scope, ui.root_render(), host(), 0x00ff00ff)
        .unwrap();
    #[cfg(feature = "retained-wgpu")]
    let mut gpu = hardware.then(|| wgpu::WgpuFrameRenderer::new(2).unwrap());
    #[cfg(feature = "retained-wgpu")]
    if let Some(gpu) = &gpu {
        eprintln!("two-axis real Noto adapter: {:?}", gpu.adapter_info());
    }
    #[cfg(not(feature = "retained-wgpu"))]
    assert!(!hardware);
    let mut old = None;
    for (size, color) in [
        ([160.0, 50.0], 0x00ffffffu32),
        ([220.0, 100.0], 0xffcc00ff),
        ([180.0, 70.0], 0x00ffffff),
    ] {
        let constraints = LayoutConstraints::viewport(size[0], size[1]);
        let (commit, frame) = ui
            .edit_local_frame(
                constraints,
                [240.0, 180.0],
                |_| Ok(()),
                |(), edit| {
                    edit.allocation_builder(outer, |input, edit| {
                        assert_eq!(
                            input.known_size,
                            LayoutSize::new(Some(size[0] * 0.5), Some(size[1]))
                        );
                        assert_eq!(input.known_size_is_definite, LayoutSize::new(true, true));
                        let branch = edit.reconcile_keys(scope, &[u64::from(size[0] > 200.0)])?[0];
                        let outputs = edit.renders(branch)?;
                        let label = if let Some(&label) = outputs.first() {
                            label
                        } else {
                            edit.append_layout(branch, outer, label(), 0)?
                        };
                        edit.properties().text(
                            label,
                            Some(TextPaint {
                                resource: text.clone(),
                                color,
                            }),
                        );
                        Ok(label)
                    })
                },
            )
            .unwrap();
        let mut reference = UiTree::new_layout(root(), 0).unwrap();
        reference.set_frame_build_policy(FrameBuildPolicy::Full);
        let parent = reference
            .append_layout(
                reference.root(),
                fixed(size[0] * 0.5, size[1])
                    .padding(LayoutEdges::all(LayoutLength::Px(3.0)))
                    .overflow(LayoutOverflow::Hidden, LayoutOverflow::Hidden),
                0x0000ffff,
            )
            .unwrap();
        reference
            .append_layout(
                reference.root(),
                fixed(size[0] * 0.5, size[1])
                    .padding(LayoutEdges::all(LayoutLength::Px(3.0)))
                    .overflow(LayoutOverflow::Hidden, LayoutOverflow::Hidden),
                0x00ff00ff,
            )
            .unwrap();
        let expected = reference.append_layout(parent, label(), 0).unwrap();
        let mut tx = Transaction::default();
        tx.text(
            expected,
            Some(TextPaint {
                resource: text.clone(),
                color,
            }),
        );
        reference
            .commit(tx, BackendCapabilities::default())
            .unwrap();
        let full = reference
            .frame_in_viewport(constraints, [240.0, 180.0])
            .unwrap();
        assert_eq!(
            ui.layout_result(commit.result.value).unwrap(),
            reference.layout_result(expected).unwrap()
        );
        assert_eq!(pixels(frame.resolve()), pixels(full.clone()));
        #[cfg(feature = "retained-wgpu")]
        if let Some(gpu) = &mut gpu {
            gpu.submit_local(&frame, [240, 180], 1.0).unwrap();
            assert_eq!(
                gpu.image().unwrap().as_raw(),
                gpu.reference_image(&full, [240, 180], 1.0)
                    .unwrap()
                    .as_raw()
            );
        }
        let before = ui.published_local_frame().unwrap().clone();
        assert!(
            ui.edit_local_frame(
                constraints,
                [240.0, 180.0],
                |_| Ok(()),
                |(), edit| {
                    edit.allocation_builder(outer, |_, edit| {
                        edit.properties().text(commit.result.value, None);
                        // Equal numeric allocation cannot authorize a new item policy.
                        edit.properties().layout_style(
                            sibling,
                            host().max_size(LayoutDimension::Px(500.0), LayoutDimension::Auto),
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
    drop((ui, text, raster, system));
    let (frame, expected) = old.unwrap();
    assert_eq!(pixels(frame.resolve()), expected);
    #[cfg(feature = "retained-wgpu")]
    if let Some(gpu) = &mut gpu {
        gpu.finish().unwrap();
    }
}
