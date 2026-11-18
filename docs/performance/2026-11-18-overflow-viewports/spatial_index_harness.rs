//! Compile the actual backend graph/index tests without the separate vendor
//! workspace. This stub matches the fields, not bytemuck/storage validation.
#![allow(dead_code)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
#[repr(C)]
pub struct SpatialNode {
    pub layout_translation: [f32; 4],
    pub scroll: [f32; 4],
    pub clip: [f32; 4],
    pub anchor_clip: [f32; 4],
    pub anchor_origin: [f32; 4],
    pub overflow_clip: [f32; 4],
    pub links: [u32; 4],
}
#[path = "../../../vendor/gpui/crates/gpui_wgpu/src/wgpu_renderer/retained/spatial_index.rs"]
mod spatial_index;
