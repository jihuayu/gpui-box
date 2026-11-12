@group(0) @binding(0) var layer: texture_2d<f32>;
@group(0) @binding(1) var<uniform> opacity: vec4<f32>;
@group(0) @binding(2) var<uniform> bounds: vec4<f32>;
@group(1) @binding(0) var<uniform> camera: vec4<f32>;

@vertex fn vs_layer(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {
    let positions = array<vec2<f32>, 6>(vec2(0.0, 0.0), vec2(1.0, 0.0), vec2(0.0, 1.0),
        vec2(0.0, 1.0), vec2(1.0, 0.0), vec2(1.0, 1.0));
    let world = bounds.xy + positions[index] * bounds.zw;
    return vec4((world - camera.xy) / camera.zw * vec2(2.0, -2.0) + vec2(-1.0, 1.0), 0.0, 1.0);
}

@fragment fn fs_layer(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let local = position.xy + camera.xy - bounds.xy;
    return textureLoad(layer, vec2<i32>(local), 0) * opacity.x;
}
