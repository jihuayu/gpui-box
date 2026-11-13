# Rejected preflight node-group capacity reservation

Status: rejected experiment, runtime restored toc0a8485. Full goal active.

## Hypothesis and implementation

After the existing stable Patch sort, count adjacent distinct NodeIds and reserve
that many PropertyChange entries before grouped preflight. The final complete
validated list remains private until every node succeeds. Last-writer order,
overwritten invalid input checks and source/property atomic publication stay
unchanged. The five-line candidate avoids geometric growth/copies of wide Pending
records, but adds an O(patches) identity scan. Final equal groups still consume
reserved capacity until return; no actual-change count is known in advance.
This is a general allocation probe, with no workload threshold or API addition.

167core tests, strict library Clippy and117mixed hardware comparisons pass.
Existing tests cover overwritten NaN/infinity, interleaved writers, final
restore/no-op, late foreign node failure, nested source rollback and graph/sink
failure. No full hardware suite was run for this rejected candidate.

## Exact reversed sustained pairs

Exactc0a8485 ordinary binary versus ordinary candidate, original/reserved then
reserved/original.10k leaves, ten isolated effects and ancestor motion, dense
color epochs every half second,800x600 scale1,60/120Hz, ten seconds/case and
admission after/before construction. All16cases /14,400scheduled ticks account
for19dense epochs/case, match fresh final pixels, retire leases, have<=2GPU
submissions and0post-initial full resyncs. Strict timing/tick analysis passes.
Source/binary hashes are stable; no known compiler/renderer conflicts were
observed in250ms process samples. No rows/outliers are removed.

| Round / Hz / admission | Dense commit median original → reserved ms | Completion p99 / max original → reserved ms | Unsatisfied ticks | Longest unsatisfied ticks |
| --- | ---: | ---: | ---: | ---: |
|1 /60 /after |4.860 →4.762 |17.13 /18.83 →17.98 /20.69 |12 →14 |1 →1 |
|1 /60 /before |4.999 →4.616 |19.26 /21.89 →21.11 /28.46 |19 →18 |1 →1 |
|1 /120 /after |5.479 →4.744 |18.01 /21.75 →17.69 /21.73 |46 →44 |3 →3 |
|1 /120 /before |5.161 →4.654 |18.00 /24.87 →17.24 /22.12 |42 →47 |2 →3 |
|2 /60 /after |4.378 →4.742 |17.96 /19.85 →18.03 /19.31 |12 →15 |1 →1 |
|2 /60 /before |4.865 →4.923 |19.27 /21.19 →19.45 /21.27 |16 →18 |1 →1 |
|2 /120 /after |4.718 →4.609 |17.11 /22.70 →17.52 /21.97 |42 →44 |3 →3 |
|2 /120 /before |5.254 →4.897 |17.48 /21.84 →17.70 /24.14 |43 →45 |4 →3 |

Dense commit medians improve6pairs by2.0–13.4%, but regress2by1.2–8.3%.
Dense total owner medians improve4and regress4; candidate remains15.05–17.32ms.
All four120Hz p95 values worsen:7.483→7.643,7.360→7.510,7.297→7.385 and
7.162→7.619ms. Both round2completion p99/loss totals worsen. One better maximum
or median does not establish a general stall/120Hz benefit. Wall times include
OS/driver/cache/scheduling effects; these pairs do not prove an allocator cause.

## Decision and evidence

Restore the original runtime exactly instead of retaining an extra pre-scan and
unneeded capacity for an inconsistent gain. The general lesson is to attribute
source settlement, sink fanout, property preparation/application and cleanup
before changing another container. Do not split atomic commits or expose an
unchecked batch mode merely to reduce benchmark time.

`rejected-candidate.zip` preserves exact original/candidate retained.rs and patch.
`evidence.zip` preserves raw rows, strict analysis, same-row phase pairs, commands,
hashes, process observations and117mixed GPU comparisons. `validation.json`
records candidate checks and restoration. Both default release binaries were
rebuilt after byte-exact restoration; the baseline remains in
`target/preflight-reserve-baseline/dense-runtime-perf.exe`.

Hardware Ryzen4800H / GTX1650Ti Vulkan is a regression host, not modern desktop
qualification. Receipts are coalesced native-wait completion upper bounds, not
GPU execution/native present/display/physical input. Native CPU responsiveness,
full rendering semantics/resources and cross-platform acceptance remain open.

```powershell
# Restore the archived candidate first; current runtime is the original.
python tooling/rendering-perf/capture-burst.py target/reserve-new --baseline target/preflight-reserve-baseline/dense-runtime-perf.exe --candidate-name reserved --pipelined
python tooling/rendering-perf/analyze-pipeline-burst.py target/reserve-new --implementation-pair --candidate-name reserved
```
