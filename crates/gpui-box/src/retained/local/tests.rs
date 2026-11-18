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
fn layout_revalidation_preserves_only_certified_same_revision_output() {
    let (mut tree, nodes) = fixture();
    let first = tree.local_frame([300.0, 200.0]).unwrap();
    tree.layout_dirty = true;
    tree.dirty = true;
    let checked = tree.local_frame([300.0, 200.0]).unwrap();
    assert!(Arc::ptr_eq(&first.0, &checked.0));
    assert_eq!(tree.frame.as_ref().unwrap().layout_runs, 1);
    assert!(!tree.layout_dirty);

    // A new source revision still needs its own packet even when changed
    // max-width leaves the actual fixed-width geometry identical.
    let mut style = tree.style(nodes[1]).unwrap().clone();
    style.max_size.width = length(100.0);
    let mut tx = Transaction::default();
    tx.style(nodes[1], style);
    tree.commit(tx, BackendCapabilities::default()).unwrap();
    let next = tree.local_frame([300.0, 200.0]).unwrap();
    assert_ne!(first.revision(), next.revision());
    assert!(!Arc::ptr_eq(&first.0, &next.0));
    equivalent(&next.resolve(), &first.resolve());

    // Revision equality alone cannot certify even internal recomputation. A
    // changed kernel output with the same source revision must be published.
    let mut style = tree.style(nodes[1]).unwrap().clone();
    style.size.width = length(55.0);
    tree.layout
        .set_style(tree.node(nodes[1]).unwrap().layout, style)
        .unwrap();
    tree.layout_dirty = true;
    tree.dirty = true;
    let changed = tree.local_frame([300.0, 200.0]).unwrap();
    assert_eq!(next.revision(), changed.revision());
    assert!(!Arc::ptr_eq(&next.0, &changed.0));
    assert_eq!(
        changed.resolve_geometry(nodes[1]).unwrap().bounds.width,
        55.0
    );
    assert_eq!(next.resolve_geometry(nodes[1]).unwrap().bounds.width, 40.0);
}

#[test]
fn layout_revalidation_cannot_restore_a_stale_resolved_baseline_over_local_changes() {
    let (mut tree, nodes) = fixture();
    let original = tree.local_frame([300.0, 200.0]).unwrap();
    let mut tx = Transaction::default();
    tx.translation(nodes[1], [7.0, 3.0]);
    tx.opacity(nodes[1], 0.5);
    tx.clip(
        nodes[1],
        Some(Rect {
            x: 0.0,
            y: 0.0,
            width: 35.0,
            height: 20.0,
        }),
    );
    tx.color(nodes[2], 0x33aaffff);
    tree.commit(tx, BackendCapabilities::default()).unwrap();
    let sparse = tree.local_frame([300.0, 200.0]).unwrap();
    assert!(sparse.follows(&original));
    assert!(sparse.0.resolved.get().is_none());
    assert!(Arc::ptr_eq(
        &original.0.template.items,
        &tree.frame.as_ref().unwrap().items
    ));
    let expected = sparse.resolve();
    tree.layout_dirty = true;
    tree.dirty = true;
    let checked = tree.local_frame([300.0, 200.0]).unwrap();
    assert_eq!(sparse.revision(), checked.revision());
    assert!(!Arc::ptr_eq(&sparse.0, &checked.0));
    equivalent(&checked.resolve(), &expected);
    assert_eq!(
        checked
            .resolve_geometry(nodes[1])
            .unwrap()
            .spatial
            .translation,
        [7.0, 3.0]
    );
    let idle = tree.local_frame([300.0, 200.0]).unwrap();
    assert!(Arc::ptr_eq(&checked.0, &idle.0));
    equivalent(&original.resolve(), &original.0.template);
}

#[cfg(feature = "retained-wgpu")]
#[test]
#[ignore = "requires WGPU adapter; measures duplicate output consumer cost"]
fn hardware_identical_local_output_consumer_control() {
    use std::time::Instant;
    let (mut tree, _) = fixture();
    let packet = tree.frame([300.0, 200.0]).unwrap();
    let shared = LocalFrame::from_frame(packet.clone());
    let mut gpu = wgpu::WgpuFrameRenderer::new(2).unwrap();
    eprintln!("identical local output adapter: {:?}", gpu.adapter_info());
    let output = std::env::var_os("GPUI_LOCAL_REVALIDATION_CSV").map(std::path::PathBuf::from);
    let mut csv = String::from("round,variant,sample,nanoseconds,submitted\n");
    for round in 0..4 {
        for variant in if round % 2 == 0 {
            [false, true]
        } else {
            [true, false]
        } {
            // Baseline reconstructs an identical full local snapshot per call,
            // exactly the prior fallback. Candidate retains the certified
            // snapshot. This measures only reconstruction/submit/completion;
            // common owner previews/layout/validation are deliberately absent.
            gpu.submit_local(&shared, [300, 200], 1.0).unwrap();
            gpu.finish().unwrap();
            for sample in 0..116 {
                let started = Instant::now();
                let frame = if variant {
                    shared.clone()
                } else {
                    LocalFrame::from_frame(packet.clone())
                };
                let submission = gpu.submit_local(&frame, [300, 200], 1.0).unwrap();
                gpu.finish().unwrap();
                assert_eq!(submission.is_some(), !variant);
                drop(frame);
                let elapsed = started.elapsed().as_nanos();
                if sample >= 16 {
                    csv.push_str(&format!(
                        "{},{},{},{},{}\n",
                        round + 1,
                        if variant { "retained" } else { "rebuilt" },
                        sample - 16,
                        elapsed,
                        u8::from(submission.is_some())
                    ));
                }
            }
            assert_eq!(
                gpu.image().unwrap().as_raw(),
                gpu.reference_image(&packet, [300, 200], 1.0)
                    .unwrap()
                    .as_raw()
            );
        }
    }
    if let Some(output) = output {
        std::fs::write(output, csv).unwrap();
    } else {
        eprintln!("set GPUI_LOCAL_REVALIDATION_CSV to retain timing samples");
    }
}

#[cfg(feature = "retained-wgpu")]
#[test]
fn geometry_ranges_reopen_actual_ancestors_after_leaving_start_subtree() {
    let (mut tree, nodes) = fixture();
    tree.local_frame([300.0, 200.0]).unwrap();
    let mut tx = Transaction::default();
    tx.translation(nodes[2], [12.375, -3.125]);
    tx.translation(nodes[30], [2.125, 0.375]);
    tx.clip(
        nodes[30],
        Some(Rect {
            x: 1.0,
            y: 2.0,
            width: 28.0,
            height: 18.0,
        }),
    );
    tx.opacity(nodes[30], 0.5);
    tree.commit(tx, BackendCapabilities::default()).unwrap();
    let local = tree.local_frame([300.0, 200.0]).unwrap();
    let fresh = tree.frame([300.0, 200.0]).unwrap();
    // Start deep in one branch, then cross to a shallower sibling. Every
    // possible range start also exercises root and single-leaf boundaries.
    for start in 0..local.len() {
        local.visit_geometry(start..local.len(), |i, item| {
            assert_eq!(
                format!("{item:?}"),
                format!("{:?}", fresh.items[i]),
                "start={start} index={i}"
            );
        });
    }
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
fn full_publication_reuses_owning_geometry_but_sparse_successor_resolves_new_placement() {
    let (mut tree, nodes) = fixture();
    let original = tree.local_frame([300.0, 200.0]).unwrap();
    let full = original.resolved_frame();
    assert!(Arc::ptr_eq(&full.items, &original.0.template.items));
    assert!(Arc::ptr_eq(
        &full.properties,
        &original.0.template.properties
    ));
    let mut tx = Transaction::default();
    tx.translation(nodes[1], [4.0, 2.0]);
    tree.commit(tx, BackendCapabilities::default()).unwrap();
    let moved = tree.local_frame([300.0, 200.0]).unwrap();
    assert!(!moved.stats().full_publication);
    assert!(moved.0.resolved.get().is_none());
    equivalent(&moved.resolve(), &tree.frame([300.0, 200.0]).unwrap());
    let mut tx = Transaction::default();
    tx.color(nodes[8], 0xaabbccff);
    tree.commit(tx, BackendCapabilities::default()).unwrap();
    let material = tree.local_frame([300.0, 200.0]).unwrap();
    // The explicit resolved subscriber above advances the owner baseline;
    // the next local subscriber conservatively starts a complete snapshot.
    assert!(material.stats().full_publication);
    assert!(Arc::ptr_eq(
        &material.resolved_frame().items,
        &material.0.template.items
    ));
    equivalent(&material.resolve(), &tree.frame([300.0, 200.0]).unwrap());
    equivalent(&original.resolve(), full);
}

#[test]
fn independent_material_and_placement_lifetimes_preserve_resolved_subscriber_and_snapshots() {
    let (mut tree, nodes) = fixture();
    let first = tree.local_frame([300.0, 200.0]).unwrap();
    let saved = first.resolve();
    let mut latest = first.clone();
    for round in 1..12 {
        let mut tx = Transaction::default();
        tx.translation(nodes[1], [round as f32 * 0.375, -0.25]);
        for (i, &node) in nodes.iter().enumerate() {
            if i % 3 == round % 3 {
                tx.color(node, 0x203040ff + (round as u32) * 0x01000000);
                tx.decoration(
                    node,
                    QuadDecoration {
                        radius: round as f32,
                        ..Default::default()
                    },
                );
            }
        }
        tree.commit(tx, BackendCapabilities::default()).unwrap();
        let frame = tree.local_frame([300.0, 200.0]).unwrap();
        assert!(!frame.stats().full_publication);
        assert!(frame.follows(&latest));
        assert!(Arc::ptr_eq(&frame.0.topology, &first.0.topology));
        assert!(frame.stats().material_nodes > 0);
        assert!(tree.local_material_dirty.is_empty());
        assert!(!tree.paint_dirty.is_empty());
        for item in frame.resolve().items.iter() {
            assert_eq!(
                format!("{:?}", frame.resolve_geometry(item.node).unwrap()),
                format!("{item:?}")
            );
        }
        latest = frame;
    }
    equivalent(&latest.resolve(), &tree.frame([300.0, 200.0]).unwrap());
    equivalent(&saved, &first.resolve());
    let after_resolved = tree.local_frame([300.0, 200.0]).unwrap();
    assert!(after_resolved.stats().full_publication);
    assert!(tree.local_material_dirty.is_empty());
    // Layout and visibility are not authorized by material provenance.
    let mut tx = Transaction::default();
    let mut style = tree.style(nodes[5]).unwrap().clone();
    style.display = taffy::Display::None;
    tx.style(nodes[5], style);
    tree.commit(tx, BackendCapabilities::default()).unwrap();
    let hidden = tree.local_frame([300.0, 200.0]).unwrap();
    assert!(hidden.stats().full_publication);
    assert!(hidden.resolve_geometry(nodes[5]).is_none());
}

#[test]
fn material_journal_follows_reordered_paint_indices_and_keeps_old_snapshots() {
    let mut tree = UiTree::new(Style::default(), 0x102030ff).unwrap();
    let nodes: Vec<_> = (0..150)
        .map(|_| {
            tree.append(tree.root(), Style::default(), 0x4488ccff)
                .unwrap()
        })
        .collect();
    let mut order = nodes.clone();
    order.reverse();
    let mut structure = StructureTransaction::default();
    structure.children(tree.root(), order);
    tree.commit_structure(structure).unwrap();
    let first = tree.local_frame([300.0, 200.0]).unwrap();
    let saved = first.resolve();
    let mut latest = first.clone();
    let mut expected = vec![0x4488ccff; nodes.len()];
    // Full pages and sparse cross-page patches, plus equal final values.
    for step in 0..3 {
        let mut tx = Transaction::default();
        for (i, &node) in nodes.iter().enumerate() {
            if step == 0 || i % 3 == step {
                if step == 0 || i % 5 != 0 {
                    expected[i] = 0x203040ff + i as u32 * 0x010000 + step as u32 * 0x0100;
                }
                tx.color(node, expected[i]);
            }
        }
        tree.commit(tx, BackendCapabilities::default()).unwrap();
        let next = tree.local_frame([300.0, 200.0]).unwrap();
        assert!(next.follows(&latest));
        assert!(!next.stats().full_publication);
        assert!(next.0.material_changed.windows(2).all(|p| p[0] < p[1]));
        for item in next.resolve().items.iter().skip(1) {
            let i = nodes.iter().position(|node| *node == item.node).unwrap();
            assert_eq!(item.fill_color, expected[i]);
        }
        latest = next;
    }
    equivalent(&latest.resolve(), &tree.frame([300.0, 200.0]).unwrap());
    equivalent(&saved, &first.resolve());
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

#[test]
fn unordered_multi_commit_journals_keep_final_values_and_reused_generation() {
    let (mut tree, nodes) = fixture();
    let original = tree.local_frame([300.0, 200.0]).unwrap();
    let original_resolved = original.resolve();
    for round in 1..5 {
        let mut tx = Transaction::default();
        for index in (0..nodes.len()).rev() {
            tx.translation(nodes[index], [round as f32, index as f32 * 0.125]);
        }
        tree.commit(tx, BackendCapabilities::default()).unwrap();
    }
    assert_eq!(tree.local_dirty.len(), nodes.len());
    let latest = tree.local_frame([300.0, 200.0]).unwrap();
    assert_eq!(latest.stats().changed_nodes, nodes.len());
    equivalent(&latest.resolve(), &tree.frame([300.0, 200.0]).unwrap());
    equivalent(&original_resolved, &original.resolve());

    let retired = *nodes.last().unwrap();
    let parent = tree.node(retired).unwrap().parent.unwrap();
    let mut tx = Transaction::default();
    tx.translation(retired, [7.0, 8.0]);
    tree.commit(tx, BackendCapabilities::default()).unwrap();
    tree.remove(retired).unwrap();
    let replacement = tree.append(parent, Style::default(), 0x556677ff).unwrap();
    assert_eq!(replacement.slot, retired.slot);
    assert_ne!(replacement.generation, retired.generation);
    let mut tx = Transaction::default();
    tx.translation(replacement, [9.0, 10.0]);
    tree.commit(tx, BackendCapabilities::default()).unwrap();
    let frame = tree.local_frame([300.0, 200.0]).unwrap();
    assert!(frame.resolve_geometry(retired).is_none());
    assert_eq!(
        frame
            .resolve_geometry(replacement)
            .unwrap()
            .spatial
            .translation,
        [9.0, 10.0]
    );
    equivalent(&frame.resolve(), &tree.frame([300.0, 200.0]).unwrap());
    assert!(tree.local_dirty.is_empty());
}
