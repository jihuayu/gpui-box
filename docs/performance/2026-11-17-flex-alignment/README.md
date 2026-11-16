# Flex alignment validation — 2026-11-17

Windows x64, Rust1.98.1 locked/offline, existing research worktree. Capability and
correctness work; no timing benchmark or continuous delivery qualification ran.

Browser geometry is an independent engine reference, not the Taffy lowering used
as its own oracle. Capture uses the repository's existing Playwright-core route:

```powershell
node docs/performance/2026-11-17-flex-alignment/capture.mjs D:/Scoop/persist/nodejs/bin/node_modules/agent-browser/node_modules/playwright-core/index.mjs
cargo +1.98.1 test --locked --offline -p gpui-box --features retained --lib --quiet
cargo +1.98.1 test --locked --offline -p gpui-box --features retained-text,retained-wgpu --lib --quiet
cargo +1.98.1 test --locked --offline -p gpui-box --features retained-text,retained-wgpu --lib alignment_tests -- --include-ignored --nocapture
cargo +1.98.1 test --locked --offline -p gpui-box-example-hello --features retained-wgpu --quiet
cargo +1.98.1 clippy --locked --offline -p gpui-box --features retained-text,retained-wgpu --lib -- -D warnings
cargo +1.98.1 test --locked --offline --manifest-path vendor/taffy/Cargo.toml --lib --quiet
cargo +1.98.1 test --locked --offline --manifest-path vendor/taffy/Cargo.toml --no-default-features --features std,taffy_tree,flexbox,grid,block_layout,float_layout,content_size --lib --quiet
cargo +1.98.1 check --locked --offline --manifest-path vendor/taffy/Cargo.toml --no-default-features --lib --quiet
cargo +1.98.1 check --locked --offline --manifest-path vendor/taffy/Cargo.toml --no-default-features --features alloc,taffy_tree,flexbox --lib --quiet
cargo +1.98.1 check --locked --offline --manifest-path vendor/taffy/Cargo.toml --all-features --lib --quiet
```

All listed checks completed successfully:

| Check | Actual result |
| --- | --- |
| Framework retained | 325 passed |
| Framework retained-text + retained-wgpu | 358 passed, 10 ignored |
| Focused alignment, explicitly including hardware | 5 passed |
| Hello with retained-wgpu, including real Noto paragraph | 25 passed, 2 ignored |
| Clippy library with text/WGPU | Passed with `-D warnings` |
| Owned Taffy default library | 204 passed |
| Owned Taffy project feature set | 198 passed |
| Owned Taffy no-default / alloc+tree+flexbox / all-features | All three checks passed |

Default-ignored GPU cases are not included in the ordinary suite pass counts.
Only the focused alignment hardware case was explicitly run in this stage.

## Browser reference and corrections

Chromium **145.0.7632.6** captures 5,376 cases in browser-reference.tsv. The JSON
records engine, schema, tolerance and twelve explicit deviations. Four directions,
LTR/RTL, NoWrap/Wrap/WrapReverse, same/opposite child direction, main/items/self/
content alignment and positive/negative free space are covered. Min/max intrinsic,
aspect ratio, padding, auto sizes, baseline and out-of-flow are not part of this
matrix and are not inferred to conform from it.

5,364 match to 0.02px, allowing Chromium's 1/64px fractional distribution. The
twelve excluded cases are overflowing SpaceBetween on reversed main or cross axes.
Current Flexbox requires safe flex-start -> logical start; Chromium145 keeps
flex-start. The native test asserts their exact offset difference separately and
does not count them as compatibility passes. The contract links normative text.

First comparison had 2,920 mismatches, mostly because the browser fixture omitted
the framework's authored default justify Start and used CSS Normal; correcting
that fixture did not require changing public defaults. Actual kernel corrections:
explicit justify Stretch resolves to FlexStart, and align-content Stretch's
overflow fallback has no implicit safe modifier. The latter corrected oversized
wrap-reverse line origins. SpaceBetween's existing normative safe behavior stays.

## Real source, frame and resource flow

Four ordinary framework regressions cover the browser matrix, invalid modifier
validation/transaction rollback, auto-margin precedence/Normal stretch, and
scope-owned responsive alignment sources through both resolved and LocalFrame
atomic edits. Repeated same breakpoint keeps revision; Safe/Unsafe cross Center
moves a 140x100 item between y0/-20 in a 100x60 host. CPU pixels equal fresh
independently positioned ordinary trees (no alignment algorithm in the reference),
hit geometry follows overflow, invalid source/style publication preserves old
state/stats/frame, and old frames remain correct after UI destruction.

Real bundled **Noto Sans CJK SC** paragraphs keep width60 and real wrapped height
while Safe/Unsafe Center changes node origin in a height30 host. The exact Arc
glyph geometry stays shared. All CPU pixels match independent ordinary origin
placement, including top clipping of overlarge text; old frames survive owner
drop. This is real shaping/rasterization evidence rather than synthetic metrics.

A default-ignored hardware regression is explicitly run against ordinary fixed
positions with four actual LocalFrame submissions. It passed on **NVIDIA GeForce
GTX 1650 Ti**, discrete GPU, **Vulkan / NVIDIA driver 571.96**, on Windows x64.
All complete output pixels match the independently positioned full reference.
Hardware pixels are not evidence of display-visible latency or modern desktop
GPU heavy-effects performance.

## Costs and scope

The [contract and ADR](../../RETAINED_FLEX_ALIGNMENT.md) records field migration,
semantic distinctions, validation and costs. Alignment values carry independent
policy bytes; styles/snapshots/equality include them. No new per-node/GPU format or
alignment-owned heap allocation is introduced. Existing layout/previews still
cost work, and no timing or memory savings follow from these tests.

Full Flex/RenderBox, remaining alignment and baseline/writing-mode semantics,
automatic scheduling, incremental preview/planning, global budgets, resource/
effect/native lifecycle and continuous GPU/CPU/frame-tail acceptance remain open.
