# Dense placement pages and linear full-validation projection

Status: implemented and measured stage; full RFC acceptance remains active.

## Change and API implications

The private local snapshot now stores placement overrides in immutable 64-entry
pages addressed by stable baseline preorder. Reads avoid NodeId/AVL lookup and
publication copies touched pages once rather than allocating one tree entry per
changed node. Untouched pages and all old snapshots remain shared and immutable.
The page directory still costs O(N/64) to clone. Page size and preorder indices
are private implementation choices, not public persistent node identities.

Full GPU spatial validation projects the retained prefix into one contiguous
array and applies upload ranges in order. It keeps every parent/depth, exact
ordered-f32, clip/scale overflow and unreferenced-node check. Overlapping writes
remain last-win. Prefix shrink/grow uses the actual retained mirror. Rejected
input cannot mutate serial/storage before validation; sparse indexed checks
retain the existing path. This removes repeated tree lookup from full validation
without allowing trusted producers to bypass checks.

The [local snapshot ADR](../../ADR_RETAINED_LOCAL_SNAPSHOT.md) records ownership
and alternatives. No public scene/transaction/snapshot semantics changed.
Dense independent changes still need batched validation and delivery; replacing
their state semantics with an ancestor transform is only valid when the
application actually requests a common transform.

## Alternating previous/current binary comparison

`ab/metadata.json` identifies both executables and the owning source manifests.
Control: commit `fe997cb`, binary SHA-256
`045b231650444c351c89c5e33858427c70798cdb586b674b5db3c02c3e37b0b2`,
matching the earlier stage's final verification. Optimized binary SHA-256:
`70283caa915dffca1c79a17ee4811fcd36cad2d73a2d57b8a32d94432c5198a8`.
Round 1 runs control then optimized; round 2 reverses that order. Each process
retains 120 samples per case/mode with rotated internal ordering and warmups.
The benchmark Rust source is unchanged between these binaries. Compare their
local modes to isolate these changes; each process also retains its original
full-extraction control. Every final image matches outside the timing interval.

Both modes retain full target redraw and full-viewport isolated layers. The
effects fixture has ten opacity groups and ten blur-8 shadows, rerasterized
on motion. Hardware is Ryzen 7 4800H / GTX 1650 Ti / NVIDIA 571.96 / Vulkan,
800×600 scale 1, a mobile regression host, not modern desktop qualification.
Sources stayed stable and 500 ms process sampling found no compiler/other
benchmark conflicts. OS, thermal, power and unrelated work remain uncontrolled.

| 10,000 independent updates | Round | Publication p50 old → new (ms) | Submission p50 old → new (ms) | Completed p50 old → new (ms) | Completed p95 reduction |
| --- | --- | ---: | ---: | ---: | ---: |
| No effects | 1 | 7.090 → 2.480 | 14.881 → 10.911 | 29.280 → 21.034 | 21.9% |
| No effects | 2 | 7.045 → 2.337 | 15.076 → 10.103 | 29.145 → 20.473 | 31.2% |
| Effects | 1 | 7.092 → 1.869 | 14.531 → 9.229 | 30.959 → 21.088 | 28.2% |
| Effects | 2 | 6.900 → 2.242 | 15.339 → 9.419 | 31.318 → 21.293 | 28.9% |

Dense publication improves 65–74%, submission 27–39%, completed median 28–32%.
This remains well above 120 Hz's 8.333 ms budget. Ancestor-only cases do not
benefit consistently: no-effect completed median regresses 6.9% and 26.1%,
with round-2 p95 regressing 60.3%; effect median improves 24.2% then regresses
9.6%, while p95 regresses in both rounds. Small absolute publication changes
include copying the page directory, but the complete-route regression is not
causally isolated by these runs. Preserve losing cases and avoid claiming a
universal speedup. All distributions/raw samples are in `ab/summary.json` and
its adjacent per-process files.

Reproduction (save the previous exact executable before building the new one):

```powershell
./tooling/rendering-perf/run-local-ab.ps1 -Output target/dense-pages-ab -ControlBinary target/dense-placement-control/dense-runtime-perf.exe -ControlCommit fe997cb -Runs 2
python tooling/rendering-perf/analyze-local-ab.py target/dense-pages-ab
./tooling/rendering-perf/run-dense.ps1 -Output target/dense-pages-cadence -SkipBuild -Runs 2 -Local
python tooling/rendering-perf/analyze-dense.py target/dense-pages-cadence
```

## Scope and remaining acceptance

The sustained scope is independent absolute-clock OS producer → bounded source
inbox → owner wake → computed/typed bindings → transaction → local mailbox →
GPU submission and synchronous completion. It counts superseded ticks and
consumed-late ticks separately. Native window/export/Present/display and
physical input are excluded. Synchronous completion can include owner scheduling
and driver wait; no GPU timestamp isolates the cause of a peak.

Two independent ten-second-per-case sustained runs completed with stable sources
and no sampled compiler/benchmark conflicts. Every final image matched. Each
120 Hz row/run has 1,200 target ticks; percentiles cover consumed samples only.

| 120 Hz workload | Run 1 p95 / p99 / max (ms) | Run 2 p95 / p99 / max (ms) | Superseded ticks, runs 1 / 2 | Consumed-late ticks, runs 1 / 2 |
| --- | ---: | ---: | ---: | ---: |
| 1,000 leaves, ancestor, no effects | 5.839 / 7.898 / 9.184 | 5.838 / 7.909 / 9.184 | 0 / 0 | 7 / 5 |
| 1,000 leaves, ancestor, effects | 6.165 / 7.179 / 8.344 | 5.062 / 7.058 / 9.959 | 0 / 0 | 1 / 1 |
| 10,000 leaves, ancestor, no effects | 8.538 / 9.745 / 10.725 | 8.229 / 9.725 / 10.414 | 0 / 0 | 65 / 56 |
| 10,000 leaves, ancestor, effects | 7.482 / 9.309 / 14.330 | 7.283 / 8.302 / 11.081 | 0 / 0 | 27 / 11 |
| 10,000 leaves, independent, no effects | 32.149 / 33.731 / 34.683 | 33.395 / 36.168 / 37.519 | 753 / 764 | 446 / 435 |
| 10,000 leaves, independent, effects | 36.133 / 39.848 / 42.742 | 37.712 / 41.038 / 43.683 | 790 / 824 | 409 / 375 |

Independent/effects p99 is lower than the previous stage's 47.70–51.90 ms,
and fewer target ticks are superseded, but 1,199 target ticks remain unsatisfied
consecutively after the equal initial sample. The same is true without effects.
These dense cases still fail 60 Hz too. This historical cadence comparison is
not concurrent A/B; the alternating binaries above supply the narrower control.

At 60 Hz, 1,000-leaf effects miss two ticks in run 1 and one in run 2;
10,000-leaf ancestor/effects misses one in run 1. No-effect ancestor cases have
zero misses. The longest 120 Hz ancestor/effect miss burst is four ticks.
All raw phase distributions and 60 Hz results are in `cadence/summary.json`.
For dense independent/effects run 1, p50 commit is 7.147 ms, publication
2.077 ms and submission 10.269 ms. Those owner costs exceed 120 Hz before
waiting for GPU completion. Ancestor publication remains about 0.010 ms while
completion tails exceed the deadline, requiring separate scheduling/GPU analysis.

The unchanged effect-layer strategy, dense reactive transaction/sink costs,
GPU dirty-set/preflight/content scans and allocation pressure remain important.
Next API work must keep lossless state/lifecycle operations separate from latest
visual sampling, admit work before snapshot construction, preserve bounded
in-flight ownership and separate effect content identity from placement.
Software mode still needs a real renderer, explicit capability/budget negotiation
and responsiveness/fidelity qualification. Visible/pipelined 60/120 Hz, modern
desktop hardware, larger surfaces, cold/resource churn, general hit/damage,
complex primitives and all other tracked RFC acceptance remain open.

## Verification

145 core tests and 11 shared-example tests passed. The new page test verifies
dense publication followed by a sparse edit, untouched-page identity and old
snapshot geometry. Core retained-only and GPU performance Clippy checks pass
with `-D warnings`. The complete hardware matrix passes 2,011 scenarios / 1,921
pixel comparisons, including indexed/full backend malformed input, overlap,
unreferenced overflow, prefix reexposure and rejected-input retry controls.

The rebuilt Windows native delivery diagnostic passes with 1,000 source messages
coalesced into one wake/transaction/render, matching pixels and completion lease
retirement without a later frame. Shared texture framebuffer comparison passes
97,792 pixels and device replacement passes 3,072 pixels. These are hidden
explicit draws; Present acceptance is not visible display confirmation. Compact
results, executable identities and local artifact paths are in `verification.json`
and `regression.json`. Formal timing uses the exact binary recorded in the A/B
and cadence metadata, with unchanged source manifests during both experiments.
