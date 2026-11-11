//! Persistent Taffy output driven publication vs complete extraction.
use super::*;
use std::time::Instant;
pub(super) type Branches = Vec<(NodeId, Vec<NodeId>)>;

fn fixed(width: f32, height: f32) -> Style {
    Style {
        size: taffy::Size {
            width: length(width),
            height: length(height),
        },
        flex_shrink: 0.0,
        ..Default::default()
    }
}
pub(super) fn fixture(
    groups: usize,
) -> Result<(UiTree, Branches), Box<dyn Error>> {
    let mut tree = UiTree::new(
        Style {
            flex_wrap: taffy::FlexWrap::Wrap,
            ..fixed(800.0, 800.0)
        },
        0x111111ff,
    )?;
    let mut branches = vec![];
    for _ in 0..groups {
        let parent = tree.append(tree.root(), fixed(80.0, 80.0), 0x22446680)?;
        let mut leaves = vec![];
        for _ in 0..100 {
            leaves.push(tree.append(parent, fixed(0.5, 8.0), 0x4488ff80)?);
        }
        branches.push((parent, leaves));
    }
    Ok((tree, branches))
}

pub(super) fn benchmark() -> Result<serde_json::Value, Box<dyn Error>> {
    let mut cases = vec![];
    for groups in [10, 100] {
        for kind in [
            "last-leaf-size",
            "parent-size",
            "first-leaf-dependent-movement",
            "last-leaf-intrinsic",
        ] {
            let (mut tree, branches) = fixture(groups)?;
            let (parent, leaves) = &branches[groups / 2];
            if kind == "last-leaf-intrinsic" {
                let mut tx = Transaction::default();
                tx.style(
                    leaves[99],
                    Style {
                        flex_shrink: 0.0,
                        ..Default::default()
                    },
                );
                tx.measure(
                    leaves[99],
                    Some(MeasureSnapshot::Fixed(CustomMeasureSnapshot {
                        width: 0.5,
                        height: 8.0,
                        baseline: None,
                    })),
                );
                tree.commit(tx, BackendCapabilities::default())?;
            }
            let original = tree.frame([800.0; 2])?;
            let mut times = [vec![], vec![]];
            let mut commits = [vec![], vec![]];
            let mut totals = [vec![], vec![]];
            let mut rounds = [vec![], vec![]];
            let mut build = [FrameBuildStats::default(); 2];
            let mut layout = [LayoutStats::default(); 2];
            let mut reference: Option<FramePacket> = None;
            for round in 0..3 {
                for mode in if round % 2 == 0 { [0, 1] } else { [1, 0] } {
                    tree.set_frame_build_policy(if mode == 0 {
                        FrameBuildPolicy::Selective
                    } else {
                        FrameBuildPolicy::Full
                    });
                    let mut samples = vec![];
                    for i in 0..26 {
                        let alternate = i % 2 == 0;
                        let mut tx = Transaction::default();
                        match kind {
                            "last-leaf-size" => {
                                tx.style(leaves[99], fixed(if alternate { 1.0 } else { 0.5 }, 8.0))
                            }
                            "parent-size" => {
                                tx.style(*parent, fixed(80.0, if alternate { 81.0 } else { 80.0 }))
                            }
                            "first-leaf-dependent-movement" => {
                                tx.style(leaves[0], fixed(if alternate { 1.0 } else { 0.5 }, 8.0))
                            }
                            _ => tx.measure(
                                leaves[99],
                                Some(MeasureSnapshot::Fixed(CustomMeasureSnapshot {
                                    width: if alternate { 1.0 } else { 0.5 },
                                    height: 8.0,
                                    baseline: None,
                                })),
                            ),
                        }
                        let start = Instant::now();
                        tree.commit(tx, BackendCapabilities::default())?;
                        let commit_us = start.elapsed().as_secs_f64() * 1e6;
                        let publish = Instant::now();
                        let frame = tree.frame([800.0; 2])?;
                        let us = publish.elapsed().as_secs_f64() * 1e6;
                        let total = start.elapsed().as_secs_f64() * 1e6;
                        assert_eq!((frame.layout_runs, frame.recorded_chunks), (1, 0));
                        assert_eq!(frame.build_stats.full_rebuild, mode == 1);
                        if kind == "last-leaf-size" || kind == "last-leaf-intrinsic" {
                            assert_eq!(
                                frame.build_stats.extracted_nodes,
                                if mode == 0 { 1 } else { groups * 101 + 1 }
                            );
                        }
                        if i >= 6 {
                            times[mode].push(us);
                            commits[mode].push(commit_us);
                            totals[mode].push(total);
                            samples.push(us);
                            build[mode] = frame.build_stats;
                            layout[mode] = frame.layout_stats;
                        }
                    }
                    samples.sort_by(f64::total_cmp);
                    rounds[mode].push(samples[samples.len() / 2]);
                    let frame = tree.published_frame().unwrap();
                    assert_eq!(*frame.properties, *original.properties);
                    if let Some(reference) = &reference {
                        assert_eq!(frame.damage, reference.damage);
                        assert_eq!(frame.items.len(), reference.items.len());
                        for (a, b) in frame.items.iter().zip(reference.items.iter()) {
                            assert_eq!(
                                (
                                    a.node,
                                    a.bounds,
                                    a.content_bounds,
                                    a.clip,
                                    a.properties,
                                    a.fill_color
                                ),
                                (
                                    b.node,
                                    b.bounds,
                                    b.content_bounds,
                                    b.clip,
                                    b.properties,
                                    b.fill_color
                                )
                            );
                            assert_eq!(a.commands, b.commands);
                        }
                        assert!(
                            frame
                                .updates
                                .iter()
                                .map(|i| i.node)
                                .eq(reference.updates.iter().map(|i| i.node))
                        );
                    } else {
                        reference = Some(frame.clone());
                    }
                }
            }
            for mode in 0..2 {
                times[mode].sort_by(f64::total_cmp);
                commits[mode].sort_by(f64::total_cmp);
                totals[mode].sort_by(f64::total_cmp);
                cases.push(serde_json::json!({"nodes":1+groups*101,"case":kind,"policy":if mode==0{"selective layout output"}else{"full extraction"},"samples":times[mode].len(),"layout_and_publication_median_us":times[mode][times[mode].len()/2],"layout_and_publication_p95_us":times[mode][times[mode].len()*95/100],"commit_median_us":commits[mode][commits[mode].len()/2],"total_median_us":totals[mode][totals[mode].len()/2],"extracted_nodes":build[mode].extracted_nodes,"copied_items":build[mode].copied_items,"layout_writes":layout[mode].layout_writes,"changed_layout_writes":layout[mode].changed_layout_writes,"net_changed_nodes":layout[mode].net_changed_nodes,"leaf_layouts":layout[mode].leaf_layouts,"measure_calls":layout[mode].measure_calls,"round_medians_us":rounds[mode],"publication_sorted_samples_us":times[mode],"total_sorted_samples_us":totals[mode],"final_items_properties_damage_delta_equal":true}));
            }
        }
    }
    Ok(
        serde_json::json!({"scope":"CPU persistent Taffy recomputation plus publication on the same tree; selective actual layout outputs vs complete extraction. Three rotating rounds, six warmups and twenty measured events each; initial construction/layout, transaction construction and GPU/consumer/presentation excluded. Original frame retained; final items/properties/damage/delta compared outside timing. Includes layout write observation, changed-ID deduplication, previous snapshot retirement and paint-page directory copies. Taffy layout is identical in both modes.","cases":cases}),
    )
}
