# Experimental indexed ink bounds for local snapshots

Status: implemented behind a separate default-disabled consumer switch after
dense-scene regressions. The overall rendering goal remains active.

## Problem and decision

The local snapshot consumer retains placement/resource payloads and changes GPU
spatial nodes without resolving every descendant paint item. Its original
isolated layer records therefore use full-viewport bounds. A full-viewport layer
cannot translate with a smaller group or use the integer-translation raster
reuse probe effectively. Reading the old resolved packet's ink would be stale.

Add a consumer-owned segment index over the private immutable render preorder.
Each leaf stores current clipped ink; internal records union child rectangles.
The snapshot supplies generation-checked subtree ranges, minimal disjoint changed
subtree ranges, and an internal geometry visitor that resolves open ancestry in
the same f32 order as the complete extractor. The visitor retains only its open
ancestry and clones transient paint items, without materializing a complete
resolved packet or changing owner state. Layer queries include nested content.

Sparse updates visit affected subtrees and update their index ancestor intervals.
Queries take O(log N); aggregate storage is O(next_power_of_two(N)). A changed
ancestor still visits every affected descendant: this is not a local-space
translation aggregate and does not make ancestor ink work sublinear. Dense
disjoint changes may revisit overlapping index ancestors. Initial/fallback
resolution rebuilds the index. Complete source/layout/material changes, skipped
predecessors, incompatible capabilities and reset retain complete recovery.

Physical bounds use the existing clipped ink plus a one-pixel fringe, clamped to
the surface. The backend still validates target extents and applies its memory
budget; changing a bound never bypasses raster/content/resource validation.
Full damage and exact linear hit traversal remain in place. The index is private
and does not authorize application-supplied dirty ranges or content identities.

## Measured default decision

`set_local_tight_layer_bounds_enabled` is independent of resolved tight bounds
and the backend translation reuse switch. It defaults to false and survives
consumer reset/device replacement. Normal local submission keeps its original
full-viewport path. Explicitly enabling both local tight bounds and backend
translation reuse lets the real local snapshot update path skip qualifying
integer group raster work; it does not automatically enable the feature for UI.

Two ten-second 120 Hz overdraw captures improve completed-frame counts, but not
stable deadlines: optimized 1,190 and 1,164 / 1,200 versus 1,132 and 1,157 for the
same local route without translation reuse. Optimized p99 is 8.01 / 10.66 ms;
original raster is 10.60 / 15.29 ms. The second optimized run still coalesces five
samples. Native input/upload/display are excluded. All 60 Hz cases meet source
deadlines, but optimized maximums can be worse than original raster.

Crucially, alternating 10,000-node captures with translation reuse disabled show
that enabling indexed tight local bounds loses heavily: ancestor/effects submit
medians regress 174.8/266.5%, completed medians 105.7/165.5%; independent/effects
submit regresses 76.5/63.4%, completed 44.0/35.0%. These exploratory sequential
captures retain exact sources/binary/raw samples, but have no periodic process
conflict sampler. No-effects submission is variable, including a slight winning
round, and completed times also lose. This is enough to reject default enable,
not to claim a hardware-independent cost ratio or a production throughput gain.

The earlier full-viewport mode remains selectable and supplies a same-executable
control. The implementation is a useful correctness/integration stepping stone;
it does not solve the target dense update bottleneck. The next design should
retain content-local ink and separate placement/clip eligibility, or use measured
cost admission to fall back without per-descendant resolution. Merely choosing a
smaller texture is insufficient for the 8.333 ms owner budget.

## Validation and remaining work

Unit tests compare every indexed subtree with complete resolution after mixed
overlapping updates, clips/shadows and old-snapshot retention. Hardware compares
mixed text/images, nested effects, fractional placement/scale, deep anchors,
skipped bases/reset and representation switching. Targeted controls verify
integer local translation reuse, fractional fallback, material/opacity/scroll/
clip edits and exact hit geometry. Normal native upload/present integration uses
the existing local consumer entry point, but visible presentation/input latency
has not been measured for this feature.

Keep general persistent local-space content descriptors, resource/sample epochs,
nonlinear clips/transforms, indexed hit/damage, CPU fallback integration, dense
owner construction, native presentation and modern desktop hardware acceptance
open. See [evidence](performance/2026-11-13-local-layer-bounds/README.md) and
[translation reuse](ADR_RETAINED_LAYER_TRANSLATION.md).
