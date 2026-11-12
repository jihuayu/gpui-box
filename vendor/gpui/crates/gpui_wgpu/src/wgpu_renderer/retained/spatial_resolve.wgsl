// Shared bounded logical placement for retained primitives.
struct SpatialNode {
    layout_translation: vec4<f32>,
    scroll: vec4<f32>,
    clip: vec4<f32>,
    anchor_clip: vec4<f32>,
    anchor_origin: vec4<f32>,
    links: vec4<u32>,
}
@group(2) @binding(2) var<storage, read> spatial_nodes: array<SpatialNode>;
@group(2) @binding(3) var<uniform> spatial_settings: vec4<f32>;
struct SpatialResult { origin: vec2<f32>, clip: Bounds, has_clip: bool }

fn spatial_resolve(slot: u32) -> SpatialResult {
    var path: array<u32, 32>;
    var count = 0u;
    var cursor = slot;
    loop {
        path[count] = cursor;
        count += 1u;
        let node = spatial_nodes[cursor];
        if ((node.links.z & 2u) != 0u || count == 32u) { break; }
        cursor = node.links.x;
    }
    let anchor = spatial_nodes[path[count - 1u]];
    var result: SpatialResult;
    result.origin = anchor.anchor_origin.xy;
    result.clip = Bounds(anchor.anchor_clip.xy, anchor.anchor_clip.zw);
    result.has_clip = (anchor.links.z & 8u) != 0u;
    var scroll = anchor.scroll.xy;
    // Reproduce producer root-to-leaf f32 operation order, then scale once.
    loop {
        count -= 1u;
        if (count == 0u) { break; }
        let node = spatial_nodes[path[count - 1u]];
        result.origin = (result.origin - scroll) + node.layout_translation.xy;
        result.origin += node.layout_translation.zw;
        if ((node.links.z & 1u) != 0u) {
            let origin = result.origin + node.clip.xy;
            if (result.has_clip) {
                let start = max(result.clip.origin, origin);
                let end = max(start, min(result.clip.origin + result.clip.size, origin + node.clip.zw));
                result.clip = Bounds(start, end - start);
            } else {
                result.clip = Bounds(origin, node.clip.zw);
                result.has_clip = true;
            }
        }
        scroll = node.scroll.xy;
    }
    return result;
}

