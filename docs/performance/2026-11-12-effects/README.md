# Post-commit effect evidence

This freezes [post-commit observation and cleanup](../../RETAINED_EFFECTS.md)
on base `7c74379`. All 12 CPU source hashes and 62 hardware source hashes were
verified against the final working tree. Adapter: GTX 1650 Ti / Vulkan 571.96.
Core tests pass 66 cases. Ten real-font/gallery tests pass, including normal
and hardware clicks through computed color and a keyed post-commit observer.
Combined and feature-specific Clippy and desktop WGPU build validation are
recorded with the stage. The hardware suite passes 139 pixel scenarios.

The existing computed GPU cases now attach a real observer to derived bucket,
color and translation. They check final consistent values, one run and prior
cleanup for the mixed property/material batch, no run for equality, read-restore
or invalid sink, and cleanup when the logical runtime drops. Four new core
cases additionally test returned failures, observer dependency validation,
multi-input deduplication, exact cleanup order, explicit disposal, keyed
reorder, cross-scope lifetime checks and runtime drop without duplicate cleanup.

## CPU observer fanout

Twelve warmup samples precede 180 measured samples. Source-equal samples
average 1,000 events; derived-equal and changed samples average 100. Counters
and zero frame requests are asserted. Each observer reads its dependency,
increments a shared counter and allocates a cleanup closure that increments
a second counter. Those callback/cleanup costs are included in timing. Setup,
final disposal, render sinks, GPU and presentation are excluded. This measures
synchronous transaction plus effect cost, not application speedup. Percentiles
are of batch averages; external host load remains uncontrolled.

| Observers | Workload | Median / P95 per event (µs) |
| --- | --- | --- |
| 0 | source-equal | 0.066 / 0.078 |
| 0 | derived-equal | 0.394 / 0.607 |
| 0 | changed | 0.491 / 0.567 |
| 1 | source-equal | 0.066 / 0.147 |
| 1 | derived-equal | 0.393 / 0.891 |
| 1 | changed | 0.640 / 0.745 |
| 1,000 | source-equal | 0.067 / 0.099 |
| 1,000 | derived-equal | 0.392 / 0.882 |
| 1,000 | changed | 232.234 / 251.366 |

Equal source/output cases do not traverse observer fanout. Changed-output
cost grows with the observer set. The computed benchmark and regenerated GPU
benchmarks are included as controls; they are not performance attribution to
effect support. The API supplies synchronous read-only effects and lifecycle
cleanup. Asynchronous/reentrant effect scheduling, dynamic dependencies,
cross-event producer coalescing, native GPU presentation and the remaining RFC
acceptance are still open. The full goal remains active.
