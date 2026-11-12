use super::*;
use windows::{
    Win32::Graphics::{Direct3D11::ID3D11Device, Dxgi::IDXGIDevice},
    core::Interface,
};

impl WgpuContext {
    pub(crate) fn new_d3d11_headless(
        native: &ID3D11Device,
    ) -> anyhow::Result<(Self, TextureFormat)> {
        let dxgi: IDXGIDevice = native.cast()?;
        let luid = unsafe { dxgi.GetAdapter()?.GetDesc()? }.AdapterLuid;
        // Keep cached contexts alive through process shutdown, as for HEADLESS.
        // Adapter identity, rather than name/vendor/device IDs, handles multi-GPU.
        type Cached = (i32, u32, WgpuContext, TextureFormat);
        static CONTEXTS: Mutex<Vec<Cached>> = Mutex::new(Vec::new());
        let mut contexts = CONTEXTS.lock();
        if let Some((_, _, context, format)) = contexts.iter().find(|(high, low, context, _)| {
            *high == luid.HighPart && *low == luid.LowPart && !context.device_lost()
        }) {
            return Ok((context.clone(), *format));
        }
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::DX12,
            flags: wgpu::InstanceFlags::default(),
            backend_options: wgpu::BackendOptions::default(),
            memory_budget_thresholds: wgpu::MemoryBudgetThresholds::default(),
            display: None,
        });
        let adapters = gpui::block_on(instance.enumerate_adapters(wgpu::Backends::DX12));
        for adapter in adapters {
            if adapter.get_info().device_type == wgpu::DeviceType::Cpu {
                continue;
            }
            let hal = unsafe { adapter.as_hal::<wgpu::hal::api::Dx12>() }
                .ok_or_else(|| anyhow::anyhow!("DX12 adapter missing"))?;
            let desc = unsafe { hal.raw_adapter().GetDesc()? };
            if desc.AdapterLuid.HighPart != luid.HighPart
                || desc.AdapterLuid.LowPart != luid.LowPart
            {
                continue;
            }
            drop(hal);
            let format = Self::headless_target_format(&adapter)
                .ok_or_else(|| anyhow::anyhow!("native target format unsupported"))?;
            let (device, queue, dual_source, color_format) =
                gpui::block_on(Self::create_device(&adapter))?;
            let context = Self::from_native_adapter_and_device(
                instance,
                adapter,
                device,
                queue,
                dual_source,
                color_format,
            );
            contexts.push((luid.HighPart, luid.LowPart, context.clone(), format));
            return Ok((context, format));
        }
        anyhow::bail!("no hardware DX12 adapter matches the native window")
    }
}
