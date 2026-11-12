# Shared image and glyph placement — 2026-11-12

Windows / NVIDIA GeForce GTX 1650 Ti / Vulkan / driver 571.96. Source base is
`27ff3f7`. Final full validation and three focused executions each contain
**172 selected source hashes**, checked against final working-tree bytes.
[verification.json](verification.json) records the executable, source/process
audits and pixel-archive checksums.

Final regression passes **1939 hardware scenarios / 1849 byte-exact pixel
comparisons**. Each focused execution passes **540 scenarios / 513 comparisons**
before timing. Core tests pass **135**, retained-only tests **110**, and gallery/
font/hardware-click tests **12**. Selected core/gallery/perf all-target Clippy
passes `-D warnings`; the desktop WGPU executable builds. Logs and raw records
are retained. Every final PNG is preserved in each run's `pixels.zip`; selected
fractional-content and negative-rounding PNGs are also directly available under
`validation/`.

## Implemented contract and checks

Images and glyphs now reference the same bounded GPU placement hierarchy as
Quad/Shadow. They retain ordered border/padding content additions and local
geometry, so ancestor motion, clip and scroll need no per-resource placement
rewrite. Image records remain **48 bytes**, glyph properties **32 bytes**, and
spatial nodes **96 bytes**. Tags are u32 words rather than f32-encoded references.
Shared images also omit unused per-draw viewport dimensions, avoiding geometry
writes on surface-only resize. Legacy world placement is retained as a control.

`PaintItem` owns four additional logical f32 values for content offsets. Full and
selective publishers include changes even when current resolved boxes are equal.
Validation checks their exact ordered relationship with content origin. This
preserves snapshots after the producer drops and avoids reverse-subtraction or
collapsed-addition rounding differences. Common vertex code resolves shared
origin/clip and passes flat fragment inputs. Glyph baseline rounding stays global
and precedes target-camera subtraction.

Exact submitted-base routing suppresses unchanged image/glyph descriptors.
Inverse property memberships still invalidate cached layers with no descriptors.
Preflight checks retained shared references and actual projected image edges/
glyph baseline extremes, including ancestor-only changes, capacity/scale/enable
changes and unmentioned draws. Updates/removals can repair bindings atomically.
Memberships mutate only during application after all preflight checks pass.
Existing completion leases hold the referenced spatial buffers/settings/bindings
alongside textures and glyph buffers/pages.

New hardware coverage includes both content paths, publisher/spatial controls and
scales 1/1.25/1.5; mixed primitives, deep anchors, fractional clips, scroll,
opacity, material/radius/size/resource edits, reparenting, missing deltas/removals,
reset and switches. Padded content checks original ordered additions, text rewrap,
isolated cropped targets and live shared/absolute rebinding without glyph-instance
or texture rebuild. Surface-only resize/restoration compares pixels and 0 versus
48 image-property bytes. Old frames render after producer/rasterizer/font-context
drop. CPU-expected opaque image texels and negative-half glyph rounding pixels
supplement shared shader references.

Raw checks reject bad references/offsets/scales, disabling/truncating a spatial
prefix with retained bindings, local-plus-ancestor overflow, and later invalid
layer/damage input before serial/state changes. Valid retry preserves pixels.
An ancestor-only write rerasterizes a fixed-bounds layer with zero image/glyph
descriptors, proving invalidation independently of layer bounds. Separate image
and glyph cases reject overflow in unmentioned retained content. Atomic absolute
rebinding allows spatial disable, and re-enabling restores identical output.

The prior suite remains included. Mixed references use fresh full-size world
placement consumers with empty caches; pure Scene tests use original GPUI.
Primitive shader math is shared; this is not an independent-driver proof.

## Controlled mixed-scene measurements

Each pair contains one alpha image and one text node (`i`), plus their ordinary
transparent quad records. **1000 pairs means 2000 content nodes**, a parent/root
and **4000 draw calls**. All images share one immutable 1×1 texture; text shares
prepared glyph geometry/masks. The grid fits an 800×600 target.

Both modes use the same owned publisher, private transition checks, cached local
primitive routing, shared Quad/Shadow tree, adaptive backend index, shaders,
content and full-root redraw. Only shared content placement/input routing versus
per-draw world placement changes. The world control skips shared validation when
its membership index and incoming shared bindings are empty; it does not pay
unnecessary shared-table setup. These are two current controls, not different
releases. Earlier image/glyph/spatial timing controls explicitly disable this new
content transport to retain their earlier comparison scope.

Three executions each contain three rounds rotating mode order, six warmups and
twenty timed frames per mode/round: **180 samples per mode/case**. Every frame
includes normal owner commit/publication/submission and GPU completion, at most
one frame in flight. Phases separate commit, publication and submission. Setup,
readback and native presentation are excluded; **108** round-end reference checks
run outside timing. Percentiles use the nearest sorted sample as in the
executable, without filtering.

| Workload | Pairs | Completed median ms, shared / world | Change | Completed p95 ms, shared / world | Submission median µs, shared / world |
| --- | ---: | ---: | ---: | ---: | ---: |
| Ancestor move | 100 | 2.223 / 2.741 | -18.9% | 4.300 / 5.072 | 955.8 / 1317.3 |
| Clip toggle | 100 | 2.078 / 2.522 | -17.6% | 4.174 / 4.705 | 1010.7 / 1294.0 |
| One text color | 100 | 1.676 / 1.727 | -3.0% | 3.763 / 6.641 | 847.4 / 858.8 |
| Ancestor move | 1000 | 11.836 / 14.886 | -20.5% | 26.333 / 21.071 | 7455.2 / 10138.8 |
| Clip toggle | 1000 | 11.807 / 17.082 | -30.9% | 17.558 / 22.608 | 7333.8 / 10233.7 |
| One text color | 1000 | 9.433 / 9.115 | +3.5% | 14.866 / 13.340 | 6094.8 / 5924.6 |

Movement and clipping improve completed medians in all three executions at both
sizes on this host. At 1000 pairs, completed median improves **1.26×** for movement
and **1.45×** for clip changes. Submission medians fall about **26.5% / 28.3%**.
Both sides still extract **2001 nodes** and issue **4000 draw calls**. Placement
work per such update is:

| Content placement work, 1000 pairs | Shared | World control |
| --- | ---: | ---: |
| Image geometry bytes | 0 | 48,000 |
| Glyph property bytes | 0 | 32,000 |
| Image descriptor updates | 0 | 1000 |
| Glyph descriptor updates | 0 | 1000 |
| Shared spatial bytes | 96 | 96 |
| Texture / glyph-mask / glyph-instance bytes | 0 | 0 |

This removes **80,000 bytes** of content-placement writes and 2000 descriptor
updates per frame, rather than reducing root draw count or resolved extraction.
Camera/global updates and other GPU traffic are outside those specific counters.

Sparse text color has no stable completed-time benefit. Both modes extract one
node, upload one **16-byte material**, and retain all geometry/properties/textures.
At 1000 pairs the pooled median/p95 regress **3.5% / 11.4%**. At 100 pairs the
small pooled improvement reverses in runs 1 and 2. Shared vertices add hierarchy
work; these wall-time samples do not isolate GPU execution cost or establish it
as the sole cause of any regression.

Movement tail latency also does **not** establish improvement. The 1000-pair
shared p95 is **26.333 ms**, worse than 21.071 ms. Run-1 contains a slow shared
third block: its round median is **25.902 ms**, compared with 12.586/13.487 ms for
the preceding shared blocks. Its slowest frame is **86.602 ms**, including
77.816 ms submission; run-1 shared p95 is **40.457 ms** versus world 22.100 ms.
The cause is not established. Runs 2/3 have lower shared p95 than world, but that
does not justify discarding run-1. Every sample, execution/round median and phase
is retained in [summary.json](summary.json) and run-1 through run-3.

## Preserved initial probe, reproduction and limits

`initial-probe` retains the first three paired executions and full validation,
before removing empty shared-index setup from the legacy path. Its four
172-source manifests were checked before that cleanup. Exact changed sources
and tracked patch are preserved as `source-snapshot.zip` / `source.patch.zip`,
and reconstructed manifests were checked again when freezing. Raw timing,
validation and process records remain; probe PNGs remain in target directories.
Final claims above use the final paired controls. Host drift prevents treating
differences between probe and final executions as an isolated fast-path gain.

```powershell
./tooling/rendering-perf/run-retained-gpu.ps1 -Output target/content-validation -ValidationOnly
./tooling/rendering-perf/run-retained-gpu.ps1 -Output target/content-run-1 -ContentSpatialOnly
# Repeat with new output directories for runs 2 and 3.
```

Other build/qualification jobs were observed before final timing and allowed to
finish. Final validation/run-1/run-2/run-3 have **97/41/39/42** process observations
at 500 ms intervals, all without compiler processes. Each runner builds before
launch. Sampling can miss shorter activity and excludes no unrelated host/GPU
work. Rotating block order is not perfectly balanced; scheduling, thermal/clock,
driver and queue effects remain possible. Phase medians do not add exactly to
pooled completion medians; completion differences are not GPU timestamps.

The index adds CPU local offsets and memberships, and shared vertices perform
bounded walks. There is no measured RSS/global physical-budget claim. Complete
resolved subtree extraction, resource-ID/order/layer/lease scans, image batching,
measured adaptive cost policy, broader primitives/layout, device/surface loss,
native-surface presentation and cross-platform/sustained 60/120 Hz/input/energy
acceptance remain open in the [full-goal tracker](../../RETAINED_IMPLEMENTATION_PROGRESS.md).
