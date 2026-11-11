# Immutable page validation reuse evidence

This freezes [GPU packet validation reuse](../../RETAINED_VALIDATION_CACHE.md)
on base `1a1ca9e`. All 53 source SHA256 entries were checked against the measured
tree. Adapter is GTX 1650 Ti / Vulkan 571.96. `validation.json` records 124 passing
pixel scenarios. The three new PNGs cover malformed-page rejection followed by
a valid partial submit, reordered-node full validation, and renderer reset.
The partial case validates 64 items and reuses three of four pages. Existing
font/image/compositing/lifecycle checks all pass with the cache enabled.

`validation-benchmark.json` compares cache reuse and forced complete validation
on the same persistent tree and renderer. One alternating color patch updates
one quad, Damage redraw, 800 × 800 target, one frame in flight. Three rounds rotate
mode order, with six warmups and twenty measured samples per mode/round (60/mode).
Timings start after UI transaction and publication. CPU submit includes validation,
remaining scene/resource/order/backend work and queue submission. Completed time
additionally waits for GPU completion. Initial setup and pixel readback are
excluded. Final pixels per mode/round match the original full GPUI renderer and
each other. Both modes upload 160 bytes without full renderer resync.

| Nodes | Validation mode | CPU submit median / P95 | Submit + completion median | Checked items | Reused pages |
| --- | --- | --- | --- | --- | --- |
| 1,001 | Immutable page reuse | 115.3 / 246.3 µs | 268.8 µs | 64 | 15 |
| 1,001 | Forced full | 312.9 / 513.2 µs | 801.1 µs | 1,001 | 0 |
| 10,001 | Immutable page reuse | 371.8 / 565.9 µs | 593.1 µs | 64 | 156 |
| 10,001 | Forced full | 2,197.1 / 2,764.0 µs | 3,235.6 µs | 10,001 | 0 |

CPU median ratios are 2.71× and 5.91× in these fixtures. Completed timings include
driver/scheduler behavior; no GPU-only speedup or sustained application FPS claim
follows. Raw samples and round medians are included. Compilation finished before
measurement; external OS/GPU load is uncontrolled. This isolates the validation
choice with the same new code rather than replaying an entire legacy application.

Stable-identity changes reuse only cache-owned immutable page Arcs and the cached
node index. Changed pages validate every record. Scale/tree/order/identity changes
fall back. Property-map changes still check all item effect references. Page
directory scans, resource and composition work remain; this is not a constant-time
complete consumer. The cache retains one latest validated snapshot until replaced,
reset or dropped, including its CPU-owned resource references. Cross-frame physical
memory budgets, general semantic/resource routing and native presentation remain
open. Other JSON files are regenerated workloads, not measurements isolating this
stage from the previous commit.

Validation also passes 52 combined-feature core tests, 47 retained-only core tests,
ten shared-example tests, desktop WGPU build and combined/feature-specific Clippy
with `-D warnings`. Malformed geometry, node aliases, dangling/cyclic effects,
delta property mismatch, failed-check recovery and reset are explicitly covered.
