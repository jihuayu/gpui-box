# Retained glyph rendering evidence

This freezes the [grayscale glyph stage](../../RETAINED_GLYPH_RENDERING.md).
`validation.json` records 100 passing hardware pixel scenarios, including 17
text cases. `text-*.png` retain the corresponding images. Text comparisons use
a fresh full retained renderer with no previous cache and the same shader;
independent CPU mask-blend assertions cover non-effect cases. The CPU oracle
uses published glyph placement and masks, not a separate shaper/rasterizer.

The normal gallery click tests verify typed signal attachment, changed glyph
pixels, color changes with zero layout/quad recording and no new rasterization.
The complete shared-example suite has ten passing tests; the core suite has 44.
Clippy with `-D warnings`, the feature-disabled example Clippy and desktop WGPU
build pass. CPU lifetime tests exercise font-context disposal, thread transfer,
weak bitmap/chunk release, four-width eviction, epoch/scale rejection and atomic
text/image transitions.

All 45 SHA256 entries in `metadata.json` were checked against the measured tree
before copying the evidence. The base commit predates this stage; source hashes
identify its exact measured code. `adapter.json` identifies NVIDIA GTX 1650 Ti,
Vulkan, driver 571.96. Font shaping/rasterization uses Cosmic/Swash with the
bundled Noto Sans CJK SC font, not the native Windows font backend.

`text-benchmark.json` measures the same 450 visible bitmap instances, sharing 14
rasterized glyph keys, in an 800 × 600 full redraw. Both modes alternate the
same two colors. Three rounds rotate mode order; each round/mode has six warmups
and twenty measured samples. A readback outside timing verifies equal output.

| GPU cache | Median / P95 completed submit | Atlas mask bytes/frame | Instance bytes/frame |
| --- | --- | --- | --- |
| Reuse atlas and instance allocations | 423.0 / 592.1 µs | 0 | 28,800 |
| Cold atlas and instance allocations | 1,091.6 / 1,592.3 µs | 2,243 | 32,832 |

The median ratio is 2.58× for this workload. Reuse round medians were 455.1,
449.1 and 402.9 µs; cold medians were 1,019.3, 1,044.7 and 1,307.8 µs. Raw sorted
samples and round medians are retained. Timing includes validation, planning,
GPU atlas/instance allocation, bindings, uploads, submit and completion. Shaping,
rasterization, transaction/layout/snapshot extraction, renderer reset and its
pipeline construction, readback and native presentation are outside timing.

The cold mode includes a new atlas texture, per-mask writes, buffer growth and
binding creation. The benefit cannot be attributed to 2,243 saved transfer bytes
alone, and is not a comparison to the complete legacy GPUI UI/render lifecycle.
Warm color updates still upload 64 bytes per glyph and rebuild/compare glyph
plans. Material indirection and selective packet extraction remain open.

The hardware suite additionally verifies five eight-glyph node color changes
merge into one 2,560-byte upload without mask upload, multiple atlas page runs,
and twelve successive resource replacements with at most three frames in flight.
These are correctness/payload assertions, not sustained throughput benchmarks.

The runner also regenerates damage, isolated-layer and decoded-image benchmarks.
Their JSON files are included for reproducibility; those workloads are separate
from this glyph cache comparison. General memory budgets, graceful atlas-pressure
handling, rich/bidi/color text, sustained 60/120Hz and native texture presentation
remain open acceptance work.
