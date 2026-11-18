use super::*;
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

fn fixed(width: f32, height: f32) -> LayoutStyle {
    LayoutStyle::column()
        .size(LayoutDimension::Px(width), LayoutDimension::Px(height))
        .flex_shrink(0.0)
}
fn constraints() -> LayoutConstraints {
    LayoutConstraints::new(
        LayoutAvailableSpace::Unbounded,
        LayoutAvailableSpace::Unbounded,
    )
}
fn root_style() -> LayoutStyle {
    LayoutStyle::row().align_items(LayoutAlignItems::Start)
}
fn pixels(frame: FramePacket) -> Vec<u8> {
    let mut job = software::SoftwareJob::new(
        frame,
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
fn reference(rows: &[(u64, u32)]) -> FramePacket {
    let mut tree = UiTree::new_layout(root_style(), 0).unwrap();
    tree.set_frame_build_policy(FrameBuildPolicy::Full);
    let list = tree
        .append_layout(tree.root(), LayoutStyle::column(), 0x0000ffff)
        .unwrap();
    for &(key, width) in rows {
        tree.append_layout(list, fixed(width as f32, 10.0), color(key))
            .unwrap();
    }
    tree.frame_in_viewport(constraints(), [180.0; 2]).unwrap()
}
fn color(key: u64) -> u32 {
    if key == 1 { 0xff0000ff } else { 0x00ff00ff }
}

#[test]
fn content_producers_build_natural_keyed_lists_and_match_complete_reference() {
    run(false);
}
#[cfg(feature = "retained-wgpu")]
#[test]
#[ignore = "requires WGPU adapter; run explicitly"]
fn hardware_content_producers_match_complete_natural_reference() {
    run(true);
}
fn run(hardware: bool) {
    #[cfg(feature = "retained-wgpu")]
    let mut gpu = hardware.then(|| wgpu::WgpuFrameRenderer::new(2).unwrap());
    #[cfg(feature = "retained-wgpu")]
    if let Some(gpu) = &gpu {
        eprintln!("content producer adapter: {:?}", gpu.adapter_info());
    }
    #[cfg(not(feature = "retained-wgpu"))]
    assert!(!hardware);
    for local in [false, true] {
        let mut ui =
            LogicalUi::new_layout(root_style(), 0, BackendCapabilities::default()).unwrap();
        let owner = ui.root_scope();
        let host = ui
            .append_render_layout(owner, ui.root_render(), LayoutStyle::column(), 0x0000ffff)
            .unwrap();
        let rows = ui.state(owner, 0, || vec![(1u64, 20u32), (2, 30)]).unwrap();
        let unrelated = ui.state(owner, 1, || 0u32).unwrap();
        let calls = Rc::new(Cell::new(0));
        let count = calls.clone();
        let producer = LayoutProducer::new(move |input, edit| {
            assert_eq!(input, LayoutBuildInput::Content);
            count.set(count.get() + 1);
            let rows = edit.read(rows)?.clone();
            let scopes =
                edit.reconcile_keys(owner, &rows.iter().map(|r| r.0).collect::<Vec<_>>())?;
            for (scope, &(key, width)) in scopes.into_iter().zip(&rows) {
                let height = edit.state(scope, 100, || 10u32)?;
                let style = fixed(width as f32, *edit.read(height)? as f32);
                let renders = edit.renders(scope)?;
                if let Some(&node) = renders.first() {
                    edit.properties().layout_style(node, style);
                } else {
                    edit.append_layout(scope, host, style, color(key))?;
                }
            }
            Ok(())
        });
        let build = |(), edit: &mut LogicalEdit<'_, '_, '_>| {
            edit.layout_producer(owner, 0, host, LayoutBuildKind::Content, &[], producer)?;
            Ok(())
        };
        if local {
            ui.edit_local_frame(constraints(), [180.0; 2], |_| Ok(()), build)
                .unwrap();
        } else {
            ui.edit_frame(constraints(), [180.0; 2], |_| Ok(()), build)
                .unwrap();
        }
        let first_scopes = ui.children(owner).unwrap().to_vec();
        let retained_node = ui.renders(first_scopes[0]).unwrap()[0];
        let mut old = None;
        for (index, model) in [
            vec![(1, 20), (2, 30)],
            vec![(2, 40), (1, 25)],
            vec![(1, 15), (3, 35)],
        ]
        .into_iter()
        .enumerate()
        {
            if index > 0 {
                ui.transaction(|tx| tx.set(rows, model.clone())).unwrap();
            }
            assert_eq!(calls.get(), index + 1);
            let packet = ui.frame_in_viewport(constraints(), [180.0; 2]).unwrap();
            assert_eq!(calls.get(), index + 1, "equal frame reuses content");
            let full = reference(&model);
            assert_eq!(pixels(packet.clone()), pixels(full.clone()));
            assert_eq!(
                ui.layout_result(host).unwrap().size,
                [model.iter().map(|r| r.1).max().unwrap() as f32, 20.0]
            );
            let scope = ui.children(owner).unwrap()[if index == 1 { 1 } else { 0 }];
            assert_eq!(scope, first_scopes[0]);
            assert_eq!(ui.renders(scope).unwrap(), [retained_node]);
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
        assert!(ui.renders(first_scopes[1]).is_err());
        ui.transaction(|tx| tx.set(unrelated, 1)).unwrap();
        assert_eq!(calls.get(), 3);
        assert_eq!(ui.layout_producer_stats().plans_skipped, 1);
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
fn content_tasks_allow_auto_grid_and_dynamic_descendants_without_observing_sizes() {
    let mut ui = LogicalUi::new_layout(
        LayoutStyle::grid(GridStyle::default().columns(vec![GridTrack::auto().into()])),
        0,
        BackendCapabilities::default(),
    )
    .unwrap();
    let owner = ui.root_scope();
    let host = ui.root_render();
    let log = RefCell::new(vec![]);
    let (commit, _) = ui
        .edit_local_frame(
            constraints(),
            [180.0; 2],
            |_| Ok(()),
            |(), edit| {
                let mut plan = LayoutBuildPlan::new();
                let log = &log;
                plan.add(
                    LayoutBuildTaskId(90),
                    host,
                    LayoutBuildKind::Content,
                    &[],
                    move |input, edit, queue| {
                        assert_eq!(input, LayoutBuildInput::Content);
                        log.borrow_mut().push(90);
                        let child = edit.append_layout(owner, host, LayoutStyle::column(), 0)?;
                        queue.add(
                            LayoutBuildTaskId(1),
                            child,
                            LayoutBuildKind::Content,
                            &[],
                            move |input, edit, _| {
                                assert_eq!(input, LayoutBuildInput::Content);
                                log.borrow_mut().push(1);
                                edit.append_layout(owner, child, fixed(37.0, 19.0), 0xff0000ff)?;
                                Ok(())
                            },
                        )
                    },
                )?;
                edit.build_layout_plan(plan)
            },
        )
        .unwrap();
    assert_eq!(*log.borrow(), [90, 1]);
    assert_eq!(commit.result.value.tasks_executed, 2);
    assert_eq!(commit.result.value.peak_dependency_edges, 1);
    assert_eq!(ui.layout_result(host).unwrap().size, [37.0, 19.0]);
}

#[test]
fn content_title_producer_precedes_remaining_space_body_without_inline_proof() {
    let mut ui = LogicalUi::new_layout(
        LayoutStyle::column().size(LayoutDimension::Percent(1.0), LayoutDimension::Percent(1.0)),
        0,
        BackendCapabilities::default(),
    )
    .unwrap();
    let owner = ui.root_scope();
    let title = ui
        .append_render_layout(
            owner,
            ui.root_render(),
            LayoutStyle::column().flex_shrink(0.0),
            0,
        )
        .unwrap();
    let body = ui
        .append_render_layout(
            owner,
            ui.root_render(),
            LayoutStyle::column()
                .flex_basis(LayoutDimension::Px(0.0))
                .flex_grow(1.0)
                .min_size(LayoutDimension::Px(0.0), LayoutDimension::Px(0.0)),
            0,
        )
        .unwrap();
    let height = ui.state(owner, 0, || 20u32).unwrap();
    let log = Rc::new(RefCell::new(vec![]));
    let body_log = log.clone();
    let body_producer = LayoutProducer::new(move |input, edit| {
        body_log.borrow_mut().push(1);
        let LayoutBuildInput::Full(input) = input else {
            panic!()
        };
        assert_eq!(
            input.known_size.height,
            Some(90.0 - *edit.read(height)? as f32)
        );
        Ok(())
    });
    let title_log = log.clone();
    let title_producer = LayoutProducer::new(move |input, edit| {
        assert_eq!(input, LayoutBuildInput::Content);
        title_log.borrow_mut().push(90);
        let value = *edit.read(height)?;
        edit.properties().measure(
            title,
            Some(MeasureSnapshot::Fixed(CustomMeasureSnapshot {
                width: 20.0,
                height: value as f32,
                baseline: Some(5.0),
            })),
        );
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
                body,
                LayoutBuildKind::Allocation,
                &[],
                body_producer,
            )?;
            edit.layout_producer(
                owner,
                1,
                title,
                LayoutBuildKind::Content,
                &[],
                title_producer,
            )?;
            Ok(())
        },
    )
    .unwrap();
    assert_eq!(*log.borrow(), [90, 1]);
    log.borrow_mut().clear();
    ui.transaction(|tx| tx.set(height, 35)).unwrap();
    assert_eq!(*log.borrow(), [90, 1]);
    assert_eq!(ui.layout_result(body).unwrap().size, [120.0, 55.0]);
}

#[test]
fn content_producer_failure_rolls_back_sources_scopes_effects_and_frame() {
    let mut ui = LogicalUi::new_layout(root_style(), 0, BackendCapabilities::default()).unwrap();
    let owner = ui.root_scope();
    let host = ui.root_render();
    let fail = ui.state(owner, 0, || false).unwrap();
    let runs = Rc::new(Cell::new(0));
    let effect_runs = runs.clone();
    let producer = LayoutProducer::new(move |input, edit| {
        assert_eq!(input, LayoutBuildInput::Content);
        let failing = *edit.read(fail)?;
        let branch = edit.reconcile_keys(owner, &[u64::from(failing)])?[0];
        if edit.renders(branch)?.is_empty() {
            edit.append_layout(branch, host, fixed(25.0, 10.0), 0xff0000ff)?;
            let effect_runs = effect_runs.clone();
            edit.effect(branch, 0, &[], move |_| {
                effect_runs.set(effect_runs.get() + 1);
                None
            })?;
        }
        if failing {
            return Err(Error::InvalidStyle);
        }
        Ok(())
    });
    ui.edit_local_frame(
        constraints(),
        [180.0; 2],
        |_| Ok(()),
        |(), edit| {
            edit.layout_producer(owner, 0, host, LayoutBuildKind::Content, &[], producer)?;
            Ok(())
        },
    )
    .unwrap();
    let scopes = ui.children(owner).unwrap().to_vec();
    let before = ui.published_local_frame().unwrap().clone();
    let stats = ui.stats();
    let producer_stats = ui.layout_producer_stats();
    assert_eq!(runs.get(), 1);
    assert!(matches!(
        ui.transaction(|tx| tx.set(fail, true)),
        Err(Error::InvalidStyle)
    ));
    assert!(!*ui.read(fail).unwrap());
    assert_eq!(ui.children(owner).unwrap(), scopes);
    assert_eq!(ui.stats(), stats);
    assert_eq!(ui.layout_producer_stats(), producer_stats);
    assert_eq!(runs.get(), 1, "failed mount effects do not execute");
    assert!(before.same(ui.published_local_frame().unwrap()));
    assert_eq!(ui.reactive().effect_count(), 1);
}

#[test]
fn content_task_cycles_and_swallowed_callback_failure_keep_old_state() {
    for cycle in [false, true] {
        let mut ui =
            LogicalUi::new_layout(root_style(), 0, BackendCapabilities::default()).unwrap();
        let owner = ui.root_scope();
        let root = ui.root_render();
        let child = ui
            .append_render_layout(owner, root, LayoutStyle::column(), 0)
            .unwrap();
        let value = ui.state(owner, 0, || 0u32).unwrap();
        let before = ui.frame_in_viewport(constraints(), [180.0; 2]).unwrap();
        let calls = Cell::new(0);
        let result = ui.edit_frame(
            constraints(),
            [180.0; 2],
            |tx| {
                tx.set(value, 1)?;
                Ok(())
            },
            |(), edit| {
                let mut plan = LayoutBuildPlan::new();
                plan.add(
                    LayoutBuildTaskId(90),
                    root,
                    LayoutBuildKind::Content,
                    if cycle { &[LayoutBuildTaskId(1)] } else { &[] },
                    |_, edit, _| {
                        calls.set(calls.get() + 1);
                        edit.append_layout(owner, child, fixed(20.0, 10.0), 0xff0000ff)?;
                        Err(Error::InvalidStyle)
                    },
                )?;
                if cycle {
                    plan.add(
                        LayoutBuildTaskId(1),
                        child,
                        LayoutBuildKind::Content,
                        &[],
                        |_, _, _| panic!("cycle callback"),
                    )?;
                    edit.build_layout_plan(plan)?;
                } else {
                    assert!(matches!(
                        edit.build_layout_plan(plan),
                        Err(Error::InvalidStyle)
                    ));
                }
                Ok(())
            },
        );
        assert!(matches!(
            result,
            Err(Error::CyclicLayoutBuildPlan | Error::InvalidLayoutBuildPlan)
        ));
        assert_eq!(calls.get(), usize::from(!cycle));
        assert_eq!(*ui.read(value).unwrap(), 0);
        assert_eq!(
            pixels(before),
            pixels(ui.frame_in_viewport(constraints(), [180.0; 2]).unwrap())
        );
    }
}

#[test]
fn content_producers_retire_dynamic_children_and_recreate_their_state_and_effects() {
    let mut ui = LogicalUi::new_layout(root_style(), 0, BackendCapabilities::default()).unwrap();
    let owner = ui.root_scope();
    let host = ui.root_render();
    let shown = ui.state(owner, 0, || true).unwrap();
    let value = ui.state(owner, 1, || 20u32).unwrap();
    let child_calls = Rc::new(Cell::new(0));
    let mounted = Rc::new(Cell::new(0));
    let cleaned = Rc::new(Cell::new(0));
    let child_count = child_calls.clone();
    let mount_count = mounted.clone();
    let cleanup_count = cleaned.clone();
    let parent = LayoutProducer::new(move |input, edit| {
        assert_eq!(input, LayoutBuildInput::Content);
        let scopes = edit.reconcile_keys(owner, if *edit.read(shown)? { &[1] } else { &[] })?;
        if let Some(&scope) = scopes.first() {
            let renders = edit.renders(scope)?;
            if renders.is_empty() {
                let inner = edit.append_layout(scope, host, LayoutStyle::column(), 0)?;
                let calls = child_count.clone();
                let mounts = mount_count.clone();
                let cleanups = cleanup_count.clone();
                let child = LayoutProducer::new(move |input, edit| {
                    assert_eq!(input, LayoutBuildInput::Content);
                    calls.set(calls.get() + 1);
                    let local = edit.state(scope, 0, || 10u32)?;
                    let width = *edit.read(value)? as f32;
                    let height = *edit.read(local)? as f32;
                    let mounts = mounts.clone();
                    let cleanups = cleanups.clone();
                    edit.effect(scope, 0, &[], move |_| {
                        mounts.set(mounts.get() + 1);
                        let cleanups = cleanups.clone();
                        Some(Box::new(move || cleanups.set(cleanups.get() + 1)))
                    })?;
                    let renders = edit.renders(scope)?;
                    if let Some(&leaf) = renders.get(1) {
                        edit.properties().layout_style(leaf, fixed(width, height));
                    } else {
                        edit.append_layout(scope, inner, fixed(width, height), 0xff0000ff)?;
                    }
                    Ok(())
                });
                edit.layout_producer(scope, 0, inner, LayoutBuildKind::Content, &[], child)?;
            }
        }
        Ok(())
    });
    ui.edit_local_frame(
        constraints(),
        [180.0; 2],
        |_| Ok(()),
        |(), edit| {
            edit.layout_producer(owner, 0, host, LayoutBuildKind::Content, &[], parent)?;
            Ok(())
        },
    )
    .unwrap();
    let old_scope = ui.children(owner).unwrap()[0];
    let old_local = ui.state(old_scope, 0, || 999u32).unwrap();
    let old_frame = ui.published_local_frame().unwrap().clone();
    assert_eq!((child_calls.get(), mounted.get(), cleaned.get()), (1, 1, 0));
    ui.transaction(|tx| tx.set(value, 30)).unwrap();
    assert_eq!((child_calls.get(), mounted.get(), cleaned.get()), (2, 1, 0));
    assert_eq!(ui.layout_result(host).unwrap().size, [30.0, 10.0]);
    ui.transaction(|tx| tx.set(shown, false)).unwrap();
    assert_eq!((child_calls.get(), mounted.get(), cleaned.get()), (2, 1, 1));
    assert!(ui.read(old_local).is_err());
    assert!(ui.children(owner).unwrap().is_empty());
    assert_eq!(ui.layout_producers.len(), 1);
    ui.transaction(|tx| tx.set(value, 40)).unwrap();
    assert_eq!(ui.layout_producer_stats().plans_skipped, 1);
    ui.transaction(|tx| tx.set(shown, true)).unwrap();
    let new_scope = ui.children(owner).unwrap()[0];
    assert_ne!(new_scope, old_scope);
    let new_local = ui.state(new_scope, 0, || 999u32).unwrap();
    assert_eq!(*ui.read(new_local).unwrap(), 10);
    assert_eq!((child_calls.get(), mounted.get(), cleaned.get()), (3, 2, 1));
    assert_eq!(ui.layout_result(host).unwrap().size, [40.0, 10.0]);
    let mut full = UiTree::new_layout(root_style(), 0).unwrap();
    full.set_frame_build_policy(FrameBuildPolicy::Full);
    let inner = full
        .append_layout(full.root(), LayoutStyle::column(), 0)
        .unwrap();
    full.append_layout(inner, fixed(20.0, 10.0), 0xff0000ff)
        .unwrap();
    assert_eq!(
        pixels(old_frame.resolve()),
        pixels(full.frame_in_viewport(constraints(), [180.0; 2]).unwrap())
    );
}

#[test]
fn content_producer_conditional_reads_and_final_values_share_normal_invalidation() {
    let mut ui = LogicalUi::new_layout(root_style(), 0, BackendCapabilities::default()).unwrap();
    let owner = ui.root_scope();
    let host = ui.root_render();
    let selected = ui.state(owner, 0, || false).unwrap();
    let a = ui.state(owner, 1, || 20u32).unwrap();
    let b = ui.state(owner, 2, || 30u32).unwrap();
    let calls = Rc::new(Cell::new(0));
    let count = calls.clone();
    ui.edit_local_frame(
        constraints(),
        [180.0; 2],
        |_| Ok(()),
        |(), edit| {
            edit.layout_producer(
                owner,
                0,
                host,
                LayoutBuildKind::Content,
                &[],
                LayoutProducer::new(move |input, edit| {
                    assert_eq!(input, LayoutBuildInput::Content);
                    count.set(count.get() + 1);
                    let width = *edit.read(if *edit.read(selected)? { b } else { a })? as f32;
                    let branch = edit.reconcile_keys(owner, &[0])?[0];
                    if let Some(&node) = edit.renders(branch)?.first() {
                        edit.properties().layout_style(node, fixed(width, 10.0));
                    } else {
                        edit.append_layout(branch, host, fixed(width, 10.0), 0xff0000ff)?;
                    }
                    Ok(())
                }),
            )?;
            Ok(())
        },
    )
    .unwrap();
    ui.transaction(|tx| tx.set(b, 35)).unwrap();
    assert_eq!(calls.get(), 1);
    ui.transaction(|tx| {
        tx.set(a, 25)?;
        tx.set(a, 20)
    })
    .unwrap();
    assert_eq!(calls.get(), 1);
    ui.transaction(|tx| {
        tx.set(selected, true)?;
        tx.set(b, 40)
    })
    .unwrap();
    assert_eq!(calls.get(), 2);
    assert_eq!(ui.layout_result(host).unwrap().size, [40.0, 10.0]);
    ui.transaction(|tx| tx.set(a, 50)).unwrap();
    assert_eq!(
        calls.get(),
        2,
        "old conditional source left the reverse index"
    );
    assert_eq!(ui.layout_producer_stats().plans_skipped, 1);
}

#[test]
fn content_producer_resize_reenters_explicit_nested_allocation_builder() {
    let mut ui = LogicalUi::new_layout(
        LayoutStyle::column().size(LayoutDimension::Percent(1.0), LayoutDimension::Percent(1.0)),
        0,
        BackendCapabilities::default(),
    )
    .unwrap();
    let owner = ui.root_scope();
    let host = ui.root_render();
    let calls = Rc::new(Cell::new(0));
    let count = calls.clone();
    let producer = LayoutProducer::new(move |input, edit| {
        assert_eq!(input, LayoutBuildInput::Content);
        count.set(count.get() + 1);
        edit.allocation_builder(host, |input, edit| {
            // Root incoming available space is distinct from known size.
            let width = input.constraints.width.resolve_definite()?;
            let branch = edit.reconcile_keys(owner, &[u64::from(width >= 140.0)])?[0];
            if edit.renders(branch)?.is_empty() {
                edit.append_layout(branch, host, fixed(width * 0.5, 10.0), 0xff0000ff)?;
            }
            Ok(())
        })
    });
    ui.edit_local_frame(
        LayoutConstraints::viewport(120.0, 90.0),
        [180.0; 2],
        |_| Ok(()),
        |(), edit| {
            edit.layout_producer(owner, 0, host, LayoutBuildKind::Content, &[], producer)?;
            Ok(())
        },
    )
    .unwrap();
    let before = ui.children(owner).unwrap()[0];
    ui.local_frame_in_viewport(LayoutConstraints::viewport(160.0, 90.0), [180.0; 2])
        .unwrap();
    assert_eq!(calls.get(), 2);
    let after = ui.children(owner).unwrap()[0];
    assert_ne!(before, after);
    assert_eq!(
        ui.layout_result(ui.renders(after).unwrap()[0])
            .unwrap()
            .size,
        [80.0, 10.0]
    );
    ui.local_frame_in_viewport(LayoutConstraints::viewport(160.0, 90.0), [180.0; 2])
        .unwrap();
    assert_eq!(calls.get(), 2);
}
