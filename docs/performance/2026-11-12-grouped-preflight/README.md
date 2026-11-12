# Grouped atomic property preflight

Status: partial implementation stage; complete RFC goal remains active.

## Decision and implementation

Stable node sorting still defines private property preparation. Each node now
coalesces and validates its patches before appending its final changes to the
complete uncommitted change list. This removes a second dense array of wide
Pending records, its growth/moves and repeated node lookups. All changed nodes
remain staged until every input succeeds; revision, sources, derived values,
render state and effects still publish at the existing atomic boundary.

Overwritten invalid values still fail validation. Interleaved node identities
preserve each field's last writer. No unchecked fast batch API, caller-owned
GPU identity or per-node publication is introduced. Resource, text/image and
measured-leaf conflict checks remain in the grouped preflight. Which error wins
when multiple different inputs are invalid is not an event-priority contract.
See [frame-budget decision](../../ADR_RETAINED_FRAME_BUDGET.md).

## Controlled comparison

Control is commit `bc29297`, binary SHA-256
`5b1a5c32fad06d994767b025d8f1b674a186af9e1048ec64bb4091d065272648`.
New binary SHA-256 is
`e31dd16ec38eebf18b754e7ba09ef60642957147ae9ef9f740bcb59fc14316e9`.
Two alternating rounds run control/current then current/control, with 120 timed
samples per mode and unchanged benchmark, shaders, effects, full redraw and
full-viewport isolated layers. Owning-source hashes remain stable and 500 ms
process sampling reports no compiler/benchmark conflicts. No sample is removed.
OS/thermal/driver activity is not fully controlled by process sampling.

Hardware is Ryzen 7 4800H / GTX 1650 Ti 4 GiB, driver 571.96, Vulkan, 800x600 at
scale 1. This mobile regression host cannot qualify modern desktop visible
120 fps. Completed time ends at synchronous GPU completion, excluding native
scene export/Present, compositor/display and physical input.

```powershell
./tooling/rendering-perf/run-local-ab.ps1 -Output target/grouped-ab -ControlBinary target/dense-prepare-control/dense-runtime-perf.exe -ControlCommit bc29297 -Runs 2
python tooling/rendering-perf/analyze-local-ab.py target/grouped-ab
./tooling/rendering-perf/run-dense.ps1 -Output target/grouped-cadence -SkipBuild -Runs 2 -Local
python tooling/rendering-perf/analyze-dense.py target/grouped-cadence
```

## Alternating results

| Round | Effects | Workload | Commit p50 old / new ms | Commit reduction | Completed p50 old / new ms | Completed reduction | Completed p95 reduction |
| --- | --- | --- | ---: | ---: | ---: | ---: | ---: |
| 1 | False | ancestor | 0.005 / 0.006 | -5.6% | 1.126 / 1.166 | -3.5% | -6.2% |
| 1 | False | independent | 5.604 / 4.393 | 21.6% | 18.278 / 17.008 | 6.9% | 4.1% |
| 1 | True | ancestor | 0.005 / 0.006 | -12.0% | 3.285 / 3.135 | 4.6% | 14.5% |
| 1 | True | independent | 5.425 / 4.076 | 24.9% | 19.777 / 18.038 | 8.8% | 5.4% |
| 2 | False | ancestor | 0.005 / 0.005 | 7.4% | 0.981 / 0.988 | -0.6% | -7.8% |
| 2 | False | independent | 5.462 / 4.162 | 23.8% | 18.051 / 16.385 | 9.2% | 10.8% |
| 2 | True | ancestor | 0.006 / 0.006 | -6.8% | 3.152 / 4.116 | -30.6% | -11.2% |
| 2 | True | independent | 5.435 / 4.063 | 25.2% | 21.062 / 18.003 | 14.5% | 13.4% |

Dense commit medians improve consistently by 21.6–25.2%, about 1.2–1.4 ms. Dense completed medians improve 6.9–14.5%. This is evidence for reducing the owner preflight working set without adding a public unchecked batch route. The second ancestor/effects case regresses 30.6% in completed median; no stage-wide latency win or 120 Hz guarantee follows. In round 1 dense/no-effects publication regresses 14.1%, despite unchanged publisher code. Whole-process timing retains driver/OS/cache effects and does not establish that every changed phase is causally affected by preflight.


## Scope and verification

Only `crates/gpui-box/src/retained.rs` differs among hashed owning sources from
`bc29297`; benchmark and shader hashes are unchanged. The full retained-wgpu/text core suite passes 148 cases; retained-text-only tests
pass 123 cases; the normal example tests pass 11 with one hardware-only test
ignored in the ordinary suite. Retained-only core and GPU benchmark Clippy pass `-D warnings`.
The new regression deliberately interleaves nodes, attempts overwritten NaN
and infinity late in the batch, verifies unchanged revision/shared snapshot on
rejection, then retries valid final writers and verifies old snapshot contents.
The current release hardware matrix passes 2,011 scenarios / 1,921 pixel
comparisons. See `regression.json` for binary/validation identities. The previous
stage's hidden native results belong to its older executable and are not a
new visible presentation result for this version.

Dense 60/120 Hz deadlines, local-space effect content identity and raster reuse,
preconstruction visual admission, local hit/damage indexing, visible pipelined
source-to-present correlation, larger surfaces/resource churn and modern desktop
hardware qualification remain open. CPU rendering still requires a real
consumer with explicit capabilities, cooperative work budgets, complete-frame
publication and responsiveness/fidelity tests. The overall RFC is not complete.

## Sustained results

Two processes each run twelve ten-second cases, with stable source manifests, exact executable hashes and no sampled compiler/benchmark conflicts. The independent OS source and owner inbox/wake/computed/sink/transaction/mailbox/GPU-completion chain is unchanged. Percentiles cover consumed samples only; skipped ticks and consumed-late ticks are disjoint unsatisfied work. Each 120 Hz row has 1,200 target ticks per run. All pixels match the full reference.

| 120 Hz workload | Run 1 p95 / p99 / max ms | Run 2 p95 / p99 / max ms | Superseded 1 / 2 | Consumed-late 1 / 2 | Longest unsatisfied burst 1 / 2 |
| --- | ---: | ---: | ---: | ---: | ---: |
| 1,000, ancestor, effects=False | 5.526 / 8.220 / 9.584 | 5.728 / 8.283 / 10.712 | 0 / 0 | 12 / 11 | 1 / 2 |
| 1,000, ancestor, effects=True | 6.102 / 7.222 / 13.585 | 6.034 / 7.274 / 11.527 | 0 / 0 | 6 / 3 | 2 / 1 |
| 10,000, ancestor, effects=False | 8.646 / 10.133 / 10.888 | 8.127 / 9.681 / 10.626 | 0 / 0 | 74 / 56 | 2 / 1 |
| 10,000, ancestor, effects=True | 7.190 / 8.449 / 10.050 | 7.061 / 8.480 / 10.099 | 0 / 0 | 14 / 16 | 1 / 1 |
| 10,000, independent, effects=False | 28.627 / 31.691 / 35.622 | 27.673 / 30.473 / 34.791 | 676 / 654 | 523 / 545 | 1199 / 1199 |
| 10,000, independent, effects=True | 31.361 / 34.334 / 40.275 | 27.895 / 29.553 / 33.507 | 713 / 672 | 486 / 527 | 1199 / 1199 |

Independent/effects 120 Hz p99 is 29.55–34.33 ms versus the preceding historical 39.14–39.27 ms; this sustained historical comparison is not alternating A/B. The causal control above supports dense commit/median improvement. Both new 120 Hz runs still have 1,199 consecutive unsatisfied ticks: 8.333 ms remains unachieved. At 60 Hz independent/effects p99 is 38.20–44.75 ms, max 40.76–47.08 ms, with 598/599 total unsatisfied ticks and longest bursts 434/599; 60 Hz also fails. No-effect dense cases have 599/1,199 consecutive unsatisfied ticks at 60/120 Hz.

Ancestor/effects 120 Hz p99 is 8.449/8.480 ms, max 10.050/10.099 ms, with 14/16 consumed-late ticks. No-effect ancestor 120 Hz misses 74/56 ticks. Small-scene 120 Hz cases miss 3–12 ticks and have a 13.585 ms maximum. At 60 Hz all 1,000-leaf and no-effect 10,000-leaf ancestor runs have zero misses; 10,000-leaf effects miss 1/2 ticks with max 18.58/22.98 ms. These results do not satisfy an absence-of-burst-stalls claim.

run-1 dense/effects p50 phases: commit 5.835 ms, publication/mailbox 2.456 ms, submit 7.998 ms. These are separate phase medians, not an additive end-to-end percentile.
run-2 dense/effects p50 phases: commit 5.604 ms, publication/mailbox 2.222 ms, submit 7.852 ms. These are separate phase medians, not an additive end-to-end percentile.

Full 60/120 Hz distributions, counters and every raw sample remain in `cadence/summary.json` and `cadence/run-*/cadence.json`. Synchronous GPU completion excludes native export/Present/display and physical input; no modern desktop or visible 120 fps claim is made.
