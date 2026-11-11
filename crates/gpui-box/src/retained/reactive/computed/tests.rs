use super::*;
use std::cell::RefCell;

#[test]
fn diamond_batches_are_ordered_once_and_equal_outputs_cut_off() {
    let (mut ui, child) = super::super::tests::fixture();
    let log = Rc::new(RefCell::new(vec![]));
    let a = ui.signal(2u32);
    let left_log = log.clone();
    let left = ui
        .computed(&[a.dependency()], move |inputs| {
            left_log.borrow_mut().push("left");
            Ok(*inputs.read(a)? / 2)
        })
        .unwrap();
    let right_log = log.clone();
    let right = ui
        .computed(&[a.dependency()], move |inputs| {
            right_log.borrow_mut().push("right");
            Ok(*inputs.read(a)? * 2)
        })
        .unwrap();
    let join_log = log.clone();
    let join = ui
        .computed(&[left.dependency(), right.dependency()], move |inputs| {
            join_log.borrow_mut().push("join");
            Ok((*inputs.read(left)? + *inputs.read(right)?) << 8 | 255)
        })
        .unwrap();
    ui.bind(join, Property::color(child)).unwrap();
    let initial = ui.frame([100.0; 2]).unwrap();
    log.borrow_mut().clear();
    let commit = ui
        .transaction(|tx| {
            tx.set(a, 8)?;
            tx.transaction(|tx| {
                tx.set(a, 6)?;
                Ok(())
            })?;
            Ok(())
        })
        .unwrap();
    assert_eq!(*log.borrow(), ["left", "right", "join"]);
    assert_eq!(
        (
            commit.computed_evaluations,
            commit.changed_sources,
            commit.sink_writes
        ),
        (3, 4, 1)
    );
    assert_eq!(commit.commit.revision, initial.revision + 1);
    log.borrow_mut().clear();
    let commit = ui
        .transaction(|tx| {
            tx.set(a, 7)?;
            Ok(())
        })
        .unwrap();
    assert_eq!(*log.borrow(), ["left", "right", "join"]);
    assert_eq!(commit.computed_equal, 1);
    // A second branch with no other dirty input must not run past equality.
    let leaf_log = log.clone();
    ui.computed(&[left.dependency()], move |inputs| {
        leaf_log.borrow_mut().push("leaf");
        Ok(*inputs.read(left)?)
    })
    .unwrap();
    log.borrow_mut().clear();
    let commit = ui
        .transaction(|tx| {
            tx.set(a, 6)?;
            Ok(())
        })
        .unwrap();
    assert_eq!(commit.computed_evaluations, 3);
    assert_eq!(*log.borrow(), ["left", "right", "join"]);
    log.borrow_mut().clear();
    let commit = ui
        .transaction(|tx| {
            tx.set(a, 99)?;
            tx.set(a, 6)?;
            Ok(())
        })
        .unwrap();
    assert_eq!(commit.computed_evaluations, 0);
    assert!(log.borrow().is_empty());
    assert!(!commit.commit.request_frame);
}

#[test]
fn reads_settle_current_values_savepoints_and_return_to_committed_value() {
    let (mut ui, child) = super::super::tests::fixture();
    let a = ui.signal(1u32);
    let b = ui
        .computed(&[a.dependency()], move |i| Ok(*i.read(a)? + 10))
        .unwrap();
    let c = ui
        .computed(&[a.dependency(), b.dependency()], move |i| {
            Ok(*i.read(a)? + *i.read(b)?)
        })
        .unwrap();
    ui.bind(c, Property::color(child)).unwrap();
    let before = ui.frame([100.0; 2]).unwrap();
    let result = ui
        .transaction(|tx| {
            tx.set(a, 3)?;
            assert_eq!(*tx.read(c)?, 16);
            let failed: Result<(), Error> = tx.transaction(|tx| {
                tx.set(a, 7)?;
                assert_eq!(*tx.read(c)?, 24);
                Err(Error::InvalidChildren)
            });
            assert_eq!(failed, Err(Error::InvalidChildren));
            assert_eq!(*tx.read(c)?, 16);
            tx.set(a, 1)?;
            assert_eq!(*tx.read(c)?, 12);
            Ok(())
        })
        .unwrap();
    assert_eq!(
        (
            result.computed_evaluations,
            result.changed_sources,
            result.sink_writes
        ),
        (4, 0, 0)
    );
    assert_eq!(before.revision, result.commit.revision);
    assert_eq!(*ui.read(c).unwrap(), 12);
    let result = ui
        .transaction(|tx| {
            tx.set(a, 8)?;
            assert_eq!(*tx.read(c)?, 26);
            tx.set(a, 9)?;
            Ok(())
        })
        .unwrap();
    assert_eq!((result.computed_evaluations, result.sink_writes), (4, 1));
    assert_eq!(*ui.read(c).unwrap(), 28);
}

#[test]
fn compute_and_sink_errors_are_atomic_and_caught_error_can_retry() {
    let (mut ui, child) = super::super::tests::fixture();
    let a = ui.signal(1i32);
    let b = ui
        .computed(&[a.dependency()], move |i| Ok(*i.read(a)? * 2))
        .unwrap();
    let c = ui
        .computed(&[a.dependency(), b.dependency()], move |i| {
            let a = *i.read(a)?;
            if a < 0 {
                return Err(Error::InvalidMeasure);
            }
            Ok(*i.read(b)? as f32 / 4.0)
        })
        .unwrap();
    ui.bind(c, Property::opacity(child)).unwrap();
    let before = ui.frame([100.0; 2]).unwrap();
    assert!(matches!(
        ui.transaction(|tx| {
            tx.set(a, -1)?;
            Ok(())
        }),
        Err(Error::InvalidMeasure)
    ));
    assert!(matches!(
        ui.transaction(|tx| {
            tx.set(a, 8)?;
            Ok(())
        }),
        Err(Error::InvalidOpacity)
    ));
    assert_eq!(
        (
            *ui.read(a).unwrap(),
            *ui.read(b).unwrap(),
            *ui.read(c).unwrap()
        ),
        (1, 2, 0.5)
    );
    assert_eq!(ui.stats().transactions, 0);
    assert_eq!(ui.frame([100.0; 2]).unwrap().revision, before.revision);
    let result = ui
        .transaction(|tx| {
            tx.set(a, -1)?;
            assert_eq!(tx.read(c), Err(Error::InvalidMeasure));
            tx.set(a, 2)?;
            assert_eq!(*tx.read(c)?, 1.0);
            Ok(())
        })
        .unwrap();
    assert_eq!((result.computed_evaluations, result.sink_writes), (2, 1));
    assert_eq!(*ui.read(b).unwrap(), 4);
}

#[test]
fn immutable_edges_validate_registration_disposal_generation_and_readonly_writes() {
    let (mut ui, _) = super::super::tests::fixture();
    let a = ui.signal(5u32);
    let count = ui.source_count();
    assert!(matches!(
        ui.computed::<u32>(&[], move |i| Ok(*i.read(a)?)),
        Err(Error::UndeclaredDependency)
    ));
    assert_eq!(ui.source_count(), count);
    let (mut other, _) = super::super::tests::fixture();
    let foreign = other.signal(0u32);
    assert!(matches!(
        ui.computed(&[foreign.dependency()], |_| Ok(1)),
        Err(Error::InvalidSignal)
    ));
    let b = ui
        .computed(&[a.dependency(), a.dependency()], move |i| Ok(*i.read(a)?))
        .unwrap();
    assert!(matches!(
        ui.transaction(|tx| {
            tx.set(b, 5)?;
            Ok(())
        }),
        Err(Error::ReadonlySignal)
    ));
    assert_eq!(ui.dispose_signal(a), Err(Error::SignalInUse));
    ui.dispose_signal(b).unwrap();
    ui.dispose_signal(a).unwrap();
    let fresh = ui.signal(7u32);
    assert_ne!(fresh, a);
    assert_eq!(ui.read(a), Err(Error::InvalidSignal));
    assert!(matches!(
        ui.computed(&[a.dependency()], |_| Ok(0)),
        Err(Error::InvalidSignal)
    ));
}

#[test]
fn mixed_dag_matches_full_recomputation_with_reads_restores_and_recycled_slots() {
    let (mut ui, _) = super::super::tests::fixture();
    let recycled = ui.signal(999u32);
    let mut signals: Vec<_> = (0..4).map(|i| ui.signal(i as u32)).collect();
    // The first derived slot sorts before its direct inputs. Rank, rather than
    // slot allocation order, must determine evaluation order.
    ui.dispose_signal(recycled).unwrap();
    let mut edges = vec![];
    let mut reference = vec![0u32, 1, 2, 3];
    for index in 4..100 {
        let a = (index * 17 + index / 2 + 1) % index;
        let b = (index * index / 3 + 3) % index;
        let left = signals[a];
        let right = signals[b];
        signals.push(
            ui.computed(&[left.dependency(), right.dependency()], move |i| {
                Ok((*i.read(left)? + *i.read(right)?) / 2 % 31)
            })
            .unwrap(),
        );
        edges.push((a, b));
        reference.push((reference[a] + reference[b]) / 2 % 31);
    }
    for event in 0..200 {
        let result = ui
            .transaction(|tx| {
                for write in 0..5 {
                    let index = (event + write) % 4;
                    let next = ((event * 13 + write * 7) % 32) as u32;
                    tx.set(signals[index], next)?;
                    reference[index] = next;
                    if write % 2 == 0 {
                        for (offset, (a, b)) in edges.iter().enumerate() {
                            reference[offset + 4] = (reference[*a] + reference[*b]) / 2 % 31;
                        }
                        assert_eq!(*tx.read(signals[99])?, reference[99]);
                        let failed: Result<(), Error> = tx.transaction(|tx| {
                            tx.set(signals[0], 500)?;
                            tx.read(signals[99])?;
                            Err(Error::InvalidChildren)
                        });
                        assert_eq!(failed, Err(Error::InvalidChildren));
                        assert_eq!(*tx.read(signals[99])?, reference[99]);
                    }
                }
                Ok(())
            })
            .unwrap();
        assert!(!result.commit.request_frame);
        for (offset, (a, b)) in edges.iter().enumerate() {
            reference[offset + 4] = (reference[*a] + reference[*b]) / 2 % 31;
        }
        for (signal, expected) in signals.iter().zip(&reference) {
            assert_eq!(ui.read(*signal).unwrap(), expected);
        }
    }
}

#[test]
fn failed_settlement_restores_values_from_previous_reads_before_retry() {
    let (mut ui, _) = super::super::tests::fixture();
    let a = ui.signal(1i32);
    let b = ui
        .computed(&[a.dependency()], move |i| Ok(*i.read(a)? * 2))
        .unwrap();
    let c = ui
        .computed(&[b.dependency()], move |i| {
            let b = *i.read(b)?;
            if b < 0 {
                Err(Error::InvalidMeasure)
            } else {
                Ok(b + 1)
            }
        })
        .unwrap();
    ui.transaction(|tx| {
        tx.set(a, 2)?;
        assert_eq!(*tx.read(c)?, 5);
        tx.set(a, -1)?;
        assert_eq!(tx.read(c), Err(Error::InvalidMeasure));
        // Correcting to the last successfully read value must re-evaluate
        // nothing, while retaining that read's staged b/c values.
        tx.set(a, 2)?;
        assert_eq!(*tx.read(b)?, 4);
        assert_eq!(*tx.read(c)?, 5);
        Ok(())
    })
    .unwrap();
    assert_eq!(
        (
            *ui.read(a).unwrap(),
            *ui.read(b).unwrap(),
            *ui.read(c).unwrap()
        ),
        (2, 4, 5)
    );
    assert_eq!(ui.stats().computed_evaluations, 2);
}
