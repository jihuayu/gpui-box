//! Real font masks → atlas → batched glyph draws; independent CPU blend oracle.
use super::*;
use gpui_box::retained::{paragraph::ParagraphSnapshot, text::*};
use std::{borrow::Cow, sync::Arc, time::Instant};
fn context() -> Result<(gpui::WindowTextSystem, GlyphRasterizer), Box<dyn Error>> {
    let system = Arc::new(gpui::TextSystem::new(Arc::new(
        gpui_wgpu::CosmicTextSystem::new_without_system_fonts("Noto Sans CJK SC"),
    )));
    system.add_fonts(vec![Cow::Borrowed(include_bytes!(
        "../../../examples/hello/assets/NotoSansSC-Regular.otf"
    ))])?;
    Ok((
        gpui::WindowTextSystem::new(system.clone()),
        GlyphRasterizer::new(system),
    ))
}
fn text(
    context: &gpui::WindowTextSystem,
    raster: &mut GlyphRasterizer,
    value: &str,
    scale: f32,
) -> Result<TextResource, Box<dyn Error>> {
    Ok(raster.prepare(
        ParagraphSnapshot::shape(context, value, gpui::font("Noto Sans CJK SC"), 20.0, 36.0)?,
        scale,
    )?)
}
fn style(x: f32, y: f32, width: f32, height: Option<f32>) -> Style {
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
            height: height.map_or(taffy::Dimension::auto(), length),
        },
        ..Default::default()
    }
}
fn set(
    tree: &mut UiTree,
    node: NodeId,
    resource: &TextResource,
    color: u32,
) -> Result<Commit, Box<dyn Error>> {
    let mut tx = Transaction::default();
    tx.text(
        node,
        Some(TextPaint {
            resource: resource.clone(),
            color,
        }),
    );
    Ok(tree.commit(tx, BackendCapabilities::default())?)
}
fn cpu_oracle(
    frame: &FramePacket,
    surface: [u32; 2],
    scale: f32,
    actual: &gpui_wgpu::RetainedImage,
) {
    assert!(frame.properties.effects.is_empty());
    let mut pixels = vec![[0u8, 0, 0, 255]; (surface[0] * surface[1]) as usize];
    let blend = |pixel: &mut [u8; 4], color: u32, coverage: f32| {
        let alpha = (color & 255) as f32 / 255.0 * coverage;
        for (channel, shift) in [24, 16, 8].into_iter().enumerate() {
            pixel[channel] = (((color >> shift) & 255) as f32 * alpha
                + pixel[channel] as f32 * (1.0 - alpha))
                .round()
                .clamp(0.0, 255.0) as u8;
        }
    };
    let contains = |rect: Rect, x: f32, y: f32| {
        x >= rect.x && y >= rect.y && x < rect.x + rect.width && y < rect.y + rect.height
    };
    for item in frame.items.iter() {
        for y in 0..surface[1] {
            for x in 0..surface[0] {
                let point = [(x as f32 + 0.5) / scale, (y as f32 + 0.5) / scale];
                if item
                    .clip
                    .is_some_and(|clip| !contains(clip, point[0], point[1]))
                {
                    continue;
                }
                if contains(item.bounds, point[0], point[1]) {
                    blend(
                        &mut pixels[(y * surface[0] + x) as usize],
                        item.fill_color,
                        1.0,
                    );
                }
            }
        }
        if let (Some(text), Some(chunk)) = (&item.text, &item.glyphs) {
            for glyph in chunk.glyphs.iter() {
                let bounds = item.glyph_bounds(glyph, scale);
                for y in 0..glyph.bitmap.size[1] {
                    for x in 0..glyph.bitmap.size[0] {
                        let px = bounds.x as i32 + x as i32;
                        let py = bounds.y as i32 + y as i32;
                        if px < 0 || py < 0 || px >= surface[0] as i32 || py >= surface[1] as i32 {
                            continue;
                        }
                        if item.clip.is_some_and(|clip| {
                            !contains(clip, (px as f32 + 0.5) / scale, (py as f32 + 0.5) / scale)
                        }) {
                            continue;
                        }
                        blend(
                            &mut pixels[py as usize * surface[0] as usize + px as usize],
                            text.color,
                            glyph.bitmap.mask()[(y * glyph.bitmap.size[0] + x) as usize] as f32
                                / 255.0,
                        );
                    }
                }
            }
        }
    }
    let mut non_background = 0;
    for (expected, actual) in pixels.iter().zip(actual.pixels()) {
        assert!(
            expected
                .iter()
                .zip(actual.0)
                .all(|(a, b)| a.abs_diff(b) <= 2),
            "CPU glyph oracle differs: {expected:?} vs {:?}",
            actual.0
        );
        non_background += usize::from(expected[..3] != [0, 0, 0]);
    }
    assert!(non_background > 50, "font mask must produce actual ink");
}
pub(super) fn run(
    renderer: &mut WgpuFrameRenderer,
    output: &std::path::Path,
    results: &mut Vec<serde_json::Value>,
) -> Result<(), Box<dyn Error>> {
    let (context, mut raster) = context()?;
    let a = text(
        &context,
        &mut raster,
        "office a\u{301} 中文测试 Retained glyphs\nsecond line",
        1.0,
    )?;
    let count = raster.rasterized_glyphs();
    let same = text(
        &context,
        &mut raster,
        "office a\u{301} 中文测试 Retained glyphs\nsecond line",
        1.0,
    )?;
    assert_eq!(
        raster.rasterized_glyphs(),
        count,
        "second paragraph shares glyph resources"
    );
    renderer.set_policy(CompositorPolicy::Damage)?;
    let mut tree = UiTree::new(style(0.0, 0.0, 400.0, Some(220.0)), 0x000000ff)?;
    let node = tree.append(tree.root(), style(8.0, 2.0, 220.0, None), 0)?;
    set(&mut tree, node, &a, 0xffdd88ff)?;
    let mut frame = tree.frame([400.0, 220.0])?;
    let initial = renderer.submit(&frame, [400, 220], 1.0)?.unwrap();
    assert!(initial.gpu.glyph_atlas_bytes > 0 && initial.gpu.glyph_atlas_uploads > 0);
    assert!(
        initial.gpu.draw_calls <= 4,
        "one atlas page batches the paragraph glyphs"
    );
    cpu_oracle(&frame, [400, 220], 1.0, &renderer.image()?);
    compare(
        renderer,
        &frame,
        [400, 220],
        1.0,
        "text-initial-unicode",
        output,
        results,
    )?;
    results.last_mut().unwrap()["glyph_atlas_bytes"] = initial.gpu.glyph_atlas_bytes.into();
    results.last_mut().unwrap()["draw_calls"] = initial.gpu.draw_calls.into();
    let before = frame.clone();
    set(&mut tree, node, &a, 0x66ccff80)?;
    frame = tree.frame([400.0, 220.0])?;
    assert_eq!((frame.layout_runs, frame.recorded_chunks), (0, 0));
    assert!(Arc::ptr_eq(
        before.items[1].glyphs.as_ref().unwrap(),
        frame.items[1].glyphs.as_ref().unwrap()
    ));
    let color = renderer.submit(&frame, [400, 220], 1.0)?.unwrap();
    assert_eq!(color.gpu.glyph_atlas_bytes, 0);
    assert_eq!(
        (
            color.gpu.glyph_instance_bytes,
            color.gpu.glyph_upload_ranges
        ),
        (0, 0)
    );
    assert_eq!(
        (
            color.gpu.glyph_material_bytes,
            color.gpu.glyph_material_ranges
        ),
        (16, 1)
    );
    cpu_oracle(&frame, [400, 220], 1.0, &renderer.image()?);
    compare(
        renderer,
        &frame,
        [400, 220],
        1.0,
        "text-color-material",
        output,
        results,
    )?;
    results.last_mut().unwrap()["glyph_material_bytes"] = color.gpu.glyph_material_bytes.into();
    results.last_mut().unwrap()["glyph_instance_bytes"] = color.gpu.glyph_instance_bytes.into();
    let mut tx = Transaction::default();
    tx.translation(node, [4.25, 3.25]);
    tree.commit(tx, BackendCapabilities::default())?;
    frame = tree.frame([400.0, 220.0])?;
    let moved = renderer.submit(&frame, [400, 220], 1.0)?.unwrap();
    assert_eq!(moved.gpu.glyph_atlas_bytes, 0);
    assert_eq!(
        (
            moved.gpu.glyph_instance_bytes,
            moved.gpu.glyph_material_bytes,
            moved.gpu.glyph_property_bytes
        ),
        (0, 0, 32)
    );
    cpu_oracle(&frame, [400, 220], 1.0, &renderer.image()?);
    compare(
        renderer,
        &frame,
        [400, 220],
        1.0,
        "text-translation-damage",
        output,
        results,
    )?;
    let mut tx = Transaction::default();
    tx.clip(
        node,
        Some(Rect {
            x: 0.0,
            y: 0.0,
            width: 96.0,
            height: 45.0,
        }),
    );
    tree.commit(tx, BackendCapabilities::default())?;
    frame = tree.frame([400.0, 220.0])?;
    let clipped = renderer.submit(&frame, [400, 220], 1.0)?.unwrap();
    assert_eq!(
        (
            clipped.gpu.glyph_instance_bytes,
            clipped.gpu.glyph_property_bytes
        ),
        (0, 32)
    );
    cpu_oracle(&frame, [400, 220], 1.0, &renderer.image()?);
    compare(
        renderer,
        &frame,
        [400, 220],
        1.0,
        "text-clip",
        output,
        results,
    )?;
    let mut tx = Transaction::default();
    tx.clip(node, None);
    tx.style(node, style(8.0, 2.0, 100.0, None));
    tree.commit(tx, BackendCapabilities::default())?;
    frame = tree.frame([400.0, 220.0])?;
    let wrap = renderer.submit(&frame, [400, 220], 1.0)?.unwrap();
    assert_eq!(wrap.gpu.glyph_atlas_bytes, 0);
    assert_eq!(frame.layout_runs, 1);
    cpu_oracle(&frame, [400, 220], 1.0, &renderer.image()?);
    compare(
        renderer,
        &frame,
        [400, 220],
        1.0,
        "text-rewrap",
        output,
        results,
    )?;
    let other = tree.append(tree.root(), style(240.0, 2.0, 150.0, None), 0)?;
    set(&mut tree, other, &same, 0xcc88ffff)?;
    frame = tree.frame([400.0, 220.0])?;
    assert_eq!(
        renderer
            .submit(&frame, [400, 220], 1.0)?
            .unwrap()
            .gpu
            .glyph_atlas_bytes,
        0
    );
    cpu_oracle(&frame, [400, 220], 1.0, &renderer.image()?);
    compare(
        renderer,
        &frame,
        [400, 220],
        1.0,
        "text-shared-atlas",
        output,
        results,
    )?;
    let mut tx = Transaction::default();
    tx.opacity(node, 0.5);
    tree.commit(tx, BackendCapabilities::default())?;
    frame = tree.frame([400.0, 220.0])?;
    renderer.submit(&frame, [400, 220], 1.0)?;
    compare(
        renderer,
        &frame,
        [400, 220],
        1.0,
        "text-isolated-opacity",
        output,
        results,
    )?;
    let mut tx = Transaction::default();
    tx.opacity(node, 0.25);
    tree.commit(tx, BackendCapabilities::default())?;
    frame = tree.frame([400.0, 220.0])?;
    let opacity = renderer.submit(&frame, [400, 220], 1.0)?.unwrap();
    assert_eq!(
        (
            opacity.gpu.glyph_atlas_bytes,
            opacity.gpu.glyph_instance_bytes,
            opacity.gpu.glyph_material_bytes,
            opacity.gpu.glyph_property_bytes,
            opacity.gpu.rasterized_layers
        ),
        (0, 0, 0, 0, 0)
    );
    compare(
        renderer,
        &frame,
        [400, 220],
        1.0,
        "text-opacity-cache",
        output,
        results,
    )?;
    set(&mut tree, node, &a, 0x55ffaaff)?;
    frame = tree.frame([400.0, 220.0])?;
    let layer_color = renderer.submit(&frame, [400, 220], 1.0)?.unwrap();
    assert_eq!(
        (
            layer_color.gpu.rasterized_layers,
            layer_color.gpu.glyph_instance_bytes,
            layer_color.gpu.glyph_material_bytes
        ),
        (1, 0, 16)
    );
    compare(
        renderer,
        &frame,
        [400, 220],
        1.0,
        "text-material-invalidates-layer",
        output,
        results,
    )?;
    let mut tx = Transaction::default();
    tx.translation(node, [4.5, 4.5]);
    tree.commit(tx, BackendCapabilities::default())?;
    frame = tree.frame([400.0, 220.0])?;
    let property_layer = renderer.submit(&frame, [400, 220], 1.0)?.unwrap();
    assert_eq!(
        (
            property_layer.gpu.rasterized_layers,
            property_layer.gpu.glyph_instance_bytes,
            property_layer.gpu.glyph_property_bytes
        ),
        (1, 0, 32)
    );
    compare(
        renderer,
        &frame,
        [400, 220],
        1.0,
        "text-property-invalidates-layer",
        output,
        results,
    )?;
    let b = text(&context, &mut raster, "changed 中文内容", 1.0)?;
    set(&mut tree, node, &b, 0xeeaa44ff)?;
    frame = tree.frame([400.0, 220.0])?;
    let replaced = renderer.submit(&frame, [400, 220], 1.0)?.unwrap();
    assert_eq!(replaced.gpu.rasterized_layers, 1);
    compare(
        renderer,
        &frame,
        [400, 220],
        1.0,
        "text-resource-replacement",
        output,
        results,
    )?;
    let old = frame.clone();
    let mut tx = Transaction::default();
    tx.text(node, None);
    tree.commit(tx, BackendCapabilities::default())?;
    tree.frame([400.0, 220.0])?;
    set(&mut tree, node, &a, 0xffffffff)?;
    frame = tree.frame([400.0, 220.0])?;
    assert!(
        renderer
            .submit(&frame, [400, 220], 1.0)?
            .unwrap()
            .full_resync
    );
    compare(
        renderer,
        &frame,
        [400, 220],
        1.0,
        "text-skipped-base",
        output,
        results,
    )?;
    renderer.reset()?;
    assert!(
        renderer
            .submit(&frame, [400, 220], 1.0)?
            .unwrap()
            .gpu
            .glyph_atlas_bytes
            > 0
    );
    compare(
        renderer,
        &frame,
        [400, 220],
        1.0,
        "text-device-reset",
        output,
        results,
    )?;
    let scaled = text(&context, &mut raster, "changed 中文内容", 1.5)?;
    let scaled_other = text(
        &context,
        &mut raster,
        "office a\u{301} 中文测试 Retained glyphs\nsecond line",
        1.5,
    )?;
    set(&mut tree, node, &scaled, 0xffffffff)?;
    set(&mut tree, other, &scaled_other, 0xcc88ffff)?;
    let mut tx = Transaction::default();
    tx.opacity(node, 1.0);
    tree.commit(tx, BackendCapabilities::default())?;
    frame = tree.frame([400.0, 220.0])?;
    assert!(
        renderer.submit(&frame, [400, 220], 1.0).is_err(),
        "scale mismatch rejected before submission"
    );
    renderer.submit(&frame, [600, 330], 1.5)?;
    cpu_oracle(&frame, [600, 330], 1.5, &renderer.image()?);
    compare(
        renderer,
        &frame,
        [600, 330],
        1.5,
        "text-device-scale",
        output,
        results,
    )?;
    let cover = tree.append(tree.root(), style(8.0, 2.0, 30.0, Some(20.0)), 0xdd88aaff)?;
    frame = tree.frame([400.0, 220.0])?;
    renderer.submit(&frame, [600, 330], 1.5)?;
    cpu_oracle(&frame, [600, 330], 1.5, &renderer.image()?);
    compare(
        renderer,
        &frame,
        [600, 330],
        1.5,
        "text-mixed-paint-order",
        output,
        results,
    )?;
    tree.remove(cover)?;
    let owned = text(&context, &mut raster, "unique lifetime 字形", 1.5)?;
    set(&mut tree, node, &owned, 0xffffffff)?;
    let leased = tree.frame([400.0, 220.0])?;
    let weak = Arc::downgrade(
        leased
            .items
            .iter()
            .find(|item| item.node == node)
            .unwrap()
            .glyphs
            .as_ref()
            .unwrap(),
    );
    renderer.submit(&leased, [600, 330], 1.5)?;
    tree.remove(node)?;
    frame = tree.frame([400.0, 220.0])?;
    renderer.submit(&frame, [600, 330], 1.5)?;
    drop(leased);
    drop(owned);
    drop(old);
    renderer.finish()?;
    assert!(weak.upgrade().is_none());
    compare(
        renderer,
        &frame,
        [600, 330],
        1.5,
        "text-removal-retirement",
        output,
        results,
    )?;
    // No readback between replacements: old page/buffer leases and queue order
    // must protect already submitted frames while dead tiles are reused.
    for i in 0..12 {
        let resource = text(&context, &mut raster, &format!("in flight 中文 {i}"), 1.5)?;
        set(&mut tree, other, &resource, 0xffdd88ff)?;
        frame = tree.frame([400.0, 220.0])?;
        let submit = renderer.submit(&frame, [600, 330], 1.5)?.unwrap();
        assert!(submit.gpu.in_flight <= 3);
    }
    renderer.finish()?;
    cpu_oracle(&frame, [600, 330], 1.5, &renderer.image()?);
    compare(
        renderer,
        &frame,
        [600, 330],
        1.5,
        "text-in-flight-replacement",
        output,
        results,
    )?;

    let mut large = UiTree::new(style(0.0, 0.0, 800.0, Some(600.0)), 0x000000ff)?;
    let large_node = large.append(large.root(), style(0.0, 0.0, 780.0, None), 0)?;
    let large_resource = raster.prepare(
        ParagraphSnapshot::shape(
            &context,
            "天地玄黄宇宙洪荒日月盈昃辰宿列张寒来暑往秋收冬藏闰余成岁律吕调阳",
            gpui::font("Noto Sans CJK SC"),
            300.0,
            480.0,
        )?,
        1.0,
    )?;
    set(&mut large, large_node, &large_resource, 0xdd88ffff)?;
    let large_frame = large.frame([800.0, 600.0])?;
    let pages = renderer.submit(&large_frame, [800, 600], 1.0)?.unwrap();
    assert!(pages.gpu.glyph_atlas_bytes > 1024 * 1024);
    assert!(
        pages.gpu.draw_calls > 3,
        "multiple atlas page runs must preserve glyph order"
    );
    cpu_oracle(&large_frame, [800, 600], 1.0, &renderer.image()?);
    compare(
        renderer,
        &large_frame,
        [800, 600],
        1.0,
        "text-multiple-atlas-pages",
        output,
        results,
    )?;

    let short = text(&context, &mut raster, "ABCDEFGH", 1.0)?;
    let mut batch = UiTree::new(style(0.0, 0.0, 400.0, Some(220.0)), 0x000000ff)?;
    let mut nodes = vec![];
    for y in 0..5 {
        let node = batch.append(batch.root(), style(4.0, y as f32 * 40.0, 350.0, None), 0)?;
        set(&mut batch, node, &short, 0xffdd88ff)?;
        nodes.push(node);
    }
    renderer.submit(&batch.frame([400.0, 220.0])?, [400, 220], 1.0)?;
    let mut tx = Transaction::default();
    for node in &nodes {
        tx.text(
            *node,
            Some(TextPaint {
                resource: short.clone(),
                color: 0x88ddffff,
            }),
        );
    }
    batch.commit(tx, BackendCapabilities::default())?;
    let batch_frame = batch.frame([400.0, 220.0])?;
    let batched = renderer.submit(&batch_frame, [400, 220], 1.0)?.unwrap();
    assert_eq!(
        (
            batched.gpu.glyph_atlas_bytes,
            batched.gpu.glyph_instance_bytes,
            batched.gpu.glyph_material_bytes,
            batched.gpu.glyph_material_ranges
        ),
        (0, 0, 5 * 16, 1)
    );
    cpu_oracle(&batch_frame, [400, 220], 1.0, &renderer.image()?);
    compare(
        renderer,
        &batch_frame,
        [400, 220],
        1.0,
        "text-batched-color-merge",
        output,
        results,
    )?;
    results.last_mut().unwrap()["glyph_material_bytes"] = batched.gpu.glyph_material_bytes.into();
    results.last_mut().unwrap()["glyph_material_ranges"] = batched.gpu.glyph_material_ranges.into();
    results.last_mut().unwrap()["glyph_instance_bytes"] = batched.gpu.glyph_instance_bytes.into();
    let mut tx = Transaction::default();
    for node in &nodes {
        tx.translation(*node, [0.5, 0.5]);
    }
    batch.commit(tx, BackendCapabilities::default())?;
    let property_frame = batch.frame([400.0, 220.0])?;
    let properties = renderer.submit(&property_frame, [400, 220], 1.0)?.unwrap();
    assert_eq!(
        (
            properties.gpu.glyph_atlas_bytes,
            properties.gpu.glyph_instance_bytes,
            properties.gpu.glyph_material_bytes,
            properties.gpu.glyph_property_bytes,
            properties.gpu.glyph_property_ranges
        ),
        (0, 0, 0, 160, 1)
    );
    cpu_oracle(&property_frame, [400, 220], 1.0, &renderer.image()?);
    compare(
        renderer,
        &property_frame,
        [400, 220],
        1.0,
        "text-batched-property-merge",
        output,
        results,
    )?;
    results.last_mut().unwrap()["glyph_property_bytes"] = 160.into();
    results.last_mut().unwrap()["glyph_property_ranges"] = 1.into();
    for node in nodes {
        batch.remove(node)?;
    }
    renderer.submit(&batch.frame([400.0, 220.0])?, [400, 220], 1.0)?;
    let node = batch.append(batch.root(), style(8.0, 4.0, 350.0, None), 0)?;
    set(&mut batch, node, &short, 0xff4488ff)?;
    let reused = batch.frame([400.0, 220.0])?;
    let submit = renderer.submit(&reused, [400, 220], 1.0)?.unwrap();
    assert_eq!(submit.gpu.glyph_material_bytes, 16);
    cpu_oracle(&reused, [400, 220], 1.0, &renderer.image()?);
    compare(
        renderer,
        &reused,
        [400, 220],
        1.0,
        "text-material-slot-reuse",
        output,
        results,
    )?;
    for (name, offset) in [
        ("text-property-negative-half", [-8.5, -0.5]),
        ("text-property-positive-half", [0.5, 0.5]),
        ("text-property-fractional", [0.49, 0.51]),
    ] {
        let mut tx = Transaction::default();
        tx.translation(node, offset);
        batch.commit(tx, BackendCapabilities::default())?;
        let frame = batch.frame([400.0, 220.0])?;
        let submit = renderer.submit(&frame, [400, 220], 1.0)?.unwrap();
        assert_eq!(
            (
                submit.gpu.glyph_instance_bytes,
                submit.gpu.glyph_material_bytes,
                submit.gpu.glyph_property_bytes
            ),
            (0, 0, 32)
        );
        cpu_oracle(&frame, [400, 220], 1.0, &renderer.image()?);
        compare(renderer, &frame, [400, 220], 1.0, name, output, results)?;
        results.last_mut().unwrap()["glyph_property_bytes"] = 32.into();
    }
    Ok(())
}

pub(super) fn benchmark() -> Result<serde_json::Value, Box<dyn Error>> {
    let (context, mut raster) = context()?;
    let value = "Retained 中文 office a\u{301} ".repeat(30);
    let resource = text(&context, &mut raster, &value, 1.0)?;
    let mut tree = UiTree::new(style(0.0, 0.0, 800.0, Some(600.0)), 0x000000ff)?;
    let node = tree.append(tree.root(), style(8.0, 0.0, 780.0, None), 0)?;
    set(&mut tree, node, &resource, 0xffdd88ff)?;
    let mut renderer = WgpuFrameRenderer::new(3)?;
    renderer.submit(&tree.frame([800.0, 600.0])?, [800, 600], 1.0)?;
    renderer.finish()?;
    let mut cases = vec![];
    let mut samples = [vec![], vec![], vec![]];
    let mut round_medians = [vec![], vec![], vec![]];
    let mut atlas_bytes = [vec![], vec![], vec![]];
    let mut instance_bytes = [vec![], vec![], vec![]];
    let mut material_bytes = [vec![], vec![], vec![]];
    let mut property_bytes = [vec![], vec![], vec![]];
    let mut baseline_pixels = None;
    for round in 0..3 {
        for mode in (0..3).map(|index| (index + round) % 3) {
            let mut round_samples = vec![];
            for i in 0..26 {
                set(
                    &mut tree,
                    node,
                    &resource,
                    if i % 2 == 0 { 0x88ddffff } else { 0xffdd88ff },
                )?;
                // Identical visible pixels: mode 1 also varies a clip edge
                // beyond the text ink, exercising the separate property path.
                let edge = if mode == 1 { (i % 2) as f32 } else { 0.0 };
                let mut tx = Transaction::default();
                tx.clip(
                    node,
                    Some(Rect {
                        x: 0.0,
                        y: 0.0,
                        width: 790.0 + edge,
                        height: 600.0,
                    }),
                );
                tree.commit(tx, BackendCapabilities::default())?;
                let frame = tree.frame([800.0, 600.0])?;
                if mode == 2 {
                    renderer.reset()?;
                }
                let start = Instant::now();
                let submit = renderer.submit(&frame, [800, 600], 1.0)?.unwrap();
                renderer.finish()?;
                if i >= 6 {
                    let elapsed = start.elapsed().as_secs_f64() * 1e6;
                    samples[mode].push(elapsed);
                    round_samples.push(elapsed);
                    atlas_bytes[mode].push(submit.gpu.glyph_atlas_bytes);
                    instance_bytes[mode].push(submit.gpu.glyph_instance_bytes);
                    material_bytes[mode].push(submit.gpu.glyph_material_bytes);
                    property_bytes[mode].push(submit.gpu.glyph_property_bytes);
                }
            }
            round_samples.sort_by(f64::total_cmp);
            round_medians[mode].push(round_samples[round_samples.len() / 2]);
            // Both modes end at the same color. Readback is outside timing.
            let actual = renderer.image()?;
            if let Some(reference) = &baseline_pixels {
                assert_eq!(actual.as_raw(), reference, "glyph warm/cold A/B pixels");
            } else {
                baseline_pixels = Some(actual.into_raw());
            }
        }
    }
    for mode in 0..3 {
        samples[mode].sort_by(f64::total_cmp);
        assert!(
            atlas_bytes[mode]
                .iter()
                .all(|bytes| *bytes == atlas_bytes[mode][0])
        );
        assert!(
            instance_bytes[mode]
                .iter()
                .all(|bytes| *bytes == instance_bytes[mode][0])
        );
        assert_eq!(atlas_bytes[mode][0] == 0, mode != 2);
        assert!(
            material_bytes[mode]
                .iter()
                .all(|bytes| *bytes == material_bytes[mode][0])
        );
        if mode == 0 {
            assert_eq!((instance_bytes[mode][0], material_bytes[mode][0]), (0, 16));
        }
        if mode == 1 {
            assert_eq!((instance_bytes[mode][0], material_bytes[mode][0]), (0, 16));
        }
        assert!(
            property_bytes[mode]
                .iter()
                .all(|bytes| *bytes == property_bytes[mode][0])
        );
        assert_eq!(
            property_bytes[mode][0],
            match mode {
                0 => 0,
                1 => 32,
                _ => 64,
            }
        );
        cases.push(serde_json::json!({"mode":match mode {0=>"reuse glyph atlas and instances; material-only color",1=>"warm atlas; material and outside-ink clip properties",_=>"cold renderer uploads same glyph masks"},"samples":samples[mode].len(),"median_us":samples[mode][samples[mode].len()/2],"p95_us":samples[mode][samples[mode].len()*95/100],"atlas_bytes_per_frame":atlas_bytes[mode][0],"instance_bytes_per_frame":instance_bytes[mode][0],"material_bytes_per_frame":material_bytes[mode][0],"property_bytes_per_frame":property_bytes[mode][0],"sorted_samples_us":samples[mode],"round_medians_us":round_medians[mode],"pixel_equal":true}));
    }
    Ok(
        serde_json::json!({"scope":"Full-redraw FramePacket submit + GPU completion for text color changes. Second warm mode also changes a clip edge beyond text ink with identical output, exercising property-only writes. Resources, shaping, rasterization, snapshot extraction and renderer reset construction outside timing. Cold mode starts with empty GPU atlas/instances, including GPU allocation/binding and buffer growth. No readback/presentation. 3 rotating rounds, 6 warmups + 20 samples per mode per round.","glyphs":resource.chunk(780.0)?.glyphs.len(),"rasterized_glyphs":raster.rasterized_glyphs(),"cases":cases}),
    )
}
