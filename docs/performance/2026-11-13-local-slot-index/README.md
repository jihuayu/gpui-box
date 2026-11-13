# Consumer-owned local spatial slot index

Status: retained reduction of repeated identity lookup; smooth120Hz unachieved.
Full RFC, modern desktop, native presentation/input and software goal active.

## Basis and lifetime

The [callback sampling study](../2026-11-13-material-sampling/README.md) identifies
spatial lookup and instance preparation as larger sampled callback spans. The
local consumer already knows exact snapshot ancestry and immutable paint order,
but rehashes every material node identity to recover an unchanged spatial slot.
Retain the actual slots in a consumer-owned Vec<u32> in submitted paint order.
After complete local recovery, build it from the installed SpatialArena. On an
exact-predecessor local material update, index this vector with the authoritative
private material index. No mutable owner state or application dirty authority
crosses the consumer boundary. Standard maps and hashers remain unchanged.

This is private derived allocation metadata, not another scene representation
or application API. Slot lookup is checked when the vector is built; a missing
allocation poisons the consumer. Reuse requires existing shared-spatial/content,
surface/scale/source/exact-predecessor checks, unchanged draw topology and a
matching vector extent. LocalFrame fields are private and its predecessor proof
preserves full topology identity, not merely length/revision. Therefore removal,
reparent/reorder, viewport/layout/full publication, missed base, reset/device
replacement or resolved handoff cannot reuse another topology's slots. A direct
resolved submit clears the vector before rebuilding allocations. Reset creates
a fresh consumer. Capability changes invalidate source and force recovery.

Spatial-only updates mutate values, not slot allocation. Instance generations,
arena checks, material validation and projected GPU preflight remain unchanged.
Existing exact local failure/reset semantics remain in force. Cache construction
adds O(N) lookups to recovery and4Nbytes (about40KB for10k nodes), excluding Vec
capacity/allocator overhead. Only one consumer index is kept; no per-frame clone
or new in-flight lease is introduced. There is no new physical-memory guarantee.

## Reversed sustained evidence

Ordinary f828eae executable versus ordinary candidate; original/indexed then
indexed/original.10k leaves, ancestor motion, ten isolated effects, dense color
epochs every half second,800x600 scale1,60/120Hz, ten seconds/case, admission
after/before construction. Tight local bounds remain disabled. All16cases /
14,400scheduled ticks account for19dense epochs, match fresh final pixels,
retire leases, use<=2GPU submissions and0post-initial full resyncs. Four250ms
samplers observe no known compiler/renderer conflicts; hashes remain stable.

Dense submit excludes explicit admission per raw row before taking its median:

| Round / Hz / admission | Dense submit median original → indexed ms | Completion p99 / max original → indexed ms | Unsatisfied ticks | Longest unsatisfied ticks |
| --- | ---: | ---: | ---: | ---: |
|1 /60 /after |9.518 →8.808 |23.64 /28.76 →24.01 /34.12 |19 →20 |1 →2 |
|1 /60 /before |9.367 →7.826 |23.06 /28.34 →21.65 /26.69 |19 →19 |1 →1 |
|1 /120 /after |9.326 →7.736 |22.10 /29.10 →20.92 /27.13 |102 →91 |4 →23 |
|1 /120 /before |9.977 →8.144 |22.22 /27.71 →20.00 /25.40 |91 →86 |5 →3 |
|2 /60 /after |9.762 →7.833 |22.27 /25.99 →21.54 /31.55 |19 →20 |1 →1 |
|2 /60 /before |9.742 →8.240 |24.28 /28.13 →21.52 /24.88 |20 →19 |2 →1 |
|2 /120 /after |9.737 →8.437 |23.98 /31.89 →22.83 /31.49 |85 →90 |3 →8 |
|2 /120 /before |9.444 →8.665 |22.02 /28.32 →20.90 /29.27 |81 →99 |4 →4 |

All eight dense submit medians improve7.5–19.8%, a consistent measured reduction
on this host. All four120Hz p99 values improve, but p95 changes9.18→9.41,
8.75→8.50,9.17→9.07 and8.59→8.96ms. Both120Hz after-work longest intervals
worsen, and both round2 deadline totals worsen. Indexed dense owner medians remain
18.41–20.41ms across cases, above8.333ms. No stable stall reduction or120fps
claim follows from better submission medians.

The worst interval's23rows (ticks330–352) are all ordinary, with no dense epoch
crossed or superseded source ticks. Submit spans are about0.98–2.54ms but receipt
upper bounds9.41–16.67ms. Preserve these rows separately; this observation does
not establish GPU execution time, driver/completion scheduling cause or a
193ms visible freeze. It is23consecutive source deadlines unfulfilled by the
covering completion observations, not absence of all completed/displayed work.

## Decision, regression coverage and limits

Retain the small general consumer index because it removes a redundant lookup
with consistent submit improvement, while explicitly retaining tail losses and
the unfinished frame budget. Do not introduce an application policy, tune by
scene size or weaken atomic validation to improve the result.

166core tests and strict library/hardware-harness Clippy pass. Existing75mixed
GPU cases cover fractional scale, nested effects/deep anchors, changed materials,
skipped bases, reset, invalid glyph scale and resolved/local handoff. Add42pixel
cases across scales1/1.25/1.5 exercising same-count painter reorder, removal and
replacement, skipped material base, resolved handoff, reset and capability toggle,
each followed by exact-predecessor material edits. They assert recovery routing,
fresh final pixels and fully retired leases. The full suite is recorded in
`validation.json`; its timings are not performance evidence.

The postcapture Rust change only adds those hardware regression cases; runtime
and dense harness bytes retain the formal capture hashes. `changed-sources.zip`
holds exact baseline/final files, `evidence.zip` raw data, phase/deadline results,
source/binary hashes, process samples, worst interval and correctness results.
Local original executable remains in `target/local-slot-baseline/`.

Hardware Ryzen4800H / GTX1650Ti Vulkan is a regression host, not modern desktop
qualification. Sampling excludes OS/power/thermal/driver isolation; completion
receipts are source-to-covering native-wait upper bounds, not actual GPU execution,
physical input or visible presentation. CPU native integration and complete
fidelity/semantic/resource/cross-platform acceptance remain open. Next investigate
remaining instance lookup/preparation and completion tail behavior independently.

```powershell
python tooling/rendering-perf/capture-burst.py target/slots-new --baseline target/local-slot-baseline/dense-runtime-perf.exe --candidate-name indexed --pipelined
python tooling/rendering-perf/analyze-pipeline-burst.py target/slots-new --implementation-pair --candidate-name indexed
cargo +1.98.1 build --release --locked --offline -p gpui-box-rendering-perf --features retained-gpu --bin dense-runtime-perf --bin retained-runtime-gpu
target/release/retained-runtime-gpu.exe target/slots-check --validation-only
```
