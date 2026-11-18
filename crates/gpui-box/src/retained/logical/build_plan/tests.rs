use super::*;
use std::{cell::RefCell, rc::Rc};
fn constraints(width: f32) -> LayoutConstraints {
    LayoutConstraints::new(
        LayoutAvailableSpace::Definite(width),
        LayoutAvailableSpace::Unbounded,
    )
}
fn root_style() -> LayoutStyle {
    LayoutStyle::column().size(LayoutDimension::Percent(1.0), LayoutDimension::Auto)
}
fn padded(value: f32) -> LayoutStyle {
    LayoutStyle::column().padding(LayoutEdges::all(LayoutLength::Px(value)))
}
fn fixed(width: f32, height: f32) -> LayoutStyle {
    LayoutStyle::row()
        .size(LayoutDimension::Px(width), LayoutDimension::Px(height))
        .flex_shrink(0.0)
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
fn reference(width: f32) -> FramePacket {
    let mut tree = UiTree::new_layout(root_style(), 0).unwrap();
    tree.set_frame_build_policy(FrameBuildPolicy::Full);
    let outer = tree
        .append_layout(tree.root(), padded(5.0), 0x0000ffff)
        .unwrap();
    let inner = tree.append_layout(outer, padded(3.0), 0x00ff00ff).unwrap();
    tree.append_layout(
        inner,
        fixed(40.0, if width < 100.0 { 15.0 } else { 45.0 }),
        0xff0000ff,
    )
    .unwrap();
    tree.append_layout(tree.root(), fixed(20.0, 10.0), 0xffffffff)
        .unwrap();
    tree.frame_in_viewport(constraints(width), [180.0; 2])
        .unwrap()
}
fn inline(input: LayoutBuildInput) -> InlineConstraintBuilderInput {
    let LayoutBuildInput::Inline(input) = input else {
        panic!("inline task input")
    };
    input
}

#[test]
fn reversed_nested_registration_runs_ancestors_once_and_matches_full_flex() {
    run_nested(false);
}
#[cfg(feature = "retained-wgpu")]
#[test]
#[ignore = "requires WGPU adapter; run explicitly"]
fn hardware_scheduled_nested_builders_match_ordinary_full_pixels() {
    run_nested(true);
}
fn run_nested(hardware: bool) {
    #[cfg(feature = "retained-wgpu")]
    let mut gpu = hardware.then(|| wgpu::WgpuFrameRenderer::new(2).unwrap());
    #[cfg(feature = "retained-wgpu")]
    if let Some(gpu) = &gpu {
        eprintln!("layout build plan adapter: {:?}", gpu.adapter_info());
    }
    #[cfg(not(feature = "retained-wgpu"))]
    assert!(!hardware);
    let mut ui = LogicalUi::new_layout(root_style(), 0, BackendCapabilities::default()).unwrap();
    let root = ui.root_render();
    let scope = ui.root_scope();
    let outer = ui
        .append_render_layout(scope, root, padded(5.0), 0x0000ffff)
        .unwrap();
    let inner = ui
        .append_render_layout(scope, outer, padded(3.0), 0x00ff00ff)
        .unwrap();
    let footer = ui
        .append_render_layout(scope, root, fixed(20.0, 10.0), 0xffffffff)
        .unwrap();
    let log = Rc::new(RefCell::new(vec![]));
    let mut old = None;
    let mut leaf = None;
    #[cfg(feature = "retained-wgpu")]
    let mut gpu_submissions = 0;
    for width in [80.0, 140.0, 140.0, 80.0] {
        log.borrow_mut().clear();
        let trace = log.clone();
        let (commit, frame) = ui
            .edit_local_frame(
                constraints(width),
                [180.0; 2],
                |_| Ok(()),
                |(), edit| {
                    let mut plan = LayoutBuildPlan::new();
                    let child_log = trace.clone();
                    // Numeric task ordering is deliberately opposite render ancestry.
                    plan.add(
                        LayoutBuildTaskId(1),
                        inner,
                        LayoutBuildKind::InlineAllocation,
                        &[],
                        move |input, edit, _| {
                            child_log.borrow_mut().push(1);
                            assert_eq!(inline(input).known_width, Some(width - 10.0));
                            let branch =
                                edit.reconcile_keys(scope, &[u64::from(width >= 100.0)])?[0];
                            if edit.renders(branch)?.is_empty() {
                                edit.append_layout(
                                    branch,
                                    inner,
                                    fixed(40.0, if width < 100.0 { 15.0 } else { 45.0 }),
                                    0xff0000ff,
                                )?;
                            }
                            Ok(())
                        },
                    )?;
                    plan.add(
                        LayoutBuildTaskId(90),
                        outer,
                        LayoutBuildKind::InlineAllocation,
                        &[],
                        move |input, edit, _| {
                            trace.borrow_mut().push(90);
                            assert_eq!(inline(input).known_width, Some(width));
                            edit.properties().layout_style(inner, padded(3.0));
                            Ok(())
                        },
                    )?;
                    edit.build_layout_plan(plan)
                },
            )
            .unwrap();
        assert_eq!(&*log.borrow(), &[90, 1]);
        assert_eq!(commit.result.value.tasks_executed, 2);
        assert_eq!(commit.result.value.planning_previews, 3);
        assert_eq!(commit.result.value.peak_dependency_edges, 1);
        assert_eq!(pixels(frame.resolve()), pixels(reference(width)));
        let height = if width < 100.0 { 15.0 } else { 45.0 };
        assert_eq!(
            ui.layout_result(footer).unwrap().location,
            [0.0, height + 16.0]
        );
        let live = ui.children(scope).unwrap()[0];
        let current = ui.renders(live).unwrap()[0];
        if width == 140.0 && leaf.is_some() {
            assert_eq!(leaf, Some(current));
        }
        leaf = if width == 140.0 { Some(current) } else { None };
        #[cfg(feature = "retained-wgpu")]
        if let Some(gpu) = &mut gpu {
            gpu_submissions +=
                usize::from(gpu.submit_local(&frame, [180, 180], 1.0).unwrap().is_some());
            assert_eq!(
                gpu.image().unwrap().as_raw(),
                gpu.reference_image(&reference(width), [180, 180], 1.0)
                    .unwrap()
                    .as_raw()
            );
        }
        old.get_or_insert(frame);
    }
    let old = old.unwrap();
    let expected = pixels(old.resolve());
    drop(ui);
    assert_eq!(pixels(old.resolve()), expected);
    #[cfg(feature = "retained-wgpu")]
    if let Some(gpu) = &mut gpu {
        // Final inline input observation dirties layout and refreshes the
        // local snapshot even at equal width. Current GPU same-frame reuse
        // requires the exact snapshot; the planner does not remove this cost.
        assert_eq!(gpu_submissions, 4);
        gpu.finish().unwrap();
    }
}

#[test]
fn dynamic_child_tasks_borrow_transaction_data_and_keep_keyed_state() {
    let mut ui = LogicalUi::new_layout(root_style(), 0, BackendCapabilities::default()).unwrap();
    let root = ui.root_render();
    let scope = ui.root_scope();
    let outer = ui
        .append_render_layout(scope, root, padded(5.0), 0)
        .unwrap();
    let words = String::from("borrowed once");
    let calls = RefCell::new(vec![]);
    let (commit, _) = ui
        .edit_frame(
            constraints(140.0),
            [180.0; 2],
            |_| Ok(()),
            |(), edit| {
                let mut plan = LayoutBuildPlan::new();
                let words = &words;
                let calls = &calls;
                plan.add(
                    LayoutBuildTaskId(10),
                    outer,
                    LayoutBuildKind::InlineAllocation,
                    &[],
                    move |input, edit, plan| {
                        calls.borrow_mut().push(words.len());
                        assert_eq!(inline(input).known_width, Some(140.0));
                        let branch = edit.reconcile_keys(scope, &[1])?[0];
                        let inner = edit.append_layout(branch, outer, padded(3.0), 0)?;
                        plan.add(
                            LayoutBuildTaskId(11),
                            inner,
                            LayoutBuildKind::InlineAllocation,
                            &[],
                            move |input, edit, _| {
                                calls.borrow_mut().push(20);
                                assert_eq!(inline(input).known_width, Some(130.0));
                                let value = edit.state(branch, 0, || 7u32)?;
                                assert_eq!(*edit.read(value)?, 7);
                                edit.append_layout(branch, inner, fixed(40.0, 15.0), 0xff0000ff)?;
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
    assert_eq!(commit.result.value.tasks_executed, 2);
    assert_eq!(&*calls.borrow(), &[words.len(), 20]);
    assert_eq!(ui.children(scope).unwrap().len(), 1);
}

#[test]
fn explicit_sibling_dependencies_settle_before_downstream_observation() {
    let mut ui = LogicalUi::new_layout(root_style(), 0, BackendCapabilities::default()).unwrap();
    let root = ui.root_render();
    let scope = ui.root_scope();
    let a = ui
        .append_render_layout(scope, root, padded(0.0), 0)
        .unwrap();
    let b = ui
        .append_render_layout(scope, root, padded(0.0), 0)
        .unwrap();
    let widths = RefCell::new(vec![]);
    let (commit, _) = ui
        .edit_frame(
            constraints(140.0),
            [180.0; 2],
            |_| Ok(()),
            |(), edit| {
                let mut plan = LayoutBuildPlan::new();
                plan.add(
                    LayoutBuildTaskId(1),
                    b,
                    LayoutBuildKind::InlineAllocation,
                    &[LayoutBuildTaskId(2)],
                    |input, edit, _| {
                        widths.borrow_mut().push(inline(input).known_width.unwrap());
                        edit.append_layout(scope, b, fixed(20.0, 15.0), 0xff0000ff)?;
                        Ok(())
                    },
                )?;
                plan.add(
                    LayoutBuildTaskId(2),
                    a,
                    LayoutBuildKind::InlineAllocation,
                    &[],
                    |_, edit, _| {
                        edit.properties().layout_style(
                            b,
                            LayoutStyle::column()
                                .size(LayoutDimension::Px(70.0), LayoutDimension::Auto),
                        );
                        Ok(())
                    },
                )?;
                edit.build_layout_plan(plan)
            },
        )
        .unwrap();
    assert_eq!(commit.result.value.tasks_executed, 2);
    assert_eq!(&*widths.borrow(), &[70.0]);
    assert_eq!(ui.layout_result(b).unwrap().size[0], 70.0);
}

#[test]
fn cycles_missing_duplicate_limits_and_swallowed_failures_do_not_publish() {
    let mut ui = LogicalUi::new_layout(root_style(), 0, BackendCapabilities::default()).unwrap();
    let root = ui.root_render();
    let scope = ui.root_scope();
    let a = ui
        .append_render_layout(scope, root, padded(0.0), 0)
        .unwrap();
    let b = ui.append_render_layout(scope, a, padded(0.0), 0).unwrap();
    let before = ui
        .frame_in_viewport(constraints(140.0), [180.0; 2])
        .unwrap();
    let stats = ui.stats();
    let calls = RefCell::new(0);
    for mode in 0..5 {
        let result = ui.edit_frame(
            constraints(140.0),
            [180.0; 2],
            |_| Ok(()),
            |(), edit| {
                let limits = if mode == 3 {
                    LayoutBuildLimits {
                        max_dependency_edges: 0,
                        ..Default::default()
                    }
                } else {
                    LayoutBuildLimits::default()
                };
                let mut plan = LayoutBuildPlan::with_limits(limits);
                plan.add(
                    LayoutBuildTaskId(1),
                    a,
                    LayoutBuildKind::InlineAllocation,
                    if mode == 0 {
                        &[LayoutBuildTaskId(2)]
                    } else if mode == 1 {
                        &[LayoutBuildTaskId(99)]
                    } else {
                        &[]
                    },
                    |_, _, _| {
                        *calls.borrow_mut() += 1;
                        Ok(())
                    },
                )?;
                if mode != 1 {
                    let add = plan.add(
                        LayoutBuildTaskId(2),
                        if mode == 2 { a } else { b },
                        LayoutBuildKind::InlineAllocation,
                        &[],
                        |_, _, _| {
                            *calls.borrow_mut() += 1;
                            Ok(())
                        },
                    );
                    if mode != 2 {
                        add?;
                    }
                }
                if mode == 4 {
                    plan.add(
                        LayoutBuildTaskId(3),
                        root,
                        LayoutBuildKind::InlineAllocation,
                        &[],
                        |_, edit, _| {
                            edit.append_layout(scope, root, fixed(1.0, 1.0), 1)?;
                            Err(Error::InvalidStyle)
                        },
                    )?;
                }
                let error = edit.build_layout_plan(plan);
                assert!(error.is_err());
                // A caller cannot catch an error and publish earlier callbacks.
                Ok(())
            },
        );
        assert!(matches!(result, Err(Error::InvalidLayoutBuildPlan)));
        assert_eq!(ui.stats(), stats);
        assert_eq!(
            ui.frame_in_viewport(constraints(140.0), [180.0; 2])
                .unwrap()
                .revision,
            before.revision
        );
    }
    assert_eq!(*calls.borrow(), 0);
}

#[test]
fn dynamic_retroactive_dependencies_and_late_layout_failure_roll_back_sources_and_output() {
    let mut ui = LogicalUi::new_layout(root_style(), 0, BackendCapabilities::default()).unwrap();
    let root = ui.root_render();
    let scope = ui.root_scope();
    let host = ui
        .append_render_layout(scope, root, padded(0.0), 0)
        .unwrap();
    let source = ui.state(scope, 0, || 1u32).unwrap();
    let before = ui
        .frame_in_viewport(constraints(140.0), [180.0; 2])
        .unwrap();
    let result = ui.edit_frame(
        constraints(140.0),
        [180.0; 2],
        |tx| {
            tx.set(source, 2)?;
            Ok(())
        },
        |(), edit| {
            let mut plan = LayoutBuildPlan::new();
            plan.add(
                LayoutBuildTaskId(1),
                host,
                LayoutBuildKind::InlineAllocation,
                &[],
                |_, _, plan| {
                    // New ancestor would need to precede an already consumed task.
                    plan.add(
                        LayoutBuildTaskId(2),
                        root,
                        LayoutBuildKind::InlineAllocation,
                        &[],
                        |_, _, _| Ok(()),
                    )?;
                    Ok(())
                },
            )?;
            edit.build_layout_plan(plan)
        },
    );
    assert!(matches!(result, Err(Error::InvalidLayoutBuildPlan)));
    assert_eq!(*ui.read(source).unwrap(), 1);
    let result = ui.edit_frame(
        constraints(140.0),
        [180.0; 2],
        |tx| {
            tx.set(source, 3)?;
            Ok(())
        },
        |(), edit| {
            let mut plan = LayoutBuildPlan::new();
            plan.add(
                LayoutBuildTaskId(1),
                host,
                LayoutBuildKind::InlineAllocation,
                &[],
                |_, edit, _| {
                    edit.append_layout(scope, host, fixed(10.0, 10.0), 1)?;
                    Ok(())
                },
            )?;
            edit.build_layout_plan(plan)?;
            // A later write changes an observed incoming input. Final ordinary
            // observation validation, not the scheduler's graph, rejects it.
            edit.properties().layout_style(
                root,
                root_style().size(LayoutDimension::Px(80.0), LayoutDimension::Auto),
            );
            Ok(())
        },
    );
    assert!(matches!(result, Err(Error::UnsafeConstraintBuilder)));
    assert_eq!(*ui.read(source).unwrap(), 1);
    assert_eq!(
        ui.frame_in_viewport(constraints(140.0), [180.0; 2])
            .unwrap()
            .revision,
        before.revision
    );
}

#[test]
fn all_builder_input_kinds_share_one_final_layout_publication() {
    let mut ui = LogicalUi::new_layout(
        root_style().size(LayoutDimension::Percent(1.0), LayoutDimension::Px(160.0)),
        0,
        BackendCapabilities::default(),
    )
    .unwrap();
    let root = ui.root_render();
    let scope = ui.root_scope();
    let full = LayoutStyle::constraint_box(ConstraintFit::Tight, ConstraintFit::Loose)
        .container_type(LayoutContainerType::Size)
        .size(LayoutDimension::Px(60.0), LayoutDimension::Px(40.0));
    let inline_box = LayoutStyle::constraint_box(ConstraintFit::Tight, ConstraintFit::Loose)
        .container_type(LayoutContainerType::InlineSize)
        .size(LayoutDimension::Px(80.0), LayoutDimension::Auto);
    let allocation = LayoutStyle::column()
        .container_type(LayoutContainerType::Size)
        .size(LayoutDimension::Px(70.0), LayoutDimension::Px(35.0));
    let mut hosts = vec![];
    for style in [full, inline_box, allocation, LayoutStyle::column()] {
        hosts.push(ui.append_render_layout(scope, root, style, 0).unwrap());
    }
    let seen = RefCell::new(vec![]);
    let (commit, _) = ui
        .edit_local_frame(
            constraints(140.0),
            [180.0; 2],
            |_| Ok(()),
            |(), edit| {
                let mut plan = LayoutBuildPlan::new();
                for (index, kind) in [
                    LayoutBuildKind::Constraint,
                    LayoutBuildKind::InlineConstraint,
                    LayoutBuildKind::Allocation,
                    LayoutBuildKind::InlineAllocation,
                ]
                .into_iter()
                .enumerate()
                {
                    let host = hosts[index];
                    let seen = &seen;
                    plan.add(
                        LayoutBuildTaskId(index as u64),
                        host,
                        kind,
                        &[],
                        move |input, edit, _| {
                            seen.borrow_mut()
                                .push(matches!(input, LayoutBuildInput::Full(_)));
                            edit.append_layout(scope, host, fixed(20.0, 15.0), 0xff0000ff)?;
                            Ok(())
                        },
                    )?;
                }
                edit.build_layout_plan(plan)
            },
        )
        .unwrap();
    assert_eq!(commit.result.value.tasks_executed, 4);
    assert_eq!(&*seen.borrow(), &[true, false, true, false]);
    assert_eq!(ui.layout_result(hosts[1]).unwrap().size, [80.0, 15.0]);
    assert_eq!(ui.layout_result(hosts[3]).unwrap().size, [140.0, 15.0]);
}

#[test]
fn scoped_responsive_rules_settle_before_scheduled_input_and_keep_effect_lifecycle() {
    use crate::retained::responsive::*;
    let mut ui = LogicalUi::new_layout(root_style(), 0, BackendCapabilities::default()).unwrap();
    let root = ui.root_render();
    let scope = ui.root_scope();
    let host = ui
        .append_render_layout(
            scope,
            root,
            LayoutStyle::column().fixed_inline_size(70.0),
            0,
        )
        .unwrap();
    let source = ui
        .state(scope, 0, || LayoutStyle::column().fixed_inline_size(70.0))
        .unwrap();
    let effects = Rc::new(std::cell::Cell::new(0));
    let cleanups = Rc::new(std::cell::Cell::new(0));
    for env in [100.0, 200.0, 100.0] {
        let (commit, _) = ui
            .edit_local_frame(
                constraints(180.0),
                [180.0; 2],
                |_| Ok(()),
                |(), edit| {
                    edit.layout_environment(&LayoutEnvironment::new([env, 180.0]))?;
                    edit.responsive_rule(
                        scope,
                        0,
                        source,
                        host,
                        LayoutQuery::ViewportInline,
                        LayoutBreakpointStyles::new(
                            LayoutStyle::column().fixed_inline_size(70.0),
                            vec![(150.0, LayoutStyle::column().fixed_inline_size(130.0))],
                        )?,
                    )?;
                    let mut plan = LayoutBuildPlan::new();
                    let effects = effects.clone();
                    let cleanups = cleanups.clone();
                    plan.add(
                        LayoutBuildTaskId(1),
                        host,
                        LayoutBuildKind::InlineAllocation,
                        &[],
                        move |input, edit, _| {
                            let input = inline(input);
                            let width = if env < 150.0 { 70.0 } else { 130.0 };
                            assert_eq!(input.known_width, Some(width));
                            let branch = edit.reconcile_keys(scope, &[u64::from(env >= 150.0)])?[0];
                            edit.effect(branch, 0, &[], move |_| {
                                effects.set(effects.get() + 1);
                                let cleanups = cleanups.clone();
                                Some(Box::new(move || cleanups.set(cleanups.get() + 1)))
                            })?;
                            edit.append_layout(branch, host, fixed(20.0, 15.0), 0xff0000ff)?;
                            Ok(())
                        },
                    )?;
                    edit.build_layout_plan(plan)
                },
            )
            .unwrap();
        assert_eq!(commit.result.value.tasks_executed, 1);
    }
    assert_eq!(effects.get(), 3);
    assert_eq!(cleanups.get(), 2);
    let before = ui.published_local_frame().unwrap().clone();
    let effect_count = effects.get();
    let cleanup_count = cleanups.get();
    let result = ui.edit_local_frame(
        constraints(180.0),
        [180.0; 2],
        |_| Ok(()),
        |(), edit| {
            let mut plan = LayoutBuildPlan::new();
            let effects = effects.clone();
            plan.add(
                LayoutBuildTaskId(1),
                host,
                LayoutBuildKind::InlineAllocation,
                &[],
                move |_, edit, _| {
                    let branch = edit.reconcile_keys(scope, &[9])?[0];
                    edit.effect(branch, 0, &[], move |_| {
                        effects.set(effects.get() + 1);
                        None
                    })?;
                    Err(Error::InvalidStyle)
                },
            )?;
            edit.build_layout_plan(plan)
        },
    );
    assert!(result.is_err());
    assert_eq!(effects.get(), effect_count);
    assert_eq!(cleanups.get(), cleanup_count);
    assert_eq!(
        ui.published_local_frame().unwrap().revision(),
        before.revision()
    );
}

#[test]
fn late_equal_width_reparent_cannot_reverse_the_already_executed_plan() {
    let mut ui = LogicalUi::new_layout(root_style(), 0, BackendCapabilities::default()).unwrap();
    let root = ui.root_render();
    let scope = ui.root_scope();
    let a = ui
        .append_render_layout(scope, root, LayoutStyle::column(), 0)
        .unwrap();
    let b = ui
        .append_render_layout(scope, root, LayoutStyle::column(), 0)
        .unwrap();
    let before = ui
        .frame_in_viewport(constraints(140.0), [180.0; 2])
        .unwrap();
    let result = ui.edit_frame(
        constraints(140.0),
        [180.0; 2],
        |_| Ok(()),
        |(), edit| {
            let mut plan = LayoutBuildPlan::new();
            plan.add(
                LayoutBuildTaskId(1),
                a,
                LayoutBuildKind::InlineAllocation,
                &[],
                |_, _, _| Ok(()),
            )?;
            plan.add(
                LayoutBuildTaskId(2),
                b,
                LayoutBuildKind::InlineAllocation,
                &[],
                |_, _, _| Ok(()),
            )?;
            edit.build_layout_plan(plan)?;
            edit.reparent(a, b)?;
            Ok(())
        },
    );
    assert!(matches!(result, Err(Error::InvalidLayoutBuildPlan)));
    assert_eq!(
        ui.frame_in_viewport(constraints(140.0), [180.0; 2])
            .unwrap()
            .revision,
        before.revision
    );
}

#[test]
fn failed_dynamic_add_reentrancy_and_missing_frame_session_poison_the_outer_edit() {
    let mut ui = LogicalUi::new_layout(root_style(), 0, BackendCapabilities::default()).unwrap();
    let root = ui.root_render();
    let scope = ui.root_scope();
    let host = ui
        .append_render_layout(scope, root, LayoutStyle::column(), 0)
        .unwrap();
    let before = ui
        .frame_in_viewport(constraints(140.0), [180.0; 2])
        .unwrap();
    for reentrant in [false, true] {
        let result = ui.edit_frame(
            constraints(140.0),
            [180.0; 2],
            |_| Ok(()),
            |(), edit| {
                let mut plan = LayoutBuildPlan::with_limits(LayoutBuildLimits {
                    max_tasks: 1,
                    ..Default::default()
                });
                plan.add(
                    LayoutBuildTaskId(1),
                    host,
                    LayoutBuildKind::InlineAllocation,
                    &[],
                    move |_, edit, queue| {
                        edit.append_layout(scope, host, fixed(10.0, 10.0), 1)?;
                        if reentrant {
                            assert!(edit.build_layout_plan(LayoutBuildPlan::new()).is_err());
                        } else {
                            assert_eq!(
                                queue.add(
                                    LayoutBuildTaskId(2),
                                    root,
                                    LayoutBuildKind::InlineAllocation,
                                    &[],
                                    |_, _, _| Ok(())
                                ),
                                Err(Error::LayoutBuildPlanLimit)
                            );
                        }
                        Ok(())
                    },
                )?;
                let _ = edit.build_layout_plan(plan);
                Ok(())
            },
        );
        assert!(matches!(result, Err(Error::InvalidLayoutBuildPlan)));
        assert_eq!(
            ui.frame_in_viewport(constraints(140.0), [180.0; 2])
                .unwrap()
                .revision,
            before.revision
        );
    }
    let result = ui.edit(
        |_| Ok(()),
        |(), edit| {
            let _ = edit.build_layout_plan(LayoutBuildPlan::new());
            Ok(())
        },
    );
    assert!(matches!(result, Err(Error::InvalidLayoutBuildPlan)));
}
