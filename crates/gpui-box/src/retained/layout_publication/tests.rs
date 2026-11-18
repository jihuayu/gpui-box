use super::*;

#[test]
fn batched_dry_queries_match_single_queries_and_preserve_published_layout_on_error() {
    let mut tree = UiTree::new_layout(LayoutStyle::column(), 0).unwrap();
    let a = tree
        .append_layout(
            tree.root(),
            LayoutStyle::column().size(LayoutDimension::Px(20.0), LayoutDimension::Px(10.0)),
            1,
        )
        .unwrap();
    let b = tree
        .append_layout(
            tree.root(),
            LayoutStyle::column().size(LayoutDimension::Px(30.0), LayoutDimension::Px(15.0)),
            2,
        )
        .unwrap();
    let frame = tree.local_frame([100.0, 80.0]).unwrap();
    let before = tree.layout_result(a).unwrap();
    let input = ChildLayoutInput::new(LayoutConstraints::viewport(100.0, 80.0));
    let expected = vec![
        tree.dry_measure_node(a, input).unwrap(),
        tree.dry_measure_node(b, input).unwrap(),
    ];
    assert_eq!(
        tree.dry_measure_nodes(&[(a, input), (b, input)]).unwrap(),
        expected
    );
    let mut invalid = input;
    invalid.known_size.height = Some(f32::NAN);
    assert!(tree.dry_measure_nodes(&[(a, input), (b, invalid)]).is_err());
    assert_eq!(tree.layout_result(a).unwrap(), before);
    assert_eq!(
        tree.published_local_frame().unwrap().revision(),
        frame.revision()
    );
    assert!(tree.dry_measure_nodes(&[]).unwrap().is_empty());
}

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

#[test]
fn containing_block_offsets_survive_static_ancestors_and_incremental_publication() {
    let raster = |frame: FramePacket| {
        let mut job = software::SoftwareJob::new(
            frame,
            software::SoftwareOptions {
                size: [300, 200],
                scale: 1.0,
                background: 0,
                effects: pacing::SoftwareEffects::Exact,
                max_bytes: 16 * 1024 * 1024,
            },
        )
        .unwrap();
        while !job.step(4096).unwrap() {}
        job.into_frame().unwrap()
    };
    let make = || {
        let mut tree = UiTree::new_layout(
            LayoutStyle::column().size(LayoutDimension::Px(300.0), LayoutDimension::Px(200.0)),
            0,
        )
        .unwrap();
        let host_style = LayoutStyle::column()
            .size(LayoutDimension::Px(100.0), LayoutDimension::Px(80.0))
            .container_type(LayoutContainerType::Size)
            .margin(LayoutEdges {
                left: LayoutLength::Px(20.0),
                top: LayoutLength::Px(10.0),
                ..LayoutEdges::zero()
            });
        let host = tree
            .append_layout(tree.root(), host_style.clone(), 0)
            .unwrap();
        let middle_style = LayoutStyle::column()
            .size(LayoutDimension::Px(60.0), LayoutDimension::Px(40.0))
            .margin(LayoutEdges {
                left: LayoutLength::Px(13.0),
                top: LayoutLength::Px(17.0),
                ..LayoutEdges::zero()
            });
        let middle = tree.append_layout(host, middle_style.clone(), 0).unwrap();
        let mut absolute_style =
            LayoutStyle::row().size(LayoutDimension::Px(10.0), LayoutDimension::Px(8.0));
        absolute_style.position = layout::LayoutPosition::Absolute;
        absolute_style.inset = LayoutEdges {
            left: LayoutLength::Px(5.0),
            top: LayoutLength::Px(6.0),
            ..LayoutEdges::all(LayoutLength::Auto)
        };
        let absolute = tree
            .append_layout(middle, absolute_style, 0xff0000ff)
            .unwrap();
        let mut nested_style =
            LayoutStyle::row().size(LayoutDimension::Px(2.0), LayoutDimension::Px(2.0));
        nested_style.position = layout::LayoutPosition::Absolute;
        nested_style.inset = LayoutEdges {
            left: LayoutLength::Px(2.0),
            top: LayoutLength::Px(3.0),
            ..LayoutEdges::all(LayoutLength::Auto)
        };
        let nested = tree
            .append_layout(absolute, nested_style, 0x00ff00ff)
            .unwrap();
        (
            tree,
            [host, middle, absolute, nested],
            host_style,
            middle_style,
        )
    };
    let (mut fast, ids, host_style, middle_style) = make();
    let (mut full, other, _, _) = make();
    full.set_frame_build_policy(FrameBuildPolicy::Full);
    let original = fast.frame([300.0, 200.0]).unwrap();
    equivalent(&original, &full.frame([300.0, 200.0]).unwrap());
    assert_eq!(fast.layout_result(ids[2]).unwrap().location, [-8.0, -11.0]);
    let bounds = |frame: &FramePacket, id| {
        frame
            .items
            .iter()
            .find(|item| item.node == id)
            .unwrap()
            .bounds
    };
    assert_eq!(
        [bounds(&original, ids[2]).x, bounds(&original, ids[2]).y],
        [25.0, 16.0]
    );
    assert_eq!(original.hit_test(27.5, 19.5), Some(ids[3]));
    let old_local = fast.local_frame([300.0, 200.0]).unwrap();
    for step in 0..4 {
        for (tree, ids) in [(&mut fast, ids), (&mut full, other)] {
            let mut tx = Transaction::default();
            match step {
                0 => {
                    let mut moved = middle_style.clone();
                    moved.margin.left = LayoutLength::Px(23.0);
                    moved.margin.top = LayoutLength::Px(27.0);
                    tx.layout_style(ids[1], moved);
                }
                1 => tx.layout_style(
                    ids[0],
                    host_style
                        .clone()
                        .container_type(LayoutContainerType::Normal),
                ),
                2 => tx.translation(ids[1], [2.0, 3.0]),
                _ => tx.layout_style(ids[0], host_style.clone()),
            }
            let published = tree.layout_result(ids[2]).unwrap();
            tree.commit(tx, BackendCapabilities::default()).unwrap();
            assert_eq!(
                tree.layout_result(ids[2]).unwrap(),
                published,
                "unpublished style cannot reinterpret old coordinates"
            );
        }
        let a = fast.frame([300.0, 200.0]).unwrap();
        let b = full.frame([300.0, 200.0]).unwrap();
        equivalent(&a, &b);
        let expected = match step {
            0 | 3 => [25.0, 16.0],
            1 => [5.0, 6.0],
            _ => [7.0, 9.0],
        };
        let expected = if step == 3 { [27.0, 19.0] } else { expected };
        let rect = bounds(&a, ids[2]);
        assert_eq!([rect.x, rect.y], expected);
        let pixels = raster(a.clone());
        assert_eq!(pixels.pixels(), raster(b).pixels());
        let offset = (expected[1] as usize * 300 + expected[0] as usize) * 4;
        assert_eq!(&pixels.pixels()[offset..offset + 4], &[255, 0, 0, 255]);
        let local = fast.local_frame([300.0, 200.0]).unwrap();
        assert_eq!(bounds(&local.resolve(), ids[2]), rect);
        assert_eq!(
            local.hit_test(expected[0] + 2.5, expected[1] + 3.5),
            Some(ids[3])
        );
        assert_eq!(a.recorded_chunks, 0);
    }
    assert_eq!(
        bounds(&old_local.resolve(), ids[2]),
        bounds(&original, ids[2])
    );
    // Topology edits also maintain the indexed out-of-flow set and normalize
    // coordinates relative to the new render parent.
    fast.edit(BackendCapabilities::default(), |edit| {
        edit.reparent(ids[2], ids[0])?;
        Ok(())
    })
    .unwrap();
    let moved = fast.frame([300.0, 200.0]).unwrap();
    assert_eq!(fast.layout_result(ids[2]).unwrap().location, [5.0, 6.0]);
    assert_eq!(
        [bounds(&moved, ids[2]).x, bounds(&moved, ids[2]).y],
        [25.0, 16.0]
    );
    fast.remove(ids[2]).unwrap();
    assert!(fast.out_of_flow.is_empty());
    fast.edit(BackendCapabilities::default(), |edit| {
        let mut style = LayoutStyle::row().size(LayoutDimension::Px(4.0), LayoutDimension::Px(4.0));
        style.position = layout::LayoutPosition::Absolute;
        edit.append_layout(ids[1], style, 3)?;
        Ok(())
    })
    .unwrap();
    assert_eq!(fast.out_of_flow.len(), 1);
    fast.frame([300.0, 200.0]).unwrap();
}
