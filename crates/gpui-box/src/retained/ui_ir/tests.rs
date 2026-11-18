use super::*;

const ROOT: UiNodeId = UiNodeId::new(0, 1);
const A: UiNodeId = UiNodeId::new(1, 1);
const B: UiNodeId = UiNodeId::new(2, 1);
fn bounds() -> LayoutConstraints {
    LayoutConstraints::viewport(180.0, 180.0)
}
fn fixed(w: f32, h: f32) -> LayoutStyle {
    LayoutStyle::row()
        .size(LayoutDimension::Px(w), LayoutDimension::Px(h))
        .flex_shrink(0.0)
}
fn snapshot() -> UiSnapshot {
    let mut root = UiNode::new(ROOT, fixed(120.0, 80.0));
    root.layout.flex_direction = LayoutFlexDirection::Column;
    root.children = vec![A, B];
    let mut a = UiNode::new(A, fixed(20.0, 10.0));
    a.properties.color = 0xff0000ff;
    let mut b = UiNode::new(B, fixed(30.0, 10.0));
    b.properties.color = 0x00ff00ff;
    // Input record order is independent of graph order.
    UiSnapshot::new(7, 3, ROOT, vec![b, root, a])
}
fn document(resources: &UiResources) -> (UiDocument, UiPublication) {
    UiDocument::from_snapshot(
        snapshot(),
        resources,
        UiLimits::default(),
        BackendCapabilities {
            solid_color_patch: true,
        },
        bounds(),
        [180.0; 2],
    )
    .unwrap()
}
fn batch(doc: &UiDocument, operations: Vec<UiOperation>) -> UiBatch {
    UiBatch {
        schema: UI_SCHEMA_VERSION,
        runtime: 7,
        epoch: 3,
        base_revision: doc.revision(),
        revision: doc.revision() + 1,
        sequence: doc.next_sequence(),
        operations,
    }
}
fn apply(
    doc: &mut UiDocument,
    batch: UiBatch,
    resources: &UiResources,
) -> Result<UiPublication, UiError> {
    doc.apply(
        batch,
        resources,
        BackendCapabilities {
            solid_color_patch: true,
        },
        bounds(),
        [180.0; 2],
    )
}
fn pixels(frame: FramePacket) -> Vec<u8> {
    pixels_with_effects(frame, pacing::SoftwareEffects::Exact)
}
fn pixels_with_effects(frame: FramePacket, effects: pacing::SoftwareEffects) -> Vec<u8> {
    let mut job = software::SoftwareJob::new(
        frame,
        software::SoftwareOptions {
            size: [180, 180],
            scale: 1.0,
            background: 0,
            effects,
            max_bytes: 1024 * 1024,
        },
    )
    .unwrap();
    while !job.step(4096).unwrap() {}
    job.into_frame().unwrap().pixels().to_vec()
}

#[test]
fn owned_messages_and_complete_publications_are_transferable() {
    fn transferable<T: Send + Sync>() {}
    transferable::<UiSnapshot>();
    transferable::<UiBatch>();
    transferable::<UiPublication>();
    let resources = UiResources::new();
    let input = std::thread::spawn(snapshot).join().unwrap();
    let (document, output) = UiDocument::from_snapshot(
        input,
        &resources,
        UiLimits::default(),
        BackendCapabilities::default(),
        bounds(),
        [180.0; 2],
    )
    .unwrap();
    drop(document);
    let hit = std::thread::spawn(move || output.hit_test(5.0, 5.0))
        .join()
        .unwrap();
    assert_eq!(hit, Some(A));
}

#[test]
fn property_batches_keep_normal_routes_noop_frames_and_exact_replay() {
    let resources = UiResources::new();
    let (mut doc, first) = document(&resources);
    let old_pixels = pixels(first.frame.resolve());
    let mut props = doc.node(A).unwrap().properties.clone();
    props.color = 0x0000ffff;
    let update = batch(
        &doc,
        vec![UiOperation::Properties {
            node: A,
            value: props.clone(),
        }],
    );
    let result = apply(&mut doc, update.clone(), &resources).unwrap();
    assert_eq!(result.frame.resolve().layout_runs, 0);
    assert_eq!(result.frame.resolve().recorded_chunks, 0);
    assert!(
        result
            .commit
            .routes
            .iter()
            .any(|(_, route)| *route == UpdatePath::InstancePatch)
    );
    assert_eq!(result.hit_test(5.0, 5.0), Some(A));
    assert_eq!(doc.revision(), 1);
    let replay = doc
        .apply(
            update.clone(),
            &resources,
            BackendCapabilities::default(),
            LayoutConstraints::viewport(1.0, 1.0),
            [1.0; 2],
        )
        .unwrap();
    assert_eq!(replay.status, UiApplyStatus::Replayed);
    assert_eq!(replay.frame.revision(), result.frame.revision());
    assert_eq!(
        pixels(replay.frame.resolve()),
        pixels(result.frame.resolve())
    );
    let mut conflict = update;
    conflict.operations.clear();
    assert!(matches!(
        apply(&mut doc, conflict, &resources),
        Err(UiError::ReplayConflict)
    ));
    let no_op = batch(
        &doc,
        vec![
            UiOperation::Properties {
                node: A,
                value: UiProperties {
                    color: 5,
                    ..props.clone()
                },
            },
            UiOperation::Properties {
                node: A,
                value: props,
            },
        ],
    );
    let no_op = apply(&mut doc, no_op, &resources).unwrap();
    assert_eq!(doc.revision(), 2);
    assert_eq!(no_op.frame.revision(), result.frame.revision());
    assert!(!no_op.commit.request_frame);
    assert_eq!(no_op.frame.resolve().layout_runs, 0);
    drop(doc);
    assert_eq!(pixels(first.frame.resolve()), old_pixels);
    assert_eq!(first.hit_test(5.0, 5.0), Some(A));
}

#[test]
fn mixed_grid_flex_topology_moves_and_order_match_fresh_complete_snapshots() {
    let resources = UiResources::new();
    let (mut doc, old) = document(&resources);
    let group = UiNodeId::new(9, 1);
    let mut grid = UiNode::new(
        group,
        LayoutStyle::grid(GridStyle::default().columns(vec![GridTrack::fr(1.0).into(); 2]))
            .size(LayoutDimension::Px(120.0), LayoutDimension::Auto),
    );
    grid.children = vec![B, A];
    let update = batch(
        &doc,
        vec![
            UiOperation::Create(grid),
            UiOperation::Children {
                node: ROOT,
                value: vec![group],
            },
            UiOperation::Layout {
                node: A,
                value: fixed(30.0, 20.0)
                    .grid_column(GridPlacement::Line(2), GridPlacement::Auto)
                    .order(-1),
            },
        ],
    );
    let next = apply(&mut doc, update, &resources).unwrap();
    let (fresh, reference) = UiDocument::from_snapshot(
        doc.snapshot(),
        &resources,
        UiLimits::default(),
        BackendCapabilities::default(),
        bounds(),
        [180.0; 2],
    )
    .unwrap();
    assert_eq!(
        pixels(next.frame.resolve()),
        pixels(reference.frame.resolve())
    );
    for id in [ROOT, group, A, B] {
        assert_eq!(
            doc.layout_result(id).unwrap(),
            fresh.layout_result(id).unwrap()
        );
    }
    let before_a = doc
        .published_frame()
        .resolve()
        .items
        .iter()
        .find(|p| p.fill_color == 0xff0000ff)
        .unwrap()
        .node;
    let update = batch(
        &doc,
        vec![
            UiOperation::Children {
                node: group,
                value: vec![B],
            },
            UiOperation::Children {
                node: ROOT,
                value: vec![A],
            },
            UiOperation::RemoveSubtree(group),
        ],
    );
    apply(&mut doc, update, &resources).unwrap();
    assert_eq!(
        doc.published_frame()
            .resolve()
            .items
            .iter()
            .find(|p| p.fill_color == 0xff0000ff)
            .unwrap()
            .node,
        before_a
    );
    assert!(doc.node(B).is_none());
    assert!(doc.node(group).is_none());
    assert_eq!(old.hit_test(5.0, 15.0), Some(B));
}

#[test]
fn invalid_batches_never_publish_and_can_retry_same_sequence() {
    let resources = UiResources::new();
    let (mut doc, output) = document(&resources);
    let original = doc.snapshot();
    let original_pixels = pixels(output.frame.resolve());
    for operations in [
        vec![UiOperation::Children {
            node: A,
            value: vec![ROOT],
        }],
        vec![UiOperation::Children {
            node: ROOT,
            value: vec![A, A],
        }],
        vec![UiOperation::Children {
            node: ROOT,
            value: vec![A],
        }],
        vec![UiOperation::Content {
            node: ROOT,
            value: UiContent::Fixed {
                size: [2.0, 3.0],
                first_baseline: None,
                last_baseline: None,
            },
        }],
        vec![UiOperation::Properties {
            node: A,
            value: UiProperties {
                opacity: f32::NAN,
                ..Default::default()
            },
        }],
        vec![UiOperation::Content {
            node: A,
            value: UiContent::Image(UiResourceId(55)),
        }],
    ] {
        let invalid = batch(&doc, operations);
        assert!(apply(&mut doc, invalid, &resources).is_err());
        assert_eq!(doc.snapshot(), original);
        assert_eq!(pixels(doc.published_frame().resolve()), original_pixels);
    }
    let update = batch(
        &doc,
        vec![UiOperation::Layout {
            node: A,
            value: fixed(40.0, 10.0),
        }],
    );
    assert!(
        doc.apply(
            update.clone(),
            &resources,
            BackendCapabilities::default(),
            bounds(),
            [f32::NAN, 180.0]
        )
        .is_err()
    );
    assert_eq!(doc.snapshot(), original);
    apply(&mut doc, update, &resources).unwrap();
    assert_eq!(doc.layout_result(A).unwrap().size[0], 40.0);
}

#[test]
fn generations_and_snapshot_recovery_reject_aba_regressions() {
    let resources = UiResources::new();
    let (mut doc, old) = document(&resources);
    let update = batch(&doc, vec![UiOperation::RemoveSubtree(A)]);
    apply(&mut doc, update, &resources).unwrap();
    let old_generation = batch(
        &doc,
        vec![UiOperation::Create(UiNode::new(A, fixed(5.0, 5.0)))],
    );
    assert!(matches!(
        apply(&mut doc, old_generation, &resources),
        Err(UiError::InvalidIdentity(A))
    ));
    let new_a = UiNodeId::new(A.slot, 2);
    let update = batch(
        &doc,
        vec![
            UiOperation::Create(UiNode::new(new_a, fixed(5.0, 5.0))),
            UiOperation::Children {
                node: ROOT,
                value: vec![new_a, B],
            },
        ],
    );
    apply(&mut doc, update, &resources).unwrap();
    assert_eq!(old.hit_test(5.0, 5.0), Some(A));
    assert_eq!(doc.hit_test(1.0, 1.0), Some(new_a));
    let mut snapshot = doc.snapshot();
    snapshot.revision += 4;
    snapshot.next_sequence += 4;
    snapshot
        .nodes
        .iter_mut()
        .find(|n| n.id == new_a)
        .unwrap()
        .properties
        .color = 0x0000ffff;
    let next = doc
        .recover(
            snapshot,
            &resources,
            BackendCapabilities::default(),
            bounds(),
            [180.0; 2],
        )
        .unwrap();
    assert_eq!(next.revision, 6);
    let mut invalid = doc.snapshot();
    invalid.revision += 1;
    invalid.next_sequence += 1;
    invalid.generations.retain(|(slot, _)| *slot != A.slot);
    assert!(
        doc.recover(
            invalid,
            &resources,
            BackendCapabilities::default(),
            bounds(),
            [180.0; 2]
        )
        .is_err()
    );
    let update = UiBatch {
        schema: UI_SCHEMA_VERSION,
        runtime: 7,
        epoch: 3,
        base_revision: 2,
        revision: 3,
        sequence: 3,
        operations: vec![],
    };
    assert!(matches!(
        apply(&mut doc, update, &resources),
        Err(UiError::ResyncRequired { .. })
    ));
}

#[test]
fn images_fixed_content_and_boxes_share_normal_intrinsic_and_resource_lifetimes() {
    let mut resources = UiResources::new();
    let image = ImageResource::rgba8([4, 2], vec![255; 32]).unwrap();
    resources.image(UiResourceId(1), image.clone()).unwrap();
    resources.image(UiResourceId(1), image.clone()).unwrap();
    assert!(matches!(
        resources.image(
            UiResourceId(1),
            ImageResource::rgba8([1, 1], vec![1; 4]).unwrap()
        ),
        Err(UiError::ResourceConflict(_))
    ));
    let (mut doc, _) = document(&resources);
    for content in [
        UiContent::Image(UiResourceId(1)),
        UiContent::Fixed {
            size: [15.0, 20.0],
            first_baseline: Some(8.0),
            last_baseline: Some(18.0),
        },
        UiContent::Image(UiResourceId(1)),
        UiContent::Box,
    ] {
        let update = batch(
            &doc,
            vec![
                UiOperation::Layout {
                    node: A,
                    value: LayoutStyle::row().flex_shrink(0.0),
                },
                UiOperation::Content {
                    node: A,
                    value: content,
                },
            ],
        );
        let output = apply(&mut doc, update, &resources).unwrap();
        let (_, fresh) = UiDocument::from_snapshot(
            doc.snapshot(),
            &resources,
            UiLimits::default(),
            BackendCapabilities::default(),
            bounds(),
            [180.0; 2],
        )
        .unwrap();
        assert_eq!(
            pixels(output.frame.resolve()),
            pixels(fresh.frame.resolve())
        );
    }
    let other_table = UiResources::new();
    let update = batch(&doc, vec![]);
    assert!(matches!(
        apply(&mut doc, update, &other_table),
        Err(UiError::WrongResourceTable)
    ));
}

#[test]
fn headers_and_receiver_limits_reject_before_state_changes() {
    let resources = UiResources::new();
    let (mut doc, _) = document(&resources);
    let original = doc.snapshot();
    for change in 0..5 {
        let mut update = batch(&doc, vec![]);
        match change {
            0 => update.schema += 1,
            1 => update.epoch += 1,
            2 => update.runtime += 1,
            3 => update.sequence += 1,
            _ => update.revision += 1,
        }
        assert!(apply(&mut doc, update, &resources).is_err());
        assert_eq!(doc.snapshot(), original);
    }
    for limits in [
        UiLimits {
            max_nodes: 2,
            ..Default::default()
        },
        UiLimits {
            max_depth: 1,
            ..Default::default()
        },
        UiLimits {
            max_generation_slots: 2,
            ..Default::default()
        },
        UiLimits {
            max_children_per_node: 1,
            ..Default::default()
        },
    ] {
        assert!(matches!(
            UiDocument::from_snapshot(
                snapshot(),
                &resources,
                limits,
                BackendCapabilities::default(),
                bounds(),
                [180.0; 2]
            ),
            Err(UiError::LimitExceeded)
        ));
    }
    let (mut constrained, _) = UiDocument::from_snapshot(
        snapshot(),
        &resources,
        UiLimits {
            max_operations: 1,
            max_shadows_per_node: 0,
            max_grid_track_entries: 1,
            ..Default::default()
        },
        BackendCapabilities::default(),
        bounds(),
        [180.0; 2],
    )
    .unwrap();
    for operations in [
        vec![
            UiOperation::Layout {
                node: A,
                value: fixed(10.0, 10.0)
            };
            2
        ],
        vec![UiOperation::Properties {
            node: A,
            value: UiProperties {
                shadows: vec![BoxShadow::default()].into(),
                ..Default::default()
            },
        }],
        vec![UiOperation::Layout {
            node: ROOT,
            value: LayoutStyle::grid(
                GridStyle::default().columns(vec![GridTrack::px(10.0).into(); 2]),
            ),
        }],
    ] {
        let update = batch(&constrained, operations);
        assert!(matches!(
            apply(&mut constrained, update, &resources),
            Err(UiError::LimitExceeded)
        ));
        assert_eq!(constrained.revision(), 0);
    }
}

#[test]
fn nested_creation_and_cancelled_reservations_preserve_final_graph_and_tombstones() {
    let resources = UiResources::new();
    let (mut doc, _) = document(&resources);
    let child = UiNodeId::new(8, 1);
    // Create can be referenced by an earlier Children operation, but updates
    // to the new node itself require its Create to have appeared already.
    let update = batch(
        &doc,
        vec![
            UiOperation::Children {
                node: A,
                value: vec![child],
            },
            UiOperation::Create(UiNode::new(child, fixed(5.0, 5.0))),
        ],
    );
    apply(&mut doc, update, &resources).unwrap();
    assert_eq!(doc.node(ROOT).unwrap().children, vec![A, B]);
    assert_eq!(doc.hit_test(1.0, 1.0), Some(child));
    let cancelled = UiNodeId::new(9, 1);
    let before = doc.published_frame().revision();
    let update = batch(
        &doc,
        vec![
            UiOperation::Create(UiNode::new(cancelled, fixed(5.0, 5.0))),
            UiOperation::RemoveSubtree(cancelled),
        ],
    );
    let no_visual = apply(&mut doc, update, &resources).unwrap();
    assert_eq!(no_visual.frame.revision(), before);
    assert!(doc.snapshot().generations.contains(&(cancelled.slot, 1)));
    let update = batch(
        &doc,
        vec![UiOperation::Create(UiNode::new(cancelled, fixed(5.0, 5.0)))],
    );
    assert!(matches!(
        apply(&mut doc, update, &resources),
        Err(UiError::InvalidIdentity(_))
    ));
    let mut recovery = doc.snapshot();
    recovery.revision += 10;
    recovery.next_sequence += 10;
    recovery.nodes.push(UiNode::new(cancelled, fixed(5.0, 5.0)));
    recovery
        .nodes
        .iter_mut()
        .find(|n| n.id == ROOT)
        .unwrap()
        .children
        .push(cancelled);
    assert!(matches!(
        doc.recover(
            recovery,
            &resources,
            BackendCapabilities::default(),
            bounds(),
            [180.0; 2]
        ),
        Err(UiError::InvalidIdentity(_))
    ));
}

#[test]
fn measurement_to_container_and_final_value_validation_are_atomic() {
    let resources = UiResources::new();
    let mut initial = snapshot();
    initial
        .nodes
        .iter_mut()
        .find(|n| n.id == A)
        .unwrap()
        .content = UiContent::Fixed {
        size: [20.0, 10.0],
        first_baseline: Some(5.0),
        last_baseline: Some(7.0),
    };
    let (mut doc, _) = UiDocument::from_snapshot(
        initial,
        &resources,
        UiLimits::default(),
        BackendCapabilities::default(),
        bounds(),
        [180.0; 2],
    )
    .unwrap();
    let invalid = batch(
        &doc,
        vec![
            UiOperation::Properties {
                node: A,
                value: UiProperties {
                    opacity: f32::NAN,
                    ..Default::default()
                },
            },
            UiOperation::Properties {
                node: A,
                value: UiProperties::default(),
            },
        ],
    );
    assert!(matches!(
        apply(&mut doc, invalid, &resources),
        Err(UiError::NonReflexiveValue)
    ));
    assert_eq!(doc.revision(), 0);
    // LayoutStyle permits an inactive physical gap under an axis override.
    // Its NaN still cannot enter an exact-replay source record.
    let mut inactive =
        fixed(20.0, 10.0).main_cross_gap(LayoutLength::Px(0.0), LayoutLength::Px(0.0));
    inactive.gap.width = LayoutLength::Px(f32::NAN);
    assert!(inactive.validate().is_ok());
    let invalid = batch(
        &doc,
        vec![UiOperation::Layout {
            node: A,
            value: inactive,
        }],
    );
    assert!(matches!(
        apply(&mut doc, invalid, &resources),
        Err(UiError::NonReflexiveValue)
    ));
    let child = UiNodeId::new(8, 1);
    let update = batch(
        &doc,
        vec![
            UiOperation::Properties {
                node: A,
                value: UiProperties {
                    opacity: 0.25,
                    ..Default::default()
                },
            },
            UiOperation::Create(UiNode::new(child, fixed(8.0, 8.0))),
            UiOperation::Children {
                node: A,
                value: vec![child],
            },
            UiOperation::Content {
                node: A,
                value: UiContent::Box,
            },
            UiOperation::Properties {
                node: A,
                value: UiProperties::default(),
            },
        ],
    );
    apply(&mut doc, update, &resources).unwrap();
    assert_eq!(doc.hit_test(1.0, 1.0), Some(child));
    let update = batch(
        &doc,
        vec![
            UiOperation::Content {
                node: A,
                value: UiContent::Fixed {
                    size: [8.0, 8.0],
                    first_baseline: None,
                    last_baseline: None,
                },
            },
            UiOperation::RemoveSubtree(child),
        ],
    );
    apply(&mut doc, update, &resources).unwrap();
    assert_eq!(doc.hit_test(1.0, 1.0), Some(A));
}

fn rich_scene(mode: usize, shadows: bool) -> UiSnapshot {
    let mut root = UiNode::new(
        ROOT,
        LayoutStyle::column().size(LayoutDimension::Px(120.0), LayoutDimension::Px(100.0)),
    );
    root.children = vec![A, B];
    let mut a = UiNode::new(
        A,
        LayoutStyle::row()
            .size(LayoutDimension::Px(100.0), LayoutDimension::Px(30.0))
            .overflow(LayoutOverflow::Hidden, LayoutOverflow::Scroll),
    );
    a.children = vec![UiNodeId::new(3, 1)];
    a.properties.opacity = 0.7;
    a.properties.translation = [mode as f32 * 2.0, 0.0];
    a.properties.scroll = [0.0, mode as f32 * 4.0];
    a.properties.clip = Some(Rect {
        x: 0.0,
        y: 0.0,
        width: 80.0,
        height: 30.0,
    });
    let mut leaf = UiNode::new(UiNodeId::new(3, 1), fixed(90.0, 60.0));
    leaf.properties.color = 0xff0000ff;
    leaf.properties.decoration = QuadDecoration {
        radius: 4.0,
        border_width: 1.0,
        border_color: 0xffff00ff,
    };
    if shadows {
        leaf.properties.shadows = vec![BoxShadow {
            offset: [2.0, 2.0],
            blur_radius: 2.0,
            color: 0x00000088,
            ..Default::default()
        }]
        .into();
    }
    let mut b = UiNode::new(B, fixed(16.0, 16.0));
    b.content = UiContent::Image(UiResourceId(42));
    UiSnapshot::new(7, 3, ROOT, vec![root, leaf, a, b])
}
fn rich_reference(mode: usize, image: &ImageResource, shadows: bool) -> FramePacket {
    let mut tree = UiTree::new_layout(
        LayoutStyle::column().size(LayoutDimension::Px(120.0), LayoutDimension::Px(100.0)),
        0,
    )
    .unwrap();
    tree.set_frame_build_policy(FrameBuildPolicy::Full);
    let group = tree
        .append_layout(
            tree.root(),
            LayoutStyle::row()
                .size(LayoutDimension::Px(100.0), LayoutDimension::Px(30.0))
                .overflow(LayoutOverflow::Hidden, LayoutOverflow::Scroll),
            0,
        )
        .unwrap();
    let leaf = tree
        .append_layout(group, fixed(90.0, 60.0), 0xff0000ff)
        .unwrap();
    let icon = tree
        .append_layout(tree.root(), fixed(16.0, 16.0), 0)
        .unwrap();
    let mut tx = Transaction::default();
    tx.translation(group, [mode as f32 * 2.0, 0.0]);
    tx.scroll(group, [0.0, mode as f32 * 4.0]);
    tx.clip(
        group,
        Some(Rect {
            x: 0.0,
            y: 0.0,
            width: 80.0,
            height: 30.0,
        }),
    );
    tx.opacity(group, 0.7);
    tx.decoration(
        leaf,
        QuadDecoration {
            radius: 4.0,
            border_width: 1.0,
            border_color: 0xffff00ff,
        },
    );
    if shadows {
        tx.shadows(
            leaf,
            vec![BoxShadow {
                offset: [2.0, 2.0],
                blur_radius: 2.0,
                color: 0x00000088,
                ..Default::default()
            }],
        );
    }
    tx.image(icon, Some(image.clone()));
    tree.commit(tx, BackendCapabilities::default()).unwrap();
    tree.frame([180.0; 2]).unwrap()
}
fn rich_case(hardware: bool) {
    let mut resources = UiResources::new();
    let image = ImageResource::rgba8([2, 2], vec![0, 255, 255, 255].repeat(4)).unwrap();
    resources.image(UiResourceId(42), image.clone()).unwrap();
    let (mut doc, first) = UiDocument::from_snapshot(
        rich_scene(0, hardware),
        &resources,
        UiLimits::default(),
        BackendCapabilities::default(),
        bounds(),
        [180.0; 2],
    )
    .unwrap();
    #[cfg(feature = "retained-wgpu")]
    let mut gpu = hardware.then(|| crate::retained::wgpu::WgpuFrameRenderer::new(2).unwrap());
    #[cfg(feature = "retained-wgpu")]
    if let Some(gpu) = &gpu {
        eprintln!("UI IR adapter: {:?}", gpu.adapter_info());
    }
    #[cfg(not(feature = "retained-wgpu"))]
    assert!(!hardware);
    let effects = if hardware {
        pacing::SoftwareEffects::OmitDecorativeShadows
    } else {
        pacing::SoftwareEffects::Exact
    };
    let old_bytes = pixels_with_effects(first.frame.resolve(), effects);
    for mode in [0, 1, 3, 0] {
        let source = rich_scene(mode, hardware);
        let props = source
            .nodes
            .iter()
            .find(|n| n.id == A)
            .unwrap()
            .properties
            .clone();
        let update = batch(
            &doc,
            vec![UiOperation::Properties {
                node: A,
                value: props,
            }],
        );
        let output = apply(&mut doc, update, &resources).unwrap();
        let reference = rich_reference(mode, &image, hardware);
        assert_eq!(
            pixels_with_effects(output.frame.resolve(), effects),
            pixels_with_effects(reference.clone(), effects)
        );
        assert_eq!(
            output.hit_test(5.0 + mode as f32 * 2.0, 5.0),
            Some(UiNodeId::new(3, 1))
        );
        #[cfg(feature = "retained-wgpu")]
        if let Some(gpu) = &mut gpu {
            gpu.submit_local(&output.frame, [180, 180], 1.0)
                .unwrap()
                .unwrap();
            assert_eq!(
                gpu.image().unwrap().as_raw(),
                gpu.reference_image(&reference, [180, 180], 1.0)
                    .unwrap()
                    .as_raw()
            );
        }
    }
    drop((doc, resources, image));
    assert_eq!(
        pixels_with_effects(first.frame.resolve(), effects),
        old_bytes
    );
    #[cfg(feature = "retained-wgpu")]
    if let Some(gpu) = &mut gpu {
        gpu.finish().unwrap();
    }
}
#[test]
fn owned_input_mixed_scene_matches_direct_complete_tree_and_old_frame_hits() {
    rich_case(false);
}
#[cfg(feature = "retained-wgpu")]
#[test]
#[ignore = "requires a WGPU adapter; run explicitly"]
fn hardware_ui_ir_matches_direct_full_scene() {
    rich_case(true);
}
