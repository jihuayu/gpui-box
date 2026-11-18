use super::*;
use std::{cell::RefCell, rc::Rc};

fn root() -> LayoutStyle {
    LayoutStyle::column().size(LayoutDimension::Percent(1.0), LayoutDimension::Percent(1.0))
}
fn label() -> LayoutStyle {
    LayoutStyle::column()
        .min_size(LayoutDimension::Px(0.0), LayoutDimension::Auto)
        .flex_shrink(0.0)
}
fn body() -> LayoutStyle {
    LayoutStyle::column()
        .flex_basis(LayoutDimension::Px(0.0))
        .flex_grow(1.0)
        .min_size(LayoutDimension::Px(0.0), LayoutDimension::Px(0.0))
        .overflow(LayoutOverflow::Hidden, LayoutOverflow::Hidden)
}
fn fixed(width: f32, height: f32) -> LayoutStyle {
    LayoutStyle::column()
        .size(LayoutDimension::Px(width), LayoutDimension::Px(height))
        .flex_shrink(0.0)
}
fn metrics(height: f32) -> Option<MeasureSnapshot> {
    Some(MeasureSnapshot::Fixed(CustomMeasureSnapshot {
        width: 20.0,
        height,
        baseline: Some(5.0),
    }))
}
fn fixture() -> (LogicalUi, NodeId, NodeId, NodeId) {
    let mut ui = LogicalUi::new_layout(root(), 0, BackendCapabilities::default()).unwrap();
    let owner = ui.root_scope();
    let header = ui
        .append_render_layout(owner, ui.root_render(), label(), 0x00ff00ff)
        .unwrap();
    let inner = ui
        .append_render_layout(owner, header, label(), 0x0000ffff)
        .unwrap();
    let host = ui
        .append_render_layout(owner, ui.root_render(), body(), 0)
        .unwrap();
    ui.edit(
        |_| Ok(()),
        |(), edit| {
            edit.properties().measure(inner, metrics(15.0));
            Ok(())
        },
    )
    .unwrap();
    (ui, header, inner, host)
}
fn pixels(packet: FramePacket) -> Vec<u8> {
    let mut job = software::SoftwareJob::new(
        packet,
        software::SoftwareOptions {
            size: [180, 180],
            scale: 1.0,
            background: 0,
            effects: pacing::SoftwareEffects::Exact,
            max_bytes: 1024 * 1024,
        },
    )
    .unwrap();
    while !job.step(4096).unwrap() {}
    job.into_frame().unwrap().pixels().to_vec()
}
fn reference(size: [f32; 2], height: f32) -> FramePacket {
    let mut full = UiTree::new_layout(root(), 0).unwrap();
    full.set_frame_build_policy(FrameBuildPolicy::Full);
    let header = full
        .append_layout(full.root(), fixed(size[0], height), 0x00ff00ff)
        .unwrap();
    full.append_layout(header, fixed(size[0], height), 0x0000ffff)
        .unwrap();
    let body = full
        .append_layout(
            full.root(),
            fixed(size[0], size[1] - height)
                .overflow(LayoutOverflow::Hidden, LayoutOverflow::Hidden),
            0,
        )
        .unwrap();
    full.append_layout(body, fixed(400.0, 300.0), 0xff0000ff)
        .unwrap();
    full.frame_in_viewport(LayoutConstraints::viewport(size[0], size[1]), [180.0; 2])
        .unwrap()
}

#[test]
fn inferred_measurement_tasks_run_descendant_headers_before_body_and_match_full_reference() {
    run(false);
}
#[cfg(feature = "retained-wgpu")]
#[test]
#[ignore = "requires WGPU adapter; run explicitly"]
fn hardware_inferred_measurement_tasks_match_complete_reference() {
    run(true);
}
fn run(hardware: bool) {
    #[cfg(feature = "retained-wgpu")]
    let mut gpu = hardware.then(|| wgpu::WgpuFrameRenderer::new(2).unwrap());
    #[cfg(feature = "retained-wgpu")]
    if let Some(gpu) = &gpu {
        eprintln!("inferred measurement adapter: {:?}", gpu.adapter_info());
    }
    #[cfg(not(feature = "retained-wgpu"))]
    assert!(!hardware);
    for local in [false, true] {
        let (mut ui, _, inner, host) = fixture();
        let owner = ui.root_scope();
        let mut old = None;
        for (size, height) in [
            ([120.0, 90.0], 30.0),
            ([150.0, 120.0], 45.0),
            ([120.0, 90.0], 15.0),
        ] {
            let log = RefCell::new(vec![]);
            let build = |(), edit: &mut LogicalEdit<'_, '_, '_>| {
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
                            LayoutSize::new(Some(size[0]), Some(size[1] - height))
                        );
                        let branch = edit.reconcile_keys(owner, &[height as u64])?[0];
                        if edit.renders(branch)?.is_empty() {
                            edit.append_layout(branch, host, fixed(400.0, 300.0), 0xff0000ff)?;
                        }
                        Ok(())
                    },
                )?;
                // A task below the measured sibling, not only its root, must
                // settle first regardless of registration or numeric ID.
                plan.add(
                    LayoutBuildTaskId(90),
                    inner,
                    LayoutBuildKind::InlineAllocation,
                    &[],
                    |_, edit, _| {
                        log.borrow_mut().push(90);
                        edit.properties().measure(inner, metrics(height));
                        Ok(())
                    },
                )?;
                edit.build_layout_plan(plan)
            };
            let (stats, packet) = if local {
                let (commit, frame) = ui
                    .edit_local_frame(
                        LayoutConstraints::viewport(size[0], size[1]),
                        [180.0; 2],
                        |_| Ok(()),
                        build,
                    )
                    .unwrap();
                (commit.result.value, frame.resolve())
            } else {
                let (commit, frame) = ui
                    .edit_frame(
                        LayoutConstraints::viewport(size[0], size[1]),
                        [180.0; 2],
                        |_| Ok(()),
                        build,
                    )
                    .unwrap();
                (commit.result.value, frame)
            };
            assert_eq!(*log.borrow(), [90, 1]);
            assert_eq!(stats.tasks_executed, 2);
            assert_eq!(stats.peak_dependency_edges, 1);
            let full = reference(size, height);
            assert_eq!(pixels(packet.clone()), pixels(full.clone()));
            #[cfg(feature = "retained-wgpu")]
            if let Some(gpu) = &mut gpu {
                gpu.submit(&packet, [180, 180], 1.0).unwrap();
                assert_eq!(
                    gpu.image().unwrap().as_raw(),
                    gpu.reference_image(&full, [180, 180], 1.0)
                        .unwrap()
                        .as_raw()
                );
            }
            old.get_or_insert_with(|| (packet.clone(), pixels(packet)));
        }
        drop(ui);
        let (packet, expected) = old.unwrap();
        assert_eq!(pixels(packet), expected);
    }
    #[cfg(feature = "retained-wgpu")]
    if let Some(gpu) = &mut gpu {
        gpu.finish().unwrap();
    }
}

#[test]
fn inferred_measurement_producers_work_in_reverse_registration_order_and_source_transactions() {
    let (mut ui, header, inner, host) = fixture();
    let owner = ui.root_scope();
    let height = ui.state(owner, 0, || 30u32).unwrap();
    let log = Rc::new(RefCell::new(vec![]));
    let count = log.clone();
    let body_producer = LayoutProducer::new(move |input, edit| {
        count.borrow_mut().push(1);
        let LayoutBuildInput::Full(input) = input else {
            panic!()
        };
        assert_eq!(
            input.known_size.height,
            Some(90.0 - *edit.read(height)? as f32)
        );
        Ok(())
    });
    let count = log.clone();
    let header_producer = LayoutProducer::new(move |_, edit| {
        count.borrow_mut().push(2);
        let height = *edit.read(height)? as f32;
        edit.properties().measure(inner, metrics(height));
        Ok(())
    });
    ui.edit_local_frame(
        LayoutConstraints::viewport(120.0, 90.0),
        [180.0; 2],
        |_| Ok(()),
        |(), edit| {
            edit.layout_producer(
                owner,
                0,
                host,
                LayoutBuildKind::Allocation,
                &[],
                body_producer,
            )?;
            edit.layout_producer(
                owner,
                1,
                header,
                LayoutBuildKind::InlineAllocation,
                &[],
                header_producer,
            )?;
            Ok(())
        },
    )
    .unwrap();
    assert_eq!(*log.borrow(), [2, 1]);
    log.borrow_mut().clear();
    ui.transaction(|tx| tx.set(height, 45)).unwrap();
    assert_eq!(*log.borrow(), [2, 1]);
    assert_eq!(ui.layout_result(host).unwrap().size, [120.0, 45.0]);
    log.borrow_mut().clear();
    ui.transaction(|tx| tx.set(height, 45)).unwrap();
    assert!(log.borrow().is_empty());
}

#[test]
fn inferred_measurement_cycles_and_edge_limits_reject_before_any_callback() {
    for cycle in [false, true] {
        let (mut ui, header, _, host) = fixture();
        let source = ui.state(ui.root_scope(), 0, || 0u32).unwrap();
        let constraints = LayoutConstraints::viewport(120.0, 90.0);
        let before = ui.frame_in_viewport(constraints, [180.0; 2]).unwrap();
        let calls = RefCell::new(0);
        let result = ui.edit_frame(
            constraints,
            [180.0; 2],
            |tx| {
                tx.set(source, 1)?;
                Ok(())
            },
            |(), edit| {
                let mut plan = LayoutBuildPlan::with_limits(LayoutBuildLimits {
                    max_tasks: 4,
                    max_dependency_edges: usize::from(cycle) * 10,
                });
                plan.add(
                    LayoutBuildTaskId(1),
                    host,
                    LayoutBuildKind::Allocation,
                    &[],
                    |_, _, _| {
                        *calls.borrow_mut() += 1;
                        Ok(())
                    },
                )?;
                plan.add(
                    LayoutBuildTaskId(2),
                    header,
                    LayoutBuildKind::InlineAllocation,
                    if cycle { &[LayoutBuildTaskId(1)] } else { &[] },
                    |_, _, _| {
                        *calls.borrow_mut() += 1;
                        Ok(())
                    },
                )?;
                edit.build_layout_plan(plan)
            },
        );
        assert!(
            matches!(result, Err(Error::CyclicLayoutBuildPlan)) && cycle
                || matches!(result, Err(Error::LayoutBuildPlanLimit)) && !cycle
        );
        assert_eq!(*calls.borrow(), 0);
        assert_eq!(*ui.read(source).unwrap(), 0);
        assert_eq!(
            pixels(before),
            pixels(ui.frame_in_viewport(constraints, [180.0; 2]).unwrap())
        );
    }
}

#[test]
fn inferred_measurement_dynamic_predecessors_cannot_retroactively_precede_consumed_input() {
    let (mut ui, header, _, host) = fixture();
    let constraints = LayoutConstraints::viewport(120.0, 90.0);
    let before = ui.frame_in_viewport(constraints, [180.0; 2]).unwrap();
    assert!(matches!(
        ui.edit_frame(
            constraints,
            [180.0; 2],
            |_| Ok(()),
            |(), edit| {
                let mut plan = LayoutBuildPlan::new();
                plan.add(
                    LayoutBuildTaskId(1),
                    host,
                    LayoutBuildKind::Allocation,
                    &[],
                    |_, _, queue| {
                        queue.add(
                            LayoutBuildTaskId(2),
                            header,
                            LayoutBuildKind::InlineAllocation,
                            &[],
                            |_, _, _| Ok(()),
                        )?;
                        Ok(())
                    },
                )?;
                edit.build_layout_plan(plan)
            }
        ),
        Err(Error::InvalidLayoutBuildPlan)
    ));
    assert_eq!(
        pixels(before),
        pixels(ui.frame_in_viewport(constraints, [180.0; 2]).unwrap())
    );
}

#[test]
fn inferred_measurement_dynamic_descendant_tasks_settle_before_pending_body() {
    let (mut ui, header, inner, host) = fixture();
    let log = RefCell::new(vec![]);
    let (commit, _) = ui
        .edit_frame(
            LayoutConstraints::viewport(120.0, 90.0),
            [180.0; 2],
            |_| Ok(()),
            |(), edit| {
                let mut plan = LayoutBuildPlan::new();
                plan.add(
                    LayoutBuildTaskId(1),
                    host,
                    LayoutBuildKind::Allocation,
                    &[LayoutBuildTaskId(90)],
                    |input, _, _| {
                        let LayoutBuildInput::Full(input) = input else {
                            panic!()
                        };
                        log.borrow_mut().push(1);
                        assert_eq!(input.known_size.height, Some(45.0));
                        Ok(())
                    },
                )?;
                plan.add(
                    LayoutBuildTaskId(90),
                    header,
                    LayoutBuildKind::InlineAllocation,
                    &[],
                    |_, _, queue| {
                        log.borrow_mut().push(90);
                        queue.add(
                            LayoutBuildTaskId(99),
                            inner,
                            LayoutBuildKind::InlineAllocation,
                            &[],
                            |_, edit, _| {
                                log.borrow_mut().push(99);
                                edit.properties().measure(inner, metrics(45.0));
                                Ok(())
                            },
                        )?;
                        Ok(())
                    },
                )?;
                edit.build_layout_plan(plan)
            },
        )
        .unwrap();
    assert_eq!(*log.borrow(), [90, 99, 1]);
    assert_eq!(commit.result.value.tasks_executed, 3);
    // body<-header explicit+inferred is deduplicated; body<-inner and
    // inner<-header are separate measurement/ancestry prerequisites.
    assert_eq!(commit.result.value.peak_dependency_edges, 3);
    assert_eq!(ui.layout_result(host).unwrap().size, [120.0, 45.0]);
}
