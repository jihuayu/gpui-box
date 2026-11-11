//! Decoded immutable RGBA textures and per-image geometry bindings.
use super::*;
use std::collections::{BTreeMap, BTreeSet};

pub type QuadImageId = [u64; 3];
pub struct QuadImage {
    pub id: QuadImageId,
    /// Unique immutable identity; reusing it requires the same Arc allocation.
    pub resource: u64,
    pub size: [u32; 2],
    pub rgba: Arc<[u8]>,
    pub bounds: [f32; 4],
    pub clip: [f32; 4],
}
struct Texture {
    texture: wgpu::Texture,
    view: wgpu::TextureView,
    pixels: Arc<[u8]>,
    size: [u32; 2],
}
struct Draw {
    resource: u64,
    geometry: [f32; 12],
    buffer: wgpu::Buffer,
    binding: wgpu::BindGroup,
}
pub(super) type ImageLease = (Vec<wgpu::Texture>, Vec<(wgpu::Buffer, wgpu::BindGroup)>);
#[derive(Default)]
pub(super) struct ImageStats {
    pub bytes: u64,
    pub uploads: usize,
    pub geometry_bytes: u64,
    pub changed: BTreeSet<QuadImageId>,
}
pub(super) struct ImageCache {
    textures: BTreeMap<u64, Texture>,
    draws: BTreeMap<QuadImageId, Draw>,
    layout: wgpu::BindGroupLayout,
    pipeline: wgpu::RenderPipeline,
}
impl ImageCache {
    pub fn new(renderer: &WgpuHeadlessRenderer) -> Self {
        let device = &renderer.context.device;
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("retained_image_layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: false },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: NonZeroU64::new(48),
                    },
                    count: None,
                },
            ],
        });
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("retained_images"),
            source: wgpu::ShaderSource::Wgsl(include_str!("images.wgsl").into()),
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("retained_images"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("retained_images"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_image"),
                buffers: &[],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_image"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: renderer.core.target_format,
                    blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: Default::default(),
            depth_stencil: None,
            multisample: Default::default(),
            multiview_mask: None,
            cache: None,
        });
        Self {
            textures: BTreeMap::new(),
            draws: BTreeMap::new(),
            layout,
            pipeline,
        }
    }
    pub fn validate(
        &self,
        images: &[QuadImage],
        max_dimension: u32,
    ) -> Result<BTreeSet<QuadImageId>> {
        let mut ids = BTreeSet::new();
        let mut resources = BTreeMap::<u64, &QuadImage>::new();
        for image in images {
            anyhow::ensure!(ids.insert(image.id), "duplicate image draw");
            anyhow::ensure!(
                image.size.iter().all(|n| *n > 0 && *n <= max_dimension),
                "invalid image dimensions"
            );
            let bytes = u64::from(image.size[0]) * u64::from(image.size[1]) * 4;
            anyhow::ensure!(
                bytes <= 64 * 1024 * 1024 && bytes == image.rgba.len() as u64,
                "invalid image pixels"
            );
            for rect in [image.bounds, image.clip] {
                anyhow::ensure!(
                    rect.iter().all(|n| n.is_finite())
                        && rect[2] >= 0.0
                        && rect[3] >= 0.0
                        && (rect[0] + rect[2]).is_finite()
                        && (rect[1] + rect[3]).is_finite(),
                    "invalid image geometry"
                );
            }
            if let Some(previous) = resources.insert(image.resource, image) {
                anyhow::ensure!(
                    previous.size == image.size && Arc::ptr_eq(&previous.rgba, &image.rgba),
                    "mutable image identity"
                );
            }
            if let Some(previous) = self.textures.get(&image.resource) {
                anyhow::ensure!(
                    previous.size == image.size && Arc::ptr_eq(&previous.pixels, &image.rgba),
                    "mutable cached image identity"
                );
            }
        }
        let bytes: u64 = resources
            .values()
            .map(|image| image.rgba.len() as u64)
            .sum();
        anyhow::ensure!(bytes <= 256 * 1024 * 1024, "live image budget exceeded");
        Ok(ids)
    }
    pub fn prepare(
        &mut self,
        renderer: &WgpuHeadlessRenderer,
        images: Vec<QuadImage>,
        size: Size<DevicePixels>,
    ) -> ImageStats {
        let mut stats = ImageStats::default();
        let live: BTreeSet<_> = images.iter().map(|image| image.resource).collect();
        let ids: BTreeSet<_> = images.iter().map(|image| image.id).collect();
        self.textures.retain(|id, _| live.contains(id));
        self.draws.retain(|id, _| ids.contains(id));
        let device = &renderer.context.device;
        let queue = &renderer.context.queue;
        for image in images {
            self.textures.entry(image.resource).or_insert_with(|| {
                let texture = device.create_texture(&wgpu::TextureDescriptor {
                    label: Some("retained_rgba8"),
                    size: wgpu::Extent3d {
                        width: image.size[0],
                        height: image.size[1],
                        depth_or_array_layers: 1,
                    },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: wgpu::TextureFormat::Rgba8Unorm,
                    usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                    view_formats: &[],
                });
                queue.write_texture(
                    wgpu::TexelCopyTextureInfo {
                        texture: &texture,
                        mip_level: 0,
                        origin: wgpu::Origin3d::ZERO,
                        aspect: wgpu::TextureAspect::All,
                    },
                    &image.rgba,
                    wgpu::TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(image.size[0] * 4),
                        rows_per_image: Some(image.size[1]),
                    },
                    wgpu::Extent3d {
                        width: image.size[0],
                        height: image.size[1],
                        depth_or_array_layers: 1,
                    },
                );
                stats.bytes += image.rgba.len() as u64;
                stats.uploads += 1;
                let view = texture.create_view(&Default::default());
                Texture {
                    texture,
                    view,
                    pixels: image.rgba.clone(),
                    size: image.size,
                }
            });
            let geometry = [
                image.bounds[0],
                image.bounds[1],
                image.bounds[2],
                image.bounds[3],
                image.clip[0],
                image.clip[1],
                image.clip[2],
                image.clip[3],
                size.width.0 as f32,
                size.height.0 as f32,
                0.0,
                0.0,
            ];
            let previous = self.draws.get(&image.id);
            let rebind = previous.is_none_or(|draw| draw.resource != image.resource);
            let changed = rebind || previous.is_some_and(|draw| draw.geometry != geometry);
            if rebind {
                let buffer = device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("retained_image_geometry"),
                    size: 48,
                    usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                });
                let binding = device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("retained_image_binding"),
                    layout: &self.layout,
                    entries: &[
                        wgpu::BindGroupEntry {
                            binding: 0,
                            resource: wgpu::BindingResource::TextureView(
                                &self.textures[&image.resource].view,
                            ),
                        },
                        wgpu::BindGroupEntry {
                            binding: 1,
                            resource: buffer.as_entire_binding(),
                        },
                    ],
                });
                self.draws.insert(
                    image.id,
                    Draw {
                        resource: image.resource,
                        geometry,
                        buffer,
                        binding,
                    },
                );
            }
            if changed {
                let draw = self.draws.get_mut(&image.id).unwrap();
                queue.write_buffer(&draw.buffer, 0, bytemuck::cast_slice(&geometry));
                draw.geometry = geometry;
                stats.geometry_bytes += 48;
                stats.changed.insert(image.id);
            }
        }
        stats
    }
    pub fn draw(&self, id: QuadImageId, pass: &mut wgpu::RenderPass<'_>) -> Result<()> {
        let draw = self.draws.get(&id).context("missing image draw")?;
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &draw.binding, &[]);
        pass.draw(0..6, 0..1);
        Ok(())
    }
    pub fn leases(&self) -> ImageLease {
        (
            self.textures.values().map(|t| t.texture.clone()).collect(),
            self.draws
                .values()
                .map(|d| (d.buffer.clone(), d.binding.clone()))
                .collect(),
        )
    }
}
