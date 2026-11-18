//! Real Noto rows enter a bounded list through ordinary keyed components/Flex.
use super::*;
use gpui_box::retained::{logical::*, virtual_list::*};
#[derive(Clone, PartialEq)]
struct Props {
    paint: TextPaint,
    height: f32,
}
struct Item;
struct NaturalItem;
impl Component for NaturalItem {
    type Props = TextPaint;
    fn mount(
        props: ReadProp<TextPaint>,
        cx: &mut ComponentContext<'_, '_, '_, '_>,
    ) -> Result<(), Error> {
        let node = cx.append_layout(natural_style(), 0x183345ff)?;
        let text = cx.computed(0, &[props.dependency()], move |i| {
            Ok(Some(i.read_prop(props)?.clone()))
        })?;
        cx.bind(text, Property::text(node))?;
        Ok(())
    }
}
fn natural_style() -> LayoutStyle {
    LayoutStyle::column()
        .size(LayoutDimension::Percent(1.0), LayoutDimension::Auto)
        .min_size(LayoutDimension::Px(0.0), LayoutDimension::Auto)
        .flex_shrink(0.0)
}
#[test]
fn measured_virtual_list_real_noto_wrap_and_resize_match_full_flex() {
    run_measured(false);
}
#[cfg(feature = "retained-wgpu")]
#[test]
#[ignore = "requires WGPU adapter; run explicitly"]
fn hardware_measured_virtual_list_real_noto_matches_full_reference() {
    run_measured(true);
}
fn run_measured(hardware: bool) {
    let system = text_system();
    let mut raster = GlyphRasterizer::new(system.clone());
    let long = raster
        .prepare(
            shape(
                &system,
                "one two three four five 中文段落 six seven eight nine",
            ),
            1.25,
        )
        .unwrap();
    let short = raster.prepare(shape(&system, "short 中文"), 1.25).unwrap();
    let paints = (0..40)
        .map(|i| TextPaint {
            resource: if i % 2 == 0 {
                long.clone()
            } else {
                short.clone()
            },
            color: 0xffffffff,
        })
        .collect::<Vec<_>>();
    let mut ui =
        LogicalUi::new_layout(fixed(120.0, 100.0), 0, BackendCapabilities::default()).unwrap();
    let root = ui.root_scope();
    let render = ui.root_render();
    let data = paints.clone();
    let items = ui
        .state(root, 0, || {
            VirtualListItems::<NaturalItem>::keyed(
                VirtualListMetrics::fixed(40, 24.0).unwrap(),
                0..40,
                move |i| Ok(data[i].clone()),
            )
            .unwrap()
        })
        .unwrap();
    let scroll = ui.state(root, 1, || [0.0; 2]).unwrap();
    let mut props = MeasuredVirtualListProps::new(items, scroll, [100.0, 80.0]);
    props.overscan = 80.0;
    ui.edit_local_frame(
        LayoutConstraints::viewport(120.0, 100.0),
        [120.0, 100.0],
        |_| Ok(()),
        |(), e| {
            e.components::<MeasuredVirtualList<NaturalItem>>(root, render, [(0, props.clone())])?;
            Ok(())
        },
    )
    .unwrap();
    #[cfg(feature = "retained-wgpu")]
    let mut gpu = hardware.then(|| wgpu::WgpuFrameRenderer::new(2).unwrap());
    #[cfg(not(feature = "retained-wgpu"))]
    assert!(!hardware);
    let mut old = None;
    for (stage, width) in [100.0, 65.0, 110.0].into_iter().enumerate() {
        props.viewport[0] = width;
        ui.edit(
            |_| Ok(()),
            |(), e| {
                e.components::<MeasuredVirtualList<NaturalItem>>(
                    root,
                    render,
                    [(0, props.clone())],
                )?;
                Ok(())
            },
        )
        .unwrap();
        let first = ui.settle_virtual_list_measurements(100).unwrap();
        assert!(first.changed_rows > 0);
        assert!(first.measured_rows < 20, "only mounted rows are measured");
        // This test drives separate owner turns, as a host would. The API
        // itself never iterates toward convergence in one input/prepare call.
        for _ in 0..8 {
            if ui
                .settle_virtual_list_measurements(100)
                .unwrap()
                .changed_rows
                == 0
            {
                break;
            }
        }
        assert_eq!(
            ui.settle_virtual_list_measurements(100)
                .unwrap()
                .changed_rows,
            0
        );
        let frame = ui.published_local_frame().unwrap().clone();
        let mut full = UiTree::new_layout(fixed(120.0, 100.0), 0).unwrap();
        full.set_frame_build_policy(FrameBuildPolicy::Full);
        let parent = full
            .append_layout(
                full.root(),
                fixed(width, 80.0).overflow(LayoutOverflow::Clip, LayoutOverflow::Scroll),
                0,
            )
            .unwrap();
        let mut tx = Transaction::default();
        for p in &paints {
            let node = full
                .append_layout(parent, natural_style(), 0x183345ff)
                .unwrap();
            tx.text(node, Some(p.clone()));
        }
        full.commit(tx, BackendCapabilities::default()).unwrap();
        let reference = full.frame([120.0, 100.0]).unwrap();
        assert_eq!(
            pixels(frame.resolve()),
            pixels(reference.clone()),
            "complete CPU stage {stage}"
        );
        let wrapper = ui.children(root).unwrap()[0];
        let site = ui.children(wrapper).unwrap()[0];
        let list = ui.children(site).unwrap()[0];
        let row_site = ui.children(list).unwrap()[1];
        for row in ui.children(row_site).unwrap() {
            let index = ui.key(*row).unwrap() as usize;
            let measured = paints[index]
                .resource
                .paragraph()
                .layout(Some(width))
                .unwrap()
                .size[1];
            assert_eq!(
                ui.read(items).unwrap().metrics().height(index).unwrap(),
                measured
            );
        }
        #[cfg(feature = "retained-wgpu")]
        if let Some(gpu) = &mut gpu {
            eprintln!("measured Noto adapter: {:?}", gpu.adapter_info());
            gpu.submit_local(&frame, [150, 125], 1.25).unwrap();
            assert_eq!(
                gpu.image().unwrap().as_raw(),
                gpu.reference_image(&reference, [150, 125], 1.25)
                    .unwrap()
                    .as_raw()
            );
        }
        old.get_or_insert_with(|| (frame.clone(), pixels(frame.resolve())));
    }
    drop((ui, props, paints, raster, system, long, short));
    let (frame, bytes) = old.unwrap();
    assert_eq!(pixels(frame.resolve()), bytes);
}
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

#[test]
fn virtual_list_anchor_real_noto_preserves_reading_position_and_full_image() {
    run_anchor(false);
}
#[cfg(feature = "retained-wgpu")]
#[test]
#[ignore = "requires WGPU adapter; run explicitly"]
fn hardware_virtual_list_anchor_real_noto_matches_full_reference() {
    run_anchor(true);
}

fn run_anchor(hardware: bool) {
    let system = text_system();
    let mut raster = GlyphRasterizer::new(system.clone());
    let a = raster
        .prepare(shape(&system, "当前阅读 中文"), 1.25)
        .unwrap();
    let b = raster.prepare(shape(&system, "another row"), 1.25).unwrap();
    let mut rows = (0..40)
        .map(|index| {
            (
                index as u64,
                Props {
                    paint: TextPaint {
                        resource: if index % 2 == 0 { a.clone() } else { b.clone() },
                        color: 0xffffffff,
                    },
                    height: 32.0,
                },
            )
        })
        .collect::<Vec<_>>();
    fn dataset(rows: &[(u64, Props)]) -> VirtualListItems<Item> {
        let data = rows.to_vec();
        VirtualListItems::keyed(
            VirtualListMetrics::variable(data.iter().map(|(_, p)| p.height)).unwrap(),
            rows.iter().map(|(key, _)| *key),
            move |index| Ok(data[index].1.clone()),
        )
        .unwrap()
    }
    let mut ui =
        LogicalUi::new_layout(fixed(120.0, 100.0), 0, BackendCapabilities::default()).unwrap();
    let root = ui.root_scope();
    let render = ui.root_render();
    let items = ui.state(root, 0, || dataset(&rows)).unwrap();
    let scroll = ui.state(root, 1, || [0.0, 100.25]).unwrap();
    let site = ui
        .edit_local_frame(
            LayoutConstraints::viewport(120.0, 100.0),
            [120.0, 100.0],
            |_| Ok(()),
            |(), edit| {
                let site = edit.component_sites(root, &[0])?[0];
                edit.component_producer(
                    root,
                    0,
                    render,
                    &[],
                    ComponentProducer::new(move |i| {
                        let mut props =
                            VirtualListProps::new(i.read(items)?.clone(), scroll, [100.0, 80.0]);
                        props.overscan = 32.0;
                        let mut plan = ComponentPlan::new();
                        plan.select::<VirtualList<Item>>(site, render, 0, props);
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
    let list = ui.children(site.scope()).unwrap()[0];
    let host = ui.renders(list).unwrap()[0];
    let old = ui.published_local_frame().unwrap().clone();
    let old_pixels = pixels(old.resolve());
    #[cfg(feature = "retained-wgpu")]
    let mut gpu = hardware.then(|| wgpu::WgpuFrameRenderer::new(2).unwrap());
    #[cfg(not(feature = "retained-wgpu"))]
    assert!(!hardware);
    for stage in 0..3 {
        let expected = match stage {
            0 => {
                rows.insert(
                    0,
                    (
                        100,
                        Props {
                            paint: TextPaint {
                                resource: b.clone(),
                                color: 0xffaa77ff,
                            },
                            height: 64.0,
                        },
                    ),
                );
                164.25
            }
            1 => {
                rows[0].1.height = 96.0;
                196.25
            }
            _ => {
                rows.remove(0);
                100.25
            }
        };
        let next = dataset(&rows);
        let rebase = ui
            .transaction(|tx| {
                tx.rebase_virtual_list(
                    items,
                    scroll,
                    next,
                    80.0,
                    80.0,
                    VirtualListScrollPolicy::KeepVisible,
                )
            })
            .unwrap()
            .value;
        assert_eq!(rebase.key, Some(3));
        assert_eq!(rebase.offset, [0.0, expected]);
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
        for (_, p) in &rows {
            let node = full
                .append_layout(parent, fixed(100.0, p.height), 0x183345ff)
                .unwrap();
            tx.text(node, Some(p.paint.clone()));
        }
        tx.scroll(parent, [0.0, expected]);
        full.commit(tx, BackendCapabilities::default()).unwrap();
        let reference = full.frame([120.0, 100.0]).unwrap();
        assert_eq!(
            ui.layout_result(host).unwrap().scrollable_overflow,
            full.layout_result(parent).unwrap().scrollable_overflow
        );
        assert_eq!(
            pixels(frame.resolve()),
            old_pixels,
            "unchanged visible content stage {stage}"
        );
        assert_eq!(pixels(frame.resolve()), pixels(reference.clone()));
        #[cfg(feature = "retained-wgpu")]
        if let Some(gpu) = &mut gpu {
            eprintln!("anchor Noto adapter: {:?}", gpu.adapter_info());
            gpu.submit_local(&frame, [150, 125], 1.25).unwrap();
            assert_eq!(
                gpu.image().unwrap().as_raw(),
                gpu.reference_image(&reference, [150, 125], 1.25)
                    .unwrap()
                    .as_raw()
            );
        }
    }
    drop((ui, rows, a, b, raster, system));
    assert_eq!(pixels(old.resolve()), old_pixels);
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
