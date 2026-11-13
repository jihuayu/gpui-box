# Sparse material callback timing

Status: bottleneck investigation, not a runtime speedup. Full goal active.

## Sampling contract

`retained-diagnostics` samples material indices where index %64 equals
revision %64. It records five nonoverlapping callback spans: item validation;
primitive classification; node-to-spatial-slot lookup; primitive preparation;
resource classification/selection and counters. Other material callbacks retain
the normal path with no clock reads. Ordinary builds compile out all sampling
fields, branches and clock reads. Resolved recovery has None; no-op has no
submission. Failure returns no partial successful timing record.

Each successful local submission carries raw sample totals and count plus total
validated material count. These are samples of actual callbacks, not whole-frame
estimates. Timings include observer overhead, cache/code-layout effects, allocator
or OS scheduling delays. Do not multiply by64 to claim phase costs, derive pure
CPU cycles or subtract these totals from the full material span to assign the
remainder to geometry. Sampling can miss work and change the code it observes.

The analyzer checks nonnegative totals/counts, zero totals for zero samples,
sample totals within the bulk material span and existing backend/local/outer
nesting. Historical captures without sample fields remain supported. Existing
source/tick/epoch/pixel/lease/admission/recovery accounting remains in force.

## Reversed sustained capture

Corrected baseline a4df7c6, unchanged by documentation commit6add252, versus
diagnostic sampling. Original/instrumented then instrumented/original. Each
process runs10k leaves, ancestor motion, ten isolated effects and dense color
epochs every half second,800x600 scale1,60/120Hz, ten seconds per case, admission
after/before construction. Tight local bounds remain disabled in this workload.
All16cases /14,400scheduled ticks account for19dense epochs, match final fresh
pixels, retire leases, use<=2GPU submissions and0post-initial full resyncs.
Source/binary hashes are stable; four250ms samplers observe no known compiler
or renderer conflict. They do not establish OS/GPU/power/thermal isolation.

Each dense frame samples155–157material callbacks; each diagnostic case records
2,969callbacks across its19dense epochs. For readability the table gives the
median **per-frame sample average** in nanoseconds, including clock overhead:

| Round / Hz / admission | Validation ns | Classification ns | Spatial lookup ns | Primitive preparation ns | Content/counters ns |
| --- | ---: | ---: | ---: | ---: | ---: |
|1 /60 /after |85.2 |53.8 |151.3 |185.4 |38.9 |
|1 /60 /before |85.3 |49.7 |152.6 |181.4 |39.7 |
|1 /120 /after |85.9 |51.3 |153.8 |190.4 |37.8 |
|1 /120 /before |87.9 |58.0 |172.9 |211.5 |42.0 |
|2 /60 /after |79.5 |53.2 |160.3 |191.0 |40.4 |
|2 /60 /before |80.8 |53.2 |165.6 |191.7 |36.8 |
|2 /120 /after |79.5 |54.8 |152.3 |185.2 |38.5 |
|2 /120 /before |80.3 |49.7 |158.0 |181.5 |38.2 |

Primitive preparation and spatial lookup are the larger sampled callback spans;
classification/content are smaller. Full dense120Hz material spans remain
4.65–5.48ms. This supports investigating repeated identity→allocation lookup and
instance preparation, rather than assuming cloning/world geometry dominates.
It does not establish a precise cost ranking free of observer effects, nor
justify bypassing item validation. The earlier standard HashMap replacement
and borrowed geometry representation were rejected independently.

## Tail evidence and limits

120Hz completion upper bounds, original → diagnostic:

| Round / admission | p95 ms | p99 ms | max ms | Unsatisfied ticks | Longest unsatisfied ticks |
| --- | ---: | ---: | ---: | ---: | ---: |
|1 /after |8.40 →8.72 |22.17 →22.15 |25.01 →27.40 |82 →83 |4 →4 |
|1 /before |8.81 →8.82 |21.87 →23.87 |32.06 →32.88 |89 →92 |5 →5 |
|2 /after |9.39 →9.49 |22.51 →21.44 |31.05 →30.54 |115 →108 |4 →4 |
|2 /before |9.51 →8.42 |22.67 →21.71 |46.14 →31.69 |132 →82 |5 →4 |

Retain worse p95/p99/max and deadline results; this is not an optimization
comparison or proof of a constant diagnostic overhead. Receipts cover source
through coalesced native-wait completion observations, not actual GPU execution,
visible presentation or physical input. This Ryzen4800H / GTX1650Ti Vulkan host
is not modern desktop qualification.120fps with effects remains unachieved.

## API consequence and validation

Exact predecessor and stable topology may support consumer-owned indexed
allocation metadata, avoiding repeated identity lookup while preserving every
generation/arena check and recovery. This is an investigation direction, not
an implemented guarantee or new application API. Cache lifetime must match the
actual submitted topology; skipped bases, removal/reuse, scale/device reset and
resolved/local handoff must invalidate or rebuild it. Do not introduce unchecked
dirty authority or application-chosen map/hash policies.

166diagnostic core tests, strict diagnostic library/harness Clippy,2harness tests
and75mixed GPU comparisons pass. Ordinary build and smoke checks are recorded
in `validation.json`. Full GPU suite was not repeated for this timing-only
extension; a4df7c6 already passed2,018hardware /1,926pixel checks. Raw data,
commands, manifests, process samples and distributions are in `evidence.zip`;
exact changed sources in `changed-sources.zip`. Diagnostic binaries remain at
`target/material-sampling-instrumented/`; default binaries are rebuilt without
diagnostics after capture. Native CPU integration, richer primitive/semantic
contracts and complete RFC acceptance remain open.

```powershell
cargo +1.98.1 build --release --locked --offline -p gpui-box-rendering-perf --features retained-diagnostics --bin dense-runtime-perf --bin retained-runtime-gpu
python tooling/rendering-perf/capture-burst.py target/sampling-new --baseline target/material-sampling-baseline/dense-runtime-perf.exe --candidate-name instrumented --pipelined
python tooling/rendering-perf/analyze-submit-phases.py target/sampling-new
```
