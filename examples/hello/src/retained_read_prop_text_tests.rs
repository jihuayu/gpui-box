use super::*;
use gpui_box::retained::logical::*;
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

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
fn read_prop_noto_payload_updates_without_remounting_and_matches_full_reference() {
    run(false);
}
#[cfg(feature = "retained-wgpu")]
#[test]
#[ignore = "requires WGPU adapter; run explicitly"]
fn hardware_read_prop_noto_payload_matches_complete_reference() {
    run(true);
}
fn run(hardware: bool) {
    let system = text_system();
    let mut raster = GlyphRasterizer::new(system.clone());
    let long_paragraph = shape(&system, "one two three 中文段落");
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
    let source = ui
        .state(owner, 0, || {
            Some(TextPaint {
                resource: long.clone(),
                color: 0x00ffffff,
            })
        })
        .unwrap();
    let mounts = Rc::new(Cell::new(0));
    let count = mounts.clone();
    let installed = Rc::new(RefCell::new(None::<(ReadProp<Option<TextPaint>>, NodeId)>));
    let handles = installed.clone();
    let producer = LayoutProducer::new(move |_, edit| {
        let value = edit.read(source)?.clone();
        let scope = edit.reconcile_keys(owner, &[7])?[0];
        let prop = edit.prop(scope, 0, value)?;
        if edit.renders(scope)?.is_empty() {
            count.set(count.get() + 1);
            let title = edit.append_layout(scope, root, LayoutStyle::column(), 0x0000ffff)?;
            edit.bind_prop(prop, Property::text(title))?;
            *handles.borrow_mut() = Some((prop, title));
        }
        Ok(())
    });
    ui.edit_local_frame(
        constraints,
        [300.0, 180.0],
        |_| Ok(()),
        |(), edit| {
            edit.layout_producer(owner, 0, root, LayoutBuildKind::Content, &[], producer)?;
            Ok(())
        },
    )
    .unwrap();
    let (prop, title) = installed.borrow().unwrap();
    #[cfg(feature = "retained-wgpu")]
    let mut gpu = hardware.then(|| wgpu::WgpuFrameRenderer::new(2).unwrap());
    #[cfg(feature = "retained-wgpu")]
    if let Some(gpu) = &gpu {
        eprintln!("ReadProp Noto adapter: {:?}", gpu.adapter_info());
    }
    #[cfg(not(feature = "retained-wgpu"))]
    assert!(!hardware);
    let mut old = None;
    for (index, (resource, paragraph, color)) in [
        (long.clone(), long_paragraph.clone(), 0x00ffffff),
        (short.clone(), short_paragraph.clone(), 0xff00ffff),
        (long.clone(), long_paragraph.clone(), 0xffff00ff),
    ]
    .into_iter()
    .enumerate()
    {
        let paint = TextPaint { resource, color };
        if index > 0 {
            ui.transaction(|tx| tx.set(source, Some(paint.clone())))
                .unwrap();
        }
        assert_eq!(mounts.get(), 1);
        assert_eq!(ui.read_prop(prop).unwrap(), &Some(paint.clone()));
        let metrics = paragraph.layout(None).unwrap();
        let mut full = UiTree::new_layout(fixed(metrics.size[0], metrics.size[1]), 0).unwrap();
        full.set_frame_build_policy(FrameBuildPolicy::Full);
        let text = full
            .append_layout(full.root(), LayoutStyle::column(), 0x0000ffff)
            .unwrap();
        let mut tx = Transaction::default();
        tx.text(text, Some(paint));
        full.commit(tx, BackendCapabilities::default()).unwrap();
        let reference = full.frame_in_viewport(constraints, [300.0, 180.0]).unwrap();
        let frame = ui.published_local_frame().unwrap().clone();
        assert_eq!(
            ui.layout_result(title).unwrap(),
            full.layout_result(text).unwrap()
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
