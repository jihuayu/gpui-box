# Visual-property subtree extraction evidence

This freezes [selective subtree extraction](../../RETAINED_SUBTREE_EXTRACTION.md)
on base `80f5b7a`. All 51 measured source hashes were verified before freezing.
Adapter: NVIDIA GTX 1650 Ti, Vulkan 571.96. `validation.json` records 121 passing
pixel scenarios, including six new `extraction-*` PNGs. Plain geometry uses the
original full GPUI renderer as oracle; isolated opacity uses a fresh full retained
renderer without reused layer caches. New cases assert 101 extractions for a
101-item affected subtree, or 2 for a leaf plus an unrelated material patch,
with no layout or command recording. Existing text/image/lifecycle cases pass.

`extraction-benchmark.json` compares selective and forced-full publication on the
same persistent tree. Each group has 100 fixed 8 × 8 children overflowing its
80 × 80 border box; ten or one hundred groups give 1,011 or 10,101 total nodes.
One leaf or one entire 101-item group changes translation; further controls change
group scroll or opacity. Three rounds rotate mode order, six warmups then twenty
samples per mode/round (60/mode/case). Complete final items and property maps are
compared outside timing. Publication includes subtree resolution, sparse property
map copy, delta/damage comparison and immutable page updates. Commit + frame also
includes transaction application. Transaction construction, initial layout, GPU
consumer validation/composition, draw/completion and presentation are excluded.

| Nodes | Change | Selective publication median / P95 | Full median | Selective commit + frame | Extracted items |
| --- | --- | --- | --- | --- | --- |
| 1,011 | Leaf translation | 3.3 / 5.3 µs | 1,466.0 µs | 4.2 µs | 1 |
| 1,011 | Subtree translation | 59.9 / 138.2 µs | 1,604.4 µs | 61.2 µs | 101 |
| 1,011 | Subtree scroll | 51.5 / 65.3 µs | 1,409.0 µs | 52.8 µs | 101 |
| 1,011 | Subtree opacity | 49.4 / 73.3 µs | 1,494.6 µs | 50.8 µs | 101 |
| 10,101 | Leaf translation | 5.4 / 6.6 µs | 17,420.7 µs | 6.2 µs | 1 |
| 10,101 | Subtree translation | 54.1 / 71.0 µs | 15,267.6 µs | 55.9 µs | 101 |
| 10,101 | Subtree scroll | 35.6 / 58.7 µs | 11,264.1 µs | 36.3 µs | 101 |
| 10,101 | Subtree opacity | 41.2 / 48.6 µs | 10,654.7 µs | 42.2 µs | 101 |

OS timing variation is substantial: the larger tree's 101-item selective cases
can be faster than the smaller tree's cases. Raw sorted samples and round medians
are retained. No compilation ran concurrently with the measurements. Stronger
deterministic evidence is the extracted item count, unchanged command/layout
counters and exact output equivalence. The large latency ratio removes full
snapshot maps/order/damage/index work only in this fixture; it is not end-to-end
FPS. The control now also builds subtree-end indices on full extraction.

The immutable directory still copies N/64 pointers; a property update copies
sparse maps proportional to all active properties. A root property affects all
visible descendants and still visits them. Layout/visibility/order rebuilds,
full consumer packet validation, composition planning and hit-test scans remain.
The other benchmark JSON files contain independently regenerated damage, layer,
image, glyph and local paint results and are not comparisons of this stage with
the immediately preceding commit.

Validation: 50 combined core tests, 47 retained-only core tests, ten shared-example
tests, desktop WGPU build, combined Clippy plus WGPU-without-text/non-WGPU-example
Clippy with `-D warnings`. Device/surface loss, native presentation and other
platforms remain open in the complete-goal acceptance table.
