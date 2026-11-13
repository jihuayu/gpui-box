//! Bounded isolated layers. Coordinates remain in physical surface space.
use super::*;
use std::collections::{BTreeMap, BTreeSet};
mod translation;

pub type QuadLayerId = [u64; 3];

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum QuadDraw {
    Instances(Range<u32>),
    Shadows(Range<u32>),
    Layer(QuadLayerId),
    Image(QuadImageId),
    Glyphs(GlyphDrawId),
}

pub struct QuadLayer {
    pub id: QuadLayerId,
    pub opacity: f32,
    /// Physical x/y/width/height; None keeps the full-viewport control path.
    pub bounds: Option<[u32; 4]>,
    pub draws: Arc<[QuadDraw]>,
}

pub(super) struct Target {
    _camera_buffer: wgpu::Buffer,
    camera: wgpu::BindGroup,
    _globals_buffer: wgpu::Buffer,
    globals: wgpu::BindGroup,
}

struct Layer {
    bounds: [u32; 4],
    target: Target,
    texture: wgpu::Texture,
    view: wgpu::TextureView,
    opacity_buffer: wgpu::Buffer,
    binding: wgpu::BindGroup,
    opacity: f32,
    draws: Arc<[QuadDraw]>,
    valid: bool,
    translation_content: Option<Vec<u8>>,
    _memory: Arc<allocation_budget::Allocation>,
}

/// Unique isolated texture payload held by the cache and completion leases.
/// Excludes driver alignment, buffers, the main target, images and glyphs.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LayerMemoryStats {
    pub limit_bytes: u64,
    pub live_bytes: u64,
    pub resident_bytes: u64,
    pub retired_bytes: u64,
    pub peak_resident_bytes: u64,
    pub resident_allocations: u64,
}

#[derive(Default)]
pub(super) struct LayerStats {
    pub rasterized: usize,
    pub reused: usize,
    pub draws: usize,
    pub property_bytes: u64,
    pub live_bytes: u64,
    pub allocated_bytes: u64,
    pub rasterized_pixels: u64,
    pub allocations: usize,
    pub rebased: usize,
    pub target_bytes: u64,
}

pub(super) type TargetLease = (wgpu::Buffer, wgpu::BindGroup, wgpu::Buffer, wgpu::BindGroup);

pub(super) type LayerLease = (
    wgpu::Texture, wgpu::Buffer, wgpu::BindGroup,
    wgpu::Buffer, wgpu::BindGroup, wgpu::Buffer, wgpu::BindGroup,
    Arc<allocation_budget::Allocation>,
);

pub(super) struct LayerCache {
    layers: BTreeMap<QuadLayerId, Layer>,
    layout: wgpu::BindGroupLayout,
    camera_layout: wgpu::BindGroupLayout,
    root: Option<Target>,
    pipeline: wgpu::RenderPipeline,
    budget: allocation_budget::AllocationBudget,
    limit_bytes: u64,
    live_bytes: u64,
    pub reuse_textures: bool,
    pub reuse_translations: bool,
}

impl LayerCache {
    pub fn new(renderer: &WgpuHeadlessRenderer) -> Self {
        let device = &renderer.context.device;
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("retained_layer_layout"),
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
                        min_binding_size: NonZeroU64::new(16),
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: NonZeroU64::new(16),
                    },
                    count: None,
                },
            ],
        });
        let camera_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("retained_target_camera_layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: NonZeroU64::new(16),
                },
                count: None,
            }],
        });
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("retained_layer_composite"),
            source: wgpu::ShaderSource::Wgsl(include_str!("layers.wgsl").into()),
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("retained_layer_pipeline_layout"),
            bind_group_layouts: &[Some(&layout), Some(&camera_layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("retained_layer_pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_layer"),
                buffers: &[],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_layer"),
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
            layers: BTreeMap::new(),
            layout,
            camera_layout,
            root: None,
            pipeline,
            budget: allocation_budget::AllocationBudget::default(),
            limit_bytes: 256 * 1024 * 1024,
            live_bytes: 0,
            reuse_textures: true,
            reuse_translations: false,
        }
    }

    pub fn clear(&mut self) {
        self.layers.clear();
        self.root = None;
        self.live_bytes = 0;
    }

    pub fn memory(&self) -> LayerMemoryStats {
        let resident = self.budget.bytes();
        LayerMemoryStats {
            limit_bytes: self.limit_bytes,
            live_bytes: self.live_bytes,
            resident_bytes: resident,
            retired_bytes: resident.saturating_sub(self.live_bytes),
            peak_resident_bytes: self.budget.peak(),
            resident_allocations: self.budget.count(),
        }
    }

    /// Caller completes and collects all flights before resetting this epoch.
    pub fn configure_budget(&mut self, bytes: u64) {
        self.clear();
        self.budget = allocation_budget::AllocationBudget::default();
        self.limit_bytes = bytes;
    }

    /// Drop stale cache ownership before considering a new allocation. Flights
    /// retain their own shared reservations until the owner collects completion.
    pub fn prepare(&mut self, layers: &[QuadLayer], size: Size<DevicePixels>, reset: bool) -> u64 {
        if reset { self.clear(); }
        let desired: BTreeMap<_, _> = layers.iter().map(|layer| (layer.id, Self::bounds(layer, size))).collect();
        self.layers.retain(|id, layer| {
            let keep = desired.get(id).is_some_and(|bounds| {
                *bounds == layer.bounds || (self.reuse_textures && bounds[2..] == layer.bounds[2..])
            });
            if !keep { self.live_bytes -= Self::bytes(layer.bounds); }
            keep
        });
        desired.iter().filter(|(id, _)| !self.layers.contains_key(*id))
            .map(|(_, bounds)| Self::bytes(*bounds)).sum()
    }

    fn bytes(bounds: [u32; 4]) -> u64 {
        u64::from(bounds[2].max(1)) * u64::from(bounds[3].max(1)) * 4
    }

    pub fn validate(
        layers: &[QuadLayer],
        order: &[QuadDraw],
        capacity: u32,
        shadow_capacity: u32,
        size: Size<DevicePixels>,
        images: &BTreeSet<QuadImageId>,
        glyphs: &BTreeSet<GlyphDrawId>,
        limit_bytes: u64,
    ) -> Result<()> {
        let mut seen = BTreeSet::new();
        let mut bytes = 0u64;
        for layer in layers {
            anyhow::ensure!(
                layer.opacity.is_finite() && (0.0..=1.0).contains(&layer.opacity),
                "invalid group opacity"
            );
            let [x, y, width, height] = Self::bounds(layer, size);
            anyhow::ensure!(
                x.checked_add(width).is_some_and(|end| end <= size.width.0 as u32)
                    && y.checked_add(height).is_some_and(|end| end <= size.height.0 as u32),
                "invalid isolated layer bounds"
            );
            bytes = bytes.checked_add(u64::from(width.max(1)) * u64::from(height.max(1)) * 4)
                .context("layer payload overflow")?;
            Self::validate_draws(&layer.draws, &seen, capacity, shadow_capacity, images, glyphs)?;
            anyhow::ensure!(seen.insert(layer.id), "duplicate or cyclic layer");
        }
        Self::validate_draws(order, &seen, capacity, shadow_capacity, images, glyphs)?;
        anyhow::ensure!(
            bytes <= limit_bytes,
            "isolated layers exceed {} MiB budget ({} bytes required; {} byte limit)",
            limit_bytes / (1024 * 1024), bytes, limit_bytes
        );
        Ok(())
    }

    fn validate_draws(
        draws: &[QuadDraw],
        seen: &BTreeSet<QuadLayerId>,
        capacity: u32,
        shadow_capacity: u32,
        images: &BTreeSet<QuadImageId>,
        glyphs: &BTreeSet<GlyphDrawId>,
    ) -> Result<()> {
        for draw in draws {
            match draw {
                QuadDraw::Instances(range) => anyhow::ensure!(
                    range.start < range.end && range.end <= capacity,
                    "draw outside allocation"
                ),
                QuadDraw::Shadows(range) => anyhow::ensure!(range.start < range.end && range.end <= shadow_capacity,
                    "shadow draw outside allocation"),
                QuadDraw::Layer(id) => {
                    anyhow::ensure!(seen.contains(id), "layer must precede its consumer")
                }
                QuadDraw::Image(id) => {
                    anyhow::ensure!(images.contains(id), "missing decoded image")
                }
                QuadDraw::Glyphs(id) => anyhow::ensure!(glyphs.contains(id), "missing glyph batch"),
            }
        }
        Ok(())
    }

    pub fn camera_layout(&self) -> &wgpu::BindGroupLayout {
        &self.camera_layout
    }

    fn bounds(layer: &QuadLayer, size: Size<DevicePixels>) -> [u32; 4] {
        layer.bounds.unwrap_or([0, 0, size.width.0 as u32, size.height.0 as u32])
    }

    fn target_values(bounds: [u32; 4]) -> ([f32; 4], GlobalParams) {
        let values = [bounds[0] as f32, bounds[1] as f32,
            bounds[2].max(1) as f32, bounds[3].max(1) as f32];
        (values, GlobalParams {
            viewport_size: [values[2], values[3]], viewport_origin: [values[0], values[1]],
            premultiplied_alpha: 0, pad: 0, origin_pad: [0; 2],
        })
    }

    fn write_target(renderer: &WgpuHeadlessRenderer, target: &Target, bounds: [u32; 4]) {
        let (camera, globals) = Self::target_values(bounds);
        // Each target is written once, before this frame's encoder is submitted.
        // Prior submitted reads precede these writes on the same WGPU queue.
        renderer.context.queue.write_buffer(&target._camera_buffer, 0, bytemuck::cast_slice(&camera));
        renderer.context.queue.write_buffer(&target._globals_buffer, 0, bytemuck::bytes_of(&globals));
    }

    fn target(&self, renderer: &WgpuHeadlessRenderer, bounds: [u32; 4]) -> Target {
        use wgpu::util::DeviceExt;
        let core = &renderer.core;
        let device = &renderer.context.device;
        let (camera_values, globals) = Self::target_values(bounds);
        let camera_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("retained_target_camera"),
            contents: bytemuck::cast_slice(&camera_values),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });
        let camera = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("retained_target_camera_binding"),
            layout: &self.camera_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0, resource: camera_buffer.as_entire_binding(),
            }],
        });
        let globals_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("retained_target_globals"),
            contents: bytemuck::bytes_of(&globals),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });
        let globals_binding = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("retained_target_globals_binding"),
            layout: &core.resources.bind_group_layouts.globals,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: globals_buffer.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                    buffer: &core.resources.globals_buffer, offset: core.gamma_offset,
                    size: NonZeroU64::new(std::mem::size_of::<GammaParams>() as u64),
                }) },
            ],
        });
        Target { _camera_buffer: camera_buffer, camera,
            _globals_buffer: globals_buffer, globals: globals_binding }
    }

    fn allocate(&self, renderer: &WgpuHeadlessRenderer, bounds: [u32; 4]) -> Result<Layer> {
        let memory = self.budget.reserve(Self::bytes(bounds), self.limit_bytes)
            .context("layer allocation exceeds admitted budget")?;
        let device = &renderer.context.device;
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("retained_isolated_layer"),
            size: wgpu::Extent3d {
                width: bounds[2].max(1),
                height: bounds[3].max(1),
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: renderer.core.target_format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let view = texture.create_view(&Default::default());
        let opacity_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("retained_layer_opacity"),
            size: 16,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let target = self.target(renderer, bounds);
        let binding = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("retained_layer_binding"),
            layout: &self.layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: opacity_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: target._camera_buffer.as_entire_binding(),
                },
            ],
        });
        Ok(Layer {
            bounds,
            target,
            texture,
            view,
            opacity_buffer,
            binding,
            opacity: -1.0,
            draws: Arc::from([]),
            valid: false,
            translation_content: None,
            _memory: memory,
        })
    }

    pub fn render(
        &mut self,
        renderer: &WgpuHeadlessRenderer,
        binding: &InstanceBinding,
        shadows: &InstanceBinding,
        spatial: &spatial::SpatialCache,
        quad_storage: &PersistentStorage<Quad>,
        shadow_storage: &PersistentStorage<gpui::Shadow>,
        encoder: &mut wgpu::CommandEncoder,
        layers: Vec<QuadLayer>,
        writes: &[Range<u32>],
        shadow_writes: &[Range<u32>],
        size: Size<DevicePixels>,
        cache: bool,
        images: &images::ImageCache,
        changed_images: &BTreeSet<QuadImageId>,
        glyphs: &glyphs::GlyphCache,
        changed_glyphs: &BTreeSet<GlyphDrawId>,
    ) -> Result<LayerStats> {
        let mut stats = LayerStats::default();
        if self.root.is_none() {
            self.root = Some(self.target(renderer, [0, 0, size.width.0 as u32, size.height.0 as u32]));
        }
        let mut changed = BTreeSet::new();
        for layer in layers {
            let bounds = Self::bounds(&layer, size);
            let content = self.reuse_translations.then(|| translation::content(
                &layer.draws, bounds, spatial, quad_storage, shadow_storage,
            )).flatten();
            let translated = cache && self.layers.get(&layer.id).is_some_and(|old| {
                old.valid && old.bounds != bounds && old.bounds[2..] == bounds[2..]
                    && content.is_some() && old.translation_content == content
            });
            let bytes = u64::from(bounds[2].max(1)) * u64::from(bounds[3].max(1)) * 4;
            stats.live_bytes += bytes;
            if !self.layers.contains_key(&layer.id) {
                self.layers.insert(layer.id, self.allocate(renderer, bounds)?);
                self.live_bytes += bytes;
                stats.allocated_bytes += bytes;
                stats.allocations += 1;
                stats.target_bytes += 48;
            } else {
                let cached = self.layers.get_mut(&layer.id).unwrap();
                if cached.bounds != bounds {
                    anyhow::ensure!(cached.bounds[2..] == bounds[2..], "layer extent changed after admission");
                    Self::write_target(renderer, &cached.target, bounds);
                    cached.bounds = bounds;
                    if !translated { cached.valid = false; }
                    stats.rebased += 1;
                    stats.target_bytes += 48;
                }
            }
            let previous = &self.layers[&layer.id];
            let opacity_changed = previous.opacity != layer.opacity;
            let dirty = !cache
                || !previous.valid
                || (!Arc::ptr_eq(&previous.draws, &layer.draws) && previous.draws != layer.draws)
                || (!translated && layer.draws.iter().any(|draw| match draw {
                    QuadDraw::Instances(range) => writes
                        .iter()
                        .any(|write| range.start < write.end && write.start < range.end),
                    QuadDraw::Shadows(range) => shadow_writes.iter().any(|write|
                        range.start < write.end && write.start < range.end),
                    QuadDraw::Layer(id) => changed.contains(id),
                    QuadDraw::Image(id) => changed_images.contains(id),
                    QuadDraw::Glyphs(id) => changed_glyphs.contains(id),
                }));
            if opacity_changed {
                let mut bytes = [0; 16];
                bytes[..4].copy_from_slice(&layer.opacity.to_ne_bytes());
                renderer
                    .context
                    .queue
                    .write_buffer(&previous.opacity_buffer, 0, &bytes);
                stats.property_bytes += 16;
            }
            if dirty {
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("retained_layer_raster"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &previous.view,
                        resolve_target: None,
                        depth_slice: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    ..Default::default()
                });
                stats.draws += self.draw(
                    &renderer.core,
                    binding,
                    shadows,
                    spatial,
                    &layer.draws,
                    &mut pass,
                    images,
                    glyphs,
                    Some(&previous.target),
                )?;
                stats.rasterized += 1;
                stats.rasterized_pixels += u64::from(bounds[2].max(1)) * u64::from(bounds[3].max(1));
            } else {
                stats.reused += 1;
            }
            if dirty || opacity_changed || translated {
                changed.insert(layer.id);
            }
            let cached = self.layers.get_mut(&layer.id).unwrap();
            cached.draws = layer.draws;
            cached.opacity = layer.opacity;
            cached.valid = true;
            cached.translation_content = content;
        }
        Ok(stats)
    }

    pub fn draw(
        &self,
        core: &WgpuRendererCore,
        binding: &InstanceBinding,
        shadows: &InstanceBinding,
        spatial: &spatial::SpatialCache,
        draws: &[QuadDraw],
        pass: &mut wgpu::RenderPass<'_>,
        images: &images::ImageCache,
        glyphs: &glyphs::GlyphCache,
        target: Option<&Target>,
    ) -> Result<usize> {
        let target = target.or(self.root.as_ref()).context("missing render camera")?;
        let mut count = 0;
        for draw in draws {
            if let QuadDraw::Layer(id) = draw {
                let layer = self.layers.get(id).context("missing raster layer")?;
                if layer.bounds[2] == 0 || layer.bounds[3] == 0 {
                    continue;
                }
            }
            count += match draw {
                QuadDraw::Glyphs(id) => glyphs.draw_calls(*id),
                _ => 1,
            };
            match draw {
                QuadDraw::Instances(range) => {
                    pass.set_pipeline(if spatial.enabled { &spatial.quads } else { &core.resources.pipelines.quads });
                    if spatial.enabled { pass.set_bind_group(2, &spatial.binding, &[]); }
                    pass.set_bind_group(0, &target.globals, &[]);
                    pass.set_bind_group(1, &binding.bind_group, &[]);
                    pass.draw(0..4, binding.first_instance + range.start..binding.first_instance + range.end);
                },
                QuadDraw::Shadows(range) => {
                    pass.set_pipeline(if spatial.enabled { &spatial.shadows } else { &core.resources.pipelines.shadows });
                    if spatial.enabled { pass.set_bind_group(2, &spatial.binding, &[]); }
                    pass.set_bind_group(0, &target.globals, &[]);
                    pass.set_bind_group(1, &shadows.bind_group, &[]);
                    pass.draw(0..4, shadows.first_instance + range.start..shadows.first_instance + range.end);
                }
                QuadDraw::Layer(id) => {
                    let layer = self.layers.get(id).context("missing raster layer")?;
                    pass.set_pipeline(&self.pipeline);
                    pass.set_bind_group(0, &layer.binding, &[]);
                    pass.set_bind_group(1, &target.camera, &[]);
                    pass.draw(0..6, 0..1);
                }
                QuadDraw::Image(id) => {
                    pass.set_bind_group(2, &spatial.binding, &[]);
                    pass.set_bind_group(1, &target.camera, &[]);
                    images.draw(*id, pass)?;
                },
                QuadDraw::Glyphs(id) => {
                    pass.set_bind_group(2, &spatial.binding, &[]);
                    pass.set_bind_group(1, &target.camera, &[]);
                    glyphs.draw(*id, pass)?;
                }
            }
        }
        Ok(count)
    }

    pub fn root_lease(&self) -> Option<TargetLease> {
        self.root.as_ref().map(|target| (
            target._camera_buffer.clone(), target.camera.clone(),
            target._globals_buffer.clone(), target.globals.clone(),
        ))
    }

    pub fn leases(&self) -> Vec<LayerLease> {
        self.layers
            .values()
            .map(|layer| {
                (
                    layer.texture.clone(),
                    layer.opacity_buffer.clone(),
                    layer.binding.clone(),
                    layer.target._camera_buffer.clone(), layer.target.camera.clone(),
                    layer.target._globals_buffer.clone(), layer.target.globals.clone(),
                    layer._memory.clone(),
                )
            })
            .collect()
    }
}
