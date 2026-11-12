# Indexed GPU spatial preflight and raster dependencies

The default backend now retains validated shader-parent adjacency and resolved
spatial state, plus independent inverse Quad/Shadow property memberships.
Stable sparse updates validate only affected spatial descendants and projected
changed references. Ancestor changes still visit affected descendants once to
check finite arithmetic; depth buckets replace repeated full ancestry walks.
Inverse users produce existing raster dirty ranges directly.

When the affected frontier exceeds `max(64, capacity / 4)`, the default uses the
original full spatial validation/invalidation path and keeps indexed reference
checks. This limits the unconditional prototype's dense bookkeeping. It
invalidates resolved memos without rewriting them; following sparse updates
read needed ancestors from the actual mirror. The threshold is a heuristic,
with its original losing probe retained alongside final measurements. Dense
completed-time improvement remains unproven; the frozen results include losses.

The subsequent [dense preflight refinement](performance/2026-11-13-dense-validation/README.md)
retains this full-prefix fallback and original-walk control, but avoids dense
ordered-map construction and resolves each projected node once in depth order.
Transport density can select full checking before unique-slot deduplication;
overlapping final writers and rejected plans retain the same correctness rules.
Additional temporary resolved/depth arrays cost O(N) memory. Alternating exact
binaries now show 7.0–19.7% dense submit median and 2.7–15.9% completed median
improvement, with a losing ancestor/effects round retained. Current sustained
dense/effects 120 Hz p99 still spans 28.24–38.80 ms and does not meet deadlines.

This reuse is based on backend-owned actual mirrors, including the full
addressable prefix. Public raw batches need no caller trust token. Final-write
projection handles overlaps; existing typed field/range checks still run for every submitted write.
Spatial capacity/scale changes rebuild, and reference bounds/prefix changes use
complete checking when the cached maximum no longer proves safety. Data
reexposed from a previously larger mirror is checked again.

Authoritative index updates are staged until the entire batch passes preflight.
Later image/glyph/layer/damage rejection cannot poison the base or memberships.
Shader ABI, GPU allocations/uploads, draw order and completion leases remain as
before. The [decision record](ADR_RETAINED_SPATIAL_INDEX.md) explains coverage
and failure invariants.

`WgpuFrameRenderer::set_indexed_spatial_enabled(false)` selects the original full
scan control; switching forces resync and reset preserves the option. The raw
`RetainedQuadRenderer` exposes the same option and invalidates indices on change.
`QuadSubmission::spatial_work` reports indexed/full mode, checked edges/resolved
nodes, reference checks and property-affected Quad/Shadow instance counts. Edges
include anchor nodes; affected-instance counts exclude independent geometry
writes and may conservatively include old descendants after reparenting.
`dense_fallback` identifies the heuristic switch; `indexed` reports that the
policy is enabled, including frames that take this full-spatial fallback.

```powershell
./tooling/rendering-perf/run-retained-gpu.ps1 -Output target/spatial-index-validation -ValidationOnly
./tooling/rendering-perf/run-retained-gpu.ps1 -Output target/spatial-index-run -SpatialIndexOnly
```

The [frozen evidence](performance/2026-11-12-spatial-index/README.md) records phase
measurements, work counts, pixels, source manifests and limitations. Both timing
controls keep owned publication, cached local primitive routing, shared GPU
placement, shader and full redraw; only backend spatial processing changes.
Older spatial/publication/consumer-routing benchmarks disable the new backend
index on both sides to preserve their comparison scope.

The full RFC goal remains active. Dense extraction/validation, composition and
resource/lease scans, global CPU/GPU budgets, native surfaces, broader primitive/
layout contracts and sustained/cross-platform acceptance remain in the
[progress tracker](RETAINED_IMPLEMENTATION_PROGRESS.md).
