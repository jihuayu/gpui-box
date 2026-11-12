# Windows retained GPU surface interoperability

Status: implemented prototype; full RFC acceptance remains open.

The Windows GPUI window renderer uses D3D11. The ordinary retained headless
consumer selects Vulkan on this machine. Passing its texture to the window is
therefore not a same-device texture import. The previous window adapter copied
pixels to the CPU and uploaded a new sprite image.

`WgpuFrameRenderer::new_for_window` now obtains the native D3D11 device and creates
a separate DX12 consumer on the matching adapter **LUID**. It does not change the
default headless backend or earlier benchmark controls. Device contexts are cached
as for the existing headless context and recreated when their lost state is known.
If no matching hardware DX12 adapter can produce the shared consumer, construction
falls back to the ordinary headless backend and the existing CPU readback bridge.

A GPU copy moves the preserved retained target into a shared committed texture.
It transitions back to COMMON/PRESENT before a ready fence is signalled on the
actual submission queue. An unnamed NT handle opens the resource and fence on
D3D11; handles close immediately after import. The tested importer rejected the
initial texture flags; adding render-target binding made import succeed. The
native painter samples the imported SRV, enqueues a ready-fence wait and draws at
the ordinary scene order with content-mask clipping and premultiplied blending.
It then signals a device-local native read fence.

The output is an immutable snapshot while any view, cached scene or caller owns
it. A pool of at most eight allocations per consumer epoch reuses a texture only
after exclusive `Arc::get_mut` ownership, producer completion and native read
completion all agree. Merely dropping the producing `FramePacket` does not permit
reuse. Read-fence signal failure leaves the allocation unavailable. Device-removed
fence values are never treated as completion. An extent change can replace an
idle allocation, preserving older leased extents. Explicit consumer reset retains
native backend selection; snapshots from the previous consumer remain owned.

The normal painter calls `submit_for_window`, which compares the current platform
D3D11 identity with the consumer's bound identity before submitting. A replacement
platform device creates a new consumer epoch, invalidates all GPU-side caches and
arenas, and consumes the next owned `FramePacket` as a complete resynchronization.
Logical and render inputs remain with the UI owner. A consumer whose optional DX12
surface is unavailable keeps that fallback selection stable instead of retrying on
every frame. Explicit `reset` advances the same epoch counter without being counted
as a platform recovery.

The window size uses the same snapped near/far edges as the surface painter,
preventing a ceil-sized offscreen output from being resampled into a different
pixel extent. The gallery uses this path for its entire retained canvas, including
plain quads. `GPUI_RETAINED_READBACK=1` selects the existing CPU image bridge while
keeping the same DX12 consumer. That bridge now converts premultiplied output to
the straight-alpha sprite input; opaque pixels keep the simple channel swap.

This is a full-target **GPU copy**, followed by normal window scene rendering.
It is not direct rendering into a native swapchain, native incremental damage,
zero-copy presentation, a global memory budget, or a claim about OS presentation
latency. Three native qualification processes replace the real D3D11 identity and
verify post-recovery pixels, but this deterministic replacement does not induce a
driver TDR or physical adapter removal. Physical device-removed qualification,
more platforms and end-to-end visible presentation measurements remain acceptance
work.

Primary API references: [shared heaps](https://learn.microsoft.com/en-us/windows/win32/direct3d12/shared-heaps),
[CreateSharedHandle](https://learn.microsoft.com/en-us/windows/win32/api/d3d12/nf-d3d12-id3d12device-createsharedhandle),
[D3D11 fences](https://learn.microsoft.com/en-us/windows/win32/direct3d11/direct3d-11-4-features).
