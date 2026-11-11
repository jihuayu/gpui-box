# Downstream changes

The reference source remains Zed revision
`f8cfc7ed9e57503def9479138894f3139f5400cc`. `UPSTREAM.json` records the original
copied bytes. Its hashes intentionally describe the source baseline.

## Android M0

`crates/gpui/src/gpui.rs` adds Android to the existing priority-queue module and
re-export target gates. The Android platform uses the same priority scheduler
as upstream's Windows, Linux, and web backends. No queue algorithm changes are
needed. This is suitable for an upstream Android enablement patch.

The new Android backend is maintained as distribution-owned
`crates/gpui-android` at the repository root while its JNI/SurfaceView contract is
developed. This keeps experimental integration separate from the copied source
baseline. The official GPUI trait implementations and renderer remain path
dependencies on this snapshot. Existing upstream backend files are retained.

Verification on 2026-11-11 found 410 of 411 source entries matching their original
hashes. `crates/gpui/src/gpui.rs` is the single intentionally modified entry.
The reduced workspace manifest and materialized license links are the existing
initial-import adaptations.

## Single-quad GPU upload experiment

The opt-in `gpui_wgpu/quad-patch-prototype` feature adds a fixed-scene opaque
solid-quad experiment. It shares the existing Quad ABI, pipelines, shaders,
headless pixel readback and full `render_frame` reference path. A dedicated
storage buffer and stable binding support either full-array uploads or a single
160-byte quad write. Session-scoped handles reject foreign and retired scenes.
The queue orders writes with prior submitted reads. GPU errors poison a session
until it is recreated.

`wgpu_renderer.rs` adds feature-gated instance-write counters, CPU spans and
render-pass timestamps. `wgpu_context.rs` exposes two helpers within the crate
and extends headless feature gates. `gpui_wgpu.rs` exports the experiment behind
the feature. The experiment has its own hardware device with timestamp support
when available. Default platform builds retain their existing runtime path.

The experiment is driven by `tooling/rendering-perf` at the distribution root.
Its report records actual adapter, source hashes, pixel comparisons and timing
scope. `UPSTREAM.json` continues to identify the original import bytes. The
410/411 count above describes the earlier M0 audit before this experimental
patch, rather than the current checkout's hash match count.

## Retained FramePacket quad consumer

The separate opt-in `retained-renderer` feature enables
`wgpu_renderer/retained.rs`. It owns persistent storage buffers and bindings,
accepts batched instance ranges, preserves explicit draw order and holds replaced
allocations through submission completion. Its bounded queue polls the oldest
submission at the configured limit. Headless feature gates also accept this
feature so distribution-owned retained frames can be validated with pixel
readback. The distribution facade owns node identity, frame revisions and the
generic instance allocation table in `crates/gpui-box/src/retained`.

Validation includes same-backend pixel comparison for resizing, growth,
transparency, reordering, deletion/reuse, dropped frames, explicit reconstruction
and a 10,000-object batch. The desktop test app exercises click-to-offscreen-GPU
submission. Native window surface presentation remains separate integration work.

### Preserved offscreen target and damage redraw

The retained consumer accepts Full, rectangular Damage and Unchanged redraws.
Damage redraw loads the preserved offscreen texture, clears only the scissor
with an opaque black quad, and replays ordered instances through the same GPUI
quad shader. Resize and reconstruction force a full clear/redraw. A separate
reference renderer prevents pixel checks from overwriting the live target.
An explicit `finish` waits for completion without readback for throughput A/B.
Clear allocations remain leased through queued work. This implementation does
not assume preserved pixels on a native swapchain or implement group opacity.

### Isolated opacity layers

The next stage adds `retained/layers.rs` and `layers.wgsl`. A postorder composition
plan references persistent quad ranges and isolated layers. Layers render to
transparent textures; the composite shader uses premultiplied blending and one
opacity uniform. Content, draw-order and nested-effect changes invalidate the
appropriate caches. Opacity-only changes reuse the affected layer contents.
Full-size layer working sets have a 256 MiB validation guard; unused layers leave
the cache and in-flight texture/buffer/binding leases protect queued work.
The guard does not yet account for peak memory in overlapping allocation epochs.
An explicit cache-disable control supports completed-work hardware A/B. A BGRA
readback helper bridges isolated output to existing window image paint while
direct GPU texture import remains pending.
