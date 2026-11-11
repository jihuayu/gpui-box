//! Warm indexed hit queries vs the previous reverse item scan on one snapshot.
use super::*;
use std::{hint::black_box, sync::Arc, time::Instant};

fn linear(frame: &FramePacket, x: f32, y: f32) -> Option<NodeId> {
    frame
        .items
        .iter()
        .rev()
        .find(|item| {
            let b = item.visible_bounds();
            x >= b.x && y >= b.y && x < b.x + b.width && y < b.y + b.height
        })
        .map(|item| item.node)
}

pub(super) fn benchmark() -> Result<serde_json::Value, Box<dyn Error>> {
    let mut cases = vec![];
    for count in [1000, 10000] {
        for overlap in [false, true] {
            let mut tree = UiTree::new(
                Style {
                    flex_wrap: taffy::FlexWrap::Wrap,
                    size: taffy::Size {
                        width: length(800.0),
                        height: length(800.0),
                    },
                    ..Default::default()
                },
                0,
            )?;
            for _ in 0..count {
                tree.append(
                    tree.root(),
                    Style {
                        size: taffy::Size {
                            width: length(8.0),
                            height: length(8.0),
                        },
                        flex_shrink: 0.0,
                        position: if overlap {
                            taffy::Position::Absolute
                        } else {
                            taffy::Position::Relative
                        },
                        ..Default::default()
                    },
                    0,
                )?;
            }
            let frame = tree.frame([800.0; 2])?;
            let queries = (0..1000)
                .map(|i| {
                    if overlap {
                        (3.0, 3.0)
                    } else {
                        (
                            ((i * 37) % 800) as f32 + 0.25,
                            ((i * 53) % ((count / 100) * 8)) as f32 + 0.25,
                        )
                    }
                })
                .collect::<Vec<_>>();
            let start = Instant::now();
            let (_, cold_stats) = frame.hit_test_with_stats(queries[0].0, queries[0].1);
            let initial_build_us = start.elapsed().as_secs_f64() * 1e6;
            let mut bounds = 0;
            let mut items = 0;
            for (x, y) in &queries {
                let (actual, stats) = frame.hit_test_with_stats(*x, *y);
                assert_eq!(actual, linear(&frame, *x, *y));
                bounds += stats.visited_bounds;
                items += stats.tested_items;
            }
            let mut timings = [vec![], vec![]];
            let mut rounds = [vec![], vec![]];
            for round in 0..3 {
                for mode in if round % 2 == 0 { [0, 1] } else { [1, 0] } {
                    let mut round_samples = vec![];
                    for i in 0..26 {
                        let start = Instant::now();
                        for (x, y) in &queries {
                            black_box(if mode == 0 {
                                frame.hit_test(black_box(*x), black_box(*y))
                            } else {
                                linear(&frame, black_box(*x), black_box(*y))
                            });
                        }
                        let us = start.elapsed().as_secs_f64() * 1e6 / (queries.len() as f64);
                        if i >= 6 {
                            timings[mode].push(us);
                            round_samples.push(us);
                        }
                    }
                    round_samples.sort_by(f64::total_cmp);
                    rounds[mode].push(round_samples[round_samples.len() / 2]);
                }
            }
            // COW mutation invalidates only the clone. Time the ensuing lazy
            // build/query, excluding clone/page copy and preserving the warm source.
            let mut cold = vec![];
            for _ in 0..20 {
                let mut fresh = frame.clone();
                Arc::make_mut(&mut fresh.items)[0].fill_color ^= 1;
                let start = Instant::now();
                black_box(fresh.hit_test(queries[0].0, queries[0].1));
                cold.push(start.elapsed().as_secs_f64() * 1e6);
            }
            cold.sort_by(f64::total_cmp);
            for mode in 0..2 {
                timings[mode].sort_by(f64::total_cmp);
                cases.push(serde_json::json!({"nodes":count+1,"scene":if overlap {"all-overlap-top-hit"} else {"grid-distributed-hits"},"policy":if mode==0 {"persistent page BVH"} else {"reverse linear scan"},"samples":timings[mode].len(),"queries_per_sample":queries.len(),"query_median_us":timings[mode][timings[mode].len()/2],"query_p95_us":timings[mode][timings[mode].len()*95/100],"initial_build_and_query_us":initial_build_us,"rebuilt_index_query_median_us":cold[cold.len()/2],"built_items":cold_stats.built_items,"mean_index_bounds_visited":bounds as f64/queries.len() as f64,"mean_index_items_tested":items as f64/queries.len() as f64,"round_query_medians_us":rounds[mode],"query_sorted_samples_us":timings[mode],"cold_sorted_samples_us":cold,"matches_linear":true}));
            }
        }
    }
    Ok(
        serde_json::json!({"scope":"CPU hit queries on the same immutable packet. Persistent ordered page bounds BVH vs previous reverse item scan; queries preserve topmost order, clipped border bounds and half-open edges. Three rotating rounds, six warmups plus twenty measured samples per mode/round; 1000 queries per sample. Warm timing excludes construction and publication. Cold lazy build/query and visited counters included separately. No GPU/presentation/input dispatch timing.","cases":cases}),
    )
}
