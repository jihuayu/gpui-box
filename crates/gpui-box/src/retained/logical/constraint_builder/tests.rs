use super::*;
use std::{cell::RefCell, rc::Rc};

fn size(w: f32, h: f32) -> LayoutStyle {
    LayoutStyle::row()
        .size(LayoutDimension::Px(w), LayoutDimension::Px(h))
        .flex_shrink(0.0)
}
fn boundary() -> LayoutStyle {
    LayoutStyle::constraint_box(ConstraintFit::Tight, ConstraintFit::Tight)
        .container_type(LayoutContainerType::Size)
}
fn fixture() -> (LogicalUi, NodeId) {
    let mut ui = LogicalUi::new_layout(
        LayoutStyle::row().size(LayoutDimension::Percent(1.0), LayoutDimension::Percent(1.0)),
        0,
        BackendCapabilities::default(),
    )
    .unwrap();
    let root = ui.root_render();
    let scope = ui.root_scope();
    let host = ui
        .edit(
            |_| Ok(()),
            |(), edit| {
                edit.append_layout(
                    scope,
                    root,
                    boundary()
                        .flex_grow(1.0)
                        .flex_basis(LayoutDimension::Px(0.0)),
                    0,
                )
            },
        )
        .unwrap()
        .result
        .value;
    (ui, host)
}
fn pixels(frame: FramePacket) -> Vec<u8> {
    let mut job = software::SoftwareJob::new(
        frame,
        software::SoftwareOptions {
            size: [180, 100],
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
fn nested_flex_allocation_builds_keyed_branches_and_retains_shared_state_and_old_frames() {
    let (mut ui, host) = fixture();
    let root = ui.root_scope();
    let shared = ui.state(root, 77, || 0xff0000ffu32).unwrap();
    let log = Rc::new(RefCell::new(vec![]));
    let mut previous = None;
    let mut oldest = None;
    let mut oldest_pixels = None;
    for width in [100.0, 100.0, 160.0, 160.0, 100.0] {
        let log = log.clone();
        let ((scope, branch_state, node, input), frame) = {
            let (commit, frame) = ui
                .edit_frame(
                    LayoutConstraints::viewport(width, 80.0),
                    [180.0, 100.0],
                    |_| Ok(()),
                    |(), edit| {
                        edit.constraint_builder(host, |input, edit| {
                            assert_eq!(
                                input.constraints.bounds,
                                BoxConstraints::tight([width, 80.0])
                            );
                            let key = u64::from(input.constraints.bounds.max.width >= 140.0);
                            let scope = edit.reconcile_keys(root, &[key])?[0];
                            let branch_state = edit.state(scope, 0, || key)?;
                            let renders = edit.renders(scope)?;
                            let node = if let Some(node) = renders.first() {
                                *node
                            } else {
                                let node =
                                    edit.append_layout(scope, host, LayoutStyle::row(), 0)?;
                                edit.append_layout(scope, node, size(25.0, 20.0), 0x00ff00ff)?;
                                let leaf = edit.append_layout(scope, node, size(25.0, 20.0), 0)?;
                                edit.bind(shared, Property::color(leaf))?;
                                if key == 1 {
                                    edit.append_layout(scope, node, size(25.0, 20.0), 0x0000ffff)?;
                                }
                                node
                            };
                            edit.effect(scope, 0, &[], move |_| {
                                log.borrow_mut().push(("run", key));
                                let log = log.clone();
                                Some(Box::new(move || log.borrow_mut().push(("cleanup", key))))
                            })?;
                            Ok((scope, branch_state, node, input))
                        })
                    },
                )
                .unwrap();
            (commit.result.value, frame)
        };
        assert_eq!(ui.layout_result(node).unwrap().size, [width, 80.0]);
        assert_eq!(*ui.read(branch_state).unwrap(), u64::from(width >= 140.0));
        let actual = pixels(frame.clone());
        let mut reference = UiTree::new_layout(size(width, 80.0), 0).unwrap();
        reference.frame_build_policy = FrameBuildPolicy::Full;
        let reference_host = reference
            .append_layout(
                reference.root(),
                boundary()
                    .flex_grow(1.0)
                    .flex_basis(LayoutDimension::Px(0.0)),
                0,
            )
            .unwrap();
        let reference_row = reference
            .append_layout(reference_host, LayoutStyle::row(), 0)
            .unwrap();
        reference
            .append_layout(reference_row, size(25.0, 20.0), 0x00ff00ff)
            .unwrap();
        reference
            .append_layout(reference_row, size(25.0, 20.0), 0xff0000ff)
            .unwrap();
        if width >= 140.0 {
            reference
                .append_layout(reference_row, size(25.0, 20.0), 0x0000ffff)
                .unwrap();
        }
        assert_eq!(
            actual,
            pixels(
                reference
                    .frame_in_viewport(LayoutConstraints::viewport(width, 80.0), [180.0, 100.0])
                    .unwrap()
            )
        );
        assert_eq!(
            &actual[(10 * 180 + 10) * 4..(10 * 180 + 10) * 4 + 4],
            &[0, 255, 0, 255]
        );
        assert_eq!(
            &actual[(10 * 180 + 35) * 4..(10 * 180 + 35) * 4 + 4],
            &[255, 0, 0, 255]
        );
        assert_eq!(
            &actual[(10 * 180 + 60) * 4..(10 * 180 + 60) * 4 + 4],
            if width >= 140.0 {
                &[0, 0, 255, 255]
            } else {
                &[0, 0, 0, 0]
            }
        );
        assert_eq!(
            pixels(
                ui.frame_in_viewport(LayoutConstraints::viewport(width, 80.0), [180.0, 100.0])
                    .unwrap()
            ),
            actual
        );
        if let Some((old_scope, old_state, old_node, old_input)) = previous {
            if old_input == input {
                assert_eq!(
                    (scope, branch_state, node),
                    (old_scope, old_state, old_node)
                );
            } else {
                assert_ne!(scope, old_scope);
                assert_eq!(ui.read(old_state), Err(Error::InvalidSignal));
                assert!(ui.layout_result(old_node).is_err());
            }
        }
        previous = Some((scope, branch_state, node, input));
        if oldest.is_none() {
            oldest = Some(frame);
            oldest_pixels = Some(actual);
        }
    }
    assert_eq!(
        *log.borrow(),
        [
            ("run", 0),
            ("cleanup", 0),
            ("run", 1),
            ("cleanup", 1),
            ("run", 0)
        ]
    );
    assert_eq!(pixels(oldest.unwrap()), oldest_pixels.unwrap());
    assert_eq!(ui.reactive().source_count(), 2);
}

#[test]
fn final_frame_failure_preserves_sources_scopes_effects_ids_and_published_pixels() {
    let (mut ui, host) = fixture();
    let root = ui.root_scope();
    let input = ui.state(root, 0, || 0u32).unwrap();
    let before = ui.frame([100.0, 80.0]).unwrap();
    let stats = ui.stats();
    let mut escaped = None;
    let mut escaped_scope = None;
    let mut escaped_source = None;
    let effects = Rc::new(RefCell::new(0));
    let log = effects.clone();
    let result = ui.edit_frame(
        LayoutConstraints::viewport(100.0, 80.0),
        [100.0, 80.0],
        |tx| {
            tx.set(input, 1)?;
            Ok(())
        },
        |(), edit| {
            let scope = edit.reconcile_keys(root, &[1])?[0];
            escaped_scope = Some(scope);
            let source = edit.state(scope, 0, || 1u32)?;
            escaped_source = Some(source);
            edit.effect(scope, 0, &[], move |_| {
                *log.borrow_mut() += 1;
                None
            })?;
            let node = edit.append_layout(scope, host, size(10.0, 10.0), 0xff0000ff)?;
            escaped = Some(node);
            edit.properties().translation(node, [f32::MAX, f32::MAX]);
            let leaf = edit.append_layout(scope, node, size(5.0, 5.0), 0xff0000ff)?;
            edit.properties().translation(leaf, [f32::MAX, f32::MAX]);
            Ok(())
        },
    );
    assert!(
        matches!(result, Err(Error::InvalidFrame)),
        "{:?}",
        result.err()
    );
    assert_eq!(*ui.read(input).unwrap(), 0);
    assert_eq!(ui.stats().transactions, stats.transactions);
    assert!(ui.children(root).unwrap().is_empty());
    assert_eq!(*effects.borrow(), 0);
    assert_eq!(ui.read(escaped_source.unwrap()), Err(Error::InvalidSignal));
    assert!(ui.layout_result(escaped.unwrap()).is_err());
    assert_eq!(ui.published_frame().unwrap().revision, before.revision);
    assert_eq!(
        pixels(ui.published_frame().unwrap().clone()),
        pixels(before)
    );
    let next = ui
        .edit(
            |_| Ok(()),
            |(), edit| {
                let scope = edit.reconcile_keys(root, &[1])?[0];
                Ok((scope, edit.append_layout(scope, host, size(10.0, 10.0), 0)?))
            },
        )
        .unwrap()
        .result
        .value;
    assert_ne!(next.0, escaped_scope.unwrap());
    assert_ne!(next.1, escaped.unwrap());
}

#[test]
fn changed_parent_after_build_rejects_without_callbacks_or_partial_publication() {
    let (mut ui, host) = fixture();
    let before = ui.frame([100.0, 80.0]).unwrap();
    let result = ui.edit_frame(
        LayoutConstraints::viewport(100.0, 80.0),
        [100.0, 80.0],
        |_| Ok(()),
        |(), edit| {
            edit.constraint_builder(host, |input, _| {
                assert_eq!(input.constraints.bounds.max.width, 100.0);
                Ok(())
            })?;
            edit.properties().layout_style(
                host,
                boundary()
                    .size(LayoutDimension::Px(60.0), LayoutDimension::Px(40.0))
                    .flex_shrink(0.0),
            );
            Ok(())
        },
    );
    assert!(matches!(result, Err(Error::UnsafeConstraintBuilder)));
    assert_eq!(ui.published_frame().unwrap().revision, before.revision);
    assert_eq!(ui.layout_result(host).unwrap().size, [100.0, 80.0]);
}

#[test]
fn nested_builders_read_current_draft_and_preferred_sizes_are_not_constraints() {
    let (mut ui, outer) = fixture();
    let root = ui.root_scope();
    let constraints = LayoutConstraints::viewport(160.0, 80.0);
    let (commit, _) = ui
        .edit_frame(
            constraints,
            [180.0, 100.0],
            |_| Ok(()),
            |(), edit| {
                edit.constraint_builder(outer, |input, edit| {
                    assert_eq!(
                        input.constraints.bounds,
                        BoxConstraints::tight([160.0, 80.0])
                    );
                    let flex = edit.append_layout(root, outer, LayoutStyle::row(), 0)?;
                    edit.append_layout(root, flex, size(30.0, 20.0), 0)?;
                    let inner = edit.append_layout(
                        root,
                        flex,
                        boundary()
                            .flex_grow(1.0)
                            .flex_basis(LayoutDimension::Px(0.0))
                            .padding(LayoutEdges::all(LayoutLength::Px(5.0))),
                        0,
                    )?;
                    edit.constraint_builder(inner, |input, edit| {
                        assert_eq!(
                            input.constraints.bounds,
                            BoxConstraints::tight([120.0, 70.0])
                        );
                        let leaf =
                            edit.append_layout(root, inner, size(900.0, 900.0), 0xff0000ff)?;
                        Ok((inner, leaf))
                    })
                })
            },
        )
        .unwrap();
    let (inner, leaf) = commit.result.value;
    assert_eq!(ui.layout_result(inner).unwrap().size, [130.0, 80.0]);
    assert_eq!(ui.layout_result(leaf).unwrap().size, [120.0, 70.0]);
}

#[test]
fn source_style_changes_settle_before_builder_and_unsafe_or_hidden_hosts_reject() {
    let (mut ui, host) = fixture();
    let root = ui.root_scope();
    let style = ui.state(root, 0, boundary).unwrap();
    ui.bind(style, Property::layout_style(host)).unwrap();
    let constraints = LayoutConstraints::viewport(160.0, 80.0);
    let changed = boundary()
        .size(LayoutDimension::Px(70.0), LayoutDimension::Px(40.0))
        .flex_shrink(0.0);
    ui.edit_frame(
        constraints,
        [180.0, 100.0],
        |tx| {
            tx.set(style, changed)?;
            Ok(())
        },
        |(), edit| {
            edit.constraint_builder(host, |input, _| {
                assert_eq!(
                    input.constraints.bounds,
                    BoxConstraints::tight([70.0, 40.0])
                );
                Ok(())
            })
        },
    )
    .unwrap();
    let before = ui.published_frame().unwrap().revision;
    for unsafe_style in [
        LayoutStyle::row(),
        LayoutStyle::constraint_box(ConstraintFit::Loose, ConstraintFit::Loose),
        boundary().display(LayoutDisplay::None),
    ] {
        let result = ui.edit_frame(
            constraints,
            [180.0, 100.0],
            |tx| {
                tx.set(style, unsafe_style)?;
                Ok(())
            },
            |(), edit| {
                edit.constraint_builder(host, |_, _| -> Result<(), Error> {
                    panic!("invalid host must not build")
                })
            },
        );
        assert!(matches!(result, Err(Error::UnsafeConstraintBuilder)));
        assert_eq!(ui.published_frame().unwrap().revision, before);
    }
    assert!(matches!(
        ui.edit(
            |_| Ok(()),
            |(), edit| edit.constraint_builder(host, |_, _| Ok(()))
        ),
        Err(Error::UnsafeConstraintBuilder)
    ));
}

#[test]
fn loose_constraints_do_not_become_child_size_and_unbounded_is_not_intrinsic() {
    for available in [
        layout::AvailableSpace::Unbounded,
        layout::AvailableSpace::MaxContent,
        layout::AvailableSpace::MinContent,
    ] {
        let style = LayoutStyle::constraint_box(ConstraintFit::Loose, ConstraintFit::Loose)
            .container_type(LayoutContainerType::Size)
            .size(LayoutDimension::Px(100.0), LayoutDimension::Px(60.0));
        let mut ui = LogicalUi::new_layout(style, 0, BackendCapabilities::default()).unwrap();
        let root = ui.root_scope();
        let host = ui.root_render();
        let (commit, _) = ui
            .edit_frame(
                LayoutConstraints::new(available, available),
                [180.0, 100.0],
                |_| Ok(()),
                |(), edit| {
                    edit.constraint_builder(host, |input, edit| {
                        assert_eq!(
                            input.constraints.bounds,
                            BoxConstraints::loose([100.0, 60.0])
                        );
                        edit.append_layout(root, host, size(20.0, 15.0), 0xff0000ff)
                    })
                },
            )
            .unwrap();
        assert_eq!(
            ui.layout_result(commit.result.value).unwrap().size,
            [20.0, 15.0]
        );
    }
}

#[test]
fn current_sources_and_protocol_switch_validate_as_one_preview_and_final_state() {
    let mut ui =
        LogicalUi::new_layout(size(100.0, 60.0), 0, BackendCapabilities::default()).unwrap();
    let root = ui.root_scope();
    let host = ui.root_render();
    let old = ui.reconcile_keys(root, &[1]).unwrap().order[0];
    ui.append_render_layout(old, host, size(10.0, 10.0), 0)
        .unwrap();
    ui.append_render_layout(old, host, size(10.0, 10.0), 0)
        .unwrap();
    let source = ui.state(root, 0, || size(100.0, 60.0)).unwrap();
    ui.bind(source, Property::layout_style(host)).unwrap();
    let (_, frame) = ui
        .edit_frame(
            LayoutConstraints::viewport(100.0, 60.0),
            [180.0, 100.0],
            |tx| {
                tx.set(
                    source,
                    boundary().size(LayoutDimension::Px(100.0), LayoutDimension::Px(60.0)),
                )?;
                Ok(())
            },
            |(), edit| {
                let branch = edit.reconcile_keys(root, &[2])?[0];
                edit.constraint_builder(host, |input, edit| {
                    assert_eq!(
                        input.constraints.bounds,
                        BoxConstraints::tight([100.0, 60.0])
                    );
                    edit.append_layout(branch, host, size(20.0, 20.0), 0xff0000ff)
                })
            },
        )
        .unwrap();
    assert_eq!(frame.items.len(), 2);
    assert!(ui.renders(old).is_err());
}

#[test]
fn final_revalidation_rejects_invalid_branch_and_layout_overflow_without_running_effects() {
    let (mut ui, host) = fixture();
    let before = ui.frame([100.0, 80.0]).unwrap();
    let root = ui.root_scope();
    let events = Rc::new(RefCell::new(0));
    let log = events.clone();
    let result = ui.edit_frame(
        LayoutConstraints::viewport(100.0, 80.0),
        [180.0, 100.0],
        |_| Ok(()),
        |(), edit| {
            edit.constraint_builder(host, |_, edit| {
                let branch = edit.reconcile_keys(root, &[1])?[0];
                edit.effect(branch, 0, &[], move |_| {
                    *log.borrow_mut() += 1;
                    None
                })?;
                edit.append_layout(branch, host, size(10.0, 10.0), 0)?;
                edit.append_layout(branch, host, size(10.0, 10.0), 0)?;
                Ok(())
            })
        },
    );
    assert!(matches!(result, Err(Error::InvalidConstraintBox)));
    assert_eq!(*events.borrow(), 0);
    assert!(ui.children(root).unwrap().is_empty());
    assert_eq!(ui.published_frame().unwrap().revision, before.revision);
}

#[test]
fn pure_builder_runs_once_even_with_intrinsic_flex_probes_and_cache_hits() {
    let (mut ui, host) = fixture();
    let root = ui.root_scope();
    let mut calls = 0;
    for _ in 0..3 {
        ui.edit_frame(
            LayoutConstraints::viewport(100.0, 80.0),
            [180.0, 100.0],
            |_| Ok(()),
            |(), edit| {
                edit.constraint_builder(host, |_, edit| {
                    calls += 1;
                    let branch = edit.reconcile_keys(root, &[1])?[0];
                    if edit.renders(branch)?.is_empty() {
                        edit.append_layout(branch, host, size(20.0, 20.0), 0xff0000ff)?;
                    }
                    Ok(())
                })
            },
        )
        .unwrap();
    }
    assert_eq!(
        calls, 3,
        "one invocation per explicit owner plan; no layout callbacks"
    );
}

#[test]
fn selected_inline_size_and_indefinite_block_mode_have_distinct_percentage_basis() {
    for available in [
        layout::AvailableSpace::Unbounded,
        layout::AvailableSpace::MaxContent,
        layout::AvailableSpace::MinContent,
    ] {
        let mut ui = LogicalUi::new_layout(boundary(), 0, BackendCapabilities::default()).unwrap();
        let host = ui.root_render();
        let (_, frame) = ui
            .edit_frame(
                LayoutConstraints::new(available, available),
                [180.0, 100.0],
                |_| Ok(()),
                |(), edit| {
                    edit.constraint_builder(host, |input, _| {
                        assert_eq!(
                            input.constraints.width,
                            layout::AvailableSpace::Definite(0.0)
                        );
                        assert_eq!(input.constraints.height, available);
                        assert_eq!(
                            input.constraints.bounds,
                            BoxConstraints::new([0.0, 0.0], [0.0, f32::INFINITY])
                        );
                        assert_eq!(input.percentage_basis.width, Some(0.0));
                        assert_eq!(input.percentage_basis.height, None);
                        Ok(())
                    })
                },
            )
            .unwrap();
        assert_eq!(frame.items.len(), 1);
    }
}

#[test]
fn failed_branch_replacement_keeps_old_state_binding_and_effect_alive_until_success() {
    let (mut ui, host) = fixture();
    let root = ui.root_scope();
    let old = ui.reconcile_keys(root, &[0]).unwrap().order[0];
    let state = ui.state(old, 0, || 0xff0000ffu32).unwrap();
    let node = ui
        .append_render_layout(old, host, size(20.0, 20.0), 0)
        .unwrap();
    ui.bind(state, Property::color(node)).unwrap();
    let events = Rc::new(RefCell::new(vec![]));
    let log = events.clone();
    ui.effect(old, 0, &[state.dependency()], move |_| {
        log.borrow_mut().push("old run");
        let log = log.clone();
        Some(Box::new(move || log.borrow_mut().push("old cleanup")))
    })
    .unwrap();
    let before = ui.frame([100.0, 80.0]).unwrap();
    let log = events.clone();
    let failure = ui.edit_frame(
        LayoutConstraints::viewport(100.0, 80.0),
        [100.0, 80.0],
        |_| Ok(()),
        |(), edit| {
            edit.constraint_builder(host, |_, edit| {
                let new = edit.reconcile_keys(root, &[1])?[0];
                edit.effect(new, 0, &[], move |_| {
                    log.borrow_mut().push("new run");
                    None
                })?;
                let node = edit.append_layout(new, host, size(20.0, 20.0), 0)?;
                edit.properties().translation(node, [f32::MAX; 2]);
                let child = edit.append_layout(new, node, size(10.0, 10.0), 0)?;
                edit.properties().translation(child, [f32::MAX; 2]);
                Ok(())
            })
        },
    );
    assert!(matches!(failure, Err(Error::InvalidFrame)));
    assert_eq!(*events.borrow(), ["old run"]);
    assert_eq!(ui.children(root).unwrap(), [old]);
    assert_eq!(ui.renders(old).unwrap(), [node]);
    assert_eq!(ui.reactive().binding_count(), 1);
    assert_eq!(ui.published_frame().unwrap().revision, before.revision);
    ui.transaction(|tx| tx.set(state, 0x00ff00ff)).unwrap();
    assert_eq!(*events.borrow(), ["old run", "old cleanup", "old run"]);
    let log = events.clone();
    ui.edit_frame(
        LayoutConstraints::viewport(100.0, 80.0),
        [100.0, 80.0],
        |_| Ok(()),
        |(), edit| {
            edit.constraint_builder(host, |_, edit| {
                let new = edit.reconcile_keys(root, &[1])?[0];
                edit.effect(new, 0, &[], move |_| {
                    log.borrow_mut().push("new run");
                    None
                })?;
                edit.append_layout(new, host, size(20.0, 20.0), 0)?;
                Ok(())
            })
        },
    )
    .unwrap();
    assert_eq!(
        *events.borrow(),
        [
            "old run",
            "old cleanup",
            "old run",
            "old cleanup",
            "new run"
        ]
    );
    assert_eq!(ui.read(state), Err(Error::InvalidSignal));
    assert_eq!(ui.reactive().binding_count(), 0);
}

#[test]
fn draft_render_order_survives_reverse_free_slot_reuse_and_next_keyed_build() {
    let (mut ui, host) = fixture();
    let root = ui.root_scope();
    let old = ui.reconcile_keys(root, &[0]).unwrap().order[0];
    let old_row = ui
        .append_render_layout(old, host, LayoutStyle::row(), 0)
        .unwrap();
    ui.append_render_layout(old, old_row, size(10.0, 10.0), 0)
        .unwrap();
    ui.append_render_layout(old, old_row, size(10.0, 10.0), 0)
        .unwrap();
    // Retire the child slot before constructing the replacement; the new row
    // and its descendants may occupy free arena slots in reverse slot order.
    ui.reconcile_keys(root, &[]).unwrap();
    let constraints = LayoutConstraints::viewport(100.0, 80.0);
    let (commit, _) = ui
        .edit_frame(
            constraints,
            [100.0, 80.0],
            |_| Ok(()),
            |(), edit| {
                edit.constraint_builder(host, |_, edit| {
                    let scope = edit.reconcile_keys(root, &[1])?[0];
                    let row = edit.append_layout(scope, host, LayoutStyle::row(), 0)?;
                    let a = edit.append_layout(scope, row, size(10.0, 10.0), 0)?;
                    let b = edit.append_layout(scope, row, size(10.0, 10.0), 0)?;
                    assert_eq!(edit.renders(scope)?, [row, a, b]);
                    Ok((scope, row, a, b))
                })
            },
        )
        .unwrap();
    let (scope, row, a, b) = commit.result.value;
    assert_eq!(ui.renders(scope).unwrap(), [row, a, b]);
    ui.edit_frame(
        constraints,
        [100.0, 80.0],
        |_| Ok(()),
        |(), edit| {
            edit.constraint_builder(host, |_, edit| {
                assert_eq!(edit.reconcile_keys(root, &[1])?, [scope]);
                assert_eq!(edit.renders(scope)?, [row, a, b]);
                Ok(())
            })
        },
    )
    .unwrap();
}
