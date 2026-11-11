# Unified keyed lifecycle evidence

This freezes [unified keyed scope/reactive lifetimes](../../RETAINED_LIFECYCLE_EDITS.md)
on base `b0cd826`. All 81 hardware runner source hashes match final files.
Windows GTX 1650 Ti / Vulkan driver 571.96 passes 157 hardware pixel scenarios.
The retained-text/WGPU core suite passes 104 tests; gallery/font passes 12,
including the explicit GPU click case. Integrated Clippy passes `-D warnings`.
Commands and scope limits are recorded in `checks.json`.

The four hardware additions are `lifecycle-keyed-build`,
`lifecycle-failure-preserves-effects`, `lifecycle-replace-retire-once` and
`lifecycle-final-retirement`. They construct keyed scopes, writable/derived state,
effects and bound output, reject invalid mixed retirement/construction without
submission or cleanup, replace a scope with exactly one cleanup and one render
revision, then retire all output. Each actual image equals full rendering;
surviving old snapshots retain their prior content. The real gallery toggle now
constructs/destroys a nested keyed component with state, computed material,
effect cleanup and bound visible output in its ordinary click/prepaint path.

Core additions cover leaked provisional scope/source/effect/binding/node handles,
generation/revision exhaustion, create-then-cancel without callback/frame work,
retained key initializers and chunks, final render ordering, dependencies across
retiring scopes, surviving/new computation rejection, surviving portal evacuation,
background sender lifetime after failed/successful retirement, zero-output
scope events and post-commit observer/cleanup panics. The logical wrapper now
publishes all ownership maps before running any user cleanup/observer.

## CPU lifecycle replacement controls

Each replaced scope owns writable state, a computed source depending on root
state, one effect with cleanup, one render node and one binding. There are 512
stable render nodes. Sequential control uses existing keyed retirement and
individual initialization APIs; draft combines the complete lifecycle event.
Both extract one final frame per event. Commit includes construction and cleanup;
total adds layout/extraction. Final geometry/material, live source/effect/binding
counts and callback counts are asserted outside timing. Three rotating rounds
per mode use ten warmups and forty measured events; raw samples are preserved
in `lifecycle-benchmark.json`. No other compilation ran during measurements.

| Replaced scopes | Sequential commit median / P95 (µs) | Draft commit median / P95 (µs) | Sequential total median / P95 (µs) | Draft total median / P95 (µs) |
| --- | --- | --- | --- | --- |
| 1 | 110.7 / 187.1 | 159.6 / 249.2 | 403.2 / 689.4 | 431.6 / 704.5 |
| 16 | 199.3 / 343.0 | 361.9 / 611.0 | 517.0 / 965.4 | 707.3 / 1190.5 |
| 128 | 840.0 / 1469.3 | 1760.5 / 2580.6 | 1304.6 / 2262.7 | 2293.2 / 3271.5 |

The draft remains slower in these controls. It provides whole-event validation,
rollback and one revision/frame intent, while the sequential control publishes
intermediate states with less validation. Both already avoid intermediate frame
extraction. Full logical rank, topology, retired binding and structure extraction
scans plus sparse maps/list copies remain. This is correctness/lifecycle progress,
not a throughput improvement claim. Other regenerated JSON benchmarks are
suite controls and their GPU/damage gains are not attributed to this change.

Native-surface presentation, 60/120 Hz/cadence, input-to-present latency, energy,
cross-platform testing, asynchronous effects/dynamic dependencies and other
RFC acceptance work remain in the [active progress table](../../RETAINED_IMPLEMENTATION_PROGRESS.md).
