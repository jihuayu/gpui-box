use super::super::tests::fixture;
use super::*;
use std::{
    cell::RefCell,
    panic::{AssertUnwindSafe, catch_unwind},
};
fn effect() -> (
    UiTaskEffect<u32, u32>,
    Rc<RefCell<Vec<(u32, UiTaskContext<u32>)>>>,
) {
    let workers = Rc::new(RefCell::new(vec![]));
    let output = workers.clone();
    (
        UiTaskEffect::new(move |input, worker| output.borrow_mut().push((input, worker))),
        workers,
    )
}
#[test]
fn equal_final_input_reuses_task_changed_input_cancels_and_starts_once() {
    let mut ui = fixture();
    let root = ui.root_scope();
    let input = ui.state(root, 0, || 1u32).unwrap();
    let (effect, workers) = effect();
    ui.task_effect(root, 0, input, effect.clone()).unwrap();
    assert_eq!(workers.borrow().len(), 1);
    let old = ui.task::<u32>(root, 0).unwrap().unwrap();
    ui.transaction(|tx| {
        tx.set(input, 2)?;
        tx.set(input, 1)?;
        Ok(())
    })
    .unwrap();
    assert_eq!(workers.borrow().len(), 1);
    assert_eq!(old.status(), UiTaskStatus::Running);
    ui.task_effect(root, 0, input, effect).unwrap();
    assert_eq!(workers.borrow().len(), 1);
    ui.transaction(|tx| {
        tx.set(input, 2)?;
        tx.set(input, 3)?;
        Ok(())
    })
    .unwrap();
    assert_eq!(workers.borrow().len(), 2);
    assert_eq!(workers.borrow()[1].0, 3);
    assert_eq!(old.status(), UiTaskStatus::Cancelled);
    assert_eq!(workers.borrow_mut().remove(0).1.complete(8), Err(8));
    let current = ui.task::<u32>(root, 0).unwrap().unwrap();
    workers.borrow_mut().pop().unwrap().1.complete(9).unwrap();
    ui.apply_task(&current, |_, _| Ok(())).unwrap();
    ui.retry_task_effects().unwrap();
    assert_eq!(current.status(), UiTaskStatus::Applied);
    assert!(workers.borrow().is_empty());
}
#[test]
fn failed_frame_or_unwind_keeps_previous_task_input_and_no_launch() {
    let mut ui = fixture();
    let root = ui.root_scope();
    let render = ui.root_render();
    let input = ui.state(root, 0, || 1u32).unwrap();
    let (effect, workers) = effect();
    ui.task_effect(root, 0, input, effect).unwrap();
    let old = ui.task::<u32>(root, 0).unwrap().unwrap();
    let before = ui.local_frame([100.0, 80.0]).unwrap();
    assert!(matches!(
        ui.edit_local_frame(
            LayoutConstraints::viewport(100.0, 80.0),
            [100.0, 80.0],
            |tx| tx.set(input, 2),
            |_, edit| {
                edit.properties().opacity(render, 2.0);
                Ok(())
            }
        ),
        Err(Error::InvalidOpacity)
    ));
    assert_eq!(*ui.read(input).unwrap(), 1);
    assert_eq!(old.status(), UiTaskStatus::Running);
    assert_eq!(workers.borrow().len(), 1);
    assert!(before.same(ui.published_local_frame().unwrap()));
    assert!(
        catch_unwind(AssertUnwindSafe(|| {
            let _ = ui.transaction(|tx| -> Result<(), Error> {
                tx.set(input, 2)?;
                panic!("source unwind")
            });
        }))
        .is_err()
    );
    assert_eq!(old.status(), UiTaskStatus::Running);
}
#[test]
fn full_capacity_commits_new_input_waits_and_retries_only_latest() {
    let mut ui = fixture();
    let root = ui.root_scope();
    let pool = UiTaskPool::new(1).unwrap();
    ui.set_task_pool(pool.clone()).unwrap();
    let input = ui.state(root, 0, || 1u32).unwrap();
    let (effect, workers) = effect();
    ui.task_effect(root, 0, input, effect).unwrap();
    let old = ui.task::<u32>(root, 0).unwrap().unwrap();
    ui.transaction(|tx| tx.set(input, 2)).unwrap();
    assert_eq!(*ui.read(input).unwrap(), 2);
    assert_eq!(old.status(), UiTaskStatus::Cancelled);
    assert_eq!(
        ui.task_effect_status(root, 0).unwrap(),
        UiTaskEffectStatus::WaitingForCapacity
    );
    assert_eq!(pool.stats().in_use, 1);
    ui.transaction(|tx| tx.set(input, 3)).unwrap();
    assert_eq!(workers.borrow().len(), 1);
    assert!(ui.task::<u32>(root, 0).unwrap().is_none());
    assert_eq!(workers.borrow_mut().pop().unwrap().1.complete(90), Err(90));
    assert_eq!(pool.stats().in_use, 0);
    ui.retry_task_effects().unwrap();
    assert_eq!(workers.borrow().len(), 1);
    assert_eq!(workers.borrow()[0].0, 3);
    assert_eq!(pool.stats().peak, 1);
    assert_eq!(
        ui.task_effect_status(root, 0).unwrap(),
        UiTaskEffectStatus::Task(UiTaskStatus::Running)
    );
}
#[test]
fn component_readprop_final_tuple_and_scope_retirement_share_task_lifecycle() {
    #[derive(Clone)]
    struct Props {
        workers: Rc<RefCell<Vec<(u32, UiTaskContext<u32>)>>>,
        value: u32,
    }
    impl PartialEq for Props {
        fn eq(&self, other: &Self) -> bool {
            self.value == other.value && Rc::ptr_eq(&self.workers, &other.workers)
        }
    }
    struct Loader;
    impl Component for Loader {
        type Props = Props;
        fn mount(
            props: ReadProp<Props>,
            cx: &mut ComponentContext<'_, '_, '_, '_>,
        ) -> Result<(), Error> {
            let workers = cx.read_prop(props)?.workers.clone();
            cx.task_effect_prop(
                0,
                props,
                UiTaskEffect::new(move |input: Props, context| {
                    workers.borrow_mut().push((input.value, context))
                }),
            )?;
            Ok(())
        }
    }
    let mut ui = fixture();
    let root = ui.root_scope();
    let render = ui.root_render();
    let workers = Rc::new(RefCell::new(vec![]));
    let props = |value| Props {
        workers: workers.clone(),
        value,
    };
    let scope = ui
        .edit_local_frame(
            LayoutConstraints::viewport(100.0, 80.0),
            [100.0, 80.0],
            |_| Ok(()),
            |(), edit| edit.components::<Loader>(root, render, [(7, props(1))]),
        )
        .unwrap()
        .0
        .result
        .value[0];
    let old = ui.task::<u32>(scope, 0).unwrap().unwrap();
    ui.edit(
        |_| Ok(()),
        |(), edit| edit.components::<Loader>(root, render, [(7, props(2))]),
    )
    .unwrap();
    assert_eq!(old.status(), UiTaskStatus::Cancelled);
    assert_eq!(workers.borrow().len(), 2);
    assert_eq!(workers.borrow()[1].0, 2);
    let current = ui.task::<u32>(scope, 0).unwrap().unwrap();
    ui.reconcile_keys(root, &[]).unwrap();
    assert_eq!(current.status(), UiTaskStatus::Cancelled);
    assert!(ui.task_effect_status(scope, 0).is_err());
}
#[test]
fn effect_key_ownership_and_dependency_retirement_are_checked() {
    let mut ui = fixture();
    let root = ui.root_scope();
    let scopes = ui.reconcile_keys(root, &[1, 2]).unwrap().order;
    let input = ui.state(scopes[0], 0, || 1u32).unwrap();
    let (effect, _workers) = effect();
    ui.task_effect(scopes[1], 0, input, effect).unwrap();
    assert!(matches!(
        ui.start_task(scopes[1], 0, |_: UiTaskContext<u32>| {}),
        Err(Error::InvalidTask)
    ));
    assert!(matches!(
        ui.cancel_task(scopes[1], 0),
        Err(Error::InvalidTask)
    ));
    assert!(ui.reconcile_keys(root, &[2]).is_err());
    assert_eq!(ui.children(root).unwrap(), scopes);
    ui.remove_task_effect(scopes[1], 0).unwrap();
    ui.reconcile_keys(root, &[2]).unwrap();
}
#[test]
fn remove_effect_cancels_and_factory_replace_restarts_even_equal_input() {
    let mut ui = fixture();
    let root = ui.root_scope();
    let input = ui.state(root, 0, || 1u32).unwrap();
    let (factory, workers) = effect();
    ui.task_effect(root, 0, input, factory).unwrap();
    let old = ui.task::<u32>(root, 0).unwrap().unwrap();
    let (replacement, new_workers) = effect();
    ui.task_effect(root, 0, input, replacement).unwrap();
    assert_eq!(old.status(), UiTaskStatus::Cancelled);
    assert_eq!(new_workers.borrow().len(), 1);
    assert_eq!(workers.borrow().len(), 1);
    let current = ui.task::<u32>(root, 0).unwrap().unwrap();
    ui.remove_task_effect(root, 0).unwrap();
    assert_eq!(current.status(), UiTaskStatus::Cancelled);
    assert!(ui.task_effect_status(root, 0).is_err());
}
#[test]
fn source_effects_see_adopted_task_and_effect_panic_does_not_launch_new_work() {
    let mut ui = fixture();
    let root = ui.root_scope();
    let input = ui.state(root, 0, || 1u32).unwrap();
    let (effect, workers) = effect();
    ui.task_effect(root, 0, input, effect).unwrap();
    let old = ui.task::<u32>(root, 0).unwrap().unwrap();
    ui.effect(root, 0, &[input.dependency()], move |values| {
        if *values.read(input).unwrap() == 2 {
            panic!("post commit")
        };
        None
    })
    .unwrap();
    assert!(
        catch_unwind(AssertUnwindSafe(|| {
            let _ = ui.transaction(|tx| tx.set(input, 2));
        }))
        .is_err()
    );
    assert_eq!(*ui.read(input).unwrap(), 2);
    assert_eq!(old.status(), UiTaskStatus::Cancelled);
    assert_eq!(workers.borrow().len(), 1);
    assert_eq!(
        ui.task_effect_status(root, 0).unwrap(),
        UiTaskEffectStatus::Task(UiTaskStatus::Cancelled)
    );
    ui.retry_task_effects().unwrap();
    assert_eq!(workers.borrow().len(), 1); // terminal panic is not an implicit retry
}

#[test]
fn computed_inputs_settle_before_launch_and_equal_projection_does_not_restart() {
    let mut ui = fixture();
    let root = ui.root_scope();
    let input = ui.state(root, 0, || 1u32).unwrap();
    let projected = ui
        .computed(root, 1, &[input.dependency()], move |i| {
            Ok(*i.read(input)? / 10)
        })
        .unwrap();
    let (effect, workers) = effect();
    ui.task_effect(root, 0, projected, effect).unwrap();
    ui.transaction(|tx| tx.set(input, 8)).unwrap();
    assert_eq!(workers.borrow().len(), 1);
    ui.transaction(|tx| {
        tx.set(input, 15)?;
        tx.set(input, 27)
    })
    .unwrap();
    assert_eq!(
        workers.borrow().iter().map(|(i, _)| *i).collect::<Vec<_>>(),
        [0, 2]
    );
}

#[test]
fn each_frame_entrypoint_can_retry_capacity_without_producers() {
    for kind in 0..6 {
        let mut ui = fixture();
        let root = ui.root_scope();
        ui.set_task_pool(UiTaskPool::new(1).unwrap()).unwrap();
        let input = ui.state(root, 0, || 1u32).unwrap();
        let (effect, workers) = effect();
        ui.task_effect(root, 0, input, effect).unwrap();
        ui.transaction(|tx| tx.set(input, 2)).unwrap();
        drop(workers.borrow_mut().pop());
        let constraints = LayoutConstraints::viewport(100.0, 80.0);
        match kind {
            0 => {
                ui.frame([100.0, 80.0]).unwrap();
            }
            1 => {
                ui.local_frame([100.0, 80.0]).unwrap();
            }
            2 => {
                ui.frame_with_constraints(constraints).unwrap();
            }
            3 => {
                ui.frame_in_viewport(constraints, [100.0, 80.0]).unwrap();
            }
            4 => {
                ui.local_frame_in_viewport(constraints, [100.0, 80.0])
                    .unwrap();
            }
            _ => {
                ui.retry_task_effects().unwrap();
            }
        }
        assert_eq!(workers.borrow().len(), 1, "frame kind {kind}");
        assert_eq!(workers.borrow()[0].0, 2);
    }
}

#[test]
fn same_candidate_declaration_then_retirement_never_schedules() {
    let mut ui = fixture();
    let root = ui.root_scope();
    let (effect, workers) = effect();
    ui.edit(
        |_| Ok(()),
        |(), edit| {
            let child = edit.reconcile_keys(root, &[1])?[0];
            let input = edit.state(child, 0, || 1u32)?;
            edit.task_effect(child, 0, input, effect)?;
            edit.reconcile_keys(root, &[])?;
            Ok(())
        },
    )
    .unwrap();
    assert!(workers.borrow().is_empty());
    assert_eq!(ui.task_pool().stats().admitted, 0);
}

#[test]
fn source_driven_producer_retirement_cancels_without_restarting_task() {
    let mut ui = fixture();
    let root = ui.root_scope();
    let render = ui.root_render();
    let child = ui.reconcile_keys(root, &[1]).unwrap().order[0];
    let request = ui.state(root, 0, || 1u32).unwrap();
    let (effect, workers) = effect();
    ui.task_effect(child, 0, request, effect).unwrap();
    let old = ui.task::<u32>(child, 0).unwrap().unwrap();
    ui.edit_local_frame(
        LayoutConstraints::viewport(100.0, 80.0),
        [100.0, 80.0],
        |_| Ok(()),
        |(), edit| {
            edit.layout_producer(
                root,
                0,
                render,
                LayoutBuildKind::InlineAllocation,
                &[],
                LayoutProducer::new(move |_, edit| {
                    let keys = if *edit.read(request)? == 1 {
                        &[1][..]
                    } else {
                        &[][..]
                    };
                    edit.reconcile_keys(root, keys)?;
                    Ok(())
                }),
            )?;
            Ok(())
        },
    )
    .unwrap();
    ui.transaction(|tx| tx.set(request, 2)).unwrap();
    assert_eq!(old.status(), UiTaskStatus::Cancelled);
    assert_eq!(workers.borrow().len(), 1);
    assert!(ui.children(root).unwrap().is_empty());
}

#[test]
fn failed_remove_or_factory_replacement_preserves_live_registration_and_worker() {
    let mut ui = fixture();
    let root = ui.root_scope();
    let input = ui.state(root, 0, || 1u32).unwrap();
    let (factory, workers) = effect();
    ui.task_effect(root, 0, input, factory).unwrap();
    let old = ui.task::<u32>(root, 0).unwrap().unwrap();
    assert!(
        ui.edit(
            |_| Ok(()),
            |(), edit| {
                edit.remove_task_effect(root, 0)?;
                Err::<(), _>(Error::InvalidStyle)
            }
        )
        .is_err()
    );
    let (replacement, new_workers) = effect();
    assert!(
        ui.edit(
            |_| Ok(()),
            |(), edit| {
                edit.task_effect(root, 0, input, replacement)?;
                Err::<(), _>(Error::InvalidStyle)
            }
        )
        .is_err()
    );
    assert_eq!(old.status(), UiTaskStatus::Running);
    assert!(new_workers.borrow().is_empty());
    ui.transaction(|tx| tx.set(input, 2)).unwrap();
    assert_eq!(workers.borrow().len(), 2);
    assert!(new_workers.borrow().is_empty());
}

#[test]
fn event_source_automatically_restarts_and_rejects_manual_key_ownership() {
    use input::*;
    let mut ui = fixture();
    let root = ui.root_scope();
    let render = ui.root_render();
    let input = ui.state(root, 0, || 1u32).unwrap();
    let (factory, workers) = effect();
    ui.task_effect(root, 0, input, factory).unwrap();
    ui.edit_local_frame(
        LayoutConstraints::viewport(100.0, 80.0),
        [100.0, 80.0],
        |_| Ok(()),
        |(), edit| {
            let handler = edit.event_handler(
                root,
                0,
                EventHandler::new(move |event, cx| {
                    if event.kind == UiInputEventKind::Activate {
                        assert!(matches!(
                            cx.start_task(0, |_: UiTaskContext<u32>| {}),
                            Err(Error::InvalidTask)
                        ));
                        assert!(matches!(cx.cancel_task(0), Err(Error::InvalidTask)));
                        cx.set(input, 2)?;
                    }
                    Ok(())
                }),
            )?;
            edit.interaction(
                root,
                render,
                Some(UiInteraction {
                    activate_handler: Some(handler),
                    ..Default::default()
                }),
            )
        },
    )
    .unwrap();
    let mut router = UiInputRouter::new();
    router
        .adopt(ui.input_scene().unwrap(), UiInputEvidence::ImageInstalled)
        .unwrap();
    ui.handle_input(&mut router, UiInput::PrimaryDown([5.0; 2]))
        .unwrap();
    ui.handle_input(&mut router, UiInput::PrimaryUp([5.0; 2]))
        .unwrap();
    assert_eq!(
        workers.borrow().iter().map(|(i, _)| *i).collect::<Vec<_>>(),
        [1, 2]
    );
    ui.remove_task_effect(root, 0).unwrap();
    ui.start_task(root, 0, |_: UiTaskContext<u32>| {}).unwrap();
    let (effect, _) = effect();
    assert!(matches!(
        ui.task_effect(root, 0, input, effect),
        Err(Error::InvalidTask)
    ));
}
