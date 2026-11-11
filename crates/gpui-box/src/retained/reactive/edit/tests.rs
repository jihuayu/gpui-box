use super::*;
use std::cell::RefCell;
#[test]
fn sources_derived_new_sinks_and_removed_output_publish_together() {
    let (mut ui, child) = super::super::tests::fixture();
    let source = ui.signal(0x0000ffffu32);
    let derived = ui
        .computed(&[source.dependency()], move |i| Ok(*i.read(source)?))
        .unwrap();
    ui.bind(derived, Property::color(child)).unwrap();
    let log = Rc::new(RefCell::new(vec![]));
    let observed = log.clone();
    ui.effect(&[derived.dependency()], move |i| {
        observed.borrow_mut().push(*i.read(derived).unwrap());
        None
    })
    .unwrap();
    let before = ui.frame([100.0, 100.0]).unwrap();
    let root = ui.root();
    let result = ui
        .edit(
            |tx| {
                tx.set(source, 0x4488ffff)?;
                Ok(())
            },
            |(), edit| {
                assert_eq!(*edit.read(derived)?, 0x4488ffff);
                let node = edit.tree().append(root, Style::default(), 0)?;
                edit.bind(derived, Property::color(node))?;
                edit.tree().remove(child)?;
                Ok(node)
            },
        )
        .unwrap();
    assert_eq!(result.result.commit.revision, before.revision + 1);
    assert_eq!(result.result.changed_sources, 2);
    assert_eq!(result.result.sink_writes, 1);
    assert_eq!(result.result.effect_runs, 1);
    assert_eq!(result.removed, [child]);
    assert_eq!(ui.binding_count(), 1);
    assert_eq!(ui.tree.node(result.result.value).unwrap().color, 0x4488ffff);
    assert_eq!(*log.borrow(), [0x0000ffff, 0x4488ffff]);
    ui.transaction(|tx| {
        tx.set(source, 0xff8800ff)?;
        Ok(())
    })
    .unwrap();
    assert_eq!(ui.tree.node(result.result.value).unwrap().color, 0xff8800ff);
}

#[test]
fn invalid_render_rolls_back_sources_bindings_stats_effects_and_frames() {
    let (mut ui, child) = super::super::tests::fixture();
    let source = ui.signal(0x0000ffffu32);
    ui.bind(source, Property::color(child)).unwrap();
    let before = ui.frame([100.0, 100.0]).unwrap();
    let stats = ui.stats();
    let root = ui.root();
    let mut escaped = None;
    let mut escaped_binding = None;
    let result = ui.edit(
        |tx| {
            tx.set(source, 7)?;
            Ok(())
        },
        |(), edit| {
            let node = edit.tree().append(root, Style::default(), 0)?;
            escaped = Some(node);
            escaped_binding = Some(edit.bind(source, Property::color(node))?);
            edit.tree().properties().opacity(node, f32::NAN);
            edit.tree().remove(child)?;
            Ok(())
        },
    );
    assert!(matches!(result, Err(Error::InvalidOpacity)));
    assert_eq!(*ui.read(source).unwrap(), 0x0000ffff);
    assert_eq!(ui.stats(), stats);
    assert_eq!(ui.binding_count(), 1);
    assert!(ui.tree.node(escaped.unwrap()).is_err());
    assert!(ui.unbind(escaped_binding.unwrap()).is_err());
    assert!(Arc::ptr_eq(
        &ui.frame([100.0, 100.0]).unwrap().items,
        &before.items
    ));
    let result = ui
        .edit(
            |_| Ok(()),
            |(), edit| {
                let node = edit.tree().append(root, Style::default(), 0)?;
                let binding = edit.bind(source, Property::color(node))?;
                Ok((node, binding))
            },
        )
        .unwrap();
    assert_ne!(result.result.value.0, escaped.unwrap());
    assert_ne!(result.result.value.1, escaped_binding.unwrap());
}

#[test]
fn property_only_and_equal_edits_retain_fast_routes_and_effect_commit_boundary() {
    let (mut ui, child) = super::super::tests::fixture();
    let source = ui.signal(0x0000ffffu32);
    ui.bind(source, Property::color(child)).unwrap();
    let runs = Rc::new(Cell::new(0));
    let observed = runs.clone();
    ui.effect(&[source.dependency()], move |_| {
        observed.set(observed.get() + 1);
        None
    })
    .unwrap();
    let before = ui.frame([100.0, 100.0]).unwrap();
    let result = ui
        .edit(
            |tx| {
                tx.set(source, 7)?;
                Ok(())
            },
            |(), _| Ok(()),
        )
        .unwrap();
    assert!(result.result.commit.request_frame);
    let after = ui.frame([100.0, 100.0]).unwrap();
    assert_eq!((after.layout_runs, after.recorded_chunks), (0, 0));
    assert_eq!(after.revision, before.revision + 1);
    assert_eq!(runs.get(), 2);
    let result = ui
        .edit(
            |tx| {
                tx.set(source, 7)?;
                Ok(())
            },
            |(), _| Ok(()),
        )
        .unwrap();
    assert!(!result.result.commit.request_frame);
    assert_eq!(runs.get(), 2);
    assert!(Arc::ptr_eq(
        &ui.frame([100.0, 100.0]).unwrap().items,
        &after.items
    ));
    let root = ui.root();
    let failed = ui.edit(
        |tx| {
            tx.set(source, 8)?;
            Ok(())
        },
        |(), edit| {
            let n = edit.tree().append(root, Style::default(), 0)?;
            edit.tree().properties().opacity(n, f32::NAN);
            Ok(())
        },
    );
    assert!(matches!(failed, Err(Error::InvalidOpacity)));
    assert_eq!(runs.get(), 2);
    assert_eq!(*ui.read(source).unwrap(), 7);
}

#[test]
fn property_child_lists_are_settled_before_skipping_removed_sinks() {
    let (mut ui, child) = super::super::tests::fixture();
    let source = ui.signal(0x0000ffffu32);
    ui.bind(source, Property::color(child)).unwrap();
    let root = ui.root();
    let host = ui.append(root, Style::default(), 0).unwrap();
    let result = ui
        .edit(
            |tx| {
                tx.set(source, 7)?;
                Ok(())
            },
            |(), edit| {
                edit.tree().properties().reorder(root, vec![host]);
                edit.tree().properties().reorder(host, vec![child]);
                edit.tree().remove(host)?;
                Ok(())
            },
        )
        .unwrap();
    assert_eq!(result.result.sink_writes, 0);
    assert_eq!(result.removed.len(), 2);
    assert_eq!(ui.binding_count(), 0);
    assert_eq!(*ui.read(source).unwrap(), 7);
}

#[test]
fn endpoint_replacement_publishes_final_writer_and_failure_keeps_old_subscription() {
    let (mut ui, child) = super::super::tests::fixture();
    let old = ui.signal(0x0000ffffu32);
    let next = ui.signal(0x4488ffffu32);
    let binding = ui.bind(old, Property::color(child)).unwrap().0;
    let before = ui.frame([100.0, 100.0]).unwrap();
    let stats = ui.stats();
    assert!(matches!(
        ui.edit(
            |tx| {
                tx.set(old, 7)?;
                tx.set(next, 8)?;
                Ok(())
            },
            |(), edit| {
                edit.unbind(binding)?;
                edit.bind(next, Property::color(child))?;
                edit.tree().properties().opacity(child, f32::NAN);
                Ok(())
            }
        ),
        Err(Error::InvalidOpacity)
    ));
    assert_eq!(ui.stats(), stats);
    assert_eq!(*ui.read(old).unwrap(), 0x0000ffff);
    let result = ui
        .edit(
            |tx| {
                tx.set(old, 7)?;
                tx.set(next, 8)?;
                Ok(())
            },
            |(), edit| {
                edit.unbind(binding)?;
                let replacement = edit.bind(next, Property::color(child))?;
                edit.tree().properties().color(child, 99);
                Ok(replacement)
            },
        )
        .unwrap();
    assert_eq!(result.result.commit.revision, before.revision + 1);
    assert_eq!(result.result.sink_writes, 1);
    assert_eq!(ui.binding_count(), 1);
    assert_eq!(ui.tree.node(child).unwrap().color, 8);
    assert!(ui.unbind(binding).is_err());
    let equal = ui
        .transaction(|tx| {
            tx.set(old, 9)?;
            Ok(())
        })
        .unwrap();
    assert_eq!(equal.sink_writes, 0);
    assert!(!equal.commit.request_frame);
    ui.transaction(|tx| {
        tx.set(next, 10)?;
        Ok(())
    })
    .unwrap();
    assert_eq!(ui.tree.node(child).unwrap().color, 10);
    let detached = ui
        .edit(
            |tx| {
                tx.set(next, 11)?;
                Ok(())
            },
            |(), edit| {
                edit.unbind(result.result.value)?;
                edit.tree().properties().color(child, 12);
                Ok(())
            },
        )
        .unwrap();
    assert_eq!(detached.result.sink_writes, 0);
    assert_eq!(ui.tree.node(child).unwrap().color, 12);
    assert_eq!(ui.binding_count(), 0);
}

#[test]
fn cancelled_binding_and_equal_rebinding_change_lifetime_without_requesting_a_frame() {
    let (mut ui, child) = super::super::tests::fixture();
    let a = ui.signal(0x0000ffffu32);
    let b = ui.signal(0x0000ffffu32);
    let old = ui.bind(a, Property::color(child)).unwrap().0;
    let before = ui.frame([100.0, 100.0]).unwrap();
    let root = ui.root();
    let result = ui
        .edit(
            |_| Ok(()),
            |(), edit| {
                let node = edit.tree().append(root, Style::default(), 0)?;
                let binding = edit.bind(a, Property::color(node))?;
                edit.unbind(binding)?;
                assert_eq!(edit.unbind(binding), Err(Error::InvalidBinding));
                edit.tree().remove(node)?;
                edit.unbind(old)?;
                assert_eq!(edit.unbind(old), Err(Error::InvalidBinding));
                let replacement = edit.bind(b, Property::color(child))?;
                Ok((binding, replacement))
            },
        )
        .unwrap();
    assert!(!result.result.commit.request_frame);
    assert_eq!(result.result.commit.revision, before.revision);
    assert_eq!(ui.binding_count(), 1);
    assert!(ui.unbind(result.result.value.0).is_err());
    assert!(Arc::ptr_eq(
        &before.items,
        &ui.frame([100.0, 100.0]).unwrap().items
    ));
    ui.transaction(|tx| {
        tx.set(a, 1)?;
        Ok(())
    })
    .unwrap();
    assert_eq!(ui.tree.node(child).unwrap().color, 0x0000ffff);
    ui.transaction(|tx| {
        tx.set(b, 2)?;
        Ok(())
    })
    .unwrap();
    assert_eq!(ui.tree.node(child).unwrap().color, 2);
}

#[test]
fn source_disposal_can_replace_endpoint_and_new_effects_validate_before_observation() {
    let (mut ui, child) = super::super::tests::fixture();
    let old = ui.signal(0x0000ffffu32);
    ui.bind(old, Property::color(child)).unwrap();
    let runs = Rc::new(Cell::new(0));
    let log = runs.clone();
    let result = ui
        .edit(
            |_| Ok(()),
            |(), edit| {
                edit.dispose_signal(old)?;
                let source = edit.signal(0x4488ffffu32)?;
                edit.bind(source, Property::color(child))?;
                edit.effect(&[source.dependency()], move |_| {
                    log.set(log.get() + 1);
                    None
                })?;
                Ok(source)
            },
        )
        .unwrap();
    assert_eq!(runs.get(), 1);
    assert!(ui.read(old).is_err());
    assert_eq!(ui.binding_count(), 1);
    assert_eq!(ui.tree.node(child).unwrap().color, 0x4488ffff);
    ui.transaction(|tx| {
        tx.set(result.result.value, 7)?;
        Ok(())
    })
    .unwrap();
    assert_eq!(runs.get(), 2);
    assert!(matches!(
        ui.edit(
            |_| Ok(()),
            |(), edit| {
                edit.dispose_signal(result.result.value)?;
                Ok(())
            }
        ),
        Err(Error::SignalInUse)
    ));
}

#[test]
fn lifetime_reservation_and_revision_exhaustion_reject_before_callbacks() {
    let (mut ui, child) = super::super::tests::fixture();
    let before = ui.frame([100.0, 100.0]).unwrap();
    ui.next_source_generation = u64::MAX;
    assert!(matches!(
        ui.edit(|_| Ok(()), |(), edit| edit.signal(1u32)),
        Err(Error::RevisionExhausted)
    ));
    assert_eq!(ui.source_count(), 0);
    ui.next_source_generation = 1;
    ui.next_effect = u64::MAX;
    assert!(matches!(
        ui.edit(|_| Ok(()), |(), edit| edit.effect(&[], |_| None)),
        Err(Error::RevisionExhausted)
    ));
    assert_eq!(ui.effect_count(), 0);
    ui.next_effect = 0;
    ui.tree.revision = u64::MAX;
    let runs = Rc::new(Cell::new(0));
    let log = runs.clone();
    assert!(matches!(
        ui.edit(
            |_| Ok(()),
            |(), edit| {
                let signal = edit.signal(7u32)?;
                edit.bind(signal, Property::color(child))?;
                edit.effect(&[signal.dependency()], move |_| {
                    log.set(log.get() + 1);
                    None
                })?;
                Ok(())
            }
        ),
        Err(Error::RevisionExhausted)
    ));
    assert_eq!(runs.get(), 0);
    assert_eq!(ui.source_count(), 0);
    assert_eq!(ui.binding_count(), 0);
    assert_eq!(ui.effect_count(), 0);
    ui.tree.revision = before.revision;
    assert!(Arc::ptr_eq(
        &before.items,
        &ui.frame([100.0, 100.0]).unwrap().items
    ));
}
