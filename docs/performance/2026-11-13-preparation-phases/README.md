# Material preparation and explicit cleanup attribution

Status: diagnostic study, no new renderer speedup. Full goal active.

## Measurement boundary

Extend the existing compile-time `retained-diagnostics` feature with three bulk
preparation spans: material visitor/validation/classification/instance preparation
(including header/resource setup); spatial application; layer records/optional
ink bounds. These nested spans exactly sum to the existing preparation span.
There are no per-node clock reads.

After the five original body spans, explicitly drop selected resource references,
owned changed resources, the resource-frame wrapper, spatial updates, the empty
index iterator and the previous local snapshot. Time their combined destruction
as `cleanup_us`. All references outlive their use; successful backend submission,
acknowledgement, current snapshot and completion leases are already established.
This is diagnostic-build-only cleanup, with no worker transfer or deferred free.
It does not individually identify which dropped object owns the cost. Failure
and complete recovery paths retain normal cleanup; recovery fields are None.

Ordinary builds contain neither the new fields nor the clocks/explicit drops.
The analyzer remains compatible with the earlier five-span captures and checks
nonnegative spans, exact preparation sums, backend nesting and body + cleanup
within the outer submit span excluding explicit admission. It reports the final
unattributed residual separately. Timing is owner wall time, not pure CPU cycles,
GPU execution, native presentation or physical input latency.

## Reversed sustained capture

598f0eb ordinary control versus the extended diagnostic build, in reversed
original/instrumented and instrumented/original order. Each process uses10k
leaves, ancestor motion, ten isolated effects, dense independent color updates
every half second,800x600 scale1,60/120Hz, ten seconds per case and admission
after/before construction. All16cases /14,400scheduled ticks account for19dense
epochs, match fresh final pixels, retire all leases, use<=2GPU submissions and
0post-initial full resyncs. All four250ms samplers observe no known compiler or
renderer conflict; source/binary hashes are stable.

Dense120Hz diagnostic medians:

| Round / admission | Material preparation ms | Spatial application ms | Layer records / bounds ms | Explicit cleanup ms | Residual after cleanup ms |
| --- | ---: | ---: | ---: | ---: | ---: |
|1 /after |4.8272 |0.0022 |0.0024 |0.9173 |0.0028 |
|1 /before |4.4907 |0.0019 |0.0019 |1.0408 |0.0021 |
|2 /after |4.9015 |0.0021 |0.0024 |0.9150 |0.0025 |
|2 /before |4.7735 |0.0020 |0.0022 |0.9922 |0.0019 |

The current pipelined workload leaves local tight bounds disabled. The third
span therefore measures layer record construction, **not a cost result for
tight ink recomputation**. The mixed hardware regression separately enables
tight bounds for correctness. The preparation bottleneck in this workload is
the material path. Its worst120Hz dense spans are5.88–8.52ms. Explicit cleanup
worst spans are1.26–1.57ms, while ordinary cleanup medians are4.3–6.6microseconds.
The previous roughly1ms outer residual is now accounted for by the selected
temporary destruction as a group; no per-object or allocator cause is proven.
Do not add the independent medians to construct a synthetic frame.

## Observer effects and acceptance failures

Instrumentation and explicit drop order can affect scheduling/code layout.
These comparisons do not establish a runtime improvement or constant observer
overhead. Preserve the losses as well as the gains:

| Round / admission at120Hz | Control → diagnostic completion p99 / max ms | Losses | Longest unsatisfied ticks |
| --- | ---: | ---: | ---: |
|1 /after |23.89 /43.93 →23.17 /27.06 |121 →106 |6 →3 |
|1 /before |21.61 /28.76 →21.86 /33.48 |93 →88 |3 →4 |
|2 /after |21.26 /26.06 →22.63 /30.20 |95 →83 |4 →6 |
|2 /before |21.27 /30.08 →21.65 /32.97 |82 →89 |4 →7 |

Diagnostic dense120Hz owner medians remain19.83–21.18ms, above8.333ms.
Round2 has longer stalls in both admission modes despite one lower loss count.
Smooth120fps is not demonstrated. This Ryzen4800H / GTX1650Ti Vulkan host does
not qualify modern desktop GPUs; process sampling does not establish power,
thermal, OS or driver isolation. Display/input and native software remain open.

## Consequences for further work

Prioritize reducing material traversal/geometry copies/instance preparation as
one general path. A profiler or sparse nested sampling should discriminate those
costs before choosing an algorithm. A container swap was independently rejected
in the [identity lookup probe](../2026-11-13-instance-lookup/README.md).

Snapshot cleanup is real owner work and must be included in frame budgets.
Blindly moving it to a worker would add retained memory, ownership, shutdown and
failure contracts. This study does not justify that API or prove which snapshot
payload is responsible. Keep exact-base authority, atomic validation, complete
recovery and completion-held ownership. CPU fallback must keep the owner runnable
and show complete output with matching interaction geometry; native integration
is still pending and is not solved by this GPU study.

165diagnostic core tests, strict diagnostic harness Clippy and75mixed hardware
comparisons pass. Historical16case diagnostic analysis still passes. Default
build/Clippy and smoke validation are recorded in `validation.json`. The full
hardware suite was not repeated for this timing-only extension. `evidence.zip`
holds raw rows, phases, commands, hashes, process samples and regressions;
`changed-sources.zip` holds exact baseline/final changed files. Diagnostic binaries
remain locally in `target/preparation-phases-instrumented/`. Normal target
binaries are rebuilt without diagnostics after capture.

```powershell
cargo +1.98.1 build --release --locked --offline -p gpui-box-rendering-perf --features retained-diagnostics --bin dense-runtime-perf --bin retained-runtime-gpu
python tooling/rendering-perf/capture-burst.py target/preparation-new --baseline target/instance-lookup-baseline/dense-runtime-perf.exe --candidate-name instrumented --pipelined
python tooling/rendering-perf/analyze-submit-phases.py target/preparation-new
```
