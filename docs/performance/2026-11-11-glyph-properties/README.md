# Independent glyph origin and clip properties

This follows [independent materials](../2026-11-11-glyph-materials/README.md), base
commit `d68a9e9`. A 32-byte per-text-node property record holds logical origin,
device scale and physical clip. Instances hold local glyph origins, integer mask
offsets, atlas tiles and a node slot index; the record shrinks from 64 to 48 bytes.
Color still occupies its separate 16-byte material record. Both buffers share a
stable slot allocator with queue-ordered reuse, geometric growth, binding rebuild
and completion leases. Adjacent property ranges merge independently of materials.

Node translation, ancestor scrolling and resolved clip changes update properties
without rewriting glyph instances or masks. The shader computes the baseline as
`(local + node_origin) * scale`, rounds halfway values away from zero (matching
Rust `f32::round`) and adds the integer mask offset. The prepared raster scale
must still match the consumer. Rewrap changes local glyph geometry; scale changes
still require appropriate masks. Each property write invalidates cached layer
content independently of material and instance changes.

`validation.json` contains 107 passing hardware pixel scenarios, 24 text cases.
Five added cases check negative/positive half-pixel and fractional origins,
property-only isolated-layer invalidation, and five-node adjacent property upload
merging. Independent CPU mask blending and fresh full-render comparisons pass.
Single-node translation/clip writes 32 property bytes, zero glyph instances and
zero masks. Five-node movement writes 160 property bytes in one range, zero
geometry/material/atlas bytes. Existing color changes remain 16 bytes per node.
The scene's separate background quad may still require its existing instance
update; these counters describe glyph resources, not the whole scene payload.

The 44 core tests, ten real-font/normal-click tests, Clippy with `-D warnings`
and desktop WGPU build pass. All 45 source SHA256 entries were verified against
the measured tree before copying evidence. Adapter: GTX 1650 Ti / Vulkan 571.96;
real font backend: Cosmic/Swash and bundled Noto Sans CJK SC.

`text-benchmark.json` retains three rotating rounds, six warmups and twenty
samples per mode per round, identical pixels and 450 glyphs at 800 × 600.

| Path | Median / P95 completed submit | Atlas bytes | Instance bytes | Material bytes | Property bytes |
| --- | --- | --- | --- | --- | --- |
| Warm color | 374.5 / 598.2 µs | 0 | 0 | 16 | 0 |
| Warm color + outside-ink clip change | 286.5 / 648.5 µs | 0 | 0 | 16 | 32 |
| Cold GPU allocations | 1,021.5 / 1,307.9 µs | 2,243 | 24,624 | 32 | 64 |

Property+color is faster in the pooled median despite doing another write. Round
medians (color 330.1/322.7/517.8 µs; property+color 234.2/262.3/538.0 µs) show
large timing variation. This does not establish the isolated latency cost or
speedup of property indirection. The deterministic result is zero geometry writes
for translation/clip, smaller instance records and pixel correctness. Historical
inline-clip measurements are retained separately; cross-run timing differences
are not an A/B estimate. The earlier material-stage instance-rewrite diagnostic
now exercises only properties and is labeled accordingly in the current runner.

Timing covers packet validation/planning, buffer/atlas preparation, submit and GPU
completion. It excludes shaping/rasterization, transaction/layout/extraction,
reset/pipeline construction, readback and native presentation. The shader lookup
and arithmetic are included, but have not been independently compared with the
prior inline-geometry shader. This is neither sustained FPS nor whole-UI latency.

The properties are per text node and currently receive flattened inherited state.
General GPU transform/clip trees, affine transforms, full-glyph scan elimination,
robust capacity fallback, physical memory budgets and native presentation remain
open. Damage/layer/image JSON files are separate regenerated workloads.
