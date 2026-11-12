@group(1) @binding(0) var<uniform> camera: vec4<f32>;
@group(0) @binding(0) var image: texture_2d<f32>;
struct Params {
    bounds: vec4<f32>,
    clip: vec4<f32>,
    metadata: vec4<u32>,
}
@group(0) @binding(1) var<uniform> params: Params;

struct ImageVarying {
    @builtin(position) position: vec4<f32>,
    @location(0) @interpolate(flat) bounds: vec4<f32>,
    @location(1) @interpolate(flat) clip: vec4<f32>,
}
@vertex fn vs_image(@builtin(vertex_index) index: u32) -> ImageVarying {
    let positions = array<vec2<f32>, 6>(vec2(0.0, 0.0), vec2(1.0, 0.0), vec2(0.0, 1.0),
        vec2(0.0, 1.0), vec2(1.0, 0.0), vec2(1.0, 1.0));
    var bounds = params.bounds;
    var clip = params.clip;
    if (params.metadata.z != 0u) {
        let placement = spatial_resolve(params.metadata.z - 1u);
        // Shared records reuse absolute origins for ordered border/padding offsets.
        bounds = vec4(((placement.origin + params.bounds.xy) + params.clip.xy) * spatial_settings.x, params.bounds.zw);
        clip = bounds;
        if (placement.has_clip) { clip = vec4(placement.clip.origin * spatial_settings.x, placement.clip.size * spatial_settings.x); }
    }
    let position = bounds.xy + positions[index] * bounds.zw;
    var out: ImageVarying;
    out.position = vec4((position - camera.xy) / camera.zw * vec2(2.0, -2.0) + vec2(-1.0, 1.0), 0.0, 1.0);
    out.bounds = bounds;
    out.clip = clip;
    return out;
}
@fragment fn fs_image(input: ImageVarying) -> @location(0) vec4<f32> {
    let p = input.position.xy + camera.xy;
    if (any(p < input.bounds.xy) || any(p >= input.bounds.xy + input.bounds.zw)
        || any(p < input.clip.xy) || any(p >= input.clip.xy + input.clip.zw)) {
        discard;
    }
    let dimensions = textureDimensions(image);
    let uv = (p - input.bounds.xy) / input.bounds.zw;
    let texel = min(vec2<u32>(uv * vec2<f32>(dimensions)), dimensions - vec2<u32>(1));
    let color = textureLoad(image, vec2<i32>(texel), 0);
    return vec4(color.rgb * color.a, color.a);
}
