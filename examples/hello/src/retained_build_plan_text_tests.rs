use super::*;
use gpui_box::retained::logical::*;

fn constraints(width: f32) -> LayoutConstraints {
    LayoutConstraints::new(
        LayoutAvailableSpace::Definite(width),
        LayoutAvailableSpace::Unbounded,
    )
}
fn root_style() -> LayoutStyle {
    LayoutStyle::column().size(LayoutDimension::Percent(1.0), LayoutDimension::Auto)
}
fn host_style() -> LayoutStyle {
    LayoutStyle::column().padding(LayoutEdges::all(LayoutLength::Px(4.0)))
}
fn pixels(frame: FramePacket) -> Vec<u8> {
    let mut job = software::SoftwareJob::new(
        frame,
        software::SoftwareOptions {
            size: [300, 400],
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
fn scheduled_nested_builders_wrap_real_noto_against_direct_full_flex() {
    run(false);
}
#[cfg(feature = "retained-wgpu")]
#[test]
#[ignore = "requires WGPU adapter; run explicitly"]
fn hardware_scheduled_builders_real_noto_match_complete_reference() {
    run(true);
}
#[test]
fn persistent_producer_wraps_real_noto_from_automatic_source_and_resize_events() {
    run_persistent(false);
}
#[cfg(feature = "retained-wgpu")]
#[test]
#[ignore = "requires WGPU adapter; run explicitly"]
fn hardware_persistent_producer_real_noto_matches_full_reference() {
    run_persistent(true);
}
fn run_persistent(hardware: bool) {
    use std::{cell::Cell, rc::Rc};
    let system = text_system();
    let paragraph = shape(&system, "one two three four five six 中文段落");
    let mut raster = GlyphRasterizer::new(system.clone());
    let text = raster.prepare(paragraph.clone(), 1.0).unwrap();
    let mut ui = LogicalUi::new_layout(root_style(), 0, BackendCapabilities::default()).unwrap();
    let scope = ui.root_scope();
    let root = ui.root_render();
    let host = ui
        .append_render_layout(scope, root, host_style(), 0)
        .unwrap();
    let color = ui.state(scope, 0, || 0x00ffffffu32).unwrap();
    let calls = Rc::new(Cell::new(0));
    let count = calls.clone();
    let prepared = text.clone();
    let producer = LayoutProducer::new(move |_, edit| {
        count.set(count.get() + 1);
        let current = *edit.read(color)?;
        let item = edit.reconcile_keys(scope, &[0])?[0];
        let renders = edit.renders(item)?;
        let leaf = match renders.first() {
            Some(node) => *node,
            None => edit.append_layout(
                item,
                host,
                LayoutStyle::row().min_size(LayoutDimension::Px(0.0), LayoutDimension::Auto),
                0,
            )?,
        };
        edit.properties().text(
            leaf,
            Some(TextPaint {
                resource: prepared.clone(),
                color: current,
            }),
        );
        Ok(())
    });
    let (_, initial) = ui
        .edit_local_frame(
            constraints(80.0),
            [300.0, 400.0],
            |_| Ok(()),
            |(), edit| {
                edit.layout_producer(
                    scope,
                    0,
                    host,
                    LayoutBuildKind::InlineAllocation,
                    &[],
                    producer,
                )?;
                Ok(())
            },
        )
        .unwrap();
    #[cfg(feature = "retained-wgpu")]
    let mut gpu = hardware.then(|| wgpu::WgpuFrameRenderer::new(2).unwrap());
    #[cfg(feature = "retained-wgpu")]
    let mut submits = 0;
    #[cfg(feature = "retained-wgpu")]
    if let Some(gpu) = &gpu {
        eprintln!("persistent glyph adapter: {:?}", gpu.adapter_info());
    }
    #[cfg(not(feature = "retained-wgpu"))]
    assert!(!hardware);
    for (width, value) in [
        (80.0, 0x00ffffffu32),
        (180.0, 0x00ffffff),
        (180.0, 0xffcc00ff),
        (180.0, 0xffcc00ff),
        (120.0, 0xffcc00ff),
    ] {
        ui.local_frame_in_viewport(constraints(width), [300.0, 400.0])
            .unwrap();
        ui.transaction(|tx| tx.set(color, value)).unwrap();
        let frame = ui.published_local_frame().unwrap().clone();
        let leaf = ui.renders(ui.children(scope).unwrap()[0]).unwrap()[0];
        let mut reference = UiTree::new_layout(root_style(), 0).unwrap();
        reference.set_frame_build_policy(FrameBuildPolicy::Full);
        let outer = reference
            .append_layout(reference.root(), host_style(), 0)
            .unwrap();
        let expected = reference
            .append_layout(
                outer,
                LayoutStyle::row().min_size(LayoutDimension::Px(0.0), LayoutDimension::Auto),
                0,
            )
            .unwrap();
        let mut tx = Transaction::default();
        tx.text(
            expected,
            Some(TextPaint {
                resource: text.clone(),
                color: value,
            }),
        );
        reference
            .commit(tx, BackendCapabilities::default())
            .unwrap();
        let full = reference
            .frame_in_viewport(constraints(width), [300.0, 400.0])
            .unwrap();
        assert_eq!(
            ui.layout_result(leaf).unwrap(),
            reference.layout_result(expected).unwrap()
        );
        assert_eq!(pixels(frame.resolve()), pixels(full.clone()));
        #[cfg(feature = "retained-wgpu")]
        if let Some(gpu) = &mut gpu {
            submits += usize::from(gpu.submit_local(&frame, [300, 400], 1.0).unwrap().is_some());
            assert_eq!(
                gpu.image().unwrap().as_raw(),
                gpu.reference_image(&full, [300, 400], 1.0)
                    .unwrap()
                    .as_raw()
            );
        }
    }
    assert_eq!(
        calls.get(),
        4,
        "initial, two changed widths and one changed signal"
    );
    let saved = pixels(initial.resolve());
    drop((ui, text, paragraph, raster, system));
    assert_eq!(saved, pixels(initial.resolve()));
    #[cfg(feature = "retained-wgpu")]
    if let Some(gpu) = &mut gpu {
        assert_eq!(submits, 4);
        gpu.finish().unwrap();
    }
}
fn run(hardware: bool) {
    let system = text_system();
    let paragraph = shape(&system, "one two three four five six 中文段落");
    let mut raster = GlyphRasterizer::new(system.clone());
    let text = raster.prepare(paragraph.clone(), 1.0).unwrap();
    let mut ui = LogicalUi::new_layout(root_style(), 0, BackendCapabilities::default()).unwrap();
    let root = ui.root_render();
    let scope = ui.root_scope();
    let host = ui
        .append_render_layout(scope, root, host_style(), 0)
        .unwrap();
    let child = std::cell::Cell::new(None);
    let mut first = None;
    #[cfg(feature = "retained-wgpu")]
    let mut gpu = hardware.then(|| wgpu::WgpuFrameRenderer::new(2).unwrap());
    #[cfg(feature = "retained-wgpu")]
    let mut submissions = 0;
    #[cfg(feature = "retained-wgpu")]
    if let Some(gpu) = &gpu {
        eprintln!("build plan glyph adapter: {:?}", gpu.adapter_info());
    }
    #[cfg(not(feature = "retained-wgpu"))]
    assert!(!hardware);
    for width in [80.0, 180.0, 120.0, 180.0, 180.0] {
        let (_, frame) = ui
            .edit_local_frame(
                constraints(width),
                [300.0, 400.0],
                |_| Ok(()),
                |(), edit| {
                    let mut plan = LayoutBuildPlan::new();
                    let text = &text;
                    let child = &child;
                    plan.add(
                        LayoutBuildTaskId(1),
                        host,
                        LayoutBuildKind::InlineAllocation,
                        &[],
                        move |input, edit, plan| {
                            let LayoutBuildInput::Inline(input) = input else {
                                panic!()
                            };
                            assert_eq!(input.known_width, Some(width));
                            let branch = edit.reconcile_keys(scope, &[0])?[0];
                            let nodes = edit.renders(branch)?;
                            let inner = if let Some(node) = nodes.first() {
                                *node
                            } else {
                                edit.append_layout(branch, host, LayoutStyle::column(), 0)?
                            };
                            plan.add(
                                LayoutBuildTaskId(2),
                                inner,
                                LayoutBuildKind::InlineAllocation,
                                &[],
                                move |input, edit, _| {
                                    let LayoutBuildInput::Inline(input) = input else {
                                        panic!()
                                    };
                                    assert_eq!(input.known_width, Some(width - 8.0));
                                    let item = edit.reconcile_keys(branch, &[0])?[0];
                                    let nodes = edit.renders(item)?;
                                    let leaf = if let Some(node) = nodes.first() {
                                        *node
                                    } else {
                                        edit.append_layout(
                                            item,
                                            inner,
                                            LayoutStyle::row().min_size(
                                                LayoutDimension::Px(0.0),
                                                LayoutDimension::Auto,
                                            ),
                                            0,
                                        )?
                                    };
                                    edit.properties().text(
                                        leaf,
                                        Some(TextPaint {
                                            resource: text.clone(),
                                            color: 0x00ffffff,
                                        }),
                                    );
                                    child.set(Some(leaf));
                                    Ok(())
                                },
                            )?;
                            Ok(())
                        },
                    )?;
                    assert_eq!(edit.build_layout_plan(plan)?.tasks_executed, 2);
                    Ok(())
                },
            )
            .unwrap();
        let mut reference = UiTree::new_layout(root_style(), 0).unwrap();
        reference.set_frame_build_policy(FrameBuildPolicy::Full);
        let outer = reference
            .append_layout(reference.root(), host_style(), 0)
            .unwrap();
        let inner = reference
            .append_layout(outer, LayoutStyle::column(), 0)
            .unwrap();
        let leaf = reference
            .append_layout(
                inner,
                LayoutStyle::row().min_size(LayoutDimension::Px(0.0), LayoutDimension::Auto),
                0,
            )
            .unwrap();
        let mut tx = Transaction::default();
        tx.text(
            leaf,
            Some(TextPaint {
                resource: text.clone(),
                color: 0x00ffffff,
            }),
        );
        reference
            .commit(tx, BackendCapabilities::default())
            .unwrap();
        let full = reference
            .frame_in_viewport(constraints(width), [300.0, 400.0])
            .unwrap();
        assert_eq!(
            ui.layout_result(child.get().unwrap()).unwrap(),
            reference.layout_result(leaf).unwrap()
        );
        assert_eq!(pixels(frame.resolve()), pixels(full.clone()));
        #[cfg(feature = "retained-wgpu")]
        if let Some(gpu) = &mut gpu {
            submissions +=
                usize::from(gpu.submit_local(&frame, [300, 400], 1.0).unwrap().is_some());
            assert_eq!(
                gpu.image().unwrap().as_raw(),
                gpu.reference_image(&full, [300, 400], 1.0)
                    .unwrap()
                    .as_raw()
            );
        }
        first.get_or_insert(frame);
    }
    let first = first.unwrap();
    let expected = pixels(first.resolve());
    drop((ui, text, paragraph, raster, system));
    assert_eq!(pixels(first.resolve()), expected);
    #[cfg(feature = "retained-wgpu")]
    if let Some(gpu) = &mut gpu {
        assert_eq!(
            submissions, 4,
            "equal-width glyph output reuses its submission"
        );
        gpu.finish().unwrap();
    }
}
