# Isolated subtree opacity and raster reuse

`Transaction::opacity` applies opacity to an isolated subtree. Overlapping
children first resolve in a transparent intermediate texture, then the whole
texture composites once with premultiplied blending. Nested groups have separate
effect nodes and render in postorder. Geometry, clip and scroll continue to use
the same owned FramePacket; opacity changes skip Taffy and command recording.

This extends the [clip/damage stage](RETAINED_PROPERTIES_DAMAGE.md). The
[complete-runtime goal](RETAINED_IMPLEMENTATION_PROGRESS.md) remains active.

## Property and raster lifecycle

The packet owns a sparse effect tree. Every paint item points to its nearest
effect, whose parent describes nesting. Values must be finite and in 0..=1;
invalid transactions apply no partial changes. Equality/coalescing still avoid
new frames. Damage compares old/new effect chains as well as ink/clip geometry.

The GPU composition plan references generation-checked instance ranges and
layer IDs containing tree/slot/generation. Layer contents are cached until a
direct instance write, order change or nested child's visual change invalidates
them. Changing only a group's own opacity updates its 16-byte uniform, uploads
zero instance bytes and reuses the group's raster. A nested opacity change
reuses that child's raster and invalidates its parent's contents.

Removing an effect or subtree evicts its unused layer from the live cache.
Resize, scale and renderer recreation rebuild textures. In-flight submissions
hold explicit leases on textures, uniforms and bindings, with the existing
three-frame queue bound. All writes and draws use the same ordered WGPU queue.
A switch can disable cache reuse for a semantic equivalent A/B control.

Layers now default to conservative clipped content bounds, including nested
content and grayscale glyph ink; see [bounded isolation textures](RETAINED_LAYER_BOUNDS.md).
A full-viewport switch remains as a reference/performance control. The validation
guard rejects actual live texture extents above 256 MiB before allocation. The
hardware suite checks both small-layer admission and oversized rejection/recovery.
The subsequent [allocation-budget stage](RETAINED_LAYER_BUDGET.md) accounts
unique layer payload across overlapping in-flight epochs and waits on pressure.
Driver/nonlayer/global-device physical memory is not accounted by that cap. Cache admission/LRU, configurable
global budgets and graceful over-budget fallback remain acceptance work.

Translation/clip/scroll still resolve into world-space quad records. They can
invalidate cached raster contents. Reusing raster through GPU transform/clip
properties remains pending. Filters and blend modes remain pending. Subsequent text/image integration and
bounded targets are covered by the linked follow-up document.

## Normal window integration

With `retained-wgpu`, the gallery has an isolated-opacity button. The normal
event handler commits the property and requests a frame. When effects exist,
prepaint obtains the GPU result and paint displays it through GPUI image paint.
The bridge caches the image by submission serial, converts RGBA readback to
GPUI's BGRA format and removes the previous image from the sprite atlas when it
is replaced or isolation ends. Effect-free output retains ordinary quad paint.

This bridge waits for readback and reuploads the image through the legacy atlas.
Direct GPU texture import/native presentation remains pending; the backend A/B
below excludes this bridge and cannot establish its end-to-end window benefit.
The hardware click test verifies the event's isolated GPU pixels, byte-for-byte
bridge conversion, same-serial image reuse, atlas presence and image retirement.
It does not capture the final native-window framebuffer.

## Hardware correctness

The current runner passes 43 image scenarios on GTX 1650 Ti / Vulkan: the prior
26 plus 17 group cases. These cover isolated overlap, opacity-only cache reuse,
content invalidation, nested effects, nested-opacity propagation, zero opacity,
hidden content updates and restoration, effect removal/recreation, fractional
scale/movement, reset, 32 queued opacity updates, clipping, nested/subtree
deletion, and working-set budget recovery.

Effect-free frames use the original full GPUI renderer as oracle. Legacy GPUI
flattens element opacity into individual primitives, so it cannot provide the
new isolation semantics. Effect frames compare with a fresh full isolated
renderer whose raster cache starts empty. Independent analytical pixel checks
additionally verify opaque sibling overlap and nested group blending; they
would fail if opacity were flattened into each child. Each validation entry
records which reference path was used.

The opacity-only case asserts zero instance bytes, 16 property bytes, zero
rasterized layers and one reused layer. The nested-opacity case asserts one
reused child and one rasterized parent. Sixteen core tests and both ordinary
click tests passed, including the explicitly enabled hardware case. Clippy with
`-D warnings` passed for the core, gallery (with/without retained WGPU) and runner.
The desktop executable built, and the existing GPU tool checked with retained
support disabled.

## Completed-work cache A/B

The same 32 translucent full-viewport quads form one isolated group. Only group
opacity alternates between 0.5 and 0.75. Both paths use full main-target redraw;
one rerasterizes the layer on every update and the other reuses it. Three rounds
rotate ordering, with eight warmup updates and forty measured updates per path
per round. The queue allows three frames and final completion is awaited.

| Surface | Rerasterize every update | Reuse raster | Speedup |
| --- | ---: | ---: | ---: |
| 1920×1080 | 5.231 ms | 0.228 ms | 22.97× |
| 3840×2160 | 20.829 ms | 0.827 ms | 25.18× |

Both paths upload zero instance bytes and 16 property bytes per change. The
cached path rasterizes zero layers per measured update. Final pixels match the
fresh full isolation path for every round. Timings include CPU transaction/frame
work, submission, backpressure and GPU completion; they exclude readback and
presentation and are not GPU-only timestamps. These are deliberate fill-rate
stress scenes, not ordinary application-wide speedups. Raw per-round values and
the repeated Full/Damage/Auto control are frozen with the source hashes.

```powershell
./tooling/rendering-perf/run-retained-gpu.ps1 -Output target/retained-layers-gpu
mise exec -- cargo test --locked --offline -p gpui-box --features retained-wgpu
mise exec -- cargo test --locked --offline -p gpui-box-example-hello --features retained-wgpu normal_click -- --include-ignored
```

[Frozen evidence](performance/2026-11-11-layers/README.md) preserves all images,
validation, adapter information and the measured source identities.
