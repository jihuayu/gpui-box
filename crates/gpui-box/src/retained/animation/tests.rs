use super::*;
use crate::retained::{frame_queue::LocalFrameQueue, input::*, ui_ir::*};

fn fixed(w: f32, h: f32) -> LayoutStyle {
    LayoutStyle::column()
        .size(LayoutDimension::Px(w), LayoutDimension::Px(h))
        .flex_shrink(0.0)
}
fn fixture() -> (UiTree, NodeId, NodeId) {
    let mut tree = UiTree::new_layout(fixed(160.0, 100.0), 0x17202fff).unwrap();
    let parent = tree
        .append_layout(tree.root(), fixed(80.0, 50.0), 0x225588ff)
        .unwrap();
    let child = tree
        .append_layout(parent, fixed(30.0, 20.0), 0x99aa33ff)
        .unwrap();
    (tree, parent, child)
}
#[test]
fn source_and_ordered_controls_activate_atomically_and_keep_old_input_geometry() {
    let (mut tree, node, _) = fixture();
    let now = Instant::now();
    let old = tree.local_frame([160.0, 100.0]).unwrap();
    let mut runtime = AnimationRuntime::new(old.clone(), AnimationLimits::default()).unwrap();
    let before = runtime.sample(now).unwrap();
    let mut tx = Transaction::default();
    tx.color(node, 0x55cc88ff);
    tree.commit(tx, BackendCapabilities::default()).unwrap();
    let next = tree.local_frame([160.0, 100.0]).unwrap();
    let invalid = AnimationBatch {
        scene: AnimationScene::of(&next),
        sequence: runtime.next_sequence(),
        controls: vec![AnimationControl::Animate(spec(
            1,
            node,
            AnimationValue::Opacity(2.0),
        ))],
    };
    assert_eq!(
        runtime.activate(next.clone(), invalid, now),
        Err(AnimationError::InvalidSpec)
    );
    assert_eq!(runtime.scene(), AnimationScene::of(&old));
    assert_eq!(runtime.next_sequence(), 1);
    assert!(before.frame.same(&runtime.sample(now).unwrap().frame));
    runtime
        .activate(
            next.clone(),
            AnimationBatch {
                scene: AnimationScene::of(&next),
                sequence: 1,
                controls: vec![AnimationControl::Animate(spec(
                    1,
                    node,
                    AnimationValue::Translation([40.0, 0.0]),
                ))],
            },
            now,
        )
        .unwrap();
    assert_eq!(runtime.scene(), AnimationScene::of(&next));
    assert_eq!(
        translation(
            &runtime.sample(now + Duration::from_millis(500)).unwrap(),
            node
        ),
        [20.0, 0.0]
    );
    assert_eq!(
        before.frame.resolve_geometry(node).unwrap().fill_color,
        0x225588ff
    );
}
fn spec(id: u64, node: NodeId, value: AnimationValue) -> AnimationSpec {
    AnimationSpec {
        id,
        node,
        destination: value,
        curve: AnimationCurve::Linear,
        duration: Duration::from_secs(1),
        start: AnimationStart::Now,
        velocity: RetargetVelocity::Reset,
    }
}
fn apply(
    runtime: &mut AnimationRuntime,
    controls: Vec<AnimationControl>,
    now: Instant,
) -> Result<(), AnimationError> {
    runtime.apply(
        AnimationBatch {
            scene: runtime.scene(),
            sequence: runtime.next_sequence(),
            controls,
        },
        now,
    )
}
fn translation(frame: &AnimationFrame, node: NodeId) -> [f32; 2] {
    frame
        .frame
        .resolve_geometry(node)
        .unwrap()
        .spatial
        .translation
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
fn render_samples_match_full_transactions_and_keep_source_and_old_frames_immutable() {
    let (mut tree, parent, child) = fixture();
    let source = tree.local_frame([160.0, 100.0]).unwrap();
    let original = pixels(source.resolve());
    let now = Instant::now();
    let mut runtime = AnimationRuntime::new(source.clone(), AnimationLimits::default()).unwrap();
    apply(
        &mut runtime,
        vec![
            AnimationControl::Animate(spec(1, parent, AnimationValue::Translation([40.0, 8.0]))),
            AnimationControl::Animate(spec(2, parent, AnimationValue::Opacity(0.5))),
        ],
        now,
    )
    .unwrap();
    let first = runtime.sample(now).unwrap();
    assert_ne!(source.stream(), first.frame.stream());
    assert_eq!(source.tree(), first.frame.tree());
    for millis in [0, 125, 500, 875, 1000] {
        let output = runtime.sample(now + Duration::from_millis(millis)).unwrap();
        let progress = millis as f32 / 1000.0;
        let mut tx = Transaction::default();
        tx.translation(parent, [40.0 * progress, 8.0 * progress]);
        tx.opacity(parent, 1.0 - 0.5 * progress);
        tree.commit(tx, BackendCapabilities::default()).unwrap();
        let full = tree.frame([160.0, 100.0]).unwrap();
        assert_eq!(pixels(output.frame.resolve()), pixels(full.clone()));
        for item in full.items.iter() {
            assert_eq!(
                format!("{:?}", output.frame.resolve_geometry(item.node).unwrap()),
                format!("{item:?}")
            );
        }
        assert_eq!(
            output
                .frame
                .hit_test(5.0 + 40.0 * progress, 5.0 + 8.0 * progress),
            Some(child)
        );
        assert_eq!(output.frame.resolve().layout_runs, 0);
        assert_eq!(output.frame.resolve().recorded_chunks, 0);
    }
    assert!(!runtime.is_animating());
    assert_eq!(
        runtime
            .drain_notices()
            .map(|n| n.outcome)
            .collect::<Vec<_>>(),
        vec![AnimationOutcome::Finished; 2]
    );
    let done = runtime.sample(now + Duration::from_secs(2)).unwrap();
    let idle = runtime.sample(now + Duration::from_secs(3)).unwrap();
    assert!(done.frame.same(&idle.frame));
    assert_eq!(translation(&done, parent), [40.0, 8.0]);
    assert_eq!(pixels(source.resolve()), original);
    drop((runtime, tree));
    assert_eq!(pixels(first.source.resolve()), original);
}

#[test]
fn retarget_cancel_release_and_scene_rebase_preserve_current_values_and_generations() {
    let (mut tree, parent, _) = fixture();
    let now = Instant::now();
    let source = tree.local_frame([160.0, 100.0]).unwrap();
    let mut r = AnimationRuntime::new(source, AnimationLimits::default()).unwrap();
    apply(
        &mut r,
        vec![AnimationControl::Animate(spec(
            1,
            parent,
            AnimationValue::Translation([40.0, 0.0]),
        ))],
        now,
    )
    .unwrap();
    let half = now + Duration::from_millis(500);
    assert_eq!(translation(&r.sample(half).unwrap(), parent), [20.0, 0.0]);
    apply(
        &mut r,
        vec![AnimationControl::Animate(spec(
            2,
            parent,
            AnimationValue::Translation([0.0, 0.0]),
        ))],
        half,
    )
    .unwrap();
    assert_eq!(translation(&r.sample(half).unwrap(), parent), [20.0, 0.0]);
    assert_eq!(
        r.drain_notices().next().unwrap().outcome,
        AnimationOutcome::Replaced
    );
    let quarter = now + Duration::from_millis(750);
    assert_eq!(
        translation(&r.sample(quarter).unwrap(), parent),
        [15.0, 0.0]
    );
    apply(
        &mut r,
        vec![AnimationControl::Cancel {
            id: 2,
            behavior: CancelBehavior::Hold,
        }],
        quarter,
    )
    .unwrap();
    assert_eq!(
        r.drain_notices().next().unwrap().outcome,
        AnimationOutcome::Cancelled
    );
    let mut tx = Transaction::default();
    tx.translation(parent, [3.0, 0.0]);
    tx.color(parent, 0xdd2244ff);
    tree.commit(tx, BackendCapabilities::default()).unwrap();
    r.set_scene(tree.local_frame([160.0, 100.0]).unwrap(), quarter)
        .unwrap();
    assert_eq!(
        translation(&r.sample(quarter).unwrap(), parent),
        [15.0, 0.0]
    );
    apply(
        &mut r,
        vec![AnimationControl::Release {
            node: parent,
            property: AnimatedProperty::Translation,
        }],
        quarter,
    )
    .unwrap();
    assert_eq!(translation(&r.sample(quarter).unwrap(), parent), [3.0, 0.0]);
    apply(
        &mut r,
        vec![AnimationControl::Animate(spec(
            3,
            parent,
            AnimationValue::Translation([20.0, 0.0]),
        ))],
        quarter,
    )
    .unwrap();
    tree.remove(parent).unwrap();
    let replacement = tree
        .append_layout(tree.root(), fixed(20.0, 20.0), 0)
        .unwrap();
    assert_ne!(parent, replacement);
    r.set_scene(tree.local_frame([160.0, 100.0]).unwrap(), quarter)
        .unwrap();
    assert_eq!(r.track_count(), 0);
    let notice = r.drain_notices().next().unwrap();
    assert_eq!(
        (notice.id, notice.outcome),
        (3, AnimationOutcome::TargetRemoved)
    );
    assert_eq!(
        translation(&r.sample(quarter).unwrap(), replacement),
        [0.0; 2]
    );
    assert_eq!(
        apply(
            &mut r,
            vec![AnimationControl::Animate(spec(
                4,
                parent,
                AnimationValue::Opacity(0.5)
            ))],
            quarter
        ),
        Err(AnimationError::InvalidTarget)
    );
}

#[test]
fn controls_and_terminal_notice_reservations_are_atomic_and_bounded() {
    let (mut tree, parent, child) = fixture();
    let now = Instant::now();
    let mut r = AnimationRuntime::new(
        tree.local_frame([160.0, 100.0]).unwrap(),
        AnimationLimits {
            max_tracks: 2,
            max_notices: 1,
            max_controls_per_batch: 4,
        },
    )
    .unwrap();
    apply(
        &mut r,
        vec![AnimationControl::Animate(spec(
            1,
            parent,
            AnimationValue::Translation([40.0, 0.0]),
        ))],
        now,
    )
    .unwrap();
    let seq = r.next_sequence();
    assert_eq!(
        apply(
            &mut r,
            vec![AnimationControl::Animate(spec(
                2,
                child,
                AnimationValue::Opacity(0.5)
            ))],
            now
        ),
        Err(AnimationError::Capacity)
    );
    assert_eq!((r.next_sequence(), r.track_count()), (seq, 1));
    assert_eq!(
        apply(
            &mut r,
            vec![AnimationControl::Animate(spec(
                2,
                parent,
                AnimationValue::Opacity(f32::NAN)
            ))],
            now
        ),
        Err(AnimationError::InvalidSpec)
    );
    let future = now + Duration::from_secs(2);
    r.sample(future).unwrap();
    assert_eq!(
        apply(
            &mut r,
            vec![AnimationControl::Animate(spec(
                2,
                child,
                AnimationValue::Opacity(0.5)
            ))],
            future
        ),
        Err(AnimationError::Capacity)
    );
    assert_eq!(r.drain_notices().count(), 1);
    apply(
        &mut r,
        vec![AnimationControl::Animate(spec(
            2,
            child,
            AnimationValue::Opacity(0.5),
        ))],
        future,
    )
    .unwrap();
    assert_eq!(
        apply(&mut r, vec![], now),
        Err(AnimationError::ClockReversed)
    );
    let mut wrong = AnimationBatch {
        scene: r.scene(),
        sequence: 1,
        controls: vec![],
    };
    assert_eq!(
        r.apply(wrong.clone(), future),
        Err(AnimationError::StaleSequence)
    );
    wrong.sequence = r.next_sequence();
    wrong.scene.revision += 1;
    assert_eq!(r.apply(wrong, future), Err(AnimationError::WrongScene));
    assert_eq!(
        apply(
            &mut r,
            vec![AnimationControl::Animate(spec(
                1,
                child,
                AnimationValue::Opacity(1.0)
            ))],
            future
        ),
        Err(AnimationError::InvalidSpec)
    );
}

#[test]
fn curves_are_finite_delayed_and_absolute_time_and_spring_retarget_preserves_velocity() {
    let now = Instant::now();
    for curve in [
        AnimationCurve::Linear,
        AnimationCurve::CubicBezier {
            x1: 0.25,
            y1: 0.1,
            x2: 0.25,
            y2: 1.0,
        },
        AnimationCurve::Keyframes(Arc::from([[0.0, 0.0], [0.25, 0.8], [1.0, 1.0]])),
        AnimationCurve::Spring(Spring {
            mass: 1.0,
            stiffness: 100.0,
            damping: 5.0,
        }),
        AnimationCurve::Spring(Spring {
            mass: 1.0,
            stiffness: 100.0,
            damping: 20.0,
        }),
        AnimationCurve::Spring(Spring {
            mass: 1.0,
            stiffness: 100.0,
            damping: 50.0,
        }),
    ] {
        curve.validate().unwrap();
        let (start, _) = curve
            .sample(
                [0.0; 2],
                [10.0, 4.0],
                [0.0; 2],
                Duration::ZERO,
                Duration::from_secs(1),
            )
            .unwrap();
        assert!(start[0].abs() < 1e-8);
        for millis in [1, 20, 500, 999] {
            let (v, s) = curve
                .sample(
                    [0.0; 2],
                    [10.0, 4.0],
                    [0.0; 2],
                    Duration::from_millis(millis),
                    Duration::from_secs(1),
                )
                .unwrap();
            assert!(v.iter().chain(&s).all(|v| v.is_finite()));
        }
    }
    assert!(
        AnimationCurve::Keyframes(Arc::from([[0.0, 0.0], [0.0, 1.0], [1.0, 1.0]]))
            .validate()
            .is_err()
    );
    let (mut tree, parent, _) = fixture();
    let mut r = AnimationRuntime::new(
        tree.local_frame([160.0, 100.0]).unwrap(),
        AnimationLimits::default(),
    )
    .unwrap();
    let mut first = spec(1, parent, AnimationValue::Translation([40.0, 0.0]));
    first.start = AnimationStart::At(now + Duration::from_millis(100));
    apply(&mut r, vec![AnimationControl::Animate(first)], now).unwrap();
    assert_eq!(translation(&r.sample(now).unwrap(), parent), [0.0; 2]);
    let at = now + Duration::from_millis(600);
    assert_eq!(translation(&r.sample(at).unwrap(), parent), [20.0, 0.0]);
    let mut next = spec(2, parent, AnimationValue::Translation([80.0, 0.0]));
    next.curve = AnimationCurve::Spring(Spring {
        mass: 1.0,
        stiffness: 100.0,
        damping: 20.0,
    });
    next.velocity = RetargetVelocity::Preserve;
    apply(&mut r, vec![AnimationControl::Animate(next)], at).unwrap();
    assert_eq!(translation(&r.sample(at).unwrap(), parent), [20.0, 0.0]);
    let shortly = translation(&r.sample(at + Duration::from_micros(100)).unwrap(), parent)[0];
    assert!((shortly - 20.004).abs() < 0.0002, "{shortly}");
    assert_eq!(
        translation(&r.sample(now + Duration::from_secs(5)).unwrap(), parent),
        [80.0, 0.0]
    );
    let end = now + Duration::from_secs(5);
    apply(
        &mut r,
        vec![AnimationControl::Animate(spec(
            3,
            parent,
            AnimationValue::Translation([10.0, 0.0]),
        ))],
        end,
    )
    .unwrap();
    apply(&mut r, vec![AnimationControl::Finish { id: 3 }], end).unwrap();
    assert_eq!(translation(&r.sample(end).unwrap(), parent), [10.0, 0.0]);
    assert!(!r.is_animating());
}

#[cfg(feature = "retained-wgpu")]
#[test]
#[ignore = "requires explicit WGPU adapter"]
fn hardware_render_timeline_translation_reuses_spatial_instances_and_recovers_skipped_sample() {
    let (mut tree, node, _) = fixture();
    let source = tree.local_frame([160.0, 100.0]).unwrap();
    let now = Instant::now();
    let mut r = AnimationRuntime::new(source, AnimationLimits::default()).unwrap();
    apply(
        &mut r,
        vec![AnimationControl::Animate(spec(
            1,
            node,
            AnimationValue::Translation([40.0, 8.0]),
        ))],
        now,
    )
    .unwrap();
    let mut gpu = wgpu::WgpuFrameRenderer::new(2).unwrap();
    eprintln!("timeline spatial adapter: {:?}", gpu.adapter_info());
    for (i, millis) in [0, 250, 500, 750, 1000].into_iter().enumerate() {
        let output = r.sample(now + Duration::from_millis(millis)).unwrap();
        if i == 2 {
            continue;
        }
        let result = gpu
            .submit_local(output.frame(), [160, 100], 1.0)
            .unwrap()
            .unwrap();
        assert_eq!(result.full_resync, i == 0 || i == 3);
        if i == 1 || i == 4 {
            assert_eq!(result.routing.prepared_quad_items, 0);
            assert!(result.routing.prepared_spatial_items > 0);
        }
        let mut tx = Transaction::default();
        tx.translation(node, [millis as f32 * 0.04, millis as f32 * 0.008]);
        tree.commit(tx, BackendCapabilities::default()).unwrap();
        let full = tree.frame([160.0, 100.0]).unwrap();
        assert!(
            gpu.image().unwrap().as_raw()
                == gpu
                    .reference_image(&full, [160, 100], 1.0)
                    .unwrap()
                    .as_raw()
        );
    }
    let mut tx = Transaction::default();
    tx.color(node, 0x55cc88ff);
    tree.commit(tx, BackendCapabilities::default()).unwrap();
    let current = tree.local_frame([160.0, 100.0]).unwrap();
    r.set_scene(current, now + Duration::from_secs(1)).unwrap();
    let rebased = r.sample(now + Duration::from_secs(1)).unwrap();
    gpu.submit_local(rebased.frame(), [160, 100], 1.0).unwrap();
    gpu.finish().unwrap();
    assert!(
        gpu.image().unwrap().as_raw()
            == gpu
                .reference_image(&tree.frame([160.0, 100.0]).unwrap(), [160, 100], 1.0)
                .unwrap()
                .as_raw()
    );
}

#[test]
fn projected_input_follows_installed_sample_and_rejects_stale_or_unrelated_frames() {
    let root = UiNodeId::new(0, 1);
    let leaf = UiNodeId::new(1, 1);
    let mut a = UiNode::new(root, fixed(160.0, 100.0));
    a.children = vec![leaf];
    a.interaction.pointer_target = false;
    let mut b = UiNode::new(leaf, fixed(30.0, 20.0));
    b.interaction.activate_handler = Some(UiHandlerId(1));
    let resources = UiResources::new();
    let (doc, pubn) = UiDocument::from_snapshot(
        UiSnapshot::new(1, 1, root, vec![a, b]),
        &resources,
        UiLimits::default(),
        BackendCapabilities::default(),
        LayoutConstraints::viewport(160.0, 100.0),
        [160.0, 100.0],
    )
    .unwrap();
    let source_scene = doc.input_scene();
    let node = pubn.frame.resolve().items[1].node;
    let now = Instant::now();
    let mut r = AnimationRuntime::new(pubn.frame, AnimationLimits::default()).unwrap();
    apply(
        &mut r,
        vec![AnimationControl::Animate(spec(
            1,
            node,
            AnimationValue::Translation([80.0, 0.0]),
        ))],
        now,
    )
    .unwrap();
    let first = r.sample(now).unwrap().input_scene(&source_scene).unwrap();
    let next = r
        .sample(now + Duration::from_millis(500))
        .unwrap()
        .input_scene(&source_scene)
        .unwrap();
    assert_eq!(next.hit_test([45.0, 5.0]), Some(leaf));
    assert_eq!(next.hit_test([5.0, 5.0]), None);
    let mut router = UiInputRouter::new();
    router
        .adopt(first.clone(), UiInputEvidence::ImageInstalled)
        .unwrap();
    router
        .adopt(next.clone(), UiInputEvidence::ImageInstalled)
        .unwrap();
    assert!(
        router
            .adopt(first, UiInputEvidence::ImageInstalled)
            .is_err()
    );
    router.route(UiInput::PrimaryDown([45.0, 5.0])).unwrap();
    let effects = router.route(UiInput::PrimaryUp([45.0, 5.0])).unwrap();
    let event = effects
        .dispatches
        .iter()
        .find(|d| d.kind == UiInputEventKind::Activate)
        .unwrap();
    assert_eq!(event.scene, next.id());
    assert!(doc.accepts_dispatch(event));
    // The adopted sample's wheel/reveal uses the same authored policy/base;
    // presentation identity must not be mistaken for a stale UI revision.
    let wheel = router
        .route(UiInput::Wheel {
            position: [45.0, 5.0],
            delta: [0.0, 8.0],
        })
        .unwrap();
    assert!(input::scroll::plan_wheels(&source_scene, &wheel.wheel_scrolls).is_ok());
    let (mut other, _, _) = fixture();
    let mut wrong = source_scene.clone();
    wrong.frame = other.local_frame([160.0, 100.0]).unwrap();
    assert!(
        r.sample(now + Duration::from_secs(1))
            .unwrap()
            .input_scene(&wrong)
            .is_err()
    );
}

#[test]
fn a_render_worker_samples_while_the_ui_owner_is_parked_and_mailbox_keeps_latest() {
    let now = Instant::now();
    let (send, recv) = std::sync::mpsc::sync_channel(1);
    let (release, wait) = std::sync::mpsc::sync_channel(0);
    let owner = std::thread::spawn(move || {
        let (mut tree, node, _) = fixture();
        let frame = tree.local_frame([160.0, 100.0]).unwrap();
        send.send((frame, node)).unwrap();
        wait.recv_timeout(Duration::from_secs(5)).unwrap();
        drop(tree);
    });
    let (source, node) = recv.recv_timeout(Duration::from_secs(5)).unwrap();
    let worker = std::thread::spawn(move || {
        let mut runtime = AnimationRuntime::new(source, AnimationLimits::default()).unwrap();
        apply(
            &mut runtime,
            vec![AnimationControl::Animate(spec(
                1,
                node,
                AnimationValue::Translation([60.0, 0.0]),
            ))],
            now,
        )
        .unwrap();
        let queue = LocalFrameQueue::new();
        for millis in [0, 100, 500, 1000] {
            queue
                .publish(
                    runtime
                        .sample(now + Duration::from_millis(millis))
                        .unwrap()
                        .frame()
                        .clone(),
                )
                .unwrap();
        }
        let last = queue.take_latest().unwrap().unwrap();
        assert_eq!(
            last.resolve_geometry(node).unwrap().spatial.translation,
            [60.0, 0.0]
        );
        assert_eq!(queue.stats().unwrap().coalesced, 3);
        assert!(!pixels(last.resolve()).is_empty());
        assert_eq!(runtime.drain_notices().count(), 1);
    });
    let result = worker.join();
    let _ = release.send(());
    owner.join().unwrap();
    result.unwrap();
}

#[test]
fn unsupported_snap_and_composed_overflow_reject_without_publishing_a_partial_sample() {
    let now = Instant::now();
    let (mut tree, node, _) = fixture();
    let mut r = AnimationRuntime::new(
        tree.local_frame([160.0, 100.0]).unwrap(),
        AnimationLimits::default(),
    )
    .unwrap();
    let before = r.sample(now).unwrap();
    let mut tx = Transaction::default();
    tx.translation(node, [f32::MAX, 0.0]);
    tree.commit(tx, BackendCapabilities::default()).unwrap();
    // Two finite local translations can compose into invalid world geometry.
    let source = tree.local_frame([160.0, 100.0]).unwrap();
    r.set_scene(source, now).unwrap();
    let child = tree.append_layout(node, fixed(10.0, 10.0), 0).unwrap();
    r.set_scene(tree.local_frame([160.0, 100.0]).unwrap(), now)
        .unwrap();
    let mut animation = spec(1, child, AnimationValue::Translation([f32::MAX, 0.0]));
    animation.start = AnimationStart::At(now - Duration::from_secs(2));
    assert_eq!(
        apply(&mut r, vec![AnimationControl::Animate(animation)], now),
        Err(AnimationError::InvalidGeometry)
    );
    assert_eq!(r.next_sequence(), 1);
    assert_eq!(r.track_count(), 0);
    assert!(
        before
            .frame
            .resolve_geometry(node)
            .unwrap()
            .bounds
            .x
            .is_finite()
    );
    let (mut tree, _, _) = fixture();
    tree.set_pixel_snap(PixelSnap::DevicePixels { scale: 1.25 })
        .unwrap();
    assert!(matches!(
        AnimationRuntime::new(
            tree.local_frame([160.0, 100.0]).unwrap(),
            AnimationLimits::default()
        ),
        Err(AnimationError::UnsupportedPixelSnap)
    ));
}
