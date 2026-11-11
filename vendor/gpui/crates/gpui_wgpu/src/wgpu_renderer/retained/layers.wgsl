@group(0) @binding(0) var layer: texture_2d<f32>;
@group(0) @binding(1) var<uniform> opacity: vec4<f32>;

@vertex fn vs_layer(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {
    let positions = array<vec2<f32>, 3>(vec2(-1.0, -1.0), vec2(3.0, -1.0), vec2(-1.0, 3.0));
    return vec4(positions[index], 0.0, 1.0);
}

@fragment fn fs_layer(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    return textureLoad(layer, vec2<i32>(position.xy), 0) * opacity.x;
}
