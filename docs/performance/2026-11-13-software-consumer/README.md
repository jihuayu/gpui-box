# Experimental CPU consumer evidence

Status: actual CPU raster prototype, not native fallback acceptance. Complete RFC goal active.

Two independent five-second captures per case use prepared immutable packets, a 320x180 surface, 100 independently moving boxes and (when enabled) 10 isolated rounded groups. This intentionally small workload validates scheduling contracts; it is not desktop resolution throughput qualification. An independent source produces 60/120 Hz visual/probe arrivals. The consumer finishes active work, retains only the latest pending sample, and services synthetic probes between work slices. Full and 512-unit paths use the same CPU raster kernel. Source/coalesced/deadline counts include all ticks; oldest unhandled probe timestamps survive coalescing. No native input, owner snapshot building, window upload or present is measured.

## Effects cases

| Run | Hz | Work quota | On-time / arrivals | Completion p99 ms | Probe p99 ms | Slice p99 / max ms | Longest unsatisfied burst |
| --- | ---: | --- | ---: | ---: | ---: | ---: | ---: |
| run-1 | 60 | complete | 289 / 300 | 25.07 | 8.51 | 19.988 / 29.111 | 5 |
| run-1 | 60 | 512 | 300 / 300 | 13.13 | 1.31 | 0.021 / 1.109 | 0 |
| run-1 | 120 | 512 | 136 / 600 | 18.31 | 1.27 | 0.019 / 0.872 | 78 |
| run-1 | 120 | complete | 242 / 600 | 15.24 | 7.02 | 10.853 / 12.657 | 18 |
| run-2 | 60 | complete | 300 / 300 | 13.08 | 2.13 | 11.603 / 12.070 | 0 |
| run-2 | 60 | 512 | 300 / 300 | 10.80 | 1.29 | 0.014 / 0.782 | 0 |
| run-2 | 120 | 512 | 188 / 600 | 17.67 | 1.30 | 0.018 / 0.850 | 85 |
| run-2 | 120 | complete | 277 / 600 | 14.39 | 6.08 | 10.830 / 12.644 | 35 |

Small slices reduce effects/120 synthetic probe p99 to 1.27–1.30 ms versus 6.08–7.02 ms, while on-time completion falls to 136/600 and 188/600 versus 242/600 and 277/600. This is responsiveness/throughput evidence, not stable 120 fps. No-effects cases meet all 600 deadlines except one complete-frame run missing one. A lower CPU visual cadence and tile replay/indexing improvements are required; do not cancel every active frame on arrival and starve publication. Quotas are operation counts, not wall-clock guarantees; worker init/retirement tails remain in `summary.json`.

## Pixel qualification

24 real CPU/WGPU comparisons cover rectangles, nested isolated opacity, nearest 2x2 mixed-alpha images, rounded borders, fractional placement/clip and real grayscale text at scales 1, 1.25, 1.5 and 2. The 16 asserted rectangle/group/image/text controls pass with channel tolerance 1. Rounded cases also pass those captures, but richer fractional decoration is not qualified. Fractional cases retain differences at scales 1 and 2 (13/26 pixels, max channel error 128); no tolerance widening or production exactness claim. Raw result is `pixel-comparisons.json`; CPU/GPU PNGs remain locally at `target/software-raster-check-final/`. GPU readback is validation-only.

## Reproduction

```powershell
cargo +1.98.1 build --release --locked --offline -p gpui-box-rendering-perf --features retained-gpu --bin software-runtime-perf --bin retained-runtime-gpu
target/release/retained-runtime-gpu.exe target/software-raster-check-final --software-only
target/release/software-runtime-perf.exe target/software-cadence/run-1 5
target/release/software-runtime-perf.exe target/software-cadence/run-2 5
python tooling/rendering-perf/analyze-software.py target/software-cadence
```

See [software API decision](../../ADR_RETAINED_SOFTWARE_CONSUMER.md). Frozen source hashes and exact executables are in `manifest.json`; raw samples include losing cases. Current host is Ryzen 4800H / GTX 1650 Ti. Native CPU integration, input/visible presentation, pressure/cancellation tails, fractional fidelity and modern desktop GPU effects/120 qualification remain open.

Final checks: 159 locked core tests, strict Clippy for both harnesses, and the existing full hardware suite (2,015 scenarios / 1,923 pixel comparisons) pass. See `verification.json`.
