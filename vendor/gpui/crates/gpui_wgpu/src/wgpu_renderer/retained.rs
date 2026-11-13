//! Persistent quad storage and ordered multi-frame submission for owned UI frames.
use super::*;
use crate::wgpu;
use gpui::Quad;
use std::{
    collections::VecDeque,
    sync::atomic::{AtomicU64, Ordering},
    time::Duration,
};
pub type RetainedImage = image::RgbaImage;
mod allocation_budget;
#[cfg(target_os = "windows")]
mod native_surface;
#[cfg(target_os = "windows")]
pub use native_surface::NativeSurfaceStats;
mod completion;
pub use completion::{CompletionReceipt, CompletionReceiver, CompletionStats};
mod glyphs;
mod images;
pub use glyphs::{GlyphBatch, GlyphDrawId, GlyphGeometry, GlyphUpdates, RetainedGlyph};
mod layers;
pub use images::{ImageUpdates, QuadImage, QuadImageId};
use layers::LayerCache;
pub use layers::{LayerMemoryStats, QuadDraw, QuadLayer, QuadLayerId};

mod storage;
mod spatial;
mod spatial_index;
mod content_spatial;
pub use content_spatial::SpatialContent;
pub use spatial_index::SpatialWorkStats;
pub use spatial::{SpatialBatch, SpatialNode};
pub use storage::InstanceWrite;
use storage::PersistentStorage;
pub type QuadWrite = InstanceWrite<Quad>;
pub type ShadowWrite = InstanceWrite<gpui::Shadow>;
#[derive(Default)]
pub struct ShadowBatch {
    pub capacity: u32,
    pub writes: Vec<ShadowWrite>,
}
pub struct QuadBatch {
    pub capacity: u32,
    pub writes: Vec<QuadWrite>,
    pub order: Option<Vec<QuadDraw>>,
    pub layers: Vec<QuadLayer>,
    pub images: ImageUpdates,
    pub glyphs: GlyphUpdates,
    pub shadows: ShadowBatch,
    pub spatial: SpatialBatch,
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
    pub spatial_work: SpatialWorkStats,
    pub serial: u64,
    pub completed: u64,
    pub uploaded_bytes: u64,
    pub shadow_upload_bytes: u64,
    pub shadow_upload_ranges: usize,
    pub shadow_buffer_generation: u64,
    pub spatial_upload_bytes: u64,
    pub spatial_upload_ranges: usize,
    pub spatial_buffer_generation: u64,
    pub upload_ranges: usize,
    pub draw_calls: usize,
    pub in_flight: usize,
    pub buffer_generation: u64,
    pub drawn_pixels: u64,
    pub partial: bool,
    pub rasterized_layers: usize,
    pub reused_layers: usize,
    pub live_layer_bytes: u64,
    pub allocated_layer_bytes: u64,
    pub rasterized_layer_pixels: u64,
    pub layer_allocations: usize,
    pub rebased_layer_textures: usize,
    pub layer_target_bytes: u64,
    pub layer_memory: LayerMemoryStats,
    pub layer_budget_waits: usize,
    pub property_upload_bytes: u64,
    pub image_upload_bytes: u64,
    pub image_uploads: usize,
    pub image_geometry_bytes: u64,
    pub image_batch_updates: usize,
    pub glyph_atlas_bytes: u64,
    pub glyph_atlas_uploads: usize,
    pub glyph_instance_bytes: u64,
    pub glyph_upload_ranges: usize,
    pub glyph_material_bytes: u64,
    pub glyph_material_ranges: usize,
    pub glyph_property_bytes: u64,
    pub glyph_property_ranges: usize,
    pub glyph_batch_updates: usize,
    pub glyph_rebuilt_instances: usize,
    pub glyph_validated_instances: usize,
}

struct Flight {
    serial: u64,
    submission: wgpu::SubmissionIndex,
    // Explicit leases keep replaced allocations alive through completion.
    _quads: storage::StorageLease,
    _shadows: storage::StorageLease,
    _spatial: spatial::SpatialLease,
    _clear: Option<(wgpu::Buffer, wgpu::BindGroup)>,
    _layers: Vec<layers::LayerLease>,
    _layer_root: Option<layers::TargetLease>,
    _images: images::ImageLease,
    _glyphs: glyphs::GlyphLease,
}

pub struct RetainedQuadRenderer {
    renderer: WgpuHeadlessRenderer,
    #[cfg(target_os = "windows")]
    native_surface: Option<native_surface::Bridge>,
    #[cfg(target_os = "windows")]
    native_window_device:
        Option<windows::Win32::Graphics::Direct3D11::ID3D11Device>,
    #[cfg(target_os = "windows")]
    native_surface_error: Option<String>,
    quads: PersistentStorage<Quad>,
    shadows: PersistentStorage<gpui::Shadow>,
    spatial: spatial::SpatialCache,
    indexed_spatial: bool,
    quad_references: spatial_index::ReferenceIndex,
    shadow_references: spatial_index::ReferenceIndex,
    order: Vec<QuadDraw>,
    size: Option<Size<DevicePixels>>,
    serial: u64,
    completed: Arc<AtomicU64>,
    completion_driver: Option<completion::CompletionDriver>,
    flights: VecDeque<Flight>,
    max_in_flight: usize,
    failed: bool,
    clear: Option<(wgpu::Buffer, InstanceBinding)>,
    force_full: bool,
    full_instance_uploads: bool,
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
        Self::from_renderer(WgpuHeadlessRenderer::new()?, max_in_flight)
    }

    fn from_renderer(renderer: WgpuHeadlessRenderer, max_in_flight: usize) -> Result<Self> {
        anyhow::ensure!((1..=8).contains(&max_in_flight), "max_in_flight must be 1..=8");
        anyhow::ensure!(
            renderer.context.adapter.get_info().device_type != wgpu::DeviceType::Cpu,
            "hardware GPU required"
        );
        anyhow::ensure!(
            !renderer.core.uses_webgl_instance_data,
            "retained storage buffers required"
        );
        let quads = PersistentStorage::new(&renderer, "retained_quad_slots");
        let shadows = PersistentStorage::new(&renderer, "retained_shadow_slots");
        let spatial = spatial::SpatialCache::new(&renderer);
        let layers = LayerCache::new(&renderer);
        let images = images::ImageCache::new(&renderer, layers.camera_layout(), &spatial.layout);
        let glyphs = glyphs::GlyphCache::new(&renderer, layers.camera_layout(), &spatial.layout);
        Ok(Self {
            renderer,
            #[cfg(target_os = "windows")]
            native_surface: None,
            #[cfg(target_os = "windows")]
            native_window_device: None,
            #[cfg(target_os = "windows")]
            native_surface_error: None,
            quads,
            shadows,
            spatial,
            indexed_spatial: true,
            quad_references: Default::default(),
            shadow_references: Default::default(),
            order: vec![],
            size: None,
            serial: 0,
            completed: Arc::new(AtomicU64::new(0)),
            completion_driver: None,
            flights: VecDeque::new(),
            max_in_flight,
            failed: false,
            clear: None,
            force_full: true,
            full_instance_uploads: false,
            layers,
            images,
            glyphs,
        })
    }

    /// Cached validated spatial topology and inverse primitive references.
    pub fn indexed_spatial_enabled(&self) -> bool { self.indexed_spatial }
    pub fn set_indexed_spatial_enabled(&mut self, enabled: bool) {
        if self.indexed_spatial != enabled {
            self.indexed_spatial = enabled;
            self.spatial.clear_index();
            self.quad_references = Default::default();
            self.shadow_references = Default::default();
        }
    }

    /// Full declared-instance-range transport control; logical dirtiness and raster work stay unchanged.
    pub fn set_full_instance_uploads_enabled(&mut self, enabled: bool) {
        self.full_instance_uploads = enabled;
    }

    /// Force texture replacement on origin changes for correctness/performance controls.
    pub fn set_layer_texture_reuse_enabled(&mut self, enabled: bool) {
        if self.layers.reuse_textures != enabled {
            self.layers.reuse_textures = enabled;
            self.force_full = true;
        }
    }
    pub fn layer_texture_reuse_enabled(&self) -> bool { self.layers.reuse_textures }
    /// Experimental exact compared-content integer translation reuse. Default off.
    pub fn set_layer_translation_reuse_enabled(&mut self, enabled:bool) {
        if self.layers.reuse_translations != enabled {
            self.layers.reuse_translations=enabled; self.layers.clear(); self.force_full=true;
        }
    }
    pub fn layer_translation_reuse_enabled(&self)->bool {self.layers.reuse_translations}

    /// Adjust the unique isolated-texture payload cap after retiring this epoch.
    pub fn set_layer_budget_bytes(&mut self, bytes: u64) -> Result<()> {
        anyhow::ensure!(bytes > 0, "layer budget must be positive");
        anyhow::ensure!(!self.failed, "recreate renderer after failed submission");
        if bytes == self.layers.memory().limit_bytes { return Ok(()); }
        self.finish()?;
        self.layers.configure_budget(bytes);
        self.force_full = true;
        Ok(())
    }

    /// Observational: completion must be collected before retired leases leave.
    pub fn layer_memory(&self) -> LayerMemoryStats { self.layers.memory() }

    pub fn recreate(&self, max_in_flight: usize) -> Result<Self> {
        #[cfg(target_os = "windows")]
        if let Some(device) = &self.native_window_device {
            return Self::new_for_windows_device(device.clone(), max_in_flight);
        }
        Self::new(max_in_flight)
    }

    pub fn adapter_info(&self) -> wgpu::AdapterInfo {
        self.renderer.context.adapter.get_info()
    }
    pub fn last_serial(&self) -> u64 {
        self.serial
    }
    /// Open one native completion subscription for this device epoch. A reset
    /// closes the previous receiver; the host must subscribe to the new epoch.
    pub fn open_completions(&mut self) -> Result<CompletionReceiver> {
        anyhow::ensure!(
            self.completion_driver
                .as_ref()
                .is_none_or(|driver| driver.closed()),
            "completion receiver already open"
        );
        let (driver, receiver) = completion::CompletionDriver::new(
            self.renderer.context.device.clone(),
            self.completed.clone(),
        )?;
        if let Some(latest) = self.flights.back() {
            driver.schedule(latest.serial, latest.submission.clone());
        }
        self.completion_driver = Some(driver);
        Ok(receiver)
    }
    /// Observational only; does not poll the device or release any leases.
    pub fn lifecycle(&self) -> (u64, u64, usize) {
        (
            self.serial,
            self.completed.load(Ordering::Acquire),
            self.flights.len(),
        )
    }
    pub fn completion_stats(&self) -> Option<CompletionStats> {
        self.completion_driver.as_ref().map(|driver| driver.stats())
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
        PersistentStorage::<Quad>::validate(&self.renderer, batch.capacity, &batch.writes)?;
        PersistentStorage::<gpui::Shadow>::validate(
            &self.renderer, batch.shadows.capacity, &batch.shadows.writes,
        )?;
        let spatial_plan = self.spatial.preflight(&self.renderer, &batch.spatial, self.indexed_spatial)?;
        let mut spatial_work = SpatialWorkStats { indexed: self.indexed_spatial, ..Default::default() };
        if batch.spatial.enabled {
            spatial_work.dense_fallback = spatial_plan.as_ref().is_some_and(|p| p.dense);
            spatial_work.full_spatial_scan = spatial_plan.as_ref().is_none_or(|p| p.full || p.dense);
            spatial_work.checked_edges = spatial_plan.as_ref().filter(|p| !p.dense).map_or(batch.spatial.capacity as usize, |p| p.affected.len());
            spatial_work.resolved_nodes = spatial_work.checked_edges;
        }
        let mut reference_plans = None;
        if batch.spatial.enabled && self.indexed_spatial {
            let quads = self.quad_references.prepare(batch.capacity, batch.spatial.capacity,
                batch.writes.iter().flat_map(|w| w.values.iter().enumerate().map(move |(i,v)| (w.start+i as u32,v.order))),
                |slot| self.quads.value(slot).order).map_err(anyhow::Error::msg)?;
            let shadows = self.shadow_references.prepare(batch.shadows.capacity, batch.spatial.capacity,
                batch.shadows.writes.iter().flat_map(|w| w.values.iter().enumerate().map(move |(i,v)| (w.start+i as u32,v.order))),
                |slot| self.shadows.value(slot).order).map_err(anyhow::Error::msg)?;
            spatial_work.quad_references = quads.checked;
            spatial_work.shadow_references = shadows.checked;
            reference_plans = Some((quads, shadows));
        } else if batch.spatial.enabled {
            spatial_work.quad_references = batch.capacity as usize;
            spatial_work.shadow_references = batch.shadows.capacity as usize;
            for index in 0..batch.capacity {
                let property = self.quads.projected(index, &batch.writes).order;
                anyhow::ensure!(property <= batch.spatial.capacity, "quad spatial reference outside allocation");
            }
            for index in 0..batch.shadows.capacity {
                let property = self.shadows.projected(index, &batch.shadows.writes).order;
                anyhow::ensure!(property <= batch.spatial.capacity, "shadow spatial reference outside allocation");
            }
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
        let (glyph_ids, validated_glyphs) = self.glyphs.validate(
            &batch.glyphs,
            self.renderer
                .context
                .device
                .limits()
                .max_storage_buffer_binding_size,
        )?;
        let content_slots=spatial_plan.as_ref().filter(|p| !p.dense).map(|p| p.affected.as_slice());
        let content_all=self.spatial.extent_changed(&batch.spatial);
        let image_spatial=self.images.validate_spatial(&batch.images,&self.spatial,&batch.spatial,content_slots,content_all)?;
        let glyph_spatial=self.glyphs.validate_spatial(&batch.glyphs,&self.spatial,&batch.spatial,content_slots,content_all)?;
        LayerCache::validate(
            &batch.layers,
            order,
            batch.capacity,
            batch.shadows.capacity,
            batch.size,
            &image_ids,
            &glyph_ids,
            self.layers.memory().limit_bytes,
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
        let size_changed = self.size != Some(batch.size);
        let required = self.layers.prepare(&batch.layers, batch.size, size_changed);
        let mut layer_budget_waits = 0;
        while self.layers.memory().resident_bytes.checked_add(required)
            .is_none_or(|bytes| bytes > self.layers.memory().limit_bytes)
        {
            let oldest = self.flights.front().context("layer reservations exceed budget without pending work")?
                .submission.clone();
            self.renderer.context.device.poll(wgpu::PollType::Wait {
                submission_index: Some(oldest), timeout: Some(Duration::from_secs(30)),
            })?;
            self.completed_serial()?;
            layer_budget_waits += 1;
        }
        self.failed = true;
        if size_changed {
            // Set shared globals and target through the same reference path.
            self.renderer.render(&Scene::default(), batch.size)?;
            self.size = Some(batch.size);
            let (buffer, binding) = storage::allocation::<Quad>(&self.renderer, 1, "retained_clear");
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
        let spatial_all = self.spatial.parameters_change(&batch.spatial);
        let affected_properties = spatial_plan.as_ref().map(|p| p.affected.as_slice()).unwrap_or(&[]);
        if let Some((quads, shadows)) = reference_plans {
            self.quad_references.commit(quads);
            self.shadow_references.commit(shadows);
        } else {
            self.quad_references = Default::default();
            self.shadow_references = Default::default();
        }
        let spatial_stats = self.spatial.apply(&self.renderer, batch.spatial)?;
        let mut quad_stats = self.quads.apply(
            &self.renderer, batch.capacity, batch.writes, self.full_instance_uploads,
        )?;
        let mut shadow_stats = self.shadows.apply(
            &self.renderer, batch.shadows.capacity, batch.shadows.writes, self.full_instance_uploads,
        )?;
        if self.spatial.enabled && (!spatial_stats.dirty.is_empty() || spatial_all) {
            if self.indexed_spatial && !spatial_work.dense_fallback {
                spatial_work.quad_affected_instances = self.quad_references.affected(affected_properties, &mut quad_stats.dirty);
                spatial_work.shadow_affected_instances = self.shadow_references.affected(affected_properties, &mut shadow_stats.dirty);
            } else {
                for (capacity, shadow, dirty) in [(batch.capacity, false, &mut quad_stats.dirty),
                    (batch.shadows.capacity, true, &mut shadow_stats.dirty)] {
                    for index in 0..capacity {
                        let property = if shadow { self.shadows.value(index).order } else { self.quads.value(index).order };
                        if self.spatial.affected(property, &spatial_stats.dirty, spatial_all) {
                            if shadow { spatial_work.shadow_affected_instances += 1; }
                            else { spatial_work.quad_affected_instances += 1; }
                            if let Some(last) = dirty.last_mut().filter(|last| last.end == index) { last.end += 1; }
                            else { dirty.push(index..index+1); }
                        }
                    }
                }
            }
        }
        self.spatial.commit_index(spatial_plan);
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
        let mut image_stats = self
            .images
            .prepare(&self.renderer, batch.images, batch.size);
        let mut glyph_stats = self
            .glyphs
            .prepare(&self.renderer, batch.glyphs, batch.size)?;
        glyph_stats.validated_glyphs = validated_glyphs;
        image_stats.changed.extend(image_spatial);
        glyph_stats.changed.extend(glyph_spatial);
        let layer_stats = self.layers.render(
            &self.renderer,
            &self.quads.binding,
            &self.shadows.binding,
            &self.spatial,
            &self.quads,
            &self.shadows,
            &mut encoder,
            batch.layers,
            &quad_stats.dirty,
            &shadow_stats.dirty,
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
                &self.quads.binding,
                &self.shadows.binding,
                &self.spatial,
                &self.order,
                &mut pass,
                &self.images,
                &self.glyphs,
                None,
            )?;
        }
        let submission = core.resources.queue.submit([encoder.finish()]);
        let completed = self.completed.clone();
        core.resources.queue.on_submitted_work_done(move || {
            completed.fetch_max(serial, Ordering::Release);
        });
        if let Some(driver) = &self.completion_driver {
            driver.schedule(serial, submission.clone());
        }
        self.flights.push_back(Flight {
            serial,
            submission,
            _quads: self.quads.lease(),
            _shadows: self.shadows.lease(),
            _spatial: self.spatial.lease(),
            _clear: self
                .clear
                .as_ref()
                .map(|(buffer, binding)| (buffer.clone(), binding.bind_group.clone())),
            _layers: self.layers.leases(),
            _layer_root: self.layers.root_lease(),
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
            spatial_work,
            serial,
            completed: self.completed.load(Ordering::Acquire),
            uploaded_bytes: quad_stats.bytes,
            upload_ranges: quad_stats.ranges,
            shadow_upload_bytes: shadow_stats.bytes,
            shadow_upload_ranges: shadow_stats.ranges,
            shadow_buffer_generation: self.shadows.generation,
            spatial_upload_bytes: spatial_stats.bytes,
            spatial_upload_ranges: spatial_stats.ranges,
            spatial_buffer_generation: self.spatial.generation(),
            draw_calls: layer_stats.draws
                + if redraw == QuadRedraw::Unchanged {
                    0
                } else {
                    root_draws + usize::from(redraw != QuadRedraw::Full)
                },
            in_flight: self.flights.len(),
            buffer_generation: self.quads.generation,
            drawn_pixels,
            partial: matches!(redraw, QuadRedraw::Damage { .. }),
            rasterized_layers: layer_stats.rasterized,
            reused_layers: layer_stats.reused,
            live_layer_bytes: layer_stats.live_bytes,
            allocated_layer_bytes: layer_stats.allocated_bytes,
            rasterized_layer_pixels: layer_stats.rasterized_pixels,
            layer_allocations: layer_stats.allocations,
            rebased_layer_textures: layer_stats.rebased,
            layer_target_bytes: layer_stats.target_bytes,
            layer_memory: self.layers.memory(),
            layer_budget_waits,
            property_upload_bytes: layer_stats.property_bytes,
            image_upload_bytes: image_stats.bytes,
            image_uploads: image_stats.uploads,
            image_geometry_bytes: image_stats.geometry_bytes,
            image_batch_updates: image_stats.batch_updates,
            glyph_atlas_bytes: glyph_stats.atlas_bytes,
            glyph_atlas_uploads: glyph_stats.atlas_uploads,
            glyph_instance_bytes: glyph_stats.instance_bytes,
            glyph_upload_ranges: glyph_stats.ranges,
            glyph_material_bytes: glyph_stats.material_bytes,
            glyph_material_ranges: glyph_stats.material_ranges,
            glyph_property_bytes: glyph_stats.property_bytes,
            glyph_property_ranges: glyph_stats.property_ranges,
            glyph_batch_updates: glyph_stats.batch_updates,
            glyph_rebuilt_instances: glyph_stats.rebuilt_glyphs,
            glyph_validated_instances: glyph_stats.validated_glyphs,
        })
    }

    /// Wait for submitted work without a texture readback.
    pub fn finish(&mut self) -> Result<u64> {
        #[cfg(target_os = "windows")]
        if let Some(bridge) = &mut self.native_surface { bridge.finish(&self.renderer)?; }
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

    /// Readback bridge for the legacy straight-alpha BGRA sprite path.
    pub fn window_image(&mut self) -> Result<Arc<gpui::RenderImage>> {
        let mut image = self.image()?;
        for pixel in image.pixels_mut() {
            if pixel.0[3] == 255 { pixel.0.swap(0, 2); }
            else { gpui::swap_rgba_pa_to_bgra(&mut pixel.0); }
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

impl Drop for RetainedQuadRenderer {
    fn drop(&mut self) {
        // Close the host subscription before releasing renderer allocations.
        // The waiter's device clone can finish without owning any UI state.
        self.completion_driver = None;
    }
}

