use super::*;
use std::cell::Cell;

fn constraints(width: f32) -> LayoutConstraints {
    LayoutConstraints::new(
        LayoutAvailableSpace::Definite(width),
        LayoutAvailableSpace::Unbounded,
    )
}
fn fixture() -> (LogicalUi, ScopeId, NodeId) {
    let mut ui = LogicalUi::new_layout(
        LayoutStyle::column().size(LayoutDimension::Percent(1.0), LayoutDimension::Auto),
        0,
        BackendCapabilities::default(),
    )
    .unwrap();
    let scope = ui.root_scope();
    let host = ui
        .append_render_layout(scope, ui.root_render(), LayoutStyle::column(), 0)
        .unwrap();
    (ui, scope, host)
}

#[test]
fn indexed_unread_equal_and_computed_updates_skip_the_entire_plan() {
    let (mut ui, scope, host) = fixture();
    let source = ui.state(scope, 1, || 1u32).unwrap();
    let unused = ui.state(scope, 2, || 0u32).unwrap();
    let computed = ui
        .computed(scope, 3, &[source.dependency()], move |input| {
            Ok(*input.read(source)? / 10)
        })
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
                    edit.read(computed)?;
                    Ok(())
                }),
            )?;
            Ok(())
        },
    )
    .unwrap();
    let frame = ui.published_local_frame().unwrap().clone();
    for changed in [unused, source, unused] {
        ui.transaction(|tx| tx.set(changed, 2)).unwrap();
        let stats = ui.layout_producer_stats();
        assert_eq!(stats.plans_skipped, 1);
        assert_eq!(stats.planning_previews, 0);
        assert_eq!(stats.considered, 0);
        assert_eq!(stats.source_candidates, 0);
        assert_eq!(stats.callbacks_reused, 1);
        assert!(frame.same(ui.published_local_frame().unwrap()));
    }
    ui.transaction(|tx| {
        tx.set(source, 20)?;
        tx.set(source, 2)
    })
    .unwrap();
    assert_eq!(calls.get(), 1);
    assert_eq!(ui.layout_producer_stats().plans_skipped, 1);
    ui.transaction(|tx| tx.set(source, 25)).unwrap();
    assert_eq!(calls.get(), 2);
    assert_eq!(ui.layout_producer_stats().plans_skipped, 0);
    assert_eq!(ui.layout_producer_stats().source_candidates, 1);
    assert!(ui.layout_producer_stats().planning_previews > 0);
}

#[test]
fn conditional_index_changes_only_adopt_after_successful_final_validation() {
    let (mut ui, scope, host) = fixture();
    let selected = ui.state(scope, 1, || false).unwrap();
    let a = ui.state(scope, 2, || 1u32).unwrap();
    let b = ui.state(scope, 3, || 2u32).unwrap();
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
                    let which = *edit.read(selected)?;
                    let value = *edit.read(if which { b } else { a })?;
                    if value == 99 {
                        edit.properties().translation(host, [f32::NAN, 0.0]);
                    }
                    Ok(())
                }),
            )?;
            Ok(())
        },
    )
    .unwrap();
    ui.transaction(|tx| tx.set(b, 3)).unwrap();
    assert_eq!(ui.layout_producer_stats().source_candidates, 0);
    let frame = ui.published_local_frame().unwrap().clone();
    let stats = ui.layout_producer_stats();
    assert!(
        ui.transaction(|tx| {
            tx.set(selected, true)?;
            tx.set(b, 99)
        })
        .is_err()
    );
    assert_eq!(ui.layout_producer_stats(), stats);
    assert!(frame.same(ui.published_local_frame().unwrap()));
    assert!(!ui.producer_index.0.contains_key(&b.dependency()));
    ui.transaction(|tx| tx.set(b, 4)).unwrap();
    assert_eq!(ui.layout_producer_stats().plans_skipped, 1);
    ui.transaction(|tx| tx.set(selected, true)).unwrap();
    assert!(ui.producer_index.0.contains_key(&b.dependency()));
    assert!(!ui.producer_index.0.contains_key(&a.dependency()));
    ui.transaction(|tx| tx.set(a, 8)).unwrap();
    assert_eq!(ui.layout_producer_stats().source_candidates, 0);
    ui.transaction(|tx| tx.set(b, 8)).unwrap();
    assert_eq!(ui.layout_producer_stats().plans_skipped, 0);
    assert_eq!(
        calls.get(),
        4,
        "initial, failed branch, successful branch, b change"
    );
}

#[test]
fn bindings_explicit_properties_environment_and_invalidation_keep_complete_plan() {
    let (mut ui, scope, host) = fixture();
    let color = ui.state(scope, 1, || 1u32).unwrap();
    let count = Rc::new(Cell::new(0));
    let calls = count.clone();
    let (result, _) = ui
        .edit_local_frame(
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
                    LayoutProducer::new(move |_, _| {
                        calls.set(calls.get() + 1);
                        Ok(())
                    }),
                )
            },
        )
        .unwrap();
    let id = result.result.value;
    ui.bind(color, Property::color(host)).unwrap();
    assert_eq!(ui.layout_producer_stats().plans_skipped, 0);
    ui.transaction(|tx| tx.set(color, 2)).unwrap();
    assert_eq!(ui.layout_producer_stats().plans_skipped, 0);
    assert_eq!(
        ui.published_local_frame()
            .unwrap()
            .resolve_geometry(host)
            .unwrap()
            .fill_color,
        2
    );
    ui.edit(
        |_| Ok(()),
        |(), edit| {
            edit.properties().color(host, 2);
            Ok(())
        },
    )
    .unwrap();
    assert_eq!(
        ui.layout_producer_stats().plans_skipped,
        0,
        "even an explicit equal patch lacks the negative certificate"
    );
    ui.local_frame_in_viewport(constraints(120.0), [180.0; 2])
        .unwrap();
    assert_eq!(ui.layout_producer_stats().plans_skipped, 0);
    ui.local_frame_in_viewport(constraints(120.0), [170.0; 2])
        .unwrap();
    assert_eq!(ui.layout_producer_stats().plans_skipped, 0);
    ui.edit(|_| Ok(()), |(), edit| edit.invalidate_layout_producer(id))
        .unwrap();
    assert_eq!(ui.layout_producer_stats().plans_skipped, 0);
    let before = count.get();
    ui.transaction(|_| Ok(())).unwrap();
    assert_eq!(count.get(), before);
    assert_eq!(ui.layout_producer_stats().plans_skipped, 1);
}

#[test]
fn responsive_writers_settle_before_negative_certificate_and_source_candidate_check() {
    use crate::retained::responsive::*;
    let (mut ui, scope, host) = fixture();
    let style = ui
        .state(scope, 1, || LayoutStyle::column().fixed_inline_size(70.0))
        .unwrap();
    let count = Rc::new(Cell::new(0));
    let calls = count.clone();
    ui.edit_local_frame(
        constraints(180.0),
        [180.0; 2],
        |_| Ok(()),
        |(), edit| {
            edit.layout_environment(&LayoutEnvironment::new([100.0, 180.0]))?;
            edit.responsive_rule(
                scope,
                0,
                style,
                host,
                LayoutQuery::ViewportInline,
                LayoutBreakpointStyles::new(
                    LayoutStyle::column().fixed_inline_size(70.0),
                    vec![(150.0, LayoutStyle::column().fixed_inline_size(130.0))],
                )?,
            )?;
            edit.layout_producer(
                scope,
                0,
                host,
                LayoutBuildKind::InlineAllocation,
                &[],
                LayoutProducer::new(move |input, edit| {
                    calls.set(calls.get() + 1);
                    let LayoutBuildInput::Inline(input) = input else {
                        panic!()
                    };
                    assert_eq!(
                        input.known_width,
                        Some(match edit.read(style)?.size.width {
                            LayoutDimension::Px(px) => px,
                            _ => panic!(),
                        })
                    );
                    Ok(())
                }),
            )?;
            Ok(())
        },
    )
    .unwrap();
    ui.edit(
        |_| Ok(()),
        |(), edit| edit.layout_environment(&LayoutEnvironment::new([120.0, 180.0])),
    )
    .unwrap();
    assert_eq!(
        ui.layout_producer_stats().plans_skipped,
        1,
        "environment metadata with equal selected output"
    );
    ui.edit(
        |_| Ok(()),
        |(), edit| edit.layout_environment(&LayoutEnvironment::new([200.0, 180.0])),
    )
    .unwrap();
    assert_eq!(ui.layout_producer_stats().plans_skipped, 0);
    assert_eq!(count.get(), 2);
    assert_eq!(ui.layout_result(host).unwrap().size[0], 130.0);
}

#[test]
fn retiring_observed_scope_rebuilds_index_and_rejects_live_cross_scope_dependency() {
    let (mut ui, scope, host) = fixture();
    let child = ui.reconcile_keys(scope, &[1]).unwrap().order[0];
    let value = ui.state(child, 0, || 1u32).unwrap();
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
                    edit.read(value)?;
                    Ok(())
                }),
            )
        },
    )
    .unwrap();
    let frame = ui.published_local_frame().unwrap().clone();
    assert!(ui.reconcile_keys(scope, &[]).is_err());
    assert!(frame.same(ui.published_local_frame().unwrap()));
    assert!(ui.producer_index.0.contains_key(&value.dependency()));
    ui.edit(
        |_| Ok(()),
        |(), edit| {
            edit.remove_layout_producer(scope, 0)?;
            edit.reconcile_keys(scope, &[])?;
            Ok(())
        },
    )
    .unwrap();
    assert!(ui.producer_index.0.is_empty());
    let child2 = ui.reconcile_keys(scope, &[1]).unwrap().order[0];
    let value2 = ui.state(child2, 0, || 2u32).unwrap();
    assert_ne!(value.dependency(), value2.dependency());
    assert!(!ui.producer_index.0.contains_key(&value2.dependency()));
}

#[test]
fn convenience_local_frame_drives_producer_resize_and_then_reuses_accepted_plan() {
    let (mut ui, scope, host) = fixture();
    let count = Rc::new(Cell::new(0));
    let calls = count.clone();
    ui.edit_local_frame(
        LayoutConstraints::viewport(100.0, 180.0),
        [100.0, 180.0],
        |_| Ok(()),
        |(), edit| {
            edit.layout_producer(
                scope,
                0,
                host,
                LayoutBuildKind::InlineAllocation,
                &[],
                LayoutProducer::new(move |input, edit| {
                    calls.set(calls.get() + 1);
                    let LayoutBuildInput::Inline(input) = input else {
                        panic!()
                    };
                    let branch = edit.reconcile_keys(scope, &[0])?[0];
                    let nodes = edit.renders(branch)?;
                    let leaf = match nodes.first() {
                        Some(node) => *node,
                        None => edit.append_layout(branch, host, LayoutStyle::row(), 0xff0000ff)?,
                    };
                    edit.properties().layout_style(
                        leaf,
                        LayoutStyle::row().size(
                            LayoutDimension::Px(20.0),
                            LayoutDimension::Px(input.known_width.unwrap() / 10.0),
                        ),
                    );
                    Ok(())
                }),
            )?;
            Ok(())
        },
    )
    .unwrap();
    let old = ui.published_local_frame().unwrap().clone();
    let resized = ui.local_frame([160.0, 180.0]).unwrap();
    assert_eq!(count.get(), 2);
    assert_eq!(ui.layout_result(host).unwrap().size, [160.0, 16.0]);
    assert_eq!(
        old.resolve().items.iter().last().unwrap().bounds.height,
        10.0
    );
    let same = ui.local_frame([160.0, 180.0]).unwrap();
    assert!(resized.same(&same));
    assert_eq!(ui.layout_producer_stats().plans_skipped, 1);
    let model = ui.state(scope, 42, || 0u32).unwrap();
    ui.reactive_mut()
        .transaction(|tx| tx.set(model, 1))
        .unwrap();
    ui.transaction(|_| Ok(())).unwrap();
    assert_eq!(
        count.get(),
        3,
        "internal resolver access invalidates accepted producer state even at equal paint revision"
    );
    assert_eq!(ui.layout_producer_stats().plans_skipped, 0);
}
