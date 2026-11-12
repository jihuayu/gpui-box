@group(1) @binding(0) var<uniform> camera: vec4<f32>;
@group(0) @binding(0) var image: texture_2d<f32>;
struct Params {
    bounds: vec4<f32>,
    clip: vec4<f32>,
    viewport: vec4<f32>,
}
@group(0) @binding(1) var<uniform> params: Params;

@vertex fn vs_image(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {
    let positions = array<vec2<f32>, 6>(vec2(0.0, 0.0), vec2(1.0, 0.0), vec2(0.0, 1.0),
        vec2(0.0, 1.0), vec2(1.0, 0.0), vec2(1.0, 1.0));
    let position = params.bounds.xy + positions[index] * params.bounds.zw;
    return vec4((position - camera.xy) / camera.zw * vec2(2.0, -2.0) + vec2(-1.0, 1.0), 0.0, 1.0);
}
@fragment fn fs_image(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let p = position.xy + camera.xy;
    if (any(p < params.bounds.xy) || any(p >= params.bounds.xy + params.bounds.zw)
        || any(p < params.clip.xy) || any(p >= params.clip.xy + params.clip.zw)) {
        discard;
    }
    let dimensions = textureDimensions(image);
    let uv = (p - params.bounds.xy) / params.bounds.zw;
    let texel = min(vec2<u32>(uv * vec2<f32>(dimensions)), dimensions - vec2<u32>(1));
    let color = textureLoad(image, vec2<i32>(texel), 0);
    return vec4(color.rgb * color.a, color.a);
}
