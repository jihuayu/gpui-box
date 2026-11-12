# Shared hierarchical GPU placement — 2026-11-12

Windows / NVIDIA GeForce GTX 1650 Ti / Vulkan / driver 571.96. Source base is
`fb8180f`. Final full validation and three focused executions each carry **160
selected source hashes**, all checked against final working-tree bytes. The
final binary hash, process audits and buffer-growth record are preserved in
[verification.json](verification.json).

The full regression passes **399 hardware scenarios**, including **394 byte-exact
pixel comparisons**, one analytical origin-only scenario and four preflight/retry
groups. Each focused execution passes **114 scenarios / 113 pixel comparisons**
plus the transport benchmark's round-end references. Core tests pass **122** with
text/WGPU, retained-only tests **109**, and gallery/font/hardware-click tests
**12**. Selected core/gallery/perf Clippy all-target checks pass `-D warnings`;
the desktop WGPU executable builds. Logs and raw JSON/PNGs are retained here.

## Correctness, topology and completion

Both property modes run mixed rounded fill/border, outer/inset shadow, alpha
image and real Chinese/Latin glyph scenes at scale 1/1.25/1.5. Cases cover parent
translation/clip/scroll, cropped group opacity, cancelling child motion, empty
masks, layout changes, cache reuse, skipped bases, reset, mode changes and
removed-generation rejection/reuse, including retirement while disabled followed
by re-enabling shared placement. Ordinary gallery clicks use the shared path
by default and compare against fresh world-instance reference output.

Steady ancestor translation, clip or scroll writes **96 property bytes and zero
Quad/Shadow geometry bytes**. Opacity-only changes still reuse content and write
the existing 16-byte compositor property. Property ancestry invalidates affected
raster ranges without requiring geometry writes.

A 72-node chain spans three bounded GPU segments. Root movement writes **288
property bytes / zero quad bytes**, preserves expected red pixels and hit output,
and matches the full renderer. Reparenting changes anchor depth while keeping
leaf world bounds and node preorder unchanged; the consumer rebuilds placement
metadata without geometry upload. Removing/completing part of the chain and
constructing a differently nested subtree exercises retired property clears and
depth-changing slot reuse. Core tests also cover local placement deltas whose
world bounds cancel, shadow-only complete publication and invalid spatial data
hidden behind reused immutable pages.

The existing large-shadow growth case now also verifies property-buffer
replacement: **two in-flight submissions**, prior serial **1**, completed serial
**0**, and property generation **2 → 3**, alongside Quad/Shadow growth. Explicit
buffer/settings/binding leases protect the prior submission. Seven malformed
spatial cases reject invalid parents/cycles/depth/NaN/reference/scaled overflow/
write range without advancing serial; a valid retry writes no geometry/property
bytes and preserves expected red pixels. A separate unwritten-slot assertion
verifies that initial GPU placeholders match nonzero typed empty-anchor records.

## Exact clip-center regression

The first cropped opacity scene exposed **67 missing pixels in row 17** when the
clip minimum lay at device y=17.5. Interpolated distances changed sign between
full and cropped targets. Historical actual/reference PNGs, stderr and metadata
are in `regression-before-fix`; that debug manifest differs from the final audits.
Shared Quad/Shadow fragment stages now test exact world pixel centers directly,
preserving inclusive minimum/maximum boundaries and empty masks.

Eight independent analytical cases cover fill/shadow, shared/world properties
and bounded/full-viewport layers. With 0.6 group opacity, pixels on both clip
edges must be `[153,0,0,255]`; adjacent outside rows must be black. All then match
the fresh reference byte-for-byte. This supplies an independent oracle in addition
to the shared shader math. Pure Quad/Shadow scenes use the complete GPUI Scene
renderer; mixed/effect scenes use a fresh world-instance consumer with full-size
targets and empty raster caches.

## Owner-to-completion A/B

The benchmark translates one parent of 1,000 or 10,000 quads on an 800×600
surface. Both modes use identical content and full root redraw. Shared placement
writes one 96-byte node; world instances rewrite the parent plus N descendants:
`(N + 1) * 160` bytes. Root geometry remains unchanged. Both controls share the
current owner publication and validation, including local spatial snapshots.
This isolates the GPU representation in the current pipeline; it is not an
older executable/release comparison.

Three executions each contain three rounds rotating mode order, six warmups and
twenty timed updates per mode/round: **180 samples per mode/case**. Timing starts
before the owner transaction and includes commit, subtree extraction/publication,
consumer validation/submission and per-frame GPU completion, with maximum one
in-flight frame. Pre-submit wall spans stop at submit return. Setup, readback,
round-end reference comparison and native presentation are excluded. Percentiles
select the nearest sorted sample, matching the executable. Samples are unfiltered.

| Quads | Steady upload bytes, shared / world | Completed median ms, shared / world | Median change | Completed p95 ms, shared / world | Pre-submit median ms, shared / world |
| ---: | ---: | ---: | ---: | ---: | ---: |
| 1,000 | 96 / 160,160 | 3.288 / 3.462 | -5.0% | 4.848 / 4.437 | 2.279 / 2.403 |
| 10,000 | 96 / 1,600,160 | 31.417 / 32.443 | -3.2% | 37.394 / 41.284 | 29.437 / 29.574 |

Upload reduction is established; a substantial, stable owner-to-completion
speedup is **not established**. At 1,000 quads the second/third execution medians
are slightly worse with sharing (3.420/3.418 and 3.612/3.527 ms), and pooled p95
is worse despite the improved pooled median. At 10,000 quads per-execution median
changes are about -5.9%, -3.9% and -0.8%. Losing round medians remain visible,
including 33.446/31.763 ms and 33.343/31.974 ms (shared/world). Host drift and
property validation/vertex work can offset transport savings. Completed means
are 3.437/3.479 ms and 32.041/33.245 ms respectively.

The pre-submit span is already about 30 ms for this 10,000-node workload. Source
inspection confirms complete CPU subtree extraction plus backend property,
instance-reference and affected-range scans remain. These measurements motivate
indexed/incremental processing; they do not isolate the cost of any one phase.
Pre-submit/completion differences are not GPU timestamps, and this workload does
not establish sustained 60/120 Hz, native latency or energy performance.
[summary.json](summary.json) retains every execution and round median; run-1
through run-3 contain all paired CPU/completed samples and reference assertions.

## Reproduction and limits

```powershell
./tooling/rendering-perf/run-retained-gpu.ps1 -Output target/spatial-validation -ValidationOnly
./tooling/rendering-perf/run-retained-gpu.ps1 -Output target/spatial-run-1 -SpatialOnly
# Repeat with fresh output directories for runs 2 and 3.
```

The runner builds before execution. Final validation/run-1/run-2/run-3 record
19/15/15/15 compiler observations at 500 ms intervals, with none observing cargo,
rustc or clippy. Shorter processes and unrelated host/GPU activity are not
excluded. All four selected-source manifests were checked again during freezing.

Shared placement currently covers Quad/Shadow with translation, rectangular
clips and scrolling. Node storage adds 96 bytes per geometric allocation slot,
plus settings/binding resources and per-vertex reads. There is no global physical
resource budget/compaction claim. Glyph/image adoption, affine/complex clips,
indexed preflight/invalidation, final native-window framebuffer/presentation,
forced device/surface loss and cross-platform qualification remain open in the
[full-goal tracker](../../RETAINED_IMPLEMENTATION_PROGRESS.md). Vulkan pixels
do not qualify native DirectX/Metal framebuffers. Isolated normal window paint
still uses the existing readback-to-atlas bridge.
