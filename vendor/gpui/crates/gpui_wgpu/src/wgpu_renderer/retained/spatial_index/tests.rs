use super::*;
fn anchor(x: f32) -> SpatialNode {
    SpatialNode {
        anchor_origin: [x, 0.0, 0.0, 0.0],
        links: [u32::MAX, 0, 2, 0],
        ..Default::default()
    }
}
fn child(parent: u32, depth: u32) -> SpatialNode {
    SpatialNode {
        layout_translation: [1.0, 2.0, 0.0, 0.0],
        links: [parent, depth, 0, 0],
        ..Default::default()
    }
}
// Independent full root-to-leaf walk, without cached edges/resolved state.
fn full(nodes: &[SpatialNode]) -> Vec<Resolved> {
    (0..nodes.len())
        .map(|slot| {
            let mut path = vec![];
            let mut cursor = slot;
            while nodes[cursor].links[2] & 2 == 0 {
                path.push(cursor);
                cursor = nodes[cursor].links[0] as usize;
                assert!(path.len() < 32);
            }
            let root = nodes[cursor];
            let mut r = Resolved {
                origin: [root.anchor_origin[0], root.anchor_origin[1]],
                scroll: [root.scroll[0], root.scroll[1]],
                clip: (root.links[2] & 8 != 0).then_some(root.anchor_clip),
            };
            for slot in path.into_iter().rev() {
                let n = nodes[slot];
                for axis in 0..2 {
                    r.origin[axis] = (r.origin[axis] - r.scroll[axis]) + n.layout_translation[axis];
                    r.origin[axis] += n.layout_translation[axis + 2];
                }
                if n.links[2] & 1 != 0 {
                    let c = [
                        n.clip[0] + r.origin[0],
                        n.clip[1] + r.origin[1],
                        n.clip[2],
                        n.clip[3],
                    ];
                    r.clip = Some(r.clip.map_or(c, |old| {
                        let x = old[0].max(c[0]);
                        let y = old[1].max(c[1]);
                        [
                            x,
                            y,
                            (old[0] + old[2]).min(c[0] + c[2]).max(x) - x,
                            (old[1] + old[3]).min(c[1] + c[3]).max(y) - y,
                        ]
                    }));
                }
                r.scroll = [n.scroll[0], n.scroll[1]];
            }
            r
        })
        .collect()
}
#[test]
fn indexed_updates_match_full_walk_through_reparent_clip_scroll_and_overlaps() {
    let mut nodes: Vec<_> = (0..129)
        .map(|i| {
            if i % 32 == 0 {
                anchor(i as f32)
            } else {
                child(i - 1, i % 32)
            }
        })
        .collect();
    let mut index = GraphIndex::default();
    let p = index
        .prepare(129, 1.0, false, std::iter::empty(), |s| nodes[s as usize])
        .unwrap();
    assert!(p.full);
    index.commit(p);
    let mut random = 17u32;
    for step in 0..600 {
        random = random.wrapping_mul(1664525).wrapping_add(1013904223);
        let slot = (random % 128) as usize;
        let mut n = nodes[slot];
        match step % 5 {
            0 => {
                if n.links[2] & 2 != 0 {
                    n.anchor_origin[0] += 0.25;
                } else {
                    n.layout_translation[2] += 0.25;
                }
            }
            1 => {
                n.scroll = [(step % 7) as f32, 1.25, 0.0, 0.0];
            }
            2 => {
                n.links[2] |= 1;
                n.clip = [0.25, 1.75, 13.0, 23.0];
            }
            3 => {
                if n.links[2] & 2 == 0 {
                    let depth = n.links[1];
                    n.links[0] = ((slot / 32 + 1) % 4 * 32) as u32 + depth - 1;
                }
            }
            _ => {
                n.links[2] &= !1;
            }
        }
        // Last overlapping write wins, as in the GPU mirror transport.
        let mut ignored = n;
        ignored.links[0] = 999;
        let writes = [(slot as u32, ignored), (slot as u32, n)];
        let p = index
            .prepare(129, 1.0, false, writes.into_iter(), |s| nodes[s as usize])
            .unwrap();
        assert!(!p.full);
        assert!(!p.affected.is_empty());
        nodes[slot] = n;
        index.commit(p);
        assert_eq!(index.resolved, full(&nodes), "step {step}");
        for (parent, children) in index.children.iter().enumerate() {
            for (position, child) in children.iter().enumerate() {
                assert_eq!(index.parents[*child as usize], Some(parent as u32));
                assert_eq!(index.positions[*child as usize], position);
            }
        }
    }
    let p = index
        .prepare(129, 1.0, false, std::iter::empty(), |s| nodes[s as usize])
        .unwrap();
    assert!(p.affected.is_empty());
}

#[test]
fn rejected_edges_overflow_and_uncommitted_plans_leave_actual_base_usable() {
    let nodes = [anchor(0.0), child(0, 1), child(1, 2), anchor(9.0)];
    let mut index = GraphIndex::default();
    let p = index
        .prepare(4, 1.0, false, std::iter::empty(), |s| nodes[s as usize])
        .unwrap();
    index.commit(p);
    let base = index.resolved.clone();
    let mut invalid = nodes[1];
    invalid.links[1] = 2;
    assert!(
        index
            .prepare(4, 1.0, false, [(1, invalid)].into_iter(), |s| nodes
                [s as usize])
            .is_err()
    );
    let mut invalid = nodes[1];
    invalid.links = [2, 1, 0, 0];
    assert!(
        index
            .prepare(4, 1.0, false, [(1, invalid)].into_iter(), |s| nodes
                [s as usize])
            .is_err()
    );
    let mut huge = nodes[1];
    huge.layout_translation[0] = f32::MAX * 0.75;
    let p = index
        .prepare(4, 1.0, false, [(1, huge)].into_iter(), |s| {
            nodes[s as usize]
        })
        .unwrap();
    // A later image/layer preflight rejection drops an otherwise valid plan.
    drop(p);
    assert_eq!(index.resolved, base);
    let p = index
        .prepare(4, 1.0, false, std::iter::empty(), |s| nodes[s as usize])
        .unwrap();
    assert!(p.affected.is_empty());
    let mut changed = nodes;
    changed[1] = huge;
    let p = index
        .prepare(4, 1.0, false, [(1, huge)].into_iter(), |s| {
            nodes[s as usize]
        })
        .unwrap();
    index.commit(p);
    assert!(
        index
            .prepare(
                4,
                1.0,
                false,
                [(0, anchor(f32::MAX * 0.5))].into_iter(),
                |s| { changed[s as usize] }
            )
            .is_err()
    );
    assert!(
        index
            .prepare(4, 2.0, false, std::iter::empty(), |s| changed[s as usize])
            .is_err()
    );
    let p = index
        .prepare(4, 1.0, false, std::iter::empty(), |s| changed[s as usize])
        .unwrap();
    assert!(p.affected.is_empty());
    assert_eq!(index.resolved, full(&changed));
}

#[test]
fn capacity_changes_revalidate_exposed_mirror_and_parent_bounds() {
    let nodes = [anchor(0.0), child(3, 1), anchor(0.0), anchor(8.0)];
    let mut index = GraphIndex::default();
    let p = index
        .prepare(4, 1.0, false, std::iter::empty(), |s| nodes[s as usize])
        .unwrap();
    index.commit(p);
    assert!(
        index
            .prepare(2, 1.0, false, std::iter::empty(), |s| nodes[s as usize])
            .is_err()
    );
    let p = index
        .prepare(1, 1.0, false, std::iter::empty(), |s| nodes[s as usize])
        .unwrap();
    assert!(p.full);
    index.commit(p);
    // Re-growing exposes the actual retained mirror, not invented empty nodes.
    assert!(
        index
            .prepare(2, 1.0, false, std::iter::empty(), |s| nodes[s as usize])
            .is_err()
    );
    let p = index
        .prepare(4, 1.0, false, std::iter::empty(), |s| nodes[s as usize])
        .unwrap();
    assert!(p.full);
    index.commit(p);
    assert_eq!(index.resolved, full(&nodes));
}

#[test]
fn inverse_users_match_full_selection_and_validate_projected_prefixes() {
    let mut properties: Vec<u32> = (0..96).map(|i| i % 14).collect();
    let mut index = ReferenceIndex::default();
    let p = index
        .prepare(96, 13, std::iter::empty(), |s| properties[s as usize])
        .unwrap();
    assert_eq!(p.checked, 96);
    index.commit(p);
    for step in 0..200 {
        let slot = step % 96;
        let value = (step * 5) % 14;
        let p = index
            .prepare(96, 13, [(slot, 13), (slot, value)].into_iter(), |s| {
                properties[s as usize]
            })
            .unwrap();
        assert_eq!(p.checked, 1);
        properties[slot as usize] = value;
        index.commit(p);
        assert_eq!(index.properties, properties);
        let affected = [step % 13, (step + 3) % 13, step % 13];
        let mut dirty = vec![];
        let count = index.affected(&affected, &mut dirty);
        let expected: Vec<_> = properties
            .iter()
            .enumerate()
            .filter(|(_, p)| **p != 0 && affected.contains(&(**p - 1)))
            .map(|(i, _)| i as u32)
            .collect();
        let actual: Vec<_> = dirty.into_iter().flatten().collect();
        assert_eq!(actual, expected);
        assert_eq!(count, expected.len());
    }
    assert!(
        index
            .prepare(96, 12, std::iter::empty(), |s| properties[s as usize])
            .is_err()
    );
    let replacements: Vec<_> = properties
        .iter()
        .enumerate()
        .filter(|(_, p)| **p == 13)
        .map(|(i, _)| (i as u32, 0))
        .collect();
    let p = index
        .prepare(96, 12, replacements.into_iter(), |s| properties[s as usize])
        .unwrap();
    assert_eq!(p.checked, 96);
    index.commit(p);
    let p = index
        .prepare(96, 12, std::iter::empty(), |_| {
            panic!("stable empty writes must use aggregate bound")
        })
        .unwrap();
    assert_eq!(p.checked, 0);
}

#[test]
fn reexposed_primitive_references_use_retained_values_and_remove_old_membership() {
    let mirror = [1, 2, 3];
    let mut index = ReferenceIndex::default();
    let plan = index
        .prepare(3, 3, std::iter::empty(), |i| mirror[i as usize])
        .unwrap();
    index.commit(plan);
    let plan = index
        .prepare(1, 1, std::iter::empty(), |i| mirror[i as usize])
        .unwrap();
    index.commit(plan);
    assert!(
        index
            .prepare(3, 1, std::iter::empty(), |i| mirror[i as usize])
            .is_err()
    );
    let plan = index
        .prepare(3, 1, [(1, 0), (2, 0)].into_iter(), |i| mirror[i as usize])
        .unwrap();
    index.commit(plan);
    let mut dirty = vec![];
    assert_eq!(index.affected(&[0, 1, 2], &mut dirty), 1);
    assert_eq!(dirty.len(), 1);
    assert_eq!(dirty[0], 0..1);
}

#[test]
fn dense_fallback_keeps_topology_and_sparse_followup_resolves_actual_ancestors() {
    let mut nodes = vec![child(0, 1); 130];
    nodes[0] = anchor(0.0);
    let mut index = GraphIndex::default();
    let plan = index
        .prepare(130, 1.0, true, std::iter::empty(), |s| nodes[s as usize])
        .unwrap();
    index.commit(plan);
    let plan = index
        .prepare(130, 1.0, true, [(0, anchor(5.0))].into_iter(), |s| {
            nodes[s as usize]
        })
        .unwrap();
    assert!(plan.dense && !plan.full);
    assert!(plan.values.is_empty());
    nodes[0] = anchor(5.0);
    index.commit(plan);
    assert!(!index.memo_valid);
    let mut leaf = nodes[1];
    leaf.layout_translation[0] = 2.0;
    let plan = index
        .prepare(130, 1.0, true, [(1, leaf)].into_iter(), |s| {
            nodes[s as usize]
        })
        .unwrap();
    assert!(!plan.dense);
    assert_eq!(plan.values[0].1.origin, [7.0, 2.0]);
    index.commit(plan);
    nodes[1] = leaf;
    let plan = index
        .prepare(
            130,
            1.0,
            true,
            [(0, anchor(f32::MAX * 0.5))].into_iter(),
            |s| nodes[s as usize],
        )
        .unwrap();
    assert!(plan.dense);
    nodes[0] = anchor(f32::MAX * 0.5);
    index.commit(plan);
    leaf.layout_translation[0] = f32::MAX * 0.75;
    assert!(
        index
            .prepare(130, 1.0, true, [(1, leaf)].into_iter(), |s| nodes
                [s as usize])
            .is_err()
    );
}
