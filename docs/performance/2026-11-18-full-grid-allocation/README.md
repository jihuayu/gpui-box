# Full Grid/Flex allocation verification, 2026-11-18

Worktree: `D:\oss\ggpui-worktrees\rendering-performance-research`.
Branch: `codex/rendering-performance-research`.
Base: `6dd573d404b6306839436b9de8a44ddb32703c85`.
See [public contract](../../RETAINED_FULL_GRID_ALLOCATION.md).
Complete RFC remains active. This is framework capability work with no timing,
memory, FPS, displayed-latency or sustained-cadence gain claimed.

## Controls

Seven regular core cases and one ignored explicit hardware wrapper cover:

- Grid→Column→Row→two Flex panes; `minmax(0,fr)` tracks weighted1:2 on both
  axes. LTR/RTL × resolved/local ×120×90→180×120→120×90 yields12complete CPU
  comparisons against an independent numeric ordinary full Flex reference.
  A400×300 constructed child does not change its assigned host slot. The GPU
  wrapper compares12submissions/images. Old packets survive UI retirement.
- 40px+50% columns, numeric span across both, explicit30px row and implicit25px
  row, both directions. Actual input is Inherent, known100×25, area percentage
  basis100×25, soft viewport100×25 with unbounded hard intervals.
- Seven feedback/unproved cases reject before the callback: Auto rows, absent
  implicit-row pattern, convenience-fr Auto minimum, baseline alignment, ratio,
  Auto+start inline sizing and intrinsic preferred height.
- Three late changes reject atomically despite equal current areas: unused
  implicit-row policy, equal sibling placement and child order. Source, stats,
  appended structure and complete old pixels retain their previous values.
- Root Flex→Grid persistent full Allocation producer runs initially, on changed
  source, and on120×90→180×120 resize (40×30→60×40 cell). Same-value source
  writes do not rerun its callback.
- Count2, AutoFit and AutoFill × LTR/RTL, nested Grid and percentage-sized item
  with2px padding/1px margins. Independently calculated area/known values compare
  within0.001px; complete LayoutResult and every CPU pixel compare exactly to an
  ordinary full kernel reference. Count has60px columns, auto-repeat40px columns.
- A different Grid cell gains large content after the host reads its allocation.
  Numeric tracks keep the observed slot unchanged, so no measurement dependency
  is invented; complete geometry and CPU output match the full reference.

Real Noto uses a Grid card's automatic-height padded title and remaining-space
Flex body. Body task1 is registered before title task90 without explicit after;
the existing measurement dependency graph executes90→1. Root200×210→280×180→
240×210, long→short→long shaped paragraphs,3px title padding. An independent
ordinary full Flex reference derives header height from the prepared paragraph
at the known card width. Complete title LayoutResult/baselines and every CPU
pixel agree; explicit GPU compares3submissions/images. Old glyph frames remain
usable after UI/text/font/raster owners retire.

Only dependency proof/capture and an internal enum label change. Existing Grid
inline helpers are factored without changing their accepted domain. Source
schema6, layout/track algorithms, frame/GPU data and renderer are unchanged.

## Results and limits

Paired Rust/Cargo1.98.1, Windows MSVC, locked/offline debug correctness.
Explicit GPU: NVIDIA GeForce GTX1650Ti, Vulkan, driver571.96.

| Check | Observed result |
| --- | --- |
| Final complete retained | 606 passed, 1 ignored |
| Final complete text/WGPU | 640 passed, 53 ignored |
| Final complete hello | 68 passed, 19 ignored |
| Seven regular core cases / real-Noto CPU | Included in suites above |
| Explicit numeric Grid/Flex GPU | 1 passed, 12 submissions/12 complete images |
| Explicit real-Noto Grid GPU | 1 passed, 3 submissions/3 complete images |
| Final library Clippy -D warnings | Passed |
| Complete retained-gpu performance harness check | Passed |
| Scoped rustfmt/diff and288local links across7documents | Passed |
| Five unrelated WIP file hashes | Unchanged |

All production and test behavior was fixed before the final full runs and both
explicit GPU controls. Counts overlap; ignored hardware cases count only when
explicitly executed. Existing retained-only SpatialUpdate unused-field and
dependency linker warnings remain without suppression. The complete suites also
retain earlier saved Chromium Grid references; no browser was rerun this stage.

Proofs repeatedly scan ancestor track/style/child data and retain immutable
handles/order lists. Ordinary private previews/candidates, final input recording
and complete geometry checks remain. No cost-model, persistent dependency index,
byte/time budget or asymptotic improvement is claimed. Finite reference images
do not establish native displayed latency, modern desktop effects smoothness,
independent presentation or sustained complete CPU frames. "99%" is figurative
user direction, not a success-rate/p99 acceptance requirement.

## Development corrections

Automatic-repeat tests initially compared an independently divided fractional
area to the kernel's subtraction of two track boundaries using exact f32 equality.
They differed by one ULP (13.333333 vs13.333334). Formula checks now use0.001px
tolerance; full kernel LayoutResult and pixel comparisons remain exact. The
formula array also needed an explicit f32 annotation for Rust's max method.
Production layout behavior was not changed to fit expected division results.

## Reproduction

```powershell
$env:TEMP='D:/oss/ggpui-worktrees/rendering-performance-research/target/style-ir-temp'
$env:TMP=$env:TEMP
cargo +1.98.1 test -p gpui-box --features retained --locked --offline --quiet
cargo +1.98.1 test -p gpui-box --features retained-text,retained-wgpu --locked --offline --quiet
cargo +1.98.1 test -p gpui-box-example-hello --features retained-wgpu --locked --offline --quiet
cargo +1.98.1 test -p gpui-box --features retained-text,retained-wgpu --locked --offline --quiet hardware_full_grid_allocation -- --ignored --nocapture
cargo +1.98.1 test -p gpui-box-example-hello --features retained-wgpu --locked --offline --quiet hardware_full_grid_noto_cards -- --ignored --nocapture
cargo +1.98.1 clippy -p gpui-box --features retained-text,retained-wgpu --lib --no-deps --locked --offline -- -D warnings
cargo +1.98.1 check -p gpui-box-rendering-perf --features retained-gpu --locked --offline --quiet
```
