# Visual admission before owner construction and pipelined completion

Status: implementation stage; the full RFC goal remains active.

## Contracts

`poll_frame_capacity` performs nonblocking completion collection, returning actual
GPU in-flight count, configured limit and device epoch. A single owner can leave
replaceable values in its bounded last-value inbox while full, awaiting the
existing completion task. The observation expires after another submit or device
replacement. It does not reserve GPU/memory, promise Present availability or
preempt arbitrary user callbacks. Layer memory pressure may still wait.

Normal gallery background delivery now uses this check before source/sink
fanout. It keeps one owner executor waiter, woken by existing completion events;
normal click/lifecycle/resource operations retain their ordered atomic semantics.
A closed epoch resubscribes before waking admission. The completion worker and
UI tree remain independently owned; no timer or additional completion worker is
introduced. This is a capacity gate, not a complete deadline admission model.

`CompletionReceiver::ready_receipt` returns the native worker's monotonic
observation instant and covering serial. Existing `ready` still returns serials.
A newest coalesced receipt covers earlier work, losing its exact completion
instant; timings for covered frames are conservative upper bounds, never GPU
hardware timestamps or Present/display/input-to-photon latency. The harness
separately records the delay before the owner observes that receipt.

See [frame-budget decision](../../ADR_RETAINED_FRAME_BUDGET.md).

## Experiment

The previous synchronous chain remains unchanged and available. The new
`dense-runtime-perf --pipeline-only` uses the same absolute-clock OS producer,
last-value inbox, computed/typed sinks and local immutable snapshots, with a
completion worker and at most two GPU submissions in flight. It compares
capacity admission before source fanout/snapshot construction with the same
capacity check after construction. Both retain full effect fidelity and use
full redraw/full-viewport isolated layers. Each process runs twelve workload
pairs for ten seconds per mode. The second process reverses the pair order.
Each final image matches a fresh full reference and all leases retire without
extra submissions. Raw phases, skipped targets, observed-late targets, longest
unsatisfied bursts, actual gate waits, queue counts and executable/source hashes
are retained. Process sampling every 500 ms rejects compiler/benchmark conflicts;
it cannot exclude all OS, power, thermal or driver effects. No outlier is removed.

The separate pressure control uses one GPU flight at 2400x1800 / scale 3; ordinary
cases use 800x600 / scale 1. Pressure tests still target 60/120 Hz with identical
semantics; an idle gate is not evidence of admission benefit. These controls
isolate gate placement within the new pipelined implementation. Comparing its
tails with earlier synchronous results is historical and not a causal A/B test
of pipeline vs synchronous timing, particularly because receipt scope differs.

Hardware is Ryzen 7 4800H / GTX 1650 Ti 4 GiB, Vulkan / driver 571.96. It is a
mobile regression baseline, not modern desktop GPU qualification. Native visible
presentation, physical input, effect content placement reuse, deadline cost
prediction and a real software consumer remain open.

```powershell
./tooling/rendering-perf/run-dense.ps1 -Output target/pipeline-formal -SkipBuild -Runs 2 -Pipeline
python tooling/rendering-perf/analyze-pipeline.py target/pipeline-formal
./tooling/rendering-perf/run-dense.ps1 -Output target/pressure-formal -SkipBuild -Runs 2 -Pipeline -Pressure
python tooling/rendering-perf/analyze-pipeline.py target/pressure-formal
```

## Interpretation limits

A receipt's worker timestamp and the time at which the owner processes it are
different. Dense owner commit/publication/submission can delay completion
handling by another full frame of work. A lower completed-work age therefore
does not establish responsive input handling or timely resource retirement.
The raw `source_to_owner_completion_observation_us` and
`completion_to_owner_observation_us` fields preserve that distinction. The
worker's newest-job mailbox may also observe an earlier submission only when a
newer covering submission finishes; all reported misses are upper bounds for
the receipt scope, not exact per-frame hardware lateness.

Preconstruction capacity admission is useful only when GPU flights actually
fill. It does not bound time spent in a non-yielding dense owner transaction,
the count of callbacks processed before a host render, memory required for an
individual primitive, or native surface export/presentation. Explicit frame
demand, cost prediction, UI scheduling/yield boundaries and cancellable software
jobs remain separate API work. No amount of extra in-flight depth should be
called a latency win without checking both ages and retained work.

## Two-flight 800x600 results

All 48 cases report zero capacity waits and zero layer-budget waits. Maximum observed GPU flights is two; at most three benchmark receipt records were pending while the covering event awaited owner processing. Those records are not extra GPU submissions. Both runs have stable source manifests and no sampled process conflicts.

| 120 Hz workload | Run | Admission before work p99 / max ms | After construction p99 / max ms | Before skips / late / longest burst | After skips / late / longest burst |
| --- | --- | ---: | ---: | ---: | ---: |
| 1,000, ancestor, effects=False | run-1 | 8.121 / 11.454 | 7.969 / 8.961 | 0 / 7 / 1 | 0 / 3 / 1 |
| 1,000, ancestor, effects=False | run-2 | 8.251 / 9.475 | 8.105 / 8.929 | 0 / 10 / 1 | 0 / 8 / 1 |
| 1,000, ancestor, effects=True | run-1 | 7.159 / 8.201 | 7.249 / 17.114 | 0 / 0 / 0 | 0 / 4 / 2 |
| 1,000, ancestor, effects=True | run-2 | 7.067 / 8.266 | 7.125 / 7.775 | 0 / 0 / 0 | 0 / 0 / 0 |
| 10,000, ancestor, effects=False | run-1 | 5.790 / 6.852 | 9.352 / 16.121 | 0 / 0 / 0 | 0 / 44 / 1 |
| 10,000, ancestor, effects=False | run-2 | 9.484 / 12.267 | 9.406 / 13.429 | 0 / 59 / 1 | 0 / 76 / 1 |
| 10,000, ancestor, effects=True | run-1 | 8.476 / 12.277 | 8.504 / 12.366 | 0 / 16 / 1 | 0 / 16 / 1 |
| 10,000, ancestor, effects=True | run-2 | 8.398 / 15.118 | 8.992 / 14.893 | 0 / 13 / 2 | 0 / 23 / 1 |
| 10,000, independent, effects=False | run-1 | 32.367 / 34.809 | 33.364 / 35.385 | 582 / 617 / 1199 | 590 / 609 / 1199 |
| 10,000, independent, effects=False | run-2 | 30.816 / 34.147 | 32.945 / 47.762 | 554 / 645 / 1199 | 590 / 609 / 1199 |
| 10,000, independent, effects=True | run-1 | 33.684 / 38.189 | 31.341 / 33.706 | 607 / 592 / 1199 | 599 / 600 / 1199 |
| 10,000, independent, effects=True | run-2 | 30.082 / 32.592 | 32.517 / 35.887 | 576 / 623 / 1199 | 584 / 615 / 1199 |

Dense independent/effects at 120 Hz has p99 upper bounds 30.08–33.68 ms and 1,199 consecutive unsatisfied target ticks in every mode/run. At 60 Hz, p99 is 34.11–42.12 ms and max reaches 53.28 ms. CPU-bound dense work still fails both targets. Before-work admission worsens dense no-effects 60 Hz p99 in both rounds (32.87→36.22 ms and 27.57→37.18 ms), and dense/effects 120 Hz p99 in round 1 (31.34→33.68 ms). No active gate wait explains any improvement or regression; these paired results demonstrate no stable performance benefit on this ordinary workload.

The pre-admission 10,000-leaf ancestor/effects 120 Hz cases still miss 16/13 targets; max is 12.28/15.12 ms. A zero-miss 1,000-leaf effect case does not close the larger scope. Small-scene 60 Hz/effects max reaches 26.05 ms in the control. All failures remain in raw evidence.

run-1, admission-before=False: dense/effects p99 completion-to-owner delay is 17.82 ms; p99 source-to-owner-completion observation is 46.28 ms. GPU receipt age alone would hide that owner backlog.
run-1, admission-before=True: dense/effects p99 completion-to-owner delay is 17.27 ms; p99 source-to-owner-completion observation is 46.22 ms. GPU receipt age alone would hide that owner backlog.
run-2, admission-before=False: dense/effects p99 completion-to-owner delay is 20.73 ms; p99 source-to-owner-completion observation is 50.54 ms. GPU receipt age alone would hide that owner backlog.
run-2, admission-before=True: dense/effects p99 completion-to-owner delay is 16.84 ms; p99 source-to-owner-completion observation is 44.30 ms. GPU receipt age alone would hide that owner backlog.

## One-flight 2400x1800 control

| Run | Effects | Hz | Before p99 / max ms | After p99 / max ms | Before waits / late / burst | After waits / late / burst |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| run-1 | False | 60 | 11.857 / 12.432 | 11.703 / 12.234 | 0 / 0 / 0 | 0 / 0 / 0 |
| run-1 | False | 120 | 7.055 / 8.096 | 7.750 / 12.088 | 0 / 0 / 0 | 6 / 9 / 3 |
| run-1 | True | 60 | 10.540 / 11.068 | 10.482 / 11.415 | 0 / 0 / 0 | 0 / 0 / 0 |
| run-1 | True | 120 | 6.462 / 7.814 | 7.141 / 7.860 | 0 / 0 / 0 | 0 / 0 / 0 |
| run-2 | False | 60 | 11.304 / 11.828 | 11.630 / 12.048 | 0 / 0 / 0 | 0 / 0 / 0 |
| run-2 | False | 120 | 7.202 / 10.302 | 6.976 / 8.475 | 2 / 2 / 1 | 0 / 1 / 1 |
| run-2 | True | 60 | 10.479 / 11.427 | 10.511 / 11.153 | 0 / 0 / 0 | 0 / 0 / 0 |
| run-2 | True | 120 | 8.220 / 13.049 | 6.344 / 7.482 | 8 / 12 / 11 | 0 / 0 / 0 |

All sixteen cases preserve final pixels, retire leases and stay within one GPU flight; source skips and layer-budget waits are zero. This workload produces 16 total gate waits: six after-work waits in round-1 no-effects/120 Hz, two before-work waits in round-2 no-effects/120 Hz and eight before-work waits in round-2 effects/120 Hz. All 60 Hz cases meet the measured completion-observation bound in these runs. The round-2 effects/120 Hz before-work case loses: p99 8.220 ms vs 6.344 ms, max 13.049 ms vs 7.482 ms, 12 late targets with an 11-tick burst vs none. A one-second exploratory run also produced waits, but is not substituted for these ten-second formal results.

Admission-before and after-work checks share the same nonblocking observation and completion subscription, so the result includes overlap/scheduling differences. No consistent performance benefit is established, and a stronger sustained GPU-pressure workload, burst/churn cases, modern desktop hardware and native visible presentation remain required. Preserve this gate as a bounded-work mechanism, not a claim of 120 fps.


## Verification and provenance

150 core tests and 11 example tests pass (one hardware click test ignored in the
ordinary suite). GPU benchmark and retained-WGPU example Clippy pass with
`-D warnings`. Completion protocol tests preserve serial-only behavior, newest
receipt coalescing, timestamps preceding owner observation, waiter replacement,
closure and single-delivery errors. Hardware checks validate empty/completed
capacity and epoch change on reset; full hardware regression passes 2,011
scenarios / 1,921 pixel comparisons, including indexed/original rejection/retry.

The final desktop binary passes native delivery: 1,000 source messages coalesce
into one wake/transaction/render, no equal extra rendering, and completion leases
retire while idle. Shared surface verification passes 97,792 framebuffer pixels;
native device replacement passes 3,072 pixels and retains the old snapshot.
These are hidden explicit diagnostic draws. They do not prove automatic visible
presentation, stress the capacity wait deterministically, or measure physical
input/OS energy. The high-resolution cases provide actual but limited gate waits;
a deterministic normal-host saturation test remains open.

Formal pipeline/pressure binary SHA-256:
`8f0737354db8556c2e7e16417b452a319e8f3af133cfcdac107a601575452774`.
Both measurements use the same executable and stable owning-source manifests;
compiler activity happens only before/after timing. Final hardware/native binary
identities and compact facts are in `regression.json` and `verification.json`.
Local `target/` outputs are not checked in. Raw formal samples are retained here.

Next work must reduce dense owner preparation/publication and driver preflight,
make effect content independent of placement, qualify burst/resource pressure,
and implement resumable CPU raster work with semantic fidelity and explicit
capabilities. Capacity observations and policy enums are not a software renderer
or a general deadline scheduler. The complete RFC acceptance stays active.
