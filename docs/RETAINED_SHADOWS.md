# Persistent rounded-box shadows and typed GPU storage

Retained nodes now own immutable rounded-box shadow lists. Typed source changes
reach the normal gallery transaction, paint publication, damage tracking,
persistent WGPU instances and ordinary GPUI window paint. This extends
[decorated quads](RETAINED_DECORATED_QUADS.md); the
[complete RFC goal](RETAINED_IMPLEMENTATION_PROGRESS.md) remains active.

## Owner contract

`BoxShadow` carries RGBA color, logical offset, nonnegative blur radius, signed
spread radius and an inset flag. `Transaction::shadows(node, Vec<BoxShadow>)`
replaces the list; `Property::<Arc<[BoxShadow]>>::shadows(node)` binds a shared
immutable list directly, without copying it through a temporary Vec. Draft tree
construction uses the same validated property transaction. Invalid values reject
the whole edit, and final restoration/equal values request no new frame.

Shadows are paint inputs: they skip Taffy and measurement, preserve node identity
and share their Arc with immutable published paint items. Old packets retain the
old list. The current implementation invalidates the owning command chunk on a
shadow edit; separate shadow-chunk identity remains future work. Layout, scroll
extents and hit geometry continue to use the element body.

Drop shadows paint before the node's fill/border; inset shadows paint after it,
before images, glyphs and descendants. List order is preserved within each phase.
Uniform corner radius comes from the existing quad decoration. Geometry follows
the pinned GPUI box-shadow path: spread expands a drop box and contracts an inset
hole, negative spread is allowed, and collapsed hole dimensions clamp to zero.
This is a rounded-box contract, without a full CSS conformance claim.

## Ink, clipping and isolation

Drop-shadow ink includes the shader's three-blur-radius Gaussian margin. Inset
ink is bounded by the element. Inherited clips and viewport intersect this ink;
old/new clipped bounds feed damage and bounded isolation targets even when the
body itself is transparent or entirely clipped out. Shadow pixels do not create
new hit targets.

The shared GPUI Scene now uses conservative shadow paint bounds for culling and
bounds-tree ordering. Previously it used only the drop core/inset hole, which
could discard a visible blur tail or collapsed inset hole. Public primitive body
bounds remain unchanged. The WGPU shadow fragment shader recovers world position
from each cropped target's origin, matching the existing quad/image/glyph paths.

An additional regression reproduced a one-pixel line when a zero-width mask lay
on a device-pixel center: both interpolated clip distances were zero. Quad and
shadow vertex stages now collapse geometry for empty masks. Six analytical/full
reference cases cover zero width/height for fill, drop shadow and inset shadow.

## Persistent storage and completion

The existing generic CPU allocation table now also allocates shadow records.
The backend shares typed `PersistentStorage<T>` growth, CPU mirror, range upload
and completion-lease code between the pinned 160-byte Quad and 112-byte Shadow
ABIs. Explicit initialization and size assertions protect the storage layout;
shadow preflight validates finite geometry, color, radii, expanded ink, write
ranges and draw ranges before mutation. Geometric growth preserves the mirror
and initializes the replacement buffer. Submission leases retain replaced
buffers/bindings through completion; removed CPU slots defer reuse by serial.

Composition contains independent shadow ranges. Adjacent slots merge; changes to
shadow count or inset phase rebuild topology, while color/geometry changes retain
the plan and invalidate only affected raster contents. A single shadow color edit
writes 112 bytes and no quad bytes. Thirty-two adjacent edits write 3,584 bytes in
one range. A quad color edit writes 160 bytes and no shadow bytes.

The hardware growth case keeps the prior submission outstanding while replacing
both typed buffers. Its records include the old/completed serial, in-flight count
and shadow generations, followed by full-reference pixel comparison. This checks
actual overlapping submissions as well as immutable CPU packets.

## Normal UI and verification

The existing rounded-border gallery button now changes decoration plus drop/inset
shadows in one outer typed transaction. The ordinary window adapter uses GPUI's
drop/inset paint APIs; the WGPU adapter consumes the same packet. The hardware
click test checks the published list and output against a fresh reference.
Isolated window output still uses the existing readback-to-atlas bridge, and a
final native-window framebuffer comparison remains open.

Hardware cases cover scales 1/1.25/1.5, alpha images and real text, material-only
edits, translation, inherited clipping, cropped group opacity, negative spread,
phase reorder, growth/shrink, detach/restore, removal/generation reuse, skipped
frames, reset, analytical coverage and atomic malformed-batch retry. The frozen
[evidence and measurements](performance/2026-11-12-shadows/README.md) report exact
counts, source hashes, compiler observations, raw timings and platform limits.

## Performance control and remaining work

The focused transport microbenchmark changes one shadow color among 1,000 or
10,000 instances. Delta and full-upload controls use the same CPU mirror, shader,
draws and full root redraw. Full upload sends the declared instance prefix
(`N * 112` bytes), excluding geometric spare slots and the unused quad placeholder.
Timing includes backend validation/submission and per-frame GPU completion;
owner UI transactions, Taffy, paint recording, readback and native presentation
are outside it. Raw samples retain host drift and losing runs. This does not
establish whole-UI speedup, FPS or isolated GPU execution time.

Typed storage currently serves two primitive ABIs. Shared GPU affine/clip/material
slots, arbitrary paths/general-shape shadows/filters, richer text and image
contracts, arena compaction, global resource/device-epoch budgets, forced device
loss and cross-platform/native-surface 60/120 Hz qualification remain in the
full-goal acceptance table. The shared Scene change has Vulkan pixel evidence;
native DirectX/Metal framebuffers have not been qualified here.
