# Bounded isolation texture evidence — 2026-11-12

Windows, NVIDIA GeForce GTX 1650 Ti, Vulkan, driver 571.96. Source base is
`2efcd4a`; metadata captures the tested working changes in 148 selected source
hashes, including the shared GPUI shader. All four manifests match the final
source bytes. The binary SHA-256 and checks are in [verification.json](verification.json).

The full final run passes 208 hardware scenarios: 207 byte-exact pixel comparisons
and one public-backend malformed/budget preflight with a valid retry. Core tests
pass 114, gallery/font and real hardware-click tests pass 12. Clippy passes
`-D warnings` across core/gallery/perf targets, and the desktop WGPU executable
builds. The [test log](layer-bounds-tests.log), [Clippy log](layer-bounds-clippy.log),
[full validation records](validation.json), PNGs and runner stdout are preserved.

## Completed-work comparison

Each case uses the same tree, renderer, shaders, Full root redraw, resource
updates, queue limit one and per-frame GPU completion. The control disables tight
bounds and allocates surface-sized layers. This is not a comparison against a
previous executable: both paths use the new per-target coordinates and bounded
rectangle composite pipeline. The timed scope includes owner transaction,
publication, submission and completion; it excludes readback/native-window paint.
No FPS, GPU-timestamp or input-latency claim follows from these results.

Each of three independent executions contains three rotating-order rounds,
six warmups and twenty measured frames per mode/round. Pooled values below use
180 observations per cell. Median is the ordinary sample median; P95 uses
nearest rank `ceil(0.95*n)`. Per-run medians, CPU spans and every completed sample
are in [summary.json](summary.json) and the three run subdirectories.

| Surface | Workload | Tight median / P95 µs | Full median / P95 µs | Median ratio full/tight |
| --- | --- | ---: | ---: | ---: |
| 1920×1080 | 8 small groups, cached opacity | 610.5 / 885.2 | 1129.8 / 1600.6 | 1.85× |
| 1920×1080 | 8 small groups, content raster | 887.8 / 1318.3 | 1424.9 / 2172.1 | 1.60× |
| 1920×1080 | 1 full-surface group, content raster | 1698.5 / 2381.6 | 1754.8 / 2391.7 | 1.03× |
| 3840×2160 | 8 small groups, cached opacity | 1220.6 / 1761.0 | 2451.3 / 3176.6 | 2.01× |
| 3840×2160 | 8 small groups, content raster | 1301.6 / 1912.5 | 2551.1 / 3391.6 | 1.96× |
| 3840×2160 | 1 full-surface group, content raster | 4520.6 / 5617.5 | 4375.3 / 5225.2 | 0.97× |

Small cases use eight 96×64 groups with four overlapping quads each. A one-pixel
fringe gives eight 98×66 allocations, **206,976 bytes** in both resolutions.
The full control uses **66,355,200 bytes** at 1080p and **265,420,800 bytes** at 4K.
For material changes, raster target area is **51,744 pixels** versus
16,588,800 / 66,355,200. Both paths upload the same 5,120 instance bytes. Cached
opacity changes upload 128 property bytes, zero instance bytes, rerasterize no
layers and inspect zero item bounds. Pixel comparisons pass after every round.

The full-surface control retains identical texture payload and raster area in
both modes. It shows no consistent benefit: pooled 1080p is about 3.2% faster
with tight bounds while 4K is about 3.3% slower. The 4K per-execution median
pairs are 4377/4357, 4773/4445 and 4498/4344 µs. Thermal, scheduling, polling and
other host activity remain possible contributors; these controls and outliers
are preserved, not subtracted from small-group results.

For the existing nine 48×40 group budget case, bounded payload is **75,600
bytes** at 4K. Full-viewport mode would require 298,598,400 bytes and rejects the
256 MiB live payload guard. Nine actual full-surface bounded layers also reject;
malformed rectangles reject before allocation and a subsequent valid empty
layer succeeds. These are texture payload counters rather than measured driver
allocation or peak physical memory. Retired epochs, alignment, buffers, decoded
images/glyphs and the main target are outside that counter/guard.

## Reproduction and measurement boundaries

```powershell
./tooling/rendering-perf/run-retained-gpu.ps1 -Output target/layer-bounds-final-validation -ValidationOnly
./tooling/rendering-perf/run-retained-gpu.ps1 -Output target/layer-bounds-run-1 -LayerBoundsOnly
# Repeat with new output directories for run 2 and run 3.
```

The runner builds before starting the executable, records source/adapter
metadata, and observes cargo/rustc/clippy processes every 500 ms. The final full
validation and repeated focused executions have respectively 8, 10, 9 and 9
observations with **zero observed compiler processes**. This does not exclude
shorter processes or other system/GPU activity. In particular, there is visible
host variation between executions, so the raw run data is part of the evidence.
No timers/benchmarks ran concurrently with our own compilation.

The fresh hardware reference forces full-viewport bounds and an empty raster
cache. Analytical sibling/nested-alpha assertions additionally verify isolation
semantics. New cases cover round borders, alpha images, real grayscale glyphs
including ink beyond layout boxes, nested target origins, negative/fractional
movement, clips, offscreen/empty groups, hidden updates/restoration, scale
1/1.25/1.5, queued reallocations and reset. The reference shares the primitive
shaders and cannot independently qualify every shader semantic or backend.

Native surface import, affine GPU property trees, general primitives, global
cross-frame physical budgets/LRU/admission, sustained animation/input/energy and
cross-platform acceptance remain in the [full goal](../../RETAINED_IMPLEMENTATION_PROGRESS.md).
