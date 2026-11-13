# Local layer bounds integration and losing dense cases

Status: experimental default-off index. Complete RFC goal active.

The local snapshot consumer can now explicitly enable tight current ink bounds and the backend integer-translation cache probe. Normal local submission keeps full-viewport layers because the dense regression below rejects default enable. See [API decision](../../ADR_RETAINED_LOCAL_LAYER_BOUNDS.md).

## Local overdraw cadence

Two independent processes, 10 seconds per case, 128 overlapping 900x650 quads in one moving isolated group, 1000x760 surface, unit scale. Both modes use the new tight index; `enabled` selects backend translation reuse. Independent absolute-clock arrivals continue during owner/driver work. Coalesced samples remain unsatisfied; completion includes source wake/owner/submission/GPU finish, not native display/input.

| Run | Hz | Translation reuse | On time / arrivals | p50 / p95 / p99 / max ms | Coalesced | Longest unsatisfied burst |
| --- | ---: | --- | ---: | ---: | ---: | ---: |
| run-1 | 60 | False | 600 / 600 | 6.88 / 8.41 / 9.56 / 9.80 | 0 | 0 |
| run-1 | 60 | True | 600 / 600 | 1.80 / 5.69 / 7.35 / 13.15 | 0 | 0 |
| run-1 | 120 | True | 1190 / 1200 | 1.96 / 5.51 / 8.00 / 11.34 | 0 | 2 |
| run-1 | 120 | False | 1132 / 1200 | 6.70 / 8.08 / 10.60 / 39.82 | 14 | 23 |
| run-2 | 60 | False | 600 / 600 | 7.24 / 8.13 / 8.51 / 9.10 | 0 | 0 |
| run-2 | 60 | True | 600 / 600 | 1.62 / 4.11 / 7.58 / 10.41 | 0 | 0 |
| run-2 | 120 | True | 1164 / 1200 | 1.93 / 6.34 / 10.66 / 17.99 | 5 | 4 |
| run-2 | 120 | False | 1157 / 1200 | 6.69 / 7.68 / 15.29 / 25.99 | 7 | 30 |

Reuse reduces median and p99 in both 120 Hz captures, but on-time improvement is small in the second process (1,164 versus 1,157) and the optimized p99 still exceeds 8.333 ms. At 60 Hz, optimized max worsens in both processes. These losses remain raw evidence; stable 120 fps is not achieved.

## 10,000-node cost control: default enable rejected

Same executable, alternating full-viewport then tight index, then tight index then full-viewport; backend translation reuse is off. Each case/mode retains 120 samples and full pixels. Captures have stable source manifests and sequential compiler-free execution, but no periodic process conflict sampler. They are exploratory cost evidence.

| Round | Effects | Independent updates | Submit p50 reduction | Completed p50 reduction |
| --- | --- | --- | ---: | ---: |
| 1 | False | False | -1.5% | -46.0% |
| 1 | False | True | -17.8% | -13.4% |
| 1 | True | False | -174.8% | -105.7% |
| 1 | True | True | -76.5% | -44.0% |
| 2 | False | False | -8.3% | -49.0% |
| 2 | False | True | 0.6% | -11.3% |
| 2 | True | False | -266.5% | -165.5% |
| 2 | True | True | -63.4% | -35.0% |

Negative reduction is slower. Both ancestor/effects and dense-independent/effects regress substantially; tight target bounds do not compensate for resolving descendant ink. Preserve the full-viewport default and require explicit experimental enable. This changes the next API work: persistent local-content bounds and separately proven placement/clip eligibility, rather than mandatory per-descendant world geometry merely to select a target.

The first captures preceded the independent default-off switch. `dense-ab/source-manifest.json` and its executable hash identify them. After gating, reproduce tight mode with `--local-tight-layers`; control uses default local/full-viewport output. Existing resolved controls retain tight bounds unless `--full-viewport-layers` is selected.

## Verification

160 core tests, strict Clippy, 72 mixed local hardware comparisons, 28 focused local-layer comparisons per process, and full 2,015-scenario / 1,923-pixel hardware regression passed before gating. The final gated rerun is recorded separately in verification metadata. The 72 mixed controls include text/images, deep anchors, fractional motion/scale, skipped bases and reset. The index never changes the snapshot used by hit testing; tests compare old and current geometry independently.

```powershell
cargo +1.98.1 build --release --locked --offline -p gpui-box-rendering-perf --features retained-gpu --bin retained-runtime-gpu --bin dense-runtime-perf
target/release/retained-runtime-gpu.exe target/local-layer-control --local-layer-translation-only
target/release/retained-runtime-gpu.exe target/local-mixed-tight --local-only --local-tight-layers
target/release/dense-runtime-perf.exe target/dense-full-viewport --local --paired-only
target/release/dense-runtime-perf.exe target/dense-tight --local --paired-only --local-tight-layers
```

No modern desktop visible 120 Hz, physical input latency, generalized local-space resource cache, CPU native integration or complete RFC acceptance is claimed.

## Final gated rerun (including losing result)

- Reuse=True: 1180/1200 on time; p99 10.26 ms; max 18.68 ms; coalesced 1; longest unsatisfied burst 3.
- Reuse=False: 1182/1200 on time; p99 9.07 ms; max 12.11 ms; coalesced 0; longest unsatisfied burst 13.

The final optimized run completes fewer source ticks on time (1,180 vs 1,182). The small differences do not support a consistent deadline win. Default-off gating and full raw failures remain essential; the measured target is not achieved. Final hashes are separate from earlier un-gated study identities.

Final gated checks pass: 160 core tests, strict GPU/dense Clippy, 28 focused local-layer and 72 mixed local comparisons, and 2,015 hardware scenarios / 1,923 pixel comparisons.
