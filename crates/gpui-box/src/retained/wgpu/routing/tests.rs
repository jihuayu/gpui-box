use super::*;

fn tree() -> UiTree {
    UiTree::new(
        Style {
            size: taffy::Size {
                width: length(100.0),
                height: length(100.0),
            },
            ..Default::default()
        },
        0x4488ccff,
    )
    .unwrap()
}
fn snapshot(frame: &FramePacket, validation: &mut validation::ValidationCache) -> Snapshot {
    validation.check(frame, [100, 100], 1.0, true).unwrap();
    Snapshot::next(frame, None, validation, true, true).unwrap()
}

#[test]
fn inherited_motion_only_prepares_bounded_anchors_and_shares_depths() {
    for policy in [FrameBuildPolicy::Selective, FrameBuildPolicy::SelectiveCopy] {
        let mut tree = tree();
        tree.set_frame_build_policy(policy);
        let mut parent = tree.root();
        for _ in 0..65 {
            parent = tree.append(parent, Style::default(), 0x44cc88ff).unwrap();
        }
        let base = tree.frame([100.0; 2]).unwrap();
        let mut validation = validation::ValidationCache::default();
        let old = snapshot(&base, &mut validation);
        assert_eq!(old.depths.iter().filter(|d| **d == 0).count(), 3);
        let mut tx = Transaction::default();
        tx.translation(tree.root(), [1.25, 2.5]);
        tree.commit(tx, BackendCapabilities::default()).unwrap();
        let next = tree.frame([100.0; 2]).unwrap();
        validation.check(&next, [100, 100], 1.0, true).unwrap();
        assert!(old.follows(&next));
        let plan = old.plan(&next, &validation, true).unwrap();
        assert_eq!(plan.entries.len(), 3);
        assert!(!plan.parent_changed);
        assert!(
            plan.entries
                .iter()
                .all(|e| e.spatial && !e.quad && !e.shadow)
        );
        let current = Snapshot::next(&next, Some(&old), &validation, false, true).unwrap();
        assert!(Arc::ptr_eq(&old.depths, &current.depths));
        let world = old.plan(&next, &validation, false).unwrap();
        assert_eq!(world.entries.len(), 66);
        assert!(world.entries.iter().all(|e| e.quad && !e.spatial));
    }
}

#[test]
fn local_material_size_radius_and_shadow_phase_route_independently() {
    let mut tree = tree();
    let mut tx = Transaction::default();
    tx.decoration(
        tree.root(),
        QuadDecoration {
            radius: 4.0,
            ..Default::default()
        },
    );
    tx.shadows(
        tree.root(),
        vec![BoxShadow {
            color: 0x00000088,
            ..Default::default()
        }],
    );
    tree.commit(tx, BackendCapabilities::default()).unwrap();
    let frame = tree.frame([100.0; 2]).unwrap();
    let old = &frame.items[0];
    let mut item = old.clone();
    item.fill_color = 0xff0000ff;
    let e = classify(old, &item, true, true);
    assert!(e.quad && !e.shadow && !e.spatial);
    item = old.clone();
    item.bounds.width += 1.0;
    let e = classify(old, &item, true, true);
    assert!(e.quad && e.shadow);
    item = old.clone();
    item.commands = Arc::from([DisplayCommand::DecoratedFill {
        color: 0,
        decoration: QuadDecoration {
            radius: 8.0,
            ..Default::default()
        },
    }]);
    let e = classify(old, &item, true, true);
    assert!(e.quad && e.shadow);
    item = old.clone();
    Arc::make_mut(&mut item.shadows)[0].inset = true;
    let e = classify(old, &item, true, true);
    assert!(!e.quad && e.shadow && !e.spatial);
    item = old.clone();
    item.clip = Some(Rect {
        x: 0.0,
        y: 0.0,
        width: 10.0,
        height: 10.0,
    });
    let e = classify(old, &item, true, true);
    assert!(!e.quad && !e.shadow && e.spatial);
    let e = classify(old, &item, false, true);
    assert!(e.quad && e.shadow && !e.spatial);
}

#[test]
fn only_exact_normal_transition_provenance_follows_submitted_snapshot() {
    for policy in [
        FrameBuildPolicy::Selective,
        FrameBuildPolicy::SelectiveCopy,
        FrameBuildPolicy::Full,
    ] {
        let mut tree = tree();
        tree.set_frame_build_policy(policy);
        let base = tree.frame([100.0; 2]).unwrap();
        let mut validation = validation::ValidationCache::default();
        let old = snapshot(&base, &mut validation);
        let mut tx = Transaction::default();
        tx.color(tree.root(), 0xff8844ff);
        tree.commit(tx, BackendCapabilities::default()).unwrap();
        let next = tree.frame([100.0; 2]).unwrap();
        assert!(old.follows(&next));
        let mut mutated = vec![];
        let mut f = next.clone();
        f.tree += 1;
        mutated.push(f);
        let mut f = next.clone();
        f.revision += 1;
        mutated.push(f);
        let mut f = next.clone();
        f.base_revision = None;
        mutated.push(f);
        let mut f = next.clone();
        Arc::make_mut(&mut f.items);
        mutated.push(f);
        let mut f = next.clone();
        Arc::make_mut(&mut f.properties);
        mutated.push(f);
        let mut f = next.clone();
        f.damage.full = !f.damage.full;
        mutated.push(f);
        let mut f = next.clone();
        f.removed = Arc::from([]);
        mutated.push(f);
        let mut f = next.clone();
        f.order = Some(Arc::from([]));
        mutated.push(f);
        let mut f = next.clone();
        Arc::make_mut(&mut f.updates)[0].fill_color = next.items[0].fill_color;
        mutated.push(f);
        let mut f = next.clone();
        f.updates = Arc::new(Vec::new().into());
        mutated.push(f);
        assert!(mutated.iter().all(|f| !old.follows(f)));
        // Validation of rejected/newer inputs must not move the submitted base.
        validation.check(&next, [100, 100], 1.0, true).unwrap();
        assert!(old.follows(&next));
        let current = Snapshot::next(&next, Some(&old), &validation, true, true).unwrap();
        assert!(!current.follows(&next));
        assert!(current.same_scene(&next));
        assert!(!current.same_scene(&mutated[3]));
    }
}

#[test]
fn provenance_keeps_no_previous_paint_root_alive() {
    let mut tree = tree();
    let base = tree.frame([100.0; 2]).unwrap();
    let weak = Arc::downgrade(&base.items);
    let mut tx = Transaction::default();
    tx.color(tree.root(), 0xff8844ff);
    tree.commit(tx, BackendCapabilities::default()).unwrap();
    let next = tree.frame([100.0; 2]).unwrap();
    drop(base);
    assert!(weak.upgrade().is_none());
    assert_eq!(next.items[0].fill_color, 0xff8844ff);
}
