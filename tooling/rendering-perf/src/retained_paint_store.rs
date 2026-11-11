//! Selective paint publication vs full extraction, same persistent owning tree.
use super::*;
use std::time::Instant;

fn fixture(count: usize) -> Result<(UiTree, Vec<NodeId>), Box<dyn Error>> {
    let mut tree = UiTree::new(
        Style {
            flex_wrap: taffy::FlexWrap::Wrap,
            size: taffy::Size {
                width: length(800.0),
                height: length(800.0),
            },
            ..Default::default()
        },
        0x000000ff,
    )?;
    let mut nodes = vec![];
    for _ in 0..count {
        nodes.push(tree.append(
            tree.root(),
            Style {
                size: taffy::Size {
                    width: length(8.0),
                    height: length(8.0),
                },
                flex_shrink: 0.0,
                ..Default::default()
            },
            0x4488ffff,
        )?);
    }
    Ok((tree, nodes))
}
pub(super) fn run(
    renderer: &mut WgpuFrameRenderer,
    output: &std::path::Path,
    results: &mut Vec<serde_json::Value>,
) -> Result<(), Box<dyn Error>> {
    let (mut tree, nodes) = fixture(200)?;
    let before = tree.frame([800.0; 2])?;
    renderer.set_policy(CompositorPolicy::Damage)?;
    renderer.submit(&before, [800, 800], 1.0)?;
    let mut tx = Transaction::default();
    tx.color(nodes[63], 0xff448880);
    tx.color(nodes[130], 0xaaff4480);
    tree.commit(
        tx,
        BackendCapabilities {
            solid_color_patch: true,
        },
    )?;
    let frame = tree.frame([800.0; 2])?;
    assert_eq!(
        (
            frame.build_stats.extracted_nodes,
            frame.build_stats.copied_pages,
            frame.build_stats.copied_items
        ),
        (2, 2, 128)
    );
    assert_eq!(before.items.shared_pages(&frame.items), 2);
    let submit = renderer.submit(&frame, [800, 800], 1.0)?.unwrap();
    assert!(!submit.full_resync && submit.gpu.uploaded_bytes == 320);
    compare(
        renderer,
        &frame,
        [800, 800],
        1.0,
        "paint-store-two-page-color",
        output,
        results,
    )?;
    results.last_mut().unwrap()["extracted_nodes"] = 2.into();
    results.last_mut().unwrap()["copied_items"] = 128.into();
    results.last_mut().unwrap()["shared_pages"] = 2.into();
    let mut tx = Transaction::default();
    tx.decoration(
        nodes[63],
        QuadDecoration {
            radius: 2.0,
            border_width: 1.0,
            border_color: 0xffffffff,
        },
    );
    tree.commit(tx, BackendCapabilities::default())?;
    let decorated = tree.frame([800.0; 2])?;
    assert_eq!(
        (
            decorated.build_stats.extracted_nodes,
            decorated.recorded_chunks
        ),
        (1, 1)
    );
    renderer.submit(&decorated, [800, 800], 1.0)?;
    compare(
        renderer,
        &decorated,
        [800, 800],
        1.0,
        "paint-store-decoration",
        output,
        results,
    )?;
    let mut tx = Transaction::default();
    tx.translation(tree.root(), [1.0, 1.0]);
    tree.commit(tx, BackendCapabilities::default())?;
    let geometry = tree.frame([800.0; 2])?;
    assert!(geometry.build_stats.full_rebuild);
    renderer.submit(&geometry, [800, 800], 1.0)?;
    let mut tx = Transaction::default();
    tx.color(nodes[130], 0x44aaffff);
    tree.commit(
        tx,
        BackendCapabilities {
            solid_color_patch: true,
        },
    )?;
    let moved = tree.frame([800.0; 2])?;
    assert_eq!(moved.build_stats.extracted_nodes, 1);
    renderer.submit(&moved, [800, 800], 1.0)?;
    compare(
        renderer,
        &moved,
        [800, 800],
        1.0,
        "paint-store-after-geometry",
        output,
        results,
    )?;
    Ok(())
}

pub(super) fn benchmark() -> Result<serde_json::Value, Box<dyn Error>> {
    let mut cases = vec![];
    for count in [1000, 10000] {
        for changed in [1, 128] {
            let (mut tree, nodes) = fixture(count)?;
            tree.frame([800.0; 2])?;
            let mut commit_times = [vec![], vec![]];
            let mut frame_times = [vec![], vec![]];
            let mut total_times = [vec![], vec![]];
            let mut rounds = [vec![], vec![]];
            let mut stats = [FrameBuildStats::default(); 2];
            let mut baseline = None;
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
                            tx.color(*node, if i % 2 == 0 { 0xff4488ff } else { 0x4488ffff });
                        }
                        let start = Instant::now();
                        tree.commit(
                            tx,
                            BackendCapabilities {
                                solid_color_patch: true,
                            },
                        )?;
                        let commit = start.elapsed().as_secs_f64() * 1e6;
                        let frame_start = Instant::now();
                        let frame = tree.frame([800.0; 2])?;
                        let publication = frame_start.elapsed().as_secs_f64() * 1e6;
                        let total = start.elapsed().as_secs_f64() * 1e6;
                        if i >= 6 {
                            assert_eq!(
                                (
                                    frame.layout_runs,
                                    frame.recorded_chunks,
                                    frame.updates.len()
                                ),
                                (0, 0, changed)
                            );
                            assert_eq!(
                                frame.build_stats.extracted_nodes,
                                if mode == 0 { changed } else { count + 1 }
                            );
                            commit_times[mode].push(commit);
                            frame_times[mode].push(publication);
                            total_times[mode].push(total);
                            samples.push(publication);
                            stats[mode] = frame.build_stats;
                        }
                    }
                    samples.sort_by(f64::total_cmp);
                    rounds[mode].push(samples[samples.len() / 2]);
                    let frame = tree.published_frame().unwrap();
                    let actual = frame
                        .items
                        .iter()
                        .map(|item| {
                            (
                                item.node,
                                item.bounds,
                                item.content_bounds,
                                item.clip,
                                item.fill_color,
                                item.commands.clone(),
                            )
                        })
                        .collect::<Vec<_>>();
                    if let Some(reference) = &baseline {
                        assert_eq!(&actual, reference, "selective/full published paint items");
                    } else {
                        baseline = Some(actual);
                    }
                }
            }
            for mode in 0..2 {
                commit_times[mode].sort_by(f64::total_cmp);
                frame_times[mode].sort_by(f64::total_cmp);
                total_times[mode].sort_by(f64::total_cmp);
                cases.push(serde_json::json!({"nodes":count+1,"changed_nodes":changed,"policy":if mode==0 {"selective paint pages"} else {"full render-tree extraction"},"samples":frame_times[mode].len(),"commit_median_us":commit_times[mode][commit_times[mode].len()/2],"publication_median_us":frame_times[mode][frame_times[mode].len()/2],"publication_p95_us":frame_times[mode][frame_times[mode].len()*95/100],"commit_and_frame_median_us":total_times[mode][total_times[mode].len()/2],"extracted_nodes":stats[mode].extracted_nodes,"copied_pages":stats[mode].copied_pages,"copied_items":stats[mode].copied_items,"round_publication_medians_us":rounds[mode],"publication_sorted_samples_us":frame_times[mode],"commit_sorted_samples_us":commit_times[mode],"total_sorted_samples_us":total_times[mode],"paint_items_equal":true}));
            }
        }
    }
    Ok(
        serde_json::json!({"scope":"CPU transaction and FramePacket publication on the same persistent tree, selective pages vs forced full extraction. Stable 8x8 quads, one or 128 contiguous color patches; no layout/command recording. 3 rotating rounds, 6 warmups + 20 samples per mode per round. Transaction construction, initial layout, renderer validation/planning, GPU and presentation excluded. Complete published items compared outside timing. Selective mode still copies page directory (N/64 pointers).","cases":cases}),
    )
}
