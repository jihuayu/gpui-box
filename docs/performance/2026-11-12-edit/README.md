# Unified render edit evidence

This freezes [unified edit transactions](../../RETAINED_EDIT_TRANSACTIONS.md) on
base `a0e382b`. All 79 recorded source hashes match the final source files.
Adapter: NVIDIA GeForce GTX 1650 Ti, Vulkan, driver 571.96. The current Windows
run passes 90 retained-text/WGPU core cases, 12 gallery/font cases including the
explicit hardware click case, and 152 hardware pixel scenarios. Integrated
Clippy passes with `-D warnings`; command summaries are in `checks.json`.

The three added hardware cases are `edit-source-construct-move-image`,
`edit-rejected-no-submit` and `edit-retirement-old-snapshot`. They combine source
and computed changes, new typed bindings, render construction, existing portal
movement, intrinsic image sizing, opacity, final-subtree deletion and old packet
retention. Output matches fresh full rendering; the first scene additionally
matches a separately built final tree. Failed edits preserve state/stats and
cause no GPU submit. The actual gallery button also creates and removes bound
visible output through normal event/prepaint/paint, checks one revision and
compares changed hardware pixels with fresh full rendering. New real-font tests
cover newly constructed text and a final text-leaf to container conversion.

## CPU structure replacement comparison

Both modes preserve 1,024 stable nodes and replace the stated number of nodes
per event. Sequential uses ordinary direct append, one property commit, and
individual direct removes; draft uses sparse construction/property/removal with
complete final validation and one revision. Both generate one frame per event.
Timing includes operation/list construction. Commit excludes layout/extraction;
total includes frame layout/extraction. GPU and host dispatch/presentation are
excluded. Three rotating rounds per mode contain ten warmups and sixty measured
samples. Every sample checks final geometry/material and expected revisions
outside timing. Raw round samples are in `edit-benchmark.json`.

| Replaced nodes | Sequential commit median / P95 (µs) | Draft commit median / P95 (µs) | Sequential total median / P95 (µs) | Draft total median / P95 (µs) |
| --- | --- | --- | --- | --- |
| 1 | 8.2 / 18.0 | 147.2 / 225.0 | 694.4 / 1192.4 | 830.0 / 1210.3 |
| 32 | 66.6 / 123.9 | 281.3 / 466.7 | 833.6 / 1452.2 | 1071.0 / 1755.6 |
| 256 | 572.8 / 868.1 | 1530.5 / 2181.0 | 1649.6 / 2501.9 | 2561.9 / 3589.2 |

The unified draft is **slower in these structure replacement controls**. Its
benefit is validated atomic publication with one revision/frame intent and
rollback, not proven throughput improvement. The direct control does less
validation and publishes intermediate live states between operations; no frame
is extracted between those operations. The draft still scans the arena/live
edges, uses ordered sparse maps and copies changed parent lists. This is not
a sublinear structure update, faster GPU path or sustained animation result.
Pure property/equal edits delegate to the original commit path and avoid these
topology scans; the free-list copy is lazy on first construction.

Other benchmark JSON files are freshly regenerated suite controls. Their
damage/compositor speedups must not be attributed to unified edit transactions.
Only the final run is frozen here; earlier exploratory timings are not a
before/after optimization claim. The complete RFC remains active: unified
scope/source/effect lifetime changes, general primitives/layout/materials,
resource budgets and platform presentation/cadence/energy acceptance remain
tracked in the [progress table](../../RETAINED_IMPLEMENTATION_PROGRESS.md).
