# Focus reveal verification, 2026-11-18

Worktree: `D:\oss\ggpui-worktrees\rendering-performance-research`.
Branch: `codex/rendering-performance-research`.
Base: `c1267c07ff9a3147652d5afef63b674afdbb193f`.
See [public contract](../../RETAINED_FOCUS_REVEAL.md).
Full RFC remains active; no timing, memory, native p99 or FPS gain is claimed.

## Controls

Eight input cases and one owner-local snapped case cover:

- Nested Tab reveal (inner50/outer60), style feedback and no hit prediction before
  adoption. Independent numeric full reference uses explicit child translations/
  clips without reveal/scroll calculation. Every CPU pixel matches; hardware
  compares2complete submissions including target-height changes.
- Exact replay, repeated old-scene request using latest offsets/no extra frame,
  final focus-style height35 (inner65), ordinary setters preceding reveal phase
  regardless of list position, and authored/effective style ownership.
- Stale reparent, disabled/hidden/missing target, invalid-layout batch rollback,
  same-batch create/reveal, ordered multiple reveals, snapshot offsets and unique
  changed-route diagnostics.
- Hidden versus Clip/Visible barriers, two independent axes, oversized/stable
  nearest behavior, transform extent limits, explicit host clip and requested500
  versus actual50 offsets.
- DevicePixels1.5 initially fully offscreen target. Geometry reads logical owner
  state before projected clips, with actual committed offsets.

Actual GPUI Tab traverses the normal key listener/router/planner/batch/complete
canvas paint. It checks focus, offset60, content origin30 and adopted hits. This
is GPUI test-platform evidence, not native shown latency.

Real Noto changes long→short→long text and80→120px host width in the same reveal
batch, resetting explicit offsets each time. Numeric offsets73/65/73 include3px
padding. LayoutResult/baseline/wrap and all CPU pixels match a separate ordinary
full tree with explicit offsets. Hardware compares3complete images; old glyph
packet survives source/font/resource owner retirement.

## Observed results

Paired Rust/Cargo1.98.1, Windows MSVC locked/offline debug correctness. Explicit
GPU used NVIDIA GeForce GTX1650Ti, Vulkan, driver571.96.

| Check | Result |
| --- | --- |
| Final complete retained | 576 passed, 1 ignored |
| Final complete retained-text/retained-wgpu | 610 passed, 48 ignored |
| Final complete hello | 63 passed, 14 ignored |
| Focus reveal filter | 9 passed, 1 ignored |
| Actual GPUI Tab | 1 passed, included in hello |
| Real-Noto CPU | 1 passed, included in hello |
| Explicit core reveal GPU | 1 passed, 2 submissions/2 complete-image comparisons |
| Explicit real-Noto reveal GPU | 1 passed, 3 submissions/3 complete-image comparisons |
| Final library Clippy -D warnings | Passed |
| Complete retained-gpu performance harness check | Passed |
| Scoped formatting/diff and279local documentation links | Passed |
| Five unrelated WIP file hashes | Unchanged |

Counts overlap; ignored hardware cases count only when explicitly executed.
The final complete suites include the unique diagnostic counts and skip-empty-
reveal-commit follow-up. Both GPU controls were explicitly rerun after that final
follow-up. Final library Clippy and harness check also cover it. Existing
retained-only SpatialUpdate warning and dependency linker messages remain.
No vendor/Taffy/renderer changes or lint suppression were added. These checks
establish correctness and compilation, not native displayed interaction latency.

## Implementation history

InputGraph::eligible needed internal retained visibility. GPUI fixture omitted
App at Window::focus, corrected to the existing signature. Noto expected offsets
initially omitted padding (70/68/70); numeric nearest positions are73/65/73 and
complete layout/CPU/GPU references agree after fixing constants. Framework
calculation was not relaxed. Clippy requested named RevealResult for a complex
tuple; no lint suppression was added.

Output LocalFrame ancestry was replaced with logical owner geometry because
pixel projection bakes clips/offsets and cannot recover hidden targets. Existing
resolve_geometry stays unchanged. Explicit host clips constrain reveal viewport.
Ordinary candidate and validation costs remain; no performance claim is inferred.

## Reproduction

```powershell
$env:TEMP='D:/oss/ggpui-worktrees/rendering-performance-research/target/style-ir-temp'
$env:TMP=$env:TEMP
cargo +1.98.1 test -p gpui-box --features retained --locked --offline --quiet
cargo +1.98.1 test -p gpui-box --features retained-text,retained-wgpu --locked --offline --quiet
cargo +1.98.1 test -p gpui-box-example-hello --features retained-wgpu --locked --offline --quiet
cargo +1.98.1 test -p gpui-box --features retained-text,retained-wgpu --locked --offline --quiet reveal -- --nocapture
cargo +1.98.1 test -p gpui-box --features retained-text,retained-wgpu --locked --offline --quiet hardware_focus_reveal -- --ignored --nocapture
cargo +1.98.1 test -p gpui-box-example-hello --features retained-wgpu --locked --offline --quiet hardware_focus_reveal_real_noto -- --ignored --nocapture
cargo +1.98.1 clippy -p gpui-box --features retained-text,retained-wgpu --lib --no-deps --locked --offline -- -D warnings
```
