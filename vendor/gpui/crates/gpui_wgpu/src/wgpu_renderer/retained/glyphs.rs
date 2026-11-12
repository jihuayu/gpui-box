//! Packed R8 pages, 48-byte local glyph records and independent node properties.
use super::*;
use std::collections::{BTreeMap, BTreeSet};
pub type GlyphDrawId = [u64; 3];
#[derive(Debug)]
pub struct RetainedGlyph {
    pub resource: u64,
    pub size: [u32; 2],
    pub mask: Arc<[u8]>,
    pub origin: [f32; 2],
    pub offset: [i32; 2],
}
/// Validated immutable local geometry. Clones retain exact glyph/mask identity;
/// mutable node origin, clip and color belong to GlyphBatch instead.
#[derive(Debug)]
pub struct GlyphGeometry {
    glyphs: Vec<RetainedGlyph>,
    unique: BTreeMap<u64, usize>,
    min_origin: [f32; 2],
    max_origin: [f32; 2],
    max_edge: u32,
}
impl GlyphGeometry {
    pub fn new(glyphs: Vec<RetainedGlyph>) -> Result<Arc<Self>> {
        let mut unique = BTreeMap::<u64, usize>::new();
        let mut min_origin = [f32::INFINITY; 2];
        let mut max_origin = [f32::NEG_INFINITY; 2];
        let mut max_edge = 0;
        for (index, glyph) in glyphs.iter().enumerate() {
            anyhow::ensure!(
                glyph.size.iter().all(|n| *n > 0 && *n <= 1024),
                "invalid glyph dimensions"
            );
            anyhow::ensure!(
                glyph.mask.len() as u64 == u64::from(glyph.size[0]) * u64::from(glyph.size[1]),
                "invalid gray glyph bytes"
            );
            anyhow::ensure!(
                glyph.origin.iter().all(|n| n.is_finite()),
                "invalid local glyph origin"
            );
            if let Some(old) = unique.insert(glyph.resource, index) {
                let old = &glyphs[old];
                anyhow::ensure!(
                    old.size == glyph.size && Arc::ptr_eq(&old.mask, &glyph.mask),
                    "mutable glyph resource"
                );
            }
            for axis in 0..2 {
                min_origin[axis] = min_origin[axis].min(glyph.origin[axis]);
                max_origin[axis] = max_origin[axis].max(glyph.origin[axis]);
            }
            max_edge = max_edge.max(glyph.size[0]).max(glyph.size[1]);
        }
        if glyphs.is_empty() {
            min_origin = [0.0; 2];
            max_origin = [0.0; 2];
        }
        Ok(Arc::new(Self {
            glyphs,
            unique,
            min_origin,
            max_origin,
            max_edge,
        }))
    }
}
pub struct GlyphBatch {
    pub id: GlyphDrawId,
    pub clip: [f32; 4],
    pub color: u32,
    pub origin: [f32; 2],
    pub scale: f32,
    pub geometry: Arc<GlyphGeometry>,
}
/// Base-relative resource updates; reset supplies the complete live draw set.
#[derive(Default)]
pub struct GlyphUpdates {
    pub reset: bool,
    pub removed: Vec<GlyphDrawId>,
    pub batches: Vec<GlyphBatch>,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
#[repr(C)]
struct Instance {
    bounds: [f32; 4],
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
    references: usize,
}
struct Draw {
    start: u32,
    capacity: u32,
    runs: Vec<(usize, Range<u32>)>,
    geometry: Arc<GlyphGeometry>,
    material: u32,
}
pub(super) type GlyphLease = (
    wgpu::Buffer,
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
    pub property_bytes: u64,
    pub property_ranges: usize,
    pub batch_updates: usize,
    pub rebuilt_glyphs: usize,
    pub validated_glyphs: usize,
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
    properties: wgpu::Buffer,
    property_records: Vec<[f32; 8]>,
    viewport: wgpu::Buffer,
    mirror: Vec<Instance>,
    free: Vec<Range<u32>>,
    layout: wgpu::BindGroupLayout,
    pipeline: wgpu::RenderPipeline,
    edge: u32,
    size: Option<Size<DevicePixels>>,
    glyph_count: usize,
    mask_bytes: u64,
}
impl GlyphCache {
    pub fn new(renderer: &WgpuHeadlessRenderer, camera_layout: &wgpu::BindGroupLayout) -> Self {
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
                        min_binding_size: NonZeroU64::new(48),
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
                wgpu::BindGroupLayoutEntry {
                    binding: 4,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: NonZeroU64::new(32),
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
            bind_group_layouts: &[Some(&layout), Some(camera_layout)],
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
        let properties = Self::property_buffer(device, 1);
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
            properties,
            property_records: vec![[0.0; 8]],
            viewport,
            mirror: vec![Instance::default()],
            free: vec![],
            layout,
            pipeline,
            edge,
            size: None,
            glyph_count: 0,
            mask_bytes: 0,
        }
    }
    fn buffer(device: &wgpu::Device, capacity: usize) -> wgpu::Buffer {
        device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("retained_glyph_slots"),
            size: capacity as u64 * 48,
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
    fn property_buffer(device: &wgpu::Device, capacity: usize) -> wgpu::Buffer {
        device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("retained_glyph_properties"),
            size: capacity as u64 * 32,
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
        properties: &wgpu::Buffer,
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
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: properties.as_entire_binding(),
                },
            ],
        })
    }
    fn removals(&self, updates: &GlyphUpdates) -> BTreeSet<GlyphDrawId> {
        if updates.reset {
            let live: BTreeSet<_> = updates.batches.iter().map(|batch| batch.id).collect();
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
        updates: &GlyphUpdates,
        max_bytes: u64,
    ) -> Result<(BTreeSet<GlyphDrawId>, usize)> {
        anyhow::ensure!(
            !updates.reset || updates.removed.is_empty(),
            "reset with explicit removals"
        );
        let removals = self.removals(updates);
        anyhow::ensure!(
            updates.reset || removals.len() == updates.removed.len(),
            "duplicate glyph removal"
        );
        let mut ids: BTreeSet<_> = self
            .draws
            .keys()
            .filter(|id| !removals.contains(*id))
            .copied()
            .collect();
        let mut writes = BTreeSet::new();
        let mut count = self.glyph_count as i64;
        let mut changes = BTreeMap::<u64, i64>::new();
        let mut definitions = BTreeMap::<u64, &RetainedGlyph>::new();
        let mut validated = 0;
        let mut release = |geometry: &GlyphGeometry| {
            count -= geometry.glyphs.len() as i64;
            for id in geometry.unique.keys() {
                *changes.entry(*id).or_default() -= 1;
            }
        };
        for id in &removals {
            if let Some(draw) = self.draws.get(id) {
                release(&draw.geometry);
            }
        }
        // Accumulate replacements before additions to retain shared mask identities.
        for batch in &updates.batches {
            anyhow::ensure!(
                writes.insert(batch.id) && !removals.contains(&batch.id),
                "duplicate/conflicting glyph update"
            );
            if let Some(old) = self.draws.get(&batch.id)
                && !Arc::ptr_eq(&old.geometry, &batch.geometry)
            {
                release(&old.geometry);
            }
        }
        for batch in &updates.batches {
            anyhow::ensure!(valid_rect(batch.clip), "invalid glyph clip");
            anyhow::ensure!(
                batch.origin.iter().all(|n| n.is_finite())
                    && batch.scale.is_finite()
                    && batch.scale > 0.0,
                "invalid glyph properties"
            );
            let geometry = &batch.geometry;
            anyhow::ensure!(geometry.max_edge <= self.edge, "glyph too large for page");
            for axis in 0..2 {
                for local in [geometry.min_origin[axis], geometry.max_origin[axis]] {
                    // Integer bitmap offsets are bounded by i32, so adding them
                    // cannot overflow a finite f32 baseline at this magnitude.
                    anyhow::ensure!(
                        ((local + batch.origin[axis]) * batch.scale).is_finite(),
                        "invalid glyph bounds"
                    );
                }
            }
            let same = self
                .draws
                .get(&batch.id)
                .is_some_and(|old| Arc::ptr_eq(&old.geometry, geometry));
            if !same {
                validated += geometry.glyphs.len();
                count += geometry.glyphs.len() as i64;
                for (id, index) in &geometry.unique {
                    let glyph = &geometry.glyphs[*index];
                    *changes.entry(*id).or_default() += 1;
                    if let Some(old) = definitions.insert(*id, glyph) {
                        anyhow::ensure!(
                            old.size == glyph.size && Arc::ptr_eq(&old.mask, &glyph.mask),
                            "mutable glyph resource"
                        );
                    }
                    if let Some(old) = self.tiles.get(id) {
                        anyhow::ensure!(
                            old.bounds[2..] == glyph.size && Arc::ptr_eq(&old.pixels, &glyph.mask),
                            "mutable cached glyph"
                        );
                    }
                }
            }
            ids.insert(batch.id);
        }
        let mut bytes = self.mask_bytes as i64;
        for (id, change) in changes {
            let old = self.tiles.get(&id);
            let before = old.map_or(0, |tile| tile.references as i64);
            let after = before + change;
            anyhow::ensure!(after >= 0, "glyph resource reference underflow");
            if before == 0 && after > 0 {
                bytes += definitions[&id].mask.len() as i64;
            }
            if before > 0 && after == 0 {
                bytes -= old.unwrap().pixels.len() as i64;
            }
        }
        anyhow::ensure!(
            count >= 0 && count as u64 * 48 <= max_bytes / 2,
            "glyph instance budget exceeded"
        );
        anyhow::ensure!(
            ids.len() as u64 * 32 <= max_bytes / 2,
            "glyph material budget exceeded"
        );
        anyhow::ensure!(
            (0..=32 * 1024 * 1024).contains(&bytes),
            "live glyph mask budget exceeded"
        );
        Ok((ids, validated))
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
                &self.properties,
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
                references: 0,
            },
        );
        stats.atlas_bytes += glyph.mask.len() as u64;
        stats.atlas_uploads += 1;
        Ok(())
    }
    fn release(&mut self, geometry: &GlyphGeometry, touched: &mut BTreeSet<u64>) {
        self.glyph_count -= geometry.glyphs.len();
        for id in geometry.unique.keys() {
            self.tiles
                .get_mut(id)
                .expect("retained glyph tile")
                .references -= 1;
            touched.insert(*id);
        }
    }
    pub fn prepare(
        &mut self,
        renderer: &WgpuHeadlessRenderer,
        updates: GlyphUpdates,
        size: Size<DevicePixels>,
    ) -> Result<GlyphStats> {
        let mut stats = GlyphStats {
            batch_updates: updates.batches.len(),
            ..Default::default()
        };
        let mut touched = BTreeSet::new();
        for id in self.removals(&updates) {
            if let Some(draw) = self.draws.remove(&id) {
                self.release(&draw.geometry, &mut touched);
                self.free.push(draw.start..draw.start + draw.capacity);
                self.free_materials.push(draw.material);
            }
        }
        for batch in &updates.batches {
            if let Some(draw) = self.draws.get(&batch.id)
                && !Arc::ptr_eq(&draw.geometry, &batch.geometry)
            {
                let geometry = draw.geometry.clone();
                self.release(&geometry, &mut touched);
            }
        }
        // Protect reused resources from eviction before packing new masks.
        let mut additions = BTreeMap::<u64, (usize, &RetainedGlyph)>::new();
        for batch in &updates.batches {
            if self
                .draws
                .get(&batch.id)
                .is_some_and(|old| Arc::ptr_eq(&old.geometry, &batch.geometry))
            {
                continue;
            }
            for (id, index) in &batch.geometry.unique {
                let entry = additions
                    .entry(*id)
                    .or_insert((0, &batch.geometry.glyphs[*index]));
                entry.0 += 1;
            }
        }
        for (id, (references, _)) in &additions {
            if let Some(tile) = self.tiles.get_mut(id) {
                tile.references += references;
            }
        }
        let dead: Vec<_> = touched
            .into_iter()
            .filter(|id| self.tiles[id].references == 0)
            .collect();
        let reclaim_pages = !dead.is_empty();
        for id in dead {
            let tile = self.tiles.remove(&id).unwrap();
            self.mask_bytes -= tile.pixels.len() as u64;
            self.pages[tile.page]
                .as_mut()
                .unwrap()
                .allocator
                .deallocate(tile.allocation);
        }
        if reclaim_pages {
            let used: BTreeSet<_> = self.tiles.values().map(|tile| tile.page).collect();
            for (index, page) in self.pages.iter_mut().enumerate() {
                if !used.contains(&index) {
                    *page = None;
                }
            }
        }
        for (id, (references, glyph)) in additions {
            if !self.tiles.contains_key(&id) {
                self.tile(renderer, glyph, &mut stats)?;
                self.tiles.get_mut(&id).unwrap().references = references;
                self.mask_bytes += glyph.mask.len() as u64;
            }
        }
        let mut dirty = vec![];
        let mut material_dirty = vec![];
        let mut property_dirty = vec![];
        for batch in updates.batches {
            let previous = self.draws.remove(&batch.id);
            let material = previous.as_ref().map_or_else(
                || {
                    self.free_materials.pop().unwrap_or_else(|| {
                        let slot = self.colors.len() as u32;
                        self.colors.push([0.0; 4]);
                        self.property_records.push([0.0; 8]);
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
            let properties = [
                batch.origin[0],
                batch.origin[1],
                batch.scale,
                0.0,
                batch.clip[0],
                batch.clip[1],
                batch.clip[2],
                batch.clip[3],
            ];
            if previous.is_none() || self.property_records[material as usize] != properties {
                self.property_records[material as usize] = properties;
                property_dirty.push(material..material + 1);
                stats.changed.insert(batch.id);
            }
            if previous
                .as_ref()
                .is_some_and(|old| Arc::ptr_eq(&old.geometry, &batch.geometry))
            {
                self.draws.insert(batch.id, previous.unwrap());
                continue;
            }
            stats.rebuilt_glyphs += batch.geometry.glyphs.len();
            self.glyph_count += batch.geometry.glyphs.len();
            let mut instances = vec![];
            let mut runs: Vec<(usize, Range<u32>)> = vec![];
            for glyph in &batch.geometry.glyphs {
                let tile = &self.tiles[&glyph.resource];
                let index = instances.len() as u32;
                instances.push(Instance {
                    bounds: [
                        glyph.origin[0],
                        glyph.origin[1],
                        glyph.offset[0] as f32,
                        glyph.offset[1] as f32,
                    ],
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
            let needed = (instances.len() as u32).max(1).next_power_of_two();
            let (start, capacity) = if let Some(previous) = previous {
                if previous.capacity >= needed {
                    (previous.start, previous.capacity)
                } else {
                    self.free
                        .push(previous.start..previous.start + previous.capacity);
                    (self.allocate(needed), needed)
                }
            } else {
                (self.allocate(needed), needed)
            };
            stats.changed.insert(batch.id);
            if !instances.is_empty() {
                self.mirror[start as usize..start as usize + instances.len()]
                    .copy_from_slice(&instances);
                dirty.push(start..start + instances.len() as u32);
            }
            self.draws.insert(
                batch.id,
                Draw {
                    start,
                    capacity,
                    runs,
                    geometry: batch.geometry,
                    material,
                },
            );
        }
        let grow_instances = self.mirror.len() as u64 * 48 > self.buffer.size();
        let grow_materials = self.colors.len() as u64 * 16 > self.materials.size();
        if grow_instances {
            anyhow::ensure!(
                self.mirror.len() as u64 * 48
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
                capacity as u64 * 32
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
            self.properties = Self::property_buffer(&renderer.context.device, capacity);
            material_dirty = vec![0..self.colors.len() as u32];
            property_dirty = vec![0..self.property_records.len() as u32];
        }
        if grow_instances || grow_materials {
            for page in self.pages.iter_mut().flatten() {
                page.binding = Self::binding(
                    &renderer.context.device,
                    &self.layout,
                    &self.buffer,
                    &self.viewport,
                    &self.materials,
                    &self.properties,
                    &page.view,
                );
            }
        }
        for range in merge_ranges(dirty) {
            renderer.context.queue.write_buffer(
                &self.buffer,
                u64::from(range.start) * 48,
                bytemuck::cast_slice(&self.mirror[range.start as usize..range.end as usize]),
            );
            stats.instance_bytes += u64::from(range.end - range.start) * 48;
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
        for range in merge_ranges(property_dirty) {
            renderer.context.queue.write_buffer(
                &self.properties,
                u64::from(range.start) * 32,
                bytemuck::cast_slice(
                    &self.property_records[range.start as usize..range.end as usize],
                ),
            );
            stats.property_bytes += u64::from(range.end - range.start) * 32;
            stats.property_ranges += 1;
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
            self.properties.clone(),
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
