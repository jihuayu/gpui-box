use super::*;
use std::{
    cell::{Cell, RefCell},
    panic::{AssertUnwindSafe, catch_unwind},
    sync::atomic::{AtomicUsize, Ordering},
    task::{Context, Wake},
};
fn fixture() -> LogicalUi {
    LogicalUi::new_layout(
        LayoutStyle::column().size(LayoutDimension::Px(100.0), LayoutDimension::Px(80.0)),
        0x112233ff,
        BackendCapabilities {
            solid_color_patch: true,
        },
    )
    .unwrap()
}
fn hold(
    ui: &mut LogicalUi,
    scope: ScopeId,
    key: u64,
) -> (UiTask<u32>, Rc<RefCell<Option<UiTaskContext<u32>>>>) {
    let worker = Rc::new(RefCell::new(None));
    let slot = worker.clone();
    let task = ui
        .start_task(scope, key, move |cx| *slot.borrow_mut() = Some(cx))
        .unwrap();
    (task, worker)
}
#[test]
fn worker_result_applies_complete_frame_once_and_failure_retries() {
    let mut ui = fixture();
    let root = ui.root_scope();
    let render = ui.root_render();
    let color = ui.state(root, 0, || 0x112233ffu32).unwrap();
    ui.bind(color, Property::color(render)).unwrap();
    let before = ui.local_frame([100.0, 80.0]).unwrap();
    let (task, worker) = hold(&mut ui, root, 0);
    assert!(matches!(
        ui.apply_task(&task, |_, _| panic!("not ready")).unwrap(),
        TaskApply::<()>::Pending
    ));
    let worker = worker.borrow_mut().take().unwrap();
    std::thread::spawn(move || worker.complete(0x445566ff).unwrap())
        .join()
        .unwrap();
    assert_eq!(task.status(), UiTaskStatus::Ready);
    assert!(matches!(
        ui.apply_task(&task, |value, tx| {
            tx.set(color, *value)?;
            Err::<(), _>(Error::InvalidOpacity)
        }),
        Err(Error::InvalidOpacity)
    ));
    assert_eq!(*ui.read(color).unwrap(), 0x112233ff);
    assert_eq!(task.status(), UiTaskStatus::Ready);
    match ui
        .apply_task(&task, |value, tx| {
            tx.set(color, *value)?;
            Ok(7)
        })
        .unwrap()
    {
        TaskApply::Applied(c) => {
            assert_eq!(c.value, 7);
            assert!(c.commit.request_frame);
        }
        _ => panic!("expected adopted output"),
    }
    assert_eq!(task.status(), UiTaskStatus::Applied);
    assert_eq!(
        ui.published_local_frame().unwrap().resolve().items[0].fill_color,
        0x445566ff
    );
    assert_eq!(before.resolve().items[0].fill_color, 0x112233ff);
    assert!(matches!(
        ui.apply_task(&task, |_, _| panic!("duplicate")).unwrap(),
        TaskApply::<()>::Ignored(UiTaskStatus::Applied)
    ));
}
#[test]
fn replacement_and_retirement_cancel_old_work_even_when_handle_survives() {
    let mut ui = fixture();
    let root = ui.root_scope();
    let scope = ui.reconcile_keys(root, &[1]).unwrap().order[0];
    let (old, old_worker) = hold(&mut ui, scope, 1);
    let (next, next_worker) = hold(&mut ui, scope, 1);
    assert_eq!(old.status(), UiTaskStatus::Cancelled);
    let old_worker = old_worker.borrow_mut().take().unwrap();
    assert!(old_worker.cancellation_requested());
    assert_eq!(old_worker.complete(10), Err(10));
    assert!(matches!(
        ui.apply_task(&old, |_, _| panic!("stale")).unwrap(),
        TaskApply::<()>::Ignored(UiTaskStatus::Cancelled)
    ));
    ui.reconcile_keys(root, &[]).unwrap();
    assert_eq!(next.status(), UiTaskStatus::Cancelled);
    assert!(
        next_worker
            .borrow()
            .as_ref()
            .unwrap()
            .cancellation_requested()
    );
    let fresh = ui.reconcile_keys(root, &[1]).unwrap().order[0];
    assert_ne!(scope, fresh);
    assert!(ui.task::<u32>(fresh, 1).unwrap().is_none());
    assert!(matches!(
        ui.apply_task(&next, |_, _| panic!("retired")).unwrap(),
        TaskApply::<()>::Ignored(UiTaskStatus::Cancelled)
    ));
}
#[test]
fn failed_or_unwinding_candidates_never_launch_or_cancel_live_task() {
    let mut ui = fixture();
    let root = ui.root_scope();
    let (old, _worker) = hold(&mut ui, root, 1);
    let escaped = RefCell::new(None);
    let launches = Rc::new(Cell::new(0));
    let count = launches.clone();
    let result = ui.edit(
        |_| Ok(()),
        |(), edit| {
            let task = edit.start_task(root, 1, move |_: UiTaskContext<u32>| {
                count.set(count.get() + 1)
            })?;
            *escaped.borrow_mut() = Some(task);
            Err::<(), _>(Error::InvalidOpacity)
        },
    );
    assert!(result.is_err());
    assert_eq!(launches.get(), 0);
    assert_eq!(old.status(), UiTaskStatus::Running);
    assert_eq!(
        escaped.borrow().as_ref().unwrap().status(),
        UiTaskStatus::Cancelled
    );
    let count = launches.clone();
    assert!(
        catch_unwind(AssertUnwindSafe(|| {
            let _ = ui.edit(
                |_| Ok(()),
                |(), edit| -> Result<(), Error> {
                    edit.start_task(root, 1, move |_: UiTaskContext<u32>| count.set(1))?;
                    panic!("failed construction")
                },
            );
        }))
        .is_err()
    );
    assert_eq!(launches.get(), 0);
    assert_eq!(old.status(), UiTaskStatus::Running);
    assert!(
        ui.edit(
            |_| Ok(()),
            |(), edit| {
                edit.cancel_task(root, 1)?;
                Err::<(), _>(Error::InvalidOpacity)
            },
        )
        .is_err()
    );
    assert_eq!(old.status(), UiTaskStatus::Running);
}
#[test]
fn same_candidate_last_task_wins_and_removed_scope_never_launches() {
    let mut ui = fixture();
    let root = ui.root_scope();
    let count = Rc::new(Cell::new(0));
    let a = count.clone();
    let b = count.clone();
    let result = ui
        .edit(
            |_| Ok(()),
            |(), edit| {
                let first = edit.start_task(root, 1, move |_: UiTaskContext<u32>| a.set(1))?;
                let last = edit.start_task(root, 1, move |c: UiTaskContext<u32>| {
                    b.set(2);
                    c.complete(2).unwrap()
                })?;
                Ok((first, last))
            },
        )
        .unwrap()
        .result
        .value;
    assert_eq!(count.get(), 2);
    assert_eq!(result.0.status(), UiTaskStatus::Cancelled);
    assert_eq!(result.1.status(), UiTaskStatus::Ready);
    let scope = ui.reconcile_keys(root, &[1]).unwrap().order[0];
    let count2 = count.clone();
    let task = ui
        .edit(
            |_| Ok(()),
            |(), edit| {
                let task = edit.start_task(scope, 1, move |_: UiTaskContext<u32>| count2.set(3))?;
                edit.reconcile_keys(root, &[])?;
                Ok(task)
            },
        )
        .unwrap()
        .result
        .value;
    assert_eq!(count.get(), 2);
    assert_eq!(task.status(), UiTaskStatus::Cancelled);
}
#[test]
fn abandoned_and_panicking_launch_cannot_leave_infinite_pending_work() {
    let mut ui = fixture();
    let root = ui.root_scope();
    let task = ui.start_task(root, 1, |_: UiTaskContext<u32>| {}).unwrap();
    assert_eq!(task.status(), UiTaskStatus::Abandoned);
    let result = catch_unwind(AssertUnwindSafe(|| {
        let _ = ui.edit(
            |_| Ok(()),
            |(), edit| {
                edit.start_task(root, 1, |_: UiTaskContext<u32>| panic!("launch failure"))?;
                edit.start_task(root, 2, |_: UiTaskContext<u32>| panic!("must not launch"))?;
                Ok(())
            },
        );
    }));
    assert!(result.is_err());
    assert_eq!(
        ui.task::<u32>(root, 1).unwrap().unwrap().status(),
        UiTaskStatus::Abandoned
    );
    assert_eq!(
        ui.task::<u32>(root, 2).unwrap().unwrap().status(),
        UiTaskStatus::Cancelled
    );
}
struct Counter(AtomicUsize);
impl Wake for Counter {
    fn wake(self: Arc<Self>) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
    fn wake_by_ref(self: &Arc<Self>) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}
#[test]
fn worker_completion_and_scope_cancel_wake_without_polling_timer() {
    let mut ui = fixture();
    let root = ui.root_scope();
    let (mut task, worker) = hold(&mut ui, root, 1);
    let counter = Arc::new(Counter(AtomicUsize::new(0)));
    let waker = Waker::from(counter.clone());
    let mut cx = Context::from_waker(&waker);
    let mut ready = Box::pin(task.ready());
    assert!(ready.as_mut().poll(&mut cx).is_pending());
    worker.borrow_mut().take().unwrap().complete(1).unwrap();
    assert_eq!(counter.0.load(Ordering::SeqCst), 1);
    assert_eq!(
        ready.as_mut().poll(&mut cx),
        Poll::Ready(UiTaskStatus::Ready)
    );
    drop(ready);
    let (mut task, worker) = hold(&mut ui, root, 1);
    let mut worker = worker.borrow_mut().take().unwrap();
    let mut cancel = Box::pin(worker.cancelled());
    assert!(cancel.as_mut().poll(&mut cx).is_pending());
    let mut ready = Box::pin(task.ready());
    assert!(ready.as_mut().poll(&mut cx).is_pending());
    ui.cancel_task(root, 1).unwrap();
    assert_eq!(
        ready.as_mut().poll(&mut cx),
        Poll::Ready(UiTaskStatus::Cancelled)
    );
    assert_eq!(cancel.as_mut().poll(&mut cx), Poll::Ready(()));
    assert_eq!(counter.0.load(Ordering::SeqCst), 3);
}
#[test]
fn foreign_results_and_wrong_types_reject_and_owner_drop_cancels() {
    let mut ui = fixture();
    let root = ui.root_scope();
    let (task, worker) = hold(&mut ui, root, 1);
    let mut other = fixture();
    assert!(matches!(
        other.apply_task(&task, |_, _| Ok(())),
        Err(Error::InvalidTask)
    ));
    assert!(matches!(
        ui.task::<String>(root, 1),
        Err(Error::InvalidTask)
    ));
    drop(ui);
    assert_eq!(task.status(), UiTaskStatus::Cancelled);
    assert!(worker.borrow().as_ref().unwrap().cancellation_requested());
}
#[test]
fn adopted_output_is_consumed_before_post_commit_effect_panic() {
    let mut ui = fixture();
    let root = ui.root_scope();
    let value = ui.state(root, 0, || 0u32).unwrap();
    ui.effect(root, 0, &[value.dependency()], move |i| {
        if *i.read(value).unwrap() == 1 {
            panic!("effect after adopted result")
        };
        None
    })
    .unwrap();
    let (task, worker) = hold(&mut ui, root, 1);
    worker.borrow_mut().take().unwrap().complete(1).unwrap();
    assert!(
        catch_unwind(AssertUnwindSafe(|| {
            let _ = ui.apply_task(&task, |v, tx| {
                tx.set(value, *v)?;
                Ok(())
            });
        }))
        .is_err()
    );
    assert_eq!(*ui.read(value).unwrap(), 1);
    assert_eq!(task.status(), UiTaskStatus::Applied);
}

#[test]
fn ready_result_layout_failure_preserves_frame_and_full_retry() {
    let mut ui = fixture();
    let root = ui.root_scope();
    let render = ui.root_render();
    let opacity = ui.state(root, 0, || 1.0f32).unwrap();
    ui.bind(opacity, Property::opacity(render)).unwrap();
    let before = ui.local_frame([100.0, 80.0]).unwrap();
    let (task, worker) = hold(&mut ui, root, 1);
    worker.borrow_mut().take().unwrap().complete(7).unwrap();
    assert!(matches!(
        ui.apply_task(&task, |_, tx| {
            tx.set(opacity, 2.0)?;
            Ok(())
        }),
        Err(Error::InvalidOpacity)
    ));
    assert_eq!(task.status(), UiTaskStatus::Ready);
    assert_eq!(*ui.read(opacity).unwrap(), 1.0);
    assert!(before.same(ui.published_local_frame().unwrap()));
    assert!(matches!(
        ui.apply_task(&task, |_, tx| {
            tx.set(opacity, 0.5)?;
            Ok(())
        })
        .unwrap(),
        TaskApply::Applied(_)
    ));
    assert_eq!(task.status(), UiTaskStatus::Applied);
    assert_eq!(*ui.read(opacity).unwrap(), 0.5);
    assert_ne!(
        before.revision(),
        ui.published_local_frame().unwrap().revision()
    );
}

#[test]
fn retiring_component_tasks_does_not_cancel_accepted_business_commands() {
    struct Save;
    impl crate::command::Command for Save {
        type Output = ();
        type Error = ();
    }
    let (sender, mut receiver) = crate::command::command_channel::<Save>(1).unwrap();
    let ticket = sender.send(Save).ok().unwrap();
    let mut ui = fixture();
    let root = ui.root_scope();
    let scope = ui.reconcile_keys(root, &[1]).unwrap().order[0];
    let (task, _worker) = hold(&mut ui, scope, 0);
    ui.reconcile_keys(root, &[]).unwrap();
    assert_eq!(task.status(), UiTaskStatus::Cancelled);
    assert_eq!(ticket.status(), crate::command::CommandStatus::Accepted);
    receiver.try_recv().unwrap().succeed((), None);
    assert_eq!(ticket.status(), crate::command::CommandStatus::Succeeded);
}

#[derive(Clone)]
struct TaskProps {
    output: Rc<RefCell<Vec<UiTask<u32>>>>,
    workers: Rc<RefCell<Vec<UiTaskContext<u32>>>>,
}
impl PartialEq for TaskProps {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.output, &other.output) && Rc::ptr_eq(&self.workers, &other.workers)
    }
}
struct Loader;
impl Component for Loader {
    type Props = TaskProps;
    fn mount(
        props: ReadProp<TaskProps>,
        cx: &mut ComponentContext<'_, '_, '_, '_>,
    ) -> Result<(), Error> {
        let props = cx.read_prop(props)?.clone();
        let workers = props.workers.clone();
        let task = cx.start_task(0, move |cx| workers.borrow_mut().push(cx))?;
        props.output.borrow_mut().push(task);
        cx.append_layout(
            LayoutStyle::row().size(LayoutDimension::Px(40.0), LayoutDimension::Px(20.0)),
            0xaabbccff,
        )?;
        Ok(())
    }
}
#[test]
fn component_mount_once_tasks_retain_reorder_and_cancel_on_full_transaction_retirement() {
    let mut ui = fixture();
    let root = ui.root_scope();
    let render = ui.root_render();
    let props = TaskProps {
        output: Rc::new(RefCell::new(vec![])),
        workers: Rc::new(RefCell::new(vec![])),
    };
    let scopes = ui
        .edit_local_frame(
            LayoutConstraints::viewport(100.0, 80.0),
            [100.0, 80.0],
            |_| Ok(()),
            |(), edit| {
                edit.components::<Loader>(root, render, [(1, props.clone()), (2, props.clone())])
            },
        )
        .unwrap()
        .0
        .result
        .value;
    assert_eq!(props.workers.borrow().len(), 2);
    let first = props.output.borrow()[0].clone();
    let second = props.output.borrow()[1].clone();
    ui.edit(
        |_| Ok(()),
        |(), edit| {
            edit.components::<Loader>(root, render, [(2, props.clone()), (1, props.clone())])
        },
    )
    .unwrap();
    assert_eq!(props.workers.borrow().len(), 2);
    assert_eq!(first.status(), UiTaskStatus::Running);
    let before = ui.published_local_frame().unwrap().clone();
    assert!(
        ui.edit(
            |_| Ok(()),
            |(), edit| {
                edit.reconcile_keys(root, &[])?;
                Err::<(), _>(Error::InvalidComponent)
            }
        )
        .is_err()
    );
    assert_eq!(first.status(), UiTaskStatus::Running);
    assert!(before.same(ui.published_local_frame().unwrap()));
    ui.edit(
        |_| Ok(()),
        |(), edit| {
            edit.components::<Loader>(root, render, [(2, props.clone())])?;
            Ok(())
        },
    )
    .unwrap();
    assert!(ui.children(root).unwrap().contains(&scopes[1]));
    assert_eq!(first.status(), UiTaskStatus::Cancelled);
    assert_eq!(second.status(), UiTaskStatus::Running);
    assert_eq!(ui.local_frame([100.0, 80.0]).unwrap().len(), 2);
}

#[test]
fn ready_task_can_retire_itself_through_component_producer_without_replaying_result() {
    let mut ui = fixture();
    let root = ui.root_scope();
    let render = ui.root_render();
    let show = ui.state(root, 0, || true).unwrap();
    let props = TaskProps {
        output: Rc::new(RefCell::new(vec![])),
        workers: Rc::new(RefCell::new(vec![])),
    };
    let payload = props.clone();
    ui.edit_local_frame(
        LayoutConstraints::viewport(100.0, 80.0),
        [100.0, 80.0],
        |_| Ok(()),
        |(), edit| {
            let site = edit.component_sites(root, &[9])?[0];
            edit.component_producer(
                root,
                0,
                render,
                &[],
                ComponentProducer::new(move |inputs| {
                    let mut plan = ComponentPlan::new();
                    if *inputs.read(show)? {
                        plan.select::<Loader>(site, render, 0, payload.clone());
                    } else {
                        plan.empty(site, render);
                    };
                    Ok(plan)
                }),
            )?;
            Ok(())
        },
    )
    .unwrap();
    let task = props.output.borrow()[0].clone();
    props
        .workers
        .borrow_mut()
        .pop()
        .unwrap()
        .complete(9)
        .unwrap();
    assert!(matches!(
        ui.apply_task(&task, |_, tx| {
            tx.set(show, false)?;
            Ok(())
        })
        .unwrap(),
        TaskApply::Applied(_)
    ));
    assert_eq!(task.status(), UiTaskStatus::Applied);
    assert_eq!(ui.published_local_frame().unwrap().len(), 1);
    assert!(matches!(
        ui.apply_task(&task, |_, _| panic!("already adopted"))
            .unwrap(),
        TaskApply::<()>::Ignored(UiTaskStatus::Applied)
    ));
}

#[test]
fn cancellation_is_visible_to_effect_cleanup_on_retirement_and_owner_drop() {
    for dispose_owner in [false, true] {
        let mut ui = fixture();
        let root = ui.root_scope();
        let scope = ui.reconcile_keys(root, &[1]).unwrap().order[0];
        let (task, _worker) = hold(&mut ui, scope, 0);
        let checked = Rc::new(Cell::new(false));
        let flag = checked.clone();
        let observed = task.clone();
        ui.effect(scope, 0, &[], move |_| {
            let flag = flag.clone();
            let observed = observed.clone();
            Some(Box::new(move || {
                assert_eq!(observed.status(), UiTaskStatus::Cancelled);
                flag.set(true)
            }))
        })
        .unwrap();
        if dispose_owner {
            drop(ui);
        } else {
            ui.reconcile_keys(root, &[]).unwrap();
        }
        assert!(checked.get());
    }
}
