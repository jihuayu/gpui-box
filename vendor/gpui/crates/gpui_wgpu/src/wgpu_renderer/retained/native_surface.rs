//! Windows D3D12 → D3D11 immutable GPU snapshots. No CPU readback or upload.
use super::*;
use windows::{
    Win32::{
        Foundation::{CloseHandle, HANDLE},
        Graphics::{Direct3D11::*, Direct3D12::*, Dxgi::Common::*},
    },
    core::Interface,
};
struct SharedHandle(HANDLE);
impl Drop for SharedHandle {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}

/// Logical payload/operation observations; driver allocations are not measured.
#[derive(Clone, Copy, Debug, Default)]
pub struct NativeSurfaceStats {
    pub allocations: u64,
    pub reuses: u64,
    pub copies: u64,
    pub cache_hits: u64,
    pub copied_bytes: u64,
    pub live_allocations: usize,
    pub live_payload_bytes: u64,
}
struct Allocation {
    texture: wgpu::Texture,
    view: ID3D11ShaderResourceView,
    fence: ID3D12Fence,
    ready: ID3D11Fence,
    value: AtomicU64,
    retirement: gpui::WindowsGpuSurfaceRetirement,
}
impl Allocation {
    fn idle(&self) -> bool {
        let completed = unsafe { self.fence.GetCompletedValue() };
        completed != u64::MAX
            && completed >= self.value.load(Ordering::Acquire)
            && self.retirement.is_complete()
    }
    fn create(
        renderer: &WgpuHeadlessRenderer,
        native: &ID3D11Device,
        source: &wgpu::Texture,
    ) -> Result<Self> {
        // This context is DX12-only and matches the D3D11 adapter LUID.
        let hal = unsafe { renderer.context.device.as_hal::<wgpu::hal::api::Dx12>() }
            .context("DX12 required")?;
        let device = hal.raw_device().clone();
        drop(hal);
        let format = match source.format() {
            wgpu::TextureFormat::Bgra8Unorm => DXGI_FORMAT_B8G8R8A8_UNORM,
            wgpu::TextureFormat::Rgba8Unorm => DXGI_FORMAT_R8G8B8A8_UNORM,
            _ => anyhow::bail!("unsupported native snapshot format"),
        };
        let desc = D3D12_RESOURCE_DESC {
            Dimension: D3D12_RESOURCE_DIMENSION_TEXTURE2D,
            Width: u64::from(source.width()),
            Height: source.height(),
            DepthOrArraySize: 1,
            MipLevels: 1,
            Format: format,
            SampleDesc: DXGI_SAMPLE_DESC {
                Count: 1,
                Quality: 0,
            },
            Layout: D3D12_TEXTURE_LAYOUT_UNKNOWN,
            // Include render-target binding for the tested D3D11 shared importer.
            Flags: D3D12_RESOURCE_FLAG_ALLOW_SIMULTANEOUS_ACCESS
                | D3D12_RESOURCE_FLAG_ALLOW_RENDER_TARGET,
            ..Default::default()
        };
        let heap = D3D12_HEAP_PROPERTIES {
            Type: D3D12_HEAP_TYPE_DEFAULT,
            CreationNodeMask: 1,
            VisibleNodeMask: 1,
            ..Default::default()
        };
        let mut resource: Option<ID3D12Resource> = None;
        unsafe {
            device
                .CreateCommittedResource(
                    &heap,
                    D3D12_HEAP_FLAG_SHARED,
                    &desc,
                    D3D12_RESOURCE_STATE_COMMON,
                    None,
                    &mut resource,
                )
                .map_err(|e| anyhow::anyhow!("create shared DX12 texture: {e}"))?;
        }
        let resource = resource.context("shared texture missing")?;
        let handle = SharedHandle(unsafe {
            device
                .CreateSharedHandle(&resource, None, 0x10000000, None)
                .map_err(|e| anyhow::anyhow!("export texture handle: {e}"))?
        });
        let d3d11: ID3D11Device1 = native.cast()?;
        let texture: ID3D11Texture2D = unsafe {
            d3d11
                .OpenSharedResource1(handle.0)
                .map_err(|e| anyhow::anyhow!("open DX12 texture on D3D11: {e}"))?
        };
        let fence: ID3D12Fence = unsafe {
            device
                .CreateFence(0, D3D12_FENCE_FLAG_SHARED)
                .map_err(|e| anyhow::anyhow!("create shared DX12 fence: {e}"))?
        };
        let fence_handle = SharedHandle(unsafe {
            device
                .CreateSharedHandle(&fence, None, 0x10000000, None)
                .map_err(|e| anyhow::anyhow!("export fence handle: {e}"))?
        });
        let d3d11: ID3D11Device5 = native.cast()?;
        let mut ready: Option<ID3D11Fence> = None;
        unsafe {
            d3d11
                .OpenSharedFence(fence_handle.0, &mut ready)
                .map_err(|e| anyhow::anyhow!("open DX12 fence on D3D11: {e}"))?;
        }
        let ready = ready.context("shared fence missing")?;
        let size = source.size();
        // Same device, exact matching format/extents, COMMON initial state. Wgpu
        // owns this wrapper; no raw mutation or reuse occurs after publication.
        let imported = unsafe {
            let raw = wgpu::hal::dx12::Device::texture_from_raw(
                resource,
                source.format(),
                wgpu::TextureDimension::D2,
                size,
                1,
                1,
            );
            renderer
                .context
                .device
                .create_texture_from_hal::<wgpu::hal::api::Dx12>(
                    raw,
                    &wgpu::TextureDescriptor {
                        label: Some("retained_native_snapshot"),
                        size,
                        mip_level_count: 1,
                        sample_count: 1,
                        dimension: wgpu::TextureDimension::D2,
                        format: source.format(),
                        usage: wgpu::TextureUsages::COPY_DST,
                        view_formats: &[],
                    },
                    wgpu::TextureUses::PRESENT,
                    false,
                )
        };
        let mut view = None;
        unsafe {
            native.CreateShaderResourceView(&texture, None, Some(&mut view))?;
        }
        Ok(Self {
            texture: imported,
            view: view.context("shared surface view missing")?,
            fence,
            ready,
            value: AtomicU64::new(0),
            retirement: gpui::WindowsGpuSurfaceRetirement::new(native)?,
        })
    }
}

pub(super) struct Bridge {
    device: ID3D11Device,
    cached: Option<(u64, gpui::WindowsGpuSurface)>,
    pool: Vec<Arc<Allocation>>,
    copy: Option<wgpu::SubmissionIndex>,
    stats: NativeSurfaceStats,
}
impl Bridge {
    pub(super) fn new(device: ID3D11Device) -> Self {
        Self {
            device,
            cached: None,
            pool: vec![],
            copy: None,
            stats: NativeSurfaceStats::default(),
        }
    }
    pub(super) fn stats(&self) -> NativeSurfaceStats {
        NativeSurfaceStats {
            live_allocations: self.pool.len(),
            live_payload_bytes: self
                .pool
                .iter()
                .map(|allocation| {
                    u64::from(allocation.texture.width())
                        * u64::from(allocation.texture.height())
                        * 4
                })
                .sum(),
            ..self.stats
        }
    }
    pub(super) fn finish(&mut self, renderer: &WgpuHeadlessRenderer) -> Result<()> {
        if self.copy.take().is_some() {
            // A queue sentinel follows the raw ready signal as well as the copy.
            let complete = renderer.context.queue.submit([]);
            renderer.context.device.poll(wgpu::PollType::Wait {
                submission_index: Some(complete),
                timeout: Some(Duration::from_secs(30)),
            })?;
        }
        Ok(())
    }
    pub(super) fn snapshot(
        &mut self,
        renderer: &WgpuHeadlessRenderer,
        serial: u64,
    ) -> Result<gpui::WindowsGpuSurface> {
        if let Some((cached, surface)) = &self.cached
            && *cached == serial
        {
            self.stats.cache_hits += 1;
            return Ok(surface.clone());
        }
        renderer.check_native_surface_errors()?;
        let source = &renderer
            .render_target
            .as_ref()
            .context("render target missing")?
            .texture;
        let matches = |allocation: &Allocation| {
            allocation.texture.size() == source.size()
                && allocation.texture.format() == source.format()
        };
        // get_mut supplies exclusive ownership and the Arc acquire ordering;
        // a relaxed strong-count observation alone is not a reuse barrier.
        let index = self
            .pool
            .iter_mut()
            .position(|allocation| {
                Arc::get_mut(allocation)
                    .is_some_and(|allocation| allocation.idle() && matches(allocation))
            })
            .or_else(|| {
                self.pool.iter_mut().position(|allocation| {
                    Arc::get_mut(allocation).is_some_and(|allocation| allocation.idle())
                })
            });
        let allocation = if let Some(index) = index {
            if matches(&self.pool[index]) {
                self.stats.reuses += 1;
            } else {
                self.pool[index] = Arc::new(Allocation::create(renderer, &self.device, source)?);
                self.stats.allocations += 1;
            }
            self.pool[index].clone()
        } else {
            anyhow::ensure!(
                self.pool.len() < 8,
                "native GPU snapshot limit reached (8 retained or in-flight allocations)"
            );
            let allocation = Arc::new(Allocation::create(renderer, &self.device, source)?);
            self.pool.push(allocation.clone());
            self.stats.allocations += 1;
            allocation
        };
        let previous = allocation
            .value
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |value| {
                value.checked_add(1).filter(|value| *value != u64::MAX)
            })
            .map_err(|_| anyhow::anyhow!("native ready fence exhausted"))?;
        let value = previous + 1;
        let mut encoder =
            renderer
                .context
                .device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("retained_native_snapshot_copy"),
                });
        encoder.copy_texture_to_texture(
            source.as_image_copy(),
            allocation.texture.as_image_copy(),
            source.size(),
        );
        encoder.transition_resources(
            std::iter::empty(),
            std::iter::once(wgpu::TextureTransition {
                texture: &allocation.texture,
                selector: None,
                state: wgpu::TextureUses::PRESENT,
            }),
        );
        self.copy = Some(renderer.context.queue.submit([encoder.finish()]));
        // Signal on the submission queue after our own copy. Another renderer
        // sharing this context may add work, but cannot signal before this copy.
        let queue = unsafe { renderer.context.queue.as_hal::<wgpu::hal::api::Dx12>() }
            .context("DX12 queue missing")?;
        unsafe {
            queue.as_raw().Signal(&allocation.fence, value)?;
        }
        let surface = unsafe {
            gpui::WindowsGpuSurface::new(
                allocation.view.clone(),
                allocation.ready.clone(),
                value,
                allocation.retirement.clone(),
                allocation.clone(),
            )?
        };
        self.cached = Some((serial, surface.clone()));
        self.stats.copies += 1;
        self.stats.copied_bytes += u64::from(source.width()) * u64::from(source.height()) * 4;
        Ok(surface)
    }
}

impl WgpuHeadlessRenderer {
    // Immutable read, distinct from the owner-side mutable error observer.
    fn check_native_surface_errors(&self) -> Result<()> {
        anyhow::ensure!(!self.context.device_lost(), "native snapshot device lost");
        Ok(())
    }
}

impl RetainedQuadRenderer {
    /// Creates a DX12 consumer on the exact adapter used by the native window.
    pub fn new_for_windows_device(device: ID3D11Device, max_in_flight: usize) -> Result<Self> {
        anyhow::ensure!(
            (1..=8).contains(&max_in_flight),
            "max_in_flight must be 1..=8"
        );
        match WgpuContext::new_d3d11_headless(&device)
            .and_then(|(context, format)| WgpuHeadlessRenderer::from_context(context, format))
        {
            Ok(renderer) => {
                let mut result = Self::from_renderer(renderer, max_in_flight)?;
                result.native_surface = Some(Bridge::new(device.clone()));
                result.native_window_device = Some(device);
                Ok(result)
            }
            Err(error) => {
                log::warn!(
                    "Native retained GPU surfaces unavailable; using the readback bridge: {error:#}"
                );
                let mut result = Self::new(max_in_flight)?;
                result.native_window_device = Some(device);
                result.native_surface_error = Some(format!("{error:#}"));
                Ok(result)
            }
        }
    }

    pub fn native_surface_stats(&self) -> Option<NativeSurfaceStats> {
        self.native_surface.as_ref().map(Bridge::stats)
    }

    pub fn native_surface_error(&self) -> Option<&str> {
        self.native_surface_error.as_deref()
    }

    pub fn window_device_matches(&self, device: &ID3D11Device) -> bool {
        self.native_window_device
            .as_ref()
            .is_some_and(|current| current.as_raw() == device.as_raw())
    }

    /// An immutable GPU copy of the current retained target, cached per serial.
    pub fn window_surface(&mut self) -> Result<Option<gpui::WindowsGpuSurface>> {
        self.renderer.check_gpu_errors()?;
        self.native_surface
            .as_mut()
            .map(|bridge| bridge.snapshot(&self.renderer, self.serial))
            .transpose()
    }
}
