# Owned layout resource verification, 2026-11-18

Worktree: `D:\oss\ggpui-worktrees\rendering-performance-research`.
Branch: `codex/rendering-performance-research`.
Base: `94c4b93ba2176cc97a299f33fe3e3ef166e1d5ae`.
See [public contract](../../RETAINED_OWNED_LAYOUT_RESOURCES.md).
Full RFC remains active. This stage closes a framework API gap; it does not
claim FPS, latency or memory improvement. "99%" is the user's figurative
description of ordinary fluid/intuitive interactions, not a numeric/p99 gate.

## Controls

Seven regular core tests cover:

- A source custom parent contains ordinary wrapping Flex, a pure measured leaf
  and a following footer. Host100→60→120, metadata3→7 and leaf30→12 cells match
  independent complete ordinary Flex geometry and every CPU pixel. The GPU
  wrapper compares3submissions/3complete images, including mixed resource/layout
  changes. Repeated equal resources skip visual work/measurement. Snapshot
  reconstruction matches; Weak checks show layout algorithms/measurers retire
  while previously published frames remain usable.
- Missing and wrong resource kinds, same-ID rebinding, leaf/parent host
  restrictions and a failing content measure reject without partial source or
  frame adoption. A custom parent returning NaN rolls back simultaneous visual
  edits; effective Grid selection through a StyleRef also rejects atomically.
- Install/clear/reinstall parent/data, create with metadata and exact replay use
  the ordinary batch. Compile-time Send+Sync checks cover records and table;
  another test moves actual snapshot/resources to a worker, creates/drops its
  document and compares complete returned pixels.
- Dry queries at60px return leaf60×40, baselines6/38 and parent60×75, without
  changing source, frame revision or published node layout. Invalid IDs/inputs
  reject. The query uses the real pure parent/content protocol.
- An independently allocating parent reads weights1/2 and assigns40/80 at120px,
  despite authored preferred999/1. Missing required metadata rolls back. After
  changing both weights to1 and width90, output matches an independent ordinary
  full Flex tree with45/45 assignments.
- Replacing the parent algorithm changes gap2→8. A second ID aliasing the same
  handle advances source without a visual frame or content measure. Leaf
  image→measure→fixed→box→measure transitions match fresh complete reconstruction.

Real Noto CPU/GPU controls use a custom parent calling measure_baselines, with
text metadata gaps4/9 and widths80/140/100. The separate complete reference uses
ordinary Column Flex row_gap and no custom parent. Complete text LayoutResult,
dry baseline query and every CPU/GPU pixel match; GPU compares3submissions/
3complete images. The old glyph packet remains usable after document/table,
text/rasterizer and font-system owners retire. This is real prepared text, not
synthetic character-width measurement.

No vendor/Taffy/GPU algorithm changes or lint suppressions are part of this
stage. Existing optional ParentMeasure and inline allocation phases remain
carried by the same ParentLayoutSnapshot; no new independent source-phase
scheduler or general content-feedback proof is claimed.

## Results and limits

Paired Rust/Cargo1.98.1, Windows MSVC, locked/offline debug correctness.

| Check | Observed result |
| --- | --- |
| Final complete retained | 583 passed, 1 ignored |
| Final complete retained-text/retained-wgpu | 617 passed, 49 ignored |
| Final complete hello | 64 passed, 15 ignored |
| New regular core cases | 7 passed, included in the complete suites |
| Real-Noto CPU | 1 passed, included in hello |
| Explicit owned custom-parent/Flex GPU | 1 passed, 3 submissions/3 complete-image comparisons |
| Explicit real-Noto custom-parent GPU | 1 passed, 3 submissions/3 complete-image comparisons |
| Final framework library Clippy -D warnings | Passed |
| Complete retained-gpu performance harness check | Passed |
| Scoped rustfmt/diff and300local links across8documents | Passed |
| Five unrelated WIP file hashes | Unchanged |

Ignored hardware tests count only when explicitly executed. Suite counts
overlap; they must not be summed as unique tests.
All production and test behavior was fixed before these complete runs; a later
runtime edit only clarified the existing content-transition comment. No
representative native timing run was introduced by this capability stage.

Retained-only unused SpatialUpdate fields and dependency linker messages are
existing warnings, not new suppressed diagnostics. These checks establish
layout/resource correctness and full output agreement. They do not establish
native displayed interaction latency, continuous desktop effects at60/120Hz,
independent presentation or sustained software complete-frame delivery.
The table still retains immutable resources for its lifetime. Node/reference
scans, normal owner candidates, final geometry checks and private dry-query
copies retain their costs.

## Test development corrections

Adding Measure content required an explicit exhaustive arm in the older
image/text transition fixture; it never creates custom measures. The Noto
reference initially expressed the gap with margin-bottom, which changes the
complete LayoutResult margin metadata. The independent reference now uses
ordinary Flex row_gap; complete LayoutResult and pixel assertions remain intact.
The implementation was not adjusted to match a weaker geometry comparison.

## Reproduction

```powershell
$env:TEMP='D:/oss/ggpui-worktrees/rendering-performance-research/target/style-ir-temp'
$env:TMP=$env:TEMP
cargo +1.98.1 test -p gpui-box --features retained --locked --offline --quiet
cargo +1.98.1 test -p gpui-box --features retained-text,retained-wgpu --locked --offline --quiet
cargo +1.98.1 test -p gpui-box-example-hello --features retained-wgpu --locked --offline --quiet
cargo +1.98.1 test -p gpui-box --features retained-text,retained-wgpu --locked --offline --quiet layout_resource_tests -- --nocapture
cargo +1.98.1 test -p gpui-box --features retained-text,retained-wgpu --locked --offline --quiet hardware_owned_layout_resources -- --ignored --nocapture
cargo +1.98.1 test -p gpui-box-example-hello --features retained-wgpu --locked --offline --quiet hardware_owned_parent_resources_real_noto -- --ignored --nocapture
cargo +1.98.1 clippy -p gpui-box --features retained-text,retained-wgpu --lib --no-deps --locked --offline -- -D warnings
cargo +1.98.1 check -p gpui-box-rendering-perf --features retained-gpu --locked --offline --quiet
```
