//! Exact compared content records for a deliberately restricted integer-pixel
//! translation probe. No caller-provided version/hash can authorize cache reuse.
//! Unsupported primitives or numeric environments always take original raster.
use super::*;

fn integer(v: f32) -> bool {
    v.is_finite() && v.abs() <= 16384.0 && v.fract() == 0.0
}
fn local(bounds: &mut Bounds<ScaledPixels>, origin: [f32; 2]) -> Option<()> {
    if ![
        bounds.origin.x.0,
        bounds.origin.y.0,
        bounds.size.width.0,
        bounds.size.height.0,
        bounds.origin.x.0 + bounds.size.width.0,
        bounds.origin.y.0 + bounds.size.height.0,
    ]
    .into_iter()
    .all(integer)
    {
        return None;
    }
    bounds.origin.x.0 -= origin[0];
    bounds.origin.y.0 -= origin[1];
    Some(())
}
fn placement(
    spatial: &spatial::SpatialCache,
    property: u32,
) -> Option<([f32; 2], Option<Bounds<ScaledPixels>>)> {
    if !spatial.enabled || property == 0 {
        return None;
    }
    spatial.current_placement(property - 1)
}
pub(super) fn content(
    draws: &[QuadDraw],
    bounds: [u32; 4],
    spatial: &spatial::SpatialCache,
    quads: &PersistentStorage<Quad>,
    shadows: &PersistentStorage<gpui::Shadow>,
) -> Option<Vec<u8>> {
    // Avoid division/reconstruction rounding in the shadow probe. Non-unit
    // device scales keep the original raster until qualified independently.
    if spatial.enabled && spatial.physical_scale() != 1.0 {
        return None;
    }
    if bounds.into_iter().any(|v| v > 16384) {
        return None;
    }
    let origin = [bounds[0] as f32, bounds[1] as f32];
    // This probe stores compared records, never a collision-prone digest. Bound
    // proof storage/work per layer; large or unsupported layers simply raster.
    let records = draws.iter().try_fold(0usize, |total, draw| match draw {
        QuadDraw::Instances(range) | QuadDraw::Shadows(range) => total.checked_add(range.len()),
        _ => None,
    })?;
    if records > 4096 || draws.len() > 4096 {
        return None;
    }
    let target = Bounds {
        origin: gpui::point(ScaledPixels(origin[0]), ScaledPixels(origin[1])),
        size: gpui::size(
            ScaledPixels(bounds[2] as f32),
            ScaledPixels(bounds[3] as f32),
        ),
    };
    let mut bytes = vec![];
    for draw in draws {
        match draw {
            QuadDraw::Instances(range) => {
                bytes.push(1);
                bytes.extend_from_slice(&(range.len() as u64).to_ne_bytes());
                for index in range.clone() {
                    let mut q = quads.value(index);
                    // World-relative patterns can depend on absolute position.
                    q.background.as_solid()?;
                    if spatial.enabled && q.order != 0 {
                        let (p, clip) = placement(spatial, q.order)?;
                        q.bounds.origin = gpui::point(ScaledPixels(p[0]), ScaledPixels(p[1]));
                        q.content_mask.bounds = clip.unwrap_or(q.bounds);
                    }
                    q.order = 0;
                    local(&mut q.bounds, origin)?;
                    q.content_mask.bounds = q.content_mask.bounds.intersect(&target);
                    local(&mut q.content_mask.bounds, origin)?;
                    // SAFETY: same initialized, pinned Quad ABI as storage uploads.
                    let records = [q];
                    bytes.extend_from_slice(unsafe { WgpuRendererCore::instance_bytes(&records) });
                }
            }
            QuadDraw::Shadows(range) => {
                bytes.push(2);
                bytes.extend_from_slice(&(range.len() as u64).to_ne_bytes());
                for index in range.clone() {
                    let mut s = shadows.value(index);
                    if !integer(s.blur_radius.0) {
                        return None;
                    }
                    if spatial.enabled && s.order != 0 {
                        let (p, clip) = placement(spatial, s.order)?;
                        let scale = spatial.physical_scale();
                        let spread = s.element_bounds.origin.x.0;
                        // Exact shader operation order, scale last.
                        s.bounds.origin = gpui::point(
                            ScaledPixels(((p[0] / scale + s.bounds.origin.x.0) - spread) * scale),
                            ScaledPixels(((p[1] / scale + s.bounds.origin.y.0) - spread) * scale),
                        );
                        s.element_bounds.origin =
                            gpui::point(ScaledPixels(p[0]), ScaledPixels(p[1]));
                        s.content_mask.bounds = clip.unwrap_or_else(|| {
                            if s.inset != 0 {
                                s.element_bounds
                            } else {
                                s.bounds.dilate(s.blur_radius * 3.0)
                            }
                        });
                    }
                    s.order = 0;
                    local(&mut s.bounds, origin)?;
                    local(&mut s.element_bounds, origin)?;
                    s.content_mask.bounds = s.content_mask.bounds.intersect(&target);
                    local(&mut s.content_mask.bounds, origin)?;
                    // SAFETY: initialized Shadow ABI with explicit pad, as upload.
                    let records = [s];
                    bytes.extend_from_slice(unsafe { WgpuRendererCore::instance_bytes(&records) });
                }
            }
            // Child/resource reuse needs sampling/content epochs, not stable IDs.
            QuadDraw::Layer(_) | QuadDraw::Image(_) | QuadDraw::Glyphs(_) => return None,
        }
    }
    Some(bytes)
}
