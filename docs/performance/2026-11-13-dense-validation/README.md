# Dense spatial backend preflight

Status: alternating and sustained GPU-completion qualification passed correctness; deadline acceptance remains open. Pipelined and hidden native regression checks pass; visible presentation remains open. Full RFC goal remains active.

Dense spatial transport now collects contiguous writes and deduplicates final parent edges in reverse order with epoch-stamped slot membership, avoiding a BTreeMap when the transport exceeds the existing dense threshold. This can deliberately select a full-prefix check for many overlapping writes even if unique dirty membership is small; it is a work heuristic, not a semantic rule or caller-trust flag. Sparse mode retains indexed descendant validation. The dense plan changes no authoritative topology until every consumer preflight succeeds; rejected plans only touch scratch stamps.

The dense final projected prefix uses 32 depth buckets and one resolved result per slot instead of repeating a root-to-leaf walk for every node. Parents may occur after their children. Each node's exact f32 origin/scroll/clip operation order is retained. All addressable nodes, including unreferenced and re-exposed slots, are checked. Raw record finiteness/flags/depth, allocation/range bounds and scale still pass the existing storage preflight, including every overwritten raw record. Range overlap remains last-writer wins. The original `set_indexed_spatial_enabled(false)` full walk is unchanged as a correctness/performance control.

The additional temporary cost is O(N) resolved states and depth membership per dense frame, alongside the projected mirror. No persistent owner ABI or GPU shader/buffer ABI changes. This does not remove full prefix work, owner binding/transaction costs, effect-layer rerasterization, resource admission or native presentation. It does not establish a general memory budget.

Core regressions exercise reversed slot order at every shader depth, nested fractional clips/scroll, malformed edges/anchors/depths, unreferenced/scaled/clip overflow, dense overlapping writes with an overwritten invalid graph edge, dropped plans and sparse follow-up using final topology. Hardware validation adds the same permuted/dense overlap and unreferenced-overflow/retry cases in indexed and original modes.

Control: local commit `d844bc3`, previous exact dense binary preserved at `target/dense-validation-control/dense-runtime-perf.exe`. Benchmark/shader/layer policy remain unchanged. Alternating control/current then current/control use 120 samples per mode/case. Sustained tests use two independent processes, twelve ten-second cases each, absolute 60/120 Hz source clocks, owner inbox/wake/computed/sink/transaction/publication/submit and synchronous GPU completion. All superseded ticks and tail outliers are retained. GPU completion excludes native display, compositor/presentation and physical input. This Ryzen 4800H / GTX 1650 Ti mobile host does not qualify modern desktop 120 fps.

```powershell
./tooling/rendering-perf/run-local-ab.ps1 -Output target/dense-validation-ab -ControlBinary target/dense-validation-control/dense-runtime-perf.exe -ControlCommit d844bc3 -Runs 2
python tooling/rendering-perf/analyze-local-ab.py target/dense-validation-ab
./tooling/rendering-perf/run-dense.ps1 -Output target/dense-validation-cadence -SkipBuild -Runs 2 -Local
python tooling/rendering-perf/analyze-dense.py target/dense-validation-cadence
```

Exact executable/owning-source hashes are frozen in the capture metadata. See [GPU spatial decision](../../ADR_RETAINED_SPATIAL_GPU.md) and [frame budget decision](../../ADR_RETAINED_FRAME_BUDGET.md).

## Alternating binary results

| Round | Effects | Work | Submission p50 old / new ms | Reduction | Completed p50 old / new ms | Reduction | Completed p95 reduction |
| --- | --- | --- | ---: | ---: | ---: | ---: | ---: |
| 1 | False | ancestor | 0.659 / 0.539 | 18.2% | 1.009 / 0.907 | 10.0% | 18.9% |
| 1 | False | independent | 7.468 / 5.999 | 19.7% | 15.254 / 13.422 | 12.0% | 22.4% |
| 1 | True | ancestor | 1.060 / 1.051 | 0.9% | 2.600 / 2.047 | 21.3% | 0.8% |
| 1 | True | independent | 7.832 / 6.647 | 15.1% | 17.098 / 14.375 | 15.9% | 20.9% |
| 2 | False | ancestor | 0.640 / 0.531 | 17.1% | 1.017 / 0.875 | 14.0% | 14.2% |
| 2 | False | independent | 6.880 / 6.002 | 12.8% | 13.240 / 12.372 | 6.6% | 7.8% |
| 2 | True | ancestor | 0.988 / 1.098 | -11.1% | 2.321 / 3.086 | -33.0% | -6.1% |
| 2 | True | independent | 7.548 / 7.022 | 7.0% | 15.089 / 14.687 | 2.7% | 4.1% |

Dense submission medians improve 7.0–19.7%; completed medians improve 2.7–15.9%. The second ancestor/effects case loses 11.1% in submission median and 33.0% in completed median. Unchanged publication loses 13.4% in the second dense/effects round; whole-process OS/driver/cache variance prevents assigning every phase delta to the changed backend. No universal tail win or deadline claim follows. All original distributions, raw samples, binary hashes and owning-source manifests remain under `ab/`. Analysis script hashes are separately preserved in `analysis-manifest.json`.

## Sustained results

| Run | Nodes | Work | Effects | Hz | p95 / p99 / max ms | Superseded | Consumed late | Longest unsatisfied burst |
| --- | ---: | --- | --- | ---: | ---: | ---: | ---: | ---: |
| run-1 | 1000 | ancestor | False | 60 | 5.399 / 7.620 / 8.902 | 0 | 0 | 0 |
| run-1 | 1000 | ancestor | False | 120 | 5.774 / 7.909 / 9.250 | 0 | 2 | 1 |
| run-1 | 1000 | ancestor | True | 60 | 11.196 / 12.371 / 13.777 | 0 | 0 | 0 |
| run-1 | 1000 | ancestor | True | 120 | 6.156 / 7.318 / 13.086 | 0 | 7 | 2 |
| run-1 | 10000 | ancestor | False | 60 | 6.013 / 9.520 / 10.107 | 0 | 0 | 0 |
| run-1 | 10000 | ancestor | False | 120 | 4.879 / 6.270 / 10.457 | 0 | 5 | 1 |
| run-1 | 10000 | ancestor | True | 60 | 6.824 / 8.038 / 10.677 | 0 | 0 | 0 |
| run-1 | 10000 | ancestor | True | 120 | 6.973 / 7.990 / 9.884 | 0 | 7 | 1 |
| run-1 | 10000 | independent | False | 60 | 19.640 / 23.428 / 27.559 | 0 | 113 | 22 |
| run-1 | 10000 | independent | False | 120 | 24.506 / 27.221 / 28.505 | 557 | 642 | 1199 |
| run-1 | 10000 | independent | True | 60 | 34.760 / 37.939 / 41.758 | 44 | 555 | 599 |
| run-1 | 10000 | independent | True | 120 | 32.682 / 38.803 / 45.448 | 685 | 514 | 1199 |
| run-2 | 1000 | ancestor | False | 60 | 5.443 / 7.746 / 8.370 | 0 | 0 | 0 |
| run-2 | 1000 | ancestor | False | 120 | 5.594 / 8.032 / 8.676 | 0 | 6 | 1 |
| run-2 | 1000 | ancestor | True | 60 | 7.749 / 12.098 / 15.597 | 0 | 0 | 0 |
| run-2 | 1000 | ancestor | True | 120 | 5.249 / 7.153 / 7.790 | 0 | 0 | 0 |
| run-2 | 10000 | ancestor | False | 60 | 6.305 / 9.266 / 10.238 | 0 | 0 | 0 |
| run-2 | 10000 | ancestor | False | 120 | 4.881 / 6.169 / 10.461 | 0 | 6 | 1 |
| run-2 | 10000 | ancestor | True | 60 | 7.296 / 8.736 / 9.434 | 0 | 0 | 0 |
| run-2 | 10000 | ancestor | True | 120 | 7.084 / 8.187 / 17.657 | 1 | 10 | 5 |
| run-2 | 10000 | independent | False | 60 | 21.874 / 27.234 / 34.401 | 2 | 150 | 39 |
| run-2 | 10000 | independent | False | 120 | 25.224 / 27.695 / 30.098 | 565 | 634 | 1199 |
| run-2 | 10000 | independent | True | 60 | 35.007 / 37.866 / 45.067 | 53 | 545 | 512 |
| run-2 | 10000 | independent | True | 120 | 25.589 / 28.243 / 33.852 | 581 | 618 | 1199 |

Both processes keep stable owning sources and no sampled compiler/benchmark conflicts; all cadence pixels match. Dense/effects 120 Hz p99 is 38.80 / 28.24 ms, max 45.45 / 33.85 ms, with 1,199 consecutive unsatisfied target ticks in each. The first run loses to the preceding historical journal result (33.87 ms p99); the second wins against its historical 32.59 ms. These sequential historical runs are not causal A/B. Dense/effects 60 Hz p99 remains 37.94 / 37.87 ms; both 60 and 120 Hz still fail. Ancestor/effects 120 Hz p99 7.99 / 8.19 ms hides 7/10 consumed-late ticks, one superseded tick and a 17.66 ms second-run maximum. Small/effects run 2 has zero 120 Hz misses, but run 1 has seven and a 13.09 ms maximum. No stable no-stall claim follows.

## Same-sample owner budget

The analyzer sums commit/publication/submission inside each raw row before deriving percentiles. It does not add phase percentiles. Submission may block on the driver; owner spans are not pure CPU instruction time. Post-submit residual includes finish/queue/scheduling and is not GPU timestamp data. Superseded ticks remain separate unsatisfied work.

| Run | Dense effects Hz | Owner p50 / p99 / max ms | Owner span over budget | Already late before finish |
| --- | ---: | ---: | ---: | ---: |
| run-1 | 60 | 12.131 / 15.162 / 16.895 | 1 | 418 |
| run-1 | 120 | 12.560 / 23.711 / 30.363 | 514 | 514 |
| run-2 | 60 | 12.367 / 17.912 / 30.110 | 8 | 416 |
| run-2 | 120 | 12.328 / 15.729 / 19.247 | 618 | 618 |

The immutable local API removes ancestor fanout, but dense independent updates still require bounded owner work. A faster desktop GPU cannot alone establish 120 Hz from these host results; backend submission can also include driver waits. CPU software jobs must execute outside lossless owner transactions with explicit cancellation/yield and complete-frame publication; that implementation remains open. Reproduce with `python tooling/rendering-perf/analyze-frame-budget.py <cadence-directory>`.

## Correctness regression

154 core tests and GPU benchmark/core Clippy `-D warnings` pass. The release hardware matrix passes 2,013 scenarios / 1,921 pixel comparisons, including both new dense permuted/overlap/overflow/retry cases. The ordinary example passes 11 tests with one hardware-only test ignored. See `regression.json` for exact release/validation hashes. Current native and pipelined results follow below.

### Correlated longest stalls

Each row below is one actual maximum completion sample; phases belong to that same frame. They are not separate phase percentiles.

| Run | Dense effects Hz | Tick | Source to owner ms | Commit ms | Publication ms | Submit ms | Finish residual ms | Source to complete ms |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| run-1 | 60 | 100 | 13.275 | 5.478 | 2.690 | 8.366 | 11.949 | 41.758 |
| run-1 | 120 | 1169 | 3.948 | 5.358 | 3.987 | 21.018 | 11.137 | 45.448 |
| run-2 | 60 | 10 | 14.522 | 4.557 | 1.512 | 6.433 | 18.043 | 45.067 |
| run-2 | 120 | 191 | 3.277 | 5.229 | 3.122 | 9.758 | 12.466 | 33.852 |

## Source identity boundary

Compared with the journal-stage frozen manifest, exactly four hashed owning sources differ: backend `spatial.rs`, `spatial_index.rs`, its core tests and the hardware `retained_spatial_index.rs` cases. Dense timing harness and all shaders retain their exact previous bytes. Analysis scripts/docs change separately; source manifests hash renderer/benchmark ownership and the runners, not every documentation or analyzer file. Analysis versions are committed with these results. No source mutation or sampled compiler conflict occurred during formal measurements.

Both exact binary modes retain identical per-case upload/draw/raster counters: ancestor updates upload 96 spatial bytes, independent updates 960,000 spatial bytes, neither uploads Quad geometry. Effects still rasterize all ten full-viewport isolated layers and reuse zero on both workloads. The optimization changes validation cost, not raster policy. Analyzer nearest-rank distributions reproduce every original cadence phase distribution exactly; the pipelined analyzer also passes the preceding 24-case frozen regression capture.

## Pipelined completion and owner observation

Two processes each cover 24 ten-second cases, with before/after-construction admission order reversed in process 2. Sources stay stable and no compiler/benchmark conflict is sampled. Every case passes final reference pixels, bounded flights and full lease retirement. All 48 cases have zero capacity waits and zero layer-budget waits; this load cannot demonstrate a capacity-gate performance benefit or deterministic normal-host saturation.

| Run | Dense effects Hz | Admit before work | Completion upper bound p99 / max ms | Owner completion observation p99 ms | Additional receipt handling p99 ms | Owner build/submit p50 / p99 ms | Superseded / late / longest burst |
| --- | ---: | --- | ---: | ---: | ---: | ---: | ---: |
| run-1 | 60 | False | 25.476 / 27.672 | 33.008 | 14.113 | 12.274 / 15.628 | 0 / 573 / 104 |
| run-1 | 60 | True | 25.862 / 27.764 | 33.459 | 15.140 | 12.680 / 16.162 | 0 / 595 / 351 |
| run-1 | 120 | False | 27.850 / 29.813 | 38.625 | 14.213 | 12.537 / 17.029 | 420 / 779 / 1199 |
| run-1 | 120 | True | 27.472 / 30.878 | 39.589 | 14.145 | 12.658 / 16.927 | 432 / 767 / 1199 |
| run-2 | 60 | True | 26.173 / 37.947 | 33.644 | 14.331 | 12.164 / 16.214 | 0 / 569 / 187 |
| run-2 | 60 | False | 25.881 / 27.151 | 33.494 | 14.408 | 13.005 / 16.517 | 0 / 595 / 265 |
| run-2 | 120 | True | 29.359 / 31.737 | 39.289 | 13.098 | 12.239 / 16.801 | 408 / 791 / 1199 |
| run-2 | 120 | False | 27.366 / 33.571 | 37.825 | 13.726 | 12.338 / 16.431 | 407 / 792 / 1199 |

Dense/effects 120 Hz completion upper-bound p99 spans 27.37–29.36 ms with 1,199 consecutive unsatisfied ticks. Before-work admission loses against after-work p99 in the second process (29.36 vs 27.37 ms) despite winning slightly in the first (27.47 vs 27.85 ms); zero waits prevent interpreting these deltas as a gating gain. Receipt timestamps coalesce and upper-bound earlier completions; owner observation adds scheduling delay and is not display completion. Native export/Present/display/input are excluded. All raw samples and correlated worst owner observations are preserved.

## Native regression and remaining acceptance

The current desktop release passes hidden native delivery (1,000 messages to one wake/transaction/render; no new frame needed for lease retirement), 97,792 shared framebuffer pixels and 3,072 replacement-device recovery pixels while retaining an old immutable snapshot. Exact metadata is in `verification.json`. Hidden manual draws and native Present call acceptance do not establish visible sustained presentation or refresh-paced input latency.

Remaining: reduce owner dense work below budget, implement and qualify placement-independent effect rasters, native visible modern desktop 60/120 Hz including resource/resize/cold/burst spikes, general local hit/damage paths and primitives, actual software rendering with capability negotiation and cooperative jobs, physical/global resource budgets and cross-platform acceptance. Full RFC goal stays active.
