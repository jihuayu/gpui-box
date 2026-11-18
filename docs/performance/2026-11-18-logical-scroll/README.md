# LogicalUi default scroll verification, 2026-11-18

Worktree: `D:\oss\ggpui-worktrees\rendering-performance-research`.
Branch: `codex/rendering-performance-research`.
Base: `34e1ef15a11ad412de4cb972cb2335eb16ede7a8`.
See [public contract and costs](../../RETAINED_LOGICAL_SCROLL_DEFAULTS.md).
Complete RFC stays active; no timing/FPS/memory/native smoothness benefit claim.

## Controls

- Nested two-axis wheel consumes inner range, chains residual to outer and
  reports remaining pixels. A second request using old installed targeting
  accumulates current committed offsets; three resulting wheel/reveal images
  compare all CPU/GPU pixels to an independent full tree using numeric child
  translations and explicit clips, with no scroll or reveal solver in reference.
- Nested Tab reveal minimally scrolls inner/outer and current hit geometry reaches
  the target. Old local frames outlive their logical owner.
- Source-bound target height changes settle before reveal; final layout and
  numeric full CPU pixels agree. Repeated reveal keeps the identical LocalFrame
  and requests no frame.
- Signal-bound scroll skips default writes and leaves the source intact;
  same-candidate unbinding restores default behavior. Same-candidate new binding
  suppresses reveal, and an ancestor cannot bypass the fixed clipped viewport.
- Hidden overflow rejects wheel consumption but allows focus reveal, including
  device-pixel snapping at scale 1.5; repeated reveal stays idempotent.
- Final disable/reparent and foreign runtime reject defaults and all tentative
  source/offset writes. Invalid final opacity preserves source/frame/offset and
  does not run ordinary effects. Nonfinite deltas and unpublished geometry reject.
- Real Noto text changes width 80→80→120 and long→short→long content while reveal
  uses final wrap geometry. Numeric offsets 73/65/73, complete LayoutResult/baseline,
  hit target and all CPU/GPU pixels match the independent full layout. Old glyph
  frame remains consumable after UI/font/raster owners are dropped.

The UiDocument wheel/reveal tests continue covering the shared helpers through
its original exact-base source path. Existing scoped-event tests retain callback
and post-commit controls.

## Results and limits

Final retained suite: 698 passed, 1 ignored; 10 compiler doctests passed.
Final text/WGPU suite: 732 passed, 64 ignored; 10 compiler doctests passed.
Final hello suite: 79 passed, 30 ignored.
Explicit logical scroll GPU and real Noto GPU: one test each passed, three
complete image comparisons each, on GTX 1650 Ti / Vulkan / driver 571.96.
Final library Clippy with `-D warnings` passed without lint suppression.
The complete retained-gpu harness check, scoped rustfmt, diff whitespace,
315 local Markdown links and five inherited WIP hashes passed. Final suites and
explicit hardware controls cover final source. Hardware executions overlap the
ignored tests in complete suites; ignored counts alone are not GPU evidence.
All commands used Rust/Cargo 1.98.1, Windows MSVC, locked/offline debug correctness.
Initial targeted controls found that a dirty resolved FramePacket cache rejected
valid revision-matching local publications after scrolling. Input frame validation
now distinguishes pending geometry from an obsolete resolved cache, retaining
strict checks for unpublished changes.
The initial external Noto compile exposed inaccessible render-node internals;
the shared scene now offers `node_id` for explicit programmatic focus. A Clippy
type-complexity finding was resolved with the internal WheelPlan result type.

Debug locked/offline correctness controls are not native cadence or input-to-display
latency evidence. Full candidate/input maps, bound-endpoint scans, registry copies
and ordinary source/producer/layout work remain. No solver/GPU/source schema change.

## Reproduction

```powershell
$env:TEMP='D:/oss/ggpui-worktrees/rendering-performance-research/target/style-ir-temp'
$env:TMP=$env:TEMP
cargo +1.98.1 test -p gpui-box --features retained --locked --offline --quiet
cargo +1.98.1 test -p gpui-box --features retained-text,retained-wgpu --locked --offline --quiet
cargo +1.98.1 test -p gpui-box-example-hello --features retained-wgpu --locked --offline --quiet
cargo +1.98.1 test -p gpui-box --features retained-text,retained-wgpu --locked --offline --quiet hardware_logical_scroll -- --ignored --nocapture
cargo +1.98.1 test -p gpui-box-example-hello --features retained-wgpu --locked --offline --quiet hardware_logical_scroll_real_noto -- --ignored --nocapture
cargo +1.98.1 clippy -p gpui-box --features retained-text,retained-wgpu --lib --no-deps --locked --offline -- -D warnings
cargo +1.98.1 check -p gpui-box-rendering-perf --features retained-gpu --locked --offline --quiet
```
