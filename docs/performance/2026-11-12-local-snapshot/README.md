# Local snapshots: large publication savings, unresolved deadline stalls

The total RFC goal remains active. This stage implements private immutable
local snapshots and a direct spatial GPU consumer in the normal GPU gallery.
It removes descendant world-geometry publication for ancestor motion. It does
not establish stall-free 120 fps, modern desktop GPU qualification or a CPU
renderer.

## Method and evidence

Ryzen 7 4800H / GTX 1650 Ti 4 GiB, NVIDIA 571.96, WGPU Vulkan,
800×600 physical pixels, scale 1. This is a mobile regression machine.
Two independent processes ran twelve sustained 10-second cases each, plus
paired controls with 120 samples per mode/case, rotated ordering and warmups.
All final images matched the full reference outside the measured interval.

The paired control uses the same runtime/shaders and original complete
reextraction. The local path uses full-viewport isolated layers; the resolved
control uses tight layer bounds. This measures complete routes, including that
trade-off, rather than isolating publication alone. Both redraw the complete
target. Effects mean ten isolated opacity groups and ten blur-8 box shadows;
all ten layers rerasterize on motion, with zero layer reuse.

The sustained chain is an independent absolute-clock OS producer → bounded
last-value `UpdateSender` → actual owner waker → computed/typed bindings →
transaction → single-pending local queue → GPU submit → synchronous completion.
Source/owner scheduling, every raw outlier and superseded tick are retained.
It excludes native scene/export/Present/display and physical input latency.
Synchronous completion must still be complemented by pipelined visible tests.

Reproduce with Rust 1.98.1:

```powershell
./tooling/rendering-perf/run-dense.ps1 -Output target/local-reproduction -Runs 2 -Local
python tooling/rendering-perf/analyze-dense.py target/local-reproduction
```

`metadata.json` confirms stable owning sources and no sampled compiler/other
benchmark conflicts at 500 ms intervals. The runner cannot exclude all OS,
thermal, power or unrelated GPU activity. Binary SHA-256:
`561455f942bd568fa30d4e5f7e026ed6cb56c847a5f939270e7deb5e317d96a6`.
Adjacent source manifests identify the measured implementation; exploratory
earlier prototypes are not included as formal evidence.

After these measurements, final lifecycle review limited the local dirty
journal to existing local subscribers and cleared it on resolved publication.
That cleanup is covered by an alternating-publication regression test and
separate final verification. The timed binary and manifests above identify the
pre-cleanup measurement; final verification does not relabel it as a different
binary's result.

## Paired results

| 10,000-leaf update | Local / control completed p50, run 1 (ms) | Run 2 (ms) | Completed p50 reduction, runs 1 / 2 |
| --- | ---: | ---: | ---: |
| Ancestor, no effects | 1.075 / 15.272 | 1.130 / 14.819 | 93.0% / 92.4% |
| Ancestor, effects | 3.037 / 18.249 | 3.487 / 17.820 | 83.4% / 80.4% |
| Independent batch, no effects | 29.206 / 41.589 | 30.675 / 42.045 | 29.8% / 27.0% |
| Independent batch, effects | 31.670 / 44.170 | 33.240 / 44.628 | 28.3% / 25.5% |

Ancestor publication p50 falls from roughly 10 ms to 0.005–0.007 ms (99.9%).
Only one local node changes, with 96 spatial bytes and no Quad geometry upload.
Independent publication improves about 59–61% but still costs 7.15–7.49 ms,
with 10,000 local changes and 960,000 spatial bytes. Its owner commit costs
5.48–5.89 ms and submission 15.23–16.14 ms. Faster raster hardware alone
cannot remove these CPU costs.

## Sustained 120 Hz tails

Each row/run has 1,200 target ticks. Percentiles describe consumed ticks, so
they must be read with superseded and consumed-late counts.

| Workload | Run 1 p95 / p99 / max (ms) | Run 2 p95 / p99 / max (ms) | Superseded ticks, runs 1 / 2 | Consumed-late ticks, runs 1 / 2 |
| --- | ---: | ---: | ---: | ---: |
| 1,000 leaves, ancestor, no effects | 5.573 / 7.684 / 9.106 | 5.690 / 8.149 / 9.342 | 0 / 0 | 4 / 9 |
| 1,000 leaves, ancestor, effects | 5.964 / 7.109 / 10.474 | 6.308 / 7.259 / 10.977 | 0 / 0 | 2 / 5 |
| 10,000 leaves, ancestor, no effects | 8.421 / 9.735 / 10.660 | 5.025 / 7.050 / 11.568 | 0 / 0 | 63 / 9 |
| 10,000 leaves, ancestor, effects | 7.359 / 8.326 / 13.439 | 7.353 / 8.540 / 10.556 | 0 / 0 | 12 / 15 |
| 10,000 leaves, independent, no effects | 41.912 / 44.816 / 46.714 | 41.270 / 44.411 / 53.980 | 878 / 873 | 321 / 326 |
| 10,000 leaves, independent, effects | 46.609 / 51.900 / 54.002 | 43.592 / 47.699 / 51.302 | 913 / 901 | 286 / 298 |

Ancestor updates no longer coalesce ticks in these runs, but all 120 Hz cases
still miss deadlines. The worst ancestor/effect burst is three consecutive
unsatisfied ticks. Independent updates have 1,199 consecutive unsatisfied ticks
after the equal initial sample and still fail 60 Hz. At 60 Hz ancestor cases
have zero misses except the run-1 1,000-leaf effect case: one 19.245 ms peak.
The raw summaries contain every 60 Hz distribution and per-phase tail.

Small scenes regress relative to the earlier resolved-path sustained stage:
1,000-leaf effects previously had p99 4.11–4.46 ms; this stage has 7.11–7.26 ms.
These are separate process runs with a changed layer strategy, not an isolated
causal A/B. Full-viewport raster work, scheduling/driver waits and temperature
remain possible contributors. The stage must not be promoted as a universal
frame-time improvement.

For run-1 10,000-leaf ancestor/effects, the 13.439 ms peak at tick 1 comprises
0.592 ms source-to-owner, 0.027 ms commit, 0.009 ms publication and 1.625 ms
submission; the remaining 11.187 ms is synchronous GPU completion/collection
and possible owner scheduling. No GPU timestamp establishes its cause.
Dense independent/effects instead spends p50 6.505 ms commit, 7.626 ms
publication and 15.904 ms submission, which already exceeds 120 Hz before
completion. Both kinds of stall need separate treatment.

## Correctness, native scope and API consequences

`native-qualification.json` contains compact current-binary Windows delivery,
shared-texture and readback controls. The normal click hardware test passes.
The new mixed local route checks 72 scale/capability/transition cases, including
text/images/shadows, deep anchors, clip/scroll/opacity, skipped bases and reset.
Native delivery coalesces 1,000 messages into one transaction/wake/render and
retires completion leases without another frame. Native framebuffer and device
replacement controls pass. These diagnostics use hidden windows with explicit
draws. A successful native Present attempt is not display confirmation.

Final post-cleanup verification passed 144 core tests, 11 shared-example tests,
Clippy with `-D warnings`, and 2,011 hardware scenarios / 1,921 pixel
comparisons. A one-second-per-case final smoke run passed all reference images
and source/process integrity checks; it does not replace the longer measured
tails above. Final native delivery also passed after cleanup. Identities and
compact results are in `regression.json`, `final-verification.json` and
`final-source-manifest.json`; full hardware/native artifacts remain under the
explicit `target/local-final-*` paths recorded there.

The [local snapshot ADR](../../ADR_RETAINED_LOCAL_SNAPSHOT.md) records ownership,
exact predecessor provenance, explicit world resolution and full recovery.
Normal GPU consumers should consume local immutable placement rather than
require `&PaintItem` with eagerly resolved descendant coordinates. This does
not finalize the general chunk API: local hit traversal is linear, damage is
full, opacity changes resolve completely and layer extents remain full viewport.

Next priorities affecting framework APIs are admission/latest visual sampling
before packet construction; dense batched state and placement processing;
placement-independent effect raster identity; explicit completion/presentation
observations; and software capability/work budgets preserving semantic layout,
clip, opacity, text and event geometry. Native visible/pipelined 60/120 Hz,
modern desktop hardware, larger surfaces/cold/resource churn and a real CPU
renderer with responsiveness/fidelity acceptance all remain open.
