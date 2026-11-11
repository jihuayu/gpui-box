//! Opt-in, fixed-scene experiment. All writes and draws use one ordered queue.
//! A dedicated buffer belongs to this session and is never recycled into another
//! scene. wgpu stages queue writes before the following submission, after earlier
//! submitted reads. No CPU mapping or unsynchronized GPU allocation reuse occurs.

use super::*;
use gpui::{Hsla, Quad};
use std::{
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, Instant},
};

#[derive(Default, Clone, Copy, Debug)]
pub struct Metrics {
    pub instance_bytes: u64,
    pub instance_writes: u64,
    pub upload_ns: u64,
    pub submit_ns: u64,
    pub cpu_ns: u64,
    pub completion_ns: u64,
    pub gpu_draw_ns: Option<f64>,
}

pub(super) struct GpuTimer {
    queries: wgpu::QuerySet,
    resolve: wgpu::Buffer,
    readback: wgpu::Buffer,
}

impl GpuTimer {
    fn new(device: &wgpu::Device) -> Self {
        Self {
            queries: device.create_query_set(&wgpu::QuerySetDescriptor {
                label: Some("quad_patch_timestamps"),
                ty: wgpu::QueryType::Timestamp,
                count: 2,
            }),
            resolve: device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("quad_patch_query_resolve"),
                size: 16,
                usage: wgpu::BufferUsages::QUERY_RESOLVE | wgpu::BufferUsages::COPY_SRC,
                mapped_at_creation: false,
            }),
            readback: device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("quad_patch_query_readback"),
                size: 16,
                usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }),
        }
    }

    pub(super) fn writes(&self) -> wgpu::RenderPassTimestampWrites<'_> {
        wgpu::RenderPassTimestampWrites {
            query_set: &self.queries,
            beginning_of_pass_write_index: Some(0),
            end_of_pass_write_index: Some(1),
        }
    }

    pub(super) fn resolve(&self, encoder: &mut wgpu::CommandEncoder) {
        encoder.resolve_query_set(&self.queries, 0..2, &self.resolve, 0);
        encoder.copy_buffer_to_buffer(&self.resolve, 0, &self.readback, 0, 16);
    }

    fn read(&self, context: &WgpuContext) -> Result<f64> {
        let (send, receive) = std::sync::mpsc::channel();
        self.readback
            .slice(..)
            .map_async(wgpu::MapMode::Read, move |result| {
                let _ = send.send(result);
            });
        context.device.poll(wgpu::PollType::Wait {
            submission_index: None,
            timeout: Some(Duration::from_secs(30)),
        })?;
        receive.recv_timeout(Duration::from_secs(30))??;
        let data = self.readback.slice(..).get_mapped_range()?;
        let start = u64::from_le_bytes(data[0..8].try_into()?);
        let end = u64::from_le_bytes(data[8..16].try_into()?);
        drop(data);
        self.readback.unmap();
        Ok(end.wrapping_sub(start) as f64 * f64::from(context.queue.get_timestamp_period()))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UploadMode {
    Original,
    RetainedFull,
    Patch,
}

#[derive(Clone, Copy, Debug)]
pub struct QuadHandle {
    session: u64,
    slot: usize,
}

/// Restricted to an immutable scene of opaque solid quads. Only a typed solid
/// color setter is exposed, so geometry, ordering and material identity cannot
/// change accidentally. Rebuild a session to change structure or target size.
pub struct QuadPatchPrototype {
    renderer: WgpuHeadlessRenderer,
    scene: Scene,
    size: Size<DevicePixels>,
    buffer: wgpu::Buffer,
    binding: InstanceBinding,
    session: u64,
    upload_mode: Option<UploadMode>,
    failed: bool,
}

impl QuadPatchPrototype {
    pub fn new(scene: Scene, size: Size<DevicePixels>) -> Result<Self> {
        anyhow::ensure!(
            !scene.quads.is_empty(),
            "prototype requires at least one quad"
        );
        anyhow::ensure!(
            scene.len() == scene.quads.len(),
            "prototype accepts only ungrouped quad operations"
        );
        for quad in &scene.quads {
            let color = quad
                .background
                .as_solid()
                .context("prototype requires solid backgrounds")?;
            anyhow::ensure!(color.a == 1.0, "prototype requires opaque backgrounds");
        }
        let context = timestamp_context()?;
        let target_format = wgpu::TextureFormat::Rgba8Unorm;
        let atlas = Arc::new(WgpuAtlas::from_context(&context));
        let mut core = WgpuRendererCore::new(
            &context,
            atlas,
            target_format,
            wgpu::CompositeAlphaMode::Opaque,
        );
        if context
            .device
            .features()
            .contains(wgpu::Features::TIMESTAMP_QUERY)
        {
            core.prototype_timer = Some(GpuTimer::new(&context.device));
        }
        let mut renderer = WgpuHeadlessRenderer {
            observed_error_generation: context.errors().current_generation(),
            context,
            core,
            render_target: None,
        };
        renderer.render(&scene, size)?;
        wait(&renderer.context)?;
        let bytes = std::mem::size_of_val(scene.quads.as_slice()) as u64;
        let device = &renderer.context.device;
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("persistent_quad_prototype"),
            size: bytes,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        // SAFETY: Quad is the existing initialized, padding-free GPU ABI used by
        // the normal renderer. The helper reinterprets that same representation.
        let data = unsafe { WgpuRendererCore::instance_bytes(&scene.quads) };
        renderer.context.queue.write_buffer(&buffer, 0, data);
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("persistent_quad_binding"),
            layout: &renderer.core.resources.bind_group_layouts.instances,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: buffer.as_entire_binding(),
            }],
        });
        renderer.context.queue.submit([]);
        wait(&renderer.context)?;
        static NEXT: AtomicU64 = AtomicU64::new(1);
        let session = NEXT.fetch_add(1, Ordering::Relaxed);
        anyhow::ensure!(session != u64::MAX, "prototype session space exhausted");
        Ok(Self {
            renderer,
            scene,
            size,
            buffer,
            binding: InstanceBinding {
                bind_group,
                first_instance: 0,
            },
            session,
            upload_mode: None,
            failed: false,
        })
    }

    pub fn handle(&self, slot: usize) -> Result<QuadHandle> {
        anyhow::ensure!(slot < self.scene.quads.len(), "quad slot out of range");
        Ok(QuadHandle {
            session: self.session,
            slot,
        })
    }

    pub fn adapter_info(&self) -> wgpu::AdapterInfo {
        self.renderer.context.adapter.get_info()
    }

    pub fn set_color(
        &mut self,
        handle: QuadHandle,
        color: Hsla,
        mode: UploadMode,
    ) -> Result<Metrics> {
        anyhow::ensure!(!self.failed, "rebuild prototype after a failed GPU update");
        anyhow::ensure!(
            handle.session == self.session && handle.slot < self.scene.quads.len(),
            "stale or foreign quad handle"
        );
        anyhow::ensure!(
            [color.h, color.s, color.l, color.a]
                .iter()
                .all(|v| v.is_finite() && (0.0..=1.0).contains(v))
                && color.a == 1.0,
            "prototype color requires finite normalized opaque HSLA"
        );
        self.renderer.check_gpu_errors()?;
        anyhow::ensure!(
            self.upload_mode.is_none_or(|selected| selected == mode),
            "use separate sessions for full and patch updates"
        );
        self.upload_mode = Some(mode);
        let background = gpui::solid_background(color);
        if self.scene.quads[handle.slot].background == background {
            return Ok(Metrics::default());
        }
        self.failed = true;
        let start = Instant::now();
        self.scene.quads[handle.slot].background = background;
        self.renderer.core.prototype_metrics = Metrics::default();
        if mode == UploadMode::Original {
            self.renderer.render(&self.scene, self.size)?;
        } else {
            let upload = Instant::now();
            let (quads, offset) = if mode == UploadMode::RetainedFull {
                (self.scene.quads.as_slice(), 0)
            } else {
                (
                    std::slice::from_ref(&self.scene.quads[handle.slot]),
                    (handle.slot * std::mem::size_of::<Quad>()) as u64,
                )
            };
            // SAFETY: Identical padding-free Quad ABI and helper used by the full path.
            let data = unsafe { WgpuRendererCore::instance_bytes(quads) };
            self.renderer
                .context
                .queue
                .write_buffer(&self.buffer, offset, data);
            let upload_ns = upload.elapsed().as_nanos() as u64;
            let core = &self.renderer.core;
            let view = &self
                .renderer
                .render_target
                .as_ref()
                .context("missing target")?
                .view;
            let mut encoder =
                core.resources
                    .device
                    .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                        label: Some("quad_patch_encoder"),
                    });
            {
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("quad_patch_pass"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view,
                        resolve_target: None,
                        depth_slice: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    timestamp_writes: core.prototype_timer.as_ref().map(GpuTimer::writes),
                    ..Default::default()
                });
                core.draw_instances(
                    &self.binding,
                    &core.resources.pipelines.quads,
                    0..self.scene.quads.len() as u32,
                    &mut pass,
                );
            }
            if let Some(timer) = &core.prototype_timer {
                timer.resolve(&mut encoder);
            }
            let submit = Instant::now();
            core.resources.queue.submit([encoder.finish()]);
            self.renderer.core.prototype_metrics = Metrics {
                upload_ns,
                submit_ns: submit.elapsed().as_nanos() as u64,
                instance_bytes: data.len() as u64,
                instance_writes: 1,
                ..Default::default()
            };
        }
        let mut result = self.renderer.core.prototype_metrics;
        result.cpu_ns = start.elapsed().as_nanos() as u64;
        wait(&self.renderer.context)?;
        result.completion_ns = start.elapsed().as_nanos() as u64;
        if let Some(timer) = &self.renderer.core.prototype_timer {
            result.gpu_draw_ns = Some(timer.read(&self.renderer.context)?);
        }
        self.renderer.check_gpu_errors()?;
        self.failed = false;
        Ok(result)
    }

    pub fn image(&mut self) -> Result<image::RgbaImage> {
        self.renderer.read_image()
    }

    /// Render the current CPU truth through the original complete renderer.
    pub fn reference_image(&mut self) -> Result<image::RgbaImage> {
        self.renderer.render(&self.scene, self.size)?;
        self.renderer.read_image()
    }
}

fn wait(context: &WgpuContext) -> Result<()> {
    context.device.poll(wgpu::PollType::Wait {
        submission_index: None,
        timeout: Some(Duration::from_secs(30)),
    })?;
    Ok(())
}

fn timestamp_context() -> Result<WgpuContext> {
    let instance = WgpuContext::instance(None);
    let mut adapters = gpui::block_on(instance.enumerate_adapters(wgpu::Backends::all()));
    adapters.sort_by_key(|adapter| match adapter.get_info().device_type {
        wgpu::DeviceType::DiscreteGpu => 0,
        wgpu::DeviceType::IntegratedGpu => 1,
        _ => 2,
    });
    let adapter = adapters
        .into_iter()
        .find(|adapter| adapter.get_info().device_type != wgpu::DeviceType::Cpu)
        .context("no hardware GPU adapter for prototype")?;
    let mut features = adapter.features() & wgpu::Features::TIMESTAMP_QUERY;
    features |= adapter.features() & wgpu::Features::DUAL_SOURCE_BLENDING;
    let (device, queue) = gpui::block_on(
        adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("quad_patch_experiment"),
            required_features: features,
            required_limits: wgpu::Limits::downlevel_defaults()
                .using_resolution(adapter.limits())
                .using_alignment(adapter.limits()),
            memory_hints: wgpu::MemoryHints::MemoryUsage,
            trace: wgpu::Trace::Off,
            experimental_features: wgpu::ExperimentalFeatures::disabled(),
            default_queue: Default::default(),
        }),
    )?;
    let dual = features.contains(wgpu::Features::DUAL_SOURCE_BLENDING);
    let color_format = WgpuContext::select_color_texture_format(&adapter)?;
    Ok(WgpuContext::from_native_adapter_and_device(
        instance,
        adapter,
        device,
        queue,
        dual,
        color_format,
    ))
}
