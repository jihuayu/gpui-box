# Persistent composition topology and shared layer draws

The WGPU wrapper no longer rebuilds mixed image/text/effect draw plans merely
because the scene contains these primitives. A composition cache retains the
last consumed immutable pages, effect maps and ordered layer draw arrays. Reuse
requires unchanged tree, item/page counts and exact node/command-count/image/
text/effect topology in every changed page. Identical pages skip item comparison.
Effect-map changes compare identities and parent relationships; opacity values
remain mutable properties. Missing bases, scale/device resync, allocation count
changes and disabled reuse force full planning.

On reuse the backend keeps its root order, receives current layer opacity values
and clones each layer's immutable draw Arc. `QuadLayer.draws` and the backend's
cached draws now use `Arc<[QuadDraw]>`; identical Arcs skip complete array equality.
Content invalidation still checks instance writes and changed image/glyph/layer
IDs, so reuse does not suppress raster work. Adding/removing text/images/effects,
changing effect ancestry or node order rebuilds the plan. A material/property
change can reuse it. Surface-only resize keeps topology but backend target/cache
recreation still rerasterizes correctly.

`FrameSubmission.composition` reports rebuilt status, compared item count, planned
item count and outgoing layer records. A core test repeatedly compares cached
results with full planning across colors, translation, opacity changes, image
detach/restore, effect insertion/removal, node reorder and forced resync. Old
layer arrays remain shared during material/property updates. Core tests pass 55
cases; ten shared-example tests, desktop WGPU build and combined/feature-specific
Clippy `-D warnings` pass. The hardware suite passes 135 scenarios, adding seven
mixed real-font/image/effect cases. Stable edits plan zero items; effect/text
topology transitions rebuild and match a fresh uncached full renderer pixelwise.

[Frozen evidence](performance/2026-11-11-composition-cache/README.md) includes
57 verified source hashes and same-consumer reuse/forced-plan controls. At 800
images plus one real text node, group-opacity CPU submit median is 570.7 µs with
reuse versus 897.5 µs full planning; mixed material changes are 3,868.7 versus
4,495.9 µs. At 80 images, full planning is faster in the measured run. Timings
include remaining backend validation/draw/lease work, exclude UI publication and
native presentation, and have substantial scheduler variation. They do not prove
universal latency or sustained FPS improvements.

Directory comparison still reads N/64 pointers; changed pages compare their item
topology, changed effect maps compare all effects, and current layer descriptors
still scale with layer count. Backend draw-ID validation, layer invalidation,
individual image draw calls and completion leases retain scene-wide work. The
cache holds one latest snapshot, not an unbounded history. General chunk IDs,
physical memory budgets, native presentation and broader RFC acceptance remain
open in the full-goal table.
