fn load_quad(instance_id: u32) -> Quad {
    var quad = load_quad_raw(instance_id);
    if (quad.order != 0u) {
        let property = spatial_resolve(quad.order - 1u);
        quad.bounds.origin = property.origin * spatial_settings.x;
        quad.content_mask = quad.bounds;
        if (property.has_clip) {
            quad.content_mask = Bounds(property.clip.origin * spatial_settings.x, property.clip.size * spatial_settings.x);
        }
    }
    return quad;
}

fn load_shadow(instance_id: u32) -> Shadow {
    var shadow = load_shadow_raw(instance_id);
    if (shadow.order != 0u) {
        let property = spatial_resolve(shadow.order - 1u);
        // Raw retained fields carry logical offset and signed spread; shape
        // sizes/radii/blur are already physical. Legacy entry points stay intact.
        let spread = shadow.element_bounds.origin.x;
        shadow.bounds.origin = ((property.origin + shadow.bounds.origin) - vec2(spread)) * spatial_settings.x;
        shadow.element_bounds.origin = property.origin * spatial_settings.x;
        shadow.content_mask = shadow.element_bounds;
        if (shadow.inset == 0u) {
            let margin = vec2(3.0 * shadow.blur_radius);
            shadow.content_mask = Bounds(shadow.bounds.origin - margin, shadow.bounds.size + 2.0 * margin);
        }
        if (property.has_clip) {
            shadow.content_mask = Bounds(property.clip.origin * spatial_settings.x, property.clip.size * spatial_settings.x);
        }
    }
    return shadow;
}
