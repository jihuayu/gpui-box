//! Dense property-map scaling control, independent of GPU consumer work.
use super::*;
use std::time::Instant;

fn fixture(count: usize) -> Result<(UiTree, Vec<NodeId>), Box<dyn Error>> {
    let mut tree = UiTree::new(
        Style {
            size: taffy::Size {
                width: length(800.0),
                height: length(800.0),
            },
            flex_wrap: taffy::FlexWrap::Wrap,
            ..Default::default()
        },
        0,
    )?;
    let mut nodes = vec![];
    let mut tx = Transaction::default();
    for _ in 0..count {
        let node = tree.append(
            tree.root(),
            Style {
                size: taffy::Size {
                    width: length(8.0),
                    height: length(8.0),
                },
                flex_shrink: 0.0,
                ..Default::default()
            },
            0x4488ff80,
        )?;
        tx.translation(node, [0.125, 0.25]);
        tx.clip(
            node,
            Some(Rect {
                x: 0.0,
                y: 0.0,
                width: 8.0,
                height: 8.0,
            }),
        );
        tx.scroll(node, [1.0, 0.0]);
        tx.opacity(node, 0.8);
        nodes.push(node);
    }
    tree.commit(tx, BackendCapabilities::default())?;
    let frame = tree.frame([800.0; 2])?;
    assert_eq!(frame.properties.effects.len(), count);
    assert_eq!(frame.properties.transforms.len(), count);
    assert_eq!(frame.properties.clips.len(), count);
    assert_eq!(frame.properties.scrolls.len(), count);
    Ok((tree, nodes))
}

pub(super) fn benchmark() -> Result<serde_json::Value, Box<dyn Error>> {
    let mut cases = vec![];
    for count in [1_000, 10_000] {
        for (kind, changed) in [("translation", 1), ("opacity", 1), ("translation", 128)] {
            let (mut tree, nodes) = fixture(count)?;
            let original = tree.published_frame().unwrap().clone();
            let mut publication = [vec![], vec![]];
            let mut total = [vec![], vec![]];
            let mut rounds = [vec![], vec![]];
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
                        let mut tx = Transaction::default();
                        for node in &nodes[count / 2..count / 2 + changed] {
                            if kind == "translation" {
                                tx.translation(
                                    *node,
                                    if i % 2 == 0 {
                                        [1.25, 0.5]
                                    } else {
                                        [0.125, 0.25]
                                    },
                                );
                            } else {
                                tx.opacity(*node, if i % 2 == 0 { 0.4 } else { 0.8 });
                            }
                        }
                        let start = Instant::now();
                        tree.commit(tx, BackendCapabilities::default())?;
                        let frame_start = Instant::now();
                        let frame = tree.frame([800.0; 2])?;
                        let us = frame_start.elapsed().as_secs_f64() * 1e6;
                        let total_us = start.elapsed().as_secs_f64() * 1e6;
                        assert_eq!((frame.layout_runs, frame.recorded_chunks), (0, 0));
                        assert_eq!(
                            frame.build_stats.extracted_nodes,
                            if mode == 0 { changed } else { count + 1 }
                        );
                        if i >= 6 {
                            publication[mode].push(us);
                            total[mode].push(total_us);
                            samples.push(us);
                        }
                    }
                    samples.sort_by(f64::total_cmp);
                    rounds[mode].push(samples[samples.len() / 2]);
                    let frame = tree.published_frame().unwrap();
                    assert_eq!(*frame.properties, *original.properties);
                    if let Some(reference) = &reference {
                        for (a, b) in frame.items.iter().zip(reference.items.iter()) {
                            assert_eq!(
                                (a.node, a.bounds, a.clip, a.properties),
                                (b.node, b.bounds, b.clip, b.properties)
                            );
                        }
                        assert_eq!(frame.damage, reference.damage);
                    } else {
                        reference = Some(frame.clone());
                    }
                }
            }
            for mode in 0..2 {
                publication[mode].sort_by(f64::total_cmp);
                total[mode].sort_by(f64::total_cmp);
                cases.push(serde_json::json!({"nodes": count + 1, "property_entries": count * 4, "case":kind, "changed_nodes":changed,
                    "policy":if mode == 0 {"selective"} else {"full"}, "samples":publication[mode].len(),
                    "publication_median_us":publication[mode][publication[mode].len()/2],
                    "publication_p95_us":publication[mode][publication[mode].len()*95/100],
                    "commit_and_frame_median_us":total[mode][total[mode].len()/2],
                    "round_publication_medians_us":rounds[mode], "publication_sorted_samples_us":publication[mode],
                    "total_sorted_samples_us":total[mode], "final_items_properties_damage_equal":true}));
            }
        }
    }
    Ok(
        serde_json::json!({"scope":"CPU commit and FramePacket publication with four sparse property records per leaf; same persistent tree, selective vs forced full extraction, three rotating rounds, 6 warmups + 20 samples each. Construction, initial layout, GPU validation/planning/draw/presentation excluded. Old snapshot retained throughout; final items/properties/damage verified outside timing. Samples include previous producer snapshot retirement and page-directory copy.", "cases":cases}),
    )
}
