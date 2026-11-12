# Slot-indexed property dirty journals

Status: partial implementation stage; full RFC acceptance stays active.

## Change and contracts

Private resolved-property and local-placement dirty collections now use per-slot positions and retained vectors of complete NodeIds. Repeated changes rewrite membership in O(1) instead of two ordered-set insertions per changed node. Clear visits only listed slots and retains allocation capacity. The resolved journal spans all local publications until a resolved frame consumes it; the local journal clears after each local publication. Both are required for alternating consumers.

Membership order is not paint or event order. Resolved extraction still sorts topology ranges; GPU spatial updates sort affected indices and anchors. Validated owner identities, exact predecessor checks, old immutable placement pages, skipped-base/full recovery and backend rejection/retry remain unchanged. Structural removal forces complete extraction; slot reuse replaces the journal entry with its new generation. This is private storage, not caller-authoritative dirty metadata.

Cost: each journal retains O(owner slot capacity) position storage and its high-water vector. Sparse cleanup does not scan capacity. This does not solve global resource budgets, effect raster reuse, owner scheduling or native presentation. See [decision](../../ADR_RETAINED_LOCAL_SNAPSHOT.md).

## Alternating exact binary comparison

Control commit `133a153`, SHA-256 `8f0737354db8556c2e7e16417b452a319e8f3af133cfcdac107a601575452774`. New binary SHA-256 `6be5760c4eb7fd23ad6278f25608d046799503468154332b457eccc41749fd60`. Two alternating rounds run control/current then current/control. Each has 120 timed samples per mode/case, unchanged benchmark/shaders/effects/full-redraw/full-viewport isolated-layer policy, stable owning-source manifests and no sampled compiler/benchmark conflicts. OS/thermal/driver effects are not completely controlled. No outlier is removed.

| Round | Effects | Local workload | Commit p50 old / new ms | Commit reduction | Completed p50 old / new ms | Completed reduction | Completed p95 reduction |
| --- | --- | --- | ---: | ---: | ---: | ---: | ---: |
| 1 | False | ancestor | 0.005 / 0.005 | -10.9% | 1.067 / 1.303 | -22.1% | -45.2% |
| 1 | False | independent | 4.146 / 2.694 | 35.0% | 16.013 / 14.474 | 9.6% | 11.8% |
| 1 | True | ancestor | 0.005 / 0.005 | -5.9% | 2.911 / 4.077 | -40.0% | -12.1% |
| 1 | True | independent | 4.317 / 3.015 | 30.2% | 18.108 / 17.708 | 2.2% | 19.1% |
| 2 | False | ancestor | 0.005 / 0.004 | 4.3% | 1.006 / 1.093 | -8.6% | 16.7% |
| 2 | False | independent | 4.732 / 2.866 | 39.4% | 19.328 / 14.466 | 25.2% | 24.6% |
| 2 | True | ancestor | 0.006 / 0.005 | 22.6% | 4.002 / 2.179 | 45.6% | 4.3% |
| 2 | True | independent | 4.400 / 2.732 | 37.9% | 19.161 / 15.095 | 21.2% | 21.7% |

Dense commit medians improve 30.2–39.4% (about 1.3–1.9 ms), dense completed medians 2.2–25.2%. Dense publication medians improve 2.0–36.6%; the representation does not directly change placement storage, so do not causally assign all publisher/driver variation to journal work. Ancestor completed medians lose 8.6–22.1% without effects and 40.0% with effects in round 1, then win 45.6% with effects in round 2. The strong small-scene variation prevents a universal tail/latency conclusion.

Hardware: Ryzen 7 4800H / GTX 1650 Ti 4 GiB, NVIDIA 571.96, Vulkan, 800x600 scale 1. This mobile regression host is not modern desktop GPU qualification. Completed-work timing includes owner commit/publication/submit and synchronous GPU finish, excluding native export/Present, compositor/display and physical input.

```powershell
./tooling/rendering-perf/run-local-ab.ps1 -Output target/journal-ab -ControlBinary target/dirty-journal-control/dense-runtime-perf.exe -ControlCommit 133a153 -Runs 2
python tooling/rendering-perf/analyze-local-ab.py target/journal-ab
./tooling/rendering-perf/run-dense.ps1 -Output target/journal-cadence -SkipBuild -Runs 2 -Local
python tooling/rendering-perf/analyze-dense.py target/journal-cadence
```

## Sustained 60/120 Hz results

Two independent processes each run twelve ten-second cases with stable source manifests and no sampled compiler/benchmark conflicts. Percentiles cover consumed samples; superseded target ticks and consumed-late ticks are disjoint unsatisfied work. This is a current-version sustained run, not an alternating causal comparison with the previous version. Every cadence pixel comparison passes.

| Run | Nodes | Work | Effects | Hz | p95 / p99 / max ms | Superseded | Consumed late | Longest unsatisfied burst |
| --- | ---: | --- | --- | ---: | ---: | ---: | ---: | ---: |
| run-1 | 1000 | ancestor | False | 60 | 5.931 / 7.889 / 9.342 | 0 | 0 | 0 |
| run-1 | 1000 | ancestor | False | 120 | 6.867 / 7.922 / 8.762 | 0 | 7 | 1 |
| run-1 | 1000 | ancestor | True | 60 | 11.532 / 12.415 / 14.601 | 0 | 0 | 0 |
| run-1 | 1000 | ancestor | True | 120 | 6.103 / 7.295 / 11.733 | 0 | 6 | 3 |
| run-1 | 10000 | ancestor | False | 60 | 6.398 / 9.554 / 10.191 | 0 | 0 | 0 |
| run-1 | 10000 | ancestor | False | 120 | 8.303 / 9.761 / 10.814 | 0 | 56 | 1 |
| run-1 | 10000 | ancestor | True | 60 | 12.560 / 13.996 / 18.664 | 0 | 1 | 1 |
| run-1 | 10000 | ancestor | True | 120 | 7.119 / 8.444 / 10.261 | 0 | 13 | 1 |
| run-1 | 10000 | independent | False | 60 | 28.434 / 32.615 / 35.428 | 4 | 368 | 89 |
| run-1 | 10000 | independent | False | 120 | 26.341 / 29.231 / 32.965 | 593 | 606 | 1199 |
| run-1 | 10000 | independent | True | 60 | 35.337 / 36.979 / 42.139 | 67 | 532 | 599 |
| run-1 | 10000 | independent | True | 120 | 29.314 / 33.870 / 41.679 | 672 | 527 | 1199 |
| run-2 | 1000 | ancestor | False | 60 | 5.794 / 7.730 / 8.364 | 0 | 0 | 0 |
| run-2 | 1000 | ancestor | False | 120 | 5.966 / 7.846 / 9.214 | 0 | 3 | 1 |
| run-2 | 1000 | ancestor | True | 60 | 11.355 / 13.088 / 14.107 | 0 | 0 | 0 |
| run-2 | 1000 | ancestor | True | 120 | 6.053 / 7.244 / 12.445 | 0 | 2 | 2 |
| run-2 | 10000 | ancestor | False | 60 | 6.349 / 9.067 / 9.994 | 0 | 0 | 0 |
| run-2 | 10000 | ancestor | False | 120 | 8.015 / 9.175 / 10.717 | 0 | 51 | 1 |
| run-2 | 10000 | ancestor | True | 60 | 7.228 / 8.613 / 10.703 | 0 | 0 | 0 |
| run-2 | 10000 | ancestor | True | 120 | 7.093 / 8.462 / 9.948 | 0 | 14 | 1 |
| run-2 | 10000 | independent | False | 60 | 21.073 / 28.049 / 35.807 | 1 | 129 | 16 |
| run-2 | 10000 | independent | False | 120 | 26.292 / 28.975 / 31.193 | 583 | 616 | 1199 |
| run-2 | 10000 | independent | True | 60 | 36.118 / 39.006 / 41.323 | 77 | 522 | 599 |
| run-2 | 10000 | independent | True | 120 | 27.154 / 32.594 / 41.271 | 613 | 586 | 1199 |

Dense/effects 120 Hz p99 remains 32.59–33.87 ms, max 41.27–41.68 ms, with 1,199 consecutive unsatisfied ticks in both runs. At 60 Hz its p99 is 36.98–39.01 ms and the longest unsatisfied burst is 599 ticks. Ancestor/effects 120 Hz p99 is 8.444–8.462 ms, with 13/14 consumed-late ticks and no skips. Journal optimization does not establish stable 60/120 fps, eliminate effects raster work, or bound owner scheduling. All raw phase samples and losses are retained.

## Verification

152 core tests, 11 ordinary example tests (one hardware test ignored) and GPU benchmark/core Clippy with `-D warnings` pass. The release hardware matrix passes 2,011 scenarios and 1,921 pixel comparisons. Hidden native delivery again coalesces 1,000 samples into one wake/transaction/render and retires completion leases without a new frame. Native shared output matches 97,792 pixels; replacement-device recovery matches 3,072 pixels while preserving old snapshots. Hidden diagnostic draws are not visible sustained presentation. Identities and exact metadata are preserved in `regression.json` and `verification.json`. The full goal remains active.
