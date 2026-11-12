//! Immutable, premultiplied GPU image shared with the native Windows renderer.
use std::{
    any::Any,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};
use windows::{
    Win32::Graphics::Direct3D11::{
        ID3D11Device, ID3D11Device5, ID3D11DeviceContext4, ID3D11Fence, ID3D11ShaderResourceView,
    },
    core::Interface,
};

/// An immutable GPU image. Clones retain the foreign allocation and synchronization
/// objects, including when GPUI caches a scene or its producing view is dropped.
#[derive(Clone)]
pub struct WindowsGpuSurface(Arc<Inner>);

struct Inner {
    device: ID3D11Device,
    view: ID3D11ShaderResourceView,
    fence: ID3D11Fence,
    value: u64,
    size: [u32; 2],
    retirement: WindowsGpuSurfaceRetirement,
    _owner: Arc<dyn Any + Send + Sync>,
}

/// Tracks the last native GPU read before a foreign allocation can be reused.
#[derive(Clone)]
pub struct WindowsGpuSurfaceRetirement(Arc<ReadFence>);
struct ReadFence {
    fence: ID3D11Fence,
    last: AtomicU64,
}
impl WindowsGpuSurfaceRetirement {
    /// Creates a device-local native read fence.
    pub fn new(device: &ID3D11Device) -> anyhow::Result<Self> {
        let device: ID3D11Device5 = device.cast()?;
        let mut fence: Option<ID3D11Fence> = None;
        unsafe {
            device.CreateFence(
                0,
                windows::Win32::Graphics::Direct3D11::D3D11_FENCE_FLAG_NONE,
                &mut fence,
            )?;
        }
        Ok(Self(Arc::new(ReadFence {
            fence: fence.ok_or_else(|| anyhow::anyhow!("native read fence missing"))?,
            last: AtomicU64::new(0),
        })))
    }
    /// A nonblocking GPU completion query. CPU snapshot ownership must also end
    /// before reuse; a completed read alone does not permit overwriting a cache.
    pub fn is_complete(&self) -> bool {
        let completed = unsafe { self.0.fence.GetCompletedValue() };
        completed != u64::MAX && completed >= self.0.last.load(Ordering::Acquire)
    }
    fn signal(&self, context: &ID3D11DeviceContext4) -> anyhow::Result<()> {
        let previous = self
            .0
            .last
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |value| {
                value.checked_add(1).filter(|v| *v != u64::MAX)
            })
            .map_err(|_| anyhow::anyhow!("native read fence exhausted"))?;
        // Record before signalling. Failure quarantines the allocation instead
        // of making it look reusable after an untracked native draw.
        unsafe {
            context.Signal(&self.0.fence, previous + 1)?;
        }
        Ok(())
    }
}

impl std::fmt::Debug for WindowsGpuSurface {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WindowsGpuSurface")
            .field("size", &self.0.size)
            .finish_non_exhaustive()
    }
}

impl WindowsGpuSurface {
    /// Wraps a shared, premultiplied RGBA/BGRA UNORM image.
    ///
    /// # Safety
    /// The view, ready fence and retirement fence must use the same device.
    /// The producer must enqueue a
    /// GPU signal of `value` after fully initializing the texture and releasing
    /// foreign write access. No writes, reuse or destruction may occur while any
    /// clone exists or the native GPU is reading it. `owner` must retain the
    /// foreign allocation. D3D11's own command references retain native reads.
    pub unsafe fn new(
        view: ID3D11ShaderResourceView,
        fence: ID3D11Fence,
        value: u64,
        retirement: WindowsGpuSurfaceRetirement,
        owner: Arc<dyn Any + Send + Sync>,
    ) -> anyhow::Result<Self> {
        use windows::Win32::Graphics::{Direct3D11::*, Dxgi::Common::*};
        let device = unsafe { view.GetDevice()? };
        let texture: ID3D11Texture2D = unsafe { view.GetResource()? }.cast()?;
        let mut desc = D3D11_TEXTURE2D_DESC::default();
        unsafe {
            texture.GetDesc(&mut desc);
        }
        anyhow::ensure!(
            desc.Width > 0
                && desc.Height > 0
                && desc.MipLevels == 1
                && desc.ArraySize == 1
                && desc.SampleDesc.Count == 1
                && matches!(
                    desc.Format,
                    DXGI_FORMAT_R8G8B8A8_UNORM | DXGI_FORMAT_B8G8R8A8_UNORM
                ),
            "unsupported GPU surface texture"
        );
        Ok(Self(Arc::new(Inner {
            device,
            view,
            fence,
            value,
            size: [desc.Width, desc.Height],
            retirement,
            _owner: owner,
        })))
    }

    /// Dimensions in physical pixels.
    pub fn size(&self) -> [u32; 2] {
        self.0.size
    }
    /// Tests COM device identity, including after native device recovery.
    pub fn belongs_to(&self, device: &ID3D11Device) -> bool {
        self.0.device.as_raw() == device.as_raw()
    }
    /// Native shader resource view.
    pub fn texture_view(&self) -> &ID3D11ShaderResourceView {
        &self.0.view
    }
    /// Producer completion fence; waits must be enqueued on the native GPU.
    pub fn ready_fence(&self) -> &ID3D11Fence {
        &self.0.fence
    }
    /// Completion value for this immutable snapshot.
    pub fn ready_value(&self) -> u64 {
        self.0.value
    }
    /// Enqueues native read retirement after the last draw sampling this surface.
    pub fn signal_read_complete(&self, context: &ID3D11DeviceContext4) -> anyhow::Result<()> {
        self.0.retirement.signal(context)
    }
    /// Whether two handles refer to the same immutable snapshot.
    pub fn ptr_eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}
