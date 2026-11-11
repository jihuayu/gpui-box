# Image delta ownership and timing evidence

This freezes [image draw deltas](../../RETAINED_IMAGE_DELTAS.md) on base `9a67ce0`.
All 56 source hashes were verified before freezing. Adapter is GTX 1650 Ti,
Vulkan 571.96. `validation.json` records 128 passing pixel cases, with four added
`image-delta-*` PNGs. Their reference is a fresh uncached full retained renderer.
Direct backend assertions run before these numbered cases and verify immutable
IDs, atomic swaps, duplicate/conflicting update rejection, invalid bytes/geometry,
recovery after rejection, shared owner removal, surface resize without image
updates, last-owner eviction and restore. Existing in-flight pixel/Arc retirement
tests pass, as do 54 core and ten shared-example tests, desktop build and combined/
feature-specific Clippy with `-D warnings`.

`image-delta-benchmark.json` uses the same persistent tree and WGPU consumer,
80 or 800 ordered image draws sharing one immutable 2 × 2 texture, an 800 × 600
target and Damage policy. It alternates one unrelated quad color or one image's
translation. The control forces full image input/reset each submit while retaining
matching backend textures; both modes have identical draw order and upload bytes.
Three rounds rotate mode order, six warmups then twenty measured samples/mode/
round (60/mode). CPU submit includes full composition planning, live resource-ID
checks, leases and GPU encoding/submission. Completed timing additionally waits
for GPU. UI transaction/publication, setup, pixel readback and native presentation
are excluded. Final per-round pixels match the fresh full renderer and each mode.

| Images | Change | Delta CPU submit median / P95 | Full CPU submit median / P95 | Delta / full submit + completion median |
| --- | --- | --- | --- | --- |
| 80 | Unrelated color | 1,181.6 / 4,080.3 µs | 1,144.2 / 2,714.0 µs | 3,921.6 / 2,526.3 µs |
| 80 | One image move | 1,095.8 / 2,438.5 µs | 1,315.5 / 3,471.4 µs | 2,374.3 / 3,432.7 µs |
| 800 | Unrelated color | 5,479.0 / 12,109.7 µs | 5,851.6 / 8,560.6 µs | 8,045.0 / 8,552.7 µs |
| 800 | One image move | 3,908.7 / 4,829.2 µs | 4,411.4 / 5,783.2 µs | 6,024.2 / 6,506.3 µs |

Delta input has zero or one image record versus 80/800 full records. Both modes
upload zero texture bytes; movement writes 48 geometry bytes and unrelated color
writes zero image geometry bytes. The 80-image unrelated-color control is faster
with full input; substantial scheduling variance prevents a general speedup
claim. Compilation completed before timing but external OS/GPU load is uncontrolled.
Raw samples and per-round medians are included. The backend still validates a
complete live ID set, the wrapper still builds composition order and images still
use individual draw/binding records. Those costs can dominate this stage's input
savings. Regenerated other JSON files are separate workloads, not a comparison
isolating this stage from its parent commit. General image batching, physical
resource budgets, async decode and native presentation remain acceptance work.
