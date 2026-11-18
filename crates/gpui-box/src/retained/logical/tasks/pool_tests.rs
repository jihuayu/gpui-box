use super::tests::{Counter, fixture, hold};
use super::*;
use std::{cell::Cell, sync::atomic::Ordering, task::Context};

#[test]
fn full_pool_keeps_current_task_and_cancelled_worker_still_holds_capacity() {
    let mut ui = fixture();
    let root = ui.root_scope();
    let pool = UiTaskPool::new(1).unwrap();
    ui.set_task_pool(pool.clone()).unwrap();
    let (old, worker) = hold(&mut ui, root, 0);
    assert_eq!(pool.stats().in_use, 1);
    assert!(matches!(
        ui.start_task(root, 0, |_: UiTaskContext<u32>| panic!("not admitted")),
        Err(Error::TaskCapacity)
    ));
    assert_eq!(old.status(), UiTaskStatus::Running);
    let mut receiver = pool.open_capacity().unwrap();
    let counter = Arc::new(Counter::default());
    let waker = Waker::from(counter.clone());
    let mut cx = Context::from_waker(&waker);
    let mut ready = Box::pin(receiver.ready());
    assert!(ready.as_mut().poll(&mut cx).is_pending());
    ui.cancel_task(root, 0).unwrap();
    assert_eq!(old.status(), UiTaskStatus::Cancelled);
    assert_eq!(pool.stats().in_use, 1);
    assert_eq!(counter.0.load(Ordering::SeqCst), 0);
    let worker = worker.borrow_mut().take().unwrap();
    std::thread::spawn(move || assert_eq!(worker.complete(8), Err(8)))
        .join()
        .unwrap();
    assert_eq!(counter.0.load(Ordering::SeqCst), 1);
    assert_eq!(
        ready.as_mut().poll(&mut cx),
        Poll::Ready(Some(pool.stats()))
    );
    assert_eq!(pool.stats().in_use, 0);
    assert_eq!(pool.stats().peak, 1);
    assert_eq!(pool.stats().rejected, 1);
    let (next, _worker) = hold(&mut ui, root, 0);
    assert_eq!(next.status(), UiTaskStatus::Running);
}
#[test]
fn ready_output_counts_until_adoption_or_drop_and_failed_apply_keeps_reservation() {
    let mut ui = fixture();
    let root = ui.root_scope();
    let pool = UiTaskPool::new(1).unwrap();
    ui.set_task_pool(pool.clone()).unwrap();
    let (task, worker) = hold(&mut ui, root, 0);
    worker.borrow_mut().take().unwrap().complete(8).unwrap();
    assert_eq!(pool.stats().in_use, 1);
    assert!(matches!(
        ui.apply_task(&task, |_, _| Err::<(), _>(Error::InvalidOpacity)),
        Err(Error::InvalidOpacity)
    ));
    assert_eq!(pool.stats().in_use, 1);
    ui.apply_task(&task, |_, _| Ok(())).unwrap();
    assert_eq!(task.status(), UiTaskStatus::Applied);
    assert_eq!(pool.stats().in_use, 0);
    let (next, worker) = hold(&mut ui, root, 0);
    worker.borrow_mut().take().unwrap().complete(9).unwrap();
    assert_eq!(pool.stats().in_use, 1);
    ui.cancel_task(root, 0).unwrap();
    assert_eq!(pool.stats().in_use, 0);
    assert_eq!(next.status(), UiTaskStatus::Cancelled);
}
#[test]
fn shared_pool_spans_owners_and_failed_candidate_releases_only_new_reservations() {
    let pool = UiTaskPool::new(2).unwrap();
    let mut first = fixture();
    let a = first.root_scope();
    first.set_task_pool(pool.clone()).unwrap();
    let mut second = fixture();
    let b = second.root_scope();
    second.set_task_pool(pool.clone()).unwrap();
    let (_old, worker) = hold(&mut first, a, 0);
    let launches = Rc::new(Cell::new(0));
    let count = launches.clone();
    assert!(
        second
            .edit(
                |_| Ok(()),
                |(), edit| {
                    edit.start_task(b, 0, move |_: UiTaskContext<u32>| count.set(1))?;
                    Err::<(), _>(Error::InvalidOpacity)
                }
            )
            .is_err()
    );
    assert_eq!(launches.get(), 0);
    assert_eq!(pool.stats().in_use, 1);
    let (_next, worker2) = hold(&mut second, b, 0);
    assert_eq!(pool.stats().in_use, 2);
    assert!(matches!(
        first.start_task(a, 1, |_: UiTaskContext<u32>| {}),
        Err(Error::TaskCapacity)
    ));
    drop(first);
    assert_eq!(pool.stats().in_use, 2);
    drop(worker.borrow_mut().take());
    assert_eq!(pool.stats().in_use, 1);
    drop(worker2.borrow_mut().take());
    assert_eq!(pool.stats().in_use, 0);
}
#[test]
fn busy_can_publish_fallback_in_same_candidate_without_poisoning_component() {
    let mut ui = fixture();
    let root = ui.root_scope();
    let render = ui.root_render();
    let pool = UiTaskPool::new(1).unwrap();
    ui.set_task_pool(pool.clone()).unwrap();
    let (_old, _worker) = hold(&mut ui, root, 0);
    ui.edit_local_frame(
        LayoutConstraints::viewport(100.0, 80.0),
        [100.0, 80.0],
        |_| Ok(()),
        |(), edit| {
            let result = edit.start_task(root, 1, |_: UiTaskContext<u32>| panic!("full"));
            if matches!(result, Err(Error::TaskCapacity)) {
                edit.properties().color(render, 0xaabbccff);
                Ok(())
            } else {
                panic!("must return capacity")
            }
        },
    )
    .unwrap();
    assert_eq!(
        ui.published_local_frame().unwrap().resolve().items[0].fill_color,
        0xaabbccff
    );
    assert_eq!(pool.stats().in_use, 1);
}
#[test]
fn pool_configuration_and_capacity_receiver_are_explicit_and_do_not_reserve() {
    assert!(matches!(UiTaskPool::new(0), Err(Error::InvalidTaskPool)));
    let mut ui = fixture();
    let root = ui.root_scope();
    let pool = UiTaskPool::new(1).unwrap();
    ui.set_task_pool(pool.clone()).unwrap();
    let receiver = pool.open_capacity().unwrap();
    assert!(matches!(pool.open_capacity(), Err(Error::InvalidTaskPool)));
    drop(receiver);
    let mut receiver = pool.open_capacity().unwrap();
    let counter = Arc::new(Counter::default());
    let waker = Waker::from(counter);
    let mut cx = Context::from_waker(&waker);
    assert!(Box::pin(receiver.ready()).as_mut().poll(&mut cx).is_ready());
    assert_eq!(pool.stats().in_use, 0);
    let (task, worker) = hold(&mut ui, root, 0);
    assert!(matches!(
        ui.set_task_pool(UiTaskPool::new(2).unwrap()),
        Err(Error::InvalidTaskPool)
    ));
    ui.cancel_task(root, 0).unwrap();
    assert_eq!(task.status(), UiTaskStatus::Cancelled);
    assert!(matches!(
        ui.set_task_pool(UiTaskPool::new(2).unwrap()),
        Err(Error::InvalidTaskPool)
    ));
    drop(worker.borrow_mut().take());
    ui.set_task_pool(UiTaskPool::new(2).unwrap()).unwrap();
    drop((ui, pool));
    assert_eq!(
        Box::pin(receiver.ready()).as_mut().poll(&mut cx),
        Poll::Ready(None)
    );
}
#[test]
fn ready_payload_destructor_runs_before_releasing_capacity() {
    struct Payload {
        pool: UiTaskPool,
        seen: Arc<std::sync::atomic::AtomicUsize>,
    }
    impl Drop for Payload {
        fn drop(&mut self) {
            self.seen.store(self.pool.stats().in_use, Ordering::SeqCst);
        }
    }
    let mut ui = fixture();
    let root = ui.root_scope();
    let pool = UiTaskPool::new(1).unwrap();
    ui.set_task_pool(pool.clone()).unwrap();
    let seen = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let count = seen.clone();
    let owned = pool.clone();
    let _task = ui
        .start_task(root, 0, move |context: UiTaskContext<Payload>| {
            context
                .complete(Payload {
                    pool: owned,
                    seen: count,
                })
                .ok()
                .unwrap();
        })
        .unwrap();
    assert_eq!(pool.stats().in_use, 1);
    ui.cancel_task(root, 0).unwrap();
    assert_eq!(seen.load(Ordering::SeqCst), 1);
    assert_eq!(pool.stats().in_use, 0);
}

#[test]
fn concurrent_owner_admission_never_exceeds_shared_capacity() {
    let pool = UiTaskPool::new(3).unwrap();
    let barrier = Arc::new(std::sync::Barrier::new(9));
    let accepted = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let workers: Vec<_> = (0..8)
        .map(|_| {
            let pool = pool.clone();
            let barrier = barrier.clone();
            let accepted = accepted.clone();
            std::thread::spawn(move || {
                let mut ui = fixture();
                let scope = ui.root_scope();
                ui.set_task_pool(pool).unwrap();
                let result = ui.start_task(scope, 0, |context: UiTaskContext<u32>| {
                    context.complete(1).unwrap();
                });
                if result.is_ok() {
                    accepted.fetch_add(1, Ordering::SeqCst);
                }
                barrier.wait();
                barrier.wait();
                assert!(result.is_ok() || matches!(result, Err(Error::TaskCapacity)));
            })
        })
        .collect();
    barrier.wait();
    let snapshot = pool.stats();
    barrier.wait();
    assert_eq!(accepted.load(Ordering::SeqCst), 3);
    assert_eq!(snapshot.in_use, 3);
    assert_eq!(snapshot.peak, 3);
    assert_eq!(snapshot.rejected, 5);
    for worker in workers {
        worker.join().unwrap();
    }
    assert_eq!(pool.stats().in_use, 0);
}

#[test]
fn final_frame_failure_releases_provisional_slot_and_preserves_old_worker() {
    let mut ui = fixture();
    let root = ui.root_scope();
    let render = ui.root_render();
    let pool = UiTaskPool::new(2).unwrap();
    ui.set_task_pool(pool.clone()).unwrap();
    let (old, _worker) = hold(&mut ui, root, 0);
    ui.local_frame([100.0, 80.0]).unwrap();
    let launches = Rc::new(Cell::new(0));
    let count = launches.clone();
    assert!(matches!(
        ui.edit_local_frame(
            LayoutConstraints::viewport(100.0, 80.0),
            [100.0, 80.0],
            |_| Ok(()),
            |(), edit| {
                edit.start_task(root, 1, move |_: UiTaskContext<u32>| count.set(1))?;
                edit.properties().opacity(render, 2.0);
                Ok(())
            }
        ),
        Err(Error::InvalidOpacity)
    ));
    assert_eq!(launches.get(), 0);
    assert_eq!(pool.stats().in_use, 1);
    assert_eq!(old.status(), UiTaskStatus::Running);
    assert!(ui.task::<u32>(root, 1).unwrap().is_none());
}
