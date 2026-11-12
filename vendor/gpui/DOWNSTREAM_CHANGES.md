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

### Bounded retained isolation targets

Retained `QuadLayer` adds optional physical bounds (`None` is the full-viewport
control). Its 256 MiB guard now sums bounded extents and validates rectangles
before allocation. Target origin/size camera buffers and quad globals are
immutable per target and explicitly leased through completion, including root
cameras. Layer extent/origin changes replace allocations and invalidate parents;
opacity-only writes still preserve cached contents. Submission counters expose
live/allocated texture payload and raster target pixels, excluding retired epochs
and driver overhead.

The shared `GlobalParams` Rust/WGSL ABI gains origin/padding (32 bytes total).
Legacy initialization keeps zero origin. Quad fragment gradient/SDF coordinates
recover world position for cropped targets. Retained image/glyph pipelines bind
an additional camera group; resource ABIs remain unchanged. Layer composition
uses a bounded six-vertex rectangle and local texture loads. Distribution-owned
page ink unions select conservative device bounds, and fresh reference rendering
forces full-viewport textures. See `docs/RETAINED_LAYER_BOUNDS.md` in the parent
repository for validation, measurements and remaining resource-budget limits.

### Completion-held isolated texture allocation budget

`retained/allocation_budget.rs` adds shared reservations per actual layer texture
allocation. Cache entries and explicit completion leases share one reservation;
only the final reference releases its byte/count accounting. Admission prunes
stale cache entries after packet validation, computes replacement payload and
waits for oldest submissions when overlapping epochs would exceed the cap.
Default cap remains 256 MiB; a positive configurable cap finishes/retires the
previous budget epoch, clears layer caches and forces resynchronization.
`LayerMemoryStats` observes live/resident/retired/peak payload and unique allocation
count without polling; submission stats also record budget-pressure waits.

The cap covers isolated texture payload within one renderer/device epoch. Driver
alignment/pooling, nonlayer resources and other device epochs are outside it;
oversized live scenes still reject. The distribution tests include the pure
reservation module directly and exercise actual pressure waits, idle native
completion retirement, reset policy retention, and atomic oversized-packet
rejection/retry. See `docs/RETAINED_LAYER_BUDGET.md` in the parent repository.

### Reuse layer allocations on physical origin changes

Same-extent target moves now retain their texture, view, buffers, bindings and
shared allocation reservation. Each distinct layer's camera/global buffer allows
COPY_DST; once-per-submission origin writes (48 bytes) follow prior submitted
reads on the same queue. This supersedes immutable-on-origin target creation:
buffers remain distinct per layer, and no target camera changes between passes
inside one encoder. Rebasing always invalidates contents and nested parents,
including when the caller supplied no primitive/resource writes. Extent changes
still use replacement/admission. The default-on reuse setting has a forced
replacement control and survives consumer reset. Submission stats distinguish
rebased textures and camera/global payload. The distribution hardware checks
mixed nested/fractional scenes and an independent target-only crop oracle; its
budget-pressure controls explicitly disable reuse. See the parent repository's
`docs/RETAINED_LAYER_REUSE.md` for measurements and remaining GPU-property work.

### Persistent rounded-box shadows and typed instance storage

`retained/storage.rs` shares typed CPU mirrors, geometric growth, range uploads,
validation and explicit completion leases for the pinned Quad (160-byte) and
Shadow (112-byte) ABIs. `QuadBatch` adds independent shadow capacity/writes;
composition adds shadow draw ranges, separate dirty-range raster invalidation
and shadow upload/generation counters. Shadow geometry/color/radius/expanded
paint bounds and draw/write ranges are preflighted. A backend-only full declared
instance-prefix upload control leaves logical dirtiness/raster work unchanged;
it excludes geometric spare slots and unused typed placeholders.

The shared Scene culls/orders shadows using conservative paint ink: a three-blur
margin for drop shadows, element bounds for inset shadows. This preserves visible
blur tails and collapsed inset holes without changing public primitive body
bounds. The shared WGPU shadow fragment shader adds each target's world origin
for cropped isolation coordinates. Quad/shadow vertices collapse empty-mask
geometry, preventing one-pixel lines at fractional zero-width/height clips.

The distribution owner supplies immutable rounded-box shadow lists, shared typed
source sinks, overflow damage, persistent CPU slots and ordinary GPUI drop/inset
window paint. Hardware tests exercise mixed text/images, phase changes, batch
writes, malformed retry and buffer growth with an outstanding prior submission.
See `docs/RETAINED_SHADOWS.md` and its frozen evidence in the parent repository.
Vulkan output is qualified here; native DirectX/Metal framebuffer verification,
general-shape primitives and global resource budgets remain pending.

### Shared hierarchical retained placement

`retained/spatial.rs` and `spatial.wgsl` share 96-byte logical placement nodes
between Quad and Shadow, using common typed mirrors/growth/uploads and explicit
completion-held property buffer/settings/binding leases. Typed initial GPU
placeholders now match CPU empty records, including nonzero empty-anchor
sentinels. A retained-only shader
variant transforms the pinned shared transport loads, passes world origin/clip
as flat varyings and leaves fragment primitive math free of tree traversal.
Exact source-match assertions guard the variant against upstream drift.

Enabled batches use `order = property slot + 1`; zero retains world instances.
Retained shadow raw origins carry logical offset/signed expansion until vertex
resolution. The pinned struct sizes do not change. Parent/depth/reference/range
and resolved finite-coordinate preflight runs before mutation; bounded 32-node
walks use resolved anchors for deeper trees. Property ancestry changes invalidate
raster ranges even with no geometry writes. Submission stats expose property
bytes/ranges and generation independently of compositor opacity. Default raw
batches retain the original convention; the distribution consumer defaults to
shared placement and provides a world-instance/reset-preserved control.

The shared Quad/Shadow fragment mask test now uses exact world pixel centers.
Interpolated distances could change sign at an inclusive mask boundary between
full and cropped targets, dropping a row of shadow. Direct comparisons preserve
inclusive minimum/maximum edges and empty-mask behavior. Analytical color checks
cover fill/shadow, both property modes and both target extents. Native Vulkan
pixels are verified; native DirectX/Metal framebuffer qualification remains open.

Owner packets now publish local placement independently of resolved hit/damage
geometry. Reparenting rebuilds depth/anchor metadata even with unchanged world
bounds/order; retired GPU slots clear to empty anchors before reuse. The hardware
suite covers mixed text/images, scrolling, isolated caches, deep/recycled chains,
malformed retry and overlapping property-buffer replacement. See the parent
repository's `docs/RETAINED_GPU_SPATIAL.md`, architecture decision and evidence.
Full affine/complex clips, shared image/glyph adoption, indexed preflight and
global physical budgets remain pending.
