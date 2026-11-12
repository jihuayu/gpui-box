# Shared hierarchical GPU placement

The default WGPU consumer now shares a persistent placement tree between Quad
and Shadow instances. Parent translation, rectangular clip and scroll updates
write shared properties while preserving local primitive geometry. This extends
[CPU property storage](RETAINED_PERSISTENT_PROPERTIES.md) and
[typed GPU storage](RETAINED_SHADOWS.md). The
[complete RFC goal](RETAINED_IMPLEMENTATION_PROGRESS.md) remains active.

## Owner publication and validation

`PaintItem::spatial` owns local render parent identity, Taffy location, visual
translation, clamped content scroll and local clip. The root also owns its
viewport clip. Existing resolved bounds/content/clip still drive window paint,
hit testing, damage and bounded isolation. Old packets retain both forms.

Selective and complete publication include changed local placement in base
deltas even when ancestor/descendant motions cancel to the same final world
bounds. Complete publication also compares shadow lists independently of equal
recorded fill commands. Core regression cases cover both conditions.

Consumer validation checks local/resolved origin and inherited clip agreement,
finite fields, parent-before-child edges and exact authoritative delta equality.
Immutable-page reuse checks direct child edges of changed pages as well: moving
a parent cannot leave an inconsistent child hidden in a reused page. Reparenting
rebuilds the edge index even when node preorder is unchanged.

## GPU contract

The generic allocation table assigns generation-checked shared slots. Each
96-byte initialized GPU node carries local layout/translation, scroll, clip,
parent/depth and anchor fields. Quad and Shadow use `order = slot + 1`; zero keeps
the original world-instance convention. Quad sizes/decorations and Shadow
shape/radii/blur remain physical local geometry. Retained shadows carry logical
offset in raw bounds origin and signed expansion in raw element-origin x until
the retained-only shader load resolves their world coordinates. This convention
is confined to the enabled shared path; the pinned GPUI ABIs are unchanged.

Vertex shaders walk a bounded segment, then evaluate it from root to leaf with
the producer's f32 addition/subtraction order. Scale is applied last. The root
and every 32nd node are resolved anchors, preserving arbitrary deeper render
trees with bounded vertex work. A root move in a 72-node chain updates three
anchors (288 bytes) and uploads no quad geometry. Shared properties currently
cover translation/rectangular clip/scroll; full affine and complex clip shapes
remain future work.

The retained shader variant reuses shared primitive math and passes resolved
origin/clip as flat varyings. Fragment shading never walks the tree. Its pinned
source transformations assert exact match counts, making upstream drift explicit.
Pipeline blending matches the shared straight-alpha primitive output; layer
composition continues to use its existing premultiplied path.

An exact pixel-center regression exposed instability in interpolated clipping
distances when switching between full and cropped targets. Shared Quad/Shadow
fragment stages now compare world pixel coordinates directly with their mask.
Both minimum and maximum boundaries remain inclusive, matching the existing
distance-test contract; empty masks remain empty. Independent expected red/black
pixels verify top/bottom pixel centers in both property modes and both target
sizes, followed by byte-exact reference comparison. Image/glyph clipping retains
its existing contract.

## Lifetime, topology and raster invalidation

The property buffer uses common typed storage growth, mirror and range uploads.
Initial GPU placeholders are initialized from the typed empty record, including
nonzero empty-anchor sentinels; unwritten property slots paint no content.
Every submission leases its current property buffer, settings buffer and binding.
Later writes follow earlier reads on the same queue; replaced allocations remain
leased through completion. CPU slots defer reuse by serial. Retired GPU node
records are explicitly cleared to empty anchors before reuse, including across
disabled control frames, so stale parent/depth edges cannot survive slot reuse.

Reparenting can change anchor boundaries while world bounds remain equal. The
consumer detects parent-map changes and rebuilds shared placement metadata from
current preorder; local instance references retain stable slots. Skipped bases,
new sources and device recreation resynchronize from the owned snapshot.

Changed property ancestry marks affected Quad/Shadow ranges dirty without
requiring geometry uploads, invalidating their isolated raster caches and parent
layers. Own-opacity changes continue to reuse content and write the existing
16-byte compositor property. The current backend scans property/instance prefixes
for validation and affected ranges; indexed dependency invalidation remains open.

`set_shared_spatial_enabled(false)` selects the original world-instance control.
Switching forces snapshot resynchronization, and reset preserves the setting.
The getter exposes it for verification. Raw backend batches default to the
original convention. Submission fields report spatial upload bytes/ranges and
buffer generation separately from quad/shadow geometry and layer opacity.

## Verification and performance scope

The hardware suite covers both modes at scales 1/1.25/1.5, mixed alpha images and
real glyphs, parent motion/clip/scroll, cropped isolation, opacity-cache reuse,
cancelled child motion, empty masks, layout changes, skipped bases, reset, mode
switching, removal/reuse, deep anchors and depth-changing reparenting. Malformed
parent/depth/NaN/overflow/reference/write ranges must reject before mutation and
permit a zero-upload retry. The existing outstanding-submission buffer-growth
case also records replacement property-buffer generations.

The focused A/B includes owner transaction, subtree publication, consumer
validation/submission and per-frame GPU completion for parent moves among 1,000
or 10,000 quads. Both modes redraw the full root with identical content. Shared
placement uploads 96 bytes per steady update; the world-instance control uploads
`(N + 1) * 160` bytes. Owner extraction and backend scans remain inside timing;
readback/setup/native presentation are excluded. Frozen raw samples and exact
counts belong in the [evidence report](performance/2026-11-12-spatial/README.md).

The [architecture decision](ADR_RETAINED_SPATIAL_GPU.md) records trade-offs.
Additional node memory and vertex reads can outweigh upload savings; no complete
UI/native-surface latency, FPS or energy claim follows from this A/B. Glyph/image
adoption of this shared tree, materials, affine properties, indexed processing,
global physical budgets/compaction, native presentation/device-loss handling and
cross-platform qualification remain in the full-goal tracker.
