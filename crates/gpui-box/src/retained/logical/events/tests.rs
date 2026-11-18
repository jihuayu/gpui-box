use super::*;
use std::cell::{Cell, RefCell};
fn fixture() -> LogicalUi {
    LogicalUi::new_layout(LayoutStyle::column(), 0, BackendCapabilities::default()).unwrap()
}
fn constraints() -> LayoutConstraints {
    LayoutConstraints::new(
        LayoutAvailableSpace::Unbounded,
        LayoutAvailableSpace::Unbounded,
    )
}
fn fixed(w: u32) -> LayoutStyle {
    LayoutStyle::column()
        .size(LayoutDimension::Px(w as f32), LayoutDimension::Px(20.0))
        .flex_shrink(0.0)
}
fn activate(ui: &LogicalUi) -> UiInputDispatch {
    let mut router = UiInputRouter::new();
    router
        .adopt(ui.input_scene().unwrap(), UiInputEvidence::ImageInstalled)
        .unwrap();
    router.route(UiInput::PrimaryDown([5.0, 5.0])).unwrap();
    router
        .route(UiInput::PrimaryUp([5.0, 5.0]))
        .unwrap()
        .dispatches
        .into_iter()
        .find(|e| e.kind == UiInputEventKind::Activate)
        .unwrap()
}
fn pixels(frame: FramePacket) -> Vec<u8> {
    let mut job = software::SoftwareJob::new(
        frame,
        software::SoftwareOptions {
            size: [180, 180],
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
fn scoped_events_state_and_post_commit_match_complete_reference() {
    run(false);
}
#[cfg(feature = "retained-wgpu")]
#[test]
#[ignore = "requires WGPU adapter; run explicitly"]
fn hardware_scoped_events_match_complete_reference() {
    run(true);
}
fn run(hardware: bool) {
    let mut ui = fixture();
    let owner = ui.root_scope();
    let host = ui.root_render();
    let width = ui.state(owner, 0, || 20u32).unwrap();
    let color = ui.state(owner, 1, || 0xff0000ffu32).unwrap();
    let effects = Rc::new(RefCell::new(vec![]));
    let log = effects.clone();
    let handler = EventHandler::new(move |event, cx| {
        assert_eq!(event.kind, UiInputEventKind::Activate);
        let next = *cx.read(width)? + 10;
        cx.set(width, next)?;
        cx.set(color, if next == 30 { 0x00ff00ff } else { 0x0000ffff })?;
        let log = log.clone();
        cx.after_commit(move || log.borrow_mut().push(next));
        Ok(())
    });
    let node = ui
        .edit_local_frame(
            constraints(),
            [180.0; 2],
            |_| Ok(()),
            |(), edit| {
                let node = edit.append_layout(owner, host, fixed(20), 0)?;
                let style = edit.computed(owner, 2, &[width.dependency()], move |i| {
                    Ok(fixed(*i.read(width)?))
                })?;
                edit.bind(style, Property::layout_style(node))?;
                edit.bind(color, Property::color(node))?;
                let id = edit.event_handler(owner, 0, handler)?;
                edit.interaction(
                    owner,
                    node,
                    Some(UiInteraction {
                        focus: UiFocusPolicy::Tab,
                        activate_handler: Some(id),
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
    assert!(!ui.has_layout_session());
    #[cfg(feature = "retained-wgpu")]
    let mut gpu = hardware.then(|| wgpu::WgpuFrameRenderer::new(2).unwrap());
    #[cfg(feature = "retained-wgpu")]
    if let Some(gpu) = &gpu {
        eprintln!("scoped events adapter: {:?}", gpu.adapter_info());
    }
    #[cfg(not(feature = "retained-wgpu"))]
    assert!(!hardware);
    let mut old = None;
    for (index, (w, c)) in [(20, 0xff0000ff), (30, 0x00ff00ff), (40, 0x0000ffff)]
        .into_iter()
        .enumerate()
    {
        if index > 0 {
            let event = activate(&ui);
            assert!(ui.dispatch_event(&event).unwrap().is_some());
        }
        assert_eq!(*ui.read(width).unwrap(), w);
        assert_eq!(effects.borrow().len(), index);
        assert_eq!(ui.layout_result(node).unwrap().size, [w as f32, 20.0]);
        let mut full = UiTree::new_layout(LayoutStyle::column(), 0).unwrap();
        full.set_frame_build_policy(FrameBuildPolicy::Full);
        full.append_layout(full.root(), fixed(w), c).unwrap();
        let reference = full.frame_in_viewport(constraints(), [180.0; 2]).unwrap();
        let frame = ui.published_local_frame().unwrap().clone();
        assert_eq!(pixels(frame.resolve()), pixels(reference.clone()));
        #[cfg(feature = "retained-wgpu")]
        if let Some(gpu) = &mut gpu {
            gpu.submit_local(&frame, [180, 180], 1.0).unwrap();
            assert_eq!(
                gpu.image().unwrap().as_raw(),
                gpu.reference_image(&reference, [180, 180], 1.0)
                    .unwrap()
                    .as_raw()
            );
        }
        old.get_or_insert_with(|| (frame.clone(), pixels(frame.resolve())));
    }
    drop(ui);
    let (frame, expected) = old.unwrap();
    assert_eq!(pixels(frame.resolve()), expected);
    #[cfg(feature = "retained-wgpu")]
    if let Some(gpu) = &mut gpu {
        gpu.finish().unwrap();
    }
}
#[test]
fn scoped_events_failed_source_or_final_frame_never_runs_business_effect() {
    for final_failure in [false, true] {
        let mut ui = fixture();
        let owner = ui.root_scope();
        let host = ui.root_render();
        let value = ui.state(owner, 0, || 0u32).unwrap();
        let effects = Rc::new(Cell::new(0));
        let count = effects.clone();
        ui.edit_local_frame(
            constraints(),
            [180.0; 2],
            |_| Ok(()),
            |(), edit| {
                let node = edit.append_layout(owner, host, fixed(20), 0xff0000ff)?;
                let opacity = edit.computed(owner, 1, &[value.dependency()], move |i| {
                    Ok(if *i.read(value)? > 0 { f32::NAN } else { 1.0 })
                })?;
                if final_failure {
                    edit.bind(opacity, Property::opacity(node))?;
                }
                let handler = edit.event_handler(
                    owner,
                    0,
                    EventHandler::new(move |_, cx| {
                        cx.set(value, 1)?;
                        let count = count.clone();
                        cx.after_commit(move || count.set(count.get() + 1));
                        if final_failure {
                            Ok(())
                        } else {
                            Err(Error::InvalidStyle)
                        }
                    }),
                )?;
                edit.interaction(
                    owner,
                    node,
                    Some(UiInteraction {
                        activate_handler: Some(handler),
                        ..Default::default()
                    }),
                )?;
                Ok(())
            },
        )
        .unwrap();
        let before = ui.published_local_frame().unwrap().clone();
        let event = activate(&ui);
        assert!(ui.dispatch_event(&event).is_err());
        assert_eq!(*ui.read(value).unwrap(), 0);
        assert_eq!(effects.get(), 0);
        assert!(before.same(ui.published_local_frame().unwrap()));
    }
}
#[test]
fn scoped_events_retired_handler_and_scope_generation_cannot_receive_old_click() {
    let mut ui = fixture();
    let owner = ui.root_scope();
    let host = ui.root_render();
    let calls = Rc::new(Cell::new(0));
    let child = ui.reconcile_keys(owner, &[1]).unwrap().order[0];
    let count = calls.clone();
    ui.edit_local_frame(
        constraints(),
        [180.0; 2],
        |_| Ok(()),
        |(), edit| {
            let node = edit.append_layout(child, host, fixed(20), 0xff0000ff)?;
            let handler = edit.event_handler(
                child,
                0,
                EventHandler::new(move |_, _| {
                    count.set(count.get() + 1);
                    Ok(())
                }),
            )?;
            edit.interaction(
                child,
                node,
                Some(UiInteraction {
                    activate_handler: Some(handler),
                    ..Default::default()
                }),
            )?;
            Ok(())
        },
    )
    .unwrap();
    let old = activate(&ui);
    ui.reconcile_keys(owner, &[]).unwrap();
    ui.frame_in_viewport(constraints(), [180.0; 2]).unwrap();
    assert!(ui.dispatch_event(&old).unwrap().is_none());
    assert_eq!(calls.get(), 0);
    assert!(ui.events.handlers.is_empty());
    assert!(ui.events.policies.is_empty());
    let next = ui.reconcile_keys(owner, &[1]).unwrap().order[0];
    assert_ne!(next, child);
}
#[test]
fn scoped_events_replacement_requires_new_id_and_policy_update() {
    let mut ui = fixture();
    let owner = ui.root_scope();
    let host = ui.root_render();
    let handler = EventHandler::new(|_, _| Ok(()));
    let (node, id) = ui
        .edit_local_frame(
            constraints(),
            [180.0; 2],
            |_| Ok(()),
            |(), edit| {
                let node = edit.append_layout(owner, host, fixed(20), 0xff0000ff)?;
                let id = edit.event_handler(owner, 0, handler.clone())?;
                edit.interaction(
                    owner,
                    node,
                    Some(UiInteraction {
                        activate_handler: Some(id),
                        ..Default::default()
                    }),
                )?;
                Ok((node, id))
            },
        )
        .unwrap()
        .0
        .result
        .value;
    let old = activate(&ui);
    assert_eq!(
        ui.edit(|_| Ok(()), |(), edit| edit.event_handler(owner, 0, handler))
            .unwrap()
            .result
            .value,
        id
    );
    assert!(matches!(
        ui.edit(
            |_| Ok(()),
            |(), edit| edit.event_handler(owner, 0, EventHandler::new(|_, _| Ok(())))
        ),
        Err(Error::InvalidEventHandler)
    ));
    let new = ui
        .edit(
            |_| Ok(()),
            |(), edit| {
                let new = edit.event_handler(owner, 0, EventHandler::new(|_, _| Ok(())))?;
                edit.interaction(
                    owner,
                    node,
                    Some(UiInteraction {
                        activate_handler: Some(new),
                        ..Default::default()
                    }),
                )?;
                Ok(new)
            },
        )
        .unwrap()
        .result
        .value;
    assert_ne!(id, new);
    assert!(ui.dispatch_event(&old).unwrap().is_none());
}
#[test]
fn scoped_events_final_failure_rolls_back_registry_and_policy() {
    let mut ui = fixture();
    let owner = ui.root_scope();
    let host = ui.root_render();
    assert!(matches!(
        ui.edit(
            |_| Ok(()),
            |(), edit| {
                let id = edit.event_handler(owner, 0, EventHandler::new(|_, _| Ok(())))?;
                edit.interaction(
                    owner,
                    host,
                    Some(UiInteraction {
                        activate_handler: Some(id),
                        ..Default::default()
                    }),
                )?;
                edit.properties().opacity(host, f32::NAN);
                Ok(())
            }
        ),
        Err(Error::InvalidOpacity)
    ));
    assert!(ui.events.handlers.is_empty());
    assert!(ui.events.policies.is_empty());
}

#[test]
fn scoped_events_keyboard_activation_and_disabled_or_reparented_dispatch_validation() {
    let mut ui = fixture();
    let owner = ui.root_scope();
    let host = ui.root_render();
    let calls = Rc::new(Cell::new(0));
    let count = calls.clone();
    let (node, other, id) = ui
        .edit_local_frame(
            constraints(),
            [180.0; 2],
            |_| Ok(()),
            |(), edit| {
                let node = edit.append_layout(owner, host, fixed(20), 0xff0000ff)?;
                let other = edit.append_layout(owner, host, LayoutStyle::column(), 0)?;
                let id = edit.event_handler(
                    owner,
                    0,
                    EventHandler::new(move |_, _| {
                        count.set(count.get() + 1);
                        Ok(())
                    }),
                )?;
                edit.interaction(
                    owner,
                    node,
                    Some(UiInteraction {
                        focus: UiFocusPolicy::Tab,
                        activate_handler: Some(id),
                        ..Default::default()
                    }),
                )?;
                Ok((node, other, id))
            },
        )
        .unwrap()
        .0
        .result
        .value;
    let mut router = UiInputRouter::new();
    router
        .adopt(ui.input_scene().unwrap(), UiInputEvidence::Published)
        .unwrap();
    router.route(UiInput::Tab { backwards: false }).unwrap();
    let enter = router
        .route(UiInput::KeyDown {
            key: UiActivationKey::Enter,
            repeat: false,
        })
        .unwrap()
        .dispatches
        .into_iter()
        .find(|e| e.kind == UiInputEventKind::Activate)
        .unwrap();
    ui.dispatch_event(&enter).unwrap().unwrap();
    assert_eq!(calls.get(), 1);
    router
        .route(UiInput::KeyUp(UiActivationKey::Enter))
        .unwrap();
    assert!(
        router
            .route(UiInput::KeyDown {
                key: UiActivationKey::Space,
                repeat: false
            })
            .unwrap()
            .dispatches
            .is_empty()
    );
    let space = router
        .route(UiInput::KeyUp(UiActivationKey::Space))
        .unwrap()
        .dispatches
        .into_iter()
        .find(|e| e.kind == UiInputEventKind::Activate)
        .unwrap();
    ui.dispatch_event(&space).unwrap().unwrap();
    assert_eq!(calls.get(), 2);
    ui.edit(
        |_| Ok(()),
        |(), edit| {
            edit.interaction(
                owner,
                node,
                Some(UiInteraction {
                    disabled: true,
                    focus: UiFocusPolicy::Tab,
                    activate_handler: Some(id),
                    ..Default::default()
                }),
            )
        },
    )
    .unwrap();
    assert!(ui.dispatch_event(&enter).unwrap().is_none());
    assert_eq!(calls.get(), 2);
    ui.edit(
        |_| Ok(()),
        |(), edit| {
            edit.interaction(
                owner,
                node,
                Some(UiInteraction {
                    focus: UiFocusPolicy::Tab,
                    activate_handler: Some(id),
                    ..Default::default()
                }),
            )
        },
    )
    .unwrap();
    let old = activate(&ui);
    ui.reparent_render(node, other).unwrap();
    ui.frame_in_viewport(constraints(), [180.0; 2]).unwrap();
    assert!(ui.dispatch_event(&old).unwrap().is_none());
    assert_eq!(calls.get(), 2);
}

#[test]
fn scoped_events_partial_build_failure_preserves_old_callback_and_rejects_unowned_handler() {
    let mut ui = fixture();
    let owner = ui.root_scope();
    let host = ui.root_render();
    let child = ui.reconcile_keys(owner, &[1]).unwrap().order[0];
    let releases = Rc::new(());
    let held = releases.clone();
    let node = ui
        .edit_local_frame(
            constraints(),
            [180.0; 2],
            |_| Ok(()),
            |(), edit| {
                let node = edit.append_layout(child, host, fixed(20), 0xff0000ff)?;
                let id = edit.event_handler(
                    child,
                    0,
                    EventHandler::new(move |_, _| {
                        let _ = &held;
                        Ok(())
                    }),
                )?;
                edit.interaction(
                    child,
                    node,
                    Some(UiInteraction {
                        activate_handler: Some(id),
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
    let old = activate(&ui);
    assert_eq!(Rc::strong_count(&releases), 2);
    assert!(matches!(
        ui.edit(
            |_| Ok(()),
            |(), edit| {
                edit.reconcile_keys(owner, &[])?;
                edit.properties().opacity(host, f32::NAN);
                Ok(())
            }
        ),
        Err(Error::InvalidOpacity)
    ));
    assert_eq!(Rc::strong_count(&releases), 2);
    assert!(ui.dispatch_event(&old).unwrap().is_some());
    assert!(matches!(
        ui.edit(
            |_| Ok(()),
            |(), edit| {
                let id = edit.event_handler(owner, 0, EventHandler::new(|_, _| Ok(())))?;
                edit.interaction(
                    child,
                    node,
                    Some(UiInteraction {
                        activate_handler: Some(id),
                        ..Default::default()
                    }),
                )
            }
        ),
        Err(Error::InvalidEventHandler)
    ));
    ui.edit(|_| Ok(()), |(), edit| edit.reconcile_keys(owner, &[]))
        .unwrap();
    assert_eq!(Rc::strong_count(&releases), 1);
    assert!(ui.events.handlers.is_empty());
    assert!(ui.events.policies.is_empty());
}

#[test]
fn scoped_events_handler_panic_rolls_back_ui_and_discards_post_commit_work() {
    let mut ui = fixture();
    let owner = ui.root_scope();
    let host = ui.root_render();
    let source = ui.state(owner, 0, || 0u32).unwrap();
    let calls = Rc::new(Cell::new(0));
    let count = calls.clone();
    ui.edit_local_frame(
        constraints(),
        [180.0; 2],
        |_| Ok(()),
        |(), edit| {
            let node = edit.append_layout(owner, host, fixed(20), 0xff0000ff)?;
            let id = edit.event_handler(
                owner,
                0,
                EventHandler::new(move |_, cx| {
                    cx.set(source, 1)?;
                    let count = count.clone();
                    cx.after_commit(move || count.set(1));
                    panic!("expected event handler panic");
                }),
            )?;
            edit.interaction(
                owner,
                node,
                Some(UiInteraction {
                    activate_handler: Some(id),
                    ..Default::default()
                }),
            )?;
            Ok(())
        },
    )
    .unwrap();
    let before = ui.published_local_frame().unwrap().clone();
    let event = activate(&ui);
    assert!(
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| ui.dispatch_event(&event)))
            .is_err()
    );
    assert_eq!(*ui.read(source).unwrap(), 0);
    assert_eq!(calls.get(), 0);
    assert!(before.same(ui.published_local_frame().unwrap()));
}
