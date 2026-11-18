use super::*;
use gpui_box::retained::logical::*;
use std::{cell::Cell, rc::Rc};

fn fixed(w: f32, h: f32) -> LayoutStyle {
    LayoutStyle::column()
        .size(LayoutDimension::Px(w), LayoutDimension::Px(h))
        .flex_shrink(0.0)
}
fn pixels(packet: FramePacket) -> Vec<u8> {
    let mut job = software::SoftwareJob::new(
        packet,
        software::SoftwareOptions {
            size: [300, 180],
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
fn content_producer_noto_intrinsic_grid_matches_numeric_full_flex() {
    run(false);
}
#[cfg(feature = "retained-wgpu")]
#[test]
#[ignore = "requires WGPU adapter; run explicitly"]
fn hardware_content_producer_noto_matches_numeric_complete_reference() {
    run(true);
}
fn run(hardware: bool) {
    let system = text_system();
    let mut raster = GlyphRasterizer::new(system.clone());
    let long_paragraph = shape(&system, "one two 中文段落");
    let short_paragraph = shape(&system, "short");
    let long = raster.prepare(long_paragraph.clone(), 1.0).unwrap();
    let short = raster.prepare(short_paragraph.clone(), 1.0).unwrap();
    let constraints = LayoutConstraints::new(
        LayoutAvailableSpace::Unbounded,
        LayoutAvailableSpace::Unbounded,
    );
    let mut ui = LogicalUi::new_layout(
        LayoutStyle::grid(GridStyle::default().columns(vec![GridTrack::auto().into()])),
        0,
        BackendCapabilities::default(),
    )
    .unwrap();
    let owner = ui.root_scope();
    let root = ui.root_render();
    let title = ui
        .append_render_layout(owner, root, LayoutStyle::column(), 0x0000ffff)
        .unwrap();
    ui.append_render_layout(owner, root, fixed(18.0, 12.0), 0x00ff00ff)
        .unwrap();
    let text = ui.state(owner, 0, || long.clone()).unwrap();
    let calls = Rc::new(Cell::new(0));
    let count = calls.clone();
    let producer = LayoutProducer::new(move |input, edit| {
        assert_eq!(input, LayoutBuildInput::Content);
        count.set(count.get() + 1);
        let resource = edit.read(text)?.clone();
        edit.properties().text(
            title,
            Some(TextPaint {
                resource,
                color: 0x00ffffff,
            }),
        );
        Ok(())
    });
    ui.edit_local_frame(
        constraints,
        [300.0, 180.0],
        |_| Ok(()),
        |(), edit| {
            edit.layout_producer(owner, 0, title, LayoutBuildKind::Content, &[], producer)?;
            Ok(())
        },
    )
    .unwrap();
    #[cfg(feature = "retained-wgpu")]
    let mut gpu = hardware.then(|| wgpu::WgpuFrameRenderer::new(2).unwrap());
    #[cfg(feature = "retained-wgpu")]
    if let Some(gpu) = &gpu {
        eprintln!("content Noto adapter: {:?}", gpu.adapter_info());
    }
    #[cfg(not(feature = "retained-wgpu"))]
    assert!(!hardware);
    let mut old = None;
    for (index, (resource, paragraph)) in [
        (long.clone(), long_paragraph.clone()),
        (short.clone(), short_paragraph.clone()),
        (long.clone(), long_paragraph.clone()),
    ]
    .into_iter()
    .enumerate()
    {
        if index > 0 {
            ui.transaction(|tx| tx.set(text, resource.clone())).unwrap();
        }
        assert_eq!(calls.get(), index + 1);
        let frame = ui.published_local_frame().unwrap().clone();
        let measured = paragraph.layout(None).unwrap();
        let width = measured.size[0].max(18.0);
        let mut full = UiTree::new_layout(fixed(width, measured.size[1] + 12.0), 0).unwrap();
        full.set_frame_build_policy(FrameBuildPolicy::Full);
        let title_ref = full
            .append_layout(full.root(), LayoutStyle::column(), 0x0000ffff)
            .unwrap();
        full.append_layout(full.root(), fixed(18.0, 12.0), 0x00ff00ff)
            .unwrap();
        let mut tx = Transaction::default();
        tx.text(
            title_ref,
            Some(TextPaint {
                resource,
                color: 0x00ffffff,
            }),
        );
        full.commit(tx, BackendCapabilities::default()).unwrap();
        let reference = full.frame_in_viewport(constraints, [300.0, 180.0]).unwrap();
        assert_eq!(
            ui.layout_result(root).unwrap().size,
            [width, measured.size[1] + 12.0]
        );
        assert_eq!(
            ui.layout_result(title).unwrap(),
            full.layout_result(title_ref).unwrap()
        );
        assert_eq!(pixels(frame.resolve()), pixels(reference.clone()));
        #[cfg(feature = "retained-wgpu")]
        if let Some(gpu) = &mut gpu {
            gpu.submit_local(&frame, [300, 180], 1.0).unwrap();
            assert_eq!(
                gpu.image().unwrap().as_raw(),
                gpu.reference_image(&reference, [300, 180], 1.0)
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
