# Consumer-owned local instance handle index

Status: retained general work reduction; sustained smooth120Hz remains unachieved.
The complete RFC, native CPU fallback and modern desktop acceptance remain active.

## Principle and lifetime

For an exact private LocalFrame predecessor with unchanged painter topology,
Quad/Shadow allocations do not change. The consumer already has authoritative
paint indices, but previously recovered the same instance vectors through
per-node BTreeMap entries on every changed material. Retain their actual handles
in a flat consumer-owned vector, with two ranges per paint item. Rebuild after
successful complete local recovery from the installed allocation maps. Keep the
original maps and full preparation path for recovery and resolved submissions.

Reuse retains all existing exact-predecessor, shared spatial/content, scale,
surface, source and painter-topology checks, and checks index extent. The local
writer checks command/shadow counts and continues calling InstanceArena::update,
which checks arena identity, generation and retirement. Material validation,
GPU projected preflight, atomic failure/reset and resource leases are unchanged.
No application dirty authority, batching mode or scene-size threshold is added.
The fast path cannot allocate/retire instances; topology/count changes recover.
Direct resolved submission clears the indices, reset/device replacement makes a
new consumer, and capability/source changes force recovery before reuse.

This trades CPU metadata for repeated identity lookup. On this64-bit target,
two usize ranges occupy32bytes per item, and each InstanceHandle24bytes. For
10k single-quad items that is about560KB logical payload, excluding the existing
spatial index, Vec capacity and allocator overhead. Multiple primitives add
24bytes each. Recovery adds O(items+handles) work and copies; clear retains Vec
capacity until consumer destruction. One index is retained per consumer, with
no per-frame clone or additional GPU lease. This is not a physical-memory bound.

## Sustained reversed pairs

Exact ordinary3228210 executable versus ordinary indexed candidate; original/
indexed then indexed/original.10k leaves, ancestor motion, ten isolated effects,
dense color epochs every half second,800x600 scale1,60/120Hz, ten seconds/case,
admission after/before construction. Tight local bounds remain disabled. Each
capture has16cases /14,400scheduled ticks,19dense epochs/case, fresh final pixel
equality, retired leases, at most two GPU submissions and zero post-initial full
resyncs. All eight process runs across both captures have stable source/binary
hashes and no known compiler/renderer conflicts sampled at250ms.

The initial capture fails the analyzer's strict receipt<=owner observation
invariant once: tick757 in round1 indexed120Hz after-work reports the covering
receipt1microsecond later than owner observation. Preserve its raw rows,
`rejected-analysis.json` and exploratory phase medians; do not widen tolerance
or use that capture as validated full-chain evidence. Cause is unestablished.
An unchanged-source/binary complete repeat passes every strict analyzer check.
Initial exploratory submit medians improve all eight pairs4.9–16.2%; this is
secondary evidence only, with mixed tails and deadline losses preserved.

Validated repeat results below subtract admission from submit **per dense row**
before taking the median. Completion values are covering native-wait upper
bounds, not GPU execution or display latency.

| Round / Hz / admission | Dense submit median original → indexed ms | Completion p99 / max original → indexed ms | Unsatisfied ticks | Longest unsatisfied ticks |
| --- | ---: | ---: | ---: | ---: |
|1 /60 /after |7.519 →7.216 |20.09 /28.68 →19.99 /23.31 |16 →14 |1 →1 |
|1 /60 /before |8.702 →7.268 |25.11 /34.02 →20.83 /25.18 |24 →19 |2 →1 |
|1 /120 /after |8.308 →6.886 |21.03 /26.21 →19.50 /24.95 |47 →44 |3 →2 |
|1 /120 /before |7.972 →7.430 |19.75 /25.86 →20.44 /24.27 |57 →45 |5 →3 |
|2 /60 /after |8.585 →6.969 |21.80 /26.94 →19.75 /23.72 |18 →18 |1 →1 |
|2 /60 /before |8.667 →7.312 |22.32 /26.04 →21.66 /24.51 |19 →19 |1 →1 |
|2 /120 /after |7.797 →7.143 |20.26 /25.04 →20.37 /22.85 |45 →49 |3 →2 |
|2 /120 /before |8.147 →7.108 |21.05 /25.63 →19.91 /27.08 |54 →43 |3 →3 |

All eight dense submit medians improve4.0–18.8%. Candidate dense owner medians
remain16.85–18.36ms versus original17.70–21.04ms; one120Hz owner median worsens
17.976→17.988ms.120Hz p95 changes6.877→7.133,7.066→7.112,7.041→7.134 and
7.068→6.885ms; two120Hz p99 values worsen, round2after-work loss total worsens
45→49, and round2before-work maximum worsens25.63→27.08ms. No stable tail or
120fps claim follows. Do not select only the winning aggregate/round.

Retain the internal index for consistent measured submit savings with explicit
metadata cost. This preserves immutable scene and complete-transaction API
contracts. It does not justify skipping validation, offloading transactions
partially, changing queue policy or simplifying effects automatically.

## Verification and evidence

The117mixed hardware comparisons include color/decoration/shadow edits, nested
effects, images/glyphs, deep/fractional placement, changed shadow count, failed
late material validation/reset and the42allocation-lifecycle pixel cases across
scales1/1.25/1.5. Lifecycle cases alternate exact edits with reorder, removal/
replacement, missed base, resolved handoff, reset and content capability toggles.
They assert recovery routing, fresh pixels and lease retirement. Full hardware
and core/lint counts are recorded in `validation.json`; correctness-run timing
is not performance evidence.

`evidence.zip` preserves initial/repeat captures, strict analysis/rejection,
raw rows, pair metrics, hashes, process samples and correctness output.
`changed-sources.zip` contains exact baseline/candidate runtime sources and diff.
The saved baseline executable remains in `target/local-allocation-baseline/`.
No owning runtime or dense harness source changed after either capture.

Ryzen4800H / GTX1650Ti Vulkan is a regression host, not modern desktop
qualification. Samples cannot rule out OS/power/thermal/driver interference.
Native present/display/physical input, larger extents/resource pressure,
responsive native CPU fallback and full semantic/resource/cross-platform
acceptance remain open. Next distinguish remaining owner construction from
completion worker/queue/GPU execution observations before choosing a remedy.

```powershell
python tooling/rendering-perf/capture-burst.py target/instances-new --baseline target/local-allocation-baseline/dense-runtime-perf.exe --candidate-name indexed --pipelined
python tooling/rendering-perf/analyze-pipeline-burst.py target/instances-new --implementation-pair --candidate-name indexed
cargo +1.98.1 build --release --locked --offline -p gpui-box-rendering-perf --features retained-gpu --bin dense-runtime-perf --bin retained-runtime-gpu
target/release/retained-runtime-gpu.exe target/instances-check --validation-only
```
