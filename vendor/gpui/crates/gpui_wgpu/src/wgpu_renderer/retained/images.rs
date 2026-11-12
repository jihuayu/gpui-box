//! Decoded immutable RGBA textures and per-image geometry bindings.
use super::*;
use std::collections::{BTreeMap, BTreeSet};

pub type QuadImageId = [u64; 3];
#[derive(Clone)]
pub struct QuadImage {
    pub id: QuadImageId,
    /// Unique immutable identity; reusing it requires the same Arc allocation.
    pub resource: u64,
    pub size: [u32; 2],
    pub rgba: Arc<[u8]>,
    pub bounds: [f32; 4],
    pub clip: [f32; 4],
}
/// Sequential updates preserve all unmentioned draws. Reset supplies the full set.
#[derive(Default)]
pub struct ImageUpdates {
    pub reset: bool,
    pub removed: Vec<QuadImageId>,
    pub images: Vec<QuadImage>,
}
struct Texture {
    texture: wgpu::Texture,
    view: wgpu::TextureView,
    pixels: Arc<[u8]>,
    size: [u32; 2],
    references: usize,
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
    pub batch_updates: usize,
}
pub(super) struct ImageCache {
    textures: BTreeMap<u64, Texture>,
    draws: BTreeMap<QuadImageId, Draw>,
    live_bytes: u64,
    viewport: Option<Size<DevicePixels>>,
    layout: wgpu::BindGroupLayout,
    pipeline: wgpu::RenderPipeline,
}
impl ImageCache {
    pub fn new(renderer: &WgpuHeadlessRenderer, camera_layout: &wgpu::BindGroupLayout) -> Self {
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
            bind_group_layouts: &[Some(&layout), Some(camera_layout)],
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
            live_bytes: 0,
            viewport: None,
            layout,
            pipeline,
        }
    }
    fn removals(&self, updates: &ImageUpdates) -> BTreeSet<QuadImageId> {
        if updates.reset {
            let live: BTreeSet<_> = updates.images.iter().map(|image| image.id).collect();
            self.draws
                .keys()
                .filter(|id| !live.contains(*id))
                .copied()
                .collect()
        } else {
            updates.removed.iter().copied().collect()
        }
    }
    pub fn validate(
        &self,
        updates: &ImageUpdates,
        max_dimension: u32,
    ) -> Result<BTreeSet<QuadImageId>> {
        anyhow::ensure!(
            !updates.reset || updates.removed.is_empty(),
            "reset with explicit image removals"
        );
        let removals = self.removals(updates);
        anyhow::ensure!(
            updates.reset || removals.len() == updates.removed.len(),
            "duplicate image removal"
        );
        let mut ids: BTreeSet<_> = self
            .draws
            .keys()
            .filter(|id| !removals.contains(*id))
            .copied()
            .collect();
        let mut writes = BTreeSet::new();
        let mut resources = BTreeMap::<u64, &QuadImage>::new();
        let mut changes = BTreeMap::<u64, i64>::new();
        for id in &removals {
            if let Some(draw) = self.draws.get(id) {
                *changes.entry(draw.resource).or_default() -= 1;
            }
        }
        for image in &updates.images {
            anyhow::ensure!(
                writes.insert(image.id) && !removals.contains(&image.id),
                "duplicate/conflicting image update"
            );
            ids.insert(image.id);
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
            if let Some(previous) = self.draws.get(&image.id) {
                if previous.resource == image.resource {
                    continue;
                }
                *changes.entry(previous.resource).or_default() -= 1;
            }
            *changes.entry(image.resource).or_default() += 1;
        }
        let mut bytes = self.live_bytes as i64;
        for (id, change) in changes {
            let old = self.textures.get(&id);
            let before = old.map_or(0, |texture| texture.references as i64);
            let after = before + change;
            anyhow::ensure!(after >= 0, "image resource reference underflow");
            if before == 0 && after > 0 {
                bytes += resources[&id].rgba.len() as i64;
            }
            if before > 0 && after == 0 {
                bytes -= old.unwrap().pixels.len() as i64;
            }
        }
        anyhow::ensure!(
            (0..=256 * 1024 * 1024).contains(&bytes),
            "live image budget exceeded"
        );
        Ok(ids)
    }
    pub fn prepare(
        &mut self,
        renderer: &WgpuHeadlessRenderer,
        updates: ImageUpdates,
        size: Size<DevicePixels>,
    ) -> ImageStats {
        let mut stats = ImageStats {
            batch_updates: updates.images.len(),
            ..Default::default()
        };
        let mut changes = BTreeMap::<u64, i64>::new();
        for id in self.removals(&updates) {
            if let Some(draw) = self.draws.remove(&id) {
                *changes.entry(draw.resource).or_default() -= 1;
            }
        }
        for image in &updates.images {
            if let Some(old) = self.draws.get(&image.id) {
                if old.resource == image.resource {
                    continue;
                }
                *changes.entry(old.resource).or_default() -= 1;
            }
            *changes.entry(image.resource).or_default() += 1;
        }
        // Apply the net ownership delta before eviction. Swaps and shared-owner
        // replacement must retain existing textures until all incoming owners count.
        for (id, change) in &changes {
            if let Some(texture) = self.textures.get_mut(id) {
                texture.references = (texture.references as i64 + change) as usize;
            }
        }
        for id in changes.keys() {
            if self
                .textures
                .get(id)
                .is_some_and(|texture| texture.references == 0)
            {
                self.live_bytes -= self.textures.remove(id).unwrap().pixels.len() as u64;
            }
        }
        let device = &renderer.context.device;
        let queue = &renderer.context.queue;
        for image in updates.images {
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
                self.live_bytes += image.rgba.len() as u64;
                let view = texture.create_view(&Default::default());
                Texture {
                    texture,
                    view,
                    pixels: image.rgba.clone(),
                    size: image.size,
                    references: changes[&image.resource] as usize,
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
        // A surface-only resize has no item delta. Retained geometry uniforms
        // still require the new viewport; updated draws already contain it.
        if self.viewport != Some(size) {
            for (id, draw) in &mut self.draws {
                if draw.geometry[8] != size.width.0 as f32
                    || draw.geometry[9] != size.height.0 as f32
                {
                    draw.geometry[8] = size.width.0 as f32;
                    draw.geometry[9] = size.height.0 as f32;
                    queue.write_buffer(&draw.buffer, 0, bytemuck::cast_slice(&draw.geometry));
                    stats.geometry_bytes += 48;
                    stats.changed.insert(*id);
                }
            }
        }
        self.viewport = Some(size);
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
