use super::*;
use std::{cell::Cell, rc::Rc};
const VIEW: [f32; 2] = [100.0, 80.0];
fn fixed(w: f32, h: f32) -> LayoutStyle {
    LayoutStyle::column()
        .size(LayoutDimension::Px(w), LayoutDimension::Px(h))
        .flex_shrink(0.0)
}
fn router(ui: &LogicalUi) -> UiInputRouter {
    let mut router = UiInputRouter::new();
    router
        .adopt(ui.input_scene().unwrap(), UiInputEvidence::ImageInstalled)
        .unwrap();
    router
}
struct Fixture {
    ui: LogicalUi,
    parent: NodeId,
    leaf: NodeId,
    count: Signal<u32>,
    state: ReadProp<style::StyleState>,
    log: Rc<RefCell<Vec<&'static str>>>,
    prevent: Rc<Cell<bool>>,
    stop: Rc<Cell<bool>>,
    fail: Rc<Cell<bool>>,
}
fn fixture() -> Fixture {
    let mut ui =
        LogicalUi::new_layout(fixed(100.0, 80.0), 0, BackendCapabilities::default()).unwrap();
    let scope = ui.root_scope();
    let root = ui.root_render();
    let count = ui.state(scope, 0, || 0u32).unwrap();
    let log = Rc::new(RefCell::new(vec![]));
    let prevent = Rc::new(Cell::new(false));
    let stop = Rc::new(Cell::new(false));
    let fail = Rc::new(Cell::new(false));
    let (parent, leaf, state) = ui
        .edit_local_frame(
            LayoutConstraints::viewport(VIEW[0], VIEW[1]),
            VIEW,
            |_| Ok(()),
            |(), edit| {
                let parent = edit.append_layout(
                    scope,
                    root,
                    fixed(60.0, 40.0).overflow(LayoutOverflow::Scroll, LayoutOverflow::Scroll),
                    0,
                )?;
                let leaf = edit.append_layout(scope, parent, fixed(60.0, 100.0), 0xff0000ff)?;
                let state = edit.interaction_state(scope, 1, leaf, false, false)?;
                let color = edit.computed(
                    scope,
                    2,
                    &[state.dependency(), count.dependency()],
                    move |i| {
                        Ok(if i.read_prop(state)?.pressed {
                            0x0000ffff
                        } else if *i.read(count)? > 0 {
                            0x00ff00ff
                        } else {
                            0xff0000ff
                        })
                    },
                )?;
                edit.bind(color, Property::color(leaf))?;
                let log_leaf = log.clone();
                let prevent_leaf = prevent.clone();
                let stop_leaf = stop.clone();
                let fail_leaf = fail.clone();
                let leaf_handler = edit.event_handler(
                    scope,
                    0,
                    EventHandler::new(move |event, cx| {
                        if matches!(
                            event.kind,
                            UiInputEventKind::PointerUp
                                | UiInputEventKind::Wheel
                                | UiInputEventKind::Focus
                        ) {
                            if event.kind == UiInputEventKind::Wheel {
                                assert_eq!(event.wheel_delta, Some([0.0, 15.0]));
                            }
                            let next = *cx.read(count)? + 1;
                            cx.set(count, next)?;
                            if prevent_leaf.get() {
                                assert!(cx.prevent_default());
                            }
                            if stop_leaf.get() {
                                cx.stop_propagation();
                            }
                            let log = log_leaf.clone();
                            cx.after_commit(move || log.borrow_mut().push("leaf"));
                            if fail_leaf.get() {
                                return Err(Error::InvalidStyle);
                            }
                        } else if event.kind == UiInputEventKind::Activate {
                            let log = log_leaf.clone();
                            cx.after_commit(move || log.borrow_mut().push("activate"));
                        }
                        Ok(())
                    }),
                )?;
                let log_parent = log.clone();
                let prevent_parent = prevent.clone();
                let parent_handler = edit.event_handler(
                    scope,
                    1,
                    EventHandler::new(move |event, cx| {
                        if matches!(
                            event.kind,
                            UiInputEventKind::PointerUp | UiInputEventKind::Wheel
                        ) {
                            assert_eq!(cx.default_prevented(), prevent_parent.get());
                            let next = *cx.read(count)? + 10;
                            cx.set(count, next)?;
                            let log = log_parent.clone();
                            cx.after_commit(move || log.borrow_mut().push("parent"));
                        }
                        Ok(())
                    }),
                )?;
                edit.interaction(
                    scope,
                    parent,
                    Some(UiInteraction {
                        pointer_handler: Some(parent_handler),
                        ..Default::default()
                    }),
                )?;
                edit.interaction(
                    scope,
                    leaf,
                    Some(UiInteraction {
                        focus: UiFocusPolicy::Tab,
                        pointer_handler: Some(leaf_handler),
                        focus_handler: Some(leaf_handler),
                        activate_handler: Some(leaf_handler),
                        ..Default::default()
                    }),
                )?;
                Ok((parent, leaf, state))
            },
        )
        .unwrap()
        .0
        .result
        .value;
    Fixture {
        ui,
        parent,
        leaf,
        count,
        state,
        log,
        prevent,
        stop,
        fail,
    }
}
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
fn reference(color: u32, scroll: f32) -> FramePacket {
    let mut tree = UiTree::new_layout(fixed(100.0, 80.0), 0).unwrap();
    tree.set_frame_build_policy(FrameBuildPolicy::Full);
    let parent = tree
        .append_layout(tree.root(), fixed(60.0, 40.0), 0)
        .unwrap();
    let leaf = tree
        .append_layout(parent, fixed(60.0, 100.0), color)
        .unwrap();
    let mut tx = Transaction::default();
    tx.translation(leaf, [0.0, -scroll]);
    tx.clip(
        parent,
        Some(Rect {
            x: 0.0,
            y: 0.0,
            width: 60.0,
            height: 40.0,
        }),
    );
    tree.commit(tx, BackendCapabilities::default()).unwrap();
    tree.frame(VIEW).unwrap()
}
#[test]
fn input_pump_pointer_callbacks_feedback_and_wheel_match_complete_reference() {
    run(false);
}
#[cfg(feature = "retained-wgpu")]
#[test]
#[ignore = "requires WGPU adapter; run explicitly"]
fn hardware_input_pump_matches_complete_reference() {
    run(true);
}
fn run(hardware: bool) {
    let Fixture {
        mut ui,
        parent,
        leaf,
        count,
        state,
        log,
        ..
    } = fixture();
    let mut router = router(&ui);
    #[cfg(feature = "retained-wgpu")]
    let mut gpu = hardware.then(|| wgpu::WgpuFrameRenderer::new(2).unwrap());
    #[cfg(feature = "retained-wgpu")]
    if let Some(gpu) = &gpu {
        eprintln!("input pump adapter: {:?}", gpu.adapter_info());
    }
    #[cfg(not(feature = "retained-wgpu"))]
    assert!(!hardware);
    let old = ui.published_local_frame().unwrap().clone();
    let expected = pixels(old.resolve());
    for (event, color, scroll, expected_count) in [
        (UiInput::PrimaryDown([5.0; 2]), 0x0000ffff, 0.0, 1),
        (UiInput::PrimaryUp([5.0; 2]), 0x00ff00ff, 0.0, 12),
        (
            UiInput::Wheel {
                position: [5.0; 2],
                delta: [0.0, 15.0],
            },
            0x00ff00ff,
            15.0,
            23,
        ),
    ] {
        let installed = router.scene_id();
        let commit = ui.handle_input(&mut router, event).unwrap();
        assert_eq!(
            router.scene_id(),
            installed,
            "processing never auto-installs new geometry"
        );
        assert_eq!(*ui.read(count).unwrap(), expected_count);
        let full = reference(color, scroll);
        assert_eq!(pixels(commit.frame.resolve()), pixels(full.clone()));
        assert_eq!(
            ui.input_scene()
                .unwrap()
                .scroll_state(public_id(parent))
                .unwrap()
                .offset,
            [0.0, scroll]
        );
        #[cfg(feature = "retained-wgpu")]
        if let Some(gpu) = &mut gpu {
            gpu.submit_local(&commit.frame, [100, 80], 1.0).unwrap();
            assert_eq!(
                gpu.image().unwrap().as_raw(),
                gpu.reference_image(&full, [100, 80], 1.0).unwrap().as_raw()
            );
        }
        let scene = ui.input_scene().unwrap();
        ui.adopt_input_scene(&mut router, scene, UiInputEvidence::ImageInstalled)
            .unwrap();
    }
    assert!(!ui.read_prop(state).unwrap().pressed);
    assert!(ui.renders(ui.root_scope()).unwrap().contains(&leaf));
    assert_eq!(
        &*log.borrow(),
        &["leaf", "leaf", "parent", "activate", "leaf", "parent"]
    );
    drop(ui);
    assert_eq!(pixels(old.resolve()), expected);
    #[cfg(feature = "retained-wgpu")]
    if let Some(gpu) = &mut gpu {
        gpu.finish().unwrap();
    }
}
#[test]
fn input_pump_stop_propagation_does_not_cancel_wheel_default() {
    let Fixture {
        mut ui,
        parent,
        count,
        stop,
        log,
        ..
    } = fixture();
    stop.set(true);
    let mut router = router(&ui);
    let result = ui
        .handle_input(
            &mut router,
            UiInput::Wheel {
                position: [5.0; 2],
                delta: [0.0, 15.0],
            },
        )
        .unwrap();
    assert_eq!(result.stopped, 1);
    assert_eq!(result.prevented, 0);
    assert_eq!(*ui.read(count).unwrap(), 1);
    assert_eq!(result.outcomes[0].consumed, [0.0, 15.0]);
    assert_eq!(&*log.borrow(), &["leaf"]);
    assert_eq!(
        ui.input_scene()
            .unwrap()
            .scroll_state(public_id(parent))
            .unwrap()
            .offset,
        [0.0, 15.0]
    );
}
#[test]
fn input_pump_prevent_default_preserves_bubbling_and_focus_transfer() {
    let Fixture {
        mut ui,
        parent,
        leaf,
        count,
        prevent,
        log,
        ..
    } = fixture();
    prevent.set(true);
    let mut router = router(&ui);
    // Focus transfer already happened in router; prevent_default only cancels
    // reveal. Pointer notifications and state remain independent defaults.
    let focus = ui
        .handle_input(&mut router, UiInput::Focus(Some(public_id(leaf))))
        .unwrap();
    assert_eq!(focus.prevented, 1);
    assert_eq!(router.focused(), Some(public_id(leaf)));
    let wheel = ui
        .handle_input(
            &mut router,
            UiInput::Wheel {
                position: [5.0; 2],
                delta: [0.0, 15.0],
            },
        )
        .unwrap();
    assert_eq!(wheel.prevented, 1);
    assert_eq!(wheel.outcomes[0].consumed, [0.0; 2]);
    assert_eq!(wheel.outcomes[0].remaining, [0.0, 15.0]);
    assert_eq!(*ui.read(count).unwrap(), 12);
    assert_eq!(&*log.borrow(), &["leaf", "leaf", "parent"]);
    assert_eq!(
        ui.input_scene()
            .unwrap()
            .scroll_state(public_id(parent))
            .unwrap()
            .offset,
        [0.0; 2]
    );
}
#[test]
fn input_pump_prevent_pointer_up_suppresses_activation_only() {
    let Fixture {
        mut ui,
        prevent,
        log,
        ..
    } = fixture();
    let mut router = router(&ui);
    ui.handle_input(&mut router, UiInput::PrimaryDown([5.0; 2]))
        .unwrap();
    prevent.set(true);
    let result = ui
        .handle_input(&mut router, UiInput::PrimaryUp([5.0; 2]))
        .unwrap();
    assert_eq!(result.prevented, 1);
    assert_eq!(result.ignored, 1);
    assert!(!log.borrow().contains(&"activate"));
    assert_eq!(router.captured(), None);
    assert!(log.borrow().contains(&"parent"));
}
#[test]
fn input_pump_error_rolls_back_source_feedback_capture_and_post_commit_work() {
    let Fixture {
        mut ui,
        count,
        state,
        fail,
        log,
        ..
    } = fixture();
    let mut router = router(&ui);
    let before = ui.published_local_frame().unwrap().clone();
    fail.set(true);
    assert!(matches!(
        ui.handle_input(&mut router, UiInput::PrimaryDown([5.0; 2])),
        Err(Error::InvalidStyle)
    ));
    assert_eq!(*ui.read(count).unwrap(), 0);
    assert!(!ui.read_prop(state).unwrap().pressed);
    assert_eq!(router.captured(), None);
    assert_eq!(router.focused(), None);
    assert_eq!(router.hovered(), None);
    assert!(log.borrow().is_empty());
    assert!(before.same(ui.published_local_frame().unwrap()));
    fail.set(false);
    ui.handle_input(&mut router, UiInput::PrimaryDown([5.0; 2]))
        .unwrap();
    assert!(router.captured().is_some());
    assert_eq!(*ui.read(count).unwrap(), 1);
}
#[test]
fn input_pump_final_frame_failure_rolls_back_even_successful_callbacks() {
    let Fixture {
        mut ui,
        count,
        state,
        log,
        ..
    } = fixture();
    let scope = ui.root_scope();
    let root = ui.root_render();
    let invalid = ui
        .computed(scope, 9, &[count.dependency()], move |i| {
            Ok(if *i.read(count)? > 0 { f32::NAN } else { 1.0 })
        })
        .unwrap();
    ui.bind(invalid, Property::opacity(root)).unwrap();
    ui.local_frame(VIEW).unwrap();
    let before = ui.published_local_frame().unwrap().clone();
    let mut router = router(&ui);
    assert!(matches!(
        ui.handle_input(&mut router, UiInput::PrimaryDown([5.0; 2])),
        Err(Error::InvalidOpacity)
    ));
    assert_eq!(*ui.read(count).unwrap(), 0);
    assert!(!ui.read_prop(state).unwrap().pressed);
    assert_eq!(router.captured(), None);
    assert!(log.borrow().is_empty());
    assert!(before.same(ui.published_local_frame().unwrap()));
}

#[test]
fn input_pump_post_commit_panic_keeps_router_aligned_with_committed_ui() {
    let Fixture {
        mut ui,
        leaf,
        count,
        ..
    } = fixture();
    let scope = ui.root_scope();
    ui.edit(
        |_| Ok(()),
        |(), edit| {
            let handler = edit.event_handler(
                scope,
                3,
                EventHandler::new(move |event, cx| {
                    if event.kind == UiInputEventKind::PointerDown {
                        cx.set(count, 7)?;
                        cx.after_commit(|| panic!("expected post-commit panic"));
                    }
                    Ok(())
                }),
            )?;
            edit.interaction(
                scope,
                leaf,
                Some(UiInteraction {
                    pointer_handler: Some(handler),
                    ..Default::default()
                }),
            )
        },
    )
    .unwrap();
    let mut router = router(&ui);
    assert!(
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(
            || ui.handle_input(&mut router, UiInput::PrimaryDown([5.0; 2]))
        ))
        .is_err()
    );
    assert_eq!(*ui.read(count).unwrap(), 7);
    assert_eq!(router.captured(), Some(public_id(leaf)));
    assert_eq!(
        pixels(ui.published_local_frame().unwrap().resolve()),
        pixels(reference(0x0000ffff, 0.0))
    );
}

#[test]
fn input_pump_final_disabled_default_is_skipped_without_losing_callback_state() {
    let Fixture {
        mut ui,
        parent,
        leaf,
        count,
        log,
        ..
    } = fixture();
    let scope = ui.root_scope();
    let root = ui.root_render();
    ui.edit_local_frame(
        LayoutConstraints::viewport(VIEW[0], VIEW[1]),
        VIEW,
        |_| Ok(()),
        |(), edit| {
            edit.layout_producer(
                scope,
                0,
                root,
                LayoutBuildKind::Content,
                &[],
                LayoutProducer::new(move |_, edit| {
                    let disabled = *edit.read(count)? > 0;
                    edit.interaction(
                        scope,
                        parent,
                        Some(UiInteraction {
                            disabled,
                            ..Default::default()
                        }),
                    )?;
                    Ok(())
                }),
            )?;
            Ok(())
        },
    )
    .unwrap();
    let mut router = router(&ui);
    let commit = ui
        .handle_input(
            &mut router,
            UiInput::Wheel {
                position: [5.0; 2],
                delta: [0.0, 15.0],
            },
        )
        .unwrap();
    // Parent callback was removed by the producer at setup; leaf still runs.
    assert_eq!(*ui.read(count).unwrap(), 1);
    assert_eq!(&*log.borrow(), &["leaf"]);
    assert_eq!(commit.outcomes[0].consumed, [0.0; 2]);
    assert_eq!(commit.outcomes[0].remaining, [0.0, 15.0]);
    assert_eq!(
        ui.input_scene()
            .unwrap()
            .scroll_state(public_id(parent))
            .unwrap()
            .offset,
        [0.0; 2]
    );
    assert!(!ui.input_scene().unwrap().graph.eligible(public_id(leaf)));
}

#[test]
fn input_pump_malformed_default_rejects_before_callbacks_and_never_advances_router() {
    let Fixture {
        mut ui, count, log, ..
    } = fixture();
    let mut router = router(&ui);
    let mut candidate = router.clone();
    let mut effects = candidate
        .route(UiInput::Wheel {
            position: [5.0; 2],
            delta: [0.0, 15.0],
        })
        .unwrap();
    effects.wheel_scrolls[0].scene.runtime += 1;
    assert!(matches!(
        ui.process_input_effects(&effects),
        Err(Error::Input(UiInputError::WrongRuntime))
    ));
    assert_eq!(*ui.read(count).unwrap(), 0);
    assert!(log.borrow().is_empty());
    assert_eq!(router.hovered(), None);
    assert!(matches!(
        ui.handle_input(&mut router, UiInput::PrimaryDown([f32::NAN, 0.0])),
        Err(Error::Input(UiInputError::InvalidPosition))
    ));
    assert_eq!(router.captured(), None);
}
