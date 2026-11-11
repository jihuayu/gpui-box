# 2026-11-11 CPU experiment evidence

These files record the final, paint-layer-controlled version of
[`gpui-box-rendering-perf`](../../../tooling/rendering-perf/README.md).
See the [research report](../../RENDERING_PERFORMANCE_RESEARCH.md) for conclusions.

- [metadata.json](metadata.json): base/upstream revisions, lockfile and measured
  source SHA-256 hashes, toolchain, OS, CPU, GPU inventory, power plan and settings.
- [run-1-summary.csv](run-1-summary.csv), [run-2-summary.csv](run-2-summary.csv),
  [run-3-summary.csv](run-3-summary.csv): per-case nearest-rank p50/p95/p99
  of **batched average nanoseconds per operation**, plus structural counters.
- [run-1-samples.csv](run-1-samples.csv), [run-2-samples.csv](run-2-samples.csv),
  [run-3-samples.csv](run-3-samples.csv): every sample's elapsed time, operation
  count and counters, before summary calculation.

Reproduction command, from the repository root:

```powershell
./tooling/rendering-perf/run.ps1 -Output target/rendering-perf-grouped-final -Runs 3
```

There are 14 cases × 3 sizes × 101 samples = 4,242 sample rows per run.
The report's main table takes the median of the three run-level p50 values.
Its variability table shows the minimum and maximum run-level p50 and p95.
These are CPU stage measurements, **not frame latency, GPU time, real text
shaping or performance of a complete application**. GPU inventory was not
exercised. `modeled_instance_bytes` is a source-derived estimate, not a transfer
measurement. Timings include counters and have substantial host variability.

The measured `Quad` is 160 bytes in this build: the modeled full-array payload
is 48,000 / 480,000 / 4,800,000 bytes at the three sizes. A hypothetical one-slot
update could reduce that payload to 160 bytes, subject to backend alignment and
allocation rules. **This does not establish a working P1 path or a 3,000× speedup.**
GPU drawing and other frame costs can remain unchanged.

The base commit identifies the existing engine. The research tool was uncommitted
when measured; the source hashes identify its actual measured contents. Docs and
result-file copies were written afterward. The final branch commit records all
deliverables together.
