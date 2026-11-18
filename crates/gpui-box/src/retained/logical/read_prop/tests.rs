use super::*;
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

#[derive(Clone, Debug, PartialEq)]
struct Row {
    key: u64,
    width: u32,
    color: u32,
}
fn fixed(width: f32, height: f32) -> LayoutStyle {
    LayoutStyle::column()
        .size(LayoutDimension::Px(width), LayoutDimension::Px(height))
        .flex_shrink(0.0)
}
fn root_style() -> LayoutStyle {
    LayoutStyle::column()
}
fn constraints() -> LayoutConstraints {
    LayoutConstraints::new(
        LayoutAvailableSpace::Unbounded,
        LayoutAvailableSpace::Unbounded,
    )
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
fn reference(rows: &[Row]) -> FramePacket {
    let mut tree = UiTree::new_layout(root_style(), 0).unwrap();
    tree.set_frame_build_policy(FrameBuildPolicy::Full);
    for row in rows {
        tree.append_layout(tree.root(), fixed(row.width as f32, 10.0), row.color)
            .unwrap();
    }
    tree.frame_in_viewport(constraints(), [180.0; 2]).unwrap()
}

#[test]
fn read_prop_keyed_payload_updates_keep_mounted_state_bindings_and_latest_event_reads() {
    run(false);
}
#[cfg(feature = "retained-wgpu")]
#[test]
#[ignore = "requires WGPU adapter; run explicitly"]
fn hardware_read_prop_keyed_payloads_match_complete_reference() {
    run(true);
}
fn run(hardware: bool) {
    #[cfg(feature = "retained-wgpu")]
    let mut gpu = hardware.then(|| wgpu::WgpuFrameRenderer::new(2).unwrap());
    #[cfg(feature = "retained-wgpu")]
    if let Some(gpu) = &gpu {
        eprintln!("ReadProp adapter: {:?}", gpu.adapter_info());
    }
    #[cfg(not(feature = "retained-wgpu"))]
    assert!(!hardware);
    for local in [false, true] {
        let first = vec![
            Row {
                key: 1,
                width: 20,
                color: 0xff0000ff,
            },
            Row {
                key: 2,
                width: 30,
                color: 0x00ff00ff,
            },
        ];
        let mut ui =
            LogicalUi::new_layout(root_style(), 0, BackendCapabilities::default()).unwrap();
        let owner = ui.root_scope();
        let host = ui.root_render();
        let model = ui.state(owner, 0, || first.clone()).unwrap();
        let mounts = Rc::new(Cell::new(0));
        let count = mounts.clone();
        let events = Rc::new(RefCell::new(Vec::<ReadProp<Row>>::new()));
        let handles = events.clone();
        let producer = LayoutProducer::new(move |input, edit| {
            assert_eq!(input, LayoutBuildInput::Content);
            let rows = edit.read(model)?.clone();
            let scopes =
                edit.reconcile_keys(owner, &rows.iter().map(|r| r.key).collect::<Vec<_>>())?;
            for (scope, row) in scopes.into_iter().zip(rows) {
                let prop = edit.prop(scope, 0, row)?;
                if edit.renders(scope)?.is_empty() {
                    count.set(count.get() + 1);
                    handles.borrow_mut().push(prop);
                    let height = edit.state(scope, 1, || 10u32)?;
                    let style = edit.computed(
                        scope,
                        2,
                        &[prop.dependency(), height.dependency()],
                        move |inputs| {
                            Ok(fixed(
                                inputs.read_prop(prop)?.width as f32,
                                *inputs.read(height)? as f32,
                            ))
                        },
                    )?;
                    let color = edit.computed(scope, 3, &[prop.dependency()], move |inputs| {
                        Ok(inputs.read_prop(prop)?.color)
                    })?;
                    let node = edit.append_layout(scope, host, LayoutStyle::column(), 0)?;
                    edit.bind(style, Property::layout_style(node))?;
                    edit.bind(color, Property::color(node))?;
                }
            }
            Ok(())
        });
        let register = |(), edit: &mut LogicalEdit<'_, '_, '_>| {
            edit.layout_producer(owner, 0, host, LayoutBuildKind::Content, &[], producer)?;
            Ok(())
        };
        if local {
            ui.edit_local_frame(constraints(), [180.0; 2], |_| Ok(()), register)
                .unwrap();
        } else {
            ui.edit_frame(constraints(), [180.0; 2], |_| Ok(()), register)
                .unwrap();
        }
        let scopes = ui.children(owner).unwrap().to_vec();
        let node1 = ui.renders(scopes[0]).unwrap()[0];
        let old_prop = events.borrow()[0];
        let old_state = ui.state(scopes[0], 1, || 999u32).unwrap();
        let mut old = None;
        for (index, rows) in [
            first,
            vec![
                Row {
                    key: 2,
                    width: 40,
                    color: 0x0000ffff,
                },
                Row {
                    key: 1,
                    width: 25,
                    color: 0xffff00ff,
                },
            ],
            vec![
                Row {
                    key: 1,
                    width: 15,
                    color: 0xff00ffff,
                },
                Row {
                    key: 3,
                    width: 35,
                    color: 0x00ffffff,
                },
            ],
        ]
        .into_iter()
        .enumerate()
        {
            if index > 0 {
                ui.transaction(|tx| tx.set(model, rows.clone())).unwrap();
            }
            assert_eq!(mounts.get(), if index < 2 { 2 } else { 3 });
            assert_eq!(
                ui.read_prop(old_prop).unwrap(),
                rows.iter().find(|r| r.key == 1).unwrap()
            );
            assert_eq!(*ui.read(old_state).unwrap(), 10);
            let scope = ui.children(owner).unwrap()[if index == 1 { 1 } else { 0 }];
            assert_eq!(scope, scopes[0]);
            assert_eq!(ui.renders(scope).unwrap(), [node1]);
            let packet = ui.frame_in_viewport(constraints(), [180.0; 2]).unwrap();
            let full = reference(&rows);
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
        assert!(ui.read_prop(events.borrow()[1]).is_err());
        drop(ui);
        let (frame, expected) = old.unwrap();
        assert_eq!(pixels(frame), expected);
    }
    #[cfg(feature = "retained-wgpu")]
    if let Some(gpu) = &mut gpu {
        gpu.finish().unwrap();
    }
}

#[test]
fn read_prop_multi_input_updates_settle_derived_values_and_publish_effects_once() {
    let mut ui = LogicalUi::new_layout(root_style(), 0, BackendCapabilities::default()).unwrap();
    let owner = ui.root_scope();
    let host = ui.root_render();
    let seen = Rc::new(RefCell::new(vec![]));
    let log = seen.clone();
    let initial = ui
        .edit_local_frame(
            constraints(),
            [180.0; 2],
            |_| Ok(()),
            |(), edit| {
                let a = edit.prop(owner, 0, 10u32)?;
                let b = edit.prop(owner, 1, 20u32)?;
                let sum = edit.computed(owner, 2, &[a.dependency(), b.dependency()], move |i| {
                    Ok(*i.read_prop(a)? + *i.read_prop(b)?)
                })?;
                let node = edit.append_layout(owner, host, fixed(20.0, 10.0), 0)?;
                edit.bind(sum, Property::color(node))?;
                edit.effect(
                    owner,
                    0,
                    &[a.dependency(), b.dependency(), sum.dependency()],
                    move |i| {
                        log.borrow_mut().push((
                            *i.read_prop(a).unwrap(),
                            *i.read_prop(b).unwrap(),
                            *i.read(sum).unwrap(),
                        ));
                        None
                    },
                )?;
                Ok((a, b, sum, node))
            },
        )
        .unwrap();
    let (a, b, sum, node) = initial.0.result.value;
    let update = ui
        .edit(
            |_| Ok(()),
            |(), edit| {
                assert_eq!(edit.prop(owner, 0, 12u32)?, a);
                assert_eq!(edit.prop(owner, 1, 25u32)?, b);
                assert_eq!(*edit.read(sum)?, 37);
                assert_eq!(edit.prop(owner, 0, 15u32)?, a);
                assert_eq!(*edit.read(sum)?, 40);
                Ok(())
            },
        )
        .unwrap();
    assert_eq!(update.result.effect_runs, 1);
    assert_eq!(*seen.borrow(), [(10, 20, 30), (15, 25, 40)]);
    assert_eq!(ui.reactive().tree().node(node).unwrap().color, 40);
    assert_eq!(*ui.read_prop(a).unwrap(), 15);
    ui.transaction(|tx| {
        assert_eq!(*tx.read_prop(b)?, 25);
        Ok(())
    })
    .unwrap();
    let before = ui.published_local_frame().unwrap().clone();
    let equal = ui
        .edit(
            |_| Ok(()),
            |(), edit| {
                edit.prop(owner, 0, 99u32)?;
                edit.prop(owner, 0, 15u32)?;
                Ok(())
            },
        )
        .unwrap();
    assert_eq!(equal.result.effect_runs, 0);
    assert!(before.same(ui.published_local_frame().unwrap()));
}

#[test]
fn read_prop_direct_binding_and_persistent_child_read_latest_parent_input() {
    let mut ui = LogicalUi::new_layout(root_style(), 0, BackendCapabilities::default()).unwrap();
    let owner = ui.root_scope();
    let root = ui.root_render();
    let model = ui.state(owner, 0, || 20u32).unwrap();
    let child_calls = Rc::new(RefCell::new(vec![]));
    let log = child_calls.clone();
    let parent = LayoutProducer::new(move |_, edit| {
        let branch = edit.reconcile_keys(owner, &[0])?[0];
        let value = *edit.read(model)?;
        let prop = edit.prop(branch, 0, value)?;
        if edit.renders(branch)?.is_empty() {
            let host = edit.append_layout(branch, root, LayoutStyle::column(), 0)?;
            let leaf = edit.append_layout(branch, host, fixed(20.0, 10.0), 0)?;
            edit.bind_prop(prop, Property::color(leaf))?;
            let log = log.clone();
            edit.layout_producer(
                branch,
                0,
                host,
                LayoutBuildKind::Content,
                &[],
                LayoutProducer::new(move |_, edit| {
                    log.borrow_mut().push(*edit.read_prop(prop)?);
                    Ok(())
                }),
            )?;
        }
        Ok(())
    });
    ui.edit_local_frame(
        constraints(),
        [180.0; 2],
        |_| Ok(()),
        |(), edit| {
            edit.layout_producer(owner, 0, root, LayoutBuildKind::Content, &[], parent)?;
            Ok(())
        },
    )
    .unwrap();
    ui.transaction(|tx| tx.set(model, 35)).unwrap();
    assert_eq!(*child_calls.borrow(), [20, 35]);
    let scope = ui.children(owner).unwrap()[0];
    let leaf = ui.renders(scope).unwrap()[1];
    assert_eq!(ui.reactive().tree().node(leaf).unwrap().color, 35);
}

#[test]
fn read_prop_type_namespace_and_generations_do_not_alias_state_or_old_instances() {
    let mut ui = LogicalUi::new_layout(root_style(), 0, BackendCapabilities::default()).unwrap();
    let owner = ui.root_scope();
    let init = ui
        .edit(
            |_| Ok(()),
            |(), edit| {
                let scope = edit.reconcile_keys(owner, &[0])?[0];
                let prop = edit.prop(scope, 0, 10u32)?;
                assert!(matches!(
                    edit.state(scope, 0, || 5u32),
                    Err(Error::InvalidSignal)
                ));
                assert!(matches!(
                    edit.prop(scope, 0, 10u64),
                    Err(Error::InvalidSignal)
                ));
                let state = edit.state(scope, 1, || 20u32)?;
                assert!(matches!(
                    edit.prop(scope, 1, 30u32),
                    Err(Error::InvalidSignal)
                ));
                Ok((scope, prop, state))
            },
        )
        .unwrap();
    let (scope, old, state) = init.result.value;
    assert!(matches!(
        ui.state(scope, 0, || 5u32),
        Err(Error::InvalidSignal)
    ));
    let foreign = LogicalUi::new_layout(root_style(), 0, BackendCapabilities::default()).unwrap();
    assert!(foreign.read_prop(old).is_err());
    ui.edit(
        |_| Ok(()),
        |(), edit| {
            edit.reconcile_keys(owner, &[])?;
            Ok(())
        },
    )
    .unwrap();
    assert!(ui.read_prop(old).is_err() && ui.read(state).is_err());
    let next = ui
        .edit(
            |_| Ok(()),
            |(), edit| {
                let scope = edit.reconcile_keys(owner, &[0])?[0];
                edit.prop(scope, 0, 40u32)
            },
        )
        .unwrap()
        .result
        .value;
    assert_ne!(old, next);
    assert!(ui.read_prop(old).is_err());
    assert_eq!(*ui.read_prop(next).unwrap(), 40);
    assert!(matches!(
        ui.edit(|_| Ok(()), |(), edit| { edit.prop(scope, 0, 20u32) }),
        Err(Error::InvalidScope)
    ));
    let foreign_root = foreign.root_scope();
    assert!(
        ui.edit(|_| Ok(()), |(), edit| edit.prop(foreign_root, 0, 5u32))
            .is_err()
    );
}

#[test]
fn read_prop_failed_derivation_or_frame_validation_preserves_payload_graph_and_effects() {
    let mut ui = LogicalUi::new_layout(root_style(), 0, BackendCapabilities::default()).unwrap();
    let owner = ui.root_scope();
    let host = ui.root_render();
    let seen = Rc::new(Cell::new(0));
    let calls = seen.clone();
    let init = ui
        .edit_local_frame(
            constraints(),
            [180.0; 2],
            |_| Ok(()),
            |(), edit| {
                let prop = edit.prop(owner, 0, 10u32)?;
                let derived = edit.computed(owner, 1, &[prop.dependency()], move |i| {
                    let value = *i.read_prop(prop)?;
                    if value == 99 {
                        return Err(Error::InvalidStyle);
                    };
                    Ok(value * 2)
                })?;
                let leaf = edit.append_layout(owner, host, fixed(20.0, 10.0), 0)?;
                edit.bind(derived, Property::color(leaf))?;
                edit.effect(owner, 0, &[derived.dependency()], move |_| {
                    calls.set(calls.get() + 1);
                    None
                })?;
                Ok((prop, derived, leaf))
            },
        )
        .unwrap();
    let (prop, derived, leaf) = init.0.result.value;
    let before = ui.published_local_frame().unwrap().clone();
    let stats = ui.stats();
    assert!(matches!(
        ui.edit(|_| Ok(()), |(), edit| edit.prop(owner, 0, 99u32)),
        Err(Error::InvalidStyle)
    ));
    assert_eq!(ui.stats(), stats);
    assert_eq!(*ui.read_prop(prop).unwrap(), 10);
    assert_eq!(*ui.read(derived).unwrap(), 20);
    assert!(before.same(ui.published_local_frame().unwrap()));
    assert_eq!(seen.get(), 1);
    let invalid = ui.edit(
        |_| Ok(()),
        |(), edit| {
            edit.prop(owner, 0, 15u32)?;
            edit.properties().opacity(leaf, f32::NAN);
            Ok(())
        },
    );
    assert!(matches!(invalid, Err(Error::InvalidOpacity)));
    assert_eq!(ui.stats(), stats);
    assert_eq!(*ui.read_prop(prop).unwrap(), 10);
    assert_eq!(seen.get(), 1);
    ui.edit(
        |_| Ok(()),
        |(), edit| {
            assert!(edit.prop(owner, 0, 99u32).is_err());
            assert_eq!(*edit.read_prop(prop)?, 10);
            edit.prop(owner, 0, 12u32)?;
            Ok(())
        },
    )
    .unwrap();
    assert_eq!(*ui.read(derived).unwrap(), 24);
    assert_eq!(seen.get(), 2);
}

#[test]
fn read_prop_late_payload_write_rejects_stale_producer_without_replay() {
    let mut ui = LogicalUi::new_layout(root_style(), 0, BackendCapabilities::default()).unwrap();
    let owner = ui.root_scope();
    let root = ui.root_render();
    let left = ui
        .append_render_layout(owner, root, LayoutStyle::column(), 0)
        .unwrap();
    let right = ui
        .append_render_layout(owner, root, LayoutStyle::column(), 0)
        .unwrap();
    let prop = ui
        .edit(|_| Ok(()), |(), edit| edit.prop(owner, 0, 10u32))
        .unwrap()
        .result
        .value;
    let before = ui.frame_in_viewport(constraints(), [180.0; 2]).unwrap();
    let seen = Cell::new(0);
    let result = ui.edit_local_frame(
        constraints(),
        [180.0; 2],
        |_| Ok(()),
        |(), edit| {
            let mut plan = LayoutBuildPlan::new();
            plan.add(
                LayoutBuildTaskId(1),
                left,
                LayoutBuildKind::Content,
                &[],
                |_, edit, _| {
                    // A persistent callback records the same source read; install it
                    // here so its own final validation cannot be erased by a late write.
                    edit.layout_producer(
                        owner,
                        0,
                        left,
                        LayoutBuildKind::Content,
                        &[],
                        LayoutProducer::new(move |_, edit| {
                            edit.read_prop(prop)?;
                            Ok(())
                        }),
                    )?;
                    Ok(())
                },
            )?;
            plan.add(
                LayoutBuildTaskId(2),
                right,
                LayoutBuildKind::Content,
                &[],
                |_, _, _| {
                    seen.set(seen.get() + 1);
                    Ok(())
                },
            )?;
            edit.build_layout_plan(plan)?;
            // Run automatic producer, then try a source-changing responsive stage
            // through a later producer registered after the reader.
            edit.layout_producer(
                owner,
                1,
                right,
                LayoutBuildKind::Content,
                &[],
                LayoutProducer::new(move |_, edit| {
                    edit.prop(owner, 0, 20u32)?;
                    Ok(())
                }),
            )?;
            Ok(())
        },
    );
    assert!(matches!(result, Err(Error::InvalidLayoutBuildPlan)));
    assert_eq!(seen.get(), 1);
    assert_eq!(*ui.read_prop(prop).unwrap(), 10);
    assert_eq!(
        pixels(before),
        pixels(ui.frame_in_viewport(constraints(), [180.0; 2]).unwrap())
    );
}

#[test]
fn read_prop_created_inputs_replace_final_values_and_failed_handles_never_revive() {
    let mut ui = LogicalUi::new_layout(root_style(), 0, BackendCapabilities::default()).unwrap();
    let owner = ui.root_scope();
    let root = ui.root_render();
    let mut leaked = None;
    let failure = ui.edit(
        |_| Ok(()),
        |(), edit| {
            leaked = Some(edit.prop(owner, 0, 1u32)?);
            Err::<(), _>(Error::InvalidStyle)
        },
    );
    assert!(matches!(failure, Err(Error::InvalidStyle)));
    assert!(ui.read_prop(leaked.unwrap()).is_err());
    let result = ui
        .edit_local_frame(
            constraints(),
            [180.0; 2],
            |_| Ok(()),
            |(), edit| {
                let prop = edit.prop(owner, 0, 10u32)?;
                assert_ne!(Some(prop), leaked);
                let sum = edit.computed(owner, 1, &[prop.dependency()], move |i| {
                    Ok(*i.read_prop(prop)? + 1)
                })?;
                let leaf = edit.append_layout(owner, root, fixed(20.0, 10.0), 0)?;
                edit.bind_prop(prop, Property::color(leaf))?;
                edit.prop(owner, 0, 20u32)?;
                edit.prop(owner, 0, 10u32)?;
                edit.prop(owner, 0, 30u32)?;
                assert_eq!(*edit.read(sum)?, 31);
                Ok((prop, sum, leaf))
            },
        )
        .unwrap();
    let (prop, sum, leaf) = result.0.result.value;
    assert_eq!(*ui.read_prop(prop).unwrap(), 30);
    assert_eq!(*ui.read(sum).unwrap(), 31);
    assert_eq!(ui.reactive().tree().node(leaf).unwrap().color, 30);
    assert!(ui.read_prop(leaked.unwrap()).is_err());
    assert!(matches!(
        ui.computed(owner, 5, &[], move |i| Ok(*i.read_prop(prop)?)),
        Err(Error::UndeclaredDependency)
    ));
}

#[test]
fn read_prop_external_derived_dependency_rejects_owner_retirement_atomically() {
    let mut ui = LogicalUi::new_layout(root_style(), 0, BackendCapabilities::default()).unwrap();
    let owner = ui.root_scope();
    let prop = ui
        .edit(
            |_| Ok(()),
            |(), edit| {
                let child = edit.reconcile_keys(owner, &[0])?[0];
                edit.prop(child, 0, 10u32)
            },
        )
        .unwrap()
        .result
        .value;
    let derived = ui
        .computed(owner, 0, &[prop.dependency()], move |i| {
            Ok(*i.read_prop(prop)?)
        })
        .unwrap();
    let scopes = ui.children(owner).unwrap().to_vec();
    assert!(matches!(
        ui.edit(
            |_| Ok(()),
            |(), edit| {
                edit.reconcile_keys(owner, &[])?;
                Ok(())
            }
        ),
        Err(Error::SignalInUse)
    ));
    assert_eq!(ui.children(owner).unwrap(), scopes);
    assert_eq!(*ui.read_prop(prop).unwrap(), 10);
    assert_eq!(*ui.read(derived).unwrap(), 10);
}
