use super::*;
use gpui_box::retained::logical::*;
use std::{cell::Cell, rc::Rc};

#[derive(Clone)]
struct LabelProps {
    width: u32,
    paint: TextPaint,
    mounts: Rc<Cell<usize>>,
}
impl PartialEq for LabelProps {
    fn eq(&self, other: &Self) -> bool {
        self.width == other.width
            && self.paint == other.paint
            && Rc::ptr_eq(&self.mounts, &other.mounts)
    }
}
struct Label;
impl Component for Label {
    type Props = LabelProps;
    fn mount(
        props: ReadProp<Self::Props>,
        cx: &mut ComponentContext<'_, '_, '_, '_>,
    ) -> Result<(), Error> {
        let initial = cx.read_prop(props)?.clone();
        let style = cx.computed(0, &[props.dependency()], move |i| {
            Ok(LayoutStyle::column().size(
                LayoutDimension::Px(i.read_prop(props)?.width as f32),
                LayoutDimension::Auto,
            ))
        })?;
        let paint = cx.computed(1, &[props.dependency()], move |i| {
            Ok(Some(i.read_prop(props)?.paint.clone()))
        })?;
        let node = cx.append_layout(LayoutStyle::column(), 0x0000ffff)?;
        cx.bind(style, Property::layout_style(node))?;
        cx.bind(paint, Property::text(node))?;
        cx.effect(0, &[], move |_| {
            initial.mounts.set(initial.mounts.get() + 1);
            None
        })?;
        Ok(())
    }
}
fn fixed(w: f32, h: f32) -> LayoutStyle {
    LayoutStyle::column()
        .size(LayoutDimension::Px(w), LayoutDimension::Px(h))
        .flex_shrink(0.0)
}
fn pixels(frame: FramePacket) -> Vec<u8> {
    let mut job = software::SoftwareJob::new(
        frame,
        software::SoftwareOptions {
            size: [300, 200],
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
fn typed_components_noto_payload_and_width_updates_match_complete_reference() {
    run(false);
}
#[cfg(feature = "retained-wgpu")]
#[test]
#[ignore = "requires WGPU adapter; run explicitly"]
fn hardware_typed_components_noto_match_complete_reference() {
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
    let host = ui.root_render();
    let mounts = Rc::new(Cell::new(0));
    let initial = LabelProps {
        width: 100,
        paint: TextPaint {
            resource: long.clone(),
            color: 0x00ffffff,
        },
        mounts: mounts.clone(),
    };
    let source = ui.state(owner, 0, || initial).unwrap();
    ui.edit_local_frame(
        constraints,
        [300.0, 200.0],
        |_| Ok(()),
        |(), edit| {
            edit.layout_producer(
                owner,
                0,
                host,
                LayoutBuildKind::Content,
                &[],
                LayoutProducer::new(move |_, edit| {
                    let value = edit.read(source)?.clone();
                    edit.components::<Label>(owner, host, [(7, value)])?;
                    Ok(())
                }),
            )?;
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
        eprintln!("typed component Noto adapter: {:?}", gpu.adapter_info());
    }
    #[cfg(not(feature = "retained-wgpu"))]
    assert!(!hardware);
    let mut old = None;
    for (index, (width, text, paragraph, color)) in [
        (100, long.clone(), long_paragraph.clone(), 0x00ffffff),
        (140, short.clone(), short_paragraph.clone(), 0xff00ffff),
        (80, long.clone(), long_paragraph.clone(), 0xffff00ff),
    ]
    .into_iter()
    .enumerate()
    {
        if index > 0 {
            ui.transaction(|tx| {
                tx.set(
                    source,
                    LabelProps {
                        width,
                        paint: TextPaint {
                            resource: text.clone(),
                            color,
                        },
                        mounts: mounts.clone(),
                    },
                )
            })
            .unwrap();
        }
        assert_eq!(mounts.get(), 1);
        assert_eq!(ui.children(owner).unwrap(), [scope]);
        assert_eq!(ui.renders(scope).unwrap(), [title]);
        let metrics = paragraph.layout(Some(width as f32)).unwrap();
        let mut full = UiTree::new_layout(fixed(width as f32, metrics.size[1]), 0).unwrap();
        full.set_frame_build_policy(FrameBuildPolicy::Full);
        let node = full
            .append_layout(
                full.root(),
                LayoutStyle::column()
                    .size(LayoutDimension::Px(width as f32), LayoutDimension::Auto),
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
        let reference = full.frame_in_viewport(constraints, [300.0, 200.0]).unwrap();
        let frame = ui.published_local_frame().unwrap().clone();
        assert_eq!(
            ui.layout_result(title).unwrap(),
            full.layout_result(node).unwrap()
        );
        assert_eq!(pixels(frame.resolve()), pixels(reference.clone()));
        #[cfg(feature = "retained-wgpu")]
        if let Some(gpu) = &mut gpu {
            gpu.submit_local(&frame, [300, 200], 1.0).unwrap();
            assert_eq!(
                gpu.image().unwrap().as_raw(),
                gpu.reference_image(&reference, [300, 200], 1.0)
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
