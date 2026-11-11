//! Mixed retained composition topology vs forced CPU replanning.
use super::*;
use std::time::Instant;

fn text_node(tree: &mut UiTree) -> Result<(NodeId, text::TextResource), Box<dyn Error>> {
    let (context, mut raster) = retained_text::context()?;
    let resource = retained_text::text(&context, &mut raster, "Retained 中文 composition", 1.0)?;
    let node = tree.append(
        tree.root(),
        Style {
            position: taffy::Position::Absolute,
            size: taffy::Size {
                width: length(350.0),
                height: length(40.0),
            },
            ..Default::default()
        },
        0,
    )?;
    let mut tx = Transaction::default();
    tx.text(
        node,
        Some(text::TextPaint {
            resource: resource.clone(),
            color: 0xffdd88ff,
        }),
    );
    tree.commit(tx, BackendCapabilities::default())?;
    Ok((node, resource))
}
pub(super) fn run(
    renderer: &mut WgpuFrameRenderer,
    output: &std::path::Path,
    results: &mut Vec<serde_json::Value>,
) -> Result<(), Box<dyn Error>> {
    let (mut tree, nodes, background) = retained_image_deltas::fixture(80)?;
    let (label, resource) = text_node(&mut tree)?;
    let mut tx = Transaction::default();
    tx.opacity(tree.root(), 0.8);
    tree.commit(tx, BackendCapabilities::default())?;
    renderer.set_policy(CompositorPolicy::Damage)?;
    renderer.set_composition_cache_enabled(true);
    renderer.set_resource_deltas_enabled(true);
    renderer.submit(&tree.frame([800.0, 600.0])?, [800, 600], 1.0)?;
    for step in 0..7 {
        let mut tx = Transaction::default();
        match step {
            0 => {
                tx.color(background, 0x00ff0080);
                tx.text(
                    label,
                    Some(text::TextPaint {
                        resource: resource.clone(),
                        color: 0x88ddffff,
                    }),
                );
            }
            1 => {
                tx.translation(nodes[5], [0.5, 1.0]);
                tx.translation(label, [1.0, 0.5]);
            }
            2 => tx.opacity(tree.root(), 0.5),
            3 => tx.opacity(label, 0.5),
            4 => tx.opacity(tree.root(), 1.0),
            5 => tx.text(label, None),
            _ => {
                tx.opacity(label, 1.0);
                tx.text(
                    label,
                    Some(text::TextPaint {
                        resource: resource.clone(),
                        color: 0xffdd88ff,
                    }),
                );
            }
        }
        tree.commit(
            tx,
            BackendCapabilities {
                solid_color_patch: true,
            },
        )?;
        let frame = tree.frame([800.0, 600.0])?;
        let submit = renderer.submit(&frame, [800, 600], 1.0)?.unwrap();
        assert_eq!(submit.composition.rebuilt, step >= 3);
        assert_eq!(
            submit.composition.planned_items,
            if step < 3 { 0 } else { frame.items.len() }
        );
        let name = [
            "material",
            "properties",
            "opacity",
            "add-nested-effect",
            "remove-parent-effect",
            "detach-text",
            "restore-text",
        ][step];
        compare(
            renderer,
            &frame,
            [800, 600],
            1.0,
            &format!("composition-{name}"),
            output,
            results,
        )?;
        let result = results.last_mut().unwrap();
        result["composition_rebuilt"] = submit.composition.rebuilt.into();
        result["planned_items"] = submit.composition.planned_items.into();
        result["inspected_items"] = submit.composition.inspected_items.into();
    }
    Ok(())
}
pub(super) fn benchmark() -> Result<serde_json::Value, Box<dyn Error>> {
    let mut cases = vec![];
    for count in [80, 800] {
        for kind in ["mixed-material", "group-opacity"] {
            let (mut tree, _, background) = retained_image_deltas::fixture(count)?;
            let (label, resource) = text_node(&mut tree)?;
            let mut tx = Transaction::default();
            tx.opacity(tree.root(), 0.8);
            tree.commit(tx, BackendCapabilities::default())?;
            let mut renderer = WgpuFrameRenderer::new(1)?;
            renderer.set_policy(CompositorPolicy::Damage)?;
            renderer.submit(&tree.frame([800.0, 600.0])?, [800, 600], 1.0)?;
            renderer.finish()?;
            let mut cpu = [vec![], vec![]];
            let mut complete = [vec![], vec![]];
            let mut rounds = [vec![], vec![]];
            let mut baseline = None;
            for round in 0..3 {
                for mode in if round % 2 == 0 { [0, 1] } else { [1, 0] } {
                    renderer.set_composition_cache_enabled(mode == 0);
                    let mut samples = vec![];
                    for i in 0..26 {
                        let mut tx = Transaction::default();
                        if kind == "mixed-material" {
                            tx.color(background, if i % 2 == 0 { 0x00ff0080 } else { 0xff000080 });
                            tx.text(
                                label,
                                Some(text::TextPaint {
                                    resource: resource.clone(),
                                    color: if i % 2 == 0 { 0x88ddffff } else { 0xffdd88ff },
                                }),
                            );
                        } else {
                            tx.opacity(tree.root(), if i % 2 == 0 { 0.5 } else { 0.8 });
                        }
                        tree.commit(
                            tx,
                            BackendCapabilities {
                                solid_color_patch: true,
                            },
                        )?;
                        let frame = tree.frame([800.0, 600.0])?;
                        let start = Instant::now();
                        let submit = renderer.submit(&frame, [800, 600], 1.0)?.unwrap();
                        let us = start.elapsed().as_secs_f64() * 1e6;
                        renderer.finish()?;
                        let completed = start.elapsed().as_secs_f64() * 1e6;
                        if i >= 6 {
                            assert_eq!(submit.composition.rebuilt, mode == 1);
                            assert_eq!(
                                submit.composition.planned_items,
                                if mode == 0 { 0 } else { count + 3 }
                            );
                            assert_eq!(submit.gpu.image_batch_updates, 0);
                            assert_eq!(submit.gpu.glyph_rebuilt_instances, 0);
                            assert_eq!(submit.gpu.glyph_instance_bytes, 0);
                            if kind == "group-opacity" {
                                assert_eq!(
                                    (
                                        submit.gpu.rasterized_layers,
                                        submit.gpu.property_upload_bytes
                                    ),
                                    (0, 16)
                                );
                            }
                            cpu[mode].push(us);
                            complete[mode].push(completed);
                            samples.push(us);
                        }
                    }
                    samples.sort_by(f64::total_cmp);
                    rounds[mode].push(samples[samples.len() / 2]);
                    let image = renderer.image()?;
                    let frame = tree.published_frame().unwrap();
                    assert_eq!(
                        image.as_raw(),
                        renderer.reference_image(frame, [800, 600], 1.0)?.as_raw()
                    );
                    if let Some(old) = &baseline {
                        assert_eq!(image.as_raw(), old);
                    } else {
                        baseline = Some(image.into_raw());
                    }
                }
            }
            for mode in 0..2 {
                cpu[mode].sort_by(f64::total_cmp);
                complete[mode].sort_by(f64::total_cmp);
                cases.push(serde_json::json!({"image_draws":count,"text_nodes":1,"case":kind,"policy":if mode==0 {"reuse composition topology"}else {"force full CPU plan"},"samples":cpu[mode].len(),"submit_median_us":cpu[mode][cpu[mode].len()/2],"submit_p95_us":cpu[mode][cpu[mode].len()*95/100],"submit_and_completion_median_us":complete[mode][complete[mode].len()/2],"planned_items":if mode==0 {0}else {count+3},"round_submit_medians_us":rounds[mode],"submit_sorted_samples_us":cpu[mode],"completed_sorted_samples_us":complete[mode],"pixel_equal":true}));
            }
        }
    }
    Ok(
        serde_json::json!({"scope":"Same persistent mixed image/real glyph/isolated-layer consumer; topology reuse vs forced full CPU composition plan. Damage policy,800x600,one frame in flight. 3 rotating rounds,6 warmups+20 samples/mode/round. UI publication/setup/readback/presentation excluded; CPU submit includes remaining backend layer validation, resource ID sets,draw encoding and completion leases. Completed time waits for GPU. Fresh uncached full renderer pixels match outside timing.","cases":cases}),
    )
}
