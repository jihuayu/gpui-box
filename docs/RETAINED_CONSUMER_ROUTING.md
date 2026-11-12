# Submitted-snapshot primitive routing

The normal WGPU consumer now reuses unchanged local Quad/Shadow inputs against
its most recently submitted immutable snapshot. Ancestor placement changes
update local property slots and deep resolved anchors without reconstructing
every descendant's already-equal geometry. Material, size, radius and shadow
changes route independently. The world-instance control still updates geometry
when origin/clip changes; image/glyph paths remain as before.

The normal producer privately attests each complete delta transition, including
its exact base/current roots and relevant headers. Unknown or modified deltas
with a claimed matching base use full snapshot recovery and full redraw. This
also repairs missing removal metadata. A different scene at the same submitted
tree/revision is rejected before equal-frame reuse; valid retries remain usable.
See the [decision and failure model](ADR_RETAINED_CONSUMER_ROUTING.md).

`WgpuFrameRenderer::set_cached_primitive_routing_enabled(false)` selects original
preparation for comparison; the default is true. Switching forces a full resync,
and reset preserves the option. `FrameSubmission::routing` reports inspected,
prepared/reused Quad/Shadow items, spatial updates/rebuilds and untrusted-delta
resyncs. Prepared item counters count preparation candidates, including empty
shadow lists in the original path; they are not instance or uploaded-byte counts.
The GPU submission retains separate actual upload counters.

Validation and completed-owner measurements, including sparse losses and scope
limits, are in the [frozen evidence](performance/2026-11-12-consumer-routing/README.md).

```powershell
./tooling/rendering-perf/run-retained-gpu.ps1 -Output target/routing-validation -ValidationOnly
./tooling/rendering-perf/run-retained-gpu.ps1 -Output target/routing-run -RoutingOnly
```

Both benchmark modes use the same owned publisher, shared GPU hierarchy, shader,
private transition checks and full redraw. Only consumer input routing changes.
Older spatial/publication benchmarks explicitly disable this new routing on
both sides so their controls retain their original meaning.

This stage does not complete the RFC. Dense resolved extraction, composition and
backend allocation-prefix scans remain. Native surface presentation, broader
primitive/property/layout contracts, physical resource budgets, device-loss and
cross-platform/sustained-frame acceptance remain tracked in
[implementation progress](RETAINED_IMPLEMENTATION_PROGRESS.md).
