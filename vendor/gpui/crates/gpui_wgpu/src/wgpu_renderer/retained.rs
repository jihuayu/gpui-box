//! Persistent quad storage and ordered multi-frame submission for owned UI frames.
use super::*;
use gpui::Quad;
use std::{
    collections::VecDeque,
    sync::atomic::{AtomicU64, Ordering},
    time::Duration,
};
pub type RetainedImage = image::RgbaImage;
mod glyphs;
mod images;
pub use glyphs::{GlyphBatch, GlyphDrawId, RetainedGlyph};
mod layers;
pub use images::{QuadImage, QuadImageId};
use layers::LayerCache;
pub use layers::{QuadDraw, QuadLayer, QuadLayerId};

pub struct QuadWrite {
    pub start: u32,
    pub values: Vec<Quad>,
}
pub struct QuadBatch {
    pub capacity: u32,
    pub writes: Vec<QuadWrite>,
    pub order: Option<Vec<QuadDraw>>,
    pub layers: Vec<QuadLayer>,
    pub images: Vec<QuadImage>,
    pub glyphs: Vec<GlyphBatch>,
    pub cache_layers: bool,
    pub size: Size<DevicePixels>,
    pub redraw: QuadRedraw,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QuadRedraw {
    Full,
    Damage {
        x: u32,
        y: u32,
        width: u32,
        height: u32,
    },
    Unchanged,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct QuadSubmission {
    pub serial: u64,
    pub completed: u64,
    pub uploaded_bytes: u64,
    pub upload_ranges: usize,
    pub draw_calls: usize,
    pub in_flight: usize,
    pub buffer_generation: u64,
    pub drawn_pixels: u64,
    pub partial: bool,
    pub rasterized_layers: usize,
    pub reused_layers: usize,
    pub property_upload_bytes: u64,
    pub image_upload_bytes: u64,
    pub image_uploads: usize,
    pub image_geometry_bytes: u64,
    pub glyph_atlas_bytes: u64,
    pub glyph_atlas_uploads: usize,
    pub glyph_instance_bytes: u64,
    pub glyph_upload_ranges: usize,
    pub glyph_material_bytes: u64,
    pub glyph_material_ranges: usize,
}

struct Flight {
    serial: u64,
    submission: wgpu::SubmissionIndex,
    // Explicit leases keep replaced allocations alive through completion.
    _buffer: wgpu::Buffer,
    _binding: wgpu::BindGroup,
    _clear: Option<(wgpu::Buffer, wgpu::BindGroup)>,
    _layers: Vec<layers::LayerLease>,
    _images: images::ImageLease,
    _glyphs: glyphs::GlyphLease,
}

pub struct RetainedQuadRenderer {
    renderer: WgpuHeadlessRenderer,
    buffer: wgpu::Buffer,
    binding: InstanceBinding,
    mirror: Vec<Quad>,
    order: Vec<QuadDraw>,
    size: Option<Size<DevicePixels>>,
    serial: u64,
    generation: u64,
    completed: Arc<AtomicU64>,
    flights: VecDeque<Flight>,
    max_in_flight: usize,
    failed: bool,
    clear: Option<(wgpu::Buffer, InstanceBinding)>,
    force_full: bool,
    layers: LayerCache,
    images: images::ImageCache,
    glyphs: glyphs::GlyphCache,
}

impl RetainedQuadRenderer {
    pub fn new(max_in_flight: usize) -> Result<Self> {
        anyhow::ensure!(
            (1..=8).contains(&max_in_flight),
            "max_in_flight must be 1..=8"
        );
        let renderer = WgpuHeadlessRenderer::new()?;
        anyhow::ensure!(
            renderer.context.adapter.get_info().device_type != wgpu::DeviceType::Cpu,
            "hardware GPU required"
        );
        anyhow::ensure!(
            !renderer.core.uses_webgl_instance_data,
            "retained storage buffers required"
        );
        let (buffer, binding) = allocation(&renderer, 1);
        let layers = LayerCache::new(&renderer);
        let images = images::ImageCache::new(&renderer);
        let glyphs = glyphs::GlyphCache::new(&renderer);
        Ok(Self {
            renderer,
            buffer,
            binding,
            mirror: vec![Quad::default()],
            order: vec![],
            size: None,
            serial: 0,
            generation: 1,
            completed: Arc::new(AtomicU64::new(0)),
            flights: VecDeque::new(),
            max_in_flight,
            failed: false,
            clear: None,
            force_full: true,
            layers,
            images,
            glyphs,
        })
    }

    pub fn adapter_info(&self) -> wgpu::AdapterInfo {
        self.renderer.context.adapter.get_info()
    }
    pub fn last_serial(&self) -> u64 {
        self.serial
    }
    pub fn completed_serial(&mut self) -> Result<u64> {
        self.renderer.context.device.poll(wgpu::PollType::Poll)?;
        let completed = self.completed.load(Ordering::Acquire);
        while self
            .flights
            .front()
            .is_some_and(|frame| frame.serial <= completed)
        {
            self.flights.pop_front();
        }
        self.renderer.check_gpu_errors()?;
        Ok(completed)
    }

    pub fn submit(&mut self, batch: QuadBatch) -> Result<QuadSubmission> {
        anyhow::ensure!(!self.failed, "recreate renderer after failed submission");
        anyhow::ensure!(
            batch.size.width.0 > 0 && batch.size.height.0 > 0,
            "nonempty render target required"
        );
        let capacity = (batch.capacity as usize).max(1);
        let bytes = capacity
            .checked_mul(std::mem::size_of::<Quad>())
            .context("quad capacity overflow")? as u64;
        anyhow::ensure!(
            bytes
                <= u64::from(
                    self.renderer
                        .context
                        .device
                        .limits()
                        .max_storage_buffer_binding_size
                ),
            "quad capacity exceeds storage binding limit"
        );
        for write in &batch.writes {
            anyhow::ensure!(
                (write.start as usize)
                    .checked_add(write.values.len())
                    .is_some_and(|end| end <= capacity),
                "write outside allocation"
            );
        }
        let order = batch.order.as_ref().unwrap_or(&self.order);
        let image_ids = self.images.validate(
            &batch.images,
            self.renderer
                .context
                .device
                .limits()
                .max_texture_dimension_2d,
        )?;
        let glyph_ids = self.glyphs.validate(
            &batch.glyphs,
            self.renderer
                .context
                .device
                .limits()
                .max_storage_buffer_binding_size,
        )?;
        LayerCache::validate(
            &batch.layers,
            order,
            batch.capacity,
            batch.size,
            &image_ids,
            &glyph_ids,
        )?;
        let serial = self
            .serial
            .checked_add(1)
            .context("submission serial exhausted")?;
        if let QuadRedraw::Damage {
            x,
            y,
            width,
            height,
        } = batch.redraw
        {
            anyhow::ensure!(
                width > 0
                    && height > 0
                    && x.checked_add(width)
                        .is_some_and(|end| end <= batch.size.width.0 as u32)
                    && y.checked_add(height)
                        .is_some_and(|end| end <= batch.size.height.0 as u32),
                "invalid damage scissor"
            );
        }
        self.completed_serial()?;
        if self.flights.len() >= self.max_in_flight {
            let oldest = self
                .flights
                .front()
                .context("missing frame")?
                .submission
                .clone();
            self.renderer.context.device.poll(wgpu::PollType::Wait {
                submission_index: Some(oldest),
                timeout: Some(Duration::from_secs(30)),
            })?;
            self.completed_serial()?;
        }
        self.failed = true;
        let size_changed = self.size != Some(batch.size);
        if size_changed {
            self.layers.clear();
            // Set shared globals and target through the same reference path.
            self.renderer.render(&Scene::default(), batch.size)?;
            self.size = Some(batch.size);
            let (buffer, binding) = allocation(&self.renderer, 1);
            let bounds = Bounds {
                origin: gpui::point(ScaledPixels(0.0), ScaledPixels(0.0)),
                size: gpui::size(
                    ScaledPixels(batch.size.width.0 as f32),
                    ScaledPixels(batch.size.height.0 as f32),
                ),
            };
            let clear = Quad {
                bounds,
                content_mask: gpui::ContentMask { bounds },
                background: gpui::rgba(0x000000ff).into(),
                ..Default::default()
            };
            let clear_records = [clear];
            // SAFETY: This initialized Quad uses the same GPU ABI as normal draws.
            self.renderer
                .context
                .queue
                .write_buffer(&buffer, 0, unsafe {
                    WgpuRendererCore::instance_bytes(&clear_records)
                });
            self.clear = Some((buffer, binding));
        }
        let grown = capacity > self.mirror.len();
        if grown {
            let limit = self
                .renderer
                .context
                .device
                .limits()
                .max_storage_buffer_binding_size as usize
                / std::mem::size_of::<Quad>();
            let allocation_capacity = capacity.next_power_of_two().min(limit);
            self.mirror.resize(allocation_capacity, Quad::default());
            (self.buffer, self.binding) = allocation(&self.renderer, allocation_capacity as u64);
            self.generation = self
                .generation
                .checked_add(1)
                .context("buffer generation exhausted")?;
        }
        for write in &batch.writes {
            let start = write.start as usize;
            self.mirror[start..start + write.values.len()].copy_from_slice(&write.values);
        }
        let mut uploaded_bytes = 0;
        let mut upload_ranges = 0;
        let dirty_ranges: Vec<_> = batch
            .writes
            .iter()
            .map(|write| write.start..write.start + write.values.len() as u32)
            .collect();
        if grown {
            uploaded_bytes = self.upload(0, &self.mirror);
            upload_ranges = 1;
        } else {
            for write in &batch.writes {
                if !write.values.is_empty() {
                    uploaded_bytes += self.upload(write.start, &write.values);
                    upload_ranges += 1;
                }
            }
        }
        if let Some(order) = batch.order {
            self.order = order;
        }
        let core = &self.renderer.core;
        let view = &self
            .renderer
            .render_target
            .as_ref()
            .context("target missing")?
            .view;
        let mut encoder =
            core.resources
                .device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("retained_quad_frame"),
                });
        let redraw = if size_changed || self.force_full {
            QuadRedraw::Full
        } else {
            batch.redraw
        };
        let image_stats = self
            .images
            .prepare(&self.renderer, batch.images, batch.size);
        let glyph_stats = self
            .glyphs
            .prepare(&self.renderer, batch.glyphs, batch.size)?;
        let layer_stats = self.layers.render(
            &self.renderer,
            &self.binding,
            &mut encoder,
            batch.layers,
            &dirty_ranges,
            batch.size,
            batch.cache_layers,
            &self.images,
            &image_stats.changed,
            &self.glyphs,
            &glyph_stats.changed,
        )?;
        let mut root_draws = 0;
        if redraw != QuadRedraw::Unchanged {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("retained_quad_pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view,
                    resolve_target: None,
                    depth_slice: None,
                    ops: wgpu::Operations {
                        load: if redraw == QuadRedraw::Full {
                            wgpu::LoadOp::Clear(wgpu::Color::BLACK)
                        } else {
                            wgpu::LoadOp::Load
                        },
                        store: wgpu::StoreOp::Store,
                    },
                })],
                ..Default::default()
            });
            if let QuadRedraw::Damage {
                x,
                y,
                width,
                height,
            } = redraw
            {
                pass.set_scissor_rect(x, y, width, height);
                core.draw_instances(
                    &self.clear.as_ref().context("clear allocation missing")?.1,
                    &core.resources.pipelines.quads,
                    0..1,
                    &mut pass,
                );
            }
            root_draws = self.layers.draw(
                core,
                &self.binding,
                &self.order,
                &mut pass,
                &self.images,
                &self.glyphs,
            )?;
        }
        let submission = core.resources.queue.submit([encoder.finish()]);
        let completed = self.completed.clone();
        core.resources.queue.on_submitted_work_done(move || {
            completed.fetch_max(serial, Ordering::Release);
        });
        self.flights.push_back(Flight {
            serial,
            submission,
            _buffer: self.buffer.clone(),
            _binding: self.binding.bind_group.clone(),
            _clear: self
                .clear
                .as_ref()
                .map(|(buffer, binding)| (buffer.clone(), binding.bind_group.clone())),
            _layers: self.layers.leases(),
            _images: self.images.leases(),
            _glyphs: self.glyphs.leases(),
        });
        self.serial = serial;
        self.renderer.check_gpu_errors()?;
        self.failed = false;
        self.force_full = false;
        let drawn_pixels = match redraw {
            QuadRedraw::Full => batch.size.width.0 as u64 * batch.size.height.0 as u64,
            QuadRedraw::Damage { width, height, .. } => u64::from(width) * u64::from(height),
            QuadRedraw::Unchanged => 0,
        };
        Ok(QuadSubmission {
            serial,
            completed: self.completed.load(Ordering::Acquire),
            uploaded_bytes,
            upload_ranges,
            draw_calls: layer_stats.draws
                + if redraw == QuadRedraw::Unchanged {
                    0
                } else {
                    root_draws + usize::from(redraw != QuadRedraw::Full)
                },
            in_flight: self.flights.len(),
            buffer_generation: self.generation,
            drawn_pixels,
            partial: matches!(redraw, QuadRedraw::Damage { .. }),
            rasterized_layers: layer_stats.rasterized,
            reused_layers: layer_stats.reused,
            property_upload_bytes: layer_stats.property_bytes,
            image_upload_bytes: image_stats.bytes,
            image_uploads: image_stats.uploads,
            image_geometry_bytes: image_stats.geometry_bytes,
            glyph_atlas_bytes: glyph_stats.atlas_bytes,
            glyph_atlas_uploads: glyph_stats.atlas_uploads,
            glyph_instance_bytes: glyph_stats.instance_bytes,
            glyph_upload_ranges: glyph_stats.ranges,
            glyph_material_bytes: glyph_stats.material_bytes,
            glyph_material_ranges: glyph_stats.material_ranges,
        })
    }

    fn upload(&self, start: u32, quads: &[Quad]) -> u64 {
        // SAFETY: Same initialized, padding-free Quad GPU ABI as the full renderer.
        let bytes = unsafe { WgpuRendererCore::instance_bytes(quads) };
        self.renderer.context.queue.write_buffer(
            &self.buffer,
            u64::from(start) * std::mem::size_of::<Quad>() as u64,
            bytes,
        );
        bytes.len() as u64
    }

    /// Wait for submitted work without a texture readback.
    pub fn finish(&mut self) -> Result<u64> {
        if let Some(latest) = self.flights.back() {
            self.renderer.context.device.poll(wgpu::PollType::Wait {
                submission_index: Some(latest.submission.clone()),
                timeout: Some(Duration::from_secs(30)),
            })?;
        }
        self.completed_serial()
    }

    pub fn image(&mut self) -> Result<image::RgbaImage> {
        let image = self.renderer.read_image()?;
        self.completed_serial()?;
        Ok(image)
    }

    /// Readback bridge for existing window paint; direct texture import is pending.
    pub fn window_image(&mut self) -> Result<Arc<gpui::RenderImage>> {
        let mut image = self.image()?;
        for pixel in image.pixels_mut() {
            pixel.0.swap(0, 2);
        }
        Ok(Arc::new(gpui::RenderImage::new(vec![image::Frame::new(
            image,
        )])))
    }

    pub fn reference_image(
        &mut self,
        scene: &Scene,
        size: Size<DevicePixels>,
    ) -> Result<image::RgbaImage> {
        self.renderer.render(scene, size)?;
        self.size = Some(size);
        self.force_full = true;
        self.renderer.read_image()
    }
}

fn allocation(renderer: &WgpuHeadlessRenderer, capacity: u64) -> (wgpu::Buffer, InstanceBinding) {
    let device = &renderer.context.device;
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("retained_quad_slots"),
        size: capacity * std::mem::size_of::<Quad>() as u64,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("retained_quad_slots_binding"),
        layout: &renderer.core.resources.bind_group_layouts.instances,
        entries: &[wgpu::BindGroupEntry {
            binding: 0,
            resource: buffer.as_entire_binding(),
        }],
    });
    (
        buffer,
        InstanceBinding {
            bind_group,
            first_instance: 0,
        },
    )
}
