@group(1) @binding(0) var<uniform> camera: vec4<f32>;
struct Glyph {
    bounds: vec4<f32>,
    tile: vec4<u32>,
    material: vec4<u32>,
}
@group(0) @binding(0) var<storage, read> glyphs: array<Glyph>;
@group(0) @binding(1) var atlas: texture_2d<f32>;
@group(0) @binding(2) var<uniform> viewport: vec4<f32>;
@group(0) @binding(3) var<storage, read> materials: array<vec4<f32>>;
struct Properties { origin_scale: vec4<f32>, clip: vec4<f32> }
@group(0) @binding(4) var<storage, read> properties: array<Properties>;
struct Varying {
    @builtin(position) position: vec4<f32>,
    @location(0) @interpolate(flat) index: u32,
    @location(1) @interpolate(flat) origin: vec2<f32>,
}
@vertex fn vs_glyph(@builtin(vertex_index) vertex: u32, @builtin(instance_index) index: u32) -> Varying {
    let points = array<vec2<f32>,6>(vec2(0.0,0.0),vec2(1.0,0.0),vec2(0.0,1.0),vec2(0.0,1.0),vec2(1.0,0.0),vec2(1.0,1.0));
    let glyph = glyphs[index];
    let property = properties[glyph.material.x];
    let baseline = (glyph.bounds.xy + property.origin_scale.xy) * property.origin_scale.z;
    // Match Rust f32::round: ties away from zero, including negative positions.
    let origin = sign(baseline) * floor(abs(baseline) + vec2(0.5)) + glyph.bounds.zw;
    let p = origin + points[vertex] * vec2<f32>(glyph.tile.zw);
    var out: Varying;
    out.position = vec4((p - camera.xy) / camera.zw * vec2(2.0,-2.0) + vec2(-1.0,1.0),0.0,1.0);
    out.index = index;
    out.origin = origin;
    return out;
}
@fragment fn fs_glyph(input: Varying) -> @location(0) vec4<f32> {
    let glyph = glyphs[input.index];
    let property = properties[glyph.material.x];
    let p = input.position.xy + camera.xy;
    if (any(p < property.clip.xy) || any(p >= property.clip.xy + property.clip.zw)) { discard; }
    let local = vec2<u32>(max(p - input.origin,vec2(0.0)));
    let uv = glyph.tile.xy + min(local,glyph.tile.zw-vec2<u32>(1));
    let color = materials[glyph.material.x];
    let alpha = textureLoad(atlas,vec2<i32>(uv),0).r * color.a;
    return vec4(color.rgb * alpha,alpha);
}
