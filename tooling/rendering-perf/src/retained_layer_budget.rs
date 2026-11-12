//! Unique allocation accounting and bounded overlapping layer epochs.
use super::*;
use std::{sync::Arc, time::Instant};

fn absolute(x: f32, y: f32, width: f32, height: f32) -> Style {
    Style {
        position: taffy::Position::Absolute,
        inset: taffy::Rect {
            left: length(x),
            top: length(y),
            right: taffy::LengthPercentageAuto::auto(),
            bottom: taffy::LengthPercentageAuto::auto(),
        },
        size: taffy::Size {
            width: length(width),
            height: length(height),
        },
        ..Default::default()
    }
}
fn fixture(width: f32, height: f32, quads: usize) -> Result<(UiTree, NodeId), Box<dyn Error>> {
    let mut tree = UiTree::new(absolute(0.0, 0.0, 1400.0, 1000.0), 0x171d28ff)?;
    let group = tree.append(tree.root(), absolute(32.0, 32.0, width, height), 0)?;
    for _ in 0..quads {
        tree.append(group, absolute(0.0, 0.0, width, height), 0x4488cc40)?;
    }
    let mut tx = Transaction::default();
    tx.opacity(group, 0.6);
    tree.commit(tx, BackendCapabilities::default())?;
    Ok((tree, group))
}
fn move_group(tree: &mut UiTree, group: NodeId, step: usize) -> Result<(), Box<dyn Error>> {
    let mut tx = Transaction::default();
    tx.translation(
        group,
        if step.is_multiple_of(2) {
            [8.0, 8.0]
        } else {
            [0.0, 0.0]
        },
    );
    tree.commit(tx, BackendCapabilities::default())?;
    Ok(())
}
fn memory_json(memory: gpui_box::retained::wgpu::LayerMemoryStats) -> serde_json::Value {
    serde_json::json!({"limit_bytes":memory.limit_bytes,"live_bytes":memory.live_bytes,
        "resident_bytes":memory.resident_bytes,"retired_bytes":memory.retired_bytes,
        "peak_resident_bytes":memory.peak_resident_bytes,"resident_allocations":memory.resident_allocations})
}
pub(super) fn run(
    output: &std::path::Path,
    results: &mut Vec<serde_json::Value>,
) -> Result<(), Box<dyn Error>> {
    let mut renderer = WgpuFrameRenderer::new(3)?;
    let (mut tree, group) = fixture(48.0, 40.0, 4)?;
    renderer.submit(&tree.frame([1400.0, 1000.0])?, [1400, 1000], 1.0)?;
    let bytes = 50 * 42 * 4;
    for step in 0..24 {
        let mut tx = Transaction::default();
        tx.opacity(group, if step % 2 == 0 { 0.5 } else { 0.75 });
        tree.commit(tx, BackendCapabilities::default())?;
        let frame = tree.frame([1400.0, 1000.0])?;
        let submit = renderer.submit(&frame, [1400, 1000], 1.0)?.unwrap();
        assert_eq!(submit.gpu.layer_allocations, 0);
        assert_eq!(
            (
                submit.gpu.layer_memory.resident_bytes,
                submit.gpu.layer_memory.resident_allocations
            ),
            (bytes, 1)
        );
        assert_eq!(submit.gpu.layer_memory.retired_bytes, 0);
    }
    compare(
        &mut renderer,
        &tree.frame([1400.0, 1000.0])?,
        [1400, 1000],
        1.0,
        "layer-budget-shared-cache",
        output,
        results,
    )?;
    results.last_mut().unwrap()["memory"] = memory_json(renderer.layer_memory());
    let before = renderer.layer_memory();
    assert!(renderer.set_layer_budget_bytes(0).is_err());
    assert_eq!(renderer.layer_memory(), before);
    renderer.set_layer_budget_bytes(bytes)?;
    assert_eq!(renderer.layer_memory().resident_bytes, 0);
    renderer.submit(&tree.frame([1400.0, 1000.0])?, [1400, 1000], 1.0)?;
    let mut tx = Transaction::default();
    tx.opacity(group, 1.0);
    tree.commit(tx, BackendCapabilities::default())?;
    let frame = tree.frame([1400.0, 1000.0])?;
    let serial = renderer
        .submit(&frame, [1400, 1000], 1.0)?
        .unwrap()
        .gpu
        .serial;
    let before_collection = renderer.layer_memory();
    assert_eq!(before_collection.live_bytes, 0);
    let mut receiver = renderer.open_completions()?;
    retained_completion::collect_through(&mut renderer, &mut receiver, serial)?;
    assert_eq!(
        (
            renderer.layer_memory().resident_bytes,
            renderer.layer_memory().resident_allocations
        ),
        (0, 0)
    );
    compare(
        &mut renderer,
        &frame,
        [1400, 1000],
        1.0,
        "layer-budget-idle-retirement",
        output,
        results,
    )?;
    results.last_mut().unwrap()["before_collection"] = memory_json(before_collection);
    results.last_mut().unwrap()["after_collection"] = memory_json(renderer.layer_memory());
    renderer.reset()?;
    assert_eq!(renderer.layer_memory().limit_bytes, bytes);
    assert_eq!(renderer.layer_memory().peak_resident_bytes, 0);
    let mut tx = Transaction::default();
    tx.opacity(group, 0.6);
    tree.commit(tx, BackendCapabilities::default())?;
    let frame = tree.frame([1400.0, 1000.0])?;
    renderer.submit(&frame, [1400, 1000], 1.0)?;
    compare(
        &mut renderer,
        &frame,
        [1400, 1000],
        1.0,
        "layer-budget-reset-limit",
        output,
        results,
    )?;

    // Forced replacement control: preserve pressure coverage despite origin reuse.
    let (mut tree, group) = fixture(1200.0, 900.0, 128)?;
    let limit = 1202 * 902 * 4;
    let mut renderer = WgpuFrameRenderer::new(3)?;
    renderer.set_layer_texture_reuse_enabled(false);
    renderer.set_layer_budget_bytes(limit)?;
    renderer.submit(&tree.frame([1400.0, 1000.0])?, [1400, 1000], 1.0)?;
    let mut waits = 0;
    for step in 0..16 {
        move_group(&mut tree, group, step)?;
        let submit = renderer
            .submit(&tree.frame([1400.0, 1000.0])?, [1400, 1000], 1.0)?
            .unwrap();
        waits += submit.gpu.layer_budget_waits;
        assert_eq!(submit.gpu.layer_allocations, 1);
        assert_eq!(submit.gpu.layer_memory.live_bytes, limit);
        assert!(submit.gpu.layer_memory.peak_resident_bytes <= limit);
        assert_eq!(submit.gpu.layer_memory.resident_allocations, 1);
    }
    assert!(
        waits > 0,
        "hardware workload must exercise pressure waiting"
    );
    compare(
        &mut renderer,
        &tree.frame([1400.0, 1000.0])?,
        [1400, 1000],
        1.0,
        "layer-budget-pressure-wait",
        output,
        results,
    )?;
    results.last_mut().unwrap()["budget_waits"] = waits.into();
    results.last_mut().unwrap()["memory"] = memory_json(renderer.layer_memory());

    // Oversized new packets reject before cache mutation, and may be retried.
    let mut backend = gpui_wgpu::RetainedQuadRenderer::new(3)?;
    backend.set_layer_budget_bytes(4096)?;
    let batch = |bounds| gpui_wgpu::QuadBatch {
        capacity: 0,
        writes: vec![],
        order: Some(vec![gpui_wgpu::QuadDraw::Layer([1, 0, 1])]),
        layers: vec![gpui_wgpu::QuadLayer {
            id: [1, 0, 1],
            opacity: 0.5,
            bounds: Some(bounds),
            draws: Arc::from([]),
        }],
        images: Default::default(),
        glyphs: Default::default(),
        cache_layers: true,
        size: gpui::size(gpui::DevicePixels(64), gpui::DevicePixels(64)),
        redraw: gpui_wgpu::QuadRedraw::Full,
    };
    backend.submit(batch([0, 0, 32, 32]))?;
    let before = backend.layer_memory();
    assert!(
        backend
            .submit(batch([0, 0, 33, 32]))
            .unwrap_err()
            .to_string()
            .contains("4096 byte limit")
    );
    assert_eq!(backend.layer_memory(), before);
    let retry = backend.submit(batch([1, 0, 32, 32]))?;
    assert_eq!(retry.serial, 2);
    backend.finish()?;
    assert_eq!(backend.layer_memory().retired_bytes, 0);
    results.push(serde_json::json!({"case":"layer-budget-preflight-retry","rejected_before_mutation":true,"valid_retry_serial":retry.serial}));
    Ok(())
}

pub(super) fn benchmark() -> Result<serde_json::Value, Box<dyn Error>> {
    let mut cases = vec![];
    for quads in [4, 128] {
        let (mut tree, group) = fixture(1200.0, 900.0, quads)?;
        let mut renderer = WgpuFrameRenderer::new(3)?;
        renderer.set_layer_texture_reuse_enabled(false);
        let narrow = 1202 * 902 * 4;
        let mut rounds: [Vec<f64>; 2] = Default::default();
        let mut cpu: [Vec<f64>; 2] = Default::default();
        let mut peaks = [0; 2];
        let mut waits = [0; 2];
        let mut retired = [0; 2];
        for round in 0..3 {
            for mode in if round % 2 == 0 { [0, 1] } else { [1, 0] } {
                renderer.set_layer_budget_bytes(if mode == 0 {
                    256 * 1024 * 1024
                } else {
                    narrow
                })?;
                renderer.submit(&tree.frame([1400.0, 1000.0])?, [1400, 1000], 1.0)?;
                renderer.finish()?;
                let mut start = Instant::now();
                for step in 0..26 {
                    if step == 6 {
                        renderer.finish()?;
                        start = Instant::now();
                    }
                    let cpu_start = Instant::now();
                    move_group(&mut tree, group, step)?;
                    let submit = renderer
                        .submit(&tree.frame([1400.0, 1000.0])?, [1400, 1000], 1.0)?
                        .unwrap();
                    let us = cpu_start.elapsed().as_secs_f64() * 1e6;
                    assert!(
                        submit.gpu.layer_memory.resident_bytes
                            <= submit.gpu.layer_memory.limit_bytes
                    );
                    peaks[mode] = peaks[mode].max(submit.gpu.layer_memory.peak_resident_bytes);
                    retired[mode] = retired[mode].max(submit.gpu.layer_memory.retired_bytes);
                    if step >= 6 {
                        cpu[mode].push(us);
                        waits[mode] += submit.gpu.layer_budget_waits;
                    }
                }
                renderer.finish()?;
                rounds[mode].push(start.elapsed().as_secs_f64() * 1e6 / 20.0);
                let actual = renderer.image()?;
                let reference =
                    renderer.reference_image(&tree.frame([1400.0, 1000.0])?, [1400, 1000], 1.0)?;
                assert!(
                    actual.as_raw() == reference.as_raw(),
                    "layer budget benchmark pixels"
                );
                assert_eq!(renderer.layer_memory().resident_bytes, narrow);
            }
        }
        let value = serde_json::json!({"quads":quads,"surface":[1400,1000],"layer_payload_bytes":narrow,
            "default_round_us":rounds[0],"bounded_round_us":rounds[1],"default_cpu_samples_us":cpu[0],"bounded_cpu_samples_us":cpu[1],
            "default_peak_payload_bytes":peaks[0],"bounded_peak_payload_bytes":peaks[1],
            "default_max_retired_bytes":retired[0],"bounded_max_retired_bytes":retired[1],
            "default_budget_waits":waits[0],"bounded_budget_waits":waits[1],"pixel_equal":true});
        println!(
            "Layer budget {quads} quads: default={} bounded={} us; peak={}/{} bytes; waits={}",
            value["default_round_us"],
            value["bounded_round_us"],
            value["default_peak_payload_bytes"],
            value["bounded_peak_payload_bytes"],
            value["bounded_budget_waits"]
        );
        cases.push(value);
    }
    Ok(
        serde_json::json!({"scope":"same renderer; 256 MiB versus one-layer allocation payload budget; alternating physical origins; Full root redraw; owner commit/publication/submission plus final completion; excludes readback/native presentation; payload counters exclude driver overhead, buffers and nonlayer resources", "max_in_flight":3,"rounds":3,"warmup":6,"samples_per_round":20,"cases":cases}),
    )
}
