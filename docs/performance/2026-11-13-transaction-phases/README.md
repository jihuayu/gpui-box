# Attribute source, sink and atomic property commit work

Status: diagnostic attribution, no new runtime speedup. Full rendering goal active.

## Measurement contract

Extend the existing opt-in retained-diagnostics feature with transaction spans:
source write/computed settlement; sink collection; tree commit; source publication
and synchronous effects. Nest two property spans within ordinary UiTree::commit:
stable sort/grouped preflight and revision/application. Application includes
consuming/dropping the complete staged change list; sink collection includes
binding lookup, typed writer invocation and Patch vector growth. These are wall
times, not pure CPU cycles or a decomposition of allocator/cache/dispatch cost.

Only ordinary ReactiveUi::transaction populates the four spans. Unified edit
and internal publish helpers return None; structural/mixed property helpers do
not pretend to measure ordinary commit. Empty property commits record preflight
and zero application. Invalid attempts return their existing error without a
timing receipt. Default builds compile out fields and clocks. No per-node timer,
new dispatch strategy or semantic scheduling change is introduced.

The pipeline stores a separate transaction_diagnostics object for every consumed
source tick, including ticks that submit no GPU work. It includes sink/source/
node/computation/effect counts. Serialization occurs after the outer commit
timestamp and before publication, so its allocation/formatting overhead is in
the diagnostic publication span. Existing GPU/material diagnostic clocks and
explicit cleanup remain enabled. The diagnostic feature changes code layout,
result size and scheduling; neither control differences nor publication overhead
can establish renderer speedup or a constant observer correction.

analyze-transaction-phases.py first runs strict full pipeline validation, then
checks nonnegative spans, property sum<=tree span<=reactive sum<=outer commit,
ordinary/dense sample completeness, sink/changed-node agreement and>=10k sinks
at all19dense epochs. Residuals retain delivery/locking/outside-call costs. It
stores same-row worst commits and p50/p95/p99/max by ordinary/dense/all groups.
Never sum independent medians or percentiles into a synthetic frame.

## Reversed sustained study

Exact ordinarye8e8f0b runtime versus instrumented runtime, original/instrumented
then instrumented/original. The baseline runtime is unchanged fromc0a8485.
10k leaves, ten isolated effects, ancestor motion and dense color epochs every
half second;800x600 scale1,60/120Hz, ten seconds/case and admission after/before
construction. All16cases /14,400scheduled ticks preserve19dense epochs/case,
fresh final pixels, retired leases,<=2GPU submissions and0post-initial full
resyncs. Four250ms samplers find no known compiler/renderer conflict and all
source/binary hashes are stable. All three pipeline/transaction/GPU-phase
analyzers pass. No outlier is removed.

Dense120Hz medians, milliseconds:

| Round / admission | Source settlement | Sink collection | Property preflight | Property application | Source publication/effects | Outer commit |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
|1 /after |0.0238 |2.0006 |1.6105 |1.2827 |0.0049 |4.9264 |
|1 /before |0.0250 |2.1539 |1.5820 |1.2415 |0.0052 |4.9068 |
|2 /after |0.0276 |2.1120 |1.8193 |1.1002 |0.0059 |4.8242 |
|2 /before |0.0252 |1.8719 |1.4523 |1.0656 |0.0051 |4.6785 |

Across60/120Hz, sink medians1.855–2.154ms, preflight1.452–1.819ms and
application1.066–1.283ms dominate source settlement0.024–0.030ms and final
publication/effects0.004–0.006ms. This fixture has zero postcommit effects; the
last column cannot predict cost with application callbacks. It uses one shared
color-derived source with10k distinct sinks, not10k independently recomputed
sources. No general graph complexity or asynchronous-effect claim follows.

Updated GPU phases independently show material preparation2.04–2.58ms and
explicit selected cleanup0.93–1.13ms in dense120Hz rows. Complete submission
still contains batch conversion, backend validation/upload and driver work.
The transaction data justify inspecting sink construction/dispatch and grouped
property representation, while preserving atomic validation and last-writer
ordering. They do not justify removing validation, splitting semantic commits,
adding caller-supplied arena IDs or offloading destruction without ownership/
retained-memory/shutdown contracts. Capacity reservation alone was independently
rejected in the preceding study.

## Observer effects and failures

| Round / admission at120Hz | Control → diagnostic completion p99 / max ms | Unsatisfied ticks | Longest unsatisfied ticks |
| --- | ---: | ---: | ---: |
|1 /after |17.75 /22.96 →17.47 /21.07 |45 →56 |3 →4 |
|1 /before |17.18 /19.20 →17.99 /23.78 |43 →41 |2 →2 |
|2 /after |18.00 /22.36 →18.27 /27.16 |42 →58 |2 →3 |
|2 /before |17.20 /20.37 →17.37 /21.06 |46 →43 |3 →3 |

Three120Hz p99/max pairs worsen; both after-work loss totals and longest
intervals worsen. Keep these results, not a speed claim from nested spans.
The completion values remain coalesced native-wait upper bounds, not GPU
execution, native present/display or physical input. Smooth120Hz is unachieved.

## Validation and artifacts

167diagnostic/default core tests, strict diagnostic/default library/harness
Clippy and117mixed hardware comparisons in both configurations pass, recorded
in validation.json. The full2060case hardware suite was not repeated for this
compile-time-only timing addition. Fields are populated only at the measured
entry points and never become authoritative update policy.

evidence.zip contains raw rows, three analyzer summaries, hashes, process samples,
commands and mixed validation. changed-sources.zip contains exact baseline/final
sources, analyzer and patch. Owning Rust source bytes do not change after capture.
Instrumented executables remain in target/transaction-phases-instrumented;
ordinary target/release executables are rebuilt after capture.

Ryzen4800H / GTX1650Ti Vulkan is a regression host, not modern desktop GPU
qualification. Process samples cannot establish OS/power/thermal/driver isolation.
Native CPU fallback responsiveness, semantic/resource completeness, display/input,
large/resource-pressure/cold scenarios and cross-platform acceptance remain open.

```powershell
cargo +1.98.1 build --release --locked --offline -p gpui-box-rendering-perf --features retained-diagnostics --bin dense-runtime-perf --bin retained-runtime-gpu
python tooling/rendering-perf/capture-burst.py target/transactions-new --baseline target/transaction-phases-baseline/dense-runtime-perf.exe --candidate-name instrumented --pipelined
python tooling/rendering-perf/analyze-transaction-phases.py target/transactions-new
python tooling/rendering-perf/analyze-submit-phases.py target/transactions-new
```
