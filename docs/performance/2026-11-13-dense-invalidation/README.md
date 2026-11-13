# Dense spatial raster dependency closure

Status: three candidates rejected on measured regressions; runtime restored to `634c8ba`. Added correctness regressions retained. Complete RFC acceptance remains active.

## Investigated change (reverted)

When submitted spatial transport exceeds `max(64, capacity / 4)` entries, the rejected dense final-prefix spatial validation computed a dirty dependency closure while resolving nodes in shader depth order. A small written frontier that merely expands into many descendants keeps the previous ancestry invalidation path. Every raw written slot seeds membership, including overlapping or equal writes, and each non-anchor inherits membership from its final projected parent. Anchors cut ancestry exactly as the shader/full-walk control does. The final validated plan retains affected slot IDs; it does not rewrite resolved memos or commit topology early.

Selected dense Quad/Shadow dispatch scans the contiguous committed reference mirror against closure membership, avoiding gather/sort. Sparse mode retains inverse-user dispatch. Same-batch primitive reference plans commit before affected-user selection, so membership refers to actual new references. Image/glyph retained-content validation consumes the same authoritative closure; new content descriptors retain their own validation. This eliminates the second full per-primitive ancestry walk in dense raster invalidation and the equivalent retained-content dependency scan. It does not skip raw record, edge, prefix, primitive/resource, overflow, clip or layer validation. The unindexed full-walk backend stays unchanged as a control.

Membership uses O(N) temporary bits plus O(affected nodes) indices during a dense preflight that already visits all N nodes. Sparse inverse-user dispatch still gathers/sorts affected primitive slots; dense selection scans all primitive references and layer checks still visit draw ranges. No caller dirty authority, public ABI, shaders, GPU upload layout, raster policy, event order or owner transaction semantics change. Dropped/rejected plans leave committed serial/mirror/indices unchanged. No stable 120 fps claim follows from removing duplicate work.

## Correctness cases

The reverted probe core test permuted all 32 shader depths, repeatedly reparents within the same depth, compares dirty closure against an independent final-ancestry walk and checks inverse-user output after same-frame reference edits, including property zero and shared users. The retained hardware tests add indexed/original five-frame comparisons for isolated Quad and image/glyph content: dense ancestor update invalidates, overlapping reparent changes content, old ancestor becomes unrelated and reuses the layer, new ancestor invalidates again. All images are compared between controls; cached/uncached frame relationships and exact raster reuse counts are asserted.

## Measurement

Control is local commit `634c8ba`; exact old dense binary preserved in `target/dense-invalidation-control/dense-runtime-perf.exe`. Alternating control/current then current/control use 120 samples per mode/case with unchanged benchmark, shaders and layer policy. Sustained tests use two independent processes, twelve ten-second 60/120 Hz cases each. Independent source arrivals continue while owner/driver work is late. Superseded ticks remain unsatisfied work, and all tail outliers/losses remain in raw captures. Same-sample frame-budget analysis computes owner/submit/finish spans before percentiles; it does not add phase percentiles or label finish residual GPU execution time.

The Ryzen 4800H / GTX 1650 Ti regression host is not modern desktop qualification. Offscreen synchronous completion excludes native export/Present, compositor/display completion and physical input. Actual software rendering remains separate outstanding work.

The actual alternating captures are `unconditional-probe/ab/`,
`selective-sort-probe/ab/` and `contiguous-probe/ab/`; only the unconditional
probe has sustained captures at `unconditional-probe/cadence/`. To reproduce a
probe, restore its archived changed sources in an isolated checkout at the
recorded control commit, build the recorded binaries, and run the existing
`run-local-ab.ps1` / `run-dense.ps1` scripts. Running those scripts against the
restored runtime does not reproduce a rejected candidate.

Exact source/binary identities and results remain under each probe directory. The runtime has been restored; restored checks pass: 154 core tests, 11 example tests (one hardware test ignored), strict benchmark Clippy, and 2,015 hardware scenarios / 1,923 pixel comparisons. See `verification.json`. Unconditional probe captures and exact changed-source ZIP are preserved under `unconditional-probe/`. See [frame-budget API decision](../../ADR_RETAINED_FRAME_BUDGET.md) and [spatial dependency decision](../../ADR_RETAINED_SPATIAL_INDEX.md).

## Probe correctness qualification

155 locked core tests and GPU benchmark/core Clippy with `-D warnings` pass. The contiguous probe release hardware matrix passes 2,015 scenarios and 1,923 pixel comparisons, including both new dense Quad and shared image/glyph cache-dependency checks. Contiguous probe binary/validation identities are preserved in `contiguous-probe/regression.json`; the unconditional identities remain in `unconditional-probe/regression.json`. Restored-runtime core/example/hardware checks pass as recorded above; the 155-test count includes a reverted closure unit test and does not describe the restored runtime.

## Unconditional probe: alternating exact binary results

| Round | Effects | Work | Submit p50 old / new ms | Reduction | Completed p50 old / new ms | Reduction | Completed p95 reduction |
| --- | --- | --- | ---: | ---: | ---: | ---: | ---: |
| 1 | False | ancestor | 0.674 / 0.783 | -16.3% | 1.051 / 1.139 | -8.4% | -15.1% |
| 1 | False | independent | 6.579 / 6.487 | 1.4% | 13.693 / 14.138 | -3.2% | -24.3% |
| 1 | True | ancestor | 1.052 / 1.217 | -15.7% | 2.508 / 2.238 | 10.7% | 1.9% |
| 1 | True | independent | 7.170 / 6.748 | 5.9% | 16.577 / 14.378 | 13.3% | 17.8% |
| 2 | False | ancestor | 0.613 / 0.828 | -35.1% | 1.009 / 1.191 | -18.1% | -8.4% |
| 2 | False | independent | 6.691 / 6.450 | 3.6% | 14.095 / 13.820 | 2.0% | 14.4% |
| 2 | True | ancestor | 1.095 / 1.280 | -16.9% | 2.337 / 4.204 | -79.9% | -3.0% |
| 2 | True | independent | 6.990 / 6.325 | 9.5% | 14.452 / 13.588 | 6.0% | 12.2% |

The unconditional probe has a real regression: ancestor submit medians lose 15.7–35.1% in all four rows, and the second ancestor/effects completed median loses 79.9%. Dense independent submit improves only 1.4–9.5%; the first independent/no-effects completed median loses 3.2% and p95 loses 24.3%. This does not justify an unconditional default. Both sustained captures were completed with stable sources before changing hashed sources; preserve them under `unconditional-probe/`. The subsequently rejected selective probe retained original ancestry invalidation when a small written frontier expands into many descendants, using the fused closure only for genuinely dense transport. No new public control or semantic weakening is needed.

## Selective gather/sort probe

The next probe restricted closure construction to dense transport, retaining old ancestry processing for ancestor motion. It still showed no stable dense/effects submit gain (both rounds lose 0.8%) and completed medians lose 0.4/1.6%. Preserve its exact `selective-sort-probe/ab/` captures and changed-source ZIP. The subsequently rejected contiguous probe switched selected dense primitive users to contiguous membership lookup; sparse behavior stays unchanged. Its measured regressions below reject that candidate.

## Final decision

The contiguous candidate regresses dense/effects submission medians 23.0/30.3% and completed medians 16.7/27.4%. Independent/no-effects submit improves 3.3% then loses 1.1%; ancestor results remain variable. No candidate establishes stable target-workload benefit. Restore all runtime changes to `634c8ba`, retain new hardware regressions and preserve every probe ZIP/capture. The final delivery contains tests and negative performance evidence, not a newly enabled invalidation algorithm. No extra full sustained/native performance run is required to claim an unchanged restored runtime; its prior frozen baseline remains applicable. The restored full hardware suite is rerun to validate the added tests.

## Contiguous probe exact results (reverted)

| Round | Effects | Independent | Submit p50 reduction | Completed p50 reduction | Completed p95 reduction |
| --- | --- | --- | ---: | ---: | ---: |
| 1 | False | False | -4.8% | -8.4% | -6.5% |
| 1 | False | True | 3.3% | 3.2% | 4.7% |
| 1 | True | False | -11.9% | -4.9% | -12.9% |
| 1 | True | True | -23.0% | -16.7% | -9.3% |
| 2 | False | False | -4.9% | -0.2% | -1.6% |
| 2 | False | True | -1.1% | -0.9% | 7.1% |
| 2 | True | False | 8.3% | 21.5% | 30.8% |
| 2 | True | True | -30.3% | -27.4% | -23.9% |
