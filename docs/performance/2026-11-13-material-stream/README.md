# Consume material geometry without a full temporary scene

Status: general working-set reduction, no120fps or stall-elimination claim.
Full requested rendering goal remains active.

## Change and contract

The consumer previously cloned resolved changes into a wide `(index, PaintItem)`
array, then traversed it to classify/build GPU records. It now validates,
classifies and prepares each item in the geometry visitor's stack-owned lifetime.
Only changed image/glyph inputs stay owned for existing resource batch builders.
This removes an O(changed items) full-payload array and Arc cloning for ordinary
primitives. Geometry traversal, arena copies and GPU validation remain. No new
runtime switch, integer restriction or public batch authority is introduced.

A late validation failure can leave private CPU arenas partly prepared, but the
existing failed flag requires reset: no GPU submit or acknowledgement occurs.
Previous complete pixels stay intact. A regression establishes an exact local
predecessor, changes an earlier quad and a later wrong-scale glyph resource,
then checks rejection, unchanged image, failed retry and correct reset/recovery
at scales1/1.25/1.5. No incomplete scene reaches GPU publication.

## Exact reversed pairs

Baseline is the exact487f9dd executable. Same existing10k ancestor/effects fixture,
800x600 scale1, persistent10k color changes every half second. Each process runs
ten seconds at60Hz and120Hz: independent absolute source, bounded inbox, atomic
reactive transaction, local queue and synchronous GPU completion. All nineteen
dense epochs and source ticks are accounted, final pixels match fresh rendering.
Known compiler/renderer sampler observes zero conflicts at250ms, which does not
establish host/GPU/driver/thermal isolation.

| Round | Hz | Original p99 / max ms | Streamed p99 / max ms | Original / streamed losses | Original / streamed longest unsatisfied ticks |
| --- | ---: | ---: | ---: | ---: | ---: |
|1 |60 |25.63 /38.90 |29.07 /41.85 |22 /26 |2 /3 |
|1 |120 |25.63 /56.22 |21.15 /42.36 |163 /60 |9 /14 |
|2 |60 |28.19 /39.99 |24.54 /32.42 |24 /20 |2 /2 |
|2 |120 |26.33 /42.81 |20.42 /32.69 |90 /64 |6 /5 |

Dense submit medians at120Hz fall13.88 ->9.32ms and12.61 ->8.54ms; owner spans
fall25.47 ->19.88ms and26.42 ->20.43ms. First60Hz submit loses10.89 ->11.73ms,
second improves12.02 ->9.35ms. First120Hz longest unsatisfied interval worsens
9 ->14 despite fewer total losses and better p99/max. Preserve those regressions.
Every dense burst remains over8.333ms. Less copying does not ensure responsiveness.

`cadence/` retains raw phases, same-row analysis, recovery records, exact commands,
hashes and process observations. Large JSON uses lossless gzip. Submission can
include driver waits; finish residual is not GPU execution time. Ryzen4800H /
GTX1650Ti Vulkan is a regression host, not modern desktop qualification. Native
input, present and visible display remain unmeasured.

## Validation scope

164core tests, strict core/harness Clippy and75final mixed GPU comparisons pass.
Full suite passes2,018hardware cases /1,926pixel comparisons. Full validation
used the earlier failure-test version; final mixed validation then explicitly
established a local predecessor before injection to strengthen path coverage.
That test-only edit is the sole post-capture Rust source change; runtime bytes
match captures. Final/baseline files are in `changed-sources.zip`. Compressed
validation JSON is retained; screenshots stay in target.

A build overlapped correctness-only full validation, so its timings are not
performance evidence. Windows could not clean a running old build artifact;
validation finished successfully and the stronger test ran afterward. No build
overlapped formal captures. Both observed tails and failures remain archived.

Next priority is total necessary work and bounded owner scheduling, alongside
native display/CPU integration. The removed array is a structural work reduction;
latency results are workload/host-specific. No deadline guarantee or extra cache
policy follows. Candidates without general benefit may be rejected.

```powershell
# Preserve487f9dd at target/material-stream-baseline/dense-runtime-perf.exe.
cargo +1.98.1 build --release --locked --offline -p gpui-box-rendering-perf --features retained-gpu --bin dense-runtime-perf --bin retained-runtime-gpu
python tooling/rendering-perf/capture-burst.py target/stream-new --stream-pair
python tooling/rendering-perf/analyze-burst.py target/stream-new
python tooling/rendering-perf/analyze-frame-budget.py target/stream-new
target/release/retained-runtime-gpu.exe target/stream-mixed --local-only --local-tight-layers
```
