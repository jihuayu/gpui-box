# Backend-verified layer translation reuse probe

Status: experimental, default disabled. Complete RFC goal active.

The current layer allocator reuses same-extent textures when bounds move, but
invalidates their raster. For an isolated group whose complete local content is
unchanged, integer physical translation can instead move the composite quad and
preserve its pixels. This can remove repeated raster work during animations and
reduce the spikes that matter for the modern desktop 120 fps target.

## Decision

Prototype eligibility inside the backend, where committed primitive mirrors,
resolved spatial chains and actual layer bounds are authoritative. No caller
dirty bit, arbitrary content version, or stable layer ID can assert equivalence.
`set_layer_translation_reuse_enabled` is an experimental backend/control switch,
not application semantic authority. It defaults to false, survives consumer reset
and device replacement, and leaves the original raster path available.

For each eligible layer, retain full compared content records in layer-local
coordinates. Resolve Quad/Shadow placement using the shader's operation order,
intersect primitive clipping with the actual raster target, then normalize
origin/clip against that target. Compare complete ordered records, not a digest.
The target intersection removes irrelevant differences in distant viewport clips;
clips that cut into the content still change the proof and invalidate it.

Reuse requires an existing valid raster, unchanged physical extent, changed
integer target origin, equal complete normalized content and normal cache enable.
Update the camera/global target buffers for future raster and composite position,
but skip the layer render pass. Layer opacity remains separate and is updated
normally. A translated child is marked changed for its containing layer so that
the parent cannot reuse stale composition. Queue order and existing completion
leases protect previous submissions and old texture allocations.

The deliberately narrow initial eligibility is:

- Solid Quad fills/decorations and box Shadows; no world-dependent patterns.
- Integer physical primitive origins/extents, clipping and target bounds within
  16,384 pixels; integer shadow blur. The shared-placement probe accepts device
  scale 1 only, avoiding division/reconstruction rounding at other scales.
- At most 4,096 records and draw segments per layer. Exact proof storage is
  bounded per eligible layer; this additional CPU storage is not charged to the
  isolated GPU texture payload budget and is not a full memory admission model.
- Image, glyph and nested-layer draws take the original raster path. Their IDs
  are insufficient content or sampling epochs. Independently changed children,
  material changes, relevant external clips, scale changes, fractional geometry,
  target extent changes and missing/evicted proof all fall back conservatively.

This is an inference prototype for the previously proposed content/placement/
raster-environment split. It does not yet implement persistent local-space layer
content descriptors, resources or generalized effect caching. Rebuilding proof
records each frame adds owner/backend work, even when no raster is saved.

## Measurement and acceptance

Keep old raster and optimized results on the same executable, alternating mode
order. Pixel controls compare both shared/world placement to a fresh full-size
renderer with no old raster. Cover positive/negative integer and fractional
motion, relevant viewport clipping, material and child motion, isolated opacity,
unsupported nested/resource paths, scale/reset and in-flight queue ordering.
Record rasterized pixels, CPU owner+submission and completed-work latency.

Targeted sustained measurements must use independent absolute-clock arrivals,
count all coalesced and deadline-unsatisfied source ticks, and retain p95/p99/max
and longest failing bursts. They exclude native display and physical input.
Large-overdraw raster savings cannot close dense independent-child updates,
local snapshot full-viewport layers, normal native visible presentation, modern
desktop hardware or richer resource/clip/effect acceptance.

References: [spatial contract](ADR_RETAINED_SPATIAL_GPU.md),
[frame budgets](ADR_RETAINED_FRAME_BUDGET.md).
