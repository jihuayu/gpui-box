//! Same shader and real masks, warm GPU atlas: retained vs rebuilt local chunks.
use super::*;
use gpui_wgpu::{
    GlyphBatch, GlyphGeometry, GlyphUpdates, QuadBatch, QuadDraw, QuadRedraw, RetainedGlyph,
    RetainedQuadRenderer,
};
use std::time::Instant;

pub(super) fn run() -> Result<serde_json::Value, Box<dyn Error>> {
    let (context, mut raster) = retained_text::context()?;
    let resource = retained_text::text(
        &context,
        &mut raster,
        &"Retained 中文 office a\u{301} ".repeat(30),
        1.0,
    )?;
    let chunk = resource.chunk(780.0)?;
    let mut cases = vec![];
    for copies in [1, 10] {
        // Ten copies deliberately repeat local geometry at the same positions,
        // increasing CPU/instance work and visible overdraw with unchanged masks.
        let geometry = || {
            GlyphGeometry::new(
                (0..copies)
                    .flat_map(|_| chunk.glyphs.iter())
                    .map(|glyph| RetainedGlyph {
                        resource: glyph.bitmap.id(),
                        size: glyph.bitmap.size,
                        mask: glyph.bitmap.mask().clone(),
                        origin: glyph.origin,
                        offset: glyph.bitmap.offset,
                    })
                    .collect(),
            )
        };
        let retained = geometry()?;
        let batch = |geometry, color| QuadBatch {
            capacity: 0,
            writes: vec![],
            order: Some(vec![QuadDraw::Glyphs([1, 0, 0])]),
            layers: vec![],
            images: Default::default(),
            glyphs: GlyphUpdates {
                reset: false,
                removed: vec![],
                batches: vec![GlyphBatch {
                    id: [1, 0, 0],
                    geometry,
                    origin: [8.0, 0.0],
                    scale: 1.0,
                    clip: [0.0, 0.0, 800.0, 600.0],
                    color,
                }],
            },
            shadows: Default::default(),
            cache_layers: true,
            size: gpui::size(gpui::DevicePixels(800), gpui::DevicePixels(600)),
            redraw: QuadRedraw::Full,
        };
        let mut renderer = RetainedQuadRenderer::new(3)?;
        renderer.submit(batch(retained.clone(), 0xffdd88ff))?;
        renderer.finish()?;
        let mut completed = [vec![], vec![]];
        let mut cpu = [vec![], vec![]];
        let mut rounds = [vec![], vec![]];
        let mut uploads = [vec![], vec![]];
        let mut baseline = None;
        for round in 0..3 {
            for mode in if round % 2 == 0 { [0, 1] } else { [1, 0] } {
                let mut samples = vec![];
                for i in 0..26 {
                    let start = Instant::now();
                    let local = if mode == 0 {
                        retained.clone()
                    } else {
                        geometry()?
                    };
                    let submit = renderer.submit(batch(
                        local,
                        if i % 2 == 0 { 0x88ddffff } else { 0xffdd88ff },
                    ))?;
                    let submit_us = start.elapsed().as_secs_f64() * 1e6;
                    renderer.finish()?;
                    if i >= 6 {
                        let elapsed = start.elapsed().as_secs_f64() * 1e6;
                        let count = chunk.glyphs.len() * copies;
                        let expected = if mode == 0 { 0 } else { count };
                        assert_eq!(
                            (
                                submit.glyph_rebuilt_instances,
                                submit.glyph_validated_instances
                            ),
                            (expected, expected)
                        );
                        assert_eq!(
                            (
                                submit.glyph_atlas_bytes,
                                submit.glyph_material_bytes,
                                submit.glyph_property_bytes
                            ),
                            (0, 16, 0)
                        );
                        assert_eq!(submit.glyph_instance_bytes, expected as u64 * 48);
                        completed[mode].push(elapsed);
                        cpu[mode].push(submit_us);
                        samples.push(submit_us);
                        uploads[mode].push(submit.glyph_instance_bytes);
                    }
                }
                samples.sort_by(f64::total_cmp);
                rounds[mode].push(samples[samples.len() / 2]);
                let actual = renderer.image()?;
                if let Some(reference) = &baseline {
                    assert_eq!(
                        actual.as_raw(),
                        reference,
                        "retained/rebuilt glyph geometry pixels"
                    );
                } else {
                    baseline = Some(actual.into_raw());
                }
            }
        }
        for mode in 0..2 {
            completed[mode].sort_by(f64::total_cmp);
            cpu[mode].sort_by(f64::total_cmp);
            cases.push(serde_json::json!({"glyphs":chunk.glyphs.len()*copies,"mode":if mode==0 {"retain immutable geometry"} else {"recreate immutable geometry and GPU instances"},"samples":cpu[mode].len(),"submit_cpu_median_us":cpu[mode][cpu[mode].len()/2],"submit_cpu_p95_us":cpu[mode][cpu[mode].len()*95/100],"completed_median_us":completed[mode][completed[mode].len()/2],"instance_bytes":uploads[mode][0],"cpu_round_medians_us":rounds[mode],"cpu_sorted_samples_us":cpu[mode],"completed_sorted_samples_us":completed[mode],"pixel_equal":true}));
        }
    }
    Ok(
        serde_json::json!({"scope":"Direct retained GPU backend, same actual font masks and shader, warm atlas in both modes, full redraw. Rebuild mode includes cloning local glyph/mask records, immutable geometry construction/validation, instance planning/upload inside CPU submit timing. UI transaction/layout/snapshot and readback excluded. 450-glyph paragraph and ten overlapping copies (4500 glyphs); 3 rotating rounds, 6 warmups and 20 samples per mode per round. Completed timing adds GPU completion wait; no presentation.","cases":cases}),
    )
}
