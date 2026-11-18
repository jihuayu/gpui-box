use super::*;
use gpui_box::retained::logical::*;
use std::{cell::Cell, rc::Rc};

#[derive(Clone, PartialEq)]
struct Label {
    lower: u32,
    upper: u32,
    text: TextResource,
    color: u32,
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
fn prop_batch_noto_interval_and_text_inputs_match_complete_numeric_reference() {
    run(false);
}
#[cfg(feature = "retained-wgpu")]
#[test]
#[ignore = "requires WGPU adapter; run explicitly"]
fn hardware_prop_batch_noto_matches_complete_numeric_reference() {
    run(true);
}
fn run(hardware: bool) {
    let system = text_system();
    let mut raster = GlyphRasterizer::new(system.clone());
    let long_paragraph = shape(&system, "one two three four 中文段落 long text");
    let short_paragraph = shape(&system, "short");
    let long = raster.prepare(long_paragraph.clone(), 1.0).unwrap();
    let short = raster.prepare(short_paragraph.clone(), 1.0).unwrap();
    let constraints = LayoutConstraints::new(
        LayoutAvailableSpace::Unbounded,
        LayoutAvailableSpace::Unbounded,
    );
    let mut ui =
        LogicalUi::new_layout(LayoutStyle::column(), 0, BackendCapabilities::default()).unwrap();
    let owner = ui.root_scope();
    let root = ui.root_render();
    let source = ui
        .state(owner, 0, || Label {
            lower: 10,
            upper: 110,
            text: long.clone(),
            color: 0x00ffffff,
        })
        .unwrap();
    let mounts = Rc::new(Cell::new(0));
    let count = mounts.clone();
    let evals = Rc::new(Cell::new(0));
    let evaluations = evals.clone();
    let producer = LayoutProducer::new(move |_, edit| {
        let value = edit.read(source)?.clone();
        let scope = edit.reconcile_keys(owner, &[7])?[0];
        let (lower, upper, text, color) = edit.props(|props| {
            Ok((
                props.prop(scope, 0, value.lower)?,
                props.prop(scope, 1, value.upper)?,
                props.prop(scope, 2, value.text)?,
                props.prop(scope, 3, value.color)?,
            ))
        })?;
        if edit.renders(scope)?.is_empty() {
            count.set(count.get() + 1);
            let style = edit.computed(
                scope,
                4,
                &[lower.dependency(), upper.dependency()],
                move |i| {
                    let (a, b) = (*i.read_prop(lower)?, *i.read_prop(upper)?);
                    if a > b {
                        return Err(Error::InvalidStyle);
                    }
                    Ok(LayoutStyle::column()
                        .size(LayoutDimension::Px((b - a) as f32), LayoutDimension::Auto))
                },
            )?;
            let calls = evaluations.clone();
            let paint = edit.computed(
                scope,
                5,
                &[
                    lower.dependency(),
                    upper.dependency(),
                    text.dependency(),
                    color.dependency(),
                ],
                move |i| {
                    calls.set(calls.get() + 1);
                    if i.read_prop(lower)? > i.read_prop(upper)? {
                        return Err(Error::InvalidStyle);
                    }
                    Ok(Some(TextPaint {
                        resource: i.read_prop(text)?.clone(),
                        color: *i.read_prop(color)?,
                    }))
                },
            )?;
            let title = edit.append_layout(scope, root, LayoutStyle::column(), 0x0000ffff)?;
            edit.bind(style, Property::layout_style(title))?;
            edit.bind(paint, Property::text(title))?;
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
    let scope = ui.children(owner).unwrap()[0];
    let title = ui.renders(scope).unwrap()[0];
    #[cfg(feature = "retained-wgpu")]
    let mut gpu = hardware.then(|| wgpu::WgpuFrameRenderer::new(2).unwrap());
    #[cfg(feature = "retained-wgpu")]
    if let Some(gpu) = &gpu {
        eprintln!("PropBatch Noto adapter: {:?}", gpu.adapter_info());
    }
    #[cfg(not(feature = "retained-wgpu"))]
    assert!(!hardware);
    let mut old = None;
    for (index, (lower, upper, text, paragraph, color)) in [
        (10, 110, long.clone(), long_paragraph.clone(), 0x00ffffff),
        (200, 340, short.clone(), short_paragraph.clone(), 0xff00ffff),
        (400, 480, long.clone(), long_paragraph.clone(), 0xffff00ff),
    ]
    .into_iter()
    .enumerate()
    {
        if index > 0 {
            ui.transaction(|tx| {
                tx.set(
                    source,
                    Label {
                        lower,
                        upper,
                        text: text.clone(),
                        color,
                    },
                )
            })
            .unwrap();
        }
        assert_eq!(mounts.get(), 1);
        assert_eq!(
            evals.get(),
            index + 1,
            "whole paint expression sees one final tuple"
        );
        let width = (upper - lower) as f32;
        let metrics = paragraph.layout(Some(width)).unwrap();
        let mut full = UiTree::new_layout(fixed(width, metrics.size[1]), 0).unwrap();
        full.set_frame_build_policy(FrameBuildPolicy::Full);
        let node = full
            .append_layout(
                full.root(),
                LayoutStyle::column().size(LayoutDimension::Px(width), LayoutDimension::Auto),
                0x0000ffff,
            )
            .unwrap();
        let mut tx = Transaction::default();
        tx.text(
            node,
            Some(TextPaint {
                resource: text,
                color,
            }),
        );
        full.commit(tx, BackendCapabilities::default()).unwrap();
        let reference = full.frame_in_viewport(constraints, [300.0, 180.0]).unwrap();
        let frame = ui.published_local_frame().unwrap().clone();
        assert_eq!(
            ui.layout_result(title).unwrap(),
            full.layout_result(node).unwrap()
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
