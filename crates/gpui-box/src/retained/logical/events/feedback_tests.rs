use super::*;
use crate::retained::{style::*, ui_ir::UiProperties};
const VIEW: [f32; 2] = [100.0, 60.0];
fn fixed(width: f32) -> LayoutStyle {
    LayoutStyle::column()
        .size(LayoutDimension::Px(width), LayoutDimension::Px(20.0))
        .flex_shrink(0.0)
}
fn plan() -> StylePlan {
    StylePlan::new(&[StyleRef::new(
        "button",
        vec![StyleDeclaration::Color(
            StyleRule::value(0xff0000ff)
                .when(
                    StyleCondition {
                        hovered: Some(true),
                        ..Default::default()
                    },
                    0x00ff00ff,
                )
                .when(
                    StyleCondition {
                        pressed: Some(true),
                        ..Default::default()
                    },
                    0x0000ffff,
                )
                .when(
                    StyleCondition {
                        focus_visible: Some(true),
                        ..Default::default()
                    },
                    0xffff00ff,
                ),
        )],
    )
    .unwrap()])
}
fn fixture() -> (LogicalUi, NodeId, ReadProp<StyleState>) {
    let mut ui = LogicalUi::new_layout(
        fixed(100.0),
        0,
        BackendCapabilities {
            solid_color_patch: true,
        },
    )
    .unwrap();
    let scope = ui.root_scope();
    let root = ui.root_render();
    let (node, state) = ui
        .edit_local_frame(
            LayoutConstraints::viewport(VIEW[0], VIEW[1]),
            VIEW,
            |_| Ok(()),
            |(), edit| {
                let node = edit.append_layout(scope, root, fixed(30.0), 0)?;
                let state = edit.interaction_state(scope, 0, node, false, true)?;
                let plan = plan();
                let output = edit.computed(scope, 1, &[state.dependency()], move |input| {
                    plan.resolve_values(
                        &fixed(30.0),
                        &UiProperties::default(),
                        &responsive::LayoutEnvironment::new(VIEW),
                        *input.read_prop(state)?,
                    )
                    .map_err(|_| Error::InvalidStyle)
                })?;
                edit.bind(output, Property::style_resolution(node))?;
                edit.interaction(
                    scope,
                    node,
                    Some(UiInteraction {
                        focus: UiFocusPolicy::Tab,
                        ..Default::default()
                    }),
                )?;
                Ok((node, state))
            },
        )
        .unwrap()
        .0
        .result
        .value;
    (ui, node, state)
}
fn router(ui: &LogicalUi) -> UiInputRouter {
    let mut router = UiInputRouter::new();
    router
        .adopt(ui.input_scene().unwrap(), UiInputEvidence::ImageInstalled)
        .unwrap();
    router
}
fn pixels(frame: FramePacket) -> Vec<u8> {
    let mut job = software::SoftwareJob::new(
        frame,
        software::SoftwareOptions {
            size: [100, 60],
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
fn logical_feedback_hover_press_keyboard_preserves_caller_state_and_complete_pixels() {
    run(false);
}
#[cfg(feature = "retained-wgpu")]
#[test]
#[ignore = "requires WGPU adapter; run explicitly"]
fn hardware_logical_feedback_matches_complete_reference() {
    run(true);
}
fn run(hardware: bool) {
    let (mut ui, node, state) = fixture();
    let mut router = router(&ui);
    #[cfg(feature = "retained-wgpu")]
    let mut gpu = hardware.then(|| wgpu::WgpuFrameRenderer::new(2).unwrap());
    #[cfg(feature = "retained-wgpu")]
    if let Some(gpu) = &gpu {
        eprintln!("logical feedback adapter: {:?}", gpu.adapter_info());
    }
    #[cfg(not(feature = "retained-wgpu"))]
    assert!(!hardware);
    let old = ui.published_local_frame().unwrap().clone();
    let expected_old = pixels(old.resolve());
    for (event, color, hovered, pressed, focus) in [
        (
            UiInput::PointerMove([5.0; 2]),
            0x00ff00ff,
            true,
            false,
            false,
        ),
        (
            UiInput::PrimaryDown([5.0; 2]),
            0x0000ffff,
            true,
            true,
            false,
        ),
        (
            UiInput::PointerMove([50.0; 2]),
            0xff0000ff,
            false,
            false,
            false,
        ),
        (
            UiInput::PrimaryUp([50.0; 2]),
            0xff0000ff,
            false,
            false,
            false,
        ),
        (
            UiInput::Tab { backwards: false },
            0xffff00ff,
            false,
            false,
            true,
        ),
        (UiInput::WindowFocusLost, 0xff0000ff, false, false, false),
    ] {
        let effects = router.route(event).unwrap();
        let commit = ui.apply_input_effects(&effects).unwrap();
        assert_eq!(
            *ui.read_prop(state).unwrap(),
            StyleState {
                hovered,
                pressed,
                focus_visible: focus,
                selected: true,
                disabled: false
            }
        );
        let frame = ui.published_local_frame().unwrap().clone();
        let mut full = UiTree::new_layout(fixed(100.0), 0).unwrap();
        full.set_frame_build_policy(FrameBuildPolicy::Full);
        full.append_layout(full.root(), fixed(30.0), color).unwrap();
        let reference = full.frame(VIEW).unwrap();
        assert_eq!(pixels(frame.resolve()), pixels(reference.clone()));
        assert_eq!(ui.layout_result(node).unwrap().size, [30.0, 20.0]);
        if commit.edit.result.commit.request_frame {
            assert!(
                commit
                    .edit
                    .result
                    .commit
                    .routes
                    .iter()
                    .any(|(id, _)| *id == node)
            );
        }
        #[cfg(feature = "retained-wgpu")]
        if let Some(gpu) = &mut gpu {
            gpu.submit_local(&frame, [100, 60], 1.0).unwrap();
            assert_eq!(
                gpu.image().unwrap().as_raw(),
                gpu.reference_image(&reference, [100, 60], 1.0)
                    .unwrap()
                    .as_raw()
            );
        }
        router
            .adopt(ui.input_scene().unwrap(), UiInputEvidence::ImageInstalled)
            .unwrap();
    }
    let before = ui.published_local_frame().unwrap().clone();
    let effects = router.route(UiInput::PointerLeave).unwrap();
    assert!(
        !ui.apply_input_effects(&effects)
            .unwrap()
            .edit
            .result
            .commit
            .request_frame
    );
    assert!(before.same(ui.published_local_frame().unwrap()));
    drop(ui);
    assert_eq!(pixels(old.resolve()), expected_old);
    #[cfg(feature = "retained-wgpu")]
    if let Some(gpu) = &mut gpu {
        gpu.finish().unwrap();
    }
}
#[test]
fn logical_feedback_and_invalid_final_frame_roll_back_together() {
    let (mut ui, node, state) = fixture();
    let effects = router(&ui).route(UiInput::PrimaryDown([5.0; 2])).unwrap();
    let before = ui.published_local_frame().unwrap().clone();
    let root = ui.root_render();
    assert!(matches!(
        ui.edit_input_effects(
            &effects,
            |_| Ok(()),
            |(), edit| {
                edit.properties().opacity(root, f32::NAN);
                Ok(())
            }
        ),
        Err(Error::InvalidOpacity)
    ));
    assert!(!ui.read_prop(state).unwrap().pressed);
    assert!(before.same(ui.published_local_frame().unwrap()));
    ui.apply_input_effects(&effects).unwrap();
    assert!(ui.read_prop(state).unwrap().pressed);
    let scope = ui.root_scope();
    ui.edit_input_effects(
        &UiInputEffects::default(),
        |_| Ok(()),
        |(), edit| {
            assert_eq!(
                edit.interaction_state(scope, 0, node, true, false)?
                    .dependency(),
                state.dependency()
            );
            Ok(())
        },
    )
    .unwrap();
    let current = *ui.read_prop(state).unwrap();
    assert!(current.disabled && !current.selected && current.pressed);
}
#[test]
fn logical_feedback_ignores_foreign_future_and_retired_identities() {
    let (mut ui, node, state) = fixture();
    let mut effects = router(&ui).route(UiInput::PrimaryDown([5.0; 2])).unwrap();
    for foreign in [true, false] {
        let mut invalid = effects.clone();
        for feedback in &mut invalid.feedback {
            if foreign {
                feedback.scene.runtime += 1;
            } else {
                feedback.scene.source_revision = u64::MAX;
            }
        }
        ui.apply_input_effects(&invalid).unwrap();
        assert!(!ui.read_prop(state).unwrap().pressed);
    }
    let scope = ui.root_scope();
    ui.edit_input_effects(
        &effects,
        |_| Ok(()),
        |(), edit| {
            edit.interaction(
                scope,
                node,
                Some(UiInteraction {
                    disabled: true,
                    ..Default::default()
                }),
            )?;
            Ok(())
        },
    )
    .unwrap();
    assert!(!ui.read_prop(state).unwrap().pressed);
    // A fresh router also treats the final disabled policy as ineligible.
    let mut router = router(&ui);
    effects = router
        .adopt(ui.input_scene().unwrap(), UiInputEvidence::ImageInstalled)
        .unwrap();
    ui.apply_input_effects(&effects).unwrap();
}
#[test]
fn logical_feedback_style_resolution_conflicts_with_covered_bindings_in_both_orders() {
    for bundle_first in [true, false] {
        let mut tree = UiTree::new_layout(fixed(30.0), 0).unwrap();
        let root = tree.root();
        tree.frame(VIEW).unwrap();
        let mut ui = ReactiveUi::new(tree, BackendCapabilities::default()).unwrap();
        let bundle = ui.signal(
            plan()
                .resolve(
                    &fixed(30.0),
                    &UiProperties::default(),
                    &responsive::LayoutEnvironment::new(VIEW),
                    StyleState::default(),
                )
                .unwrap(),
        );
        let color = ui.signal(0x00ff00ffu32);
        if bundle_first {
            ui.bind(bundle, Property::style_resolution(root)).unwrap();
            assert!(matches!(
                ui.bind(color, Property::color(root)),
                Err(Error::BindingConflict)
            ));
        } else {
            let binding = ui.bind(color, Property::color(root)).unwrap().0;
            assert!(matches!(
                ui.bind(bundle, Property::style_resolution(root)),
                Err(Error::BindingConflict)
            ));
            ui.edit(
                |_| Ok(()),
                |(), edit| {
                    edit.unbind(binding)?;
                    edit.bind(bundle, Property::style_resolution(root))
                },
            )
            .unwrap();
            assert!(matches!(
                ui.edit(
                    |_| Ok(()),
                    |(), edit| edit.bind(color, Property::color(root))
                ),
                Err(Error::BindingConflict)
            ));
        }
    }
}

#[test]
fn logical_feedback_appearance_binding_preserves_default_scroll_and_separate_scroll_binding() {
    let mut ui = LogicalUi::new_layout(fixed(30.0), 0, BackendCapabilities::default()).unwrap();
    let scope = ui.root_scope();
    let root = ui.root_render();
    let (state, output) = ui
        .edit_local_frame(
            LayoutConstraints::viewport(VIEW[0], VIEW[1]),
            VIEW,
            |_| Ok(()),
            |(), edit| {
                edit.append_layout(
                    scope,
                    root,
                    LayoutStyle::column()
                        .size(LayoutDimension::Px(30.0), LayoutDimension::Px(100.0))
                        .flex_shrink(0.0),
                    0,
                )?;
                let state = edit.interaction_state(scope, 0, root, false, false)?;
                let plan = plan();
                let output = edit.computed(scope, 1, &[state.dependency()], move |i| {
                    plan.resolve_values(
                        &fixed(30.0).overflow(LayoutOverflow::Scroll, LayoutOverflow::Scroll),
                        &UiProperties::default(),
                        &responsive::LayoutEnvironment::new(VIEW),
                        *i.read_prop(state)?,
                    )
                    .map_err(|_| Error::InvalidStyle)
                })?;
                edit.bind(output, Property::style_resolution_without_scroll(root))?;
                Ok((state, output))
            },
        )
        .unwrap()
        .0
        .result
        .value;
    let mut router = router(&ui);
    let effects = router
        .route(UiInput::Wheel {
            position: [5.0; 2],
            delta: [0.0, 10.0],
        })
        .unwrap();
    ui.apply_input_effects(&effects).unwrap();
    let root_id = ui.input_scene().unwrap().node_id(root).unwrap();
    assert_eq!(
        ui.input_scene()
            .unwrap()
            .scroll_state(root_id)
            .unwrap()
            .offset,
        [0.0, 10.0]
    );
    let effects = router.route(UiInput::PointerMove([5.0; 2])).unwrap();
    ui.apply_input_effects(&effects).unwrap();
    assert!(ui.read_prop(state).unwrap().hovered);
    assert_eq!(
        ui.input_scene()
            .unwrap()
            .scroll_state(root_id)
            .unwrap()
            .offset,
        [0.0, 10.0]
    );
    let scroll = ui.state(scope, 2, || [0.0, 5.0]).unwrap();
    ui.bind(scroll, Property::scroll(root)).unwrap();
    ui.local_frame(VIEW).unwrap();
    assert!(
        !ui.input_scene()
            .unwrap()
            .scroll_state(root_id)
            .unwrap()
            .wheel_axes[1]
    );
    assert!(matches!(
        ui.bind(output, Property::style_resolution(root)),
        Err(Error::BindingConflict)
    ));
}

#[test]
fn logical_feedback_scope_retirement_drops_registration_and_old_readonly_input() {
    for transactional in [false, true] {
        let mut ui =
            LogicalUi::new_layout(fixed(100.0), 0, BackendCapabilities::default()).unwrap();
        let owner = ui.root_scope();
        let root = ui.root_render();
        let child = ui.reconcile_keys(owner, &[1]).unwrap().order[0];
        let state = ui
            .edit_local_frame(
                LayoutConstraints::viewport(VIEW[0], VIEW[1]),
                VIEW,
                |_| Ok(()),
                |(), edit| {
                    let node = edit.append_layout(child, root, fixed(30.0), 0xff0000ff)?;
                    edit.interaction_state(child, 0, node, false, false)
                },
            )
            .unwrap()
            .0
            .result
            .value;
        let effects = router(&ui).route(UiInput::PointerMove([5.0; 2])).unwrap();
        if transactional {
            ui.edit(|_| Ok(()), |(), edit| edit.reconcile_keys(owner, &[]))
                .unwrap();
        } else {
            ui.reconcile_keys(owner, &[]).unwrap();
        }
        ui.local_frame(VIEW).unwrap();
        assert!(ui.events.feedback.is_empty());
        assert!(matches!(ui.read_prop(state), Err(Error::InvalidSignal)));
        ui.apply_input_effects(&effects).unwrap();
        assert!(ui.events.feedback.is_empty());
    }
}
