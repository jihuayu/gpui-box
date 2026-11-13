# Lazy integer ink translation evidence

Status: experimental local tight index remains default off; complete RFC goal active.

## Paired same-executable proof control

Two alternating rounds use the same executable with `--no-ink-translation` for the control. Both enable experimental tight local bounds; backend raster translation reuse is off. Each mode/case retains 120 samples and full reference pixels. New integer fixture fits 10,000 leaves and shadows within viewport margins, uses a 680x480 parent at 60,60, group 42px spacing and +/-2 motion. This fixture is intentionally eligible; it must not replace or be directly compared to earlier fractional fixtures.

| Round | Effects | Independent | Submit p50 reduction | Completed p50 reduction | Resolved / translated ink nodes |
| --- | --- | --- | ---: | ---: | ---: |
| 1 | False | False | 5.7% | 7.1% | 0 / 0 |
| 1 | False | True | -5.4% | 2.8% | 0 / 0 |
| 1 | True | False | 56.1% | 60.7% | 0 / 10011 |
| 1 | True | True | -4.2% | 2.2% | 10000 / 0 |
| 2 | False | False | 10.4% | 7.2% | 0 / 0 |
| 2 | False | True | -5.4% | -3.7% | 0 / 0 |
| 2 | True | False | 69.8% | 51.5% | 0 / 10011 |
| 2 | True | True | -7.7% | -9.2% | 10000 / 0 |

Ancestor/effects skips 10,011 per-item ink resolutions. Dense independently changing leaves cannot qualify; submit loses 4.2/7.7%, second completed median loses 9.2%. The original full-viewport default is unchanged. Additional raw/exact/lazy index arrays consume CPU memory and cold preparation; no full memory admission claim.

## Sustained independent source arrivals

| Round | Mode | Hz | p50 / p95 / p99 / max ms | Superseded | Consumed late | Longest unsatisfied burst |
| --- | --- | ---: | ---: | ---: | ---: | ---: |
| 1 | control | 60 | 5.62 / 10.17 / 15.24 / 19.96 | 0 | 5 | 2 |
| 1 | control | 120 | 5.39 / 8.44 / 13.49 / 20.04 | 2 | 62 | 10 |
| 1 | optimized | 60 | 2.83 / 5.44 / 9.79 / 15.10 | 0 | 0 | 0 |
| 1 | optimized | 120 | 2.74 / 5.40 / 14.86 / 30.70 | 9 | 18 | 10 |
| 2 | control | 60 | 6.33 / 11.50 / 13.78 / 18.48 | 0 | 1 | 1 |
| 2 | control | 120 | 6.69 / 12.27 / 15.75 / 21.81 | 3 | 331 | 13 |
| 2 | optimized | 60 | 2.41 / 4.49 / 8.10 / 18.22 | 0 | 1 | 1 |
| 2 | optimized | 120 | 3.66 / 7.11 / 10.38 / 15.60 | 0 | 30 | 4 |

At 120Hz consumed+superseded deadline losses fall from 64/334 to 27/30. But first-round optimized p99 and max worsen to 14.86/30.70ms vs 13.49/20.04ms. Second-round optimized p99/max improve to 10.38/15.60ms vs 15.75/21.81ms. The algorithm reduces routine work, not all spikes; stable 120fps is not achieved. First 60Hz control loses 5 deadlines; optimized has 0, then second optimized loses1. Do not omit these failures.

Independent worker input continues during owner/driver/GPU work. Source scheduling delay, ingress, reactive transaction, local publication, queue, submit and synchronous completion are included. Native window upload, compositor/visible present and physical input are excluded. Sources were byte-stable and commands sequential; no periodic process conflict sampler was used, so captures are exploratory local-host evidence rather than sampler-qualified acceptance.

## Exactness and controls

Private snapshot provenance proves one changed root translation with unchanged descendants. Conservative uncut integer geometry must fit inherited clip at both positions; fractional geometry, resources, clip/scroll/opacity edits and dense frontiers fall back. Lazy offsets are pushed before partial edits.161 core tests, strict Clippy, 28 targeted local comparisons, 72 mixed text/image/deep/fractional controls and the 2,015-scenario/1,923-pixel full hardware suite pass. See [decision](../../ADR_RETAINED_INK_TRANSLATION.md).

```powershell
cargo +1.98.1 build --release --locked --offline -p gpui-box-rendering-perf --features retained-gpu --bin dense-runtime-perf --bin retained-runtime-gpu
target/release/dense-runtime-perf.exe target/ink-control --local --paired-only --local-tight-layers --integer-motion --no-ink-translation
target/release/dense-runtime-perf.exe target/ink-optimized --local --paired-only --local-tight-layers --integer-motion
target/release/dense-runtime-perf.exe target/ink-cadence --local --cadence-only --ink-cadence-only --local-tight-layers --integer-motion
```

Raw data, exact source manifests and binary hashes are retained. Hardware is Ryzen 4800H/GTX 1650 Ti Vulkan, not modern desktop qualification. General fractional/local content, resource epochs, dense independence, native visible 120 Hz, input latency and CPU fallback integration remain open.
