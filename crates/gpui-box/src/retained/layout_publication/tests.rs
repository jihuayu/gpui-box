use super::*;

fn fixed(w: f32, h: f32) -> Style {
    Style {
        size: Size {
            width: length(w),
            height: length(h),
        },
        flex_shrink: 0.0,
        ..Default::default()
    }
}
type Branches = Vec<(NodeId, Vec<NodeId>)>;
fn fixture() -> (UiTree, Branches) {
    let mut tree = UiTree::new(
        Style {
            flex_direction: taffy::FlexDirection::Column,
            ..fixed(300.0, 300.0)
        },
        0x111111ff,
    )
    .unwrap();
    let mut branches = vec![];
    for _ in 0..8 {
        let parent = tree
            .append(tree.root(), fixed(180.0, 28.0), 0x4488ff80)
            .unwrap();
        let mut children = vec![];
        for _ in 0..20 {
            children.push(tree.append(parent, fixed(8.0, 8.0), 0x44ff8880).unwrap());
        }
        branches.push((parent, children));
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
        assert_eq!(
            (a.bounds, a.content_bounds, a.clip, a.fill_color),
            (b.bounds, b.content_bounds, b.clip, b.fill_color)
        );
        assert_eq!(a.commands, b.commands);
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
            .map(identity)
        );
    }
    macro_rules! props {
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
    props!(transforms);
    props!(clips);
    props!(scrolls);
    props!(effects);
    assert_eq!(a.damage, b.damage);
    assert_eq!(
        a.updates
            .iter()
            .map(|i| identity(Some(i.node)))
            .collect::<Vec<_>>(),
        b.updates
            .iter()
            .map(|i| identity(Some(i.node)))
            .collect::<Vec<_>>()
    );
    assert_eq!(
        a.removed
            .iter()
            .map(|id| identity(Some(*id)))
            .collect::<Vec<_>>(),
        b.removed
            .iter()
            .map(|id| identity(Some(*id)))
            .collect::<Vec<_>>()
    );
    for y in [0.0, 8.0, 27.0, 35.0, 200.0] {
        for x in [0.0, 7.0, 50.0, 170.0, 290.0] {
            assert_eq!(identity(a.hit_test(x, y)), identity(b.hit_test(x, y)));
        }
    }
}

#[test]
fn fixed_parent_and_leaf_sizes_extract_only_changed_geometry_and_retain_pages() {
    let (mut tree, branches) = fixture();
    let original = tree.frame([300.0; 2]).unwrap();
    let parent = branches[3].0;
    let leaf = branches[3].1[19];
    let mut tx = Transaction::default();
    tx.style(leaf, fixed(9.0, 10.0));
    tree.commit(tx, BackendCapabilities::default()).unwrap();
    let resized = tree.frame([300.0; 2]).unwrap();
    assert_eq!(resized.build_stats.extracted_nodes, 1);
    assert_eq!(resized.layout_runs, 1);
    assert!(!resized.build_stats.full_rebuild);
    assert!(resized.layout_stats.layout_writes > 0);
    assert!(resized.layout_stats.net_changed_nodes > 0);
    assert!(Arc::ptr_eq(&resized.properties, &original.properties));
    assert_eq!(
        resized.items.shared_pages(&original.items),
        resized.items.page_count() - 1
    );
    let mut tx = Transaction::default();
    tx.style(parent, fixed(190.0, 28.0));
    tree.commit(tx, BackendCapabilities::default()).unwrap();
    let parent_size = tree.frame([300.0; 2]).unwrap();
    assert_eq!(
        parent_size.build_stats.extracted_nodes, 1,
        "unchanged child layouts remain shared"
    );
    assert_eq!(parent_size.updates.len(), 1);
    assert_eq!(
        original
            .items
            .iter()
            .find(|i| i.node == leaf)
            .unwrap()
            .bounds
            .width,
        8.0
    );
}

#[test]
fn flex_dependents_scroll_clamps_inherited_properties_and_mixed_updates_match_full() {
    let (mut fast, nodes) = fixture();
    let (mut full, other) = fixture();
    full.set_frame_build_policy(FrameBuildPolicy::Full);
    let original = fast.frame([300.0; 2]).unwrap();
    full.frame([300.0; 2]).unwrap();
    for round in 0..40 {
        for (tree, branches) in [(&mut fast, &nodes), (&mut full, &other)] {
            let (parent, leaves) = &branches[3];
            let mut tx = Transaction::default();
            match round % 8 {
                0 => {
                    tx.style(leaves[0], fixed(15.0, 12.0));
                    tx.color(branches[7].1[4], 0xff4488ff);
                }
                1 => {
                    tx.style(
                        *parent,
                        Style {
                            flex_wrap: taffy::FlexWrap::Wrap,
                            align_content: taffy::AlignContent::START,
                            ..fixed(90.0, 28.0)
                        },
                    );
                    tx.clip(
                        *parent,
                        Some(Rect {
                            x: 0.0,
                            y: 0.0,
                            width: 85.0,
                            height: 28.0,
                        }),
                    );
                    tx.opacity(*parent, 0.5);
                    tx.scroll(*parent, [1000.0, 1000.0]);
                }
                2 => {
                    tx.style(leaves[5], fixed(8.0, 25.0));
                    tx.translation(branches[0].0, [1.0, 2.0]);
                }
                3 => {
                    tx.style(
                        *parent,
                        Style {
                            padding: taffy::Rect {
                                left: length(3.0),
                                right: length(2.0),
                                top: length(1.0),
                                bottom: length(2.0),
                            },
                            ..fixed(120.0, 35.0)
                        },
                    );
                }
                4 => {
                    tx.measure(
                        leaves[19],
                        Some(MeasureSnapshot::Fixed(CustomMeasureSnapshot {
                            width: 25.0,
                            height: 15.0,
                            baseline: Some(9.0),
                        })),
                    );
                    tx.style(
                        leaves[19],
                        Style {
                            flex_shrink: 0.0,
                            ..Default::default()
                        },
                    );
                }
                5 => {
                    tx.style(*parent, fixed(180.0, 28.0));
                    tx.scroll(*parent, [0.0; 2]);
                    tx.clip(*parent, None);
                    tx.opacity(*parent, 1.0);
                    tx.translation(branches[0].0, [0.0; 2]);
                }
                6 => {
                    tx.style(leaves[0], fixed(8.0, 8.0));
                    tx.style(leaves[5], fixed(8.0, 8.0));
                    tx.style(leaves[19], fixed(8.0, 8.0));
                    tx.measure(leaves[19], None);
                }
                _ => {
                    tx.style(
                        *parent,
                        Style {
                            align_items: taffy::AlignItems::END,
                            ..fixed(180.0, 28.0)
                        },
                    );
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
        let a = fast.frame([300.0; 2]).unwrap();
        let b = full.frame([300.0; 2]).unwrap();
        equivalent(&a, &b);
        assert_eq!(a.layout_runs, 1);
        assert_eq!(a.recorded_chunks, 0);
        assert!(!a.build_stats.full_rebuild);
        assert!(a.build_stats.extracted_nodes < a.items.len());
    }
    assert_eq!(original.items[0].bounds.height, 300.0);
}

#[test]
fn unchanged_final_layout_publishes_no_geometry_and_visibility_structure_resize_fall_back() {
    let (mut tree, nodes) = fixture();
    let before = tree.frame([300.0; 2]).unwrap();
    let mut tx = Transaction::default();
    let leaf = nodes[0].1[0];
    tx.style(
        leaf,
        Style {
            min_size: Size {
                width: length(1.0),
                height: length(1.0),
            },
            ..fixed(8.0, 8.0)
        },
    );
    tree.commit(tx, BackendCapabilities::default()).unwrap();
    let same = tree.frame([300.0; 2]).unwrap();
    assert_eq!(same.layout_stats.net_changed_nodes, 0);
    assert_eq!(same.build_stats.extracted_nodes, 0);
    assert!(Arc::ptr_eq(&same.items, &before.items));
    assert!(same.updates.is_empty());
    assert_eq!(same.damage.bounds, None);
    let mut tx = Transaction::default();
    tx.style(
        nodes[0].0,
        Style {
            display: taffy::Display::None,
            ..fixed(180.0, 28.0)
        },
    );
    tree.commit(tx, BackendCapabilities::default()).unwrap();
    let hidden = tree.frame([300.0; 2]).unwrap();
    assert!(hidden.build_stats.full_rebuild);
    let mut tx = Transaction::default();
    tx.style(leaf, fixed(40.0, 25.0));
    tree.commit(tx, BackendCapabilities::default()).unwrap();
    let hidden_change = tree.frame([300.0; 2]).unwrap();
    assert_eq!(hidden_change.build_stats.extracted_nodes, 0);
    assert!(Arc::ptr_eq(&hidden.items, &hidden_change.items));
    let mut tx = Transaction::default();
    tx.style(nodes[0].0, fixed(180.0, 28.0));
    tree.commit(tx, BackendCapabilities::default()).unwrap();
    assert!(tree.frame([300.0; 2]).unwrap().build_stats.full_rebuild);
    tree.remove(leaf).unwrap();
    let new = tree.append(nodes[0].0, fixed(8.0, 8.0), 0).unwrap();
    assert_ne!(new, leaf);
    assert!(tree.frame([300.0; 2]).unwrap().build_stats.full_rebuild);
    assert!(tree.frame([301.0, 300.0]).unwrap().build_stats.full_rebuild);
}

#[test]
fn layout_observer_agrees_with_all_final_results_and_original_entry_point() {
    let mut layout = TaffyTree::<()>::new();
    layout.disable_rounding();
    let a = layout.new_leaf(fixed(10.0, 20.0)).unwrap();
    let b = layout.new_leaf(fixed(15.0, 20.0)).unwrap();
    let root = layout
        .new_with_children(fixed(100.0, 40.0), &[a, b])
        .unwrap();
    let mut control = layout.clone();
    let available = Size {
        width: AvailableSpace::Definite(100.0),
        height: AvailableSpace::Definite(40.0),
    };
    for width in [10.0, 25.0, 25.0, 5.0] {
        layout.set_style(a, fixed(width, 20.0)).unwrap();
        control.set_style(a, fixed(width, 20.0)).unwrap();
        let before = [root, a, b].map(|id| *layout.layout(id).unwrap());
        let mut observed = std::collections::HashSet::new();
        layout
            .compute_layout_with_measure_and_observer(
                root,
                available,
                |inputs, _, _, style| {
                    taffy::compute_leaf_layout(inputs, style, |_, _| 0.0, |_, _| Size::ZERO)
                },
                |id, old, new| {
                    if old != new {
                        observed.insert(id);
                    }
                },
            )
            .unwrap();
        control.compute_layout(root, available).unwrap();
        for (id, old) in [root, a, b].into_iter().zip(before) {
            assert_eq!(layout.layout(id).unwrap(), control.layout(id).unwrap());
            if *layout.layout(id).unwrap() != old {
                assert!(observed.contains(&id));
            }
        }
    }
}

#[test]
fn grid_block_absolute_constraints_and_baselines_match_complete_extraction() {
    let make = || {
        let mut tree = UiTree::new(
            Style {
                flex_direction: taffy::FlexDirection::Column,
                ..fixed(300.0, 300.0)
            },
            0x111111ff,
        )
        .unwrap();
        let mut groups = vec![];
        let mut leaves = vec![];
        for display in [
            taffy::Display::Grid,
            taffy::Display::Block,
            taffy::Display::Flex,
        ] {
            let group = tree
                .append(
                    tree.root(),
                    Style {
                        display,
                        ..fixed(150.0, 70.0)
                    },
                    0x22448880,
                )
                .unwrap();
            groups.push(group);
            for i in 0..8 {
                let leaf = tree
                    .append(
                        group,
                        Style {
                            position: if i == 7 {
                                taffy::Position::Absolute
                            } else {
                                taffy::Position::Relative
                            },
                            ..fixed(12.0, 10.0)
                        },
                        0x88ff4480,
                    )
                    .unwrap();
                leaves.push(leaf);
            }
        }
        (tree, groups, leaves)
    };
    let (mut fast, groups, leaves) = make();
    let (mut full, other_groups, other_leaves) = make();
    full.set_frame_build_policy(FrameBuildPolicy::Full);
    fast.frame([300.0; 2]).unwrap();
    full.frame([300.0; 2]).unwrap();
    for step in 0..64 {
        for (tree, groups, leaves) in [
            (&mut fast, &groups, &leaves),
            (&mut full, &other_groups, &other_leaves),
        ] {
            let slot = step % leaves.len();
            let mut style = tree.style(leaves[slot]).unwrap().clone();
            let mut tx = Transaction::default();
            match step % 8 {
                0 => {
                    style.size.width = length(10.0 + (step % 13) as f32);
                    style.min_size.height = length(5.0);
                }
                1 => {
                    style.size.width = percent(0.15 + (step % 4) as f32 * 0.05);
                    style.size.height = length(12.0 + (step % 6) as f32);
                }
                2 => {
                    style.margin.left = length(2.0);
                    style.margin.top = length(3.0);
                }
                3 => {
                    style.position = taffy::Position::Absolute;
                    style.inset.left = length((step % 20) as f32);
                    style.inset.top = length(4.0);
                }
                4 => {
                    style.position = taffy::Position::Relative;
                    style.inset.left = length(1.25);
                    style.inset.top = length(-0.5);
                }
                5 => {
                    style.size.height = taffy::Dimension::auto();
                    tx.measure(
                        leaves[slot],
                        Some(MeasureSnapshot::Fixed(CustomMeasureSnapshot {
                            width: 14.0,
                            height: 13.0,
                            baseline: Some(7.0),
                        })),
                    );
                }
                6 => {
                    let group = groups[step % 3];
                    let mut parent = tree.style(group).unwrap().clone();
                    parent.padding.left = length(3.0);
                    parent.border.top = length(1.0);
                    parent.direction = if step % 2 == 0 {
                        taffy::Direction::Rtl
                    } else {
                        taffy::Direction::Ltr
                    };
                    tx.style(group, parent);
                    tx.scroll(group, [100.0, 100.0]);
                }
                _ => {
                    style.max_size.width = length(15.0);
                    tx.clip(
                        groups[step % 3],
                        Some(Rect {
                            x: 0.0,
                            y: 0.0,
                            width: 130.0,
                            height: 60.0,
                        }),
                    );
                }
            }
            tx.style(leaves[slot], style);
            tree.commit(tx, BackendCapabilities::default()).unwrap();
        }
        let a = fast.frame([300.0; 2]).unwrap();
        let b = full.frame([300.0; 2]).unwrap();
        equivalent(&a, &b);
        assert!(!a.build_stats.full_rebuild);
    }
}
