//! Same-renderer cached/full validation timing and rejection/recovery checks.
use super::*;
use std::{sync::Arc, time::Instant};

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
            0x4488ff80,
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
    let initial = tree.frame([800.0; 2])?;
    renderer.set_validation_cache_enabled(true);
    renderer.set_policy(CompositorPolicy::Damage)?;
    assert!(
        renderer
            .submit(&initial, [800, 800], 1.0)?
            .unwrap()
            .validation
            .full_scan
    );
    let mut tx = Transaction::default();
    tx.color(nodes[70], 0xff448880);
    tree.commit(
        tx,
        BackendCapabilities {
            solid_color_patch: true,
        },
    )?;
    let frame = tree.frame([800.0; 2])?;
    let mut malformed = frame.clone();
    Arc::make_mut(&mut malformed.items)[5].bounds.x = f32::NAN;
    assert!(matches!(
        renderer.submit(&malformed, [800, 800], 1.0),
        Err(gpui_box::retained::Error::InvalidFrame)
    ));
    let mut malformed = frame.clone();
    Arc::make_mut(&mut malformed.items)[5].node = nodes[0];
    assert!(matches!(
        renderer.submit(&malformed, [800, 800], 1.0),
        Err(gpui_box::retained::Error::InvalidFrame)
    ));
    let submitted = renderer.submit(&frame, [800, 800], 1.0)?.unwrap();
    assert_eq!(
        (
            submitted.validation.validated_items,
            submitted.validation.reused_pages
        ),
        (64, 3)
    );
    assert!(!submitted.validation.full_scan);
    compare(
        renderer,
        &frame,
        [800, 800],
        1.0,
        "validation-reject-recover",
        output,
        results,
    )?;
    results.last_mut().unwrap()["validated_items"] = 64.into();
    results.last_mut().unwrap()["reused_pages"] = 3.into();
    let mut tx = Transaction::default();
    tx.reorder(tree.root(), nodes.iter().rev().copied().collect());
    tree.commit(tx, BackendCapabilities::default())?;
    let frame = tree.frame([800.0; 2])?;
    assert!(
        renderer
            .submit(&frame, [800, 800], 1.0)?
            .unwrap()
            .validation
            .full_scan
    );
    compare(
        renderer,
        &frame,
        [800, 800],
        1.0,
        "validation-reorder",
        output,
        results,
    )?;
    renderer.reset()?;
    assert!(
        renderer
            .submit(&frame, [800, 800], 1.0)?
            .unwrap()
            .validation
            .full_scan
    );
    compare(
        renderer,
        &frame,
        [800, 800],
        1.0,
        "validation-reset",
        output,
        results,
    )?;
    Ok(())
}

pub(super) fn benchmark() -> Result<serde_json::Value, Box<dyn Error>> {
    let mut cases = vec![];
    for count in [1000, 10000] {
        let (mut tree, nodes) = fixture(count)?;
        let mut renderer = WgpuFrameRenderer::new(1)?;
        renderer.set_policy(CompositorPolicy::Damage)?;
        renderer.submit(&tree.frame([800.0; 2])?, [800, 800], 1.0)?;
        renderer.finish()?;
        let mut cpu = [vec![], vec![]];
        let mut completed = [vec![], vec![]];
        let mut rounds = [vec![], vec![]];
        let mut stats = [wgpu::ValidationStats::default(); 2];
        let mut pixels = None;
        for round in 0..3 {
            for mode in if round % 2 == 0 { [0, 1] } else { [1, 0] } {
                renderer.set_validation_cache_enabled(mode == 0);
                let mut samples = vec![];
                for i in 0..26 {
                    let mut tx = Transaction::default();
                    tx.color(
                        nodes[count / 2],
                        if i % 2 == 0 { 0xff448880 } else { 0x4488ff80 },
                    );
                    tree.commit(
                        tx,
                        BackendCapabilities {
                            solid_color_patch: true,
                        },
                    )?;
                    let frame = tree.frame([800.0; 2])?;
                    let start = Instant::now();
                    let submitted = renderer.submit(&frame, [800, 800], 1.0)?.unwrap();
                    let submit_us = start.elapsed().as_secs_f64() * 1e6;
                    renderer.finish()?;
                    let completed_us = start.elapsed().as_secs_f64() * 1e6;
                    assert!(!submitted.full_resync);
                    assert_eq!(submitted.gpu.uploaded_bytes, 160);
                    if i >= 6 {
                        assert_eq!(
                            submitted.validation.validated_items,
                            if mode == 0 { 64 } else { count + 1 }
                        );
                        assert_eq!(submitted.validation.full_scan, mode != 0);
                        cpu[mode].push(submit_us);
                        completed[mode].push(completed_us);
                        samples.push(submit_us);
                        stats[mode] = submitted.validation;
                    }
                }
                samples.sort_by(f64::total_cmp);
                rounds[mode].push(samples[samples.len() / 2]);
                let frame = tree.published_frame().unwrap();
                let image = renderer.image()?;
                assert_eq!(
                    image.as_raw(),
                    renderer.reference_image(frame, [800, 800], 1.0)?.as_raw()
                );
                if let Some(reference) = &pixels {
                    assert_eq!(image.as_raw(), reference);
                } else {
                    pixels = Some(image.as_raw().clone());
                }
            }
        }
        for mode in 0..2 {
            cpu[mode].sort_by(f64::total_cmp);
            completed[mode].sort_by(f64::total_cmp);
            cases.push(serde_json::json!({"nodes":count+1,"policy":if mode==0 {"immutable page reuse"} else {"forced full validation"},"samples":cpu[mode].len(),"submit_median_us":cpu[mode][cpu[mode].len()/2],"submit_p95_us":cpu[mode][cpu[mode].len()*95/100],"submit_and_completion_median_us":completed[mode][completed[mode].len()/2],"validated_items":stats[mode].validated_items,"validated_pages":stats[mode].validated_pages,"reused_pages":stats[mode].reused_pages,"effect_reference_items":stats[mode].effect_reference_items,"instance_upload_bytes":160,"pixel_equal":true,"round_submit_medians_us":rounds[mode],"submit_sorted_samples_us":cpu[mode],"completed_sorted_samples_us":completed[mode]}));
        }
    }
    Ok(
        serde_json::json!({"scope":"Same persistent tree and WGPU consumer, immutable page validation reuse vs forced complete validation. One alternating color patch, 800x800 target, Damage policy, one frame in flight. CPU submit includes remaining scene/resource/composition/backend work. Completed timing also waits for GPU. UI transaction/publication, initial setup and readbacks excluded. Three rotating rounds, six warmups and twenty samples per mode per round. Exact pixels checked against full GPUI renderer outside timing.","cases":cases}),
    )
}
