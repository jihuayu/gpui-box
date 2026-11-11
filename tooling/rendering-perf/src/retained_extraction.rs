//! Property-subtree publication controls, pixel checks and isolated CPU timings.
use super::*;
use std::time::Instant;

type Branches = Vec<(NodeId, Vec<NodeId>)>;
fn fixture(groups: usize) -> Result<(UiTree, Branches), Box<dyn Error>> {
    let fixed = |w, h| Style {
        size: taffy::Size {
            width: length(w),
            height: length(h),
        },
        flex_shrink: 0.0,
        ..Default::default()
    };
    let mut tree = UiTree::new(
        Style {
            flex_wrap: taffy::FlexWrap::Wrap,
            ..fixed(800.0, 800.0)
        },
        0x000000ff,
    )?;
    let mut branches = vec![];
    for _ in 0..groups {
        let group = tree.append(tree.root(), fixed(80.0, 80.0), 0x33445580)?;
        let mut leaves = vec![];
        for _ in 0..100 {
            leaves.push(tree.append(group, fixed(8.0, 8.0), 0x4488ff80)?);
        }
        branches.push((group, leaves));
    }
    Ok((tree, branches))
}

pub(super) fn run(
    renderer: &mut WgpuFrameRenderer,
    output: &std::path::Path,
    results: &mut Vec<serde_json::Value>,
) -> Result<(), Box<dyn Error>> {
    let (mut tree, branches) = fixture(8)?;
    let initial = tree.frame([800.0; 2])?;
    renderer.set_policy(CompositorPolicy::Damage)?;
    renderer.submit(&initial, [800, 800], 1.0)?;
    for round in 0..6 {
        let mut tx = Transaction::default();
        let (group, leaves) = &branches[1];
        match round {
            0 => {
                tx.translation(*group, [3.25, 1.5]);
                tx.translation(leaves[2], [-0.5, 0.25]);
            }
            1 => {
                tx.clip(
                    *group,
                    Some(Rect {
                        x: 1.0,
                        y: 1.0,
                        width: 65.0,
                        height: 70.0,
                    }),
                );
                tx.scroll(*group, [8000.0, 8000.0]);
            }
            2 => {
                tx.opacity(*group, 0.4);
                tx.opacity(leaves[2], 0.6);
            }
            3 => {
                tx.translation(leaves[2], [2.0, 0.5]);
                tx.color(branches[6].1[1], 0xff4488ff);
            }
            4 => {
                tx.opacity(*group, 1.0);
                tx.clip(*group, None);
                tx.scroll(*group, [0.0; 2]);
            }
            _ => {
                tx.translation(*group, [0.0; 2]);
                tx.opacity(leaves[2], 1.0);
                tx.translation(leaves[2], [0.0; 2]);
            }
        }
        tree.commit(
            tx,
            BackendCapabilities {
                solid_color_patch: true,
            },
        )?;
        let frame = tree.frame([800.0; 2])?;
        let expected = if round == 3 { 2 } else { 101 };
        assert_eq!(frame.build_stats.extracted_nodes, expected);
        assert!(!frame.build_stats.full_rebuild);
        assert_eq!((frame.layout_runs, frame.recorded_chunks), (0, 0));
        renderer.submit(&frame, [800, 800], 1.0)?;
        let name = [
            "move-nested",
            "clip-scroll",
            "nested-opacity",
            "leaf-and-material",
            "remove-parent-properties",
            "restore",
        ][round];
        compare(
            renderer,
            &frame,
            [800, 800],
            1.0,
            &format!("extraction-{name}"),
            output,
            results,
        )?;
        let result = results.last_mut().unwrap();
        result["extracted_nodes"] = expected.into();
        result["copied_items"] = frame.build_stats.copied_items.into();
        result["shared_pages"] = initial.items.shared_pages(&frame.items).into();
    }
    Ok(())
}

fn median(samples: &[f64]) -> f64 {
    samples[samples.len() / 2]
}
pub(super) fn benchmark() -> Result<serde_json::Value, Box<dyn Error>> {
    let mut cases = vec![];
    for groups in [10, 100] {
        for kind in [
            "leaf-translation",
            "subtree-translation",
            "subtree-scroll",
            "subtree-opacity",
        ] {
            let (mut tree, branches) = fixture(groups)?;
            tree.frame([800.0; 2])?;
            let group = branches[groups / 2].0;
            let leaf = branches[groups / 2].1[20];
            let mut publication = [vec![], vec![]];
            let mut total = [vec![], vec![]];
            let mut rounds = [vec![], vec![]];
            let mut stats = [FrameBuildStats::default(); 2];
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
                            "leaf-translation" => {
                                tx.translation(leaf, if alternate { [1.0, 2.0] } else { [0.0; 2] })
                            }
                            "subtree-translation" => {
                                tx.translation(group, if alternate { [1.0, 2.0] } else { [0.0; 2] })
                            }
                            "subtree-scroll" => {
                                tx.scroll(group, if alternate { [4.0, 0.0] } else { [0.0; 2] })
                            }
                            _ => tx.opacity(group, if alternate { 0.4 } else { 1.0 }),
                        }
                        let start = Instant::now();
                        tree.commit(tx, BackendCapabilities::default())?;
                        let frame_start = Instant::now();
                        let frame = tree.frame([800.0; 2])?;
                        let us = frame_start.elapsed().as_secs_f64() * 1e6;
                        let total_us = start.elapsed().as_secs_f64() * 1e6;
                        if i >= 6 {
                            assert_eq!((frame.layout_runs, frame.recorded_chunks), (0, 0));
                            let extracted = if mode == 1 {
                                1 + groups * 101
                            } else if kind == "leaf-translation" {
                                1
                            } else {
                                101
                            };
                            assert_eq!(frame.build_stats.extracted_nodes, extracted);
                            publication[mode].push(us);
                            total[mode].push(total_us);
                            samples.push(us);
                            stats[mode] = frame.build_stats;
                        }
                    }
                    samples.sort_by(f64::total_cmp);
                    rounds[mode].push(median(&samples));
                    let frame = tree.published_frame().unwrap();
                    if let Some(reference) = &reference {
                        assert_eq!(*frame.properties, *reference.properties);
                        for (a, b) in frame.items.iter().zip(reference.items.iter()) {
                            assert_eq!(
                                (a.node, a.bounds, a.content_bounds, a.clip, a.properties),
                                (b.node, b.bounds, b.content_bounds, b.clip, b.properties)
                            );
                            assert_eq!(a.commands, b.commands);
                        }
                    } else {
                        reference = Some(frame.clone());
                    }
                }
            }
            for mode in 0..2 {
                publication[mode].sort_by(f64::total_cmp);
                total[mode].sort_by(f64::total_cmp);
                cases.push(serde_json::json!({"nodes":1+groups*101,"case":kind,"policy":if mode == 0 {"selective subtree"} else {"full extraction"},"samples":publication[mode].len(),"publication_median_us":median(&publication[mode]),"publication_p95_us":publication[mode][publication[mode].len()*95/100],"commit_and_frame_median_us":median(&total[mode]),"extracted_nodes":stats[mode].extracted_nodes,"copied_pages":stats[mode].copied_pages,"copied_items":stats[mode].copied_items,"round_publication_medians_us":rounds[mode],"publication_sorted_samples_us":publication[mode],"total_sorted_samples_us":total[mode],"published_items_properties_equal":true}));
            }
        }
    }
    Ok(
        serde_json::json!({"scope":"CPU transaction and FramePacket publication on the same persistent tree. Property subtree extraction vs forced full extraction; 3 rotating rounds, 6 warmups + 20 samples per mode per round. Transaction construction, initial layout, GPU consumer/planning/draw/presentation excluded. Complete final items/properties compared outside timing. Page directory and sparse property-map clones remain.","cases":cases}),
    )
}
