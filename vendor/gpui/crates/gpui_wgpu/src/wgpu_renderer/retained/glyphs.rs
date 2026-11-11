//! Packed R8 glyph pages and persistent batched 64-byte instance records.
use super::*;
use std::collections::{BTreeMap, BTreeSet};
pub type GlyphDrawId = [u64; 3];
pub struct RetainedGlyph {
    pub resource: u64,
    pub size: [u32; 2],
    pub mask: Arc<[u8]>,
    pub bounds: [f32; 4],
}
pub struct GlyphBatch {
    pub id: GlyphDrawId,
    pub clip: [f32; 4],
    pub color: u32,
    pub glyphs: Vec<RetainedGlyph>,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
#[repr(C)]
struct Instance {
    bounds: [f32; 4],
    clip: [f32; 4],
    tile: [u32; 4],
    material: [u32; 4],
}
struct Page {
    texture: wgpu::Texture,
    view: wgpu::TextureView,
    binding: wgpu::BindGroup,
    allocator: etagere::AtlasAllocator,
}
struct Tile {
    page: usize,
    allocation: etagere::AllocId,
    bounds: [u32; 4],
    pixels: Arc<[u8]>,
}
struct Draw {
    start: u32,
    capacity: u32,
    instances: Vec<Instance>,
    runs: Vec<(usize, Range<u32>)>,
    resources: Vec<u64>,
    material: u32,
}
pub(super) type GlyphLease = (
    wgpu::Buffer,
    wgpu::Buffer,
    wgpu::Buffer,
    Vec<(wgpu::Texture, wgpu::BindGroup)>,
);
#[derive(Default)]
pub(super) struct GlyphStats {
    pub atlas_bytes: u64,
    pub atlas_uploads: usize,
    pub instance_bytes: u64,
    pub ranges: usize,
    pub material_bytes: u64,
    pub material_ranges: usize,
    pub changed: BTreeSet<GlyphDrawId>,
}
pub(super) struct GlyphCache {
    pages: Vec<Option<Page>>,
    tiles: BTreeMap<u64, Tile>,
    draws: BTreeMap<GlyphDrawId, Draw>,
    buffer: wgpu::Buffer,
    materials: wgpu::Buffer,
    colors: Vec<[f32; 4]>,
    free_materials: Vec<u32>,
    viewport: wgpu::Buffer,
    mirror: Vec<Instance>,
    free: Vec<Range<u32>>,
    layout: wgpu::BindGroupLayout,
    pipeline: wgpu::RenderPipeline,
    edge: u32,
    size: Option<Size<DevicePixels>>,
}
impl GlyphCache {
    pub fn new(renderer: &WgpuHeadlessRenderer) -> Self {
        let device = &renderer.context.device;
        let edge = 1024.min(device.limits().max_texture_dimension_2d);
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("retained_glyphs"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: NonZeroU64::new(64),
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: false },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::VERTEX,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: NonZeroU64::new(16),
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: NonZeroU64::new(16),
                    },
                    count: None,
                },
            ],
        });
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("retained_glyphs"),
            source: wgpu::ShaderSource::Wgsl(include_str!("glyphs.wgsl").into()),
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("retained_glyphs"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("retained_glyphs"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_glyph"),
                buffers: &[],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_glyph"),
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
        let buffer = Self::buffer(device, 1);
        let materials = Self::material_buffer(device, 1);
        let viewport = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("retained_glyph_viewport"),
            size: 16,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        Self {
            pages: vec![],
            tiles: BTreeMap::new(),
            draws: BTreeMap::new(),
            buffer,
            materials,
            colors: vec![[0.0; 4]],
            free_materials: vec![],
            viewport,
            mirror: vec![Instance::default()],
            free: vec![],
            layout,
            pipeline,
            edge,
            size: None,
        }
    }
    fn buffer(device: &wgpu::Device, capacity: usize) -> wgpu::Buffer {
        device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("retained_glyph_slots"),
            size: capacity as u64 * 64,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        })
    }
    fn material_buffer(device: &wgpu::Device, capacity: usize) -> wgpu::Buffer {
        device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("retained_glyph_materials"),
            size: capacity as u64 * 16,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        })
    }
    fn binding(
        device: &wgpu::Device,
        layout: &wgpu::BindGroupLayout,
        buffer: &wgpu::Buffer,
        viewport: &wgpu::Buffer,
        materials: &wgpu::Buffer,
        view: &wgpu::TextureView,
    ) -> wgpu::BindGroup {
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("retained_glyph_page"),
            layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: viewport.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: materials.as_entire_binding(),
                },
            ],
        })
    }
    pub fn validate(
        &self,
        batches: &[GlyphBatch],
        max_bytes: u64,
    ) -> Result<BTreeSet<GlyphDrawId>> {
        let mut ids = BTreeSet::new();
        let mut unique = BTreeMap::<u64, &RetainedGlyph>::new();
        let mut count = 0u64;
        for batch in batches {
            anyhow::ensure!(ids.insert(batch.id), "duplicate glyph draw");
            anyhow::ensure!(valid_rect(batch.clip), "invalid glyph clip");
            count += batch.glyphs.len() as u64;
            for glyph in &batch.glyphs {
                anyhow::ensure!(
                    glyph.size.iter().all(|n| *n > 0 && *n <= self.edge),
                    "glyph too large for page"
                );
                anyhow::ensure!(
                    glyph.mask.len() as u64 == u64::from(glyph.size[0]) * u64::from(glyph.size[1]),
                    "invalid gray glyph bytes"
                );
                anyhow::ensure!(
                    valid_rect(glyph.bounds)
                        && glyph.bounds[2] == glyph.size[0] as f32
                        && glyph.bounds[3] == glyph.size[1] as f32,
                    "invalid glyph bounds"
                );
                if let Some(old) = unique.insert(glyph.resource, glyph) {
                    anyhow::ensure!(
                        old.size == glyph.size && Arc::ptr_eq(&old.mask, &glyph.mask),
                        "mutable glyph resource"
                    );
                }
                if let Some(old) = self.tiles.get(&glyph.resource) {
                    anyhow::ensure!(
                        old.bounds[2..] == glyph.size && Arc::ptr_eq(&old.pixels, &glyph.mask),
                        "mutable cached glyph"
                    );
                }
            }
        }
        anyhow::ensure!(
            count * 64 <= max_bytes / 2,
            "glyph instance budget exceeded"
        );
        anyhow::ensure!(
            batches.len() as u64 * 16 <= max_bytes / 2,
            "glyph material budget exceeded"
        );
        anyhow::ensure!(
            unique
                .values()
                .map(|glyph| glyph.mask.len() as u64)
                .sum::<u64>()
                <= 32 * 1024 * 1024,
            "live glyph mask budget exceeded"
        );
        Ok(ids)
    }
    fn tile(
        &mut self,
        renderer: &WgpuHeadlessRenderer,
        glyph: &RetainedGlyph,
        stats: &mut GlyphStats,
    ) -> Result<()> {
        if self.tiles.contains_key(&glyph.resource) {
            return Ok(());
        }
        let requested = etagere::size2(glyph.size[0] as i32, glyph.size[1] as i32);
        let allocation = self.pages.iter_mut().enumerate().find_map(|(index, page)| {
            page.as_mut()?
                .allocator
                .allocate(requested)
                .map(|allocation| (index, allocation))
        });
        let (index, allocation) = if let Some(found) = allocation {
            found
        } else {
            anyhow::ensure!(
                self.pages.iter().flatten().count() < 64,
                "glyph atlas page budget exceeded"
            );
            let device = &renderer.context.device;
            let texture = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("retained_gray_atlas"),
                size: wgpu::Extent3d {
                    width: self.edge,
                    height: self.edge,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::R8Unorm,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            });
            let view = texture.create_view(&Default::default());
            let binding = Self::binding(
                device,
                &self.layout,
                &self.buffer,
                &self.viewport,
                &self.materials,
                &view,
            );
            let mut allocator =
                etagere::AtlasAllocator::new(etagere::size2(self.edge as i32, self.edge as i32));
            let allocation = allocator
                .allocate(requested)
                .context("glyph allocation failed")?;
            let page = Page {
                texture,
                view,
                binding,
                allocator,
            };
            let index = self
                .pages
                .iter()
                .position(Option::is_none)
                .unwrap_or(self.pages.len());
            if index == self.pages.len() {
                self.pages.push(Some(page));
            } else {
                self.pages[index] = Some(page);
            }
            (index, allocation)
        };
        let bounds = [
            allocation.rectangle.min.x as u32,
            allocation.rectangle.min.y as u32,
            glyph.size[0],
            glyph.size[1],
        ];
        renderer.context.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &self.pages[index].as_ref().unwrap().texture,
                mip_level: 0,
                origin: wgpu::Origin3d {
                    x: bounds[0],
                    y: bounds[1],
                    z: 0,
                },
                aspect: wgpu::TextureAspect::All,
            },
            &glyph.mask,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(glyph.size[0]),
                rows_per_image: Some(glyph.size[1]),
            },
            wgpu::Extent3d {
                width: glyph.size[0],
                height: glyph.size[1],
                depth_or_array_layers: 1,
            },
        );
        self.tiles.insert(
            glyph.resource,
            Tile {
                page: index,
                allocation: allocation.id,
                bounds,
                pixels: glyph.mask.clone(),
            },
        );
        stats.atlas_bytes += glyph.mask.len() as u64;
        stats.atlas_uploads += 1;
        Ok(())
    }
    pub fn prepare(
        &mut self,
        renderer: &WgpuHeadlessRenderer,
        batches: Vec<GlyphBatch>,
        size: Size<DevicePixels>,
    ) -> Result<GlyphStats> {
        let mut stats = GlyphStats::default();
        let live: BTreeSet<_> = batches
            .iter()
            .flat_map(|batch| batch.glyphs.iter().map(|glyph| glyph.resource))
            .collect();
        let dead: Vec<_> = self
            .tiles
            .keys()
            .filter(|id| !live.contains(id))
            .copied()
            .collect();
        for id in dead {
            let tile = self.tiles.remove(&id).unwrap();
            self.pages[tile.page]
                .as_mut()
                .unwrap()
                .allocator
                .deallocate(tile.allocation);
        }
        let used: BTreeSet<_> = self.tiles.values().map(|tile| tile.page).collect();
        for (index, page) in self.pages.iter_mut().enumerate() {
            if !used.contains(&index) {
                *page = None;
            }
        }
        let ids: BTreeSet<_> = batches.iter().map(|batch| batch.id).collect();
        let dead: Vec<_> = self
            .draws
            .keys()
            .filter(|id| !ids.contains(*id))
            .copied()
            .collect();
        for id in dead {
            let draw = self.draws.remove(&id).unwrap();
            self.free.push(draw.start..draw.start + draw.capacity);
            self.free_materials.push(draw.material);
        }
        let mut dirty = vec![];
        let mut material_dirty = vec![];
        for batch in batches {
            for glyph in &batch.glyphs {
                self.tile(renderer, glyph, &mut stats)?;
            }
            let mut instances = vec![];
            let mut runs: Vec<(usize, Range<u32>)> = vec![];
            let previous = self.draws.remove(&batch.id);
            let material = previous.as_ref().map_or_else(
                || {
                    self.free_materials.pop().unwrap_or_else(|| {
                        let slot = self.colors.len() as u32;
                        self.colors.push([0.0; 4]);
                        slot
                    })
                },
                |draw| draw.material,
            );
            let color = [
                ((batch.color >> 24) & 255) as f32 / 255.0,
                ((batch.color >> 16) & 255) as f32 / 255.0,
                ((batch.color >> 8) & 255) as f32 / 255.0,
                (batch.color & 255) as f32 / 255.0,
            ];
            if previous.is_none() || self.colors[material as usize] != color {
                self.colors[material as usize] = color;
                material_dirty.push(material..material + 1);
                stats.changed.insert(batch.id);
            }
            for glyph in &batch.glyphs {
                let tile = &self.tiles[&glyph.resource];
                let index = instances.len() as u32;
                instances.push(Instance {
                    bounds: glyph.bounds,
                    clip: batch.clip,
                    tile: tile.bounds,
                    material: [material, 0, 0, 0],
                });
                if let Some((page, range)) = runs.last_mut()
                    && *page == tile.page
                {
                    range.end += 1;
                } else {
                    runs.push((tile.page, index..index + 1));
                }
            }
            let resources: Vec<_> = batch.glyphs.iter().map(|glyph| glyph.resource).collect();
            let needed = (instances.len() as u32).max(1).next_power_of_two();
            let (start, capacity, changed) = if let Some(previous) = previous {
                if previous.capacity >= needed {
                    (
                        previous.start,
                        previous.capacity,
                        previous.instances != instances
                            || previous.runs != runs
                            || previous.resources != resources,
                    )
                } else {
                    self.free
                        .push(previous.start..previous.start + previous.capacity);
                    let start = self.allocate(needed);
                    (start, needed, true)
                }
            } else {
                let start = self.allocate(needed);
                (start, needed, true)
            };
            if changed {
                stats.changed.insert(batch.id);
                if !instances.is_empty() {
                    self.mirror[start as usize..start as usize + instances.len()]
                        .copy_from_slice(&instances);
                    dirty.push(start..start + instances.len() as u32);
                }
            }
            self.draws.insert(
                batch.id,
                Draw {
                    start,
                    capacity,
                    instances,
                    runs,
                    resources,
                    material,
                },
            );
        }
        let grow_instances = self.mirror.len() as u64 * 64 > self.buffer.size();
        let grow_materials = self.colors.len() as u64 * 16 > self.materials.size();
        if grow_instances {
            anyhow::ensure!(
                self.mirror.len() as u64 * 64
                    <= u64::from(
                        renderer
                            .context
                            .device
                            .limits()
                            .max_storage_buffer_binding_size
                    ),
                "fragmented glyph buffer limit"
            );
            self.buffer = Self::buffer(&renderer.context.device, self.mirror.len());
            dirty = vec![0..self.mirror.len() as u32];
        }
        if grow_materials {
            let capacity = self.colors.len().next_power_of_two();
            anyhow::ensure!(
                capacity as u64 * 16
                    <= u64::from(
                        renderer
                            .context
                            .device
                            .limits()
                            .max_storage_buffer_binding_size
                    ),
                "fragmented glyph material limit"
            );
            self.materials = Self::material_buffer(&renderer.context.device, capacity);
            material_dirty = vec![0..self.colors.len() as u32];
        }
        if grow_instances || grow_materials {
            for page in self.pages.iter_mut().flatten() {
                page.binding = Self::binding(
                    &renderer.context.device,
                    &self.layout,
                    &self.buffer,
                    &self.viewport,
                    &self.materials,
                    &page.view,
                );
            }
        }
        for range in merge_ranges(dirty) {
            renderer.context.queue.write_buffer(
                &self.buffer,
                u64::from(range.start) * 64,
                bytemuck::cast_slice(&self.mirror[range.start as usize..range.end as usize]),
            );
            stats.instance_bytes += u64::from(range.end - range.start) * 64;
            stats.ranges += 1;
        }
        for range in merge_ranges(material_dirty) {
            renderer.context.queue.write_buffer(
                &self.materials,
                u64::from(range.start) * 16,
                bytemuck::cast_slice(&self.colors[range.start as usize..range.end as usize]),
            );
            stats.material_bytes += u64::from(range.end - range.start) * 16;
            stats.material_ranges += 1;
        }
        if self.size != Some(size) {
            renderer.context.queue.write_buffer(
                &self.viewport,
                0,
                bytemuck::cast_slice(&[size.width.0 as f32, size.height.0 as f32, 0.0, 0.0]),
            );
            self.size = Some(size);
            stats.changed.extend(self.draws.keys());
        }
        Ok(stats)
    }
    fn allocate(&mut self, capacity: u32) -> u32 {
        self.free.sort_by_key(|range| range.start);
        let mut merged: Vec<Range<u32>> = vec![];
        for range in self.free.drain(..) {
            if let Some(last) = merged.last_mut()
                && last.end == range.start
            {
                last.end = range.end;
            } else {
                merged.push(range);
            }
        }
        self.free = merged;
        if let Some(index) = self
            .free
            .iter()
            .position(|range| range.end - range.start >= capacity)
        {
            let start = self.free[index].start;
            self.free[index].start += capacity;
            if self.free[index].is_empty() {
                self.free.remove(index);
            }
            return start;
        }
        let start = self.mirror.len() as u32;
        self.mirror
            .resize(start as usize + capacity as usize, Instance::default());
        start
    }
    pub fn draw(&self, id: GlyphDrawId, pass: &mut wgpu::RenderPass<'_>) -> Result<usize> {
        let draw = self.draws.get(&id).context("missing glyph batch")?;
        pass.set_pipeline(&self.pipeline);
        for (page, range) in &draw.runs {
            pass.set_bind_group(
                0,
                &self.pages[*page]
                    .as_ref()
                    .context("missing glyph page")?
                    .binding,
                &[],
            );
            pass.draw(0..6, range.start + draw.start..range.end + draw.start);
        }
        Ok(draw.runs.len())
    }
    pub fn draw_calls(&self, id: GlyphDrawId) -> usize {
        self.draws.get(&id).map_or(0, |draw| draw.runs.len())
    }
    pub fn leases(&self) -> GlyphLease {
        (
            self.buffer.clone(),
            self.viewport.clone(),
            self.materials.clone(),
            self.pages
                .iter()
                .flatten()
                .map(|page| (page.texture.clone(), page.binding.clone()))
                .collect(),
        )
    }
}
fn merge_ranges(mut dirty: Vec<Range<u32>>) -> Vec<Range<u32>> {
    dirty.sort_by_key(|range| range.start);
    let mut merged: Vec<Range<u32>> = vec![];
    for range in dirty {
        if let Some(last) = merged.last_mut()
            && range.start <= last.end
        {
            last.end = last.end.max(range.end);
        } else {
            merged.push(range);
        }
    }
    merged
}
fn valid_rect(rect: [f32; 4]) -> bool {
    rect.iter().all(|n| n.is_finite())
        && rect[2] >= 0.0
        && rect[3] >= 0.0
        && (rect[0] + rect[2]).is_finite()
        && (rect[1] + rect[3]).is_finite()
}
