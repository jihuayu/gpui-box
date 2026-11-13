# Refresh immutable material pages in bounded payload batches

Status: retained general publication optimization; full RFC and smooth120Hz
acceptance remain active. No scene-specific mode or caller batching authority.

## Principle and implementation

Local publication previously cloned/refreshed every dirty PaintItem into one
wide transient array, then transferred those items into64-item immutable pages.
Dirty indices were already sorted in paint order. Refresh, compare and install
one page at a time instead, retaining at most64transient material payloads.
Final changed indices remain sorted; equal final materials are omitted. Fully
changed pages move their items, partially changed pages clone the old page once,
and untouched pages retain their Arc identity. Existing hit bounds are patched
after every page succeeds. The resolved publisher and original patch_owned
path remain unchanged.

The new root stays private until the complete operation succeeds. A late error
drops all private replacement pages; the old snapshot and dirty journals remain
available for retry. Material refresh may populate existing node/text caches,
as before; it does not commit another semantic transaction. Placement publication
still follows successful material refresh. Old in-flight snapshots retain their
pages/resources, and no predecessor roots are newly chained.

The change removes O(changed items) wide temporary payload storage, not the
required complete output pages. Changed indices, directory and dirty-page
metadata remain proportional to membership/pages. Each touched page allocates a
small temporary vector; allocator/scheduling/page-fault/destruction costs are
not bounded by the64-item logical batch. No worker transfer, validation bypass,
partial transaction, application threshold or new frame API is introduced.
This producer optimization is independent of GPU/CPU consumer choice, but these
measurements do not establish native software responsiveness.

## Reversed sustained evidence

Exact7bac8a3 ordinary executable versus ordinary candidate, original/paged then
paged/original.10k leaves with ten isolated effects and ancestor motion, dense
independent color epochs every half second,800x600 scale1,60/120Hz and admission
after/before construction. Ten seconds/case;16cases /14,400scheduled ticks.
Every case crosses all19dense epochs, matches fresh final pixels, retires leases,
uses at most2GPU submissions and has0post-initial full resyncs. All four runs
have stable source/binary hashes and no known compiler/renderer conflicts in
250ms samples. Strict same-row timing and tick accounting checks pass.

| Round / Hz / admission | Dense publication median original → paged ms | Completion p99 / max original → paged ms | Unsatisfied ticks | Longest unsatisfied ticks |
| --- | ---: | ---: | ---: | ---: |
|1 /60 /after |5.547 →3.478 |20.53 /25.43 →19.13 /24.08 |18 →11 |1 →1 |
|1 /60 /before |5.853 →3.553 |20.81 /25.54 →19.30 /23.70 |19 →17 |1 →1 |
|1 /120 /after |6.712 →3.251 |21.68 /43.95 →17.64 /20.54 |74 →43 |5 →2 |
|1 /120 /before |5.691 →3.062 |19.41 /22.16 →17.23 /28.32 |45 →44 |3 →4 |
|2 /60 /after |5.838 →3.173 |20.28 /24.68 →18.26 /22.17 |18 →15 |1 →1 |
|2 /60 /before |5.673 →3.084 |20.05 /21.55 →17.88 /18.87 |19 →17 |1 →1 |
|2 /120 /after |5.742 →3.280 |19.63 /21.24 →18.04 /22.20 |48 →45 |3 →3 |
|2 /120 /before |6.243 →3.447 |20.32 /23.71 →17.22 /20.43 |45 →60 |2 →6 |

All eight dense publication medians improve37.3–51.6%, all eight completion p99
values improve, and dense owner medians fall16.73–20.07→15.01–15.78ms. The
first120Hz after-work baseline is especially slow; the reverse pairs still show
publication gains. These are owner wall-time observations, not pure CPU cycles
or causal microarchitectural attribution. Do not sum independent percentiles.

Failures remain:120Hz p95 changes7.759→7.426,7.063→7.534,6.839→7.530 and
7.224→7.994ms. First before-work max and longest interval worsen; second after
maximum worsens. Second before-work deadline total worsens45→60 and longest
interval2→6ticks. Its worst six rows (ticks187–192) are ordinary, not dense,
with no skipped source ticks and completion upper bounds8.99–11.17ms. They
do not prove a50ms visible freeze or identify GPU/driver/OS cause. Preserve
those exact rows in `longest-interval-rows.json`.

Retain the general working-set reduction because publication gains are
consistent and material temporary lifetime is simpler. No stable stall remedy
or120fps qualification follows: dense owner work still exceeds8.333ms.

## Correctness and reproducibility

New core coverage compares sparse cross-page refresh against existing owned
patching, includes unchanged entries and existing hit bounds, preserves old
material after a failure on a later page, and checks no-change page sharing.
Existing local tests cover material equality across multiple commits, immutable
snapshot history, alternating resolved/local publication and topology recovery.
The117mixed hardware comparisons exercise effects, material/shadow changes,
resources, fractional/deep placement and42allocation-lifecycle transitions.
Full hardware/core/lint results are recorded in `validation.json`; correctness
run timing is not performance evidence.

`changed-sources.zip` stores exact baseline/candidate runtime/test sources and
patch. `evidence.zip` stores raw captures, strict summary, phase pairs, worst
interval, process samples, hashes and correctness output. The saved baseline is
`target/page-stream-baseline/dense-runtime-perf.exe`. Owning Rust source bytes
remain unchanged after capture. No notion document or remote branch is updated.

Ryzen4800H / GTX1650Ti Vulkan is a regression host, not modern desktop
qualification. Completion receipts are coalesced native-wait upper bounds,
not GPU execution, native present/display or physical input latency. OS/power/
thermal/driver isolation, resource-pressure/large-extents/cold spikes, complete
native CPU fallback and full semantic/resource/cross-platform acceptance remain
open. Continue distinguishing owner necessary work from completion-tail causes.

```powershell
python tooling/rendering-perf/capture-burst.py target/pages-new --baseline target/page-stream-baseline/dense-runtime-perf.exe --candidate-name paged --pipelined
python tooling/rendering-perf/analyze-pipeline-burst.py target/pages-new --implementation-pair --candidate-name paged
cargo +1.98.1 build --release --locked --offline -p gpui-box-rendering-perf --features retained-gpu --bin dense-runtime-perf --bin retained-runtime-gpu
target/release/retained-runtime-gpu.exe target/pages-check --validation-only
```
