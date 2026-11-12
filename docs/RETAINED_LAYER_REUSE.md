# Persistent textures and coordinates for layer movement

Same-extent isolation layers now retain their texture, view, opacity buffer,
camera/global buffers and bind groups when only their physical origin changes.
The renderer updates 48 bytes of target coordinates and rerasterizes contents.
This extends [bounded targets](RETAINED_LAYER_BOUNDS.md) and
[cross-frame allocation budgets](RETAINED_LAYER_BUDGET.md). The
[complete RFC goal](RETAINED_IMPLEMENTATION_PROGRESS.md) remains active.

## Queue ordering and invalidation

Every layer has distinct camera/global buffers. Both now permit COPY_DST.
Origin changes write that layer's camera (16 bytes) and quad globals (32 bytes)
once before the current command encoder is submitted. Every pass in that frame
uses the same values for that target. No camera buffer is shared between targets
or rewritten between their passes. Prior submissions read their values before
these writes on the same WGPU queue, just as existing instance/opacity updates
already do. Submission leases still keep textures, buffers and bindings alive.

Preflight keeps a cache entry only when its extents match (or its whole bounds
are unchanged). Rebasing changes cached bounds and marks its raster invalid,
even with zero instance/resource writes. Postorder rendering then propagates
the changed child's contents to its parent. Transparent clears prevent old
pixels from surviving when the new world-space crop contains less/no content.
Rasterized pixels and primitive shading remain unchanged by this optimization.

Actual extent changes, surface epochs, retired IDs and cache removal still
replace/drop textures through the allocation budget. This does not reuse a
raster through a transform-only composition pass; translation still resolves
world-space primitive properties and rerasterizes affected layers. Shared GPU
affine/clip trees, oversize texture pooling and raster-transform reuse remain open.

## Controls and observability

`WgpuFrameRenderer::set_layer_texture_reuse_enabled(false)` (also available on
the backend) forces replacement on origin changes as a correctness/performance
control. Default is true and reset retains the setting. Switching modes forces
a complete consumer resynchronization/redraw without changing rendering semantics.
Budget stress controls explicitly select forced replacement so their pressure
wait/retirement behavior remains covered after the default improves.

`QuadSubmission::rebased_layer_textures` counts same-extent origin updates.
`layer_target_bytes` counts initialized or written layer camera/global payload
(48 bytes per allocation/rebase; root target excluded). Initial buffer data and
COPY_DST writes are logical application payload, excluding driver overhead.
Own-opacity bytes remain separately reported. A rebase needs zero texture
allocation/reservation and zero own-opacity write; a replacement initializes the
new texture parameters and 16-byte opacity uniform.

A one-layer payload budget can therefore keep several frames queued while they
share one physical texture allocation. Queue-length backpressure still applies;
origin updates no longer incur allocation-budget waits. Old coordinate snapshots
are supplied by queue order, while old CPU FramePackets remain immutable.

## Verification and timing

Hardware cases compare mixed nested quads/images/real glyphs at scale 1/1.5,
continuous origin updates, clipping/extent replacement, both control modes and
mode persistence across reset. A direct backend case changes only target bounds,
with no instance/resource writes, and analytically checks disappearing/restored
solid pixels. Queued origin flips stress shared texture/coordinate lifetimes.
The full hardware regression passes 224 scenarios (221 byte-exact comparisons,
one analytical origin-only case, two preflight cases). Core tests pass 116,
gallery/font/real-click tests 12, Clippy and desktop WGPU build pass.

Completed-work controls use the same renderer and cap with origin reuse enabled
or forced replacement. Both paths rerasterize contents and redraw the full main
target. Cases cover light/heavy overdraw and default/single-layer caps. Three
rounds rotate order, warm up six updates and time twenty with final GPU completion.
Per-mode maximum post-submit reservations are recorded rather than lifetime peak
history, which can include a previous control mode. Reference readback follows
timing. [Frozen evidence](performance/2026-11-12-layer-reuse/README.md) preserves
three executions. Under a one-layer budget, light/heavy overdraw improve
1.74×/1.06× while eliminating budget waits. With the default cap, reservations
fall threefold but throughput shows no material improvement (light −0.5%, heavy
+1.2% time). All four 151-source manifests match final bytes and no compiler was
observed during the executions.
