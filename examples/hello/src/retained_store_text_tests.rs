use super::*;
use gpui_box::{retained::logical::*, store::StoreWriter};
use std::{cell::Cell, rc::Rc};

#[derive(PartialEq)]
struct Model {
    width: u32,
    short: bool,
    color: u32,
}
struct Label;
impl Component for Label {
    type Props = (u32, TextPaint);
    fn mount(
        props: ReadProp<Self::Props>,
        cx: &mut ComponentContext<'_, '_, '_, '_>,
    ) -> Result<(), Error> {
        let style = cx.computed(0, &[props.dependency()], move |i| {
            Ok(LayoutStyle::column().size(
                LayoutDimension::Px(i.read_prop(props)?.0 as f32),
                LayoutDimension::Auto,
            ))
        })?;
        let paint = cx.computed(1, &[props.dependency()], move |i| {
            Ok(Some(i.read_prop(props)?.1.clone()))
        })?;
        let node = cx.append_layout(LayoutStyle::column(), 0x0000ffff)?;
        cx.bind(style, Property::layout_style(node))?;
        cx.bind(paint, Property::text(node))?;
        Ok(())
    }
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
fn business_store_noto_complete_snapshot_updates_match_complete_reference() {
    run(false);
}
#[cfg(feature = "retained-wgpu")]
#[test]
#[ignore = "requires WGPU adapter; run explicitly"]
fn hardware_business_store_noto_match_complete_reference() {
    run(true);
}
fn run(hardware: bool) {
    let system = text_system();
    let mut raster = GlyphRasterizer::new(system.clone());
    let long_paragraph = shape(&system, "one two three four 中文段落 long text");
    let short_paragraph = shape(&system, "short");
    let long = raster.prepare(long_paragraph.clone(), 1.0).unwrap();
    let short = raster.prepare(short_paragraph.clone(), 1.0).unwrap();
    let mut writer = StoreWriter::new(Model {
        width: 100,
        short: false,
        color: 0x00ffffff,
    })
    .unwrap();
    let mut ui =
        LogicalUi::new_layout(LayoutStyle::column(), 0, BackendCapabilities::default()).unwrap();
    let owner = ui.root_scope();
    let host = ui.root_render();
    let mut input = ui.attach_store(owner, 0, &writer.read_only()).unwrap();
    let selector = ui
        .select_store(owner, 1, input.prop(), |s| Ok((s.width, s.short, s.color)))
        .unwrap();
    let calls = Rc::new(Cell::new(0));
    let count = calls.clone();
    let (l, s) = (long.clone(), short.clone());
    let constraints = LayoutConstraints::new(
        LayoutAvailableSpace::Unbounded,
        LayoutAvailableSpace::Unbounded,
    );
    let site = ui
        .edit_local_frame(
            constraints,
            [300.0, 200.0],
            |_| Ok(()),
            |(), edit| {
                let site = edit.component_sites(owner, &[1])?[0];
                edit.component_producer(
                    owner,
                    0,
                    host,
                    &[],
                    ComponentProducer::new(move |i| {
                        count.set(count.get() + 1);
                        let (width, short, color) = *i.read_selector(selector)?;
                        let mut plan = ComponentPlan::new();
                        plan.select::<Label>(
                            site,
                            host,
                            0,
                            (
                                width,
                                TextPaint {
                                    resource: if short { s.clone() } else { l.clone() },
                                    color,
                                },
                            ),
                        );
                        Ok(plan)
                    }),
                )?;
                Ok(site)
            },
        )
        .unwrap()
        .0
        .result
        .value;
    let scope = ui.children(site.scope()).unwrap()[0];
    let node = ui.renders(scope).unwrap()[0];
    #[cfg(feature = "retained-wgpu")]
    let mut gpu = hardware.then(|| wgpu::WgpuFrameRenderer::new(2).unwrap());
    #[cfg(feature = "retained-wgpu")]
    if let Some(gpu) = &gpu {
        eprintln!("business store Noto adapter: {:?}", gpu.adapter_info());
    }
    #[cfg(not(feature = "retained-wgpu"))]
    assert!(!hardware);
    let mut old = None;
    for (index, (width, is_short, color)) in [
        (100, false, 0x00ffffff),
        (140, true, 0xff00ffff),
        (80, false, 0xffff00ff),
    ]
    .into_iter()
    .enumerate()
    {
        if index > 0 {
            let before = writer.read_only().snapshot().unwrap().revision();
            writer
                .commit(
                    before,
                    Model {
                        width: 25,
                        short: !is_short,
                        color: 0xffffffff,
                    },
                )
                .unwrap();
            let before = writer.read_only().snapshot().unwrap().revision();
            let committed = writer
                .commit(
                    before,
                    Model {
                        width,
                        short: is_short,
                        color,
                    },
                )
                .unwrap();
            assert_eq!(
                ui.apply_store(&mut input).unwrap().unwrap().revision,
                committed.revision
            );
        }
        assert_eq!(calls.get(), index + 1);
        assert_eq!(ui.children(site.scope()).unwrap(), [scope]);
        assert_eq!(ui.renders(scope).unwrap(), [node]);
        let paragraph = if is_short {
            &short_paragraph
        } else {
            &long_paragraph
        };
        let resource = if is_short {
            short.clone()
        } else {
            long.clone()
        };
        let metrics = paragraph.layout(Some(width as f32)).unwrap();
        let mut full = UiTree::new_layout(
            LayoutStyle::column()
                .size(
                    LayoutDimension::Px(width as f32),
                    LayoutDimension::Px(metrics.size[1]),
                )
                .flex_shrink(0.0),
            0,
        )
        .unwrap();
        full.set_frame_build_policy(FrameBuildPolicy::Full);
        let title = full
            .append_layout(
                full.root(),
                LayoutStyle::column()
                    .size(LayoutDimension::Px(width as f32), LayoutDimension::Auto),
                0x0000ffff,
            )
            .unwrap();
        let mut tx = Transaction::default();
        tx.text(title, Some(TextPaint { resource, color }));
        full.commit(tx, BackendCapabilities::default()).unwrap();
        let reference = full.frame_in_viewport(constraints, [300.0, 200.0]).unwrap();
        let frame = ui.published_local_frame().unwrap().clone();
        assert_eq!(
            ui.layout_result(node).unwrap(),
            full.layout_result(title).unwrap()
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
        input,
        writer,
        raster,
        system,
        long,
        short,
        long_paragraph,
        short_paragraph,
    ));
    let (frame, expected) = old.unwrap();
    assert_eq!(pixels(frame.resolve()), expected);
    #[cfg(feature = "retained-wgpu")]
    if let Some(gpu) = &mut gpu {
        gpu.finish().unwrap();
    }
}
