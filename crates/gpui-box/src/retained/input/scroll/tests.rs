use super::*;
use crate::retained::{style::*, ui_ir::*};
const ROOT: UiNodeId = UiNodeId::new(0, 1);
const OUTER: UiNodeId = UiNodeId::new(1, 1);
const INNER: UiNodeId = UiNodeId::new(2, 1);
const LEAF: UiNodeId = UiNodeId::new(3, 1);
const TAIL: UiNodeId = UiNodeId::new(4, 1);
fn fixed(width: f32, height: f32) -> LayoutStyle {
    LayoutStyle::column()
        .size(LayoutDimension::Px(width), LayoutDimension::Px(height))
        .flex_shrink(0.0)
}
fn snapshot() -> UiSnapshot {
    let mut root = UiNode::new(ROOT, fixed(160.0, 100.0));
    root.children = vec![OUTER];
    let mut outer = UiNode::new(
        OUTER,
        fixed(80.0, 60.0).overflow(LayoutOverflow::Scroll, LayoutOverflow::Scroll),
    );
    outer.children = vec![INNER, TAIL];
    outer.properties.color = 0x334455ff;
    outer.properties.opacity = 0.8;
    let mut inner = UiNode::new(
        INNER,
        fixed(70.0, 40.0).overflow(LayoutOverflow::Scroll, LayoutOverflow::Scroll),
    );
    inner.children = vec![LEAF];
    let mut leaf = UiNode::new(LEAF, fixed(100.0, 100.0));
    leaf.properties.color = 0xff0000ff;
    leaf.interaction.focus = UiFocusPolicy::Tab;
    leaf.interaction.activate_handler = Some(UiHandlerId(1));
    let mut tail = UiNode::new(TAIL, fixed(120.0, 100.0));
    tail.properties.color = 0x00ff00ff;
    UiSnapshot::new(800, 1, ROOT, vec![root, outer, inner, leaf, tail])
}
fn fixture() -> (UiDocument, UiResources, UiPublication, UiInputRouter) {
    let resources = UiResources::new();
    let (document, publication) = UiDocument::from_snapshot(
        snapshot(),
        &resources,
        UiLimits::default(),
        BackendCapabilities::default(),
        LayoutConstraints::viewport(160.0, 100.0),
        [160.0, 100.0],
    )
    .unwrap();
    let mut router = UiInputRouter::new();
    router
        .adopt(publication.input.clone(), UiInputEvidence::ImageInstalled)
        .unwrap();
    (document, resources, publication, router)
}
fn wheel(router: &mut UiInputRouter, delta: [f32; 2]) -> UiInputEffects {
    router
        .route(UiInput::Wheel {
            position: [10.0, 10.0],
            delta,
        })
        .unwrap()
}
fn apply(
    document: &mut UiDocument,
    resources: &UiResources,
    operations: Vec<UiOperation>,
) -> UiPublication {
    let batch = UiBatch {
        schema: UI_SCHEMA_VERSION,
        runtime: 800,
        epoch: 1,
        base_revision: document.revision(),
        revision: document.revision() + 1,
        sequence: document.next_sequence(),
        operations,
    };
    apply_batch(document, resources, batch)
}
fn apply_batch(
    document: &mut UiDocument,
    resources: &UiResources,
    batch: UiBatch,
) -> UiPublication {
    document
        .apply(
            batch,
            resources,
            BackendCapabilities::default(),
            LayoutConstraints::viewport(160.0, 100.0),
            [160.0, 100.0],
        )
        .unwrap()
}
fn scrolls(document: &UiDocument) -> ([f32; 2], [f32; 2]) {
    (
        document.node(INNER).unwrap().properties.scroll,
        document.node(OUTER).unwrap().properties.scroll,
    )
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
fn reference(inner: [f32; 2], outer: [f32; 2]) -> FramePacket {
    let mut tree = UiTree::new_layout(fixed(160.0, 100.0), 0).unwrap();
    tree.set_frame_build_policy(FrameBuildPolicy::Full);
    let outer_node = tree
        .append_layout(
            tree.root(),
            fixed(80.0, 60.0).overflow(LayoutOverflow::Scroll, LayoutOverflow::Scroll),
            0x334455ff,
        )
        .unwrap();
    let inner_node = tree
        .append_layout(
            outer_node,
            fixed(70.0, 40.0).overflow(LayoutOverflow::Scroll, LayoutOverflow::Scroll),
            0,
        )
        .unwrap();
    tree.append_layout(inner_node, fixed(100.0, 100.0), 0xff0000ff)
        .unwrap();
    tree.append_layout(outer_node, fixed(120.0, 100.0), 0x00ff00ff)
        .unwrap();
    let mut tx = Transaction::default();
    tx.scroll(inner_node, inner);
    tx.scroll(outer_node, outer);
    tx.opacity(outer_node, 0.8);
    tx.clip(
        outer_node,
        Some(Rect {
            x: 0.0,
            y: 0.0,
            width: 80.0,
            height: 60.0,
        }),
    );
    tx.clip(
        inner_node,
        Some(Rect {
            x: 0.0,
            y: 0.0,
            width: 70.0,
            height: 40.0,
        }),
    );
    tree.commit(tx, BackendCapabilities::default()).unwrap();
    tree.frame([160.0, 100.0]).unwrap()
}

#[test]
fn nested_axes_accumulate_before_adoption_and_match_independent_full_frames() {
    run(false);
}
#[cfg(feature = "retained-wgpu")]
#[test]
#[ignore = "requires WGPU adapter; run explicitly"]
fn hardware_wheel_chain_matches_complete_reference_images() {
    run(true);
}
fn run(hardware: bool) {
    let (mut doc, resources, initial, mut router) = fixture();
    assert_eq!(
        initial.input.scroll_state(INNER).unwrap().maximum,
        [30.0, 60.0]
    );
    assert_eq!(
        initial.input.scroll_state(OUTER).unwrap().maximum,
        [40.0, 80.0]
    );
    router.route(UiInput::Focus(Some(LEAF))).unwrap();
    #[cfg(feature = "retained-wgpu")]
    let mut gpu = hardware.then(|| wgpu::WgpuFrameRenderer::new(2).unwrap());
    #[cfg(feature = "retained-wgpu")]
    if let Some(gpu) = &gpu {
        eprintln!("wheel chain adapter: {:?}", gpu.adapter_info());
    }
    #[cfg(not(feature = "retained-wgpu"))]
    assert!(!hardware);
    for (delta, inner, outer, remaining) in [
        ([20.0, 45.0], [20.0, 45.0], [0.0, 0.0], [0.0, 0.0]),
        ([30.0, 40.0], [30.0, 60.0], [20.0, 25.0], [0.0, 0.0]),
        ([100.0, 100.0], [30.0, 60.0], [40.0, 80.0], [80.0, 45.0]),
        ([-15.0, -20.0], [15.0, 40.0], [40.0, 80.0], [0.0, 0.0]),
    ] {
        let effects = wheel(&mut router, delta);
        assert_eq!(
            effects.wheel_scrolls[0].scene,
            initial.input.id(),
            "old adopted geometry remains authoritative"
        );
        let plan = doc.plan_input_scroll(&effects).unwrap();
        assert_eq!(plan.outcomes[0].remaining, remaining);
        let result = apply_batch(&mut doc, &resources, plan.batch);
        assert_eq!(scrolls(&doc), (inner, outer));
        assert_eq!(router.focused(), Some(LEAF));
        assert!(
            effects
                .dispatches
                .iter()
                .all(|e| e.kind != UiInputEventKind::Activate)
        );
        let full = reference(inner, outer);
        assert_eq!(pixels(result.frame.resolve()), pixels(full.clone()));
        assert_eq!(doc.node(OUTER).unwrap().properties.opacity, 0.8);
        #[cfg(feature = "retained-wgpu")]
        if let Some(gpu) = &mut gpu {
            assert!(
                gpu.submit_local(&result.frame, [160, 100], 1.0)
                    .unwrap()
                    .is_some()
            );
            assert_eq!(
                gpu.image().unwrap().as_raw(),
                gpu.reference_image(&full, [160, 100], 1.0)
                    .unwrap()
                    .as_raw()
            );
        }
    }
    router
        .adopt(doc.input_scene(), UiInputEvidence::ImageInstalled)
        .unwrap();
    assert_eq!(router.scene_id(), Some(doc.input_scene().id()));
    assert_eq!(initial.input.hit_test([10.0, 10.0]), Some(LEAF));
    assert_eq!(
        router.hovered(),
        Some(TAIL),
        "new installed scroll geometry changes the stationary-pointer hit only on adoption"
    );
    let old_pixels = pixels(initial.frame.resolve());
    drop(doc);
    assert_eq!(old_pixels, pixels(initial.frame.resolve()));
    #[cfg(feature = "retained-wgpu")]
    if let Some(gpu) = &mut gpu {
        gpu.finish().unwrap();
    }
}

#[test]
fn batched_wheels_accumulate_and_exact_replay_does_not_scroll_twice() {
    let (mut doc, resources, _, mut router) = fixture();
    let mut effects = wheel(&mut router, [0.0, 30.0]);
    effects
        .wheel_scrolls
        .extend(wheel(&mut router, [0.0, 40.0]).wheel_scrolls);
    let plan = doc.plan_input_scroll(&effects).unwrap();
    assert_eq!(plan.outcomes.len(), 2);
    let batch = plan.batch.clone();
    let first = apply_batch(&mut doc, &resources, batch.clone());
    assert_eq!(scrolls(&doc), ([0.0, 60.0], [0.0, 10.0]));
    let replay = apply_batch(&mut doc, &resources, batch);
    assert_eq!(replay.status, UiApplyStatus::Replayed);
    assert_eq!(first.frame.revision(), replay.frame.revision());
    let stale = doc
        .plan_input_scroll(&wheel(&mut router, [0.0, 5.0]))
        .unwrap();
    apply(&mut doc, &resources, vec![]);
    assert!(matches!(
        doc.apply(
            stale.batch,
            &resources,
            BackendCapabilities::default(),
            LayoutConstraints::viewport(160.0, 100.0),
            [160.0, 100.0]
        ),
        Err(UiError::ResyncRequired { .. } | UiError::ReplayConflict)
    ));
    assert_eq!(scrolls(&doc), ([0.0, 60.0], [0.0, 10.0]));
}

#[test]
fn hidden_and_style_owned_axes_chain_without_overwriting_their_writer() {
    let (mut doc, resources, _, mut router) = fixture();
    let style = StyleRef::new(
        "scroll owner",
        vec![StyleDeclaration::Scroll(StyleRule::value([0.0, 10.0]))],
    )
    .unwrap();
    let publication = apply(
        &mut doc,
        &resources,
        vec![UiOperation::Styles {
            node: INNER,
            value: vec![style],
        }],
    );
    assert_eq!(
        publication.input.scroll_state(INNER).unwrap().wheel_axes,
        [false; 2]
    );
    let plan = doc
        .plan_input_scroll(&wheel(&mut router, [10.0, 20.0]))
        .unwrap();
    apply_batch(&mut doc, &resources, plan.batch);
    assert_eq!(scrolls(&doc), ([0.0; 2], [10.0, 20.0]));
    assert_eq!(
        doc.resolved_node(INNER).unwrap().properties.scroll,
        [0.0, 10.0]
    );
    let mut layout = doc.node(INNER).unwrap().layout.clone();
    layout.overflow.height = LayoutOverflow::Hidden;
    apply(
        &mut doc,
        &resources,
        vec![
            UiOperation::Styles {
                node: INNER,
                value: vec![],
            },
            UiOperation::Layout {
                node: INNER,
                value: Box::new(layout),
            },
            UiOperation::Scroll {
                node: INNER,
                value: [0.0, 15.0],
            },
        ],
    );
    let plan = doc
        .plan_input_scroll(&wheel(&mut router, [0.0, 10.0]))
        .unwrap();
    apply_batch(&mut doc, &resources, plan.batch);
    assert_eq!(scrolls(&doc), ([0.0, 15.0], [10.0, 30.0]));
}

#[test]
fn stale_reparent_disabled_recovery_and_invalid_delta_never_partially_apply() {
    let (mut doc, resources, initial, mut router) = fixture();
    let effects = wheel(&mut router, [0.0, 10.0]);
    let mut policy = doc.node(LEAF).unwrap().interaction;
    policy.disabled = true;
    apply(
        &mut doc,
        &resources,
        vec![UiOperation::Interaction {
            node: LEAF,
            value: policy,
        }],
    );
    assert_eq!(
        doc.plan_input_scroll(&effects).unwrap_err(),
        UiInputError::StaleScrollRoute
    );
    router
        .adopt(doc.input_scene(), UiInputEvidence::Published)
        .unwrap();
    let blocked = wheel(&mut router, [0.0, 10.0]);
    assert_eq!(blocked.wheel_scrolls[0].target, None);
    assert_eq!(
        doc.plan_input_scroll(&blocked).unwrap().outcomes[0].remaining,
        [0.0, 10.0]
    );
    assert_eq!(
        router
            .route(UiInput::Wheel {
                position: [10.0; 2],
                delta: [0.0, f32::NAN]
            })
            .unwrap_err(),
        UiInputError::InvalidScrollDelta
    );
    assert_eq!(
        router
            .route(UiInput::Wheel {
                position: [f32::INFINITY, 0.0],
                delta: [0.0; 2]
            })
            .unwrap_err(),
        UiInputError::InvalidPosition
    );
    let mut recovered = doc.snapshot();
    recovered.revision += 1;
    recovered.next_sequence += 1;
    doc.recover(
        recovered,
        &resources,
        BackendCapabilities::default(),
        LayoutConstraints::viewport(160.0, 100.0),
        [160.0, 100.0],
    )
    .unwrap();
    assert_eq!(
        doc.plan_input_scroll(&effects).unwrap_err(),
        UiInputError::StaleScrollRoute
    );
    assert_eq!(initial.input.scroll_state(INNER).unwrap().offset, [0.0; 2]);

    let (mut doc, resources, _, mut router) = fixture();
    let effects = wheel(&mut router, [0.0, 20.0]);
    apply(
        &mut doc,
        &resources,
        vec![
            UiOperation::Children {
                node: INNER,
                value: vec![],
            },
            UiOperation::Children {
                node: OUTER,
                value: vec![INNER, LEAF, TAIL],
            },
        ],
    );
    assert_eq!(
        doc.plan_input_scroll(&effects).unwrap_err(),
        UiInputError::StaleScrollRoute
    );
}

#[test]
fn live_offset_clamping_preserves_other_fields_and_handles_finite_huge_deltas() {
    let (mut doc, resources, _, mut router) = fixture();
    apply(
        &mut doc,
        &resources,
        vec![UiOperation::Scroll {
            node: INNER,
            value: [500.0, 500.0],
        }],
    );
    let plan = doc
        .plan_input_scroll(&wheel(&mut router, [-5.0, -5.0]))
        .unwrap();
    apply_batch(&mut doc, &resources, plan.batch);
    assert_eq!(
        doc.node(INNER).unwrap().properties.scroll,
        [25.0, 55.0],
        "start at visible clamped offset, not stale requested500"
    );
    let plan = doc
        .plan_input_scroll(&wheel(&mut router, [f32::MAX, f32::MAX]))
        .unwrap();
    assert_eq!(plan.outcomes[0].consumed, [45.0, 85.0]);
    assert!(plan.outcomes[0].remaining.iter().all(|v| v.is_finite()));
    apply_batch(&mut doc, &resources, plan.batch);
    assert_eq!(scrolls(&doc), ([30.0, 60.0], [40.0, 80.0]));
    let old = doc.published_frame();
    let plan = doc
        .plan_input_scroll(&wheel(&mut router, [10.0, 10.0]))
        .unwrap();
    let output = apply_batch(&mut doc, &resources, plan.batch);
    assert_eq!(old.revision(), output.frame.revision());
    assert!(!output.commit.request_frame);
    assert!(matches!(
        doc.apply(
            UiBatch {
                schema: UI_SCHEMA_VERSION,
                runtime: 800,
                epoch: 1,
                base_revision: doc.revision(),
                revision: doc.revision() + 1,
                sequence: doc.next_sequence(),
                operations: vec![UiOperation::Scroll {
                    node: INNER,
                    value: [0.0, -1.0]
                }]
            },
            &resources,
            BackendCapabilities::default(),
            LayoutConstraints::viewport(160.0, 100.0),
            [160.0, 100.0]
        ),
        Err(UiError::Retained(Error::InvalidScroll))
    ));
}

#[test]
fn wheel_ignores_primary_capture_and_only_adoption_moves_hit_geometry() {
    let (mut doc, resources, initial, mut router) = fixture();
    router.route(UiInput::PrimaryDown([10.0; 2])).unwrap();
    assert_eq!(router.captured(), Some(LEAF));
    let plan = doc
        .plan_input_scroll(&wheel(&mut router, [0.0, 20.0]))
        .unwrap();
    let output = apply_batch(&mut doc, &resources, plan.batch);
    assert_eq!(router.captured(), Some(LEAF));
    assert_eq!(router.focused(), Some(LEAF));
    assert_eq!(router.scene_id(), Some(initial.input.id()));
    router
        .adopt(output.input, UiInputEvidence::ImageInstalled)
        .unwrap();
    assert_eq!(router.captured(), Some(LEAF));
    let outside = router
        .route(UiInput::Wheel {
            position: [100.0, 80.0],
            delta: [0.0, 10.0],
        })
        .unwrap();
    assert_eq!(
        doc.plan_input_scroll(&outside).unwrap().outcomes[0].remaining,
        [0.0, 10.0]
    );
    assert_eq!(outside.wheel_scrolls[0].target, Some(ROOT));
}

#[test]
fn fractional_deltas_do_not_reverse_chain_residual_and_failed_batch_keeps_offsets() {
    let (mut doc, resources, _, mut router) = fixture();
    apply(
        &mut doc,
        &resources,
        vec![UiOperation::Scroll {
            node: INNER,
            value: [10.0, 10.0],
        }],
    );
    for delta in [0.0000006, -0.0000006] {
        let plan = doc
            .plan_input_scroll(&wheel(&mut router, [0.0, delta]))
            .unwrap();
        let result = &plan.outcomes[0];
        assert!(result.remaining[1] == 0.0 || result.remaining[1].signum() == delta.signum());
        assert!(result.consumed[1].abs() <= delta.abs());
        apply_batch(&mut doc, &resources, plan.batch);
    }
    let before = doc.snapshot();
    let frame = doc.published_frame();
    let mut plan = doc
        .plan_input_scroll(&wheel(&mut router, [0.0, 20.0]))
        .unwrap();
    plan.batch.operations.push(UiOperation::Scroll {
        node: OUTER,
        value: [f32::NAN, 0.0],
    });
    assert!(
        doc.apply(
            plan.batch,
            &resources,
            BackendCapabilities::default(),
            LayoutConstraints::viewport(160.0, 100.0),
            [160.0, 100.0]
        )
        .is_err()
    );
    assert_eq!(doc.snapshot(), before);
    assert_eq!(doc.published_frame().revision(), frame.revision());
    assert_send_sync::<UiScrollState>();
    assert_send_sync::<UiWheelScroll>();
    assert_send_sync::<UiScrollPlan>();
}
fn assert_send_sync<T: Send + Sync>() {}
