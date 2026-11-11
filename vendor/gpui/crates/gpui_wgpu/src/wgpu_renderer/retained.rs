//! Persistent quad storage and ordered multi-frame submission for owned UI frames.
use super::*;
use gpui::Quad;
use std::{
    collections::VecDeque,
    sync::atomic::{AtomicU64, Ordering},
    time::Duration,
};
pub type RetainedImage = image::RgbaImage;

pub struct QuadWrite {
    pub start: u32,
    pub values: Vec<Quad>,
}
pub struct QuadBatch {
    pub capacity: u32,
    pub writes: Vec<QuadWrite>,
    pub order: Option<Vec<Range<u32>>>,
    pub size: Size<DevicePixels>,
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
}

struct Flight {
    serial: u64,
    submission: wgpu::SubmissionIndex,
    // Explicit leases keep replaced allocations alive through completion.
    _buffer: wgpu::Buffer,
    _binding: wgpu::BindGroup,
}

pub struct RetainedQuadRenderer {
    renderer: WgpuHeadlessRenderer,
    buffer: wgpu::Buffer,
    binding: InstanceBinding,
    mirror: Vec<Quad>,
    order: Vec<Range<u32>>,
    size: Option<Size<DevicePixels>>,
    serial: u64,
    generation: u64,
    completed: Arc<AtomicU64>,
    flights: VecDeque<Flight>,
    max_in_flight: usize,
    failed: bool,
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
        anyhow::ensure!(
            order
                .iter()
                .all(|range| range.start < range.end && range.end <= batch.capacity),
            "draw outside allocation"
        );
        let serial = self
            .serial
            .checked_add(1)
            .context("submission serial exhausted")?;
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
        if self.size != Some(batch.size) {
            // Set shared globals and target through the same reference path.
            self.renderer.render(&Scene::default(), batch.size)?;
            self.size = Some(batch.size);
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
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("retained_quad_pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view,
                    resolve_target: None,
                    depth_slice: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                ..Default::default()
            });
            for range in &self.order {
                core.draw_instances(
                    &self.binding,
                    &core.resources.pipelines.quads,
                    range.clone(),
                    &mut pass,
                );
            }
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
        });
        self.serial = serial;
        self.renderer.check_gpu_errors()?;
        self.failed = false;
        Ok(QuadSubmission {
            serial,
            completed: self.completed.load(Ordering::Acquire),
            uploaded_bytes,
            upload_ranges,
            draw_calls: self.order.len(),
            in_flight: self.flights.len(),
            buffer_generation: self.generation,
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

    pub fn image(&mut self) -> Result<image::RgbaImage> {
        let image = self.renderer.read_image()?;
        self.completed_serial()?;
        Ok(image)
    }

    pub fn reference_image(
        &mut self,
        scene: &Scene,
        size: Size<DevicePixels>,
    ) -> Result<image::RgbaImage> {
        self.renderer.render(scene, size)?;
        self.size = Some(size);
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
