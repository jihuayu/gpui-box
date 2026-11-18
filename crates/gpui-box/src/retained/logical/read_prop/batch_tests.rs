use super::*;
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

fn fixed(w: f32, h: f32) -> LayoutStyle {
    LayoutStyle::column()
        .size(LayoutDimension::Px(w), LayoutDimension::Px(h))
        .flex_shrink(0.0)
}
fn constraints() -> LayoutConstraints {
    LayoutConstraints::new(
        LayoutAvailableSpace::Unbounded,
        LayoutAvailableSpace::Unbounded,
    )
}
fn fixture() -> LogicalUi {
    LogicalUi::new_layout(LayoutStyle::column(), 0, BackendCapabilities::default()).unwrap()
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

#[test]
fn prop_batch_interval_updates_measure_only_final_tuple_and_restore_skips_derived_work() {
    let mut ui = fixture();
    let owner = ui.root_scope();
    let host = ui.root_render();
    let calls = Rc::new(Cell::new(0));
    let count = calls.clone();
    let effects = Rc::new(RefCell::new(vec![]));
    let log = effects.clone();
    let initial = ui
        .edit_local_frame(
            constraints(),
            [180.0; 2],
            |_| Ok(()),
            |(), edit| {
                let (lower, upper) = edit.props(|props| {
                    Ok((props.prop(owner, 0, 10u32)?, props.prop(owner, 1, 20u32)?))
                })?;
                let width = edit.computed(
                    owner,
                    2,
                    &[lower.dependency(), upper.dependency()],
                    move |inputs| {
                        count.set(count.get() + 1);
                        let (a, b) = (*inputs.read_prop(lower)?, *inputs.read_prop(upper)?);
                        if a > b {
                            return Err(Error::InvalidStyle);
                        }
                        Ok(fixed((b - a) as f32, 10.0))
                    },
                )?;
                let node = edit.append_layout(owner, host, LayoutStyle::column(), 0xff0000ff)?;
                edit.bind(width, Property::layout_style(node))?;
                edit.effect(
                    owner,
                    0,
                    &[lower.dependency(), upper.dependency()],
                    move |i| {
                        log.borrow_mut()
                            .push((*i.read_prop(lower).unwrap(), *i.read_prop(upper).unwrap()));
                        None
                    },
                )?;
                Ok((lower, upper, node))
            },
        )
        .unwrap();
    let (lower, upper, node) = initial.0.result.value;
    assert!(matches!(
        ui.edit(|_| Ok(()), |(), edit| edit.prop(owner, 0, 30u32)),
        Err(Error::InvalidStyle)
    ));
    let before = calls.get();
    ui.edit(
        |_| Ok(()),
        |(), edit| {
            edit.props(|props| {
                assert_eq!(props.prop(owner, 0, 100u32)?, lower);
                assert_eq!(props.prop(owner, 1, 40u32)?, upper);
                props.prop(owner, 0, 30u32)?;
                Ok(())
            })?;
            assert_eq!((*edit.read_prop(lower)?, *edit.read_prop(upper)?), (30, 40));
            Ok(())
        },
    )
    .unwrap();
    assert_eq!(calls.get(), before + 1);
    assert_eq!(*effects.borrow(), [(10, 20), (30, 40)]);
    assert_eq!(ui.layout_result(node).unwrap().size, [10.0, 10.0]);
    let before = calls.get();
    let frame = ui.published_local_frame().unwrap().clone();
    let equal = ui
        .edit(
            |_| Ok(()),
            |(), edit| {
                edit.props(|props| {
                    props.prop(owner, 0, 100u32)?;
                    props.prop(owner, 0, 30u32)?;
                    Ok(())
                })
            },
        )
        .unwrap();
    assert_eq!(
        calls.get(),
        before,
        "restored final roots do not reevaluate derivatives"
    );
    assert_eq!(equal.result.effect_runs, 0);
    assert!(frame.same(ui.published_local_frame().unwrap()));
}

#[test]
fn prop_batch_failed_declaration_or_derive_restores_new_and_existing_sources() {
    let mut ui = fixture();
    let owner = ui.root_scope();
    let (a, b) = ui
        .edit(
            |_| Ok(()),
            |(), edit| {
                let a = edit.prop(owner, 0, 10u32)?;
                let b = edit.prop(owner, 1, 20u32)?;
                edit.computed(owner, 2, &[a.dependency(), b.dependency()], move |i| {
                    let (a, b) = (*i.read_prop(a)?, *i.read_prop(b)?);
                    if a > b {
                        return Err(Error::InvalidStyle);
                    }
                    Ok(b - a)
                })?;
                Ok((a, b))
            },
        )
        .unwrap()
        .result
        .value;
    let source_count = ui.reactive().source_count();
    let mut leaked = None;
    ui.edit(
        |_| Ok(()),
        |(), edit| {
            let failure = edit.props(|props| {
                props.prop(owner, 0, 30u32)?;
                leaked = Some(props.prop(owner, 100, "tentative".to_owned())?);
                Ok(())
            });
            assert!(matches!(failure, Err(Error::InvalidStyle)));
            assert_eq!(*edit.read_prop(a)?, 10);
            assert_eq!(*edit.read_prop(b)?, 20);
            assert!(edit.read_prop(leaked.unwrap()).is_err());
            let success = edit.props(|props| {
                props.prop(owner, 0, 30u32)?;
                props.prop(owner, 1, 40u32)?;
                props.prop(owner, 100, "final".to_owned())
            })?;
            assert_ne!(success, leaked.unwrap());
            assert_eq!(edit.read_prop(success)?, "final");
            Ok(())
        },
    )
    .unwrap();
    assert_eq!(ui.reactive().source_count(), source_count + 1);
    assert_eq!(
        (*ui.read_prop(a).unwrap(), *ui.read_prop(b).unwrap()),
        (30, 40)
    );
    assert!(ui.read_prop(leaked.unwrap()).is_err());
    ui.edit(
        |_| Ok(()),
        |(), edit| {
            let failure = edit.props(|props| {
                props.prop(owner, 0, 35u32)?;
                assert!(matches!(
                    props.prop(owner, 0, 1u64),
                    Err(Error::InvalidSignal)
                ));
                Ok(()) // catching a declaration error cannot adopt earlier writes
            });
            assert!(matches!(failure, Err(Error::InvalidPropBatch)));
            assert_eq!(*edit.read_prop(a)?, 30);
            Ok(())
        },
    )
    .unwrap();
}

#[test]
fn prop_batch_panic_unwind_and_explicit_abort_leave_no_input_or_counter_changes() {
    let mut ui = fixture();
    let owner = ui.root_scope();
    let mut leaked = None;
    let stats = ui.stats();
    ui.edit(
        |_| Ok(()),
        |(), edit| {
            let unwind = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let _: Result<(), Error> = edit.props(|props| {
                    leaked = Some(props.prop(owner, 0, 10u32)?);
                    panic!("expected prop batch unwind")
                });
            }));
            assert!(unwind.is_err());
            assert!(edit.read_prop(leaked.unwrap()).is_err());
            let abort: Result<(), Error> = edit.props(|props| {
                props.prop(owner, 0, 20u32)?;
                Err(Error::InvalidStyle)
            });
            assert!(matches!(abort, Err(Error::InvalidStyle)));
            let prop = edit.prop(owner, 0, 30u32)?;
            assert_ne!(Some(prop), leaked);
            Ok(())
        },
    )
    .unwrap();
    assert_eq!(ui.reactive().source_count(), 1);
    assert_eq!(ui.stats().source_writes, stats.source_writes);
    assert!(ui.read_prop(leaked.unwrap()).is_err());
}

#[test]
fn prop_batch_new_inputs_and_draft_computed_settle_as_one_graph() {
    let mut ui = fixture();
    let owner = ui.root_scope();
    let host = ui.root_render();
    let seen = Rc::new(Cell::new(0));
    let calls = seen.clone();
    let result = ui
        .edit_local_frame(
            constraints(),
            [180.0; 2],
            |_| Ok(()),
            |(), edit| {
                let (a, b) = edit.props(|props| {
                    Ok((props.prop(owner, 0, 10u32)?, props.prop(owner, 1, 20u32)?))
                })?;
                let derived =
                    edit.computed(owner, 2, &[a.dependency(), b.dependency()], move |i| {
                        calls.set(calls.get() + 1);
                        let (a, b) = (*i.read_prop(a)?, *i.read_prop(b)?);
                        if a > b {
                            return Err(Error::InvalidStyle);
                        }
                        Ok(b - a)
                    })?;
                let node = edit.append_layout(owner, host, fixed(20.0, 10.0), 0)?;
                edit.bind(derived, Property::color(node))?;
                edit.props(|props| {
                    props.prop(owner, 0, 30u32)?;
                    props.prop(owner, 1, 55u32)?;
                    let text = props.prop(owner, 3, "initial".to_owned())?;
                    assert_eq!(props.prop(owner, 3, "final".to_owned())?, text);
                    Ok(())
                })?;
                assert_eq!(*edit.read(derived)?, 25);
                Ok((a, b, derived, node))
            },
        )
        .unwrap();
    let (a, b, derived, node) = result.0.result.value;
    assert_eq!(seen.get(), 2);
    assert_eq!(*ui.read(derived).unwrap(), 25);
    assert_eq!(
        (*ui.read_prop(a).unwrap(), *ui.read_prop(b).unwrap()),
        (30, 55)
    );
    assert_eq!(ui.reactive().tree().node(node).unwrap().color, 25);
}

#[test]
fn prop_batch_keyed_payloads_mount_once_and_match_complete_reference() {
    run(false);
}
#[cfg(feature = "retained-wgpu")]
#[test]
#[ignore = "requires WGPU adapter; run explicitly"]
fn hardware_prop_batch_keyed_payloads_match_complete_reference() {
    run(true);
}
fn run(hardware: bool) {
    #[cfg(feature = "retained-wgpu")]
    let mut gpu = hardware.then(|| wgpu::WgpuFrameRenderer::new(2).unwrap());
    #[cfg(feature = "retained-wgpu")]
    if let Some(gpu) = &gpu {
        eprintln!("PropBatch adapter: {:?}", gpu.adapter_info());
    }
    #[cfg(not(feature = "retained-wgpu"))]
    assert!(!hardware);
    for local in [false, true] {
        let initial = vec![(1u64, 10u32, 30u32, 0xff0000ff), (2, 20, 45, 0x00ff00ff)];
        let mut ui = fixture();
        let owner = ui.root_scope();
        let host = ui.root_render();
        let model = ui.state(owner, 0, || initial.clone()).unwrap();
        let mounts = Rc::new(Cell::new(0));
        let count = mounts.clone();
        let evals = Rc::new(Cell::new(0));
        let evaluations = evals.clone();
        let producer = LayoutProducer::new(move |_, edit| {
            let rows = edit.read(model)?.clone();
            let scopes =
                edit.reconcile_keys(owner, &rows.iter().map(|r| r.0).collect::<Vec<_>>())?;
            let inputs = edit.props(|props| {
                scopes
                    .iter()
                    .copied()
                    .zip(rows.iter())
                    .map(|(scope, &(_, a, b, color))| {
                        Ok((
                            scope,
                            props.prop(scope, 0, a)?,
                            props.prop(scope, 1, b)?,
                            props.prop(scope, 2, color)?,
                        ))
                    })
                    .collect::<Result<Vec<_>, Error>>()
            })?;
            for (scope, a, b, color) in inputs {
                if edit.renders(scope)?.is_empty() {
                    count.set(count.get() + 1);
                    let calls = evaluations.clone();
                    let style =
                        edit.computed(scope, 3, &[a.dependency(), b.dependency()], move |i| {
                            calls.set(calls.get() + 1);
                            let (a, b) = (*i.read_prop(a)?, *i.read_prop(b)?);
                            if a > b {
                                return Err(Error::InvalidStyle);
                            }
                            Ok(fixed((b - a) as f32, 10.0))
                        })?;
                    let node = edit.append_layout(scope, host, LayoutStyle::column(), 0)?;
                    edit.bind(style, Property::layout_style(node))?;
                    edit.bind_prop(color, Property::color(node))?;
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
        let node = ui.renders(first_scopes[0]).unwrap()[0];
        let mut old = None;
        for (index, rows) in [
            initial,
            vec![(2, 50, 90, 0x0000ffff), (1, 40, 65, 0xffff00ff)],
            vec![(1, 70, 100, 0xff00ffff), (3, 25, 60, 0x00ffffff)],
        ]
        .into_iter()
        .enumerate()
        {
            if index > 0 {
                ui.transaction(|tx| tx.set(model, rows.clone())).unwrap();
            }
            assert_eq!(mounts.get(), if index < 2 { 2 } else { 3 });
            let scope = ui.children(owner).unwrap()[if index == 1 { 1 } else { 0 }];
            assert_eq!(scope, first_scopes[0]);
            assert_eq!(ui.renders(scope).unwrap(), [node]);
            let mut full = UiTree::new_layout(LayoutStyle::column(), 0).unwrap();
            full.set_frame_build_policy(FrameBuildPolicy::Full);
            for &(_, a, b, color) in &rows {
                full.append_layout(full.root(), fixed((b - a) as f32, 10.0), color)
                    .unwrap();
            }
            let reference = full.frame_in_viewport(constraints(), [180.0; 2]).unwrap();
            let frame = ui.frame_in_viewport(constraints(), [180.0; 2]).unwrap();
            assert_eq!(pixels(frame.clone()), pixels(reference.clone()));
            #[cfg(feature = "retained-wgpu")]
            if let Some(gpu) = &mut gpu {
                gpu.submit(&frame, [180, 180], 1.0).unwrap();
                assert_eq!(
                    gpu.image().unwrap().as_raw(),
                    gpu.reference_image(&reference, [180, 180], 1.0)
                        .unwrap()
                        .as_raw()
                );
            }
            old.get_or_insert_with(|| (frame.clone(), pixels(frame)));
        }
        // All live affected calculations run once per batch; the retired key2
        // can still be conservatively evaluated before final scope retirement.
        assert_eq!(evals.get(), 6);
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
fn prop_batch_final_render_failure_and_invalid_scope_restore_complete_frame_and_sources() {
    let mut ui = fixture();
    let owner = ui.root_scope();
    let host = ui.root_render();
    let init = ui
        .edit_local_frame(
            constraints(),
            [180.0; 2],
            |_| Ok(()),
            |(), edit| {
                let (a, b) = edit.props(|props| {
                    Ok((props.prop(owner, 0, 10u32)?, props.prop(owner, 1, 20u32)?))
                })?;
                let leaf = edit.append_layout(owner, host, fixed(20.0, 10.0), 0xff0000ff)?;
                Ok((a, b, leaf))
            },
        )
        .unwrap();
    let (a, b, leaf) = init.0.result.value;
    let frame = ui.published_local_frame().unwrap().clone();
    let stats = ui.stats();
    let sources = ui.reactive().source_count();
    let mut leaked = None;
    assert!(matches!(
        ui.edit(
            |_| Ok(()),
            |(), edit| {
                edit.props(|props| {
                    props.prop(owner, 0, 30u32)?;
                    props.prop(owner, 1, 40u32)?;
                    leaked = Some(props.prop(owner, 100, 99u32)?);
                    Ok(())
                })?;
                edit.properties().opacity(leaf, f32::NAN);
                Ok(())
            }
        ),
        Err(Error::InvalidOpacity)
    ));
    assert_eq!(
        (*ui.read_prop(a).unwrap(), *ui.read_prop(b).unwrap()),
        (10, 20)
    );
    assert_eq!(ui.stats(), stats);
    assert_eq!(ui.reactive().source_count(), sources);
    assert!(ui.read_prop(leaked.unwrap()).is_err());
    assert!(frame.same(ui.published_local_frame().unwrap()));
    let foreign = fixture();
    let foreign_scope = foreign.root_scope();
    ui.edit(
        |_| Ok(()),
        |(), edit| {
            assert!(matches!(
                edit.props(|props| {
                    props.prop(owner, 0, 30u32)?;
                    props.prop(foreign_scope, 0, 1u32)?;
                    Ok(())
                }),
                Err(Error::InvalidScope)
            ));
            assert_eq!(*edit.read_prop(a)?, 10);
            let child = edit.reconcile_keys(owner, &[0])?[0];
            edit.reconcile_keys(owner, &[])?;
            assert!(matches!(
                edit.props(|props| props.prop(child, 0, 1u32)),
                Err(Error::InvalidScope)
            ));
            Ok(())
        },
    )
    .unwrap();
}

#[test]
fn prop_batch_derived_unwind_preserves_prior_successful_draft_values_and_allocations() {
    let mut ui = fixture();
    let owner = ui.root_scope();
    let prop = ui
        .edit(
            |_| Ok(()),
            |(), edit| {
                let prop = edit.prop(owner, 0, 10u32)?;
                edit.computed(owner, 1, &[prop.dependency()], move |i| {
                    let value = *i.read_prop(prop)?;
                    assert_ne!(value, 99, "expected derived unwind");
                    Ok(value)
                })?;
                Ok(prop)
            },
        )
        .unwrap()
        .result
        .value;
    let mut leaked = None;
    ui.edit(
        |_| Ok(()),
        |(), edit| {
            edit.prop(owner, 0, 20u32)?;
            let prior = edit.prop(owner, 10, "prior".to_owned())?;
            let panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let _: Result<(), Error> = edit.props(|props| {
                    props.prop(owner, 0, 99u32)?;
                    leaked = Some(props.prop(owner, 11, "discard".to_owned())?);
                    Ok(())
                });
            }));
            assert!(panic.is_err());
            assert_eq!(*edit.read_prop(prop)?, 20);
            assert_eq!(edit.read_prop(prior)?, "prior");
            assert!(edit.read_prop(leaked.unwrap()).is_err());
            edit.props(|props| {
                props.prop(owner, 0, 30u32)?;
                Ok(())
            })?;
            Ok(())
        },
    )
    .unwrap();
    assert_eq!(*ui.read_prop(prop).unwrap(), 30);
    assert!(ui.read_prop(leaked.unwrap()).is_err());
    assert_eq!(ui.reactive().source_count(), 3);
}
