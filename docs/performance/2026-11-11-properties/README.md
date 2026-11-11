# Property and damage evidence

This freezes the [property/damage stage](../../RETAINED_PROPERTIES_DAMAGE.md)
on GTX 1650 Ti / Vulkan. `metadata.json` records the source hashes and toolchain;
`adapter.json` records hardware. `validation.json` and the PNGs contain 26 passing
pixel scenarios. `damage-benchmark.json` records three rotating A/B rounds per
scene with full, partial and area-threshold automatic redraw.

Timing includes CPU update work and GPU completion, excludes readback and
presentation, and does not isolate GPU execution time. The grid case shows no
speedup. The two overlapping-layer cases are deliberate fill-rate stress tests.
Raw per-round timings preserve the substantial variance in short partial draws.
See the linked report for the scope, limitations and reproduction commands.
