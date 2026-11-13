# Integer translation raster reuse probe

Status: experimental, default off. Overall goal active.

Two independent exact-executable processes compare original raster and backend-verified translation reuse. Each has alternating old/new/new/old paired runs (20 measured updates each), plus ten-second sustained cases at 60/120 Hz with mode order reversed at 120 Hz. Workload: 128 overlapping 900x650 solid quads in one isolated-opacity layer, 1000x760 target, integer ancestor motion, device scale 1, resolved snapshot route. Host: Ryzen 4800H / GTX 1650 Ti, Vulkan. This is a deliberately raster-heavy opportunity case; it does not establish dense-independent-child or modern desktop acceptance.

## Sustained evidence

| Run | Hz | Reuse | On time / source | Completion p50 / p95 / p99 / max ms | Coalesced | Longest failing burst |
| --- | ---: | --- | ---: | ---: | ---: | ---: |
| run-1 | 60 | False | 600 / 600 | 7.05 / 8.18 / 8.85 / 11.49 | 0 | 0 |
| run-1 | 60 | True | 600 / 600 | 1.98 / 5.78 / 8.43 / 11.22 | 0 | 0 |
| run-1 | 120 | True | 1185 / 1200 | 1.88 / 5.71 / 8.70 / 11.00 | 0 | 2 |
| run-1 | 120 | False | 1134 / 1200 | 6.73 / 8.28 / 16.48 / 25.94 | 11 | 7 |
| run-2 | 60 | False | 600 / 600 | 6.95 / 7.89 / 8.89 / 13.04 | 0 | 0 |
| run-2 | 60 | True | 600 / 600 | 1.77 / 4.86 / 8.09 / 10.87 | 0 | 0 |
| run-2 | 120 | True | 1187 / 1200 | 1.75 / 5.27 / 8.39 / 10.50 | 0 | 2 |
| run-2 | 120 | False | 1143 / 1200 | 6.85 / 8.01 / 15.07 / 30.80 | 12 | 33 |

Both 120 Hz optimized runs reduce completion p99 from 15.07–16.48 ms to 8.39–8.70 ms and max from 25.94–30.80 ms to 10.50–11.00 ms. On-time counts improve from 1,134/1,143 to 1,185/1,187 of 1,200 source ticks. Both still miss deadlines and fail stable 120 fps acceptance. Source wake/scheduler delay remains included; GPU timestamps are not recorded. All 60 Hz cases meet their deadlines, but the optimized 60 Hz first-run p99 improves only slightly (8.85 -> 8.43 ms). Keep the losing/weak tail evidence.

Independent worker arrivals continue while owner/driver/GPU work blocks. Every source tick is either consumed or coalesced; coalesced ticks remain unsatisfied. Samples contain source-to-owner, owner+submit and source-to-synchronous-completion for the same frame. No native surface upload/Present, compositor, display or physical input latency is measured. The complete source-to-present objective remains open.

## Paired evidence

| Run | Round | Reuse | Owner+submit p50 ms | Completed p50 / p99 ms | Raster layers / 20 |
| --- | ---: | --- | ---: | ---: | ---: |
| run-1 | 0 | False | 0.562 | 8.503 / 9.300 | 20 |
| run-1 | 0 | True | 0.457 | 0.631 / 1.726 | 0 |
| run-1 | 1 | False | 0.570 | 6.229 / 6.839 | 20 |
| run-1 | 1 | True | 0.420 | 0.645 / 0.856 | 0 |
| run-2 | 0 | False | 0.572 | 9.300 / 27.951 | 20 |
| run-2 | 0 | True | 0.333 | 0.507 / 1.668 | 0 |
| run-2 | 1 | False | 0.604 | 6.628 / 8.336 | 20 |
| run-2 | 1 | True | 0.566 | 0.780 / 0.963 | 0 |

Raster reuse eliminates 588,104 layer pixels per qualifying update, while root composition still runs. Exact record proof adds CPU/backend work; paired submission losses are preserved above. This motivates separating immutable local content epochs from placement, but not a caller-trusted cache version or enabling an unbounded full scan by default.

## Exactness and scope

61 pixel comparisons per original expanded process cover positive/negative motion for shared/world records, rounded border and box shadow, fractional fallback, independent child/material change, relevant external clips, viewport edges, unsupported nested/image/text paths, non-unit-scale fallback, group opacity and reset. Compare against a fresh full-size renderer with no old cache. Camera-only rebasing with unchanged world content remains original-raster behavior. Additional multi-flight regression is recorded separately after the first two captures. Source/binary identity changes for that added harness are explicit; no runtime algorithm changes are attributed to its new capture.

The prototype requires equal complete normalized content bytes, equal extent, integer target movement and valid prior content. It is off by default and excludes nested/resource draws, fractional primitive geometry, shared non-unit scale, patterns and proofs above 4,096 records. CPU proof bytes are separately bounded and excluded from the GPU texture payload budget. See [decision and limitations](../../ADR_RETAINED_LAYER_TRANSLATION.md).

## Reproduction

```powershell
cargo +1.98.1 build --release --locked --offline -p gpui-box-rendering-perf --features retained-gpu --bin retained-runtime-gpu
target/release/retained-runtime-gpu.exe target/layer-translation-run --layer-translation-only
python tooling/rendering-perf/analyze-layer-translation.py docs/performance/2026-11-13-layer-translation
```

The exact initial manifests/captures are retained. Full hardware regression, core tests and strict Clippy are listed in verification metadata. Native modern desktop qualification, general local-space effects/resources, dense owner costs and CPU fallback integration remain outstanding.

The final harness adds a 12-submit no-readback/no-finish queue-order case; 62 targeted pixel controls pass. `run-3-final/` preserves its additional paired/cadence capture without replacing the initial two processes. Final source hashes and binary are recorded separately in the manifest.

Final additional 120 Hz process: reuse=True: on-time 1185/1200, p99 8.76 ms, max 11.01 ms, coalesced 0, longest unsatisfied burst 2; reuse=False: on-time 1130/1200, p99 15.60 ms, max 58.60 ms, coalesced 21, longest unsatisfied burst 21; this additional process also fails stable 120 fps acceptance.
