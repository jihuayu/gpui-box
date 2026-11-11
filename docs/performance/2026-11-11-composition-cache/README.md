# Persistent composition topology evidence

This freezes [composition plan reuse](../../RETAINED_COMPOSITION_CACHE.md) on
base `b24357c`. All 57 source SHA256 entries were verified against the measured
tree. Adapter: GTX 1650 Ti / Vulkan 571.96. The hardware suite passes 135 pixel
cases; seven new `composition-*` PNGs cover real-font/image/effect combinations,
material/property/opacity reuse and effect/text topology insertion/removal.
The reference is a fresh full retained renderer without cache reuse. New outputs
record zero planned items for stable edits and 83 planned items for topology
transitions. One core test separately compares cached plans with full plans and
checks shared draw arrays. Core tests pass 55 cases; ten shared-example tests,
desktop WGPU build and combined/feature-specific Clippy `-D warnings` pass.

`composition-benchmark.json` compares reuse and forced CPU planning on the same
persistent tree, consumer and resource deltas. Fixtures have 80/800 images sharing
one 2 × 2 texture, a real grayscale text node and an isolated root layer. Updates
alternate a quad and text color or group opacity. Both modes use Damage redraw,
800 × 600, one frame in flight and warm resources/layers. Three rounds rotate
mode order, six warmups plus twenty samples/mode/round (60/mode). CPU submit
includes packet validation, resource IDs, layer invalidation and draw encoding/
submission; completed time additionally waits for GPU. UI publication, setup,
readback and native presentation are excluded. Final pixels per mode/round match
the fresh uncached full renderer and each other. Opacity-only changes upload
16 property bytes, rasterize zero layers and rebuild zero glyph instances in
both modes; image update batches remain zero. Forced planning visits 83/803
items, reuse plans zero.

| Images + text | Change | Reuse submit median / P95 | Full-plan submit median / P95 | Reuse / full submit + completion median |
| --- | --- | --- | --- | --- |
| 80 + 1 | Mixed material | 969.6 / 1,454.0 µs | 785.1 / 1,055.1 µs | 2,306.7 / 2,297.8 µs |
| 80 + 1 | Group opacity | 350.4 / 449.2 µs | 294.5 / 507.4 µs | 878.6 / 819.3 µs |
| 800 + 1 | Mixed material | 3,868.7 / 5,055.7 µs | 4,495.9 / 6,462.3 µs | 5,848.6 / 6,844.2 µs |
| 800 + 1 | Group opacity | 570.7 / 804.1 µs | 897.5 / 1,119.6 µs | 1,517.1 / 1,801.1 µs |

The smaller fixtures are slower with reuse in pooled medians. Large-scene median
ratios are 1.16× for mixed material and 1.57× for group opacity, but raw timing
variation is substantial; no universal multiplier or sustained application FPS
claim follows. Raw samples and round medians are included. Compilation completed
before timing; external load is uncontrolled. Eliminated full-plan item count
and retained Arc identity are deterministic evidence independent of these timings.

This avoids wrapper CPU planning and layer draw-array rebuild/equality. Backend
live ID sets, layer validation/invalidation, per-image draw/binding calls and GPU
leases still traverse data. Property map changes also retain full effect-reference
checks. Other regenerated JSON workloads are independent controls, not a direct
comparison with the parent commit. Physical memory budgets, shared GPU property
trees, native surface presentation and complete lifecycle acceptance remain open.
