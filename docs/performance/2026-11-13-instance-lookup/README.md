# Rejected identity lookup container probe

Status: rejected. Runtime restored to598f0eb; overall rendering goal active.

## Hypothesis and invariant

Quad/Shadow allocation maps identify nodes; explicit immutable paint order
determines rendering order. Replace those two BTreeMaps with standard HashMaps,
sorting retirement IDs on full recovery to preserve the original slot reuse
order. Keep the standard hasher, generation checks, packet provenance, complete
preflight, full recovery and all other maps unchanged. No workload detection,
application knob or public API is added.

This reduces asymptotic lookup depth, but hashing a24-byte identity, random
access and changed code layout can offset that benefit. Correctness and actual
performance must be measured independently.

## Reversed sustained comparison

One baseline executable and one candidate executable run original/candidate,
then candidate/original. Each process runs10k leaves, ancestor motion, isolated
effects and dense independent color updates every half second,800x600 scale1,
60/120Hz, ten seconds per case, admission after/before construction. All16cases
and14,400scheduled ticks account for19dense epochs, match fresh final pixels,
retire all leases, use<=2GPU submissions and0post-initial full resyncs.

Dense submit excludes explicit admission by subtracting it **per raw row before
taking the median**, not by subtracting independent medians. Completion is the
source-to-covering native-wait receipt upper bound, not actual GPU execution or
visible presentation.

| Round / Hz / admission | Dense submit median original → candidate ms | Completion p99 original → candidate ms | Losses original → candidate | Longest unsatisfied ticks original → candidate |
| --- | ---: | ---: | ---: | ---: |
|1 /60 /after |9.742 →9.975 |24.86 →23.52 |20 →19 |2 →1 |
|1 /60 /before |9.626 →10.100 |25.00 →23.45 |23 →22 |2 →2 |
|1 /120 /after |9.347 →9.877 |21.62 →22.02 |79 →82 |4 →3 |
|1 /120 /before |9.545 →10.885 |21.51 →24.05 |73 →80 |4 →4 |
|2 /60 /after |9.333 →10.210 |24.10 →24.54 |20 →20 |2 →2 |
|2 /60 /before |9.597 →10.702 |23.63 →24.24 |20 →19 |2 →1 |
|2 /120 /after |9.756 →10.590 |22.66 →22.81 |100 →96 |4 →5 |
|2 /120 /before |9.763 →10.374 |22.47 →22.40 |83 →84 |3 →4 |

All eight dense submit medians regress2.4–14.0%. Tails are mixed; in round2
120Hz both longest unsatisfied intervals worsen. Candidate dense owner medians
remain19.70–22.51ms versus an8.333ms120Hz budget. There is insufficient benefit
to retain this replacement. This does not prove that hashing is universally
slower or identify its causal microarchitectural cost.

## Decision and evidence

Restore both runtime files exactly. `rejected-candidate.zip` retains their exact
baseline/candidate sources and patch. `evidence.zip` includes raw rows, paired
same-row medians, analyzer output, process samples, commands, source/binary
hashes and75mixed GPU comparisons. Candidate165core tests and strict library
Clippy pass. No full hardware suite was run for this rejected change.

All four captures have stable source/binary hashes and no sampled known
compiler/renderer conflicts at250ms intervals. This Ryzen4800H / GTX1650Ti
Vulkan host is a regression host, not modern desktop qualification. OS/thermal/
power/driver isolation, native display/input and responsive native CPU fallback
remain unqualified.

Next split preparation into material, spatial and ink-bound work with bulk
timing, and attribute explicit temporary cleanup. Do not bypass validation,
introduce custom scene-specific hashing or expose implementation policies to
applications merely to improve a benchmark.

```powershell
# Restore archived candidate first; current runtime is the original implementation.
python tooling/rendering-perf/capture-burst.py target/instance-new --baseline target/instance-lookup-baseline/dense-runtime-perf.exe --candidate-name hashed --pipelined
python tooling/rendering-perf/analyze-pipeline-burst.py target/instance-new --implementation-pair --candidate-name hashed
```
