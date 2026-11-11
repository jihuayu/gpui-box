# Computed DAG evidence

This freezes [transactional computed dependencies](../../RETAINED_COMPUTED_DAG.md)
on base `f6cd1d7`. Both metadata files match the measured tree: 10 CPU source
SHA256 entries and 60 hardware entries verified. Hardware: GTX 1650 Ti, Vulkan
571.96. The full suite passes 139 pixel scenarios. Four new `computed-*` images
cover equal derived output, a mixed material/property batch, intermediate-read
restore and invalid sink rollback. Core tests pass 61 cases with text/WGPU and
55 without them; ten shared-example real-font/normal-click tests, desktop WGPU
build and combined/feature-specific Clippy `-D warnings` pass. The gallery color
button now uses an actual computed signal before its typed sink.

The CPU fixture groups 1,000/10,000 render nodes in tens. Each group has a
writable integer input, a bucket (`input / 4`) and ten derived colors. DAG mode
evaluates only reachable changes and stops at equal output; eager control
recomputes all buckets/colors after a changed input, with the same transaction
and typed sinks. Eager control skips source-equal events. The control represents
whole-derived-list recomputation, not an optimized manually indexed dispatcher
or a timed comparison with the parent commit. Both modes produce the same final
geometry, materials and commands in every round.

Three rotating rounds use twelve warmup samples and sixty measured samples per
mode. Source-equal, bucket-equal and one-group samples average 1,000, 100 and ten
events respectively; all-group samples measure one event. Percentiles for
batched samples describe averages, not single-event tail latency. Timing covers
source writes, derived evaluation/equality, sink dispatch and render commit.
It excludes extraction/layout, GPU, notify and native presentation. Compilation
finishes before timing; unrelated host load is uncontrolled.

| Render nodes | Update | DAG median / P95 (µs) | Eager median / P95 (µs) | DAG / eager evaluations | Sinks |
| --- | --- | --- | --- | --- | --- |
| 1,000 | source-equal | 0.065 / 0.087 | 0.064 / 0.077 | 0 / 0 | 0 |
| 1,000 | bucket-equal | 0.398 / 0.413 | 18.578 / 25.702 | 1 / 1,100 | 0 |
| 1,000 | one-group | 4.490 / 5.050 | 27.540 / 31.130 | 11 / 1,100 | 10 |
| 1,000 | all-groups | 953.400 / 1,494.100 | 656.900 / 1,012.800 | 1,100 / 1,100 | 1,000 |
| 10,000 | source-equal | 0.067 / 0.097 | 0.067 / 0.099 | 0 / 0 | 0 |
| 10,000 | bucket-equal | 0.656 / 0.667 | 272.710 / 606.243 | 1 / 11,000 | 0 |
| 10,000 | one-group | 6.980 / 7.630 | 471.580 / 751.830 | 11 / 11,000 | 10 |
| 10,000 | all-groups | 13,818.500 / 17,270.100 | 8,981.400 / 11,660.300 | 11,000 / 11,000 | 10,000 |

Sparse updates remove substantial computation. Dense updates are slower with
current ordered graph queues, erased value allocation and staged maps. These
are explicit workload boundaries; no universal speedup, FPS, energy or input
latency claim follows. Raw samples, per-round distributions and work counts
are included. Other regenerated GPU benchmark files are regression controls,
not evidence of a performance change caused by computed propagation.

The ten-node mixed batch evaluates three derived nodes, writes twenty typed
properties in one revision, performs zero layout/recording and uploads 1,600
instance bytes. Equality and read/restore produce zero sink writes and no GPU
submit. Invalid opacity leaves both writable/derived values and frame revision
unchanged. A separate 96-derived-node/200-event test compares mixed dependencies
against independent full recomputation, including read boundaries, recycled
slots and failed nested scopes.

This implements explicit static computed dependencies, including keyed lifetime
and cross-scope disposal checks. General effect scheduling/cleanup, automatic
dynamic tracking, cross-event scheduling and real OS idle/completion wakes
remain open. Native presentation still uses the readback bridge. The complete
RFC goal remains active.
