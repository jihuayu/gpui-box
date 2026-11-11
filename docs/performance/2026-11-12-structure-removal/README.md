# Atomic subtree/key retirement evidence

This freezes the removal follow-up to [structural transactions](../../RETAINED_STRUCTURE_TRANSACTIONS.md)
on base `c8b807f`. All 73 hardware source hashes match the final sources.
Adapter: GTX 1650 Ti / Vulkan 571.96. Core retained-text/WGPU tests pass 77
cases, gallery/font tests pass 11 including real GPU clicks, and integrated
Clippy passes with `-D warnings`. The hardware suite passes 149 pixel cases.

New core cases cover overlapping/duplicate removal roots, rejection of a move
into a removed current subtree, stale generation rejection after slot reuse,
revision-exhaustion rejection before removal, and mixed key removal/insertion
with multi-host output, cleanup, source/binding disposal and one revision.
Existing logical disposal/dependency/portal preflight tests remain passing.
The new hardware `structure-keyed-remove-batch` case removes multiple portal
outputs and their scope, creates a new empty key, checks one revision and one
cleanup, rejects the old packet as stale and matches original full GPUI pixels.

The existing three-mode structural benchmark was regenerated because the
preparation path changed. Protocol remains three rotating rounds, ten warmups
and one hundred measured samples per mode/round. Commit and list construction
are timed; layout/extraction/GPU are excluded. Raw round samples are recorded.

| Hosts × children | Sequential median / P95 (µs) | Property transaction median / P95 (µs) | Final topology median / P95 (µs) |
| --- | --- | --- | --- |
| 2 × 8 | 1.8 / 1.9 | 1.5 / 2.3 | 1.5 / 2.1 |
| 32 × 32 | 64.0 / 97.2 | 74.9 / 135.2 | 60.5 / 105.2 |

These timings are reorder controls, not a removal-throughput comparison. Host
load varies; no performance improvement is attributed to removal support.
The supported atomic lifecycle is key/subtree retirement plus surviving order;
constructing render output and arbitrary property/source creation in the same
batch, event/focus/semantics lifetime and remaining RFC requirements stay open.
