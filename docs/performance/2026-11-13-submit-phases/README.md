# Diagnostic submission phase attribution

Status: measured bottleneck attribution, not a renderer speedup. Full goal active.

## Compile-time diagnostic contract

`retained-diagnostics` is an opt-in feature propagated from the harness through
gpui-box to gpui_wgpu. Disabled builds contain no phase clock reads or timing
fields. Enabled builds add `FrameSubmission::local_phase_times` and
`QuadSubmission::phase_times`; these experimental diagnostic fields are not
rendering policy, GPU timestamps or application deadline guarantees. Failure
paths return errors without publishing partial diagnostic success. Resolved
recovery records `local_phase_times=None`; no-op updates have no GPU submission.

The local consumer's5nonoverlapping owner spans are:

| Index | Span | Work included |
| ---: | --- | --- |
|0 |provenance |stale/base/topology checks, initial owner collection and spatial update transport |
|1 |preparation |header validation, geometry visitor, per-item validation/classification, resource selection, instance preparation, spatial application and optional layer bounds |
|2 |batch |dirty range collection, Record-to-GPU conversion and image/glyph/spatial batch construction |
|3 |backend |complete call into retained WGPU backend, including its return cleanup |
|4 |ack |dirty acknowledgement, snapshot retention and owner completion collection |

The backend's12spans, in order, are primitive/spatial validation; content/layer
validation plus initial completion poll; flight/layer admission; resized target
setup; reference-plan commit plus spatial/Quad/Shadow storage/upload; dependency
invalidation/index commit; command encoder and image/glyph preparation; layer
invalidation/raster encoding; root draw encoding; encoder finish/queue submit;
completion callback/driver scheduling, leases and error check; return assembly.
Queue submit and write-buffer wall spans can include driver work. Layer encoding
is command preparation, never measured raster execution on the GPU.

The analyzer checks nested sums per raw row, requiring backend spans <= the
local backend span and local spans <= outer submit excluding explicit admission.
It computes the outer residual separately: internal instrumentation ends before
implicit local temporary destruction and function return. Do not silently assign
that residual to GPU time. Per-frame diagnostic counters correlate1.6MB /10range
Quad writes,96byte spatial changes,10k reference checks and10rasterized layers
with dense bursts. Normal source/no-op/recovery rows remain distinct.

## Reversed sustained captures

Base runtime fe138e3. One original executable and one diagnostic executable run
original/diagnostic then diagnostic/original. Each process has10k leaves with
ancestor motion and isolated effects,800x600 scale1,60/120Hz, ten seconds per
case, admission after/before construction and a persistent dense color epoch
every half second. All16cases /14,400scheduled ticks account for19epochs, match
fresh final pixels, retire all leases, use<=2GPU submissions and0post-initial
full resyncs. Four250ms process samplers report no known compiler/renderer
interference; source and binary hashes remain stable.

Dense120Hz diagnostic phase medians (19samples each):

| Round / admission | Outer submit excluding admission ms | Consumer preparation ms | Batch construction ms | Backend total ms | Storage/upload ms | Primitive/spatial validation ms | Queue submit ms | Outer residual ms |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
|1 /after |9.574 |4.598 |0.657 |2.894 |1.455 |0.727 |0.399 |0.908 |
|1 /before |9.753 |4.408 |0.666 |2.928 |1.430 |0.721 |0.415 |0.999 |
|2 /after |9.420 |4.540 |0.578 |2.749 |1.407 |0.783 |0.397 |0.942 |
|2 /before |9.606 |4.488 |0.588 |2.781 |1.411 |0.684 |0.403 |0.945 |

These independent medians must not be summed. Same-row totals are available
in `submit-phase-summary.json`. Local total medians are8.37–8.75ms; preparation
is roughly half of that work, while batch conversion is0.58–0.67ms. This supports
investigating geometry/material preparation before another upload-array probe.
The backend does real work too, but queue submit alone is not the dominant
observed owner span. It says nothing about actual GPU execution time.

Ordinary120Hz local-total medians are1.04–1.11ms; preparation is0.005–0.006ms,
batch0.004ms, backend1.02–1.10ms, primitive/spatial validation0.30–0.32ms,
storage/upload0.043–0.045ms and queue submit0.405–0.437ms. Dense and ordinary
paths therefore need different attribution, not different application policies.

All four120Hz worst-local-submit rows are dense; preparation grows5.82–6.95ms,
total local work10.17–12.66ms. The worst-completion row is not necessarily the
worst-submit row. Round2 after's35.89ms worst completion has local work9.80ms,
preparation5.32ms and backend3.22ms: source arrival, transaction/publication and
completion scheduling can dominate other parts of that same update. Raw worst
local and worst completion rows are both retained with their exact counters.

## Observer and host limitations

Do not interpret the diagnostic/control pair as an optimization. Dense outer
submit medians change−7.2% to+3.3% across cases despite adding clock reads,
fields and JSON output, demonstrating nontrivial observer/code-layout/host
variance. This experiment cannot establish a constant instrumentation overhead
or unbiased sub-millisecond gains. Diagnostic serialization happens after the
submitted timestamp but affects subsequent scheduling and receipt observation.

At120Hz, original → diagnostic completion p99/max and total losses are:

| Round / admission | Original p99 / max ms | Diagnostic p99 / max ms | Original → diagnostic losses | Original → diagnostic longest unsatisfied ticks |
| --- | ---: | ---: | ---: | ---: |
|1 /after |23.44 /29.66 |22.03 /27.98 |128 →88 |4 →3 |
|1 /before |23.36 /29.48 |23.18 /29.57 |86 →83 |4 →4 |
|2 /after |26.68 /28.22 |23.60 /35.89 |114 →82 |4 →10 |
|2 /before |21.54 /28.31 |22.17 /29.93 |82 →98 |4 →4 |

Round2 after's longest unsatisfied interval worsens4 →10 despite better p99 and
total losses. Round2 before loses p99/max/losses. These are preserved, not erased
as instrumentation noise. Owner dense medians remain19.95–20.91ms, well over
8.333ms, and neither executable establishes smooth120fps.

The completion channel covers source through coalesced native-wait receipts,
not display/input. This Ryzen4800H / GTX1650Ti Vulkan host does not qualify modern
desktop GPUs. Process sampling does not prove idle/thermal/power/driver isolation.
Physical input, visible presentation, CPU native fallback and cross-platform
acceptance remain open.

## Verification, evidence and next work

Diagnostic strict harness/library Clippy,165core tests,2harness tests and75mixed
GPU comparisons pass. Full diagnostic correctness suite passes2,018hardware
cases /1,926pixel comparisons. This suite's timings are not performance data.
Default-feature build/Clippy and smoke evidence are recorded separately.

`evidence.zip` retains raw phases, summaries, original commands, process samples,
manifests/source hashes and validation. `changed-sources.zip` contains exact
baseline/final changed files. Original and diagnostic binaries are retained
locally at `target/submit-phase-control/` and `target/submit-phase-instrumented/`.
The normal target executable is rebuilt without diagnostics after capture.

Next: split preparation with sparse sampling or an external CPU profiler, then
test a general reduction of repeated geometry/material work. Explicitly attribute
post-body temporary destruction before moving it or claiming worker ownership.
Retain atomic validation, exact predecessors and full recovery; never remove
preflight or let applications supply unchecked dirty authority to meet a number.

```powershell
# Save default fe138e3 executable under target/submit-phase-control first.
cargo +1.98.1 build --release --locked --offline -p gpui-box-rendering-perf --features retained-diagnostics --bin dense-runtime-perf --bin retained-runtime-gpu
python tooling/rendering-perf/capture-burst.py target/phases-new --baseline target/submit-phase-control/dense-runtime-perf.exe --candidate-name instrumented --pipelined
python tooling/rendering-perf/analyze-submit-phases.py target/phases-new
python tooling/rendering-perf/analyze-pipeline-burst.py target/phases-new --implementation-pair --candidate-name instrumented
# Restore normal non-instrumented target binaries afterward.
cargo +1.98.1 build --release --locked --offline -p gpui-box-rendering-perf --features retained-gpu --bin dense-runtime-perf --bin retained-runtime-gpu
```
