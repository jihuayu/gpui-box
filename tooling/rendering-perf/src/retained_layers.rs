use super::*;

pub(super) fn run(
    renderer: &mut WgpuFrameRenderer,
    output: &std::path::Path,
    results: &mut Vec<serde_json::Value>,
) -> Result<(), Box<dyn Error>> {
    renderer.set_policy(CompositorPolicy::Damage)?;
    let absolute = |x, y, width, height| Style {
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
    };
    let mut tree = UiTree::new(absolute(0.0, 0.0, 128.0, 96.0), 0x000000ff)?;
    let group = tree.append(tree.root(), absolute(8.0, 8.0, 80.0, 64.0), 0)?;
    let red = tree.append(group, absolute(0.0, 0.0, 48.0, 40.0), 0xff0000ff)?;
    let blue = tree.append(group, absolute(16.0, 8.0, 48.0, 40.0), 0x0000ffff)?;
    let mut tx = Transaction::default();
    tx.opacity(group, 0.5);
    tree.commit(tx, BackendCapabilities::default())?;
    let mut frame = tree.frame([128.0, 96.0])?;
    let submit = renderer.submit(&frame, [128, 96], 1.0)?.unwrap();
    assert_eq!(submit.gpu.rasterized_layers, 1);
    let image = renderer.image()?;
    // Independent analytical oracle: overlapping opaque siblings are resolved
    // before group alpha. Flattening alpha would leave red in the overlap.
    let overlap = image.get_pixel(28, 20).0;
    assert!(
        overlap[0] == 0 && overlap[1] == 0 && overlap[2].abs_diff(128) <= 1 && overlap[3] == 255,
        "group opacity must isolate overlapping siblings: {overlap:?}"
    );
    let red_only = image.get_pixel(12, 12).0;
    assert!(red_only[0].abs_diff(128) <= 1 && red_only[1..3] == [0, 0]);
    compare(
        renderer,
        &frame,
        [128, 96],
        1.0,
        "group-isolation",
        output,
        results,
    )?;

    let mut tx = Transaction::default();
    tx.opacity(group, 0.25);
    tree.commit(tx, BackendCapabilities::default())?;
    frame = tree.frame([128.0, 96.0])?;
    assert_eq!(
        (
            frame.layout_runs,
            frame.recorded_chunks,
            frame.updates.len()
        ),
        (0, 0, 0)
    );
    let opacity = renderer.submit(&frame, [128, 96], 1.0)?.unwrap();
    assert_eq!(
        (
            opacity.gpu.uploaded_bytes,
            opacity.gpu.property_upload_bytes,
            opacity.gpu.rasterized_layers,
            opacity.gpu.reused_layers
        ),
        (0, 16, 0, 1)
    );
    assert!(opacity.gpu.partial);
    compare(
        renderer,
        &frame,
        [128, 96],
        1.0,
        "group-opacity-cache",
        output,
        results,
    )?;

    let mut tx = Transaction::default();
    tx.color(red, 0x00ff00ff);
    tree.commit(tx, BackendCapabilities::default())?;
    frame = tree.frame([128.0, 96.0])?;
    let content = renderer.submit(&frame, [128, 96], 1.0)?.unwrap();
    assert_eq!(
        (content.gpu.uploaded_bytes, content.gpu.rasterized_layers),
        (160, 1)
    );
    compare(
        renderer,
        &frame,
        [128, 96],
        1.0,
        "group-content-invalidation",
        output,
        results,
    )?;

    let mut tx = Transaction::default();
    tx.opacity(blue, 0.5);
    tree.commit(tx, BackendCapabilities::default())?;
    frame = tree.frame([128.0, 96.0])?;
    let nested = renderer.submit(&frame, [128, 96], 1.0)?.unwrap();
    assert_eq!(nested.gpu.rasterized_layers, 2);
    let image = renderer.image()?;
    let overlap = image.get_pixel(28, 20).0;
    assert!(
        overlap[0] == 0 && overlap[1].abs_diff(32) <= 1 && overlap[2].abs_diff(32) <= 1,
        "nested isolated composition: {overlap:?}"
    );
    compare(
        renderer,
        &frame,
        [128, 96],
        1.0,
        "group-nested",
        output,
        results,
    )?;

    let mut tx = Transaction::default();
    tx.opacity(blue, 0.75);
    tree.commit(tx, BackendCapabilities::default())?;
    frame = tree.frame([128.0, 96.0])?;
    let nested = renderer.submit(&frame, [128, 96], 1.0)?.unwrap();
    assert_eq!(
        (
            nested.gpu.rasterized_layers,
            nested.gpu.reused_layers,
            nested.gpu.property_upload_bytes
        ),
        (1, 1, 16)
    );
    compare(
        renderer,
        &frame,
        [128, 96],
        1.0,
        "group-nested-opacity-invalidation",
        output,
        results,
    )?;

    for (opacity, name) in [
        (0.0, "group-zero"),
        (1.0, "group-remove-isolation"),
        (0.5, "group-restore-isolation"),
    ] {
        let mut tx = Transaction::default();
        tx.opacity(group, opacity);
        tree.commit(tx, BackendCapabilities::default())?;
        frame = tree.frame([128.0, 96.0])?;
        renderer.submit(&frame, [128, 96], 1.0)?;
        if opacity == 0.0 {
            assert_eq!(renderer.image()?.get_pixel(28, 20).0, [0, 0, 0, 255]);
        }
        compare(renderer, &frame, [128, 96], 1.0, name, output, results)?;
        if opacity == 0.0 {
            let mut tx = Transaction::default();
            tx.color(red, 0xff00ffff);
            tree.commit(tx, BackendCapabilities::default())?;
            frame = tree.frame([128.0, 96.0])?;
            renderer.submit(&frame, [128, 96], 1.0)?;
            assert_eq!(renderer.image()?.get_pixel(28, 20).0, [0, 0, 0, 255]);
            compare(
                renderer,
                &frame,
                [128, 96],
                1.0,
                "group-hidden-content",
                output,
                results,
            )?;
        }
    }
    let resized = renderer.submit(&frame, [192, 144], 1.5)?.unwrap();
    assert_eq!(resized.gpu.rasterized_layers, 2);
    compare(
        renderer,
        &frame,
        [192, 144],
        1.5,
        "group-scale",
        output,
        results,
    )?;
    let mut tx = Transaction::default();
    tx.translation(group, [7.25, 3.5]);
    tree.commit(tx, BackendCapabilities::default())?;
    frame = tree.frame([128.0, 96.0])?;
    let moved = renderer.submit(&frame, [192, 144], 1.5)?.unwrap();
    assert_eq!(moved.gpu.rasterized_layers, 2);
    compare(
        renderer,
        &frame,
        [192, 144],
        1.5,
        "group-move",
        output,
        results,
    )?;
    renderer.reset()?;
    renderer.submit(&frame, [192, 144], 1.5)?;
    compare(
        renderer,
        &frame,
        [192, 144],
        1.5,
        "group-reset",
        output,
        results,
    )?;
    for i in 0..32 {
        let mut tx = Transaction::default();
        tx.opacity(group, if i % 2 == 0 { 0.25 } else { 0.5 });
        tx.opacity(blue, if i % 3 == 0 { 0.4 } else { 0.75 });
        tree.commit(tx, BackendCapabilities::default())?;
        frame = tree.frame([128.0, 96.0])?;
        if let Some(submitted) = renderer.submit(&frame, [192, 144], 1.5)? {
            assert!(submitted.gpu.in_flight <= 3);
        }
    }
    compare(
        renderer,
        &frame,
        [192, 144],
        1.5,
        "group-in-flight",
        output,
        results,
    )?;
    let mut tx = Transaction::default();
    tx.clip(
        group,
        Some(Rect {
            x: 0.0,
            y: 0.0,
            width: 32.0,
            height: 24.0,
        }),
    );
    tree.commit(tx, BackendCapabilities::default())?;
    frame = tree.frame([128.0, 96.0])?;
    renderer.submit(&frame, [192, 144], 1.5)?;
    compare(
        renderer,
        &frame,
        [192, 144],
        1.5,
        "group-clip",
        output,
        results,
    )?;
    tree.remove(blue)?;
    frame = tree.frame([128.0, 96.0])?;
    renderer.submit(&frame, [192, 144], 1.5)?;
    compare(
        renderer,
        &frame,
        [192, 144],
        1.5,
        "group-remove-nested",
        output,
        results,
    )?;
    tree.remove(group)?;
    frame = tree.frame([128.0, 96.0])?;
    renderer.submit(&frame, [192, 144], 1.5)?;
    compare(
        renderer,
        &frame,
        [192, 144],
        1.5,
        "group-remove-subtree",
        output,
        results,
    )?;
    let mut budget_tree = UiTree::new(absolute(0.0, 0.0, 128.0, 96.0), 0x000000ff)?;
    let mut tx = Transaction::default();
    for _ in 0..9 {
        let node = budget_tree.append(
            budget_tree.root(),
            absolute(8.0, 8.0, 48.0, 40.0),
            0x4488cc80,
        )?;
        tx.opacity(node, 0.5);
    }
    budget_tree.commit(tx, BackendCapabilities::default())?;
    let budget_frame = budget_tree.frame([128.0, 96.0])?;
    let bounded = renderer.submit(&budget_frame, [3840, 2160], 1.0)?.unwrap();
    assert_eq!(bounded.gpu.live_layer_bytes, 9 * 50 * 42 * 4);
    assert_eq!(bounded.gpu.layer_allocations, 9);
    assert_eq!(bounded.gpu.rasterized_layer_pixels, 9 * 50 * 42);
    renderer.finish()?;
    renderer.set_tight_layer_bounds_enabled(false);
    assert!(
        matches!(renderer.submit(&budget_frame, [3840, 2160], 1.0), Err(gpui_box::retained::Error::Gpu(message)) if message.contains("256 MiB"))
    );
    renderer.reset()?;
    renderer.set_tight_layer_bounds_enabled(true);
    renderer.submit(&budget_frame, [128, 96], 1.0)?;
    compare(
        renderer,
        &budget_frame,
        [128, 96],
        1.0,
        "group-budget-recovery",
        output,
        results,
    )?;
    results.last_mut().unwrap()["oversized_layer_budget_rejected"] = true.into();
    Ok(())
}

pub(super) fn benchmark() -> Result<serde_json::Value, Box<dyn Error>> {
    use std::time::Instant;
    let mut cases = vec![];
    for surface in [[1920, 1080], [3840, 2160]] {
        let viewport = [surface[0] as f32, surface[1] as f32];
        let full = Style {
            position: taffy::Position::Absolute,
            size: taffy::Size {
                width: percent(1.0),
                height: percent(1.0),
            },
            ..Default::default()
        };
        let mut tree = UiTree::new(full.clone(), 0x171d28ff)?;
        let group = tree.append(tree.root(), full.clone(), 0)?;
        for _ in 0..32 {
            tree.append(group, full.clone(), 0x4488cc22)?;
        }
        let mut tx = Transaction::default();
        tx.opacity(group, 0.5);
        tree.commit(tx, BackendCapabilities::default())?;
        let mut renderer = WgpuFrameRenderer::new(3)?;
        let mut timing: [Vec<f64>; 2] = Default::default();
        let mut alternate = true;
        for round in 0..3 {
            for offset in 0..2 {
                let mode = (round + offset) % 2;
                renderer.set_raster_cache_enabled(mode == 1);
                renderer.submit(&tree.frame(viewport)?, surface, 1.0)?;
                renderer.finish()?;
                let mut start = Instant::now();
                for i in 0..48 {
                    if i == 8 {
                        renderer.finish()?;
                        start = Instant::now();
                    }
                    alternate = !alternate;
                    let mut tx = Transaction::default();
                    tx.opacity(group, if alternate { 0.5 } else { 0.75 });
                    tree.commit(tx, BackendCapabilities::default())?;
                    let frame = tree.frame(viewport)?;
                    let submit = renderer.submit(&frame, surface, 1.0)?.unwrap();
                    assert_eq!(
                        (submit.gpu.uploaded_bytes, submit.gpu.property_upload_bytes),
                        (0, 16)
                    );
                    assert_eq!(submit.gpu.rasterized_layers, usize::from(mode == 0));
                }
                renderer.finish()?;
                timing[mode].push(start.elapsed().as_secs_f64() * 1e6 / 40.0);
                let actual = renderer.image()?;
                let reference = renderer.reference_image(&tree.frame(viewport)?, surface, 1.0)?;
                assert!(
                    actual.as_raw() == reference.as_raw(),
                    "raster benchmark final pixels"
                );
            }
        }
        let mean = |mode: usize| timing[mode].iter().sum::<f64>() / 3.0;
        let value = serde_json::json!({
            "surface": surface, "uncached_mean_us": mean(0), "cached_mean_us": mean(1),
            "speedup": mean(0) / mean(1), "uncached_rounds_us": timing[0], "cached_rounds_us": timing[1],
            "instance_upload_bytes": 0, "property_upload_bytes": 16, "cached_rasterized_layers": 0,
            "pixel_equal": true,
        });
        println!("Layer cache A/B: {value}");
        cases.push(value);
    }
    Ok(serde_json::json!({
        "scope": "32 translucent full-viewport quads inside one isolated group; group-opacity animation; Full redraw for both paths; completed-work wall clock including CPU update and final GPU completion; no readback or native presentation",
        "rounds":3, "warmup":8, "samples_per_round":40, "max_in_flight":3, "cases":cases,
    }))
}
