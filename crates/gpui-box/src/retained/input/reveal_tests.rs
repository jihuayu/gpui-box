use super::*;
use crate::retained::{style::*, ui_ir::*};
const ROOT: UiNodeId = UiNodeId::new(0, 1);
const OUTER: UiNodeId = UiNodeId::new(1, 1);
const PREFIX: UiNodeId = UiNodeId::new(2, 1);
const INNER: UiNodeId = UiNodeId::new(3, 1);
const FIRST: UiNodeId = UiNodeId::new(4, 1);
const TARGET: UiNodeId = UiNodeId::new(5, 1);
const TAIL: UiNodeId = UiNodeId::new(6, 1);
fn fixed(w: f32, h: f32) -> LayoutStyle {
    LayoutStyle::column()
        .size(LayoutDimension::Px(w), LayoutDimension::Px(h))
        .flex_shrink(0.0)
}
fn snapshot() -> UiSnapshot {
    let mut root = UiNode::new(ROOT, fixed(140.0, 100.0));
    root.children = vec![OUTER];
    let mut outer = UiNode::new(
        OUTER,
        fixed(100.0, 60.0).overflow(LayoutOverflow::Hidden, LayoutOverflow::Scroll),
    );
    outer.children = vec![PREFIX, INNER, TAIL];
    outer.properties.color = 0x334455ff;
    outer.properties.opacity = 0.8;
    let prefix = UiNode::new(PREFIX, fixed(100.0, 80.0));
    let mut inner = UiNode::new(
        INNER,
        fixed(90.0, 40.0).overflow(LayoutOverflow::Hidden, LayoutOverflow::Scroll),
    );
    inner.children = vec![FIRST, TARGET];
    let mut first = UiNode::new(FIRST, fixed(90.0, 70.0));
    first.properties.color = 0xff0000ff;
    let mut target = UiNode::new(TARGET, fixed(90.0, 20.0));
    target.properties.color = 0x00ff00ff;
    target.interaction.focus = UiFocusPolicy::Tab;
    target.interaction.focus_handler = Some(UiHandlerId(1));
    let tail = UiNode::new(TAIL, fixed(100.0, 100.0));
    UiSnapshot::new(
        900,
        1,
        ROOT,
        vec![root, outer, prefix, inner, first, target, tail],
    )
}
fn from_snapshot(snapshot: UiSnapshot) -> (UiDocument, UiResources, UiPublication, UiInputRouter) {
    let resources = UiResources::new();
    let (document, publication) = UiDocument::from_snapshot(
        snapshot,
        &resources,
        UiLimits::default(),
        BackendCapabilities::default(),
        LayoutConstraints::viewport(140.0, 100.0),
        [140.0, 100.0],
    )
    .unwrap();
    let mut router = UiInputRouter::new();
    router
        .adopt(publication.input.clone(), UiInputEvidence::ImageInstalled)
        .unwrap();
    (document, resources, publication, router)
}
fn apply(
    doc: &mut UiDocument,
    resources: &UiResources,
    operations: Vec<UiOperation>,
) -> Result<UiPublication, UiError> {
    let scene = doc.input_scene().id();
    doc.apply(
        UiBatch {
            schema: UI_SCHEMA_VERSION,
            runtime: scene.runtime,
            epoch: scene.epoch,
            base_revision: doc.revision(),
            revision: doc.revision() + 1,
            sequence: doc.next_sequence(),
            operations,
        },
        resources,
        BackendCapabilities::default(),
        LayoutConstraints::viewport(140.0, 100.0),
        [140.0, 100.0],
    )
}
fn pixels(frame: FramePacket) -> Vec<u8> {
    let mut job = software::SoftwareJob::new(
        frame,
        software::SoftwareOptions {
            size: [140, 100],
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
fn reference(inner_offset: f32, outer_offset: f32, target_height: f32) -> FramePacket {
    let mut tree = UiTree::new_layout(fixed(140.0, 100.0), 0).unwrap();
    tree.set_frame_build_policy(FrameBuildPolicy::Full);
    let outer = tree
        .append_layout(tree.root(), fixed(100.0, 60.0), 0x334455ff)
        .unwrap();
    tree.append_layout(outer, fixed(100.0, 80.0), 0).unwrap();
    let inner = tree.append_layout(outer, fixed(90.0, 40.0), 0).unwrap();
    let first = tree
        .append_layout(inner, fixed(90.0, 70.0), 0xff0000ff)
        .unwrap();
    let target = tree
        .append_layout(inner, fixed(90.0, target_height), 0x00ff00ff)
        .unwrap();
    tree.append_layout(outer, fixed(100.0, 100.0), 0).unwrap();
    // Independent numeric full reference: translate children, explicitly clip
    // inner's content, then outer's whole subtree. No scroll/reveal solver.
    let mut tx = Transaction::default();
    tx.translation(first, [0.0, -inner_offset]);
    tx.translation(target, [0.0, -inner_offset]);
    tx.translation(inner, [0.0, -outer_offset]);
    // Prefix and tail have no paint/hit assertion in this numeric pixel control.
    tx.clip(
        first,
        Some(Rect {
            x: 0.0,
            y: inner_offset,
            width: 90.0,
            height: 40.0,
        }),
    );
    tx.clip(
        target,
        Some(Rect {
            x: 0.0,
            y: inner_offset - 70.0,
            width: 90.0,
            height: 40.0,
        }),
    );
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
    tree.frame([140.0, 100.0]).unwrap()
}

#[test]
fn tab_reveals_nested_target_minimally_with_atomic_feedback_and_current_geometry() {
    run(false);
}
#[cfg(feature = "retained-wgpu")]
#[test]
#[ignore = "requires WGPU adapter; run explicitly"]
fn hardware_focus_reveal_matches_numeric_complete_reference() {
    run(true);
}
fn run(hardware: bool) {
    let (mut doc, resources, initial, mut router) = from_snapshot(snapshot());
    let effects = router.route(UiInput::Tab { backwards: false }).unwrap();
    assert_eq!(router.focused(), Some(TARGET));
    assert_eq!(effects.focus_reveals.len(), 1);
    assert_eq!(
        effects
            .dispatches
            .iter()
            .filter(|d| d.kind == UiInputEventKind::Focus)
            .count(),
        1
    );
    let plan = doc.plan_input_scroll(&effects).unwrap();
    let replay = plan.batch.clone();
    let first = doc
        .apply(
            plan.batch,
            &resources,
            BackendCapabilities::default(),
            LayoutConstraints::viewport(140.0, 100.0),
            [140.0, 100.0],
        )
        .unwrap();
    assert_eq!(doc.node(INNER).unwrap().properties.scroll, [0.0, 50.0]);
    assert_eq!(doc.node(OUTER).unwrap().properties.scroll, [0.0, 60.0]);
    assert!(doc.node(TARGET).unwrap().style_state.focus_visible);
    assert_eq!(first.commit.changed_nodes, 2);
    assert_eq!(first.commit.routes.len(), 2);
    assert_eq!(initial.input.hit_test([10.0, 50.0]), Some(PREFIX));
    assert_eq!(first.input.hit_test([10.0, 50.0]), Some(TARGET));
    assert_eq!(router.scene_id(), Some(initial.input.id()));
    assert_eq!(
        pixels(first.frame.resolve()),
        pixels(reference(50.0, 60.0, 20.0))
    );
    let again = doc
        .apply(
            replay,
            &resources,
            BackendCapabilities::default(),
            LayoutConstraints::viewport(140.0, 100.0),
            [140.0, 100.0],
        )
        .unwrap();
    assert_eq!(again.status, UiApplyStatus::Replayed);
    // Re-request with old adopted geometry uses current offsets and changes no frame.
    let plan = doc.plan_input_scroll(&effects).unwrap();
    let equal = doc
        .apply(
            plan.batch,
            &resources,
            BackendCapabilities::default(),
            LayoutConstraints::viewport(140.0, 100.0),
            [140.0, 100.0],
        )
        .unwrap();
    assert!(!equal.commit.request_frame);
    assert_eq!(first.frame.revision(), equal.frame.revision());
    // Explicit writes and new target height settle before reveal irrespective
    // of the intent's location in the operation list.
    let second = apply(
        &mut doc,
        &resources,
        vec![
            UiOperation::ScrollIntoView { node: TARGET },
            UiOperation::Scroll {
                node: INNER,
                value: [0.0, 0.0],
            },
            UiOperation::Layout {
                node: TARGET,
                value: Box::new(fixed(90.0, 30.0)),
            },
        ],
    )
    .unwrap();
    assert_eq!(doc.node(INNER).unwrap().properties.scroll, [0.0, 60.0]);
    assert_eq!(doc.node(OUTER).unwrap().properties.scroll, [0.0, 60.0]);
    assert_eq!(
        pixels(second.frame.resolve()),
        pixels(reference(60.0, 60.0, 30.0))
    );
    #[cfg(feature = "retained-wgpu")]
    if hardware {
        let mut gpu = wgpu::WgpuFrameRenderer::new(2).unwrap();
        eprintln!("focus reveal adapter: {:?}", gpu.adapter_info());
        for (output, full) in [
            (&first, reference(50.0, 60.0, 20.0)),
            (&second, reference(60.0, 60.0, 30.0)),
        ] {
            assert!(
                gpu.submit_local(&output.frame, [140, 100], 1.0)
                    .unwrap()
                    .is_some()
            );
            assert_eq!(
                gpu.image().unwrap().as_raw(),
                gpu.reference_image(&full, [140, 100], 1.0)
                    .unwrap()
                    .as_raw()
            );
        }
        gpu.finish().unwrap();
    }
    #[cfg(not(feature = "retained-wgpu"))]
    assert!(!hardware);
    router
        .adopt(second.input, UiInputEvidence::ImageInstalled)
        .unwrap();
    assert_eq!(router.focused(), Some(TARGET));
    let old = pixels(initial.frame.resolve());
    drop(doc);
    assert_eq!(old, pixels(initial.frame.resolve()));
}

#[test]
fn reveal_uses_final_focus_style_layout_and_never_overwrites_style_owned_scroll() {
    let mut source = snapshot();
    source
        .nodes
        .iter_mut()
        .find(|n| n.id == TARGET)
        .unwrap()
        .styles = vec![
        StyleRef::new(
            "focus-size",
            vec![StyleDeclaration::Height(
                StyleRule::value(LayoutDimension::Px(20.0)).when(
                    StyleCondition {
                        focus_visible: Some(true),
                        ..Default::default()
                    },
                    LayoutDimension::Px(35.0),
                ),
            )],
        )
        .unwrap(),
    ];
    let (mut doc, resources, _, mut router) = from_snapshot(source);
    let effects = router.route(UiInput::Focus(Some(TARGET))).unwrap();
    let plan = doc.plan_input_scroll(&effects).unwrap();
    let output = doc
        .apply(
            plan.batch,
            &resources,
            BackendCapabilities::default(),
            LayoutConstraints::viewport(140.0, 100.0),
            [140.0, 100.0],
        )
        .unwrap();
    assert_eq!(doc.node(INNER).unwrap().properties.scroll, [0.0, 65.0]);
    assert_eq!(doc.node(OUTER).unwrap().properties.scroll, [0.0, 60.0]);
    assert_eq!(
        pixels(output.frame.resolve()),
        pixels(reference(65.0, 60.0, 35.0))
    );
    let owned = StyleRef::new(
        "owned-offset",
        vec![StyleDeclaration::Scroll(StyleRule::value([0.0, 10.0]))],
    )
    .unwrap();
    apply(
        &mut doc,
        &resources,
        vec![
            UiOperation::Styles {
                node: INNER,
                value: vec![owned],
            },
            UiOperation::Scroll {
                node: OUTER,
                value: [0.0; 2],
            },
            UiOperation::ScrollIntoView { node: TARGET },
        ],
    )
    .unwrap();
    assert_eq!(
        doc.node(INNER).unwrap().properties.scroll,
        [0.0, 65.0],
        "style writer keeps authored offset untouched"
    );
    assert_eq!(
        doc.node(OUTER).unwrap().properties.scroll,
        [0.0, 0.0],
        "fully clipped target cannot be rescued by ancestor"
    );
}

#[test]
fn reveal_rejects_stale_target_and_final_disabled_or_failed_layout_atomically() {
    let (mut doc, resources, _, mut router) = from_snapshot(snapshot());
    let effects = router.route(UiInput::Tab { backwards: false }).unwrap();
    let old = doc.snapshot();
    let frame = doc.published_frame();
    let mut bad = fixed(90.0, 20.0);
    bad.size.height = LayoutDimension::Px(f32::NAN);
    assert!(
        apply(
            &mut doc,
            &resources,
            vec![
                UiOperation::ScrollIntoView { node: TARGET },
                UiOperation::Layout {
                    node: TARGET,
                    value: Box::new(bad)
                }
            ]
        )
        .is_err()
    );
    assert_eq!(doc.snapshot(), old);
    assert_eq!(doc.published_frame().revision(), frame.revision());
    let mut policy = doc.node(TARGET).unwrap().interaction;
    policy.disabled = true;
    assert!(matches!(
        apply(
            &mut doc,
            &resources,
            vec![
                UiOperation::ScrollIntoView { node: TARGET },
                UiOperation::Interaction {
                    node: TARGET,
                    value: policy
                }
            ]
        ),
        Err(UiError::IneligibleReveal(TARGET))
    ));
    assert_eq!(doc.snapshot(), old);
    apply(
        &mut doc,
        &resources,
        vec![
            UiOperation::Children {
                node: INNER,
                value: vec![FIRST],
            },
            UiOperation::Children {
                node: OUTER,
                value: vec![PREFIX, INNER, TARGET, TAIL],
            },
        ],
    )
    .unwrap();
    assert_eq!(
        doc.plan_input_scroll(&effects).unwrap_err(),
        UiInputError::StaleScrollRoute
    );
    assert!(
        apply(
            &mut doc,
            &resources,
            vec![UiOperation::ScrollIntoView {
                node: UiNodeId::new(99, 1)
            }]
        )
        .is_err()
    );
}

#[test]
fn same_batch_creation_and_multiple_reveals_use_final_tree_and_ordered_offsets() {
    let (mut doc, resources, _, _) = from_snapshot(snapshot());
    let new = UiNodeId::new(7, 1);
    apply(
        &mut doc,
        &resources,
        vec![
            UiOperation::ScrollIntoView { node: new },
            UiOperation::Create(Box::new(UiNode::new(new, fixed(90.0, 30.0)))),
            UiOperation::Children {
                node: INNER,
                value: vec![FIRST, TARGET, new],
            },
        ],
    )
    .unwrap();
    assert_eq!(doc.node(INNER).unwrap().properties.scroll, [0.0, 80.0]);
    apply(
        &mut doc,
        &resources,
        vec![
            UiOperation::ScrollIntoView { node: new },
            UiOperation::ScrollIntoView { node: FIRST },
        ],
    )
    .unwrap();
    assert_eq!(
        doc.node(INNER).unwrap().properties.scroll,
        [0.0, 30.0],
        "oversized first box selects nearest edge"
    );
    assert_eq!(
        doc.snapshot()
            .nodes
            .iter()
            .find(|n| n.id == INNER)
            .unwrap()
            .properties
            .scroll,
        [0.0, 30.0]
    );
}

#[test]
fn hidden_axes_reveal_while_clip_and_visible_do_not_and_oversized_target_is_stable() {
    for mode in [
        LayoutOverflow::Hidden,
        LayoutOverflow::Clip,
        LayoutOverflow::Visible,
    ] {
        let mut source = snapshot();
        source
            .nodes
            .iter_mut()
            .find(|n| n.id == INNER)
            .unwrap()
            .layout = fixed(90.0, 40.0).overflow(LayoutOverflow::Visible, mode);
        let (mut doc, resources, _, _) = from_snapshot(source);
        apply(
            &mut doc,
            &resources,
            vec![UiOperation::ScrollIntoView { node: TARGET }],
        )
        .unwrap();
        let expected = if mode == LayoutOverflow::Hidden {
            50.0
        } else {
            0.0
        };
        assert_eq!(doc.node(INNER).unwrap().properties.scroll, [0.0, expected]);
        if mode == LayoutOverflow::Clip {
            assert_eq!(doc.node(OUTER).unwrap().properties.scroll, [0.0, 0.0]);
        }
    }
    let (mut doc, resources, _, _) = from_snapshot(snapshot());
    apply(
        &mut doc,
        &resources,
        vec![
            UiOperation::Layout {
                node: TARGET,
                value: Box::new(fixed(90.0, 80.0)),
            },
            UiOperation::ScrollIntoView { node: TARGET },
        ],
    )
    .unwrap();
    assert_eq!(doc.node(INNER).unwrap().properties.scroll, [0.0, 70.0]);
    let before = doc.published_frame();
    apply(
        &mut doc,
        &resources,
        vec![UiOperation::ScrollIntoView { node: TARGET }],
    )
    .unwrap();
    assert_eq!(doc.published_frame().revision(), before.revision());
}

#[test]
fn transformed_target_does_not_invent_layout_scroll_extent() {
    let mut source = snapshot();
    source
        .nodes
        .iter_mut()
        .find(|n| n.id == INNER)
        .unwrap()
        .layout = fixed(90.0, 40.0).overflow(LayoutOverflow::Hidden, LayoutOverflow::Hidden);
    source
        .nodes
        .iter_mut()
        .find(|n| n.id == TARGET)
        .unwrap()
        .properties
        .translation = [100.0, 0.0];
    let (mut doc, resources, _, _) = from_snapshot(source);
    // Transforms are visual and do not enlarge layout scroll extents: clamp
    // rather than inventing more layout overflow to reveal a transformed box.
    apply(
        &mut doc,
        &resources,
        vec![UiOperation::ScrollIntoView { node: TARGET }],
    )
    .unwrap();
    assert_eq!(doc.node(INNER).unwrap().properties.scroll[0], 0.0);
    assert_eq!(doc.node(INNER).unwrap().properties.scroll[1], 50.0);
}

#[test]
fn two_axis_reveal_clamps_independently_and_pointer_focus_does_not_jump() {
    let mut source = snapshot();
    source
        .nodes
        .iter_mut()
        .find(|n| n.id == INNER)
        .unwrap()
        .layout = LayoutStyle::row()
        .size(LayoutDimension::Px(90.0), LayoutDimension::Px(40.0))
        .flex_shrink(0.0)
        .overflow(LayoutOverflow::Hidden, LayoutOverflow::Hidden);
    source
        .nodes
        .iter_mut()
        .find(|n| n.id == FIRST)
        .unwrap()
        .layout = fixed(100.0, 70.0);
    let mut target = fixed(30.0, 20.0);
    target.margin.top = LayoutLength::Px(50.0);
    source
        .nodes
        .iter_mut()
        .find(|n| n.id == TARGET)
        .unwrap()
        .layout = target;
    let (mut doc, resources, _, mut router) = from_snapshot(source);
    assert!(
        router
            .route(UiInput::PrimaryDown([10.0, 10.0]))
            .unwrap()
            .focus_reveals
            .is_empty()
    );
    apply(
        &mut doc,
        &resources,
        vec![UiOperation::ScrollIntoView { node: TARGET }],
    )
    .unwrap();
    assert_eq!(doc.node(INNER).unwrap().properties.scroll, [40.0, 30.0]);
    assert_eq!(doc.node(OUTER).unwrap().properties.scroll, [0.0, 60.0]);
    assert_eq!(doc.input_scene().hit_test([70.0, 50.0]), Some(TARGET));
    let before = doc.snapshot();
    let mut hidden = doc.node(TARGET).unwrap().layout.clone();
    hidden.display = LayoutDisplay::None;
    assert!(matches!(
        apply(
            &mut doc,
            &resources,
            vec![
                UiOperation::ScrollIntoView { node: TARGET },
                UiOperation::Layout {
                    node: TARGET,
                    value: Box::new(hidden)
                }
            ]
        ),
        Err(UiError::IneligibleReveal(TARGET))
    ));
    assert_eq!(doc.snapshot(), before);
}

#[test]
fn explicit_scroll_host_clip_constrains_reveal_and_requested_offsets_reclamp() {
    let (mut doc, resources, _, _) = from_snapshot(snapshot());
    let mut properties = doc.node(INNER).unwrap().properties.clone();
    properties.clip = Some(Rect {
        x: 0.0,
        y: 0.0,
        width: 90.0,
        height: 25.0,
    });
    let next = apply(
        &mut doc,
        &resources,
        vec![
            UiOperation::Properties {
                node: INNER,
                value: properties,
            },
            UiOperation::Scroll {
                node: INNER,
                value: [0.0, 500.0],
            },
            UiOperation::ScrollIntoView { node: TARGET },
        ],
    )
    .unwrap();
    assert_eq!(
        doc.node(INNER).unwrap().properties.scroll,
        [0.0, 500.0],
        "already-clamped nearest result need not rewrite authored overshoot"
    );
    assert_eq!(next.input.scroll_state(INNER).unwrap().offset, [0.0, 50.0]);
    assert_eq!(doc.node(OUTER).unwrap().properties.scroll, [0.0, 45.0]);
    assert_eq!(next.input.hit_test([10.0, 57.0]), Some(TARGET));
}
