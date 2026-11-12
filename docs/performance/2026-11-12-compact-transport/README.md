# Compact local spatial transport and arena dirty words

Status: measured implementation stage; total RFC acceptance remains active.

## Changes and preserved contracts

Local incremental spatial records now carry only NodeId, SpatialState and an
optional resolved anchor origin/clip. Ordinary records do not clone PaintItem
or its command, image, glyph and shadow resource references. Depth-32 anchors
still resolve exact ordered world geometry. The complete local snapshot and
resolved fallback remain available for skipped bases, layout/material changes,
opacity changes, consumer switches and reset. These records remain private.

The generic instance arena uses dirty bit words plus a touched-word list instead
of an ordered entry per dirty slot. It sorts touched words, enumerates set bits
in ascending slot order and merges adjacent writes. Repeated writes remain
idempotent. Retirement removes pending dirty slots, generation checks reject
old handles, and only successful submission acknowledges writes. A separate
listed flag prevents duplicate touched words when a word becomes empty before
acknowledgement. Sparse iteration visits touched words, not the whole arena.

The reactive binding table now hashes exact BindingId lookups. Source-local
ordered binding sets still control dispatch. Removal explicitly sorts detached
identities before cleanup. No hash traversal defines business event/effect
ordering, and failed transactions/drafts still publish neither sources nor
sinks. This avoids a redundant lookup tree without adding an application API
that bypasses transaction or sink validation.

See the [snapshot decision](../../ADR_RETAINED_LOCAL_SNAPSHOT.md). Neither GPU
slots nor private page/transport representation are stable public identities.
Persistent bitmap storage grows with arena capacity; touched-word sorting and
temporary spatial vectors still allocate. Dense state transactions, GPU
validation, world-space layer caches and synchronous completion remain costs.

## Measurement controls

The previous executable is commit `88d3596`, SHA-256
`70283caa915dffca1c79a17ee4811fcd36cad2d73a2d57b8a32d94432c5198a8`,
matching the previous stage's formal metadata. The benchmark Rust source,
effects, full target redraw and full-viewport isolated-layer strategy are
unchanged. Each binary also runs its complete-extraction control. Compare local
modes between binaries for the internal transport/lookup change.

Two alternating rounds run previous→current then current→previous, preserving
120 timed samples per mode/case with internal rotation and warmups. The separate
sustained experiment runs twelve ten-second cases in each of two processes.
Owning source manifests and executable hashes identify the implementation;
500 ms sampling rejects compiler/other benchmark activity. This does not exclude
every OS, thermal, power or unrelated GPU effect. No outlier is removed.

Hardware: Ryzen 7 4800H, GTX 1650 Ti 4 GiB, NVIDIA 571.96, WGPU Vulkan,
800×600 physical pixels at scale 1. This is a mobile regression machine,
not modern desktop GPU qualification. Effects are ten opacity groups with
ten blur-8 shadows; all ten layers still rerasterize on motion.

```powershell
./tooling/rendering-perf/run-local-ab.ps1 -Output target/compact-ab -ControlBinary target/dense-transport-control/dense-runtime-perf.exe -ControlCommit 88d3596 -Runs 2
python tooling/rendering-perf/analyze-local-ab.py target/compact-ab
./tooling/rendering-perf/run-dense.ps1 -Output target/compact-cadence -SkipBuild -Runs 2 -Local
python tooling/rendering-perf/analyze-dense.py target/compact-cadence
```

The sustained chain is independent absolute-clock visual producer → bounded
source inbox → owner executor wake → computed/typed bindings → state transaction
→ local snapshot/mailbox → GPU submit and synchronous completion. Superseded
and consumed-late ticks are disjoint and both remain unsatisfied target work.
Percentiles alone exclude skipped ticks, so deadline/coalescing counts must be
read alongside them. These measurements exclude native scene/export, Present,
compositor/display and physical input. They do not prove visible 120 fps.

## Remaining work affecting API design

Keep lossless state/resource/lifecycle operations separate from latest visual
sampling and bound pending/in-flight work. Dense batches still need atomic
validation; a fast transport is not permission to drop business state updates.
Effect raster content identity must separate from placement, preserving exact
clip/opacity and fractional pixel behavior. Local hit/damage indexing, general
primitives, resource pressure, visible pipelined presentation and modern desktop
hardware remain open. Software rendering still needs a real implementation,
explicit capability/work budgets and responsiveness/fidelity tests. Requested
preferences are not backend support, and no CPU-raster performance claim is made.

## Results against the previous binary

Current binary SHA-256: `5b1a5c32fad06d994767b025d8f1b674a186af9e1048ec64bb4091d065272648`. Both alternating rounds preserve exact source manifests and report no sampled conflicts. Positive reduction means faster; negative values are regressions.

| Round | Effects | Local workload | Commit p50 reduction | Publication p50 reduction | Submit p50 reduction | Completed p50 reduction | Completed p95 reduction |
| --- | --- | --- | ---: | ---: | ---: | ---: | ---: |
| 1 | False | ancestor | -3.4% | 1.6% | 7.7% | 3.6% | 7.2% |
| 1 | False | independent | -2.0% | -3.0% | 19.2% | 5.7% | -2.3% |
| 1 | True | ancestor | 0.0% | 2.5% | -0.8% | -10.8% | 0.7% |
| 1 | True | independent | -4.0% | -7.5% | 17.8% | 4.0% | 1.8% |
| 2 | False | ancestor | 0.0% | 6.6% | 10.4% | 11.0% | 23.6% |
| 2 | False | independent | -9.6% | -14.3% | 16.0% | 1.0% | 3.0% |
| 2 | True | ancestor | -4.8% | -1.3% | -4.1% | -1.9% | 1.2% |
| 2 | True | independent | -7.5% | -8.2% | 16.9% | 0.4% | 0.8% |

Dense submission improves 16–19%, but completed medians improve only 0.4–5.7%. Dense commit regresses 2–10% and publication regresses 3–14%; the aggregate comparison cannot isolate the binding hash lookup and supplies no evidence of a standalone transaction benefit. Ancestor/effects completed medians regress 10.8% and 1.9%. Dense no-effect completed p95 regresses 2.3% in round 1. This is a narrower transport improvement, not a general tail-latency win.

## Sustained absolute-clock results

Each row/run lasts ten seconds; 120 Hz has 1,200 target ticks. Percentiles cover consumed samples; superseded target ticks remain unsatisfied work. Full 60/120 Hz phase distributions are retained in `cadence/summary.json` and unfiltered samples in `run-*/cadence.json`.

| 120 Hz workload | Run 1 p95 / p99 / max ms | Run 2 p95 / p99 / max ms | Superseded 1 / 2 | Consumed-late 1 / 2 | Longest unsatisfied burst 1 / 2 |
| --- | ---: | ---: | ---: | ---: | ---: |
| 1,000, ancestor, effects=False | 5.340 / 7.734 / 8.689 | 5.596 / 7.942 / 8.818 | 0 / 0 | 3 / 2 | 1 / 1 |
| 1,000, ancestor, effects=True | 5.280 / 6.925 / 7.817 | 6.049 / 7.498 / 15.011 | 0 / 0 | 0 / 8 | 0 / 3 |
| 10,000, ancestor, effects=False | 4.909 / 6.049 / 9.572 | 8.543 / 9.838 / 10.758 | 0 / 0 | 2 / 73 | 1 / 2 |
| 10,000, ancestor, effects=True | 7.122 / 8.276 / 10.930 | 7.161 / 8.496 / 12.897 | 0 / 0 | 11 / 19 | 1 / 3 |
| 10,000, independent, effects=False | 30.140 / 31.501 / 38.945 | 30.331 / 33.582 / 43.805 | 719 / 713 | 480 / 486 | 1199 / 1199 |
| 10,000, independent, effects=True | 35.432 / 39.267 / 42.623 | 35.738 / 39.138 / 49.191 | 792 / 781 | 407 / 418 | 1199 / 1199 |

Dense independent/effects p99 remains 39.14–39.27 ms at 120 Hz, with 1,199 consecutive unsatisfied ticks. At 60 Hz its p99 is 44.06–45.95 ms, max 48.78–54.25 ms, and each run has 599 unsatisfied target ticks. Thus both dense targets fail. At 60 Hz, no 1,000-leaf ancestor case misses; 10,000-leaf ancestor cases miss 0/1 without effects and 0/2 with effects. Ancestor/effects 120 Hz misses are 11/19 with bursts 1/3. Small-scene run 2 still spikes to 15.01 ms; preserving failures matters more than average FPS.

Dense independent/effects run 2 p50 commit is 7.158 ms, publication/mailbox 2.401 ms and submit 8.252 ms. Owner work alone exceeds 8.333 ms. Further GPU upload optimization cannot remove this state/publication bottleneck. Relative to the preceding stage, dense p99 is broadly similar; those historical runs are not a causal A/B comparison.

## Verification

147 core tests and 11 example tests pass; core retained-only and GPU benchmark Clippy pass with `-D warnings`. Hardware qualification on the current rebuilt executable passes 2,011 scenarios / 1,921 pixel comparisons, including full/local routing, malformed preflight, skipped bases, generation reuse and rejected-input retry. The new tests cover dirty-word retirement/reuse and ordered binding dispatch across hash growth/unbinding.

Windows native delivery passes 1,000 messages coalesced into one wake/transaction/render with completion retirement while idle. Shared surface comparison passes 97,792 pixels; device replacement passes 3,072 pixels while preserving the old snapshot. These are hidden explicit diagnostic draws, not visible presentation or input-to-photon evidence. Executable identities and compact results are in `verification.json` and `regression.json`; local `target/` artifacts are not checked in.
