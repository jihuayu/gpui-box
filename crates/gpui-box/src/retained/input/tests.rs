use super::style::*;
use super::ui_ir::*;
use super::*;
const ROOT: UiNodeId = UiNodeId::new(0, 1);
const BUTTON: UiNodeId = UiNodeId::new(1, 1);
const LABEL: UiNodeId = UiNodeId::new(2, 1);
const NEXT: UiNodeId = UiNodeId::new(3, 1);
const OVERLAY: UiNodeId = UiNodeId::new(4, 1);
fn size(width: f32, height: f32) -> LayoutStyle {
    LayoutStyle::row().size(LayoutDimension::Px(width), LayoutDimension::Px(height))
}
fn constraints() -> LayoutConstraints {
    LayoutConstraints::viewport(160.0, 100.0)
}
fn control(id: u64) -> UiInteraction {
    UiInteraction {
        focus: UiFocusPolicy::Tab,
        pointer_handler: Some(UiHandlerId(id)),
        focus_handler: Some(UiHandlerId(id + 10)),
        activate_handler: Some(UiHandlerId(id + 20)),
        ..Default::default()
    }
}
fn fixture() -> (UiDocument, UiResources, UiPublication) {
    let mut root = UiNode::new(ROOT, size(160.0, 100.0));
    root.children = vec![BUTTON, NEXT, OVERLAY];
    root.interaction.pointer_handler = Some(UiHandlerId(90));
    let mut button = UiNode::new(BUTTON, size(60.0, 30.0));
    button.children = vec![LABEL];
    button.interaction = control(1);
    button.style_state.selected = true;
    button.styles = vec![
        StyleRef::new(
            "button",
            vec![StyleDeclaration::Color(
                StyleRule::value(0xff0000ff)
                    .when(
                        StyleCondition {
                            hovered: Some(true),
                            ..Default::default()
                        },
                        0x0000ffff,
                    )
                    .when(
                        StyleCondition {
                            pressed: Some(true),
                            ..Default::default()
                        },
                        0x00ff00ff,
                    ),
            )],
        )
        .unwrap(),
    ];
    let mut label = UiNode::new(LABEL, size(25.0, 15.0));
    label.properties.color = 0xffffffff;
    let mut next = UiNode::new(NEXT, size(60.0, 30.0));
    next.interaction = control(2);
    next.layout.order = -1;
    let mut overlay = UiNode::new(OVERLAY, size(60.0, 30.0));
    overlay.layout.position = LayoutPosition::Absolute;
    overlay.layout.inset = LayoutEdges {
        left: LayoutLength::Px(60.0),
        top: LayoutLength::Px(0.0),
        ..LayoutEdges::all(LayoutLength::Auto)
    };
    overlay.interaction.pointer_target = false;
    let resources = UiResources::new();
    let (document, publication) = UiDocument::from_snapshot(
        UiSnapshot::new(400, 1, ROOT, vec![root, button, label, next, overlay]),
        &resources,
        UiLimits::default(),
        BackendCapabilities::default(),
        constraints(),
        [160.0, 100.0],
    )
    .unwrap();
    (document, resources, publication)
}
fn batch(document: &UiDocument, operations: Vec<UiOperation>) -> UiBatch {
    UiBatch {
        schema: UI_SCHEMA_VERSION,
        runtime: 400,
        epoch: 1,
        base_revision: document.revision(),
        revision: document.revision() + 1,
        sequence: document.next_sequence(),
        operations,
    }
}
fn apply(
    document: &mut UiDocument,
    resources: &UiResources,
    operations: Vec<UiOperation>,
) -> UiPublication {
    document
        .apply(
            batch(document, operations),
            resources,
            BackendCapabilities::default(),
            constraints(),
            [160.0, 100.0],
        )
        .unwrap()
}
fn activates(effects: &UiInputEffects) -> Vec<UiNodeId> {
    effects
        .dispatches
        .iter()
        .filter(|event| event.kind == UiInputEventKind::Activate)
        .map(|event| event.current_target)
        .collect()
}
fn pixels(frame: FramePacket) -> Vec<u8> {
    let mut job = software::SoftwareJob::new(
        frame,
        software::SoftwareOptions {
            size: [160, 100],
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
fn primary_capture_nested_target_bubbling_drag_out_and_keyboard_focus_are_conventional() {
    let (document, _, publication) = fixture();
    let mut router = UiInputRouter::new();
    router
        .adopt(publication.input, UiInputEvidence::ImageInstalled)
        .unwrap();
    assert_eq!(
        router
            .route(UiInput::PointerMove([65.0, 5.0]))
            .unwrap()
            .dispatches
            .iter()
            .filter(|e| e.kind == UiInputEventKind::PointerEnter)
            .map(|e| e.current_target)
            .collect::<Vec<_>>(),
        vec![ROOT, BUTTON]
    );
    assert_eq!(router.hovered(), Some(LABEL));
    let down = router.route(UiInput::PrimaryDown([65.0, 5.0])).unwrap();
    assert_eq!(router.captured(), Some(LABEL));
    assert_eq!(router.focused(), Some(BUTTON));
    assert_eq!(
        down.dispatches
            .iter()
            .filter(|e| e.kind == UiInputEventKind::PointerDown)
            .map(|e| e.current_target)
            .collect::<Vec<_>>(),
        vec![BUTTON, ROOT]
    );
    assert!(down.feedback.iter().any(|f| f.node == BUTTON && f.pressed));
    let outside = router.route(UiInput::PointerMove([145.0, 60.0])).unwrap();
    assert!(
        outside
            .feedback
            .iter()
            .any(|f| f.node == BUTTON && !f.pressed)
    );
    assert!(
        outside
            .dispatches
            .iter()
            .any(|e| e.kind == UiInputEventKind::PointerMove && e.target == LABEL)
    );
    assert!(activates(&router.route(UiInput::PrimaryUp([145.0, 60.0])).unwrap()).is_empty());
    router.route(UiInput::PrimaryDown([65.0, 5.0])).unwrap();
    router.route(UiInput::PointerMove([145.0, 60.0])).unwrap();
    router.route(UiInput::PointerMove([105.0, 20.0])).unwrap();
    let up = router.route(UiInput::PrimaryUp([105.0, 20.0])).unwrap();
    assert_eq!(activates(&up), vec![BUTTON]);
    assert!(up.dispatches.iter().all(|e| document.accepts_dispatch(e)));
    assert_eq!(router.captured(), None);
}

#[test]
fn tabs_use_declaration_order_and_activation_keys_suppress_repeat_and_cancel_on_blur() {
    let (_, _, publication) = fixture();
    let mut router = UiInputRouter::new();
    router
        .adopt(publication.input, UiInputEvidence::Published)
        .unwrap();
    // Layout order moves NEXT left of BUTTON; keyboard order remains authored.
    let focus = router.route(UiInput::Tab { backwards: false }).unwrap();
    assert_eq!(router.focused(), Some(BUTTON));
    assert!(
        focus
            .feedback
            .iter()
            .any(|f| f.node == BUTTON && f.focus_visible)
    );
    assert_eq!(
        activates(
            &router
                .route(UiInput::KeyDown {
                    key: UiActivationKey::Enter,
                    repeat: false
                })
                .unwrap()
        ),
        vec![BUTTON]
    );
    assert!(
        activates(
            &router
                .route(UiInput::KeyDown {
                    key: UiActivationKey::Enter,
                    repeat: true
                })
                .unwrap()
        )
        .is_empty()
    );
    assert!(
        activates(
            &router
                .route(UiInput::KeyUp(UiActivationKey::Enter))
                .unwrap()
        )
        .is_empty()
    );
    let space = router
        .route(UiInput::KeyDown {
            key: UiActivationKey::Space,
            repeat: false,
        })
        .unwrap();
    assert!(activates(&space).is_empty());
    assert!(space.feedback.iter().any(|f| f.node == BUTTON && f.pressed));
    assert_eq!(
        activates(
            &router
                .route(UiInput::KeyUp(UiActivationKey::Space))
                .unwrap()
        ),
        vec![BUTTON]
    );
    router
        .route(UiInput::KeyDown {
            key: UiActivationKey::Space,
            repeat: false,
        })
        .unwrap();
    router.route(UiInput::Tab { backwards: false }).unwrap();
    assert_eq!(router.focused(), Some(NEXT));
    assert!(
        activates(
            &router
                .route(UiInput::KeyUp(UiActivationKey::Space))
                .unwrap()
        )
        .is_empty()
    );
    router.route(UiInput::Tab { backwards: false }).unwrap();
    assert_eq!(router.focused(), Some(BUTTON));
    router.route(UiInput::Tab { backwards: true }).unwrap();
    assert_eq!(router.focused(), Some(NEXT));
    router.route(UiInput::WindowFocusLost).unwrap();
    assert_eq!(router.focused(), None);
    assert_eq!(router.hovered(), None);
}

#[test]
fn adopted_scene_keeps_visible_geometry_until_host_installs_the_next_complete_frame() {
    let (mut document, resources, first) = fixture();
    let old = first.input.clone();
    let mut router = UiInputRouter::new();
    router
        .adopt(first.input, UiInputEvidence::ImageInstalled)
        .unwrap();
    let mut properties = document.node(BUTTON).unwrap().properties.clone();
    properties.translation = [0.0, 40.0];
    let second = apply(
        &mut document,
        &resources,
        vec![UiOperation::Properties {
            node: BUTTON,
            value: properties,
        }],
    );
    assert_eq!(old.hit_test([65.0, 5.0]), Some(LABEL));
    assert_eq!(second.input.hit_test([65.0, 5.0]), Some(ROOT));
    router.route(UiInput::PrimaryDown([65.0, 5.0])).unwrap();
    let up = router.route(UiInput::PrimaryUp([65.0, 5.0])).unwrap();
    assert_eq!(activates(&up), vec![BUTTON]);
    assert_eq!(up.dispatches.last().unwrap().scene.source_revision, 0);
    // Existing identity/handler can process an old visible click after motion.
    assert!(document.accepts_dispatch(up.dispatches.last().unwrap()));
    router
        .adopt(second.input, UiInputEvidence::ImageInstalled)
        .unwrap();
    router.route(UiInput::PrimaryDown([65.0, 5.0])).unwrap();
    assert!(activates(&router.route(UiInput::PrimaryUp([65.0, 5.0])).unwrap()).is_empty());
    assert_eq!(
        router.adopt(old, UiInputEvidence::Published).unwrap_err(),
        UiInputError::StaleScene
    );
    drop((document, resources));
    assert_eq!(
        router
            .route(UiInput::PointerMove([65.0, 45.0]))
            .unwrap()
            .dispatches
            .last()
            .unwrap()
            .target,
        LABEL
    );
}

#[test]
fn disabled_subtrees_occlude_pointer_targets_and_cancel_focus_capture_and_space() {
    let (mut document, resources, first) = fixture();
    let mut router = UiInputRouter::new();
    router
        .adopt(first.input, UiInputEvidence::Published)
        .unwrap();
    router.route(UiInput::PrimaryDown([65.0, 5.0])).unwrap();
    router
        .route(UiInput::KeyDown {
            key: UiActivationKey::Space,
            repeat: false,
        })
        .unwrap();
    let mut policy = document.node(BUTTON).unwrap().interaction;
    policy.disabled = true;
    let next = apply(
        &mut document,
        &resources,
        vec![UiOperation::Interaction {
            node: BUTTON,
            value: policy,
        }],
    );
    let effects = router
        .adopt(next.input, UiInputEvidence::ImageInstalled)
        .unwrap();
    assert!(
        effects
            .dispatches
            .iter()
            .any(|e| e.kind == UiInputEventKind::PointerCancel)
    );
    assert!(
        effects
            .dispatches
            .iter()
            .any(|e| e.kind == UiInputEventKind::Blur)
    );
    assert_eq!(router.captured(), None);
    assert_eq!(router.focused(), None);
    assert_eq!(router.hovered(), None);
    assert!(activates(&router.route(UiInput::PrimaryUp([65.0, 5.0])).unwrap()).is_empty());
    assert!(
        activates(
            &router
                .route(UiInput::KeyUp(UiActivationKey::Space))
                .unwrap()
        )
        .is_empty()
    );
    router.route(UiInput::Tab { backwards: false }).unwrap();
    assert_eq!(router.focused(), Some(NEXT));
    assert_eq!(
        router.route(UiInput::Focus(Some(BUTTON))).unwrap_err(),
        UiInputError::IneligibleFocus
    );
    // A disabled front box blocks the enabled control behind, rather than
    // passing an accidental click through to it.
    let mut overlay = document.node(OVERLAY).unwrap().interaction;
    overlay.pointer_target = true;
    overlay.disabled = true;
    let next = apply(
        &mut document,
        &resources,
        vec![UiOperation::Interaction {
            node: OVERLAY,
            value: overlay,
        }],
    );
    router
        .adopt(next.input, UiInputEvidence::Published)
        .unwrap();
    let blocked = router.route(UiInput::PrimaryDown([65.0, 5.0])).unwrap();
    assert!(
        blocked
            .dispatches
            .iter()
            .all(|event| event.kind == UiInputEventKind::Blur)
    );
    assert_eq!(router.captured(), None);
}

#[test]
fn atomic_feedback_uses_normal_color_routes_preserves_caller_state_and_matches_direct_tree() {
    run_feedback(false);
}
#[cfg(feature = "retained-wgpu")]
#[test]
#[ignore = "requires WGPU adapter; run explicitly"]
fn hardware_input_feedback_matches_direct_complete_pixels() {
    run_feedback(true);
}
fn run_feedback(hardware: bool) {
    let (mut document, resources, first) = fixture();
    let mut router = UiInputRouter::new();
    router
        .adopt(first.input, UiInputEvidence::ImageInstalled)
        .unwrap();
    #[cfg(feature = "retained-wgpu")]
    let mut gpu = hardware.then(|| wgpu::WgpuFrameRenderer::new(2).unwrap());
    #[cfg(feature = "retained-wgpu")]
    if let Some(gpu) = &gpu {
        eprintln!("input feedback adapter: {:?}", gpu.adapter_info());
    }
    #[cfg(not(feature = "retained-wgpu"))]
    assert!(!hardware);
    let mut held = None;
    #[cfg(feature = "retained-wgpu")]
    let mut gpu_revision = None;
    #[cfg(feature = "retained-wgpu")]
    let mut gpu_submissions = 0;
    for (event, color) in [
        (UiInput::PointerMove([65.0, 5.0]), 0x0000ffff),
        (UiInput::PrimaryDown([65.0, 5.0]), 0x00ff00ff),
        (UiInput::PointerMove([145.0, 60.0]), 0xff0000ff),
        (UiInput::PrimaryUp([145.0, 60.0]), 0xff0000ff),
        (UiInput::Tab { backwards: false }, 0xff0000ff),
    ] {
        let effects = router.route(event).unwrap();
        let operations = effects.style_operations(&document);
        let publication = apply(&mut document, &resources, operations);
        assert!(document.node(BUTTON).unwrap().style_state.selected);
        assert_eq!(
            document.resolved_node(BUTTON).unwrap().properties.color,
            color
        );
        let mut snapshot = document.snapshot();
        // Independently author resolved colors, bypassing style/input evaluation.
        for node in &mut snapshot.nodes {
            node.styles.clear();
            if node.id == BUTTON {
                node.properties.color = color;
            }
        }
        let (_, reference) = UiDocument::from_snapshot(
            snapshot,
            &resources,
            UiLimits::default(),
            BackendCapabilities::default(),
            constraints(),
            [160.0, 100.0],
        )
        .unwrap();
        assert_eq!(
            pixels(publication.frame.resolve()),
            pixels(reference.frame.resolve())
        );
        #[cfg(feature = "retained-wgpu")]
        if let Some(gpu) = &mut gpu {
            let submitted = gpu
                .submit_local(&publication.frame, [160, 100], 1.0)
                .unwrap();
            assert_eq!(
                submitted.is_some(),
                gpu_revision != Some(publication.frame.revision())
            );
            gpu_submissions += usize::from(submitted.is_some());
            gpu_revision = Some(publication.frame.revision());
            assert_eq!(
                gpu.image().unwrap().as_raw(),
                gpu.reference_image(&reference.frame.resolve(), [160, 100], 1.0)
                    .unwrap()
                    .as_raw()
            );
        }
        held = Some(publication.frame.clone());
        router
            .adopt(publication.input, UiInputEvidence::ImageInstalled)
            .unwrap();
    }
    let held = held.unwrap();
    let expected = pixels(held.resolve());
    drop((document, resources, router));
    assert_eq!(pixels(held.resolve()), expected);
    #[cfg(feature = "retained-wgpu")]
    if let Some(gpu) = &mut gpu {
        assert_eq!(gpu_submissions, 3);
        gpu.finish().unwrap();
    }
}

#[test]
fn stale_handlers_removed_generations_and_recovery_cannot_receive_old_activation() {
    let (mut document, resources, first) = fixture();
    let mut router = UiInputRouter::new();
    router
        .adopt(first.input, UiInputEvidence::Published)
        .unwrap();
    router.route(UiInput::PrimaryDown([65.0, 5.0])).unwrap();
    let up = router.route(UiInput::PrimaryUp([65.0, 5.0])).unwrap();
    let activation = up.dispatches.last().unwrap().clone();
    assert!(document.accepts_dispatch(&activation));
    let mut policy = document.node(BUTTON).unwrap().interaction;
    policy.activate_handler = Some(UiHandlerId(101));
    let next = apply(
        &mut document,
        &resources,
        vec![UiOperation::Interaction {
            node: BUTTON,
            value: policy,
        }],
    );
    assert!(!document.accepts_dispatch(&activation));
    router
        .adopt(next.input, UiInputEvidence::Published)
        .unwrap();
    router.route(UiInput::PrimaryDown([65.0, 5.0])).unwrap();
    let next = apply(
        &mut document,
        &resources,
        vec![UiOperation::RemoveSubtree(BUTTON)],
    );
    let retired = router
        .adopt(next.input, UiInputEvidence::Published)
        .unwrap();
    assert!(
        retired
            .dispatches
            .iter()
            .any(|event| event.kind == UiInputEventKind::PointerCancel)
    );
    assert_eq!(router.captured(), None);
    assert!(!document.accepts_dispatch(&activation));
    let replacement_id = UiNodeId::new(BUTTON.slot, 2);
    let mut replacement = UiNode::new(replacement_id, size(60.0, 30.0));
    replacement.interaction = control(1);
    let mut children = document.node(ROOT).unwrap().children.clone();
    children.push(replacement_id);
    let replacement = apply(
        &mut document,
        &resources,
        vec![
            UiOperation::Create(Box::new(replacement)),
            UiOperation::Children {
                node: ROOT,
                value: children,
            },
        ],
    );
    assert!(!document.accepts_dispatch(&activation));
    router
        .adopt(replacement.input, UiInputEvidence::Published)
        .unwrap();
    let mut snapshot = document.snapshot();
    snapshot.revision += 1;
    snapshot.next_sequence += 1;
    let next = document
        .recover(
            snapshot,
            &resources,
            BackendCapabilities::default(),
            constraints(),
            [160.0, 100.0],
        )
        .unwrap();
    router
        .adopt(next.input, UiInputEvidence::Published)
        .unwrap();
    assert!(!document.accepts_dispatch(&activation));
    let old_id = router.scene_id().unwrap();
    let mut snapshot = document.snapshot();
    snapshot.epoch = 2;
    let (_, next) = UiDocument::from_snapshot(
        snapshot,
        &resources,
        UiLimits::default(),
        BackendCapabilities::default(),
        constraints(),
        [160.0, 100.0],
    )
    .unwrap();
    assert_eq!(
        router
            .adopt(next.input.clone(), UiInputEvidence::Published)
            .unwrap_err(),
        UiInputError::WrongEpoch
    );
    assert_eq!(router.scene_id(), Some(old_id));
    router.reset();
    router
        .adopt(next.input, UiInputEvidence::Published)
        .unwrap();
}

#[test]
fn invalid_input_policy_host_and_schema_fail_atomically() {
    fn transferable<T: Send + Sync>() {}
    transferable::<UiInputScene>();
    transferable::<UiInputEffects>();
    transferable::<UiInputRouter>();
    let (mut document, resources, first) = fixture();
    let original = document.snapshot();
    let policy = UiInteraction {
        activate_handler: Some(UiHandlerId(0)),
        ..Default::default()
    };
    let bad = batch(
        &document,
        vec![
            UiOperation::Interaction {
                node: BUTTON,
                value: policy,
            },
            UiOperation::Interaction {
                node: BUTTON,
                value: control(1),
            },
        ],
    );
    assert!(matches!(
        document.apply(
            bad,
            &resources,
            BackendCapabilities::default(),
            constraints(),
            [160.0, 100.0]
        ),
        Err(UiError::InvalidHandler)
    ));
    assert_eq!(document.snapshot(), original);
    let mut router = UiInputRouter::new();
    assert_eq!(
        router.route(UiInput::Cancel).unwrap_err(),
        UiInputError::NoScene
    );
    router
        .adopt(first.input, UiInputEvidence::Published)
        .unwrap();
    router.route(UiInput::PrimaryDown([65.0, 5.0])).unwrap();
    assert_eq!(
        router
            .route(UiInput::PrimaryUp([f32::NAN, 5.0]))
            .unwrap_err(),
        UiInputError::InvalidPosition
    );
    assert_eq!(router.captured(), Some(LABEL));
    let mut policy = control(1);
    policy.disabled = true;
    let bad = batch(
        &document,
        vec![UiOperation::Interaction {
            node: BUTTON,
            value: policy,
        }],
    );
    assert!(
        document
            .apply(
                bad,
                &resources,
                BackendCapabilities::default(),
                constraints(),
                [f32::NAN, 100.0]
            )
            .is_err()
    );
    assert_eq!(document.snapshot(), original);
    for schema in [1, 2] {
        let mut snapshot = original.clone();
        snapshot.schema = schema;
        assert!(
            matches!(UiDocument::from_snapshot(snapshot,&resources,UiLimits::default(),BackendCapabilities::default(),constraints(),[160.0,100.0]),Err(UiError::UnsupportedSchema(value)) if value==schema)
        );
    }
}

#[test]
fn clip_scroll_and_reparent_keep_routing_tied_to_exact_geometry_and_ancestry() {
    let (mut document, resources, first) = fixture();
    let old = first.input.clone();
    let mut properties = document.node(BUTTON).unwrap().properties.clone();
    properties.clip = Some(Rect {
        x: 0.0,
        y: 0.0,
        width: 10.0,
        height: 30.0,
    });
    let clipped = apply(
        &mut document,
        &resources,
        vec![UiOperation::Properties {
            node: BUTTON,
            value: properties,
        }],
    );
    assert_eq!(clipped.input.hit_test([65.0, 5.0]), Some(LABEL));
    assert_eq!(clipped.input.hit_test([75.0, 5.0]), Some(ROOT));
    assert_eq!(old.hit_test([75.0, 5.0]), Some(LABEL));
    // A valid old click is rejected after its surviving target moves to a new
    // render parent, even when both original handlers remain on the old path.
    let mut router = UiInputRouter::new();
    router
        .adopt(clipped.input, UiInputEvidence::Published)
        .unwrap();
    router.route(UiInput::PrimaryDown([65.0, 5.0])).unwrap();
    let up = router.route(UiInput::PrimaryUp([65.0, 5.0])).unwrap();
    let event = up
        .dispatches
        .iter()
        .find(|e| e.kind == UiInputEventKind::PointerUp)
        .unwrap()
        .clone();
    assert!(document.accepts_dispatch(&event));
    let host = UiNodeId::new(5, 1);
    let mut wrapper = UiNode::new(host, size(30.0, 20.0));
    wrapper.children = vec![LABEL];
    let moved = apply(
        &mut document,
        &resources,
        vec![
            UiOperation::Create(Box::new(wrapper)),
            UiOperation::Children {
                node: BUTTON,
                value: vec![host],
            },
        ],
    );
    assert!(!document.accepts_dispatch(&event));
    router
        .adopt(moved.input, UiInputEvidence::Published)
        .unwrap();
    router.route(UiInput::PrimaryDown([65.0, 5.0])).unwrap();
    let mut hidden = document.node(BUTTON).unwrap().layout.clone();
    hidden.display = LayoutDisplay::None;
    let hidden = apply(
        &mut document,
        &resources,
        vec![UiOperation::Layout {
            node: BUTTON,
            value: Box::new(hidden),
        }],
    );
    let effects = router
        .adopt(hidden.input, UiInputEvidence::Published)
        .unwrap();
    assert!(
        effects
            .dispatches
            .iter()
            .any(|e| e.kind == UiInputEventKind::PointerCancel)
    );
    assert_eq!(router.captured(), None);
    router.route(UiInput::Tab { backwards: false }).unwrap();
    assert_eq!(router.focused(), Some(NEXT));
}

#[test]
fn pointer_scene_follows_resolved_scroll_and_old_scene_is_transferable_after_owner_drop() {
    let root = ROOT;
    let child = BUTTON;
    let mut parent = UiNode::new(root, size(50.0, 30.0));
    parent.children = vec![child];
    parent.layout.overflow = LayoutSize::new(LayoutOverflow::Scroll, LayoutOverflow::Scroll);
    let mut item = UiNode::new(child, size(100.0, 60.0).flex_shrink(0.0));
    item.interaction = control(1);
    let resources = UiResources::new();
    let (mut document, first) = UiDocument::from_snapshot(
        UiSnapshot::new(400, 1, root, vec![parent, item]),
        &resources,
        UiLimits::default(),
        BackendCapabilities::default(),
        constraints(),
        [160.0, 100.0],
    )
    .unwrap();
    let mut properties = document.node(root).unwrap().properties.clone();
    properties.scroll = [20.0, 10.0];
    let next = apply(
        &mut document,
        &resources,
        vec![UiOperation::Properties {
            node: root,
            value: properties,
        }],
    );
    let resolved = next.frame.resolve();
    for point in [[1.0, 1.0], [49.0, 29.0], [51.0, 1.0], [1.0, 31.0]] {
        let internal = next.frame.hit_test(point[0], point[1]);
        let external = next.input.hit_test(point);
        assert_eq!(external.is_some(), internal.is_some());
        assert_eq!(internal, resolved.hit_test(point[0], point[1]));
    }
    let scene = next.input;
    drop((document, resources));
    std::thread::spawn(move || {
        let mut router = UiInputRouter::new();
        router
            .adopt(scene, UiInputEvidence::ImageInstalled)
            .unwrap();
        router.route(UiInput::PrimaryDown([1.0, 1.0])).unwrap();
        assert_eq!(
            activates(&router.route(UiInput::PrimaryUp([1.0, 1.0])).unwrap()),
            vec![BUTTON]
        );
    })
    .join()
    .unwrap();
    assert_eq!(first.input.hit_test([1.0, 1.0]), Some(BUTTON));
}

#[test]
fn pass_through_preserves_descendants_and_nested_controls_do_not_activate_each_other() {
    let (mut document, resources, first) = fixture();
    let mut router = UiInputRouter::new();
    router
        .adopt(first.input, UiInputEvidence::Published)
        .unwrap();
    let mut label = document.node(LABEL).unwrap().interaction;
    label.focus = UiFocusPolicy::Tab;
    label.focus_handler = Some(UiHandlerId(210));
    label.activate_handler = Some(UiHandlerId(200));
    let mut button = document.node(BUTTON).unwrap().interaction;
    button.pointer_target = false;
    let publication = apply(
        &mut document,
        &resources,
        vec![
            UiOperation::Interaction {
                node: LABEL,
                value: label,
            },
            UiOperation::Interaction {
                node: BUTTON,
                value: button,
            },
        ],
    );
    assert_eq!(publication.input.hit_test([65.0, 5.0]), Some(LABEL));
    assert_eq!(publication.input.hit_test([105.0, 20.0]), Some(ROOT));
    router
        .adopt(publication.input, UiInputEvidence::Published)
        .unwrap();
    router.route(UiInput::PrimaryDown([65.0, 5.0])).unwrap();
    assert_eq!(
        activates(&router.route(UiInput::PrimaryUp([65.0, 5.0])).unwrap()),
        vec![LABEL]
    );
    // Press the outer control, release over nested control: neither should
    // interpret the gesture as its own complete press/release activation.
    button.pointer_target = true;
    let publication = apply(
        &mut document,
        &resources,
        vec![UiOperation::Interaction {
            node: BUTTON,
            value: button,
        }],
    );
    router
        .adopt(publication.input, UiInputEvidence::Published)
        .unwrap();
    router.route(UiInput::PrimaryDown([105.0, 20.0])).unwrap();
    assert!(activates(&router.route(UiInput::PrimaryUp([65.0, 5.0])).unwrap()).is_empty());
    router.route(UiInput::PrimaryDown([65.0, 5.0])).unwrap();
    let mut snapshot = document.snapshot();
    let preserved = snapshot
        .nodes
        .iter_mut()
        .find(|node| node.id == LABEL)
        .unwrap();
    preserved.style_state.hovered = true;
    preserved.style_state.pressed = true;
    preserved.style_state.focus_visible = true;
    snapshot.revision += 1;
    snapshot.next_sequence += 1;
    let recovered = document
        .recover(
            snapshot,
            &resources,
            BackendCapabilities::default(),
            constraints(),
            [160.0, 100.0],
        )
        .unwrap();
    let effects = router
        .adopt(recovered.input, UiInputEvidence::ImageInstalled)
        .unwrap();
    assert!(
        effects
            .dispatches
            .iter()
            .any(|event| event.kind == UiInputEventKind::PointerCancel)
    );
    assert!(
        effects
            .dispatches
            .iter()
            .any(|event| event.kind == UiInputEventKind::Blur)
    );
    assert_eq!(router.captured(), None);
    assert_eq!(router.focused(), None);
    let clears = effects.style_operations(&document);
    assert!(clears.iter().any(|operation|matches!(operation,UiOperation::StyleState {node,value} if *node==LABEL&&!value.pressed&&!value.focus_visible)));
}
