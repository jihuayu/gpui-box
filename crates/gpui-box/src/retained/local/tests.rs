use super::*;

fn fixture() -> (UiTree, Vec<NodeId>) {
    let mut tree = UiTree::new(
        Style {
            size: Size {
                width: length(300.0),
                height: length(200.0),
            },
            ..Default::default()
        },
        0x203040ff,
    )
    .unwrap();
    let mut nodes = vec![tree.root()];
    for i in 0..80 {
        let parent = if i < 45 {
            *nodes.last().unwrap()
        } else {
            nodes[2]
        };
        nodes.push(
            tree.append(
                parent,
                Style {
                    position: taffy::Position::Absolute,
                    inset: taffy::Rect {
                        left: length(0.125),
                        top: length(0.25),
                        right: taffy::LengthPercentageAuto::auto(),
                        bottom: taffy::LengthPercentageAuto::auto(),
                    },
                    size: Size {
                        width: length(40.0),
                        height: length(25.0),
                    },
                    ..Default::default()
                },
                0x4080c080,
            )
            .unwrap(),
        );
    }
    (tree, nodes)
}

fn equivalent(a: &FramePacket, b: &FramePacket) {
    assert_eq!(
        format!("{:?}", a.items.iter().collect::<Vec<_>>()),
        format!("{:?}", b.items.iter().collect::<Vec<_>>())
    );
    assert_eq!(a.properties, b.properties);
}

#[test]
fn local_snapshots_resolve_exactly_after_properties_material_layout_and_structure() {
    let (mut tree, nodes) = fixture();
    let original = tree.local_frame([300.0, 200.0]).unwrap();
    let saved = original.resolve();
    for round in 0..20 {
        let mut tx = Transaction::default();
        for (i, &node) in nodes.iter().enumerate() {
            if i % 7 == round % 7 {
                tx.translation(
                    node,
                    if round % 3 == 0 {
                        [0.0; 2]
                    } else {
                        [round as f32 * 0.125, -0.25]
                    },
                );
                tx.clip(
                    node,
                    (round % 2 == 0).then_some(Rect {
                        x: 1.0,
                        y: 2.0,
                        width: 30.0,
                        height: 20.0,
                    }),
                );
                tx.opacity(node, if round % 3 == 0 { 1.0 } else { 0.8 });
                tx.scroll(node, if round % 2 == 0 { [0.0; 2] } else { [3.0, 4.0] });
            }
        }
        if round == 9 {
            tx.color(nodes[3], 0x80cc44ff);
        }
        if round == 12 {
            let mut style = tree.style(nodes[3]).unwrap().clone();
            style.size.width = length(45.0);
            tx.style(nodes[3], style);
        }
        tree.commit(tx, BackendCapabilities::default()).unwrap();
        let local = tree.local_frame([300.0, 200.0]).unwrap();
        let resolved = local.resolve();
        for item in resolved.items.iter() {
            assert_eq!(
                format!("{:?}", local.resolve_geometry(item.node).unwrap()),
                format!("{item:?}")
            );
        }
        for (x, y) in [(5.0, 5.0), (20.0, 15.0), (290.0, 190.0), (f32::NAN, 0.0)] {
            assert_eq!(local.hit_test(x, y), resolved.hit_test(x, y));
        }
        equivalent(&resolved, &tree.frame([300.0, 200.0]).unwrap());
    }
    tree.remove(nodes[20]).unwrap();
    let after = tree.local_frame([300.0, 200.0]).unwrap();
    assert!(after.resolve_geometry(nodes[20]).is_none());
    equivalent(&after.resolve(), &tree.frame([300.0, 200.0]).unwrap());
    equivalent(&saved, &original.resolve());
    drop(tree);
    assert_eq!(
        format!("{:?}", original.resolve_geometry(nodes[30]).unwrap()),
        format!("{:?}", saved.items[30])
    );
}

#[test]
fn consecutive_local_updates_are_sparse_and_deep_anchors_follow_ancestor_motion() {
    let (mut tree, nodes) = fixture();
    let first = tree.local_frame([300.0, 200.0]).unwrap();
    let template = first.0.template.items.clone();
    let mut old = first.clone();
    for round in 1..8 {
        let mut tx = Transaction::default();
        tx.translation(nodes[1], [round as f32, 0.0]);
        tree.commit(tx, BackendCapabilities::default()).unwrap();
        let frame = tree.local_frame([300.0, 200.0]).unwrap();
        assert_eq!(frame.stats().changed_nodes, 1);
        assert!(!frame.stats().full_publication);
        assert!(Arc::ptr_eq(&frame.0.template.items, &template));
        assert!(frame.0.resolved.get().is_none());
        let updates = frame.spatial_updates(&old).unwrap();
        assert_eq!(updates.len(), 2); // edited depth 1 and depth-32 anchor
        assert_eq!(updates[1].node, nodes[32]);
        old = frame;
    }
    equivalent(&old.resolve(), &tree.frame([300.0, 200.0]).unwrap());
    assert!(!old.follows(&first));
}

#[test]
fn local_mailbox_coalesces_without_resolving_and_snapshots_do_not_chain_payloads() {
    let (mut tree, nodes) = fixture();
    let queue = frame_queue::LocalFrameQueue::new();
    let first = tree.local_frame([300.0, 200.0]).unwrap();
    let weak = Arc::downgrade(&first.0);
    assert!(queue.publish(first.clone()).unwrap());
    for round in 1..4 {
        let mut tx = Transaction::default();
        tx.translation(nodes[1], [round as f32, 0.0]);
        tree.commit(tx, BackendCapabilities::default()).unwrap();
        let frame = tree.local_frame([300.0, 200.0]).unwrap();
        assert!(frame.0.resolved.get().is_none());
        assert!(!queue.publish(frame).unwrap());
    }
    assert_eq!(queue.stats().unwrap().coalesced, 3);
    let latest = queue.take_latest().unwrap().unwrap();
    assert!(!latest.follows(&first));
    assert!(latest.0.resolved.get().is_none());
    assert_eq!(queue.publish(first.clone()), Err(Error::StaleFrame));
    drop(first);
    assert!(weak.upgrade().is_none());
    assert!(!queue.publish(latest.clone()).unwrap());
    assert!(queue.take_latest().unwrap().is_none());
    assert_eq!(
        latest
            .resolve_geometry(nodes[1])
            .unwrap()
            .spatial
            .translation,
        [3.0, 0.0]
    );
}

#[test]
fn resolved_only_and_alternating_publications_do_not_accumulate_local_journals() {
    let (mut tree, nodes) = fixture();
    tree.frame([300.0, 200.0]).unwrap();
    for round in 1..8 {
        let mut tx = Transaction::default();
        tx.translation(nodes[1], [round as f32, 0.0]);
        tree.commit(tx, BackendCapabilities::default()).unwrap();
        assert!(tree.local_dirty.is_empty());
        tree.frame([300.0, 200.0]).unwrap();
    }
    tree.local_frame([300.0, 200.0]).unwrap();
    for round in 8..16 {
        let mut tx = Transaction::default();
        tx.translation(nodes[1], [round as f32, 0.0]);
        tree.commit(tx, BackendCapabilities::default()).unwrap();
        assert_eq!(tree.local_dirty.len(), 1);
        let resolved = tree.frame([300.0, 200.0]).unwrap();
        assert!(tree.local_dirty.is_empty());
        equivalent(
            &tree.local_frame([300.0, 200.0]).unwrap().resolve(),
            &resolved,
        );
    }
}

#[test]
fn placement_pages_preserve_untouched_pages_and_dense_snapshot_history() {
    let (mut tree, nodes) = fixture();
    tree.local_frame([300.0, 200.0]).unwrap();
    let mut tx = Transaction::default();
    for &node in &nodes {
        tx.translation(node, [1.0, 2.0]);
    }
    tree.commit(tx, BackendCapabilities::default()).unwrap();
    let dense = tree.local_frame([300.0, 200.0]).unwrap();
    let saved = dense.resolve();
    let mut tx = Transaction::default();
    tx.translation(nodes[1], [3.0, 4.0]);
    tree.commit(tx, BackendCapabilities::default()).unwrap();
    let sparse = tree.local_frame([300.0, 200.0]).unwrap();
    assert!(!Arc::ptr_eq(
        dense.0.placements.pages[0].as_ref().unwrap(),
        sparse.0.placements.pages[0].as_ref().unwrap(),
    ));
    assert!(Arc::ptr_eq(
        dense.0.placements.pages[1].as_ref().unwrap(),
        sparse.0.placements.pages[1].as_ref().unwrap(),
    ));
    equivalent(&saved, &dense.resolve());
    equivalent(&sparse.resolve(), &tree.frame([300.0, 200.0]).unwrap());
    assert_eq!(dense.placement(1).spatial.translation, [1.0, 2.0]);
    assert_eq!(sparse.placement(1).spatial.translation, [3.0, 4.0]);
}
