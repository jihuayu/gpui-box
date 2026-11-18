use super::*;
use std::{
    cell::{Cell, RefCell},
    panic::{AssertUnwindSafe, catch_unwind},
};
const VIEW: [f32; 2] = [100.0, 80.0];
fn fixed(w: f32, h: f32) -> LayoutStyle {
    LayoutStyle::column()
        .size(LayoutDimension::Px(w), LayoutDimension::Px(h))
        .flex_shrink(0.0)
}
struct Fixture {
    ui: LogicalUi,
    scope: ScopeId,
    node: NodeId,
    value: Signal<u32>,
    fail: Rc<Cell<u8>>,
    tasks: Rc<RefCell<Vec<UiTask<u32>>>>,
    workers: Rc<RefCell<Vec<UiTaskContext<u32>>>>,
    launches: Rc<Cell<usize>>,
    after: Rc<Cell<usize>>,
}
fn fixture(capacity: usize) -> Fixture {
    let mut ui = LogicalUi::new_layout(
        fixed(100.0, 80.0),
        0,
        BackendCapabilities {
            solid_color_patch: true,
        },
    )
    .unwrap();
    ui.set_task_pool(UiTaskPool::new(capacity).unwrap())
        .unwrap();
    let root = ui.root_scope();
    let host = ui.root_render();
    let scope = ui.reconcile_keys(root, &[1]).unwrap().order[0];
    let value = ui.state(scope, 0, || 0xff0000ffu32).unwrap();
    let fail = Rc::new(Cell::new(0));
    let tasks = Rc::new(RefCell::new(vec![]));
    let workers = Rc::new(RefCell::new(vec![]));
    let launches = Rc::new(Cell::new(0));
    let after = Rc::new(Cell::new(0));
    let (failure, output, work, count, business) = (
        fail.clone(),
        tasks.clone(),
        workers.clone(),
        launches.clone(),
        after.clone(),
    );
    let node = ui
        .edit_local_frame(
            LayoutConstraints::viewport(100.0, 80.0),
            VIEW,
            |_| Ok(()),
            |(), edit| {
                let node = edit.append_layout(scope, host, fixed(40.0, 30.0), 0)?;
                edit.bind(value, Property::color(node))?;
                let opacity = edit.computed(scope, 1, &[value.dependency()], move |i| {
                    Ok(if *i.read(value)? == 1 { 2.0 } else { 1.0 })
                })?;
                edit.bind(opacity, Property::opacity(node))?;
                let handler = edit.event_handler(
                    scope,
                    0,
                    EventHandler::new(move |event, cx| {
                        if event.kind != UiInputEventKind::Activate {
                            return Ok(());
                        }
                        let mode = failure.get();
                        if mode == 4 {
                            assert!(cx.cancel_task(7)?);
                            return Ok(());
                        }
                        if mode == 6 {
                            assert!(cx.cancel_task(7)?);
                            return Err(Error::InvalidStyle);
                        }
                        if mode == 5 {
                            assert!(cx.task::<u32>(7)?.is_some());
                            return Ok(());
                        }
                        let workers = work.clone();
                        let count = count.clone();
                        let task = match cx.start_task(7, move |context: UiTaskContext<u32>| {
                            count.set(count.get() + 1);
                            workers.borrow_mut().push(context);
                        }) {
                            Ok(task) => task,
                            Err(Error::TaskCapacity) => {
                                cx.set(value, 0xaaaa00ff)?;
                                return Ok(());
                            }
                            Err(error) => return Err(error),
                        };
                        assert_eq!(
                            cx.task::<u32>(7)?.unwrap().status(),
                            UiTaskStatus::PendingCommit
                        );
                        output.borrow_mut().push(task);
                        cx.set(value, if mode == 2 { 1 } else { 0x00ff00ff })?;
                        let business = business.clone();
                        cx.after_commit(move || business.set(business.get() + 1));
                        if mode == 1 {
                            return Err(Error::InvalidStyle);
                        }
                        if mode == 3 {
                            panic!("handler unwind");
                        }
                        Ok(())
                    }),
                )?;
                edit.interaction(
                    scope,
                    node,
                    Some(UiInteraction {
                        focus: UiFocusPolicy::Tab,
                        activate_handler: Some(handler),
                        ..Default::default()
                    }),
                )?;
                Ok(node)
            },
        )
        .unwrap()
        .0
        .result
        .value;
    Fixture {
        ui,
        scope,
        node,
        value,
        fail,
        tasks,
        workers,
        launches,
        after,
    }
}
fn router(ui: &LogicalUi) -> UiInputRouter {
    let mut r = UiInputRouter::new();
    r.adopt(ui.input_scene().unwrap(), UiInputEvidence::ImageInstalled)
        .unwrap();
    r
}
fn activate(ui: &mut LogicalUi, r: &mut UiInputRouter) -> Result<LogicalInputCommit, Error> {
    ui.handle_input(r, UiInput::PrimaryDown([5.0; 2]))?;
    ui.handle_input(r, UiInput::PrimaryUp([5.0; 2]))
}
#[test]
fn event_task_launch_and_result_share_scope_and_complete_owner_path() {
    run(false);
}
#[cfg(feature = "retained-wgpu")]
#[test]
#[ignore = "requires WGPU adapter; run explicitly"]
fn hardware_event_task_result_matches_complete_reference() {
    run(true);
}
fn run(hardware: bool) {
    let mut f = fixture(2);
    let mut router = router(&f.ui);
    let before = f.ui.published_local_frame().unwrap().clone();
    let commit = activate(&mut f.ui, &mut router).unwrap();
    assert_eq!(commit.delivered, 1);
    assert_eq!(f.launches.get(), 1);
    assert_eq!(f.after.get(), 1);
    assert_eq!(f.tasks.borrow()[0].status(), UiTaskStatus::Running);
    assert_eq!(
        f.ui.task::<u32>(f.scope, 7).unwrap().unwrap().status(),
        UiTaskStatus::Running
    );
    assert!(f.ui.task::<u32>(f.ui.root_scope(), 7).unwrap().is_none());
    assert_eq!(router.scene_id().unwrap().frame_revision, before.revision());
    let worker = f.workers.borrow_mut().pop().unwrap();
    std::thread::spawn(move || worker.complete(0x0000ffff).unwrap())
        .join()
        .unwrap();
    let task = f.tasks.borrow()[0].clone();
    assert!(matches!(
        f.ui.apply_task(&task, |value, tx| {
            tx.set(f.value, *value)?;
            Ok(())
        })
        .unwrap(),
        TaskApply::Applied(_)
    ));
    let frame = f.ui.published_local_frame().unwrap().clone();
    assert_eq!(
        frame.resolve_geometry(f.node).unwrap().fill_color,
        0x0000ffff
    );
    let mut reference = UiTree::new_layout(fixed(100.0, 80.0), 0).unwrap();
    reference.set_frame_build_policy(FrameBuildPolicy::Full);
    reference
        .append_layout(reference.root(), fixed(40.0, 30.0), 0x0000ffff)
        .unwrap();
    let reference = reference.frame(VIEW).unwrap();
    fn pixels(frame: FramePacket) -> Vec<u8> {
        let mut job = software::SoftwareJob::new(
            frame,
            software::SoftwareOptions {
                size: [100, 80],
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
    assert_eq!(pixels(frame.resolve()), pixels(reference.clone()));
    #[cfg(feature = "retained-wgpu")]
    if hardware {
        let mut gpu = wgpu::WgpuFrameRenderer::new(2).unwrap();
        eprintln!("event task adapter: {:?}", gpu.adapter_info());
        gpu.submit_local(&frame, [100, 80], 1.0).unwrap();
        assert_eq!(
            gpu.image().unwrap().as_raw(),
            gpu.reference_image(&reference, [100, 80], 1.0)
                .unwrap()
                .as_raw()
        );
    }
    #[cfg(not(feature = "retained-wgpu"))]
    assert!(!hardware);
    f.ui.adopt_input_scene(
        &mut router,
        f.ui.input_scene().unwrap(),
        UiInputEvidence::ImageInstalled,
    )
    .unwrap();
    assert_eq!(router.scene_id(), Some(f.ui.input_scene().unwrap().id()));
}
#[test]
fn callback_error_layout_failure_and_unwind_discard_event_task_without_launch() {
    for mode in [1, 2, 3] {
        let mut f = fixture(2);
        let mut r = router(&f.ui);
        f.fail.set(mode);
        f.ui.handle_input(&mut r, UiInput::PrimaryDown([5.0; 2]))
            .unwrap();
        let before = f.ui.published_local_frame().unwrap().clone();
        let captured = r.captured();
        let result = catch_unwind(AssertUnwindSafe(|| {
            f.ui.handle_input(&mut r, UiInput::PrimaryUp([5.0; 2]))
        }));
        assert!(match mode {
            3 => result.is_err(),
            _ => result.unwrap().is_err(),
        });
        assert_eq!(f.launches.get(), 0);
        assert_eq!(f.after.get(), 0);
        assert_eq!(f.ui.task_pool().stats().in_use, 0);
        assert!(f.ui.task::<u32>(f.scope, 7).unwrap().is_none());
        assert_eq!(r.captured(), captured);
        assert_eq!(*f.ui.read(f.value).unwrap(), 0xff0000ff);
        assert!(before.same(f.ui.published_local_frame().unwrap()));
        assert_eq!(f.tasks.borrow()[0].status(), UiTaskStatus::Cancelled);
    }
}
#[test]
fn event_busy_fallback_preserves_task_and_explicit_cancel_is_transactional() {
    let mut f = fixture(1);
    let mut r = router(&f.ui);
    activate(&mut f.ui, &mut r).unwrap();
    let old = f.tasks.borrow()[0].clone();
    activate(&mut f.ui, &mut r).unwrap();
    assert_eq!(f.launches.get(), 1);
    assert_eq!(old.status(), UiTaskStatus::Running);
    assert_eq!(*f.ui.read(f.value).unwrap(), 0xaaaa00ff);
    f.fail.set(4);
    activate(&mut f.ui, &mut r).unwrap();
    assert_eq!(old.status(), UiTaskStatus::Cancelled);
    assert_eq!(f.ui.task_pool().stats().in_use, 1);
    assert_eq!(f.workers.borrow_mut().pop().unwrap().complete(9), Err(9));
    assert_eq!(f.ui.task_pool().stats().in_use, 0);
}
#[test]
fn failed_event_replacement_keeps_live_old_task_and_discards_new_candidate() {
    let mut f = fixture(2);
    let mut r = router(&f.ui);
    activate(&mut f.ui, &mut r).unwrap();
    let old = f.tasks.borrow()[0].clone();
    f.fail.set(1);
    assert!(activate(&mut f.ui, &mut r).is_err());
    assert_eq!(old.status(), UiTaskStatus::Running);
    assert_eq!(f.tasks.borrow()[1].status(), UiTaskStatus::Cancelled);
    assert_eq!(f.launches.get(), 1);
    assert_eq!(f.ui.task_pool().stats().in_use, 1);
    f.fail.set(6);
    assert!(
        f.ui.handle_input(&mut r, UiInput::PrimaryUp([5.0; 2]))
            .is_err()
    );
    assert_eq!(old.status(), UiTaskStatus::Running);
    assert!(f.ui.task::<u32>(f.scope, 7).unwrap().is_some());
}
#[test]
fn standalone_dispatch_uses_same_event_task_staging_and_delayed_stale_event_never_starts() {
    let mut f = fixture(2);
    let mut r = router(&f.ui);
    r.route(UiInput::PrimaryDown([5.0; 2])).unwrap();
    let effects = r.route(UiInput::PrimaryUp([5.0; 2])).unwrap();
    let event = effects
        .dispatches
        .iter()
        .find(|e| e.kind == UiInputEventKind::Activate)
        .unwrap();
    assert!(f.ui.dispatch_event(event).unwrap().is_some());
    assert_eq!(f.launches.get(), 1);
    let root = f.ui.root_scope();
    f.ui.edit(
        |_| Ok(()),
        |(), edit| {
            edit.reconcile_keys(root, &[])?;
            Ok(())
        },
    )
    .unwrap();
    assert!(f.ui.dispatch_event(event).unwrap().is_none());
    assert_eq!(f.launches.get(), 1);
    assert_eq!(f.tasks.borrow()[0].status(), UiTaskStatus::Cancelled);
}

#[test]
fn event_task_retired_by_same_source_producer_never_launches() {
    let mut f = fixture(2);
    let root = f.ui.root_scope();
    let render = f.ui.root_render();
    let show = f.ui.state(root, 0, || true).unwrap();
    let node = f.node;
    let scope = f.scope;
    let tasks = f.tasks.clone();
    let launches = f.launches.clone();
    f.ui.edit_local_frame(
        LayoutConstraints::viewport(100.0, 80.0),
        VIEW,
        |_| Ok(()),
        |(), edit| {
            let handler = edit.event_handler(
                scope,
                0,
                EventHandler::new(move |event, cx| {
                    if event.kind == UiInputEventKind::Activate {
                        let count = launches.clone();
                        let task = cx.start_task(7, move |_: UiTaskContext<u32>| {
                            count.set(count.get() + 1)
                        })?;
                        tasks.borrow_mut().push(task);
                        cx.set(show, false)?;
                    }
                    Ok(())
                }),
            )?;
            edit.interaction(
                scope,
                node,
                Some(UiInteraction {
                    focus: UiFocusPolicy::Tab,
                    activate_handler: Some(handler),
                    ..Default::default()
                }),
            )?;
            edit.layout_producer(
                root,
                0,
                render,
                LayoutBuildKind::Content,
                &[],
                LayoutProducer::new(move |_, edit| {
                    if !*edit.read(show)? {
                        edit.reconcile_keys(root, &[])?;
                    }
                    Ok(())
                }),
            )?;
            Ok(())
        },
    )
    .unwrap();
    let mut r = router(&f.ui);
    activate(&mut f.ui, &mut r).unwrap();
    assert_eq!(f.launches.get(), 0);
    assert_eq!(f.tasks.borrow()[0].status(), UiTaskStatus::Cancelled);
    assert_eq!(f.ui.task_pool().stats().in_use, 0);
    assert_eq!(f.ui.published_local_frame().unwrap().len(), 1);
}

#[test]
fn bubbling_handlers_with_different_scope_owners_have_independent_task_keys() {
    let mut f = fixture(3);
    let root = f.ui.root_scope();
    let render = f.ui.root_render();
    let parent_worker = Rc::new(RefCell::new(None));
    let slot = parent_worker.clone();
    f.ui.edit_local_frame(
        LayoutConstraints::viewport(100.0, 80.0),
        VIEW,
        |_| Ok(()),
        |(), edit| {
            let handler = edit.event_handler(
                root,
                0,
                EventHandler::new(move |event, cx| {
                    if event.kind == UiInputEventKind::PointerUp {
                        let slot = slot.clone();
                        cx.start_task(7, move |context: UiTaskContext<u32>| {
                            *slot.borrow_mut() = Some(context)
                        })?;
                    }
                    Ok(())
                }),
            )?;
            edit.interaction(
                root,
                render,
                Some(UiInteraction {
                    pointer_handler: Some(handler),
                    ..Default::default()
                }),
            )?;
            Ok(())
        },
    )
    .unwrap();
    let mut r = router(&f.ui);
    activate(&mut f.ui, &mut r).unwrap();
    let parent = f.ui.task::<u32>(root, 7).unwrap().unwrap();
    let child = f.ui.task::<u32>(f.scope, 7).unwrap().unwrap();
    assert_eq!(parent.status(), UiTaskStatus::Running);
    assert_eq!(child.status(), UiTaskStatus::Running);
    f.ui.edit(
        |_| Ok(()),
        |(), edit| {
            edit.reconcile_keys(root, &[])?;
            Ok(())
        },
    )
    .unwrap();
    assert_eq!(child.status(), UiTaskStatus::Cancelled);
    assert_eq!(parent.status(), UiTaskStatus::Running);
}
