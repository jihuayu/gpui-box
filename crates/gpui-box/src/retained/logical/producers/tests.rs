use super::*;
use std::cell::Cell;

fn root_style() -> LayoutStyle {
    LayoutStyle::column().size(LayoutDimension::Percent(1.0), LayoutDimension::Auto)
}
fn fixed(width: f32, height: f32) -> LayoutStyle {
    LayoutStyle::row()
        .size(LayoutDimension::Px(width), LayoutDimension::Px(height))
        .flex_shrink(0.0)
}
fn constraints(width: f32) -> LayoutConstraints {
    LayoutConstraints::new(
        LayoutAvailableSpace::Definite(width),
        LayoutAvailableSpace::Unbounded,
    )
}
fn input_width(input: LayoutBuildInput) -> f32 {
    let LayoutBuildInput::Inline(input) = input else {
        panic!()
    };
    input.known_width.unwrap()
}

#[test]
fn registered_producer_tracks_dynamic_signal_reads_and_automatic_transactions() {
    let mut ui = LogicalUi::new_layout(root_style(), 0, BackendCapabilities::default()).unwrap();
    let scope = ui.root_scope();
    let root = ui.root_render();
    let host = ui
        .append_render_layout(scope, root, LayoutStyle::column(), 0)
        .unwrap();
    let a = ui.state(scope, 1, || 10u32).unwrap();
    let b = ui.state(scope, 2, || 20u32).unwrap();
    let selected = ui.state(scope, 3, || false).unwrap();
    let calls = Rc::new(Cell::new(0));
    let count = calls.clone();
    let producer = LayoutProducer::new(move |input, edit| {
        count.set(count.get() + 1);
        assert_eq!(input_width(input), 100.0);
        let value = *edit.read(if *edit.read(selected)? { b } else { a })?;
        let branch = edit.reconcile_keys(scope, &[value as u64])?[0];
        if edit.renders(branch)?.is_empty() {
            edit.append_layout(branch, host, fixed(20.0, value as f32), 0xff0000ff)?;
        }
        Ok(())
    });
    ui.edit_local_frame(
        constraints(100.0),
        [180.0; 2],
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
    assert_eq!(calls.get(), 1);
    assert!(ui.producer_session.is_some());
    assert!(ui.layout_session.is_none());
    let first = ui.published_local_frame().unwrap().clone();
    ui.transaction(|tx| tx.set(b, 30)).unwrap();
    assert_eq!(
        calls.get(),
        1,
        "unread signal is not a construction dependency"
    );
    assert_eq!(ui.layout_producer_stats().callbacks_reused, 1);
    assert!(first.same(ui.published_local_frame().unwrap()));
    ui.transaction(|tx| tx.set(a, 15)).unwrap();
    assert_eq!(calls.get(), 2);
    ui.transaction(|tx| tx.set(selected, true)).unwrap();
    assert_eq!(calls.get(), 3);
    ui.transaction(|tx| tx.set(a, 17)).unwrap();
    assert_eq!(calls.get(), 3, "obsolete conditional read retired");
    ui.transaction(|tx| {
        tx.set(b, 40)?;
        tx.set(b, 30)
    })
    .unwrap();
    assert_eq!(calls.get(), 3);
    ui.transaction(|tx| tx.set(b, 35)).unwrap();
    assert_eq!(calls.get(), 4);
    assert_eq!(ui.layout_result(host).unwrap().size, [100.0, 35.0]);
    assert_eq!(
        first.resolve().items.iter().last().unwrap().bounds.height,
        10.0
    );
}

#[test]
fn resized_parent_registers_and_retires_dynamic_child_producers() {
    let mut ui = LogicalUi::new_layout(root_style(), 0, BackendCapabilities::default()).unwrap();
    let scope = ui.root_scope();
    let root = ui.root_render();
    let host = ui
        .append_render_layout(scope, root, LayoutStyle::column(), 0)
        .unwrap();
    let parent_calls = Rc::new(Cell::new(0));
    let child_calls = Rc::new(Cell::new(0));
    let effects = Rc::new(Cell::new(0));
    let cleanups = Rc::new(Cell::new(0));
    let runs = effects.clone();
    let retires = cleanups.clone();
    let pc = parent_calls.clone();
    let cc = child_calls.clone();
    let producer = LayoutProducer::new(move |input, edit| {
        pc.set(pc.get() + 1);
        let width = input_width(input);
        let keys = if width < 120.0 { vec![] } else { vec![1] };
        let children = edit.reconcile_keys(scope, &keys)?;
        if let Some(&branch) = children.first() {
            let renders = edit.renders(branch)?;
            let inner = match renders.first() {
                Some(id) => *id,
                None => edit.append_layout(branch, host, LayoutStyle::column(), 0)?,
            };
            let count = cc.clone();
            let runs = runs.clone();
            let retires = retires.clone();
            let child = LayoutProducer::new(move |input, edit| {
                count.set(count.get() + 1);
                let output = edit.reconcile_keys(branch, &[0])?[0];
                let runs = runs.clone();
                let retires = retires.clone();
                edit.effect(output, 0, &[], move |_| {
                    runs.set(runs.get() + 1);
                    let retires = retires.clone();
                    Some(Box::new(move || retires.set(retires.get() + 1)))
                })?;
                let renders = edit.renders(output)?;
                let leaf = match renders.first() {
                    Some(id) => *id,
                    None => edit.append_layout(output, inner, fixed(20.0, 10.0), 0x00ff00ff)?,
                };
                edit.properties()
                    .layout_style(leaf, fixed(20.0, input_width(input) / 10.0));
                Ok(())
            });
            edit.layout_producer(
                branch,
                0,
                inner,
                LayoutBuildKind::InlineAllocation,
                &[],
                child,
            )?;
        }
        Ok(())
    });
    ui.edit_local_frame(
        constraints(140.0),
        [180.0; 2],
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
    assert_eq!(child_calls.get(), 1);
    assert_eq!(ui.layout_producers.len(), 2);
    assert_eq!((effects.get(), cleanups.get()), (1, 0));
    let old_scope = ui.children(scope).unwrap()[0];
    let old_child = ui.renders(old_scope).unwrap()[0];
    let old = ui.published_local_frame().unwrap().clone();
    ui.local_frame_in_viewport(constraints(140.0), [180.0; 2])
        .unwrap();
    assert_eq!(parent_calls.get(), 1);
    assert_eq!(child_calls.get(), 1);
    ui.local_frame_in_viewport(constraints(80.0), [180.0; 2])
        .unwrap();
    assert_eq!(
        child_calls.get(),
        1,
        "pending retired child must not execute"
    );
    assert_eq!(ui.layout_producers.len(), 1);
    assert!(ui.children(scope).unwrap().is_empty());
    assert_eq!((effects.get(), cleanups.get()), (1, 1));
    ui.local_frame_in_viewport(constraints(160.0), [180.0; 2])
        .unwrap();
    assert_eq!(child_calls.get(), 2);
    assert_eq!((effects.get(), cleanups.get()), (2, 1));
    assert_ne!(ui.children(scope).unwrap()[0], old_scope);
    assert!(
        ui.published_local_frame()
            .unwrap()
            .resolve_geometry(old_child)
            .is_none()
    );
    assert!(old.resolve_geometry(old_child).is_some());
    assert_eq!(ui.layout_result(host).unwrap().size, [160.0, 16.0]);
}

#[test]
fn equal_handles_retain_cache_replacement_and_explicit_invalidation_rerun() {
    let mut ui = LogicalUi::new_layout(root_style(), 0, BackendCapabilities::default()).unwrap();
    let scope = ui.root_scope();
    let host = ui.root_render();
    let calls = Rc::new(Cell::new(0));
    let count = calls.clone();
    let producer = LayoutProducer::new(move |_, _| {
        count.set(count.get() + 1);
        Ok(())
    });
    let declare = |edit: &mut LogicalEdit<'_, '_, '_>| {
        edit.layout_producer(
            scope,
            0,
            host,
            LayoutBuildKind::InlineAllocation,
            &[],
            producer.clone(),
        )
    };
    let (first, _) = ui
        .edit_local_frame(
            constraints(100.0),
            [180.0; 2],
            |_| Ok(()),
            |(), edit| declare(edit),
        )
        .unwrap();
    let id = first.result.value;
    assert_eq!(calls.get(), 1);
    let (same, _) = ui
        .edit_local_frame(
            constraints(100.0),
            [180.0; 2],
            |_| Ok(()),
            |(), edit| declare(edit),
        )
        .unwrap();
    assert_eq!(same.result.value, id);
    assert_eq!(calls.get(), 1);
    ui.edit(|_| Ok(()), |(), edit| edit.invalidate_layout_producer(id))
        .unwrap();
    assert_eq!(calls.get(), 2);
    let count = calls.clone();
    ui.edit(
        |_| Ok(()),
        |(), edit| {
            edit.layout_producer(
                scope,
                0,
                host,
                LayoutBuildKind::InlineAllocation,
                &[],
                LayoutProducer::new(move |_, _| {
                    count.set(count.get() + 1);
                    Ok(())
                }),
            )
        },
    )
    .unwrap();
    assert_eq!(calls.get(), 3);
    ui.edit(|_| Ok(()), |(), edit| edit.remove_layout_producer(scope, 0))
        .unwrap();
    assert!(ui.producer_session.is_none());
    assert!(ui.layout_producers.is_empty());
}

#[test]
fn callbacks_fail_atomically_and_registry_cache_only_adopts_after_validation() {
    let mut ui = LogicalUi::new_layout(root_style(), 0, BackendCapabilities::default()).unwrap();
    let scope = ui.root_scope();
    let host = ui.root_render();
    let fail = ui.state(scope, 0, || false).unwrap();
    let calls = Rc::new(Cell::new(0));
    let count = calls.clone();
    let producer = LayoutProducer::new(move |_, edit| {
        count.set(count.get() + 1);
        if *edit.read(fail)? {
            return Err(Error::InvalidStyle);
        }
        Ok(())
    });
    ui.edit_local_frame(
        constraints(100.0),
        [180.0; 2],
        |_| Ok(()),
        |(), edit| {
            edit.layout_producer(
                scope,
                0,
                host,
                LayoutBuildKind::InlineAllocation,
                &[],
                producer,
            )
        },
    )
    .unwrap();
    let frame = ui.published_local_frame().unwrap().clone();
    let stats = ui.layout_producer_stats();
    assert!(ui.transaction(|tx| tx.set(fail, true)).is_err());
    assert!(!*ui.read(fail).unwrap());
    assert_eq!(ui.layout_producer_stats(), stats);
    assert!(frame.same(ui.published_local_frame().unwrap()));
    ui.transaction(|tx| tx.set(fail, false)).unwrap();
    assert_eq!(calls.get(), 2, "failed cached read was not adopted");
    let invalid = ui.edit(
        |_| Ok(()),
        |(), edit| {
            let replacement = LayoutProducer::new(move |_, edit| {
                edit.properties().translation(host, [f32::MAX, 0.0]);
                edit.properties().clip(
                    host,
                    Some(Rect {
                        x: f32::MAX,
                        y: 0.0,
                        width: 10.0,
                        height: 10.0,
                    }),
                );
                Ok(())
            });
            edit.layout_producer(
                scope,
                0,
                host,
                LayoutBuildKind::InlineAllocation,
                &[],
                replacement,
            )?;
            Ok(())
        },
    );
    assert!(invalid.is_err());
    assert!(frame.same(ui.published_local_frame().unwrap()));
    ui.transaction(|_| Ok(())).unwrap();
    assert_eq!(calls.get(), 2);
}

#[test]
fn explicit_prerequisites_late_source_writes_and_cycles_are_validated() {
    use crate::retained::responsive::*;
    let mut ui = LogicalUi::new_layout(root_style(), 0, BackendCapabilities::default()).unwrap();
    let scope = ui.root_scope();
    let root = ui.root_render();
    let a = ui
        .append_render_layout(scope, root, LayoutStyle::column(), 0)
        .unwrap();
    let b = ui
        .append_render_layout(scope, root, LayoutStyle::column(), 0)
        .unwrap();
    let style = ui
        .state(scope, 0, || LayoutStyle::column().fixed_inline_size(100.0))
        .unwrap();
    let calls = Rc::new(RefCell::new(vec![]));
    let log = calls.clone();
    let writer = LayoutProducer::new(move |_, edit| {
        log.borrow_mut().push(1);
        edit.responsive_rule(
            scope,
            0,
            style,
            b,
            LayoutQuery::ViewportInline,
            LayoutBreakpointStyles::new(LayoutStyle::column().fixed_inline_size(70.0), vec![])?,
        )?;
        Ok(())
    });
    let log = calls.clone();
    let reader = LayoutProducer::new(move |input, edit| {
        log.borrow_mut().push(2);
        assert_eq!(input_width(input), 70.0);
        assert_eq!(edit.read(style)?.size.width, LayoutDimension::Px(70.0));
        Ok(())
    });
    let (result, _) = ui
        .edit_local_frame(
            constraints(100.0),
            [180.0; 2],
            |_| Ok(()),
            |(), edit| {
                edit.layout_environment(&LayoutEnvironment::new([100.0, 180.0]))?;
                let writer = edit.layout_producer(
                    scope,
                    1,
                    a,
                    LayoutBuildKind::InlineAllocation,
                    &[],
                    writer,
                )?;
                let reader = edit.layout_producer(
                    scope,
                    2,
                    b,
                    LayoutBuildKind::InlineAllocation,
                    &[writer],
                    reader,
                )?;
                Ok((writer, reader))
            },
        )
        .unwrap();
    assert_eq!(&*calls.borrow(), &[1, 2]);
    let (writer_id, reader_id) = result.result.value;
    let before = ui.published_local_frame().unwrap().clone();
    let cycle = ui.edit(
        |_| Ok(()),
        |(), edit| {
            edit.layout_producer(
                scope,
                1,
                a,
                LayoutBuildKind::InlineAllocation,
                &[reader_id],
                LayoutProducer::new(|_, _| Ok(())),
            )?;
            Ok(())
        },
    );
    assert!(matches!(cycle, Err(Error::CyclicLayoutBuildPlan)));
    assert!(before.same(ui.published_local_frame().unwrap()));

    // A later producer cannot change a source already consumed upstream and
    // publish stale construction. No callback replay to hide the ordering bug.
    let late = ui.edit(
        |_| Ok(()),
        |(), edit| {
            edit.layout_producer(
                scope,
                1,
                a,
                LayoutBuildKind::InlineAllocation,
                &[],
                LayoutProducer::new(move |_, edit| {
                    edit.read(style)?;
                    Ok(())
                }),
            )?;
            edit.layout_producer(
                scope,
                2,
                b,
                LayoutBuildKind::InlineAllocation,
                &[writer_id],
                LayoutProducer::new(move |_, edit| {
                    edit.responsive_rule(
                        scope,
                        0,
                        style,
                        b,
                        LayoutQuery::ViewportInline,
                        LayoutBreakpointStyles::new(
                            LayoutStyle::column().fixed_inline_size(60.0),
                            vec![],
                        )?,
                    )?;
                    Ok(())
                }),
            )?;
            Ok(())
        },
    );
    assert!(late.is_err());
    assert_eq!(
        ui.read(style).unwrap().size.width,
        LayoutDimension::Px(70.0)
    );
    assert!(before.same(ui.published_local_frame().unwrap()));
}

#[test]
fn worker_updates_computed_reads_and_external_render_edits_invalidate_safely() {
    let mut ui = LogicalUi::new_layout(root_style(), 0, BackendCapabilities::default()).unwrap();
    let scope = ui.root_scope();
    let root = ui.root_render();
    let source = ui.state(scope, 0, || 1u32).unwrap();
    let computed = ui
        .computed(scope, 1, &[source.dependency()], move |i| {
            Ok(*i.read(source)? / 2)
        })
        .unwrap();
    let host = ui
        .append_render_layout(scope, root, LayoutStyle::column(), 0)
        .unwrap();
    let leaf = ui
        .append_render_layout(scope, host, fixed(20.0, 10.0), 0)
        .unwrap();
    let calls = Rc::new(Cell::new(0));
    let count = calls.clone();
    ui.edit_local_frame(
        constraints(100.0),
        [180.0; 2],
        |_| Ok(()),
        |(), edit| {
            edit.layout_producer(
                scope,
                0,
                host,
                LayoutBuildKind::InlineAllocation,
                &[],
                LayoutProducer::new(move |_, edit| {
                    count.set(count.get() + 1);
                    let value = *edit.read(computed)?;
                    edit.properties().color(leaf, value);
                    Ok(())
                }),
            )?;
            Ok(())
        },
    )
    .unwrap();
    let mut receiver = ui.open_updates(4).unwrap();
    let sender = ui.update_sender(&receiver, source).unwrap();
    std::thread::spawn(move || {
        sender.send(2).unwrap();
        sender.send(3).unwrap();
    })
    .join()
    .unwrap();
    ui.apply_updates(&mut receiver).unwrap();
    assert_eq!(calls.get(), 2);
    ui.transaction(|tx| tx.set(source, 2)).unwrap();
    assert_eq!(calls.get(), 2, "equal computed output reuses construction");
    ui.edit(
        |_| Ok(()),
        |(), edit| {
            edit.properties().color(leaf, 99);
            Ok(())
        },
    )
    .unwrap();
    assert_eq!(
        calls.get(),
        3,
        "render edits conservatively invalidate cached output"
    );
    assert_eq!(
        ui.published_local_frame()
            .unwrap()
            .resolve_geometry(leaf)
            .unwrap()
            .fill_color,
        1
    );
}

#[test]
fn no_session_duplicate_host_and_caught_registration_errors_reject_publication() {
    let mut ui = LogicalUi::new_layout(root_style(), 0, BackendCapabilities::default()).unwrap();
    let scope = ui.root_scope();
    let host = ui.root_render();
    assert!(
        ui.edit(
            |_| Ok(()),
            |(), edit| {
                let _ = edit.layout_producer(
                    scope,
                    0,
                    host,
                    LayoutBuildKind::InlineAllocation,
                    &[],
                    LayoutProducer::new(|_, _| Ok(())),
                );
                Ok(())
            }
        )
        .is_err()
    );
    assert!(ui.layout_producers.is_empty());
    let result = ui.edit_local_frame(
        constraints(100.0),
        [180.0; 2],
        |_| Ok(()),
        |(), edit| {
            edit.layout_producer(
                scope,
                0,
                host,
                LayoutBuildKind::InlineAllocation,
                &[],
                LayoutProducer::new(|_, _| Ok(())),
            )?;
            let _ = edit.layout_producer(
                scope,
                1,
                host,
                LayoutBuildKind::InlineAllocation,
                &[],
                LayoutProducer::new(|_, _| Ok(())),
            );
            Ok(())
        },
    );
    assert!(result.is_err());
    assert!(ui.layout_producers.is_empty());
}

#[test]
fn cross_scope_sources_cannot_retire_while_a_surviving_producer_observes_them() {
    let mut ui = LogicalUi::new_layout(root_style(), 0, BackendCapabilities::default()).unwrap();
    let scope = ui.root_scope();
    let root = ui.root_render();
    let branch = ui.reconcile_keys(scope, &[1]).unwrap().order[0];
    let source = ui.state(branch, 0, || 12u32).unwrap();
    let producer = LayoutProducer::new(move |_, edit| {
        edit.read(source)?;
        Ok(())
    });
    ui.edit_local_frame(
        constraints(100.0),
        [180.0; 2],
        |_| Ok(()),
        |(), edit| {
            edit.layout_producer(
                scope,
                0,
                root,
                LayoutBuildKind::InlineAllocation,
                &[],
                producer,
            )
        },
    )
    .unwrap();
    let before = ui.published_local_frame().unwrap().clone();
    assert!(ui.reconcile_keys(scope, &[]).is_err());
    assert_eq!(*ui.read(source).unwrap(), 12);
    assert_eq!(ui.children(scope).unwrap(), &[branch]);
    assert!(before.same(ui.published_local_frame().unwrap()));
    ui.edit(
        |_| Ok(()),
        |(), edit| {
            edit.remove_layout_producer(scope, 0)?;
            edit.reconcile_keys(scope, &[])?;
            Ok(())
        },
    )
    .unwrap();
    assert!(ui.read(source).is_err());
    assert!(ui.layout_producers.is_empty());
}

#[test]
fn producer_supports_all_builder_kinds_in_one_automatic_local_publication() {
    let mut ui = LogicalUi::new_layout(
        root_style().size(LayoutDimension::Percent(1.0), LayoutDimension::Px(160.0)),
        0,
        BackendCapabilities::default(),
    )
    .unwrap();
    let root = ui.root_render();
    let scope = ui.root_scope();
    let count = Rc::new(Cell::new(0));
    let styles = [
        LayoutStyle::constraint_box(ConstraintFit::Tight, ConstraintFit::Loose)
            .container_type(LayoutContainerType::Size)
            .size(LayoutDimension::Px(60.0), LayoutDimension::Px(40.0)),
        LayoutStyle::constraint_box(ConstraintFit::Tight, ConstraintFit::Loose)
            .container_type(LayoutContainerType::InlineSize)
            .size(LayoutDimension::Px(80.0), LayoutDimension::Auto),
        LayoutStyle::column()
            .container_type(LayoutContainerType::Size)
            .size(LayoutDimension::Px(70.0), LayoutDimension::Px(35.0)),
        LayoutStyle::column(),
    ];
    let mut hosts = vec![];
    for style in styles {
        hosts.push(ui.append_render_layout(scope, root, style, 0).unwrap());
    }
    ui.edit_local_frame(
        constraints(140.0),
        [180.0; 2],
        |_| Ok(()),
        |(), edit| {
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
                let count = count.clone();
                edit.layout_producer(
                    scope,
                    index as u64,
                    host,
                    kind,
                    &[],
                    LayoutProducer::new(move |input, _| {
                        count.set(count.get() + 1);
                        assert_eq!(
                            matches!(input, LayoutBuildInput::Full(_)),
                            index == 0 || index == 2
                        );
                        Ok(())
                    }),
                )?;
            }
            Ok(())
        },
    )
    .unwrap();
    assert_eq!(count.get(), 4);
    ui.transaction(|_| Ok(())).unwrap();
    assert_eq!(count.get(), 4);
    assert_eq!(ui.layout_producer_stats().callbacks_reused, 4);
}
