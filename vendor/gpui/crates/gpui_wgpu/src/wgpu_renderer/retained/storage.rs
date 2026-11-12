//! Typed persistent storage with shared growth, range uploads and completion leases.
use super::*;

pub struct InstanceWrite<T> {
    pub start: u32,
    pub values: Vec<T>,
}
pub(super) type StorageLease = (wgpu::Buffer, wgpu::BindGroup);

// SAFETY: implementations must use the pinned, fully initialized and padding-free
// GPUI shader ABI, including every explicit pad field.
pub(super) unsafe trait Instance: Copy {
    fn empty() -> Self;
    fn validate(&self) -> Result<()> {
        Ok(())
    }
}
// SAFETY: the pinned Quad ABI is the existing full renderer's initialized ABI.
unsafe impl Instance for Quad {
    fn empty() -> Self {
        Self::default()
    }
}
// SAFETY: Shadow is repr(C), all fields and its explicit pad are initialized;
// its 112-byte size matches the pinned WGSL/storage declaration.
unsafe impl Instance for gpui::Shadow {
    fn empty() -> Self {
        Self {
            order: 0,
            blur_radius: ScaledPixels(0.0),
            bounds: Default::default(),
            corner_radii: gpui::Corners::all(ScaledPixels(0.0)),
            content_mask: Default::default(),
            color: gpui::Hsla::default(),
            element_bounds: Default::default(),
            element_corner_radii: gpui::Corners::all(ScaledPixels(0.0)),
            inset: 0,
            pad: 0,
        }
    }
    fn validate(&self) -> Result<()> {
        let valid_bounds = |b: Bounds<ScaledPixels>| {
            [
                b.origin.x.0,
                b.origin.y.0,
                b.size.width.0,
                b.size.height.0,
                b.origin.x.0 + b.size.width.0,
                b.origin.y.0 + b.size.height.0,
            ]
            .iter()
            .all(|v| v.is_finite())
                && b.size.width.0 >= 0.0
                && b.size.height.0 >= 0.0
        };
        anyhow::ensure!(
            valid_bounds(self.bounds)
                && valid_bounds(self.element_bounds)
                && valid_bounds(self.content_mask.bounds),
            "invalid shadow bounds"
        );
        anyhow::ensure!(
            self.blur_radius.0.is_finite()
                && self.blur_radius.0 >= 0.0
                && (self.blur_radius.0 * 3.0).is_finite()
                && self.inset <= 1,
            "invalid shadow parameters"
        );
        let paint_bounds = if self.inset != 0 {
            self.element_bounds
        } else {
            self.bounds.dilate(self.blur_radius * 3.0)
        };
        anyhow::ensure!(valid_bounds(paint_bounds), "shadow paint bounds overflow");
        for radii in [self.corner_radii, self.element_corner_radii] {
            anyhow::ensure!(
                [
                    radii.top_left.0,
                    radii.top_right.0,
                    radii.bottom_left.0,
                    radii.bottom_right.0
                ]
                .iter()
                .all(|v| v.is_finite() && *v >= 0.0),
                "invalid shadow radius"
            );
        }
        anyhow::ensure!(
            [self.color.h, self.color.s, self.color.l, self.color.a]
                .iter()
                .all(|v| v.is_finite())
                && (0.0..=1.0).contains(&self.color.a),
            "invalid shadow color"
        );
        Ok(())
    }
}
const _: () = assert!(std::mem::size_of::<Quad>() == 160);
const _: () = assert!(std::mem::size_of::<gpui::Shadow>() == 112);

#[derive(Default)]
pub(super) struct StorageStats {
    pub bytes: u64,
    pub ranges: usize,
    pub dirty: Vec<Range<u32>>,
}
pub(super) struct PersistentStorage<T: Instance> {
    pub buffer: wgpu::Buffer,
    pub binding: InstanceBinding,
    pub generation: u64,
    mirror: Vec<T>,
    label: &'static str,
}
impl<T: Instance> PersistentStorage<T> {
    pub fn new(renderer: &WgpuHeadlessRenderer, label: &'static str) -> Self {
        let (buffer, binding) = allocation::<T>(renderer, 1, label);
        let storage = Self {
            buffer,
            binding,
            generation: 1,
            mirror: vec![T::empty()],
            label,
        };
        // Typed empties may contain nonzero sentinels (spatial anchors). Keep
        // the initial GPU placeholder identical to the CPU mirror before writes.
        storage.upload(renderer, 0, &storage.mirror);
        storage
    }
    pub fn validate(
        renderer: &WgpuHeadlessRenderer,
        capacity: u32,
        writes: &[InstanceWrite<T>],
    ) -> Result<()> {
        if capacity == 0 && writes.is_empty() {
            return Ok(());
        }
        let count = (capacity as usize).max(1);
        let bytes = count
            .checked_mul(std::mem::size_of::<T>())
            .context("instance capacity overflow")? as u64;
        anyhow::ensure!(
            bytes
                <= u64::from(
                    renderer
                        .context
                        .device
                        .limits()
                        .max_storage_buffer_binding_size
                ),
            "instance capacity exceeds storage binding limit"
        );
        for write in writes {
            anyhow::ensure!(
                (write.start as usize)
                    .checked_add(write.values.len())
                    .is_some_and(|end| end <= count),
                "write outside allocation"
            );
            for value in &write.values {
                value.validate()?;
            }
        }
        Ok(())
    }
    pub fn apply(
        &mut self,
        renderer: &WgpuHeadlessRenderer,
        capacity: u32,
        writes: Vec<InstanceWrite<T>>,
        full_upload: bool,
    ) -> Result<StorageStats> {
        if capacity == 0 && writes.is_empty() {
            return Ok(StorageStats::default());
        }
        let capacity = (capacity as usize).max(1);
        let grown = capacity > self.mirror.len();
        if grown {
            let limit = renderer
                .context
                .device
                .limits()
                .max_storage_buffer_binding_size as usize
                / std::mem::size_of::<T>();
            let count = capacity.next_power_of_two().min(limit);
            self.mirror.resize(count, T::empty());
            (self.buffer, self.binding) = allocation::<T>(renderer, count as u64, self.label);
            self.generation = self
                .generation
                .checked_add(1)
                .context("buffer generation exhausted")?;
        }
        let mut stats = StorageStats::default();
        for write in &writes {
            let start = write.start as usize;
            self.mirror[start..start + write.values.len()].copy_from_slice(&write.values);
            if !write.values.is_empty() {
                stats
                    .dirty
                    .push(write.start..write.start + write.values.len() as u32);
            }
        }
        if grown {
            stats.bytes = self.upload(renderer, 0, &self.mirror);
            stats.ranges = 1;
        } else if full_upload {
            // Capacity is the declared addressable prefix, not geometric spare slots.
            stats.bytes = self.upload(renderer, 0, &self.mirror[..capacity]);
            stats.ranges = 1;
        } else {
            for write in &writes {
                if !write.values.is_empty() {
                    stats.bytes += self.upload(renderer, write.start, &write.values);
                    stats.ranges += 1;
                }
            }
        }
        Ok(stats)
    }
    fn upload(&self, renderer: &WgpuHeadlessRenderer, start: u32, values: &[T]) -> u64 {
        // SAFETY: Instance's contract is the pinned initialized GPU ABI.
        let bytes = unsafe { WgpuRendererCore::instance_bytes(values) };
        renderer.context.queue.write_buffer(
            &self.buffer,
            u64::from(start) * std::mem::size_of::<T>() as u64,
            bytes,
        );
        bytes.len() as u64
    }
    pub fn value(&self, index: u32) -> T {
        self.mirror.get(index as usize).copied().unwrap_or_else(T::empty)
    }
    pub fn projected(&self, index: u32, writes: &[InstanceWrite<T>]) -> T {
        for write in writes.iter().rev() {
            if let Some(value) = index.checked_sub(write.start).and_then(|offset| write.values.get(offset as usize)) {
                return *value;
            }
        }
        self.value(index)
    }
    pub fn lease(&self) -> StorageLease {
        (self.buffer.clone(), self.binding.bind_group.clone())
    }
}
pub(super) fn allocation<T: Instance>(
    renderer: &WgpuHeadlessRenderer,
    capacity: u64,
    label: &str,
) -> (wgpu::Buffer, InstanceBinding) {
    let device = &renderer.context.device;
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some(label),
        size: capacity * std::mem::size_of::<T>() as u64,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some(label),
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
