use super::*;
use gpui_box::retained::logical::*;
use std::cell::RefCell;

fn root() -> LayoutStyle {
    LayoutStyle::column().size(LayoutDimension::Percent(1.0), LayoutDimension::Percent(1.0))
}
fn header() -> LayoutStyle {
    LayoutStyle::column()
        .flex_shrink(0.0)
        .min_size(LayoutDimension::Px(0.0), LayoutDimension::Auto)
        .padding(LayoutEdges::all(LayoutLength::Px(3.0)))
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
        .flex_shrink(0.0)
        .min_size(LayoutDimension::Px(0.0), LayoutDimension::Auto)
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
fn inferred_measurement_noto_plan_settles_wrapped_title_before_body_without_explicit_after() {
    run(false);
}
#[cfg(feature = "retained-wgpu")]
#[test]
#[ignore = "requires WGPU adapter; run explicitly"]
fn hardware_inferred_measurement_noto_plan_matches_complete_reference() {
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
    let heading = ui
        .append_render_layout(owner, ui.root_render(), header(), 0x0000ffff)
        .unwrap();
    let title = ui.append_render_layout(owner, heading, label(), 0).unwrap();
    let host = ui
        .append_render_layout(owner, ui.root_render(), body(), 0)
        .unwrap();
    #[cfg(feature = "retained-wgpu")]
    let mut gpu = hardware.then(|| wgpu::WgpuFrameRenderer::new(2).unwrap());
    #[cfg(feature = "retained-wgpu")]
    if let Some(gpu) = &gpu {
        eprintln!(
            "inferred measurement Noto adapter: {:?}",
            gpu.adapter_info()
        );
    }
    #[cfg(not(feature = "retained-wgpu"))]
    assert!(!hardware);
    let mut old = None;
    for (size, text, paragraph) in [
        ([100.0, 210.0], long.clone(), long_paragraph.clone()),
        ([180.0, 180.0], short.clone(), short_paragraph.clone()),
        ([140.0, 210.0], long.clone(), long_paragraph.clone()),
    ] {
        let title_height = paragraph.layout(Some(size[0] - 6.0)).unwrap().size[1] + 6.0;
        let remaining = (size[1] - title_height).max(0.0);
        let constraints = LayoutConstraints::viewport(size[0], size[1]);
        let log = RefCell::new(vec![]);
        let (commit, frame) = ui
            .edit_local_frame(
                constraints,
                [240.0; 2],
                |_| Ok(()),
                |(), edit| {
                    let mut plan = LayoutBuildPlan::new();
                    plan.add(
                        LayoutBuildTaskId(1),
                        host,
                        LayoutBuildKind::Allocation,
                        &[],
                        |input, edit, _| {
                            log.borrow_mut().push(1);
                            let LayoutBuildInput::Full(input) = input else {
                                panic!()
                            };
                            assert_eq!(
                                input.known_size,
                                LayoutSize::new(Some(size[0]), Some(remaining))
                            );
                            let branch = edit.reconcile_keys(owner, &[size[0] as u64])?[0];
                            if edit.renders(branch)?.is_empty() {
                                edit.append_layout(branch, host, fixed(400.0, 300.0), 0xff0000ff)?;
                            }
                            Ok(())
                        },
                    )?;
                    plan.add(
                        LayoutBuildTaskId(90),
                        heading,
                        LayoutBuildKind::InlineAllocation,
                        &[],
                        |_, edit, _| {
                            log.borrow_mut().push(90);
                            edit.properties().text(
                                title,
                                Some(TextPaint {
                                    resource: text.clone(),
                                    color: 0x00ffffff,
                                }),
                            );
                            Ok(())
                        },
                    )?;
                    edit.build_layout_plan(plan)
                },
            )
            .unwrap();
        assert_eq!(*log.borrow(), [90, 1]);
        assert_eq!(commit.result.value.peak_dependency_edges, 1);
        let mut full = UiTree::new_layout(root(), 0).unwrap();
        full.set_frame_build_policy(FrameBuildPolicy::Full);
        let h = full
            .append_layout(
                full.root(),
                fixed(size[0], title_height).padding(LayoutEdges::all(LayoutLength::Px(3.0))),
                0x0000ffff,
            )
            .unwrap();
        let t = full.append_layout(h, label(), 0).unwrap();
        let b = full
            .append_layout(
                full.root(),
                fixed(size[0], remaining).overflow(LayoutOverflow::Hidden, LayoutOverflow::Hidden),
                0,
            )
            .unwrap();
        full.append_layout(b, fixed(400.0, 300.0), 0xff0000ff)
            .unwrap();
        let mut tx = Transaction::default();
        tx.text(
            t,
            Some(TextPaint {
                resource: text.clone(),
                color: 0x00ffffff,
            }),
        );
        full.commit(tx, BackendCapabilities::default()).unwrap();
        let reference = full.frame_in_viewport(constraints, [240.0; 2]).unwrap();
        assert_eq!(
            ui.layout_result(title).unwrap(),
            full.layout_result(t).unwrap()
        );
        assert_eq!(pixels(frame.resolve()), pixels(reference.clone()));
        #[cfg(feature = "retained-wgpu")]
        if let Some(gpu) = &mut gpu {
            gpu.submit_local(&frame, [240, 240], 1.0).unwrap();
            assert_eq!(
                gpu.image().unwrap().as_raw(),
                gpu.reference_image(&reference, [240, 240], 1.0)
                    .unwrap()
                    .as_raw()
            );
        }
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
