//! Indexed/full backend preflight, rejection retry and prefix lifetime checks.
use super::*;
use gpui_wgpu::{
    InstanceWrite, QuadBatch, QuadDraw, QuadLayer, QuadRedraw, ShadowWrite, SpatialBatch,
    SpatialNode,
};

pub(super) fn run(
    output: &std::path::Path,
    results: &mut Vec<serde_json::Value>,
) -> Result<(), Box<dyn Error>> {
    retained_routing::run_backend_controls(output, results)?;
    for indexed in [true, false] {
        preflight(indexed, results)?;
        dense_then_sparse(indexed, results)?;
        dense_permuted_overlap(indexed, results)?;
    }
    dense_layer_dependencies(results)?;
    Ok(())
}

fn dense_layer_dependencies(results: &mut Vec<serde_json::Value>) -> Result<(), Box<dyn Error>> {
    let mut frames = vec![];
    for indexed in [true, false] {
        let mut gpu = gpui_wgpu::RetainedQuadRenderer::new(3)?;
        gpu.set_indexed_spatial_enabled(indexed);
        let mut nodes = vec![
            SpatialNode {
                links: [0, 1, 0, 0],
                ..Default::default()
            };
            132
        ];
        nodes[0] = anchor(0.0);
        nodes[131] = anchor(0.0);
        let quad = gpui::Quad {
            order: 2,
            bounds: gpui::Bounds {
                origin: gpui::point(gpui::ScaledPixels(8.0), gpui::ScaledPixels(8.0)),
                size: gpui::size(gpui::ScaledPixels(16.0), gpui::ScaledPixels(16.0)),
            },
            content_mask: gpui::ContentMask {
                bounds: gpui::Bounds {
                    origin: gpui::point(gpui::ScaledPixels(0.0), gpui::ScaledPixels(0.0)),
                    size: gpui::size(gpui::ScaledPixels(64.0), gpui::ScaledPixels(64.0)),
                },
            },
            background: gpui::rgba(0xff0000ff).into(),
            ..Default::default()
        };
        let mut images = vec![];
        for step in 0..5 {
            let mut input = batch(132, vec![], vec![], vec![]);
            input.order = Some(vec![QuadDraw::Layer([91, 1, 0])]);
            input.layers = vec![QuadLayer {
                id: [91, 1, 0],
                opacity: 0.5,
                bounds: None,
                draws: std::sync::Arc::from([QuadDraw::Instances(0..1)]),
            }];
            match step {
                0 => {
                    input.spatial.writes.push(InstanceWrite {
                        start: 0,
                        values: nodes.clone(),
                    });
                    input.writes.push(InstanceWrite {
                        start: 0,
                        values: vec![quad],
                    });
                }
                1 => input.spatial.writes.push(InstanceWrite {
                    start: 0,
                    values: vec![anchor(4.0)],
                }),
                2 => {
                    // Dense overlapping transport, last writer moves the only
                    // referenced node to another anchor in the same submission.
                    nodes[1].links[0] = 131;
                    input.spatial.writes = (0..80)
                        .map(|_| InstanceWrite {
                            start: 1,
                            values: vec![nodes[1]],
                        })
                        .collect();
                }
                3 => input.spatial.writes.push(InstanceWrite {
                    start: 0,
                    values: vec![anchor(9.0)],
                }),
                _ => input.spatial.writes.push(InstanceWrite {
                    start: 131,
                    values: vec![anchor(6.0)],
                }),
            }
            let submitted = gpu.submit(input)?;
            assert_eq!(submitted.rasterized_layers, usize::from(step != 3));
            assert_eq!(submitted.reused_layers, usize::from(step == 3));
            if indexed && step == 1 {
                assert!(submitted.spatial_work.dense_fallback);
                assert_eq!(submitted.spatial_work.quad_affected_instances, 1);
            }
            if indexed && step == 3 {
                assert!(submitted.spatial_work.dense_fallback);
                assert_eq!(submitted.spatial_work.quad_affected_instances, 0);
            }
            images.push(gpu.image()?);
        }
        assert_ne!(images[0].as_raw(), images[1].as_raw());
        assert_eq!(images[0].as_raw(), images[2].as_raw());
        assert_eq!(images[2].as_raw(), images[3].as_raw());
        assert_ne!(images[3].as_raw(), images[4].as_raw());
        frames.push(images);
    }
    for (a, b) in frames[0].iter().zip(&frames[1]) {
        assert_eq!(a.as_raw(), b.as_raw());
    }
    results.push(serde_json::json!({"case":"dense-layer-final-topology-dependencies","pixel_equal":true,"dense_ancestor_invalidates":true,"overlap_reparent_invalidates":true,"unrelated_old_ancestor_reuses":true,"new_ancestor_invalidates":true,"indexed_full_control_frames":5}));
    Ok(())
}

fn dense_permuted_overlap(
    indexed: bool,
    results: &mut Vec<serde_json::Value>,
) -> Result<(), Box<dyn Error>> {
    let mut gpu = gpui_wgpu::RetainedQuadRenderer::new(3)?;
    gpu.set_indexed_spatial_enabled(indexed);
    let mut nodes: Vec<_> = (0..160)
        .map(|i| {
            if i % 32 == 31 {
                anchor(0.0)
            } else {
                SpatialNode {
                    layout_translation: [0.125, 0.25, 0.0, 0.0],
                    links: [i + 1, 31 - i % 32, 0, 0],
                    ..Default::default()
                }
            }
        })
        .collect();
    gpu.submit(batch(
        160,
        vec![InstanceWrite {
            start: 0,
            values: nodes.clone(),
        }],
        vec![],
        vec![],
    ))?;
    let mut overwritten = nodes[0];
    overwritten.links[0] = 999;
    let mut writes: Vec<_> = (0..80)
        .map(|_| InstanceWrite {
            start: 0,
            values: vec![overwritten],
        })
        .collect();
    writes.push(InstanceWrite {
        start: 0,
        values: vec![nodes[0]],
    });
    let update = gpu.submit(batch(160, writes, vec![], vec![]))?;
    if indexed {
        assert!(update.spatial_work.dense_fallback);
    }
    // Whole-prefix dense validation must reject overflow on an unreferenced
    // node, keep the committed serial/base and permit an unchanged retry.
    nodes[31] = anchor(f32::MAX * 0.75);
    nodes[30].layout_translation = [f32::MAX * 0.75, 0.0, 0.0, 0.0];
    let serial = gpu.last_serial();
    assert!(
        gpu.submit(batch(
            160,
            vec![InstanceWrite {
                start: 0,
                values: nodes
            }],
            vec![],
            vec![]
        ))
        .is_err()
    );
    assert_eq!(gpu.last_serial(), serial);
    gpu.submit(batch(160, vec![], vec![], vec![]))?;
    gpu.finish()?;
    results.push(serde_json::json!({"case":format!("spatial-dense-permuted-overlap-{indexed}"),"all_32_depths":true,"parent_after_child":true,"overwritten_invalid_edge":true,"unreferenced_overflow_rejected":true,"unchanged_retry":true}));
    Ok(())
}
fn anchor(x: f32) -> SpatialNode {
    SpatialNode {
        anchor_origin: [x, 0.0, 0.0, 0.0],
        links: [u32::MAX, 0, 2, 0],
        ..Default::default()
    }
}
fn batch(
    capacity: u32,
    nodes: Vec<InstanceWrite<SpatialNode>>,
    quads: Vec<gpui_wgpu::QuadWrite>,
    shadows: Vec<ShadowWrite>,
) -> QuadBatch {
    QuadBatch {
        capacity: 2,
        writes: quads,
        order: Some(vec![QuadDraw::Shadows(0..1), QuadDraw::Instances(0..2)]),
        layers: vec![],
        images: Default::default(),
        glyphs: Default::default(),
        shadows: gpui_wgpu::ShadowBatch {
            capacity: 1,
            writes: shadows,
        },
        spatial: SpatialBatch {
            enabled: true,
            capacity,
            scale: 1.0,
            writes: nodes,
        },
        cache_layers: true,
        size: gpui::size(gpui::DevicePixels(64), gpui::DevicePixels(64)),
        redraw: QuadRedraw::Full,
    }
}
fn preflight(indexed: bool, results: &mut Vec<serde_json::Value>) -> Result<(), Box<dyn Error>> {
    let mut gpu = gpui_wgpu::RetainedQuadRenderer::new(3)?;
    gpu.set_indexed_spatial_enabled(indexed);
    let nodes = [
        anchor(0.0),
        SpatialNode {
            layout_translation: [f32::MAX * 0.75, 0.0, 0.0, 0.0],
            links: [0, 1, 0, 0],
            ..Default::default()
        },
        anchor(8.0),
        SpatialNode {
            links: [2, 1, 0, 0],
            ..Default::default()
        },
    ];
    let bounds = gpui::Bounds {
        origin: gpui::point(gpui::ScaledPixels(8.0), gpui::ScaledPixels(8.0)),
        size: gpui::size(gpui::ScaledPixels(16.0), gpui::ScaledPixels(16.0)),
    };
    let quad = gpui::Quad {
        order: 1,
        bounds,
        content_mask: gpui::ContentMask { bounds },
        background: gpui::rgba(0xff0000ff).into(),
        ..Default::default()
    };
    let shadow = gpui::Shadow {
        order: 4,
        blur_radius: gpui::ScaledPixels(0.0),
        bounds: Default::default(),
        corner_radii: Default::default(),
        content_mask: Default::default(),
        color: Default::default(),
        element_bounds: Default::default(),
        element_corner_radii: Default::default(),
        inset: 0,
        pad: 0,
    };
    gpu.submit(batch(
        4,
        vec![InstanceWrite {
            start: 0,
            values: nodes.to_vec(),
        }],
        vec![InstanceWrite {
            start: 0,
            values: vec![quad, gpui::Quad { order: 4, ..quad }],
        }],
        vec![InstanceWrite {
            start: 0,
            values: vec![shadow],
        }],
    ))?;
    let before = gpu.image()?;
    assert_eq!(before.get_pixel(12, 12).0, [255, 0, 0, 255]);
    for kind in 0..11 {
        let mut input = batch(4, vec![], vec![], vec![]);
        match kind {
            0 => {
                let mut n = nodes[0];
                n.links = [2, 1, 0, 0];
                input.spatial.writes.push(InstanceWrite {
                    start: 0,
                    values: vec![n],
                });
            }
            1 => {
                let mut n = nodes[1];
                n.links = [3, 1, 0, 0];
                input.spatial.writes.push(InstanceWrite {
                    start: 1,
                    values: vec![n],
                });
            }
            2 => input.spatial.writes.push(InstanceWrite {
                start: 0,
                values: vec![anchor(f32::MAX * 0.5)],
            }),
            3 => input.spatial.scale = 2.0,
            4 => input.spatial.capacity = 1,
            5 => input.writes.push(InstanceWrite {
                start: 0,
                values: vec![gpui::Quad { order: 5, ..quad }],
            }),
            6 => input.shadows.writes.push(InstanceWrite {
                start: 0,
                values: vec![gpui::Shadow { order: 5, ..shadow }],
            }),
            7 => {
                input.spatial.writes.push(InstanceWrite {
                    start: 2,
                    values: vec![anchor(32.0)],
                });
                input.layers.push(QuadLayer {
                    id: [1, 2, 3],
                    opacity: 2.0,
                    bounds: None,
                    draws: std::sync::Arc::from([]),
                });
            }
            8 => {
                input.spatial.writes.push(InstanceWrite {
                    start: 2,
                    values: vec![anchor(32.0)],
                });
                input.redraw = QuadRedraw::Damage {
                    x: 0,
                    y: 0,
                    width: 0,
                    height: 1,
                };
            }
            9 => {
                input.writes.push(InstanceWrite {
                    start: 0,
                    values: vec![gpui::Quad { order: 2, ..quad }],
                });
                input.redraw = QuadRedraw::Damage {
                    x: 0,
                    y: 0,
                    width: 0,
                    height: 1,
                };
            }
            _ => {
                let mut n = nodes[2];
                n.clip[0] = f32::NAN;
                input.spatial.writes.push(InstanceWrite {
                    start: 2,
                    values: vec![n],
                });
            }
        }
        assert!(
            gpu.submit(input).is_err(),
            "indexed={indexed}, malformed={kind}"
        );
        assert_eq!(gpu.last_serial(), 1);
    }
    let retry = gpu.submit(batch(4, vec![], vec![], vec![]))?;
    assert_eq!(retry.serial, 2);
    assert_eq!(retry.spatial_upload_bytes, 0);
    if indexed {
        assert_eq!(
            (
                retry.spatial_work.resolved_nodes,
                retry.spatial_work.quad_references,
                retry.spatial_work.shadow_references
            ),
            (0, 0, 0)
        );
    }
    assert_eq!(before.as_raw(), gpu.image()?.as_raw());
    // Prefix shrink is legal when references are replaced in the same batch.
    let shrink = gpu.submit(batch(
        1,
        vec![],
        vec![InstanceWrite {
            start: 0,
            values: vec![gpui::Quad { order: 0, ..quad }; 2],
        }],
        vec![InstanceWrite {
            start: 0,
            values: vec![gpui::Shadow { order: 0, ..shadow }],
        }],
    ))?;
    if indexed {
        assert!(shrink.spatial_work.full_spatial_scan);
    }
    // Retained values outside the last logical prefix reappear on growth.
    let grow = gpu.submit(batch(2, vec![], vec![], vec![]))?;
    if indexed {
        assert!(grow.spatial_work.full_spatial_scan);
        assert_eq!(grow.spatial_work.resolved_nodes, 2);
    }
    let mut scaled = batch(2, vec![], vec![], vec![]);
    scaled.spatial.scale = 2.0;
    assert!(gpu.submit(scaled).is_err());
    gpu.submit(batch(4, vec![], vec![], vec![]))?;
    // Final-write projection matches actual overlapping storage writes.
    let overlap = gpu.submit(batch(
        4,
        vec![
            InstanceWrite {
                start: 2,
                values: vec![SpatialNode {
                    links: [99, 1, 0, 0],
                    ..Default::default()
                }],
            },
            InstanceWrite {
                start: 2,
                values: vec![anchor(12.0)],
            },
        ],
        vec![],
        vec![],
    ))?;
    if indexed {
        assert_eq!(overlap.spatial_work.resolved_nodes, 2);
        assert_eq!(overlap.spatial_work.quad_affected_instances, 0);
    }
    gpu.finish()?;
    results.push(serde_json::json!({"case":format!("spatial-index-backend-preflight-{indexed}"),"malformed_rejected":12,"late_rejection_keeps_base":true,"unchanged_retry_pixels":true,"unreferenced_descendant_overflow":true,"prefix_shrink_grow_uses_retained_mirror":true,"overlapping_last_write_wins":true,"analytical_pixel":true}));
    Ok(())
}

fn dense_then_sparse(
    indexed: bool,
    results: &mut Vec<serde_json::Value>,
) -> Result<(), Box<dyn Error>> {
    let mut gpu = gpui_wgpu::RetainedQuadRenderer::new(3)?;
    gpu.set_indexed_spatial_enabled(indexed);
    let mut nodes = vec![
        SpatialNode {
            links: [0, 1, 0, 0],
            ..Default::default()
        };
        132
    ];
    nodes[0] = anchor(0.0);
    nodes[131] = anchor(0.0);
    gpu.submit(batch(
        132,
        vec![InstanceWrite {
            start: 0,
            values: nodes,
        }],
        vec![],
        vec![],
    ))?;
    let mut invalid = batch(
        132,
        vec![InstanceWrite {
            start: 0,
            values: vec![SpatialNode {
                links: [131, 1, 0, 0],
                ..Default::default()
            }],
        }],
        vec![],
        vec![],
    );
    assert!(gpu.submit(invalid).is_err());
    assert_eq!(gpu.last_serial(), 1);
    let update = gpu.submit(batch(
        132,
        vec![InstanceWrite {
            start: 0,
            values: vec![anchor(f32::MAX * 0.5)],
        }],
        vec![],
        vec![],
    ))?;
    if indexed {
        assert!(update.spatial_work.dense_fallback);
    }
    invalid = batch(
        132,
        vec![InstanceWrite {
            start: 1,
            values: vec![SpatialNode {
                layout_translation: [f32::MAX * 0.75, 0.0, 0.0, 0.0],
                links: [0, 1, 0, 0],
                ..Default::default()
            }],
        }],
        vec![],
        vec![],
    );
    assert!(gpu.submit(invalid).is_err());
    assert_eq!(gpu.last_serial(), 2);
    let retry = gpu.submit(batch(132, vec![], vec![], vec![]))?;
    if indexed {
        assert_eq!(retry.spatial_work.resolved_nodes, 0);
    }
    results.push(serde_json::json!({"case":format!("spatial-index-dense-sparse-{indexed}"),"dense_child_edge_rejected":true,"sparse_uses_actual_ancestor_after_dense":true,"overflow_rejected":true,"valid_retry":true}));
    Ok(())
}
