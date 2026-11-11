# Selective paint publication evidence

This freezes the [paged Paint Store stage](../../RETAINED_PAINT_STORE.md) on base
`dc42733`. All 48 source SHA256 entries in `metadata.json` were checked against
the measured tree before freezing. Hardware is GTX 1650 Ti / Vulkan 571.96.

`validation.json` records 115 passing pixel scenarios. The three added cases
compare retained output with the original full GPUI renderer: changes in two
different pages (two extracted nodes, 128 copied items, two shared pages), a
decoration change (one extracted node and one new command chunk), and color
after ancestor movement rebuilt geometry. The first case uploads 320 GPU bytes
without full resync. Matching PNGs are included. Existing real-font, image,
damage, clipping, opacity and in-flight lifetime cases continue to pass.

`paint-store-benchmark.json` compares selective pages and forced full extraction
on the same persistent tree. Each case alternates colors on one or 128 contiguous
8 × 8 quads; no layout or command recording runs. Three rounds rotate mode order,
with six warmups and twenty measured samples per mode per round (60/mode/case).
Complete published item records are compared outside timing. Timers exclude
transaction construction, initial layout, rendering validation/planning, GPU and
presentation. Full extraction includes snapshot construction, delta and damage
comparison, order detection and rebuilding the node index.

| Total nodes | Changed nodes | Selective publication median / P95 | Full publication median | Selective commit + frame median | Copied items |
| --- | --- | --- | --- | --- | --- |
| 1,001 | 1 | 2.0 / 2.9 µs | 602.1 µs | 2.6 µs | 64 |
| 1,001 | 128 | 26.4 / 34.0 µs | 498.0 µs | 43.7 µs | 192 |
| 10,001 | 1 | 3.2 / 3.7 µs | 9,910.7 µs | 3.8 µs | 64 |
| 10,001 | 128 | 28.8 / 33.9 µs | 10,029.0 µs | 46.1 µs | 192 |

The page directory copies N/64 pointers even for one changed node. Contiguous
128-node patches straddle three pages. Old frames keep immutable pages; this
adds bounded per-frame page copies while frames remain in flight, without yet
providing a general physical resource budget. The large ratio reflects removing
all-node CPU extraction/maps/damage/order work in this fixture. Rendering still
validates the complete packet and traverses composition order. These results
do not represent whole-application frame times, sustained FPS or energy savings.
OS/GPU scheduling and timing variation are uncontrolled; raw samples/round
medians are retained. Other JSON files preserve regenerated damage, layer,
image and glyph measurements separately from this publication experiment.

Validation also passes 48 combined core tests, 45 retained-only tests, ten
shared-example tests, desktop WGPU build and Clippy `-D warnings` including WGPU
without text and the non-WGPU example. Layout/geometry/order changes still use
full extraction; generic stable chunks, spatial indices and native presentation
remain acceptance work.
