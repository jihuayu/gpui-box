# Order lightweight indices before material refresh

Status: general publication working-set reduction; full rendering goal active.
No public API, quality reduction or scene-specific runtime policy is added.

## Necessary work and change

The local publisher previously cloned each dirty PaintItem in journal order,
sorted the wide `(index, PaintItem)` array, and separately sorted its material
indices. The membership journal intentionally has no paint-order contract;
logical IDs and render preorder differ after reorder or reparenting.

Map membership to topology indices first, sort that lightweight array once,
then refresh/compare material and append patches and changed indices in order.
Owned page publication already requires sorted unique patches. The algorithm
eliminates the wide-record sort and duplicate changed-index sort, and accesses
old material in page order. Unchanged final material is still omitted. Sparse
work remains proportional to dirty membership, not the complete scene. Full
recovery, independent resolved subscriber journals, GPU routing/validation,
atomic publication and immutable old snapshots retain their existing contracts.

This does not eliminate necessary material clones, command creation, page
directory copying or per-item consumer work. It adds one transient index array
in place of the transient dirty NodeId array. It does not promise constant-time
owner work or a new memory bound. The complete snapshots and failed-publication
retry rules still apply. No application-selected threshold or fixture probe is
used to choose a path.

## Exact reversed synchronous pairs

Baseline is the86e3ca7 executable. The runtime change is only ordering inside
local publication. Each process runs the existing10k ancestor/effects fixture
at60/120Hz for ten seconds per case; every half second persistently updates all
leaf colors. Both binaries run in original/ordered then ordered/original order.
All8cases account for source ticks and19dense epochs and match fresh pixels.

| Round | Hz | Dense publication original → ordered median ms | Completion original → ordered p99 / max ms | Losses original → ordered | Longest unsatisfied ticks original → ordered |
| --- | ---: | ---: | ---: | ---: | ---: |
|1 |60 |6.61 →5.69 |24.90 /28.89 →23.43 /26.52 |19 →20 |1 →2 |
|1 |120 |6.44 →5.52 |22.19 /35.27 →24.75 /27.79 |106 →106 |5 →3 |
|2 |60 |6.26 →5.70 |23.87 /32.89 →23.93 /25.87 |20 →19 |2 →1 |
|2 |120 |6.46 →5.47 |23.93 /32.25 →22.86 /28.78 |95 →81 |5 →4 |

Publication medians fall9–15%; first120Hz completion p99 regresses despite
lower max, and first60Hz loses a deadline/longest interval.120Hz dense owner
medians change20.41 →20.36ms and21.51 →20.29ms. A local work reduction does
not remove the end-to-end burst bottleneck.

## Pipelined pairs

Both binaries use default independent completion servicing, at most2GPU frames
in flight,60/120Hz, ten seconds and before/after-construction admission. Four
reversed processes add16cases /14,400scheduled ticks. Every case crosses19dense
epochs, performs no full resync after initial setup, matches fresh final pixels
and retires all GPU/frame leases. Completion receipt times are coalesced upper
bounds; owner observation is measured separately.

| Round / Hz / admission | Dense publication original → ordered median ms | Completion original → ordered p99 / max ms | Completion losses original → ordered | Longest original → ordered |
| --- | ---: | ---: | ---: | ---: |
|1 /60 /after |6.54 →5.73 |24.89 /33.26 →24.38 /28.89 |21 →19 |2 →1 |
|1 /60 /before |6.07 →6.28 |24.30 /33.84 →25.22 /29.63 |21 →19 |3 →1 |
|1 /120 /after |6.36 →5.82 |23.92 /29.57 →22.38 /27.99 |112 →84 |3 →5 |
|1 /120 /before |6.29 →5.50 |23.29 /31.62 →21.95 /27.43 |86 →99 |6 →4 |
|2 /60 /after |6.97 →5.46 |32.62 /43.44 →25.01 /31.38 |27 →19 |3 →1 |
|2 /60 /before |6.25 →5.47 |24.51 /31.82 →23.20 /26.44 |21 →19 |2 →1 |
|2 /120 /after |6.41 →5.62 |24.48 /32.61 →23.13 /32.50 |95 →87 |5 →4 |
|2 /120 /before |5.83 →5.76 |22.21 /26.89 →22.59 /26.44 |82 →84 |4 →5 |

Seven publication medians improve; one60Hz median regresses3.4%.120Hz p99
loses in the last before case. Both120Hz before cases lose total completion
deadlines; first after case improves losses but worsens longest interval.
Retain all comparisons. Owner work still approaches20ms; no queue/admission
policy or120fps guarantee follows. Raw p50/p95/p99/max and same-row phase
correlations remain in the summaries. Do not sum phase percentiles.

## Verification and evidence

165core tests (retained/text/WGPU),75mixed GPU comparisons,2,018full hardware
scenarios /1,926pixel comparisons and strict library Clippy pass. The new core
regression reverses150child identities, then applies
full and sparse material changes with equal final values across page boundaries.
It checks exact predecessor/ordering, actual colors, complete resolved output
and unchanged old snapshots. Existing tests cover generation reuse, skipped
bases, hidden nodes and mixed material/placement/resource failure/reset.

Strict `--tests` Clippy fails on the pre-existing `items_after_test_module` in
`local_ink.rs`; library-only strict Clippy passes. No lint suppression or unrelated
file change was added. The133test retained-only configuration passes with the
existing unused SpatialUpdate field warning. Full-suite timing is not used for
performance comparison.

Both formal capture manifests show stable source/binary hashes and no known
compiler/renderer observations at250ms sampling. This is not host/thermal/GPU
isolation. Neither benchmark includes native visible presentation or physical
input. Ryzen4800H / GTX1650Ti Vulkan is a regression host, not modern desktop
GPU qualification. CPU native fallback remains open.

`evidence.zip` includes commands, raw rows, summaries, manifests, source hashes,
process observations and correctness validation. `changed-sources.zip` records
candidate and exact baseline versions for changed Rust/tool files. Timings from
the full correctness suite are not performance evidence.

```powershell
# Save86e3ca7 dense-runtime-perf.exe into target/publication-order-baseline first.
cargo +1.98.1 build --release --locked --offline -p gpui-box-rendering-perf --features retained-gpu --bin dense-runtime-perf --bin retained-runtime-gpu
python tooling/rendering-perf/capture-burst.py target/order-new --publication-pair
python tooling/rendering-perf/analyze-burst.py target/order-new
python tooling/rendering-perf/capture-burst.py target/order-pipeline-new --publication-pair --pipelined
python tooling/rendering-perf/analyze-pipeline-burst.py target/order-pipeline-new --implementation-pair
```

Next general work: measure and reduce remaining owner transaction/material
construction and consumer record work, preserve private validated transitions
and complete shown geometry, then qualify native display and real input.
This narrower publication improvement does not close any remaining RFC area.
