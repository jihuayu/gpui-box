use super::*;

const VIEW: [f32; 2] = [160.0, 100.0];
fn fixed(w: f32, h: f32) -> LayoutStyle {
    LayoutStyle::column()
        .size(LayoutDimension::Px(w), LayoutDimension::Px(h))
        .flex_shrink(0.0)
}
struct Fixture {
    ui: LogicalUi,
    outer: NodeId,
    inner: NodeId,
    target: NodeId,
}
fn fixture(reveal: bool) -> Fixture {
    let mut ui =
        LogicalUi::new_layout(fixed(160.0, 100.0), 0, BackendCapabilities::default()).unwrap();
    let scope = ui.root_scope();
    let root = ui.root_render();
    let (outer, inner, target) = ui
        .edit_local_frame(
            LayoutConstraints::viewport(VIEW[0], VIEW[1]),
            VIEW,
            |_| Ok(()),
            |(), edit| {
                let outer = edit.append_layout(
                    scope,
                    root,
                    fixed(100.0, 60.0).overflow(LayoutOverflow::Scroll, LayoutOverflow::Scroll),
                    0x334455ff,
                )?;
                if reveal {
                    edit.append_layout(scope, outer, fixed(100.0, 80.0), 0)?;
                }
                let inner = edit.append_layout(
                    scope,
                    outer,
                    fixed(90.0, 40.0).overflow(LayoutOverflow::Scroll, LayoutOverflow::Scroll),
                    0,
                )?;
                edit.append_layout(scope, inner, fixed(120.0, 70.0), 0xff0000ff)?;
                let target = edit.append_layout(scope, inner, fixed(90.0, 20.0), 0x00ff00ff)?;
                edit.append_layout(scope, outer, fixed(120.0, 100.0), 0)?;
                edit.properties().opacity(outer, 0.8);
                edit.interaction(
                    scope,
                    target,
                    Some(UiInteraction {
                        focus: UiFocusPolicy::Tab,
                        ..Default::default()
                    }),
                )?;
                Ok((outer, inner, target))
            },
        )
        .unwrap()
        .0
        .result
        .value;
    Fixture {
        ui,
        outer,
        inner,
        target,
    }
}
fn router(ui: &LogicalUi) -> UiInputRouter {
    let mut router = UiInputRouter::new();
    router
        .adopt(ui.input_scene().unwrap(), UiInputEvidence::ImageInstalled)
        .unwrap();
    router
}
fn wheel(router: &mut UiInputRouter, delta: [f32; 2]) -> UiInputEffects {
    router
        .route(UiInput::Wheel {
            position: [10.0; 2],
            delta,
        })
        .unwrap()
}
fn offset(ui: &LogicalUi, node: NodeId) -> [f32; 2] {
    ui.ui.tree().node(node).unwrap().scroll
}
fn pixels(frame: FramePacket) -> Vec<u8> {
    let mut job = software::SoftwareJob::new(
        frame,
        software::SoftwareOptions {
            size: [160, 100],
            scale: 1.0,
            background: 0,
            effects: pacing::SoftwareEffects::Exact,
            max_bytes: 2 * 1024 * 1024,
        },
    )
    .unwrap();
    while !job.step(4096).unwrap() {}
    job.into_frame().unwrap().pixels().to_vec()
}
// Independent numeric full tree: authored child translations and explicit
// clips, with no overflow/default wheel/reveal algorithm in the reference.
fn reference(
    reveal: bool,
    inner_offset: [f32; 2],
    outer_offset: [f32; 2],
    height: f32,
) -> FramePacket {
    let mut tree = UiTree::new_layout(fixed(160.0, 100.0), 0).unwrap();
    tree.set_frame_build_policy(FrameBuildPolicy::Full);
    let outer = tree
        .append_layout(tree.root(), fixed(100.0, 60.0), 0x334455ff)
        .unwrap();
    let prefix = reveal.then(|| tree.append_layout(outer, fixed(100.0, 80.0), 0).unwrap());
    let inner = tree.append_layout(outer, fixed(90.0, 40.0), 0).unwrap();
    let first = tree
        .append_layout(inner, fixed(120.0, 70.0), 0xff0000ff)
        .unwrap();
    let target = tree
        .append_layout(inner, fixed(90.0, height), 0x00ff00ff)
        .unwrap();
    let tail = tree.append_layout(outer, fixed(120.0, 100.0), 0).unwrap();
    let mut tx = Transaction::default();
    for node in [Some(inner), Some(tail), prefix].into_iter().flatten() {
        tx.translation(node, outer_offset.map(|v| -v));
    }
    for (node, y) in [(first, 0.0), (target, 70.0)] {
        tx.translation(node, inner_offset.map(|v| -v));
        tx.clip(
            node,
            Some(Rect {
                x: inner_offset[0],
                y: inner_offset[1] - y,
                width: 90.0,
                height: 40.0,
            }),
        );
    }
    tx.clip(
        outer,
        Some(Rect {
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 60.0,
        }),
    );
    tx.opacity(outer, 0.8);
    tree.commit(tx, BackendCapabilities::default()).unwrap();
    tree.frame(VIEW).unwrap()
}
#[test]
fn logical_scroll_wheel_chains_and_accumulates_at_current_offsets_with_complete_pixels() {
    run(false);
}
#[cfg(feature = "retained-wgpu")]
#[test]
#[ignore = "requires WGPU adapter; run explicitly"]
fn hardware_logical_scroll_defaults_match_complete_reference() {
    run(true);
}
fn run(hardware: bool) {
    #[cfg(feature = "retained-wgpu")]
    let mut gpu = hardware.then(|| wgpu::WgpuFrameRenderer::new(2).unwrap());
    #[cfg(feature = "retained-wgpu")]
    if let Some(gpu) = &gpu {
        eprintln!("logical scroll adapter: {:?}", gpu.adapter_info());
    }
    #[cfg(not(feature = "retained-wgpu"))]
    assert!(!hardware);
    let mut retained = vec![];
    for reveal in [false, true] {
        let Fixture {
            mut ui,
            outer,
            inner,
            target,
        } = fixture(reveal);
        let mut router = router(&ui);
        let old = ui.published_local_frame().unwrap().clone();
        let old_pixels = pixels(old.resolve());
        let states = if reveal {
            let effects = router.route(UiInput::Tab { backwards: false }).unwrap();
            ui.apply_input_scroll(&effects).unwrap();
            vec![([0.0, 50.0], [0.0, 60.0])]
        } else {
            // Reuse the old installed targeting twice. Delta accumulation reads
            // current committed offsets rather than replaying absolute plans.
            let effects = wheel(&mut router, [50.0, 90.0]);
            let commit = ui.apply_input_scroll(&effects).unwrap();
            assert_eq!(commit.outcomes[0].consumed, [50.0, 90.0]);
            assert_eq!(commit.outcomes[0].remaining, [0.0; 2]);
            assert_eq!(offset(&ui, inner), [30.0, 50.0]);
            assert_eq!(offset(&ui, outer), [20.0, 40.0]);
            let first = ui.published_local_frame().unwrap().clone();
            let reference = reference(false, [30.0, 50.0], [20.0, 40.0], 20.0);
            assert_eq!(pixels(first.resolve()), pixels(reference.clone()));
            #[cfg(feature = "retained-wgpu")]
            if let Some(gpu) = &mut gpu {
                gpu.submit_local(&first, [160, 100], 1.0).unwrap();
                assert_eq!(
                    gpu.image().unwrap().as_raw(),
                    gpu.reference_image(&reference, [160, 100], 1.0)
                        .unwrap()
                        .as_raw()
                );
            }
            let commit = ui.apply_input_scroll(&effects).unwrap();
            assert_eq!(commit.outcomes[0].consumed, [0.0, 40.0]);
            assert_eq!(commit.outcomes[0].remaining, [50.0, 50.0]);
            vec![([30.0, 50.0], [20.0, 80.0])]
        };
        for (i, o) in states {
            assert_eq!(offset(&ui, inner), i);
            assert_eq!(offset(&ui, outer), o);
            let frame = ui.published_local_frame().unwrap().clone();
            let full = reference(reveal, i, o, 20.0);
            assert_eq!(pixels(frame.resolve()), pixels(full.clone()));
            #[cfg(feature = "retained-wgpu")]
            if let Some(gpu) = &mut gpu {
                gpu.submit_local(&frame, [160, 100], 1.0).unwrap();
                assert_eq!(
                    gpu.image().unwrap().as_raw(),
                    gpu.reference_image(&full, [160, 100], 1.0)
                        .unwrap()
                        .as_raw()
                );
            }
        }
        if reveal {
            assert_eq!(
                ui.input_scene().unwrap().hit_test([5.0, 45.0]),
                Some(public_id(target))
            );
        }
        drop(ui);
        assert_eq!(pixels(old.resolve()), old_pixels);
        retained.push(old);
    }
    #[cfg(feature = "retained-wgpu")]
    if let Some(gpu) = &mut gpu {
        gpu.finish().unwrap();
    }
}

#[test]
fn logical_scroll_reveal_uses_final_source_layout_and_is_idempotent() {
    let Fixture {
        mut ui,
        outer,
        inner,
        target,
    } = fixture(true);
    let scope = ui.root_scope();
    let height = ui.state(scope, 0, || 20u32).unwrap();
    let style = ui
        .computed(scope, 1, &[height.dependency()], move |i| {
            Ok(fixed(90.0, *i.read(height)? as f32))
        })
        .unwrap();
    ui.bind(style, Property::layout_style(target)).unwrap();
    ui.local_frame(VIEW).unwrap();
    let effects = router(&ui)
        .route(UiInput::Tab { backwards: false })
        .unwrap();
    ui.edit_input_scroll(
        &effects,
        |tx| {
            tx.set(height, 30)?;
            Ok(())
        },
        |(), _| Ok(()),
    )
    .unwrap();
    assert_eq!(*ui.read(height).unwrap(), 30);
    assert_eq!(ui.layout_result(target).unwrap().size, [90.0, 30.0]);
    assert_eq!(offset(&ui, inner), [0.0, 60.0]);
    assert_eq!(offset(&ui, outer), [0.0, 60.0]);
    assert_eq!(
        pixels(ui.published_local_frame().unwrap().resolve()),
        pixels(reference(true, [0.0, 60.0], [0.0, 60.0], 30.0))
    );
    let before = ui.published_local_frame().unwrap().clone();
    assert!(
        !ui.apply_input_scroll(&effects)
            .unwrap()
            .edit
            .result
            .commit
            .request_frame
    );
    assert!(before.same(ui.published_local_frame().unwrap()));
}

#[test]
fn logical_scroll_bound_property_is_controlled_and_unbind_restores_default_behavior() {
    let Fixture {
        mut ui,
        outer,
        inner,
        ..
    } = fixture(false);
    let scope = ui.root_scope();
    let scroll = ui.state(scope, 0, || [0.0f32; 2]).unwrap();
    let binding = ui.bind(scroll, Property::scroll(inner)).unwrap().0;
    ui.local_frame(VIEW).unwrap();
    let effects = wheel(&mut router(&ui), [10.0, 20.0]);
    ui.apply_input_scroll(&effects).unwrap();
    assert_eq!(offset(&ui, inner), [0.0; 2]);
    assert_eq!(offset(&ui, outer), [10.0, 20.0]);
    assert_eq!(*ui.read(scroll).unwrap(), [0.0; 2]);
    ui.edit_input_scroll(
        &effects,
        |_| Ok(()),
        |(), edit| {
            edit.unbind(binding)?;
            Ok(())
        },
    )
    .unwrap();
    assert_eq!(offset(&ui, inner), [10.0, 20.0]);
    assert_eq!(offset(&ui, outer), [10.0, 20.0]);
}

#[test]
fn logical_scroll_final_disabled_reparent_or_foreign_route_rolls_back_all_writes() {
    for mode in 0..3 {
        let Fixture {
            mut ui,
            outer: _,
            inner,
            target: _,
        } = fixture(false);
        let scope = ui.root_scope();
        let root = ui.root_render();
        let value = ui.state(scope, 0, || 0u32).unwrap();
        let effects = wheel(&mut router(&ui), [10.0, 20.0]);
        let before = ui.published_local_frame().unwrap().clone();
        let mut effects = effects;
        if mode == 2 {
            effects.wheel_scrolls[0].scene.runtime += 1;
        }
        let result = ui.edit_input_scroll(
            &effects,
            |tx| {
                tx.set(value, 1)?;
                Ok(())
            },
            |(), edit| {
                if mode == 0 {
                    edit.interaction(
                        scope,
                        inner,
                        Some(UiInteraction {
                            disabled: true,
                            ..Default::default()
                        }),
                    )?;
                }
                if mode == 1 {
                    edit.reparent(inner, root)?;
                }
                Ok(())
            },
        );
        assert!(matches!(
            result,
            Err(Error::Input(
                UiInputError::StaleScrollRoute | UiInputError::WrongRuntime
            ))
        ));
        assert_eq!(*ui.read(value).unwrap(), 0);
        assert_eq!(offset(&ui, inner), [0.0; 2]);
        assert!(before.same(ui.published_local_frame().unwrap()));
    }
}
#[test]
fn logical_scroll_final_frame_error_preserves_offsets_source_and_effects() {
    let Fixture {
        mut ui,
        outer,
        inner,
        ..
    } = fixture(false);
    let scope = ui.root_scope();
    let value = ui.state(scope, 0, || 0u32).unwrap();
    let called = Rc::new(std::cell::Cell::new(0));
    let log = called.clone();
    ui.effect(scope, 0, &[value.dependency()], move |_| {
        log.set(log.get() + 1);
        None
    })
    .unwrap();
    let before_calls = called.get();
    let before = ui.published_local_frame().unwrap().clone();
    let effects = wheel(&mut router(&ui), [10.0, 20.0]);
    let result = ui.edit_input_scroll(
        &effects,
        |tx| {
            tx.set(value, 1)?;
            Ok(())
        },
        |(), edit| {
            edit.properties().opacity(outer, f32::NAN);
            Ok(())
        },
    );
    assert!(matches!(result, Err(Error::InvalidOpacity)));
    assert_eq!(*ui.read(value).unwrap(), 0);
    assert_eq!(offset(&ui, inner), [0.0; 2]);
    assert_eq!(called.get(), before_calls);
    assert!(before.same(ui.published_local_frame().unwrap()));
}

#[test]
fn logical_scroll_final_binding_and_hidden_reveal_preserve_application_control() {
    let Fixture {
        mut ui,
        outer,
        inner,
        ..
    } = fixture(true);
    let scope = ui.root_scope();
    let source = ui.state(scope, 0, || [0.0f32; 2]).unwrap();
    let effects = router(&ui)
        .route(UiInput::Tab { backwards: false })
        .unwrap();
    ui.edit_input_scroll(
        &effects,
        |_| Ok(()),
        |(), edit| {
            edit.bind(source, Property::scroll(inner))?;
            Ok(())
        },
    )
    .unwrap();
    // The target is entirely outside the controlled inner viewport; scrolling
    // an ancestor cannot make hidden content pass through that fixed viewport.
    assert_eq!(offset(&ui, inner), [0.0; 2]);
    assert_eq!(offset(&ui, outer), [0.0; 2]);

    let Fixture {
        mut ui,
        outer,
        inner,
        ..
    } = fixture(true);
    ui.edit(
        |_| Ok(()),
        |(), edit| {
            edit.properties().layout_style(
                outer,
                fixed(100.0, 60.0).overflow(LayoutOverflow::Hidden, LayoutOverflow::Hidden),
            );
            edit.properties().layout_style(
                inner,
                fixed(90.0, 40.0).overflow(LayoutOverflow::Hidden, LayoutOverflow::Hidden),
            );
            edit.pixel_snap(PixelSnap::DevicePixels { scale: 1.5 })?;
            Ok(())
        },
    )
    .unwrap();
    ui.local_frame(VIEW).unwrap();
    let mut router = router(&ui);
    let no_wheel = ui
        .apply_input_scroll(&wheel(&mut router, [0.0, 20.0]))
        .unwrap();
    assert_eq!(no_wheel.outcomes[0].remaining, [0.0, 20.0]);
    let effects = router.route(UiInput::Tab { backwards: false }).unwrap();
    ui.apply_input_scroll(&effects).unwrap();
    assert_eq!(offset(&ui, inner), [0.0, 50.0]);
    assert_eq!(offset(&ui, outer), [0.0, 60.0]);
    let before = ui.published_local_frame().unwrap().clone();
    ui.apply_input_scroll(&effects).unwrap();
    assert!(before.same(ui.published_local_frame().unwrap()));
}

#[test]
fn logical_scroll_rejects_nonfinite_input_and_unpublished_geometry() {
    let Fixture { mut ui, outer, .. } = fixture(false);
    let mut effects = wheel(&mut router(&ui), [0.0, 1.0]);
    effects.wheel_scrolls[0].delta[1] = f32::NAN;
    assert!(matches!(
        ui.apply_input_scroll(&effects),
        Err(Error::Input(UiInputError::InvalidScrollDelta))
    ));
    ui.edit(
        |_| Ok(()),
        |(), edit| {
            edit.properties().translation(outer, [2.0, 0.0]);
            Ok(())
        },
    )
    .unwrap();
    assert!(matches!(ui.input_scene(), Err(Error::InvalidFrame)));
    ui.local_frame(VIEW).unwrap();
    assert!(ui.input_scene().is_ok());
}
