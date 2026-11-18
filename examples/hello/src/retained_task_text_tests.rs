//! Whole background results enter normal text/Flex source settlement.
use super::*;
use gpui_box::retained::logical::*;
fn fixed(w: f32, h: f32) -> LayoutStyle {
    LayoutStyle::column()
        .size(LayoutDimension::Px(w), LayoutDimension::Px(h))
        .flex_shrink(0.0)
}
fn pixels(frame: FramePacket) -> Vec<u8> {
    let mut job = software::SoftwareJob::new(
        frame,
        software::SoftwareOptions {
            size: [300, 240],
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
fn scoped_task_real_noto_results_follow_flex_and_match_full_reference() {
    run(false, false, false);
}
#[cfg(feature = "retained-wgpu")]
#[test]
#[ignore = "requires WGPU adapter; run explicitly"]
fn hardware_scoped_task_real_noto_matches_full_reference() {
    run(true, false, false);
}
#[test]
fn event_task_real_noto_results_follow_flex_and_match_full_reference() {
    run(false, true, false);
}
#[cfg(feature = "retained-wgpu")]
#[test]
#[ignore = "requires WGPU adapter; run explicitly"]
fn hardware_event_task_real_noto_matches_full_reference() {
    run(true, true, false);
}
#[test]
fn task_effect_real_noto_results_follow_flex_and_match_full_reference() {
    run(false, false, true);
}
#[cfg(feature = "retained-wgpu")]
#[test]
#[ignore = "requires WGPU adapter; run explicitly"]
fn hardware_task_effect_real_noto_matches_full_reference() {
    run(true, false, true);
}
fn run(hardware: bool, event_driven: bool, effect_driven: bool) {
    let system = text_system();
    let mut raster = GlyphRasterizer::new(system.clone());
    let paragraphs = [
        shape(&system, "one two three 中文任务结果"),
        shape(&system, "short 中文"),
        shape(&system, "one two three 中文任务结果"),
    ];
    let texts: Vec<_> = paragraphs
        .iter()
        .map(|p| raster.prepare(p.clone(), 1.0).unwrap())
        .collect();
    let mut ui =
        LogicalUi::new_layout(LayoutStyle::column(), 0, BackendCapabilities::default()).unwrap();
    let owner = ui.root_scope();
    let root = ui.root_render();
    let scope = ui.reconcile_keys(owner, &[1]).unwrap().order[0];
    let content = ui
        .state(scope, 0, || {
            Some(TextPaint {
                resource: texts[0].clone(),
                color: 0xffffffff,
            })
        })
        .unwrap();
    let style = ui
        .state(scope, 1, || {
            LayoutStyle::column()
                .size(LayoutDimension::Px(90.0), LayoutDimension::Auto)
                .flex_shrink(0.0)
        })
        .unwrap();
    let title = ui
        .append_render_layout(scope, root, LayoutStyle::column(), 0x153050ff)
        .unwrap();
    ui.bind(content, Property::text(title)).unwrap();
    ui.bind(style, Property::layout_style(title)).unwrap();
    let sibling = ui
        .append_render_layout(scope, root, fixed(70.0, 20.0), 0x55aa33ff)
        .unwrap();
    let constraints = LayoutConstraints::new(
        LayoutAvailableSpace::Unbounded,
        LayoutAvailableSpace::Unbounded,
    );
    ui.edit_local_frame(constraints, [300.0, 240.0], |_| Ok(()), |(), _| Ok(()))
        .unwrap();
    let workers = std::rc::Rc::new(std::cell::RefCell::new(
        None::<UiTaskContext<(TextPaint, f32)>>,
    ));
    let request = ui.state(scope, 2, || 0usize).unwrap();
    let received = std::rc::Rc::new(std::cell::RefCell::new(vec![]));
    if effect_driven {
        let workers = workers.clone();
        let received = received.clone();
        ui.task_effect(
            scope,
            0,
            request,
            UiTaskEffect::new(move |input, worker| {
                received.borrow_mut().push(input);
                *workers.borrow_mut() = Some(worker);
            }),
        )
        .unwrap();
    }
    if event_driven {
        let workers = workers.clone();
        ui.edit_local_frame(
            constraints,
            [300.0, 240.0],
            |_| Ok(()),
            |(), edit| {
                let handler = edit.event_handler(
                    scope,
                    0,
                    EventHandler::new(move |event, cx| {
                        if event.kind == gpui_box::retained::input::UiInputEventKind::Activate {
                            let workers = workers.clone();
                            cx.start_task(0, move |worker| *workers.borrow_mut() = Some(worker))?;
                        }
                        Ok(())
                    }),
                )?;
                edit.interaction(
                    scope,
                    title,
                    Some(gpui_box::retained::input::UiInteraction {
                        focus: gpui_box::retained::input::UiFocusPolicy::Tab,
                        activate_handler: Some(handler),
                        ..Default::default()
                    }),
                )?;
                Ok(())
            },
        )
        .unwrap();
    }
    #[cfg(feature = "retained-wgpu")]
    let mut gpu = hardware.then(|| wgpu::WgpuFrameRenderer::new(2).unwrap());
    #[cfg(feature = "retained-wgpu")]
    if let Some(gpu) = &gpu {
        eprintln!("task Noto adapter: {:?}", gpu.adapter_info());
    }
    #[cfg(not(feature = "retained-wgpu"))]
    assert!(!hardware);
    let old = ui.published_local_frame().unwrap().clone();
    let old_pixels = pixels(old.resolve());
    for (index, width) in [90.0, 145.0, 65.0].into_iter().enumerate() {
        let text = texts[index].clone();
        let color = [0xffffffff, 0xffaa33ff, 0x88ddffff][index];
        let task = if effect_driven {
            ui.transaction(|tx| {
                tx.set(request, 99)?;
                tx.set(request, index)
            })
            .unwrap();
            assert_eq!(*received.borrow(), (0..=index).collect::<Vec<_>>());
            ui.task::<(TextPaint, f32)>(scope, 0).unwrap().unwrap()
        } else if event_driven {
            use gpui_box::retained::input::*;
            let mut router = UiInputRouter::new();
            router
                .adopt(ui.input_scene().unwrap(), UiInputEvidence::ImageInstalled)
                .unwrap();
            ui.handle_input(&mut router, UiInput::PrimaryDown([2.0; 2]))
                .unwrap();
            ui.handle_input(&mut router, UiInput::PrimaryUp([2.0; 2]))
                .unwrap();
            ui.task::<(TextPaint, f32)>(scope, 0).unwrap().unwrap()
        } else {
            let slot = workers.clone();
            ui.start_task(scope, 0, move |cx: UiTaskContext<(TextPaint, f32)>| {
                *slot.borrow_mut() = Some(cx)
            })
            .unwrap()
        };
        let cx = workers.borrow_mut().take().unwrap();
        std::thread::spawn(move || {
            cx.complete((
                TextPaint {
                    resource: text,
                    color,
                },
                width,
            ))
            .unwrap()
        })
        .join()
        .unwrap();
        assert!(matches!(
            ui.apply_task(&task, |(paint, width), tx| {
                tx.set(content, Some(paint.clone()))?;
                tx.set(
                    style,
                    LayoutStyle::column()
                        .size(LayoutDimension::Px(*width), LayoutDimension::Auto)
                        .flex_shrink(0.0),
                )?;
                Ok(())
            })
            .unwrap(),
            TaskApply::Applied(_)
        ));
        let metrics = paragraphs[index].layout(Some(width)).unwrap();
        let mut full =
            UiTree::new_layout(fixed(width.max(70.0), metrics.size[1] + 20.0), 0).unwrap();
        full.set_frame_build_policy(FrameBuildPolicy::Full);
        let label = full
            .append_layout(
                full.root(),
                LayoutStyle::column()
                    .size(LayoutDimension::Px(width), LayoutDimension::Auto)
                    .flex_shrink(0.0),
                0x153050ff,
            )
            .unwrap();
        let after = full
            .append_layout(full.root(), fixed(70.0, 20.0), 0x55aa33ff)
            .unwrap();
        let mut tx = Transaction::default();
        tx.text(
            label,
            Some(TextPaint {
                resource: texts[index].clone(),
                color,
            }),
        );
        full.commit(tx, BackendCapabilities::default()).unwrap();
        let reference = full.frame_in_viewport(constraints, [300.0, 240.0]).unwrap();
        let frame = ui.published_local_frame().unwrap().clone();
        assert_eq!(
            ui.layout_result(title).unwrap(),
            full.layout_result(label).unwrap()
        );
        assert_eq!(
            ui.layout_result(sibling).unwrap(),
            full.layout_result(after).unwrap()
        );
        assert_eq!(pixels(frame.resolve()), pixels(reference.clone()));
        #[cfg(feature = "retained-wgpu")]
        if let Some(gpu) = &mut gpu {
            gpu.submit_local(&frame, [300, 240], 1.0).unwrap();
            assert_eq!(
                gpu.image().unwrap().as_raw(),
                gpu.reference_image(&reference, [300, 240], 1.0)
                    .unwrap()
                    .as_raw()
            );
        }
    }
    if effect_driven {
        // A changed request is pending when its owning component is removed.
        ui.transaction(|tx| tx.set(request, 99)).unwrap();
        let task = ui.task::<(TextPaint, f32)>(scope, 0).unwrap().unwrap();
        ui.reconcile_keys(owner, &[]).unwrap();
        assert_eq!(task.status(), UiTaskStatus::Cancelled);
        assert!(
            workers
                .borrow_mut()
                .take()
                .unwrap()
                .complete((
                    TextPaint {
                        resource: texts[0].clone(),
                        color: 0xffffffff,
                    },
                    90.0
                ))
                .is_err()
        );
    } else {
        let late = std::rc::Rc::new(std::cell::RefCell::new(None));
        let slot = late.clone();
        let task = ui
            .start_task(scope, 0, move |cx: UiTaskContext<u32>| {
                *slot.borrow_mut() = Some(cx)
            })
            .unwrap();
        ui.edit(
            |_| Ok(()),
            |(), edit| {
                edit.reconcile_keys(owner, &[])?;
                Ok(())
            },
        )
        .unwrap();
        assert_eq!(task.status(), UiTaskStatus::Cancelled);
        assert_eq!(late.borrow_mut().take().unwrap().complete(8), Err(8));
    }
    drop((ui, texts, paragraphs, raster, system));
    assert_eq!(pixels(old.resolve()), old_pixels);
    #[cfg(feature = "retained-wgpu")]
    if let Some(gpu) = &mut gpu {
        gpu.finish().unwrap();
    }
}
