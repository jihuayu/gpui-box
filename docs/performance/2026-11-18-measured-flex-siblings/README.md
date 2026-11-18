# Measured sibling Flex verification, 2026-11-18

Worktree: `D:\oss\ggpui-worktrees\rendering-performance-research`.
Branch: `codex/rendering-performance-research`.
Base: `deea9a4dcf6f886c8a4b6b846ccfef2e7b1097f7`.
See [public contract](../../RETAINED_MEASURED_FLEX_SIBLINGS.md).
The full RFC stays active. This closes a common composition gap, with no timing,
FPS, memory or native smoothness claim. "99%" is figurative user direction.

## Controls

Six regular core cases and one ignored explicit GPU wrapper cover:

- Four Flex directions × three size/intrinsic changes. A nested measured sibling
  changes20×15→35×30→20×15 while the body consumes remaining space and constructs
  400×300 content. Complete CPU pixels match numeric complete full Flex; the
  explicit GPU wrapper compares12submissions/12complete images. A late fill
  change is accepted as paint-only. Old output survives owner retirement.
- Five late layout-input changes reject despite unchanged sibling height:
  fixed-measure width, adding a zero-sized child, parent metadata and a numerically
  inactive style limit and layout order. Source, structure, stats and complete pixels roll back.
- A measured-header InlineAllocation producer precedes a full Allocation body
  using explicit after. Source15→30 updates body75→60; ordinary body source
  updates and same-value reuse preserve normal producer semantics.
- Wrap and WrapReverse with a pure ContentMeasure sibling naturally place an
  80×30 header and50×20 body into separate lines. CPU pixels match a numeric full
  reference. Installing a new pure measure handle with the same answer after
  observation rejects; reusing the original handle is accepted.
- A once-only plan deliberately constructs the body before changing the header
  measurement. Final validation rejects rather than replaying callbacks or
  publishing stale content.
- A measured image sibling changes pixel resource at unchanged20×15 intrinsic
  size after observation; layout dependency remains equal. Changing intrinsic
  size to20×10 afterwards rejects and preserves the old frame.

Real Noto title/toolbar tests use ordinary Column,3px padding and a body filling
remaining height, with no size containment. Long→short→long title snapshots and
100×210→180×180→140×210 root sizes change actual title wrap/height. A separate
numeric reference derives title height from the real shaped paragraph's wrap,
uses fixed header/body sizes and ordinary text layout, then compares complete
title LayoutResult/baselines and every CPU pixel. The explicit GPU control
compares3submissions/3complete images. Same-paragraph text color changes remain
legal after body construction; replacing the shaped paragraph after observation
rejects. Old glyph output survives UI/resource/font/raster owners retirement.

No production Taffy/vendor/GPU algorithm, public builder type or source schema
changes. The previous independent-parent/containment proof keeps precedence,
and previous numeric full-Flex controls remain covered by regressions.

## Results and limits

Paired Rust/Cargo1.98.1, Windows MSVC locked/offline debug correctness. Explicit
GPU: NVIDIA GeForce GTX1650Ti, Vulkan, driver571.96.

| Check | Observed result |
| --- | --- |
| Complete retained | 594 passed, 1 ignored |
| Final complete text/WGPU | 628 passed, 51 ignored |
| Final complete hello | 66 passed, 17 ignored |
| New regular core cases | 6 passed, included in complete suites |
| Final retained-only sibling filter | 6 passed, includes final order observation |
| Real-Noto CPU | 1 passed, included in hello |
| Explicit four-direction sibling GPU | 1 passed, 12 submissions/12 complete images |
| Explicit real-Noto title GPU | 1 passed, 3 submissions/3 complete images |
| Final library Clippy -D warnings | Passed |
| Complete retained-gpu performance harness check | Passed |
| Scoped rustfmt/diff and276local documentation links | Passed |
| Five unrelated WIP hashes | Unchanged |

The complete retained run preceded the final layout_order observation and its
extra mutation assertion. Final text/WGPU, hello, both explicit GPU controls,
library Clippy and harness include it. Final retained-only focused6core cases
also pass with the order observation. Suite counts overlap; ignored
hardware tests count only when explicitly run. Existing retained-only unused
SpatialUpdate fields and dependency linker messages remain without suppression.

The added proof walks content-dependent sibling subtrees and retains cloned
immutable layout inputs/child lists per dependent builder until transaction end.
Final recapture repeats the walk. There is no reverse output index, global
memory budget or incremental rollback; full candidates/previews/geometry remain.
Correctness and GPU readback matching do not establish native displayed latency,
independent presentation, desktop effects cadence or sustained CPU output.

## Development corrections

The first image fixture staged explicit measure(None) alongside a final Image;
that violates the existing single measurement-owner contract and returned
InvalidMeasure. The fixture now changes Image alone, letting the ordinary image
channel replace intrinsic measurement. No production validation was relaxed.
The dependency audit also added the framework-owned layout_order field to each
subtree observation; it is separate from raw Taffy Style and child storage order.
A late order change with unchanged singleton geometry now rejects explicitly.

## Reproduction

```powershell
$env:TEMP='D:/oss/ggpui-worktrees/rendering-performance-research/target/style-ir-temp'
$env:TMP=$env:TEMP
cargo +1.98.1 test -p gpui-box --features retained --locked --offline --quiet
cargo +1.98.1 test -p gpui-box --features retained-text,retained-wgpu --locked --offline --quiet
cargo +1.98.1 test -p gpui-box-example-hello --features retained-wgpu --locked --offline --quiet
cargo +1.98.1 test -p gpui-box --features retained-text,retained-wgpu --locked --offline --quiet sibling_content_full_flex -- --nocapture
cargo +1.98.1 test -p gpui-box --features retained-text,retained-wgpu --locked --offline --quiet hardware_sibling_content_full_flex -- --ignored --nocapture
cargo +1.98.1 test -p gpui-box-example-hello --features retained-wgpu --locked --offline --quiet hardware_measured_sibling_noto_full_flex -- --ignored --nocapture
cargo +1.98.1 clippy -p gpui-box --features retained-text,retained-wgpu --lib --no-deps --locked --offline -- -D warnings
cargo +1.98.1 check -p gpui-box-rendering-perf --features retained-gpu --locked --offline --quiet
```
