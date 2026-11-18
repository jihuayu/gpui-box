//! Real Noto rows enter a bounded list through ordinary keyed components/Flex.
use super::*;
use gpui_box::retained::{logical::*, virtual_list::*};
#[derive(Clone, PartialEq)]
struct Props {
    paint: TextPaint,
    height: f32,
}
struct Item;
fn fixed(w: f32, h: f32) -> LayoutStyle {
    LayoutStyle::column()
        .size(LayoutDimension::Px(w), LayoutDimension::Px(h))
        .flex_shrink(0.0)
}
impl Component for Item {
    type Props = Props;
    fn mount(
        props: ReadProp<Props>,
        cx: &mut ComponentContext<'_, '_, '_, '_>,
    ) -> Result<(), Error> {
        let node = cx.append_layout(fixed(100.0, 32.0), 0x183345ff)?;
        let content = cx.computed(0, &[props.dependency()], move |i| {
            Ok(Some(i.read_prop(props)?.paint.clone()))
        })?;
        let style = cx.computed(1, &[props.dependency()], move |i| {
            Ok(fixed(100.0, i.read_prop(props)?.height))
        })?;
        cx.bind(content, Property::text(node))?;
        cx.bind(style, Property::layout_style(node))?;
        Ok(())
    }
}
#[test]
fn virtual_list_real_noto_rows_match_full_flex_and_complete_cpu_image() {
    run(false);
}
#[cfg(feature = "retained-wgpu")]
#[test]
#[ignore = "requires WGPU adapter; run explicitly"]
fn hardware_virtual_list_real_noto_matches_full_reference() {
    run(true);
}
fn pixels(frame: FramePacket) -> Vec<u8> {
    let mut job = software::SoftwareJob::new(
        frame,
        software::SoftwareOptions {
            size: [150, 125],
            scale: 1.25,
            background: 0,
            effects: pacing::SoftwareEffects::Exact,
            max_bytes: 1024 * 1024,
        },
    )
    .unwrap();
    while !job.step(4096).unwrap() {}
    job.into_frame().unwrap().pixels().to_vec()
}
fn run(hardware: bool) {
    let system = text_system();
    let mut raster = GlyphRasterizer::new(system.clone());
    let a = raster.prepare(shape(&system, "中文 list"), 1.25).unwrap();
    let b = raster.prepare(shape(&system, "short"), 1.25).unwrap();
    let data = Arc::new(
        (0..60)
            .map(|i| Props {
                paint: TextPaint {
                    resource: if i % 2 == 0 { a.clone() } else { b.clone() },
                    color: if i % 2 == 0 { 0xffdd77ff } else { 0xffffffff },
                },
                height: if i % 3 == 0 { 64.0 } else { 32.0 },
            })
            .collect::<Vec<_>>(),
    );
    let metrics = VirtualListMetrics::variable(data.iter().map(|p| p.height)).unwrap();
    let mut ui =
        LogicalUi::new_layout(fixed(120.0, 100.0), 0, BackendCapabilities::default()).unwrap();
    let root = ui.root_scope();
    let render = ui.root_render();
    let scroll = ui.state(root, 0, || [0.0; 2]).unwrap();
    let rows = data.clone();
    let props = VirtualListProps {
        items: VirtualListItems::<Item>::new(metrics, move |index| {
            Ok((index as u64, rows[index].clone()))
        }),
        scroll,
        viewport: [100.0, 80.0],
        overscan: 32.0,
        max_rows: 10,
    };
    let list = ui
        .edit_local_frame(
            LayoutConstraints::viewport(120.0, 100.0),
            [120.0, 100.0],
            |_| Ok(()),
            |(), edit| edit.components::<VirtualList<Item>>(root, render, [(0, props)]),
        )
        .unwrap()
        .0
        .result
        .value[0];
    let host = ui.renders(list).unwrap()[0];
    #[cfg(feature = "retained-wgpu")]
    let mut gpu = hardware.then(|| wgpu::WgpuFrameRenderer::new(2).unwrap());
    #[cfg(not(feature = "retained-wgpu"))]
    assert!(!hardware);
    let old = ui.published_local_frame().unwrap().clone();
    let old_pixels = pixels(old.resolve());
    for offset in [0.0, 31.25, 775.5, 2_000.25] {
        ui.transaction(|tx| tx.set(scroll, [0.0, offset])).unwrap();
        let frame = ui.published_local_frame().unwrap().clone();
        let mut full = UiTree::new_layout(fixed(120.0, 100.0), 0).unwrap();
        full.set_frame_build_policy(FrameBuildPolicy::Full);
        let parent = full
            .append_layout(
                full.root(),
                fixed(100.0, 80.0).overflow(LayoutOverflow::Clip, LayoutOverflow::Scroll),
                0,
            )
            .unwrap();
        let mut tx = Transaction::default();
        for p in data.iter() {
            let node = full
                .append_layout(parent, fixed(100.0, p.height), 0x183345ff)
                .unwrap();
            tx.text(node, Some(p.paint.clone()));
        }
        tx.scroll(parent, [0.0, offset]);
        full.commit(tx, BackendCapabilities::default()).unwrap();
        let reference = full.frame([120.0, 100.0]).unwrap();
        // Off-window text baselines are deliberately not measured. Compare the
        // scroll host's box/overflow; visible row text and baseline geometry are
        // exercised by the complete image rather than claiming hidden baselines.
        let actual = ui.layout_result(host).unwrap();
        let expected = full.layout_result(parent).unwrap();
        assert_eq!(
            (
                actual.location,
                actual.size,
                actual.content_size,
                actual.scrollable_overflow
            ),
            (
                expected.location,
                expected.size,
                expected.content_size,
                expected.scrollable_overflow
            )
        );
        assert_eq!(
            pixels(frame.resolve()),
            pixels(reference.clone()),
            "offset {offset}"
        );
        #[cfg(feature = "retained-wgpu")]
        if let Some(gpu) = &mut gpu {
            eprintln!("virtual Noto adapter: {:?}", gpu.adapter_info());
            gpu.submit_local(&frame, [150, 125], 1.25).unwrap();
            assert_eq!(
                gpu.image().unwrap().as_raw(),
                gpu.reference_image(&reference, [150, 125], 1.25)
                    .unwrap()
                    .as_raw()
            );
        }
    }
    drop((ui, data, a, b, raster, system));
    assert_eq!(pixels(old.resolve()), old_pixels);
}
