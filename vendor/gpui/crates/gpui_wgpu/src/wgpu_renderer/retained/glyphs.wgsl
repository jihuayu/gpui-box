struct Glyph {
    bounds: vec4<f32>,
    clip: vec4<f32>,
    tile: vec4<u32>,
    color: vec4<f32>,
}
@group(0) @binding(0) var<storage, read> glyphs: array<Glyph>;
@group(0) @binding(1) var atlas: texture_2d<f32>;
@group(0) @binding(2) var<uniform> viewport: vec4<f32>;
struct Varying {
    @builtin(position) position: vec4<f32>,
    @location(0) @interpolate(flat) index: u32,
}
@vertex fn vs_glyph(@builtin(vertex_index) vertex: u32, @builtin(instance_index) index: u32) -> Varying {
    let points = array<vec2<f32>,6>(vec2(0.0,0.0),vec2(1.0,0.0),vec2(0.0,1.0),vec2(0.0,1.0),vec2(1.0,0.0),vec2(1.0,1.0));
    let glyph = glyphs[index];
    let p = glyph.bounds.xy + points[vertex] * glyph.bounds.zw;
    var out: Varying;
    out.position = vec4(p / viewport.xy * vec2(2.0,-2.0) + vec2(-1.0,1.0),0.0,1.0);
    out.index = index;
    return out;
}
@fragment fn fs_glyph(input: Varying) -> @location(0) vec4<f32> {
    let glyph = glyphs[input.index];
    let p = input.position.xy;
    if (any(p < glyph.clip.xy) || any(p >= glyph.clip.xy + glyph.clip.zw)) { discard; }
    let local = vec2<u32>(max(p - glyph.bounds.xy,vec2(0.0)));
    let uv = glyph.tile.xy + min(local,glyph.tile.zw-vec2<u32>(1));
    let alpha = textureLoad(atlas,vec2<i32>(uv),0).r * glyph.color.a;
    return vec4(glyph.color.rgb * alpha,alpha);
}
