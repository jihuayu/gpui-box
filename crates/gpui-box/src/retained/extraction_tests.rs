use super::*;

type Branches = Vec<(NodeId, Vec<(NodeId, Vec<NodeId>)>)>;

fn fixture() -> (UiTree, Branches) {
    let fixed = |w, h| Style {
        size: Size {
            width: length(w),
            height: length(h),
        },
        flex_shrink: 0.0,
        ..Default::default()
    };
    let mut tree = UiTree::new(
        Style {
            flex_wrap: taffy::FlexWrap::Wrap,
            ..fixed(800.0, 600.0)
        },
        0x112233ff,
    )
    .unwrap();
    let mut branches = vec![];
    for _ in 0..20 {
        let group = tree
            .append(
                tree.root(),
                Style {
                    flex_direction: taffy::FlexDirection::Column,
                    ..fixed(80.0, 60.0)
                },
                0x44556680,
            )
            .unwrap();
        let mut rows = vec![];
        for _ in 0..6 {
            let row = tree.append(group, fixed(100.0, 24.0), 0x55667780).unwrap();
            let leaves = (0..2)
                .map(|_| tree.append(row, fixed(70.0, 20.0), 0x77889980).unwrap())
                .collect();
            rows.push((row, leaves));
        }
        branches.push((group, rows));
    }
    (tree, branches)
}

fn identity(id: Option<NodeId>) -> Option<(usize, u64)> {
    id.map(|id| (id.slot, id.generation))
}

fn equivalent(a: &FramePacket, b: &FramePacket) {
    assert_eq!(a.items.len(), b.items.len());
    for (a, b) in a.items.iter().zip(b.items.iter()) {
        assert_eq!(identity(Some(a.node)), identity(Some(b.node)));
        assert_eq!(a.bounds, b.bounds);
        assert_eq!(a.content_bounds, b.content_bounds);
        assert_eq!(a.clip, b.clip);
        assert_eq!(a.commands, b.commands);
        assert_eq!(a.fill_color, b.fill_color);
        assert_eq!(
            [
                a.properties.transform,
                a.properties.clip,
                a.properties.scroll,
                a.properties.effect
            ]
            .map(identity),
            [
                b.properties.transform,
                b.properties.clip,
                b.properties.scroll,
                b.properties.effect
            ]
            .map(identity),
        );
    }
    macro_rules! compare_map {
        ($field:ident) => {
            assert_eq!(
                a.properties
                    .$field
                    .iter()
                    .map(|(id, p)| (identity(Some(*id)), identity(p.parent), p.value))
                    .collect::<Vec<_>>(),
                b.properties
                    .$field
                    .iter()
                    .map(|(id, p)| (identity(Some(*id)), identity(p.parent), p.value))
                    .collect::<Vec<_>>()
            );
        };
    }
    compare_map!(transforms);
    compare_map!(clips);
    compare_map!(scrolls);
    compare_map!(effects);
    assert_eq!(a.damage, b.damage);
    assert_eq!(
        a.updates
            .iter()
            .map(|item| identity(Some(item.node)))
            .collect::<Vec<_>>(),
        b.updates
            .iter()
            .map(|item| identity(Some(item.node)))
            .collect::<Vec<_>>()
    );
    for (x, y) in [(0.0, 0.0), (25.0, 15.0), (103.0, 42.0), (450.0, 95.0)] {
        assert_eq!(identity(a.hit_test(x, y)), identity(b.hit_test(x, y)));
    }
}

#[test]
fn nested_property_extraction_matches_full_for_combined_updates_and_removal() {
    let (mut fast, nodes) = fixture();
    let (mut full, other) = fixture();
    full.set_frame_build_policy(FrameBuildPolicy::Full);
    let original = fast.frame([800.0, 600.0]).unwrap();
    full.frame([800.0, 600.0]).unwrap();
    let original_properties = original.properties.clone();
    for round in 0..40 {
        for (tree, branches) in [(&mut fast, &nodes), (&mut full, &other)] {
            let mut tx = Transaction::default();
            let (group, rows) = &branches[1];
            let row = rows[2].0;
            let leaf = rows[2].1[0];
            match round % 8 {
                0 => {
                    tx.translation(*group, [3.25, -2.5]);
                    tx.translation(row, [-1.0, 0.25]);
                    tx.clip(
                        *group,
                        Some(Rect {
                            x: 1.0,
                            y: 2.0,
                            width: 72.0,
                            height: 55.0,
                        }),
                    );
                    tx.clip(
                        row,
                        Some(Rect {
                            x: 0.0,
                            y: 0.0,
                            width: 88.0,
                            height: 22.0,
                        }),
                    );
                    tx.scroll(*group, [8.0, 41.0]);
                    tx.scroll(row, [5000.0, 5000.0]);
                    tx.opacity(*group, 0.4);
                    tx.opacity(row, 0.7);
                }
                1 => {
                    // Parent context includes inherited clip, effect and scroll.
                    tx.translation(leaf, [0.5, 1.5]);
                    tx.clip(
                        leaf,
                        Some(Rect {
                            x: 0.0,
                            y: 0.0,
                            width: 65.0,
                            height: 18.0,
                        }),
                    );
                    tx.opacity(leaf, 0.6);
                    tx.color(leaf, 0xff884480);
                }
                2 => {
                    tx.translation(row, [0.0; 2]);
                    tx.clip(row, None);
                    tx.scroll(row, [0.0; 2]);
                    tx.opacity(row, 1.0);
                }
                3 => {
                    tx.translation(*group, [0.0; 2]);
                    tx.clip(*group, None);
                    tx.scroll(*group, [0.0; 2]);
                    tx.opacity(*group, 1.0);
                }
                4 => {
                    tx.translation(tree.root(), [-0.25, 0.5]);
                    tx.opacity(tree.root(), 0.9);
                    tx.translation(leaf, [0.0; 2]);
                }
                5 => {
                    // Disjoint dirty roots and an unrelated local material.
                    tx.translation(branches[3].0, [1.0, 2.0]);
                    tx.opacity(branches[8].0, 0.5);
                    tx.color(branches[17].1[0].1[1], 0xff448880);
                }
                6 => {
                    tx.translation(tree.root(), [0.0; 2]);
                    tx.opacity(tree.root(), 1.0);
                    tx.clip(leaf, None);
                    tx.opacity(leaf, 1.0);
                }
                _ => {
                    tx.translation(branches[3].0, [0.0; 2]);
                    tx.opacity(branches[8].0, 1.0);
                    tx.color(leaf, 0x77889980);
                    tx.color(branches[17].1[0].1[1], 0x77889980);
                }
            }
            tree.commit(
                tx,
                BackendCapabilities {
                    solid_color_patch: true,
                },
            )
            .unwrap();
        }
        let a = fast.frame([800.0, 600.0]).unwrap();
        let b = full.frame([800.0, 600.0]).unwrap();
        equivalent(&a, &b);
        assert!(!a.build_stats.full_rebuild);
        assert_eq!((a.layout_runs, a.recorded_chunks), (0, 0));
        match round % 8 {
            0 | 3 => assert_eq!(a.build_stats.extracted_nodes, 19),
            1 => assert_eq!(a.build_stats.extracted_nodes, 1),
            2 => assert_eq!(a.build_stats.extracted_nodes, 3),
            4 | 6 => assert_eq!(a.build_stats.extracted_nodes, 381),
            5 => assert_eq!(a.build_stats.extracted_nodes, 39),
            _ => {}
        }
    }
    assert_eq!(*original.properties, *original_properties);
    assert!(original.properties.transforms.is_empty());
    assert!(original.properties.effects.is_empty());
    assert_eq!(original.items[0].bounds.x, 0.0);
}

#[test]
fn hidden_property_changes_stay_invisible_until_layout_restore() {
    let (mut tree, branches) = fixture();
    let id = branches[0].0;
    let style = tree.style(id).unwrap().clone();
    let mut hidden = style.clone();
    hidden.display = taffy::Display::None;
    let mut tx = Transaction::default();
    tx.style(id, hidden);
    tree.commit(tx, BackendCapabilities::default()).unwrap();
    let before = tree.frame([800.0, 600.0]).unwrap();
    let mut tx = Transaction::default();
    tx.translation(id, [3.0, 4.0]);
    tx.opacity(id, 0.5);
    tree.commit(tx, BackendCapabilities::default()).unwrap();
    let after = tree.frame([800.0, 600.0]).unwrap();
    assert_eq!(after.build_stats.extracted_nodes, 0);
    assert!(Arc::ptr_eq(&before.items, &after.items));
    assert!(Arc::ptr_eq(&before.properties, &after.properties));
    assert_eq!(after.damage.bounds, None);
    let mut tx = Transaction::default();
    tx.style(id, style);
    tree.commit(tx, BackendCapabilities::default()).unwrap();
    let restored = tree.frame([800.0, 600.0]).unwrap();
    assert!(restored.build_stats.full_rebuild);
    assert!(restored.properties.effects.contains_key(&id));
    assert_eq!(restored.items[1].bounds.x, 3.0);
}

#[test]
fn local_map_edits_share_unmodified_kinds_and_retired_generations_stay_in_old_frames() {
    let (mut tree, branches) = fixture();
    let (group, rows) = &branches[0];
    let leaf = rows[0].1[0];
    let mut tx = Transaction::default();
    for id in [*group, leaf] {
        tx.translation(id, [1.0, 2.0]);
        tx.opacity(id, 0.5);
        tx.clip(
            id,
            Some(Rect {
                x: 0.0,
                y: 0.0,
                width: 40.0,
                height: 20.0,
            }),
        );
        tx.scroll(id, [1.0, 0.0]);
    }
    tree.commit(tx, BackendCapabilities::default()).unwrap();
    let original = tree.frame([800.0, 600.0]).unwrap();
    let mut tx = Transaction::default();
    tx.translation(leaf, [3.0, 4.0]);
    tree.commit(tx, BackendCapabilities::default()).unwrap();
    let moved = tree.frame([800.0, 600.0]).unwrap();
    assert_eq!(moved.build_stats.extracted_nodes, 1);
    assert!(
        moved
            .properties
            .effects
            .shares_root(&original.properties.effects)
    );
    assert!(
        moved
            .properties
            .clips
            .shares_root(&original.properties.clips)
    );
    assert!(
        moved
            .properties
            .scrolls
            .shares_root(&original.properties.scrolls)
    );
    assert!(
        !moved
            .properties
            .transforms
            .shares_root(&original.properties.transforms)
    );
    let mut tx = Transaction::default();
    tx.translation(*group, [0.0; 2]);
    tx.clip(*group, None);
    tx.scroll(*group, [0.0; 2]);
    tx.opacity(*group, 1.0);
    tree.commit(tx, BackendCapabilities::default()).unwrap();
    let reset = tree.frame([800.0, 600.0]).unwrap();
    assert_eq!(reset.properties.transforms[&leaf].parent, None);
    assert_eq!(reset.properties.effects[&leaf].parent, None);
    assert_eq!(reset.properties.clips[&leaf].parent, None);
    assert_eq!(reset.properties.scrolls[&leaf].parent, None);
    assert_eq!(original.properties.effects[&leaf].parent, Some(*group));
    assert_eq!(original.properties.transforms[&leaf].value, [1.0, 2.0]);
    tree.remove(leaf).unwrap();
    let new = tree.append(rows[0].0, Style::default(), 0).unwrap();
    assert_eq!(new.slot, leaf.slot);
    assert_ne!(new.generation, leaf.generation);
    let mut tx = Transaction::default();
    tx.opacity(new, 0.7);
    tree.commit(tx, BackendCapabilities::default()).unwrap();
    let current = tree.frame([800.0, 600.0]).unwrap();
    assert!(!current.properties.effects.contains_key(&leaf));
    assert_eq!(current.properties.effects[&new].value, 0.7);
    assert_eq!(original.properties.effects[&leaf].value, 0.5);
}
