use super::*;
use std::cell::RefCell;

#[test]
fn observers_see_only_final_published_values_run_once_and_cleanup_before_rerun() {
    let (mut ui, child) = super::super::tests::fixture();
    let a = ui.signal(2u32);
    let bucket = ui
        .computed(&[a.dependency()], move |i| Ok(*i.read(a)? / 2))
        .unwrap();
    let color = ui
        .computed(&[bucket.dependency()], move |i| {
            Ok(*i.read(bucket)? << 8 | 255)
        })
        .unwrap();
    ui.bind(color, Property::color(child)).unwrap();
    ui.frame([100.0; 2]).unwrap();
    let events = Rc::new(RefCell::new(vec![]));
    let log = events.clone();
    let effect = ui
        .effect(
            &[bucket.dependency(), color.dependency(), color.dependency()],
            move |i| {
                let value = *i.read(bucket).unwrap();
                assert_eq!(*i.read(color).unwrap(), value << 8 | 255);
                let SourceContext::Runtime(runtime) = i.context else {
                    panic!("committed effect context")
                };
                let published = runtime.tree.published_frame();
                // The tree revision changes before observation; an owned output packet
                // is still generated later by the host's ordinary prepaint phase.
                if value == 2 {
                    assert!(runtime.tree.revision > published.unwrap().revision);
                }
                log.borrow_mut().push(("run", value));
                let log = log.clone();
                Some(Box::new(move || log.borrow_mut().push(("cleanup", value))))
            },
        )
        .unwrap();
    assert_eq!(*events.borrow(), [("run", 1)]);
    let commit = ui
        .transaction(|tx| {
            tx.set(a, 3)?;
            Ok(())
        })
        .unwrap();
    assert_eq!(commit.effect_runs, 0);
    let commit = ui
        .transaction(|tx| {
            tx.set(a, 8)?;
            tx.read(color)?;
            tx.set(a, 4)?;
            Ok(())
        })
        .unwrap();
    assert_eq!((commit.effect_runs, commit.effect_cleanups), (1, 1));
    assert_eq!(*events.borrow(), [("run", 1), ("cleanup", 1), ("run", 2)]);
    assert_eq!((ui.stats().effect_runs, ui.stats().effect_cleanups), (1, 1));
    ui.dispose_effect(effect).unwrap();
    assert_eq!(events.borrow().last(), Some(&("cleanup", 2)));
    assert_eq!(ui.dispose_effect(effect), Err(Error::InvalidEffect));
    assert_eq!(ui.effect_count(), 0);
    ui.dispose_signal(color).unwrap();
}

#[test]
fn rejected_or_restored_transactions_never_observe_or_cleanup() {
    let (mut ui, child) = super::super::tests::fixture();
    let a = ui.signal(1i32);
    let opacity = ui
        .computed(&[a.dependency()], move |i| {
            let a = *i.read(a)?;
            if a < 0 {
                return Err(Error::InvalidMeasure);
            }
            Ok(a as f32 / 2.0)
        })
        .unwrap();
    ui.bind(opacity, Property::opacity(child)).unwrap();
    let log = Rc::new(RefCell::new(vec![]));
    let observed = log.clone();
    ui.effect(&[a.dependency(), opacity.dependency()], move |i| {
        let value = *i.read(a).unwrap();
        observed.borrow_mut().push(value);
        let observed = observed.clone();
        Some(Box::new(move || observed.borrow_mut().push(-100)))
    })
    .unwrap();
    for (value, error) in [(-1, Error::InvalidMeasure), (8, Error::InvalidOpacity)] {
        assert!(
            matches!(ui.transaction(|tx| { tx.set(a, value)?; Ok(()) }), Err(actual) if actual==error)
        );
    }
    let failed: Result<ReactiveCommit<()>, Error> = ui.transaction(|tx| {
        tx.set(a, 2)?;
        Err(Error::InvalidChildren)
    });
    assert!(matches!(failed, Err(Error::InvalidChildren)));
    ui.transaction(|tx| {
        tx.set(a, 2)?;
        tx.read(opacity)?;
        tx.set(a, 1)?;
        let failed: Result<(), Error> = tx.transaction(|tx| {
            tx.set(a, 2)?;
            Err(Error::InvalidChildren)
        });
        assert!(failed.is_err());
        Ok(())
    })
    .unwrap();
    assert_eq!(*log.borrow(), [1]);
    assert_eq!(ui.stats().effect_runs, 0);
    drop(ui);
    assert_eq!(*log.borrow(), [1, -100]);
}

#[test]
fn effect_dependencies_validate_owner_generation_and_guard_source_disposal() {
    let (mut ui, _) = super::super::tests::fixture();
    let (mut foreign, _) = super::super::tests::fixture();
    let input = ui.signal(1u32);
    assert!(matches!(
        foreign.effect(&[input.dependency()], |_| None),
        Err(Error::InvalidSignal)
    ));
    let effect = ui
        .effect(&[input.dependency()], move |i| {
            assert_eq!(*i.read(input).unwrap(), 1);
            None
        })
        .unwrap();
    assert_eq!(foreign.dispose_effect(effect), Err(Error::InvalidEffect));
    assert_eq!(ui.dispose_signal(input), Err(Error::SignalInUse));
    ui.dispose_effect(effect).unwrap();
    ui.dispose_signal(input).unwrap();
    assert!(matches!(
        ui.effect(&[input.dependency()], |_| None),
        Err(Error::InvalidSignal)
    ));
    let input = ui.signal(9u32);
    ui.transaction(|tx| {
        tx.set(input, 10)?;
        Ok(())
    })
    .unwrap();
    assert_eq!(ui.stats().effect_runs, 0);
    let effect = ui
        .effect(&[], move |i| {
            assert_eq!(i.read(input), Err(Error::UndeclaredDependency));
            None
        })
        .unwrap();
    assert_eq!(ui.effect_count(), 1);
    ui.dispose_effect(effect).unwrap();
}
