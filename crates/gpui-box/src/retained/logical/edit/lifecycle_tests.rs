use super::*;
use std::{cell::RefCell, rc::Rc, sync::Arc};
fn ui() -> LogicalUi {
    LogicalUi::new(
        Style::default(),
        0,
        BackendCapabilities {
            solid_color_patch: true,
        },
    )
    .unwrap()
}

#[test]
fn keyed_build_observes_final_sources_and_retirement_cleans_all_dependencies_once() {
    let mut ui = ui();
    let root = ui.root_scope();
    let render = ui.root_render();
    let input = ui.state(root, 0, || 2u32).unwrap();
    let before = ui.frame([100.0, 100.0]).unwrap();
    let log = Rc::new(RefCell::new(vec![]));
    let observed = log.clone();
    let result = ui
        .edit(
            |tx| {
                tx.set(input, 3)?;
                Ok(())
            },
            |(), draft| {
                let scopes = draft.reconcile_keys(root, &[1, 2])?;
                let color = draft.state(scopes[0], 0, || 0x4488ffffu32)?;
                let derived = draft.computed(
                    scopes[1],
                    0,
                    &[input.dependency(), color.dependency()],
                    move |i| Ok(*i.read(color)? + *i.read(input)?),
                )?;
                assert_eq!(*draft.read(derived)?, 0x44890002);
                draft.effect(scopes[1], 0, &[derived.dependency()], move |i| {
                    let value = *i.read(derived).unwrap();
                    observed.borrow_mut().push(("run", value));
                    let log = observed.clone();
                    Some(Box::new(move || log.borrow_mut().push(("cleanup", value))))
                })?;
                let host = draft.append(scopes[0], render, Style::default(), 0)?;
                let node = draft.append(scopes[1], host, Style::default(), 0)?;
                draft.bind(derived, Property::color(node))?;
                Ok((scopes, color, derived, node))
            },
        )
        .unwrap();
    let (scopes, color, derived, node) = result.result.value;
    assert_eq!(result.result.commit.revision, before.revision + 1);
    assert_eq!(result.result.effect_runs, 1);
    assert_eq!(*log.borrow(), [("run", 0x44890002)]);
    assert_eq!(ui.children(root).unwrap(), scopes);
    assert_eq!(ui.renders(scopes[1]).unwrap(), [node]);
    assert_eq!(ui.read(derived).unwrap(), &0x44890002);
    ui.transaction(|tx| {
        tx.set(input, 4)?;
        Ok(())
    })
    .unwrap();
    assert_eq!(ui.reactive().tree().node(node).unwrap().color, 0x44890003);
    let before = ui.frame([100.0, 100.0]).unwrap();
    let result = ui
        .edit(
            |tx| {
                tx.set(input, 5)?;
                Ok(())
            },
            |(), draft| {
                draft.reconcile_keys(root, &[3])?;
                Ok(())
            },
        )
        .unwrap();
    assert_eq!(result.result.commit.revision, before.revision + 1);
    assert_eq!(
        (result.result.effect_runs, result.result.effect_cleanups),
        (0, 1)
    );
    assert_eq!(ui.reactive().source_count(), 1);
    assert_eq!(ui.reactive().binding_count(), 0);
    assert_eq!(ui.reactive().effect_count(), 0);
    assert!(ui.read(color).is_err() && ui.read(derived).is_err());
    assert!(ui.renders(scopes[0]).is_err() && ui.renders(scopes[1]).is_err());
    assert_eq!(log.borrow().len(), 4);
}

#[test]
fn failed_keyed_build_leaves_no_state_effect_or_binding_and_never_revives_handles() {
    let mut ui = ui();
    let root = ui.root_scope();
    let render = ui.root_render();
    let input = ui.state(root, 0, || 1u32).unwrap();
    let before = ui.frame([100.0, 100.0]).unwrap();
    let stats = ui.stats();
    let runs = Rc::new(std::cell::Cell::new(0));
    let run = runs.clone();
    let mut leaked = None;
    let failure = ui.edit(
        |tx| {
            tx.set(input, 2)?;
            Ok(())
        },
        |(), draft| {
            let scope = draft.reconcile_keys(root, &[1])?[0];
            let source = draft.state(scope, 0, || 7u32)?;
            let effect = draft.effect(scope, 0, &[source.dependency()], move |_| {
                run.set(run.get() + 1);
                None
            })?;
            let node = draft.append(scope, render, Style::default(), 0)?;
            let binding = draft.bind(source, Property::color(node))?;
            leaked = Some((scope, source, effect, node, binding));
            draft.properties().opacity(node, f32::NAN);
            Ok(())
        },
    );
    assert!(matches!(failure, Err(Error::InvalidOpacity)));
    let (scope, source, effect, node, binding) = leaked.unwrap();
    assert_eq!(runs.get(), 0);
    assert_eq!(ui.stats(), stats);
    assert_eq!(*ui.read(input).unwrap(), 1);
    assert_eq!(ui.reactive().source_count(), 1);
    assert_eq!(ui.reactive().binding_count(), 0);
    assert_eq!(ui.reactive().effect_count(), 0);
    assert!(
        ui.renders(scope).is_err()
            && ui.read(source).is_err()
            && ui.reactive().tree().node(node).is_err()
    );
    assert!(Arc::ptr_eq(
        &before.items,
        &ui.frame([100.0, 100.0]).unwrap().items
    ));
    let next = ui.reconcile_keys(root, &[1]).unwrap().order[0];
    assert_eq!(scope.slot, next.slot);
    assert_ne!(scope, next);
    let signal = ui.state(next, 0, || 7u32).unwrap();
    assert_ne!(signal, source);
    assert!(ui.ui.unbind(binding).is_err());
    assert!(ui.ui.dispose_effect(effect).is_err());
}

#[test]
fn cancelled_lifetimes_remain_idle_and_new_callbacks_never_run() {
    let mut ui = ui();
    let root = ui.root_scope();
    let render = ui.root_render();
    let before = ui.frame([100.0, 100.0]).unwrap();
    let runs = Rc::new(std::cell::Cell::new(0));
    let log = runs.clone();
    let result = ui
        .edit(
            |_| Ok(()),
            |(), draft| {
                let scope = draft.reconcile_keys(root, &[1])?[0];
                let source = draft.state(scope, 0, || 1u32)?;
                draft.effect(scope, 0, &[source.dependency()], move |_| {
                    log.set(log.get() + 1);
                    None
                })?;
                let node = draft.append(scope, render, Style::default(), 0)?;
                draft.bind(source, Property::color(node))?;
                draft.reconcile_keys(root, &[])?;
                Ok((scope, source, node))
            },
        )
        .unwrap();
    assert!(!result.result.commit.request_frame);
    assert_eq!(runs.get(), 0);
    assert_eq!(ui.reactive().source_count(), 0);
    assert_eq!(ui.reactive().binding_count(), 0);
    assert_eq!(ui.reactive().effect_count(), 0);
    assert!(ui.children(root).unwrap().is_empty());
    assert!(Arc::ptr_eq(
        &before.items,
        &ui.frame([100.0, 100.0]).unwrap().items
    ));
    assert!(ui.read(result.result.value.1).is_err());
}

#[test]
fn surviving_computations_and_portals_block_disposal_before_callbacks_or_source_changes() {
    let mut ui = ui();
    let root = ui.root_scope();
    let render = ui.root_render();
    let scopes = ui.reconcile_keys(root, &[1, 2]).unwrap().order;
    let source = ui.state(scopes[0], 0, || 1u32).unwrap();
    let derived = ui
        .computed(scopes[1], 0, &[source.dependency()], move |i| {
            Ok(*i.read(source)?)
        })
        .unwrap();
    let host = ui
        .append_render(scopes[0], render, Style::default(), 0)
        .unwrap();
    let portal = ui
        .append_render(scopes[1], host, Style::default(), 0)
        .unwrap();
    let before = ui.frame([100.0, 100.0]).unwrap();
    assert!(matches!(
        ui.edit(
            |tx| {
                tx.set(source, 2)?;
                Ok(())
            },
            |(), draft| {
                draft.reparent(portal, render)?;
                draft.reconcile_keys(root, &[2])?;
                Ok(())
            }
        ),
        Err(Error::SignalInUse)
    ));
    assert_eq!(*ui.read(source).unwrap(), 1);
    assert_eq!(*ui.read(derived).unwrap(), 1);
    assert_eq!(
        ui.reactive().tree().node(portal).unwrap().parent,
        Some(host)
    );
    assert_eq!(ui.frame([100.0, 100.0]).unwrap().revision, before.revision);
    // Removing the dependent too permits the combined dependency retirement.
    ui.edit(
        |_| Ok(()),
        |(), draft| {
            draft.reconcile_keys(root, &[])?;
            Ok(())
        },
    )
    .unwrap();
    assert!(ui.read(source).is_err() && ui.read(derived).is_err());
}

#[test]
fn mixed_key_order_keeps_state_and_chunks_and_sorts_portal_siblings_once() {
    let mut ui = ui();
    let root = ui.root_scope();
    let render = ui.root_render();
    let scopes = ui.reconcile_keys(root, &[1, 2]).unwrap().order;
    let a = ui
        .append_render(scopes[0], render, Style::default(), 1)
        .unwrap();
    let b = ui
        .append_render(scopes[1], render, Style::default(), 2)
        .unwrap();
    let state = ui.state(scopes[0], 0, || 3u32).unwrap();
    let before = ui.frame([100.0, 100.0]).unwrap();
    let result = ui
        .edit(
            |_| Ok(()),
            |(), draft| {
                let order = draft.reconcile_keys(root, &[2, 3, 1])?;
                assert_eq!(
                    draft.state(order[2], 0, || panic!("retained state must not initialize"))?,
                    state
                );
                let child = draft.append(order[1], render, Style::default(), 4)?;
                Ok((order, child))
            },
        )
        .unwrap();
    let frame = ui.frame([100.0, 100.0]).unwrap();
    assert_eq!(frame.revision, before.revision + 1);
    assert_eq!(frame.recorded_chunks, 1);
    assert_eq!(
        frame.items.iter().map(|i| i.node).collect::<Vec<_>>(),
        [render, b, result.result.value.1, a]
    );
    assert_eq!(ui.children(root).unwrap(), result.result.value.0);
    assert!(Arc::ptr_eq(
        &frame.items.iter().find(|i| i.node == a).unwrap().commands,
        &before.items.iter().find(|i| i.node == a).unwrap().commands
    ));
}

#[test]
fn surviving_portal_requires_evacuation_and_new_dependencies_cannot_target_retired_state() {
    let mut ui = ui();
    let root = ui.root_scope();
    let render = ui.root_render();
    let scopes = ui.reconcile_keys(root, &[1, 2]).unwrap().order;
    let host = ui
        .append_render(scopes[0], render, Style::default(), 1)
        .unwrap();
    let portal = ui
        .append_render(scopes[1], host, Style::default(), 2)
        .unwrap();
    let state = ui.state(scopes[0], 0, || 3u32).unwrap();
    let before = ui.frame([100.0, 100.0]).unwrap();
    assert!(matches!(
        ui.edit(
            |_| Ok(()),
            |(), draft| {
                draft.reconcile_keys(root, &[2])?;
                Ok(())
            }
        ),
        Err(Error::ScopeInUse)
    ));
    assert!(matches!(
        ui.edit(
            |_| Ok(()),
            |(), draft| {
                draft.reparent(portal, render)?;
                draft.computed(scopes[1], 0, &[state.dependency()], move |i| {
                    Ok(*i.read(state)?)
                })?;
                draft.reconcile_keys(root, &[2])?;
                Ok(())
            }
        ),
        Err(Error::SignalInUse)
    ));
    assert_eq!(ui.frame([100.0, 100.0]).unwrap().revision, before.revision);
    ui.edit(
        |_| Ok(()),
        |(), draft| {
            draft.reparent(portal, render)?;
            draft.reconcile_keys(root, &[2])?;
            Ok(())
        },
    )
    .unwrap();
    assert_eq!(
        ui.reactive().tree().node(portal).unwrap().parent,
        Some(render)
    );
    assert!(ui.read(state).is_err());
}

#[test]
fn scope_generation_exhaustion_preserves_children_and_frame() {
    let mut ui = ui();
    let root = ui.root_scope();
    let before = ui.frame([100.0, 100.0]).unwrap();
    ui.next_scope_generation = u64::MAX;
    assert!(matches!(
        ui.edit(
            |_| Ok(()),
            |(), draft| {
                draft.reconcile_keys(root, &[1])?;
                Ok(())
            }
        ),
        Err(Error::RevisionExhausted)
    ));
    assert!(ui.children(root).unwrap().is_empty());
    assert!(Arc::ptr_eq(
        &before.items,
        &ui.frame([100.0, 100.0]).unwrap().items
    ));
}

#[test]
fn background_sender_remains_live_after_failed_disposal_and_expires_after_commit() {
    let mut ui = ui();
    let root = ui.root_scope();
    let render = ui.root_render();
    let scope = ui.reconcile_keys(root, &[1]).unwrap().order[0];
    let source = ui.state(scope, 0, || 0x4488ffffu32).unwrap();
    let node = ui
        .append_render(scope, render, Style::default(), 0)
        .unwrap();
    ui.bind(source, Property::color(node)).unwrap();
    let mut receiver = ui.open_updates(2).unwrap();
    let sender = ui.update_sender(&receiver, source).unwrap();
    sender.send(0x22cc88ff).unwrap();
    assert!(
        ui.edit(
            |_| Ok(()),
            |(), draft| {
                let scope = draft.reconcile_keys(root, &[2])?[0];
                let node = draft.append(scope, render, Style::default(), 0)?;
                draft.properties().opacity(node, f32::NAN);
                Ok(())
            }
        )
        .is_err()
    );
    ui.apply_updates(&mut receiver).unwrap();
    assert_eq!(*ui.read(source).unwrap(), 0x22cc88ff);
    sender.send(0xff8844ff).unwrap();
    ui.edit(
        |_| Ok(()),
        |(), draft| {
            draft.reconcile_keys(root, &[])?;
            Ok(())
        },
    )
    .unwrap();
    assert_eq!(sender.send(7), Err(Error::InvalidSignal));
    let batch = ui.apply_updates(&mut receiver).unwrap();
    assert_eq!((batch.applied, batch.retired), (0, 1));
    let replacement = ui.state(root, 0, || 7u32).unwrap();
    assert_ne!(replacement, source);
    assert_eq!(*ui.read(replacement).unwrap(), 7);
}

#[test]
fn zero_output_lifetimes_commit_without_render_work_and_reuse_initialization_once() {
    let mut ui = ui();
    let root = ui.root_scope();
    let before = ui.frame([100.0, 100.0]).unwrap();
    let runs = Rc::new(std::cell::Cell::new(0));
    let log = runs.clone();
    let result = ui
        .edit(
            |_| Ok(()),
            |(), draft| {
                let scope = draft.reconcile_keys(root, &[1])?[0];
                let source = draft.state(scope, 0, || 3u32)?;
                draft.effect(scope, 0, &[source.dependency()], move |_| {
                    log.set(log.get() + 1);
                    None
                })?;
                Ok((scope, source))
            },
        )
        .unwrap();
    assert!(!result.result.commit.request_frame);
    assert_eq!(runs.get(), 1);
    let (scope, source) = result.result.value;
    let result = ui
        .edit(
            |_| Ok(()),
            |(), draft| {
                assert_eq!(draft.reconcile_keys(root, &[1])?, [scope]);
                assert_eq!(draft.state(scope, 0, || panic!("state retained"))?, source);
                draft.effect(scope, 0, &[source.dependency()], |_| {
                    panic!("effect retained")
                })?;
                Ok(())
            },
        )
        .unwrap();
    assert!(!result.result.commit.request_frame);
    assert_eq!(runs.get(), 1);
    assert!(Arc::ptr_eq(
        &before.items,
        &ui.frame([100.0, 100.0]).unwrap().items
    ));
    ui.edit(
        |_| Ok(()),
        |(), draft| {
            draft.reconcile_keys(root, &[])?;
            Ok(())
        },
    )
    .unwrap();
    assert_eq!(ui.reactive().source_count(), 0);
    assert_eq!(ui.reactive().effect_count(), 0);
}

#[test]
fn postcommit_callback_panics_leave_logical_source_and_render_ownership_consistent() {
    let mut ui = ui();
    let root = ui.root_scope();
    let render = ui.root_render();
    let mut scope = None;
    let mut source = None;
    let mut node = None;
    let panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        ui.edit(
            |_| Ok(()),
            |(), draft| {
                let id = draft.reconcile_keys(root, &[1])?[0];
                scope = Some(id);
                let state = draft.state(id, 0, || 0x4488ffffu32)?;
                source = Some(state);
                let output = draft.append(id, render, Style::default(), 0)?;
                node = Some(output);
                draft.bind(state, Property::color(output))?;
                draft.effect(id, 0, &[state.dependency()], |_| {
                    panic!("postcommit observer")
                })?;
                Ok(())
            },
        )
        .unwrap();
    }));
    assert!(panic.is_err());
    assert_eq!(ui.children(root).unwrap(), [scope.unwrap()]);
    assert_eq!(ui.renders(scope.unwrap()).unwrap(), [node.unwrap()]);
    assert_eq!(*ui.read(source.unwrap()).unwrap(), 0x4488ffff);
    assert_eq!(ui.reactive().binding_count(), 1);
    assert_eq!(
        ui.reactive().tree().node(node.unwrap()).unwrap().color,
        0x4488ffff
    );
    ui.edit(
        |_| Ok(()),
        |(), draft| {
            draft.reconcile_keys(root, &[])?;
            Ok(())
        },
    )
    .unwrap();
    assert!(ui.renders(scope.unwrap()).is_err());
    assert!(ui.read(source.unwrap()).is_err());
    // Cleanup panics also occur only after all owning maps and edges retire.
    ui.edit(
        |_| Ok(()),
        |(), draft| {
            let scope = draft.reconcile_keys(root, &[2])?[0];
            let source = draft.state(scope, 0, || 1u32)?;
            draft.effect(scope, 0, &[source.dependency()], |_| {
                Some(Box::new(|| panic!("postcommit cleanup")))
            })?;
            Ok(())
        },
    )
    .unwrap();
    assert!(
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            ui.edit(
                |_| Ok(()),
                |(), draft| {
                    draft.reconcile_keys(root, &[])?;
                    Ok(())
                },
            )
            .unwrap();
        }))
        .is_err()
    );
    assert!(ui.children(root).unwrap().is_empty());
    assert_eq!(ui.reactive().source_count(), 0);
    assert_eq!(ui.reactive().effect_count(), 0);
}
