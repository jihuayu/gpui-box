# Sustained burst recovery: repeated work dominates rare material changes

Status: incomplete acceptance. Primary target remains modern desktop GPU at
120 fps with effects; software prioritizes smooth interaction. This host is
Ryzen4800H / GTX1650Ti Vulkan. These are regression/exploratory measurements,
not native visible presentation or modern desktop qualification.

## Method

800x600 logical/physical surface, scale1, 10,000 solid leaves, ten isolated opacity
groups with shadows. An ancestor moves continuously +/-2 in the existing integer
fixture. Every half second all leaf colors change to a distinct opaque color.
The source uses an independent absolute clock and bounded last-value inbox;
the owner executes real computed equality, typed sinks, atomic commit, snapshot,
queue and GPU submit, synchronously finishes and checks final pixels against a
fresh renderer. Nineteen dense changes occur per ten-second rate capture. Color
epoch persists across coalescing; skipped source ticks remain unsatisfied work.
Tick0 is included. No compilation runs during measurements.

`policy/` compares three same-executable modes in reversed order across two
rounds: normal full-viewport local path, experimental tight/lazy bounds, and
tight/lazy plus complete normalized raster translation proof. The last mode is
a losing control, not a new default. `recovery/` compares exact binaries before
and after sharing the owner-resolved snapshot during full local recovery, again
in reversed order. Both recovery modes use normal full-viewport layers and keep
all GPU validation. See [decision](../../ADR_RETAINED_BURST_RECOVERY.md).

Each process contains60/120Hz ten-second cases. Source/phase samples and process
observations are losslessly compressed when large. `sources.json` and manifests
retain byte hashes and exact commands; source manifests include unrelated stable
files for completeness, not ownership of that work. Every run has zero observed
known compiler/renderer conflicts at250ms sampling. This sampler does not prove
absence of other CPU/GPU activity, page faults or driver/thermal variation.
The original source/harness and new owning source are retained in
`changed-sources.zip`; binary hashes differ only for the recovery modification.

## Same-executable policy comparison

120Hz, 1,200 total source ticks per case; p99/max cover consumed samples while
losses additionally count superseded ticks.

| Round | Mode | p99 / max ms | Unsatisfied ticks | Longest consecutive unsatisfied ticks |
| --- | --- | ---: | ---: | ---: |
| 1 | default | 38.60 /43.84 |105 |5 |
| 1 | tight/lazy |40.29 /47.80 |98 |6 |
| 1 | tight/lazy/raster proof |43.98 /86.48 |141 |12 |
| 2 | default |45.76 /49.63 |103 |6 |
| 2 | tight/lazy |41.60 /55.71 |97 |6 |
| 2 | tight/lazy/raster proof |43.96 /49.81 |121 |7 |

Routine raster-proof overhead raises owner medians from about1.2ms to2.8ms.
Dense changes cannot reuse old raster content and still require full recovery.
The experimental modes do not eliminate burst stalls. Default60Hz also has a
135.33ms outlier; it remains in the raw evidence. Do not generalize these integer
fixtures to fractional/resource-rich scenes or silently enable the proof.

## General full-snapshot reuse comparison

The owner already resolved full publication. New code retains those same
immutable paint/property arrays for consumer recovery rather than rebuilding
them again. No integer motion, primitive or scale condition authorizes this
reuse. New placement successors still resolve their own current geometry.

| Round | Hz | Original p99 / max ms | Reuse p99 / max ms | Original / reuse unsatisfied ticks |
| --- | ---: | ---: | ---: | ---: |
|1 |60 |45.56 /57.24 |40.57 /44.95 |40 /38 |
|1 |120 |39.25 /42.37 |37.47 /46.18 |105 /134 |
|2 |60 |43.54 /46.07 |40.16 /42.39 |38 /38 |
|2 |120 |40.05 /50.92 |34.29 /41.09 |91 /88 |

Dense-change submission medians fall20.78 ->16.16ms and19.95 ->16.88ms at60Hz,
19.83 ->16.31ms and20.30 ->15.02ms at120Hz: reductions18–26%. Dense completed
medians fall41.17 ->36.80ms and39.72 ->38.94ms at60Hz,39.73 ->37.67ms and
40.31 ->35.29ms at120Hz. Routine owner medians stay about1.2ms. First120Hz
maximum and losses worsen despite improved dense submission; retain that result.
Worst source ticks before first on-time post-burst recovery:5 ->5 then6 ->4
at120Hz. No120Hz pass, hard owner bound or GPU execution-time claim follows.

Same-row phase analysis separates owner commit/publication/submission from the
remaining finish/scheduling interval; it does not add unrelated phase percentiles.
`summary.json` preserves dense/ordinary groups, nineteen per-epoch recovery records
and correlated worst rows. Recovery means first on-time completion before the
next burst. It is not the time a physical display showed that output.

## Validation and implications

163core tests pass, including512mixed nested updates with empty clips, scroll,
fractional ancestry and later child edits, plus immutable full-cache/placement
successor and material recovery checks. Strict core/harness Clippy passes.
72mixed local GPU comparisons and2,015full hardware cases /1,923pixel comparisons
pass. Full validation records are compressed here; screenshots remain under
`target/burst-validation*`. Both exact binaries pass final reference pixels in
every sustained case.

The general issue is mismatched dependency lifetimes: material, placement and
topology share a full-publication recovery boundary. More raster throughput
cannot remove duplicate owner/consumer work. Next work should preserve those
independent facts and update necessary dependencies through one common route,
without color-only shortcut APIs or unchecked dirty authority. Prefer reducing
work/allocations and retaining validation to accumulating special-case caches.
If a general change has no stable benefit, preserve the failure and stop tuning
that candidate. CPU worker/native integration and richer fractional fidelity
remain separate incomplete requirements.

```powershell
cargo +1.98.1 build --release --locked --offline -p gpui-box-rendering-perf --features retained-gpu --bin dense-runtime-perf
python tooling/rendering-perf/capture-burst.py target/new-policy
# For the recovery pair first preserve the original exact executable at
# target/burst-baseline/dense-runtime-perf.exe, then build the new source.
python tooling/rendering-perf/capture-burst.py target/new-recovery --recovery-pair
python tooling/rendering-perf/analyze-burst.py target/new-recovery
python tooling/rendering-perf/analyze-frame-budget.py target/new-recovery
```
