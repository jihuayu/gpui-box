# Independent material/placement burst evidence

Status: general prototype, full goal active. This change separates dependency
lifetimes; no integer-specific runtime path, public batching authority or new
optimization switch. The measurement fixture remains the previous10k integer
ancestor/effects scene, for isolation from earlier changes. Other geometry is
covered by correctness controls, not performance qualification.

## Captures

Two exact executables, reversed order each round. Baseline is rebuilt from
`ac55a59` owning sources with unchanged lock/harness; candidate has independent
local material pages and consumer writes. Sources restored byte-for-byte after
baseline build. Ten seconds at60Hz and120Hz per process, independent absolute
source, bounded inbox, computed equality, actual atomic10k typed color changes
every half second, local queue, GPU submit and synchronous finish. All nineteen
dense epochs survive coalescing, every source tick is accounted, final pixels
match a fresh renderer. Device: GTX1650Ti Vulkan; CPU: Ryzen4800H.

`initial/` retains the first pair before weak owner identity and validation
statistics/predecessor hardening. `final/` uses final runtime sources, including
weak owner identity and early predecessor check. Both manifests retain exact
source/binary hashes, commands, process observations and stability checks.
No known compiler/other renderer conflict was observed in250ms samples. This
does not prove host/driver/thermal isolation. Large JSON is losslessly gzip
compressed; all raw phases, recovery samples and losing tails remain.

## Final exact-source pairs

| Round | Hz | Original p99 / max ms | Material pages p99 / max ms | Original / new unsatisfied ticks | Original / new longest unsatisfied ticks |
| --- | ---: | ---: | ---: | ---: | ---: |
|1 |60 |40.97 /48.13 |26.20 /31.75 |38 /25 |2 /3 |
|1 |120 |37.04 /52.91 |22.56 /28.60 |105 /65 |7 /3 |
|2 |60 |65.89 /85.40 |24.85 /30.93 |69 /23 |5 /2 |
|2 |120 |45.52 /72.32 |23.05 /34.34 |164 /62 |11 /4 |

First60Hz longest burst worsens2 ->3 despite fewer total losses. The second
baseline is slower across phases; do not attribute all cross-process variation
to the change. P99 and raw dense medians improve in both reversed pairs, but
every dense change remains over8.333ms. Stable120fps is not achieved.

Dense-frame phase medians in ms (same selected dense rows):

| Round | Hz | Original / new publication | Original / new submission | Original / new owner span | Original / new source completion |
| --- | ---: | ---: | ---: | ---: | ---: |
|1 |60 |13.80 /6.22 |16.44 /10.49 |35.52 /21.00 |37.11 /22.85 |
|1 |120 |13.48 /6.08 |16.14 /10.67 |35.86 /21.89 |38.83 /22.75 |
|2 |60 |21.71 /6.78 |26.69 /11.10 |58.02 /23.25 |58.96 /24.17 |
|2 |120 |19.64 /7.36 |19.55 /11.00 |46.46 /22.77 |47.15 /23.47 |

Owner span is the sum of commit/publication/submission for each row before taking
its distribution. Table phase medians must not be added to synthesize a frame.
Submission may include driver waits; post-submit residual is finish/scheduling,
not GPU execution time. No native present, display or physical input is measured.

## Initial pair and losing result

Initial120Hz original/new p99:34.79 ->22.10ms and35.26 ->22.61ms; max43.61
->30.81ms and44.48 ->39.19ms; losses134 ->84 and98 ->95. The second longest
unsatisfied burst worsens5 ->6. Initial60Hz p99:35.95 ->21.37ms and47.11
->22.68ms; losses40 ->20 and43 ->21. This evidence is retained independently,
not mixed with final-source captures to increase sample count.

## Correctness and ownership

164core tests pass,11ordinary example tests pass,1hardware click test is ignored
by that ordinary example command. Strict core Clippy passes both with/without
retained-text, and GPU harness Clippy passes.72mixed local GPU comparisons and
2,015full hardware scenarios /1,923pixel comparisons pass on final source.
Compressed full/mixed records and verification counters are included here.

Mixed tests cover scales1/1.25/1.5, nested opacity, fractional clip/scroll, deep
spatial anchors, simultaneous color/radius/border/shadow/text-color changes,
same-size image replacement, shadow count/inset topology fallback, skipped
material snapshots, device reset, owner drop and same-revision content rejection.
Pure material updates assert zero spatial upload and nonzero primitive upload.
Core tests preserve old snapshots, independent local/resolved subscriber journals,
material/placement updates over many frames and visibility/layout fallback.

`changed-sources.zip` stores final and baseline owning files, including harness
changes. Final cadence manifests identify exact source bytes; files not owned by
this change may appear in broad manifests but are not modified by this work.
Images remain in `target/material-shipping-*`; raw validation is retained here.

## Implication and remaining work

Material, placement and topology should remain separate facts in framework APIs.
Owned immutable provenance authorizes incremental routing; application dirty
hints do not. Existing primitive/resource routing handles changed payloads;
changed draw topology and unsupported transitions retain full recovery. See
[ADR](../../ADR_RETAINED_MATERIAL_PLACEMENT.md).

Dense commit, material clones/current geometry, resource/dependency scans and
backend submission still consume substantial work. No claim that all updates
are sublinear or120fps is supported. Native visible/pipelined burst, memory
pressure, modern desktop hardware, richer text/image/flex/semantics and CPU native
fallback remain open. CPU prioritizes owner/input responsiveness with completed
images and matching hit geometry. Future optimization should reduce necessary
work and allocation in this common route; candidates without stable benefit may
be rejected rather than adding scene-specific caches or quality exceptions.

```powershell
# Preserve exact baseline first at target/material-baseline/dense-runtime-perf.exe.
cargo +1.98.1 build --release --locked --offline -p gpui-box-rendering-perf --features retained-gpu --bin dense-runtime-perf --bin retained-runtime-gpu
python tooling/rendering-perf/capture-burst.py target/material-new --material-pair
python tooling/rendering-perf/analyze-burst.py target/material-new
python tooling/rendering-perf/analyze-frame-budget.py target/material-new
target/release/retained-runtime-gpu.exe target/material-mixed --local-only --local-tight-layers
target/release/retained-runtime-gpu.exe target/material-full --validation-only
```
