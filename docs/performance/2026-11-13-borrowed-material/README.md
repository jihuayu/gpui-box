# Rejected borrowed material geometry probe

Status: rejected candidate; retain the independent ancestry correctness fix.
Full rendering/API/CPU/native acceptance goal remains active.

## Principle and implementation

The material consumer resolves each changed PaintItem by cloning its shared
commands/shadows/resources, updates only geometry, then validates/classifies and
prepares instances before dropping that transient item. The candidate borrows
the immutable material and resolves geometry into an internal view, retaining
owned items only for image/glyph resource updates. Shared Quad/Shadow builders
and validation accept that geometry view. Public snapshots retain full ownership,
same-base authority, projected GPU preflight, error/reset behavior and recovery.
There is no custom workload branch, changed quality, unsafe code or public knob.

This removes Arc increment/decrement traffic but introduces a second internal
geometry representation and adaptation of validation/builders. Less nominal
work must still justify its abstraction with measured latency results.

## Independent defect found before capture

A new range-vs-fresh-extraction regression found that the existing geometry
visitor retained only its starting parent's context. A range starting deep in
one branch could cross to a shallower sibling and incorrectly reuse that deep
context, corrupting resolved position/clip/property inheritance. Commit a4df7c6
retains all open ancestors in the traversal stack and pops to each actual parent.
Every possible range start in a deep mixed translation/clip/opacity fixture now
matches fresh extraction.166core tests, strict library Clippy and75mixed GPU
comparisons passed for this independent fix.

The candidate and baseline both include that fix. The corrected baseline binary
was rebuilt/saved before restoring the candidate. Rejection restores a4df7c6,
not the incorrect traversal. The borrowed-ownership regression additionally
checks payload identity/refcounts during visits, owned geometry against a fresh
extractor across deep/sibling ranges and unchanged old snapshots.

## Reversed sustained comparison

Original/candidate then candidate/original,10k leaves, ancestor motion, ten
isolated effects and dense independent color epochs every half second. Each
process runs800x600 scale1,60/120Hz, ten seconds per case, admission after/before
construction. All16cases /14,400scheduled ticks account for19dense epochs,
match fresh final pixels, retire leases, use<=2GPU submissions and0post-initial
full resyncs. Source/binary hashes remain stable; all four250ms process samplers
report no known compiler/renderer interference.

Dense submit subtracts explicit admission per raw row before taking its median.
Completion is the source-to-covering native-wait receipt upper bound, not GPU
execution, native presentation or physical input latency.

| Round / Hz / admission | Dense submit median original → candidate ms | Completion p99 / max original → candidate ms | Losses | Longest unsatisfied ticks |
| --- | ---: | ---: | ---: | ---: |
|1 /60 /after |9.652 →9.606 |23.86 /31.37 →23.70 /30.02 |20 →20 |2 →2 |
|1 /60 /before |9.528 →8.807 |25.53 /29.24 →22.71 /28.63 |20 →19 |2 →1 |
|1 /120 /after |9.288 →9.132 |21.94 /27.77 →23.90 /32.34 |87 →98 |4 →13 |
|1 /120 /before |9.292 →9.332 |21.60 /26.73 →21.94 /25.81 |77 →89 |4 →3 |
|2 /60 /after |9.184 →9.111 |22.49 /27.34 →25.40 /42.43 |19 →20 |1 →2 |
|2 /60 /before |9.507 →10.625 |22.76 /28.69 →28.48 /45.98 |19 →23 |1 →2 |
|2 /120 /after |9.546 →9.139 |22.43 /26.20 →22.68 /28.19 |92 →93 |5 →3 |
|2 /120 /before |10.023 →9.165 |23.64 /31.18 →21.28 /28.17 |95 →103 |7 →3 |

Six submit medians improve and two regress, spanning−8.6% to+11.8%. The60Hz
before-work improvement reverses in round2. All four120Hz comparisons lose total
completion deadlines; first120Hz after-work longest unsatisfied interval grows
4→13ticks. Same-row owner/phase and full p95/p99/max distributions are archived.
This is insufficient evidence for stable stall improvement. Host/scheduling/
thermal/driver variance prevents attributing each tail to the implementation,
but does not justify discarding adverse results or claiming a proven gain.

## Decision and evidence

Restore the six candidate runtime/test files to a4df7c6. Do not retain the extra
geometry representation, switch or a second application-selected path. The
exact candidate, baseline and patch are in `rejected-candidate.zip`; raw data,
manifests, source hashes, paired medians and validation are in `evidence.zip`.
Candidate167core tests, strict library Clippy and75mixed GPU comparisons pass.
Correctness does not decide rejection; measured benefit is insufficient.

Candidate binaries remain in `target/borrowed-material-rejected-binaries/`,
corrected baseline in `target/borrowed-material-baseline/`. Default restored
binaries are rebuilt and the full GPU lifecycle/correctness suite is recorded
separately in `validation.json`; suite timings are not performance evidence.

This Ryzen4800H / GTX1650Ti Vulkan host is not modern desktop qualification.
Process sampling is not OS/power/thermal/driver isolation.120fps under effects,
physical input/display and responsive native software fallback remain open.
Next use sampled profiling to distinguish geometry resolution, validation,
classification and instance preparation before choosing another abstraction.
Preserve atomic validation, exact predecessors and complete output ownership.

```powershell
# Restore archived candidate first; current runtime is the corrected baseline.
python tooling/rendering-perf/capture-burst.py target/borrow-new --baseline target/borrowed-material-baseline/dense-runtime-perf.exe --candidate-name borrowed --pipelined
python tooling/rendering-perf/analyze-pipeline-burst.py target/borrow-new --implementation-pair --candidate-name borrowed
```
