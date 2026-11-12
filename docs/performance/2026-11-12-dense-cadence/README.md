# Dense publication and sustained deadline evidence

The total RFC goal remains active. These results establish CPU/GPU work and
tail behavior on this host, not modern desktop GPU or native display acceptance.

Hardware: Ryzen 7 4800H, GTX 1650 Ti 4 GiB, NVIDIA 571.96, WGPU Vulkan.
800×600 physical output, scale 1. This mobile GPU is a regression baseline.
Two independent processes completed twelve sustained 10-second cases each
(240 seconds of target cadence), after paired publication controls.
Every final image matched the full reference path, outside the timed span.

Reproduce from the worktree with Rust 1.98.1:

```powershell
./tooling/rendering-perf/run-dense.ps1 -Output target/dense-reproduction -Runs 2
python tooling/rendering-perf/analyze-dense.py target/dense-reproduction
```

The runner rejects existing output, hashes owning runtime/native/benchmark
sources before/after build and measurement, hashes the executable and samples
compiler/other benchmark processes every 500 ms. Both runs have stable sources
and no sampled conflicting processes. It cannot exclude every unrelated OS,
thermal, power or GPU workload. All samples and outliers are preserved.
Metadata and source manifests are adjacent; `summary.json` derives from
`run-1`/`run-2` raw JSON without filtering. The run binary SHA-256 is
`6ac16f692f096a12566878b158fc86c78059b827e612fac7572895aecd1aa775`.

## Paired path comparison

Both modes use the same runtime, shaders, consumer routing, buffers and GPU.
The control uses `SelectiveReextract`; the optimized path retains local payloads
and resolves property-only subtrees without Taffy/glyph preparation. Dense
property edits merge once into balanced immutable maps. Hash index lookup and
consumer validation improvements apply to both modes: this isolates the
publication change, not every change from the base commit.

Each process rotates mode order over three rounds, discards six warmups per
round, retains 120 samples per mode/case and compares full-reference pixels.
An ancestor move changes one 96-byte placement record for 10,000 leaf quads.
The independent case changes 10,000 translations in one validated transaction,
uploading 960,000 spatial bytes. Quad geometry upload stays zero in both cases.
Effects add ten isolated opacity groups (0.85) and ten blur-8 box shadows;
the current path rasterizes all ten groups on ancestor movement (zero reuse).

| 10,000-leaf update | Publication p50 reduction, runs 1 / 2 | Owner-to-GPU-completed p50 reduction, runs 1 / 2 |
| --- | ---: | ---: |
| Ancestor, no effects | 18.1% / 15.5% | 12.5% / 9.6% |
| Ancestor, effects | 19.7% / 17.4% | 12.4% / 10.0% |
| Independent batch, no effects | 3.0% / 3.1% | **−0.5%** / 2.0% |
| Independent batch, effects | 7.6% / 0.3% | 4.2% / **−0.8%** |

Dense independent improvements are not stable. In run 1, no-effect batch
completed p95 regressed from 35.79 to 39.15 ms. In run 2, effect batch p95
regressed from 38.37 to 38.72 ms. Ancestor effects in run 1 improved median
but p99 regressed from 20.45 to 23.12 ms. This stage does not solve burst stalls.

## Sustained source-to-GPU completion

An independent OS thread generates absolute-clock visual state at 60/120 Hz.
It sends through the actual bounded `UpdateSender` inbox. The owner sleeps on
the actual receiver's executor waker, then drains one reactive transaction,
evaluates a computed translation, fans out typed bindings, publishes an owned
packet through `FrameQueue`, submits and synchronously waits for GPU completion.
Source schedule delay, owner wait, transaction, publication and submit phases
are retained separately with p50/p95/p99/max.

The producer runs independently during owner/GPU work. Obsolete visual samples
are counted, and the queue capacity is one pending source. Business commands
are not part of this last-value workload. First sample equals the prepared
placement, so it correctly produces no additional GPU submission. An unchanged
sample is not evidence of rendering a fresh frame.

| 120 Hz workload | Run | p95 / p99 / max (ms) | Superseded ticks | Consumed ticks missing deadline |
| --- | --- | ---: | ---: | ---: |
| 1,000 leaves, ancestor, no effects | 1 / 2 | 2.72 / 3.20 / 3.71; 3.23 / 4.10 / 5.25 | 0 / 0 | 0 / 0 |
| 1,000 leaves, ancestor, effects | 1 / 2 | 3.63 / 4.46 / 5.84; 3.49 / 4.11 / **9.26** | 0 / 0 | 0 / **1** |
| 10,000 leaves, ancestor, no effects | 1 / 2 | 27.53 / 33.69 / 38.53; 23.52 / 25.45 / 30.77 | 577 / 519 | 622 / 680 |
| 10,000 leaves, ancestor, effects | 1 / 2 | 30.46 / 36.28 / 38.68; 24.78 / 28.51 / 30.39 | 657 / 568 | 542 / 631 |
| 10,000 leaves, independent, no effects | 1 / 2 | 54.02 / 65.63 / 70.44; 53.93 / 61.12 / 63.92 | 938 / 928 | 261 / 271 |
| 10,000 leaves, independent, effects | 1 / 2 | 87.82 / 94.23 / 103.26; 42.89 / 45.17 / 67.21 | 990 / 899 | 209 / 300 |

Each row/run has 1,200 target ticks. Superseded and consumed-late ticks are
disjoint; their sum is `unsatisfied_ticks`. All 10,000-node 120 Hz cases have
1,199 unsatisfied consecutive ticks after the equal initial sample. Percentiles
cover consumed samples only and therefore must be read with those skip counts.
The 1,000-leaf 60 Hz cases have no missed or superseded ticks in either process.
The 10,000-node cases also fail sustained 60 Hz; see raw summaries for all tails.

For the 9.255 ms peak (run 2, effect case, tick 675), the source-to-owner
interval was 0.468 ms, commit 0.021 ms, publication 0.399 ms and submission
0.973 ms. The remaining roughly 7.394 ms is the synchronous GPU completion
wait/collection interval, which can include driver or owner scheduling; no GPU
timestamp query establishes its cause. It is not evidence that CPU publication
caused this particular spike. Conversely, 10,000-node ancestor/effects in run 2
spends publication p50 8.821 ms and submit p50 6.283 ms, already above 120 Hz's
budget without native presentation. Dense independent/effects spends commit
p50 5.044 ms, publication 14.829 ms and submit 13.183 ms. Faster GPU hardware
alone cannot eliminate those owner costs.

This is conservative synchronous completion, not pipelined throughput. It
includes synthetic-source OS scheduling, not physical input. It excludes normal
window scene paint, native surface export, Present, compositor queue and scanout.
It therefore supplies a real full retained update chain through GPU completion,
not complete input-to-display latency or visible 120 fps.

## Native receipt qualification

The Windows backend now exposes an optional present-attempt receipt with native
status, device epoch, sequence and host call start/return. The native diagnostic
explicitly presents its real D3D11 scene and checks normal native acceptance.
The hidden qualification observed S_OK, epoch 1, sequence 1, 4.168 ms Present
call time. It still reports `display_confirmed=false`, `visible=false` and
`os_present_latency_measured=false`. Its 97,792 fixture pixels, cached replay
and 3,072 device-replacement pixels passed. This one call is API qualification,
not a statistically useful performance measurement or visible-window result.
Compact evidence is in `native-receipt.json`; full local artifacts are at
`target/native-present-receipt-1`.

## API conclusions and open work

The [frame budget ADR](../../ADR_RETAINED_FRAME_BUDGET.md) records the decisions.
Publication still expands world geometry for every moved descendant. Merely
reducing GPU bytes or raising GPU power cannot remove that CPU work. A local-first
packet/resolved-geometry API is the next architectural bottleneck. Effect cache
placement is also world-based: moving unchanged content invalidates each layer.
Those are higher priority than stabilizing current resolved-item interfaces.

Keep animation sampling, lossless business state/lifecycle, bounded in-flight
work and native/display receipts distinct. Software mode must prioritize
responsiveness under explicit decorative-effect permissions, retaining semantic
opacity/clip/text/hit behavior. No actual software renderer exists in this stage.
Modern desktop GPU qualification, native sustained scene-to-receipt correlation,
display feedback, CPU raster/fidelity tests and remaining RFC scope stay open.

## Regression checks

The retained hardware matrix passed 1,939 scenarios / 1,849 pixel comparisons,
including images, text, effects, property inheritance, slot/resource lifetime
and full-reference recovery. Full raw local output is
`target/retained-dense-regression`; compact facts and binary identity are in
`regression.json`. Core tests cover dense insert/update/remove batches and old
snapshots, combined nested properties versus reextraction, rational clock drift
and rounded boundary/overflow handling. Core/benchmark/native-example Clippy
passed with `-D warnings`.
The final combined run passed 140 core tests and 11 shared-example tests;
the separately invoked hardware normal-click test also passed its full pixel
comparison. The ordinary suite leaves that hardware case ignored by default.
