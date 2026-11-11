# Persistent page hit index evidence

This freezes the [ordered page hit index](../../RETAINED_HIT_INDEX.md) on base
`49a5854`. All 55 source SHA256 entries were verified against the measured tree.
The GPU adapter is GTX 1650 Ti / Vulkan 571.96; hit benchmarks are CPU-only.
`validation.json` retains all 124 passing pixel regression cases. Existing normal
clicks run through the indexed public hit method and pass ten shared-example
tests. Core tests pass 54 combined-feature / 49 retained-only cases; combined and
feature-specific Clippy use `-D warnings`, and the desktop WGPU build passes.

`hit-index-benchmark.json` compares the new index and the previous reverse item
scan on the exact same snapshot. Grid queries are distributed deterministic
points inside occupied rows. The overlap control places all items at the same
location, so a reverse scan immediately hits the last item. Three rounds rotate
mode order, with six warmups and twenty measured samples/mode/round (60/mode).
Each sample issues 1,000 queries and reports elapsed time per query. `black_box`
consumes coordinates/results. Every query's answer is compared before timing.
Warm timings exclude building, transaction/publication, dispatch, GPU and native
presentation. Cold rebuild/query is measured in twenty distinct COW-invalidated
packet clones, excluding packet/page clone and mutation time; first-build timing
is also retained. No external OS load control is implied.

| Nodes | Scene | Indexed median / P95 per query | Reverse median / P95 | Cold rebuild + query median | Indexed tested items / visited bounds |
| --- | --- | --- | --- | --- | --- |
| 1,001 | Distributed grid | 0.3524 / 0.5827 µs | 3.2854 / 4.7550 µs | 5.3 µs | 123.23 / 8.97 |
| 1,001 | All overlap, top hit | 0.0393 / 0.0430 µs | 0.0081 / 0.0081 µs | 5.0 µs | 1.00 / 5.00 |
| 10,001 | Distributed grid | 0.3745 / 0.5290 µs | 20.6553 / 25.4401 µs | 54.1 µs | 74.38 / 13.64 |
| 10,001 | All overlap, top hit | 0.0581 / 0.0662 µs | 0.0080 / 0.0089 µs | 54.5 µs | 1.00 / 9.00 |

Grid median ratios are 9.32× / 55.15×. The overlap control is slower with the index
because the original scan hits in one step; no adaptive switching is implemented.
Tiny batched per-query costs should not be read as precise single-event latency.
Raw sorted samples/round medians are included. The JSON stores index construction
and index traversal fields alongside both policy rows for common fixture context;
those fields describe the index, not the linear implementation. Layout/order
full publication still causes a linear lazy build. Ordered page unions can become
loose, and broad overlap/misses can approach a full traversal.

Update propagation is covered by correctness/Arc sharing tests and normal UI
tests. These warm query measurements do not quantify index-maintenance overhead
in every publication workload: the other regenerated publication benchmarks do
not warm a hit index before their timed updates. In a real UI that has queried
the frame, subsequent material/property updates also refresh affected page bounds.
The index holds bounds/ranges and shared index nodes without retaining item or
resource Arcs. General hit geometry, adaptive query policy, complete interaction
lifetime and the remaining RFC areas stay in the full-goal acceptance table.
