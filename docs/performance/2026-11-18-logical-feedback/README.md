# LogicalUi style feedback verification, 2026-11-18

Worktree: `D:\oss\ggpui-worktrees\rendering-performance-research`.
Branch: `codex/rendering-performance-research`.
Base: `0c5769f32fe229617aff05bde89d542fffb34db0`.
See [contract, ownership and costs](../../RETAINED_LOGICAL_STYLE_FEEDBACK.md).
Complete RFC stays active; no FPS/latency/memory/native smoothness gain claim.

## Controls

- Pointer hover, press, move outside, release, Tab and window focus loss drive
  ordinary read-only state and StylePlan/computed/binding. Caller selected remains
  true and disabled remains false. Six full CPU/GPU images match independent
  fixed-layout/color references. Equal leave preserves the same local frame.
- Invalid final opacity rolls back feedback/frame; retry applies pressed.
  Redeclaring input preserves router state while changing caller flags.
- Foreign/future feedback is ignored; final disabled policy clears pressed.
- Full StyleResolution binding conflicts with covered individual color bindings
  in both orders/immediate/draft paths, and same-edit unbind permits replacement.
- Appearance-only style binding preserves default wheel offset across hover,
  permits a separate scroll binding and reports it controlled; adding full style
  beside appearance-only rejects.
- Transactional/manual scope retirement removes registrations, invalidates the
  readonly input and ignores old feedback.
- ComponentContext real Noto label uses feedback-driven width 60→125 and pressed
  opacity 0.6; hover/down/up/leave produce four complete CPU/GPU images matching
  independent numeric full Flex, complete text LayoutResult/baseline and old glyph
  frame retention after UI/font/raster disposal. Text remains a separate binding.
- New compiler control rejects ReactiveTransaction writes through the read-only
  interaction input. Existing scoped event/scroll/style controls remain.

## Results and limits

Final retained suite: 704 passed, 1 ignored; 11 compiler doctests passed.
Final text/WGPU suite: 738 passed, 65 ignored; 11 compiler doctests passed.
Final hello suite: 80 passed, 31 ignored.
Explicit logical-feedback GPU: 1 passed, six complete image comparisons.
Explicit real Noto feedback GPU: 1 passed, four complete image comparisons.
Hardware: GTX 1650 Ti / Vulkan / driver 571.96. Explicit executions overlap the
ignored full-suite tests; ignored counts alone are not hardware coverage.
Final library Clippy with `-D warnings` passed without lint suppression.
The complete retained-gpu harness check passed.
Scoped rustfmt, diff whitespace, 324 local Markdown links and five inherited WIP
hashes passed. Final suites and explicit hardware controls cover final source.
Rust/Cargo 1.98.1, Windows MSVC, locked/offline debug correctness.
Initial compilation exposed module visibility and was corrected without changing
public ownership. Review added explicit appearance-only scroll preservation;
full style bindings otherwise correctly count as application-controlled scroll.

Full current scene maps, disposable eligibility preview, source/computed/producer
and candidate/layout costs remain. Debug offscreen equivalence does not establish
native visible latency/cadence. Source schema 6, solver and GPU format unchanged.

## Reproduction

```powershell
$env:TEMP='D:/oss/ggpui-worktrees/rendering-performance-research/target/style-ir-temp'
$env:TMP=$env:TEMP
cargo +1.98.1 test -p gpui-box --features retained --locked --offline --quiet
cargo +1.98.1 test -p gpui-box --features retained-text,retained-wgpu --locked --offline --quiet
cargo +1.98.1 test -p gpui-box-example-hello --features retained-wgpu --locked --offline --quiet
cargo +1.98.1 test -p gpui-box --features retained-text,retained-wgpu --locked --offline --quiet hardware_logical_feedback -- --ignored --nocapture
cargo +1.98.1 test -p gpui-box-example-hello --features retained-wgpu --locked --offline --quiet hardware_logical_feedback_real_noto -- --ignored --nocapture
cargo +1.98.1 clippy -p gpui-box --features retained-text,retained-wgpu --lib --no-deps --locked --offline -- -D warnings
cargo +1.98.1 check -p gpui-box-rendering-perf --features retained-gpu --locked --offline --quiet
```
