//! Full-viewport isolated layers. Contents are premultiplied by render blending.
use super::*;
use std::collections::{BTreeMap, BTreeSet};

pub type QuadLayerId = [u64; 3];

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum QuadDraw {
    Instances(Range<u32>),
    Layer(QuadLayerId),
    Image(QuadImageId),
}

pub struct QuadLayer {
    pub id: QuadLayerId,
    pub opacity: f32,
    pub draws: Vec<QuadDraw>,
}

struct Layer {
    texture: wgpu::Texture,
    view: wgpu::TextureView,
    opacity_buffer: wgpu::Buffer,
    binding: wgpu::BindGroup,
    opacity: f32,
    draws: Vec<QuadDraw>,
    valid: bool,
}

#[derive(Default)]
pub(super) struct LayerStats {
    pub rasterized: usize,
    pub reused: usize,
    pub draws: usize,
    pub property_bytes: u64,
}

pub(super) type LayerLease = (wgpu::Texture, wgpu::Buffer, wgpu::BindGroup);

pub(super) struct LayerCache {
    layers: BTreeMap<QuadLayerId, Layer>,
    layout: wgpu::BindGroupLayout,
    pipeline: wgpu::RenderPipeline,
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
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: NonZeroU64::new(16),
                    },
                    count: None,
                },
            ],
        });
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("retained_layer_composite"),
            source: wgpu::ShaderSource::Wgsl(include_str!("layers.wgsl").into()),
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("retained_layer_pipeline_layout"),
            bind_group_layouts: &[Some(&layout)],
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
            pipeline,
        }
    }

    pub fn clear(&mut self) {
        self.layers.clear();
    }

    pub fn validate(
        layers: &[QuadLayer],
        order: &[QuadDraw],
        capacity: u32,
        size: Size<DevicePixels>,
        images: &BTreeSet<QuadImageId>,
    ) -> Result<()> {
        let mut seen = BTreeSet::new();
        for layer in layers {
            anyhow::ensure!(
                layer.opacity.is_finite() && (0.0..=1.0).contains(&layer.opacity),
                "invalid group opacity"
            );
            Self::validate_draws(&layer.draws, &seen, capacity, images)?;
            anyhow::ensure!(seen.insert(layer.id), "duplicate or cyclic layer");
        }
        Self::validate_draws(order, &seen, capacity, images)?;
        let bytes = u64::from(size.width.0 as u32) * u64::from(size.height.0 as u32) * 4;
        anyhow::ensure!(
            bytes.saturating_mul(layers.len() as u64) <= 256 * 1024 * 1024,
            "isolated layers exceed 256 MiB budget"
        );
        Ok(())
    }

    fn validate_draws(
        draws: &[QuadDraw],
        seen: &BTreeSet<QuadLayerId>,
        capacity: u32,
        images: &BTreeSet<QuadImageId>,
    ) -> Result<()> {
        for draw in draws {
            match draw {
                QuadDraw::Instances(range) => anyhow::ensure!(
                    range.start < range.end && range.end <= capacity,
                    "draw outside allocation"
                ),
                QuadDraw::Layer(id) => {
                    anyhow::ensure!(seen.contains(id), "layer must precede its consumer")
                }
                QuadDraw::Image(id) => {
                    anyhow::ensure!(images.contains(id), "missing decoded image")
                }
            }
        }
        Ok(())
    }

    fn allocate(&self, renderer: &WgpuHeadlessRenderer, size: Size<DevicePixels>) -> Layer {
        let device = &renderer.context.device;
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("retained_isolated_layer"),
            size: wgpu::Extent3d {
                width: size.width.0 as u32,
                height: size.height.0 as u32,
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
            ],
        });
        Layer {
            texture,
            view,
            opacity_buffer,
            binding,
            opacity: -1.0,
            draws: vec![],
            valid: false,
        }
    }

    pub fn render(
        &mut self,
        renderer: &WgpuHeadlessRenderer,
        binding: &InstanceBinding,
        encoder: &mut wgpu::CommandEncoder,
        layers: Vec<QuadLayer>,
        writes: &[Range<u32>],
        size: Size<DevicePixels>,
        cache: bool,
        images: &images::ImageCache,
        changed_images: &BTreeSet<QuadImageId>,
    ) -> Result<LayerStats> {
        let mut stats = LayerStats::default();
        let mut changed = BTreeSet::new();
        let live: BTreeSet<_> = layers.iter().map(|layer| layer.id).collect();
        self.layers.retain(|id, _| live.contains(id));
        for layer in layers {
            if !self.layers.contains_key(&layer.id) {
                self.layers.insert(layer.id, self.allocate(renderer, size));
            }
            let previous = &self.layers[&layer.id];
            let opacity_changed = previous.opacity != layer.opacity;
            let dirty = !cache
                || !previous.valid
                || previous.draws != layer.draws
                || layer.draws.iter().any(|draw| match draw {
                    QuadDraw::Instances(range) => writes
                        .iter()
                        .any(|write| range.start < write.end && write.start < range.end),
                    QuadDraw::Layer(id) => changed.contains(id),
                    QuadDraw::Image(id) => changed_images.contains(id),
                });
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
                self.draw(&renderer.core, binding, &layer.draws, &mut pass, images)?;
                stats.draws += layer.draws.len();
                stats.rasterized += 1;
            } else {
                stats.reused += 1;
            }
            if dirty || opacity_changed {
                changed.insert(layer.id);
            }
            let cached = self.layers.get_mut(&layer.id).unwrap();
            cached.draws = layer.draws;
            cached.opacity = layer.opacity;
            cached.valid = true;
        }
        Ok(stats)
    }

    pub fn draw(
        &self,
        core: &WgpuRendererCore,
        binding: &InstanceBinding,
        draws: &[QuadDraw],
        pass: &mut wgpu::RenderPass<'_>,
        images: &images::ImageCache,
    ) -> Result<()> {
        for draw in draws {
            match draw {
                QuadDraw::Instances(range) => core.draw_instances(
                    binding,
                    &core.resources.pipelines.quads,
                    range.clone(),
                    pass,
                ),
                QuadDraw::Layer(id) => {
                    let layer = self.layers.get(id).context("missing raster layer")?;
                    pass.set_pipeline(&self.pipeline);
                    pass.set_bind_group(0, &layer.binding, &[]);
                    pass.draw(0..3, 0..1);
                }
                QuadDraw::Image(id) => images.draw(*id, pass)?,
            }
        }
        Ok(())
    }

    pub fn leases(&self) -> Vec<LayerLease> {
        self.layers
            .values()
            .map(|layer| {
                (
                    layer.texture.clone(),
                    layer.opacity_buffer.clone(),
                    layer.binding.clone(),
                )
            })
            .collect()
    }
}
