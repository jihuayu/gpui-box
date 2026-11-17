# Explicit scrollbar allocation evidence, 2026-11-17

Worktree: `D:\oss\ggpui-worktrees\rendering-performance-research`.
Branch: `codex/rendering-performance-research`.
See [contract and ADR](../../RETAINED_SCROLLBAR_GUTTERS.md).
Full RFC acceptance stays open. No timing/peak-memory/sustained-FPS gain is claimed.

## Independent browser geometry

Chromium145.0.7632.6 on Windows produced288 cases: four Flex directions × three
wrap modes × two directions × three gutter widths(0/6/12) × vertical/both
scrollbars × BorderBox/ContentBox. Each case compares host border/content sizes
and x/y/width/height of three differently sized items. All288 cases matched
within0.02px; no deviations are excluded. Browser data and capture script are
checked in beside this report. This is a finite numeric Flex matrix, not full
CSS overflow/scrollbar conformance.

```powershell
node docs/performance/2026-11-17-scrollbar-gutters/capture.mjs D:/Scoop/persist/nodejs/bin/node_modules/agent-browser/node_modules/playwright-core/index.mjs
```

The capture intentionally removes Playwright's default `--hide-scrollbars`.
An explicit guard requires the80×60/padding3/border2/6px both-scroll case to have
64×44 content. The initial hidden-scrollbar capture incorrectly had70×50 and
caused192 matrix failures. It was discarded and recollected; the guard prevents
future silent overlay-only references. An initial HTML fixture also ran before
body creation; an explicit body corrects that setup failure.

## Normal pipeline controls

- Public style/results test all four overflow modes and both axes, checking
  actual reservations/content sizes and invalid negative/NaN/infinite rejection.
- Typed host style changes0→6 LTR→6 RTL→same→12 RTL→0 update images and all CPU
  pixels against fresh numeric-padding full controls. Resolved/local atomic edits
  check invalid-source rollback, same-final-value P0 revision/zero layout and old
  frames after owner drop. Direction-only changes have identical scalar Layout
  fields but must move the leaf's content origin.
- Flex, Tight ConstraintBox and pure owned parent each place a100% child in
 48×28 content within a60×40/padding3/both6 host. LTR origins are3/3 and RTL9/3.
  ChildLayoutStyle checks public width4 reaches measured parent item inputs.
- Content-only scalar and complete metric fixtures now set width through the
  public API, replacing raw-style escape hatches. Soft opportunities, complete
  baseline and overflow retain their existing expected values.
- Container query uses fixed100px host and threshold89. Gutter12→0→12 changes
  actual inline content88→100→88 and selects Row→Column→Row from public styles.
- Incoming builder on100px ordinary host reads100px border-box assignment;
  outgoing contained builder reads88px after padding3 and gutter6. Both construct
  an ordinary nested host whose incoming width is88. Content height20 gives
  the same final88×20 content result.
- Scroll requests1000/1000 on a60×40/padding3 host with100×80 child clamp to
 46+gutter per axis. Changes6→12→0→6 move cached descendants to−43−gutter through
  existing selective/local publication.
- Real bundled Noto text at gutter0/16LTR/16RTL/32RTL/0 inside100×400/padding3
  uses94−gutter wrap width for layout, first/last baseline and actual glyph
  extraction. Complete CPU pixels/content geometry match independent numeric
  padding controls, resolved/local both. Old glyph frames remain consumable.
  The initial RTL reference forgot direction and moved the root within its
  viewport differently; the fixture now uses the same direction and independently
  substitutes only numeric padding. Failures are not counted as passes.

Explicit NVIDIA GeForce GTX1650Ti / Vulkan / driver571.96 execution passed five
image local-submit/readback comparisons and ten real-glyph submit/readback
comparisons (five width/direction states × resolved/local atomic production).
All pixels match the numeric-padding full controls. These15 finite offscreen
comparisons do not establish visible presentation, modern heavy-effects or
sustained-frame performance. Ignored tests alone do not establish hardware evidence.

## Reproduction and costs

| Check | Actual result |
| --- | --- |
| retained complete suite |442 passed |
| retained-text + retained-wgpu complete suite |475 passed,30 ignored |
| explicit image WGPU case |1 passed,5 complete-pixel submissions |
| hello retained-wgpu with actual Noto glyphs |46 passed,3 ignored |
| explicit real-glyph WGPU case |1 passed,10 complete-pixel submissions |
| vendor default / project-config library suites |228 /222 passed |
| vendor no-default / alloc+tree+Flex / all-features checks |passed |
| framework library Clippy `-D warnings` |passed |
| scoped framework/glyph rustfmt and git diff check |passed |

An earlier retained441 run did not include the final builder test; the final442
and text/WGPU475 include it. During test construction, conflicting raw/public
glob names and assumed convenience methods caused compilation errors; fixtures
now use the real public types/viewport constructor/draft property entry. Those
failed attempts do not count as passes. Vendor counts are unchanged: its leaf
inset-side correction reuses existing sizing/overflow tests and adds no new test.

```powershell
cargo +1.98.1 test -p gpui-box --features retained --locked --offline --quiet
cargo +1.98.1 test -p gpui-box --features retained-text,retained-wgpu --locked --offline --quiet
cargo +1.98.1 test -p gpui-box --features retained-text,retained-wgpu --locked --offline --quiet hardware_scrollbar -- --ignored --nocapture
cargo +1.98.1 test -p gpui-box-example-hello --features retained-wgpu --locked --offline --quiet
cargo +1.98.1 test -p gpui-box-example-hello --features retained-wgpu --locked --offline --quiet hardware_public_scrollbar -- --ignored --nocapture
cargo +1.98.1 clippy -p gpui-box --features retained-text,retained-wgpu --lib --no-deps --locked --offline -- -D warnings

$env:CARGO_PROFILE_TEST_DEBUG='0'
$env:CARGO_PROFILE_TEST_INCREMENTAL='false'
$env:CARGO_PROFILE_TEST_CODEGEN_UNITS='256'
cargo +1.98.1 test --manifest-path vendor/taffy/Cargo.toml --lib --locked --offline --quiet
cargo +1.98.1 test --manifest-path vendor/taffy/Cargo.toml --lib --no-default-features --features std,taffy_tree,flexbox,grid,block_layout,float_layout,content_size --locked --offline --quiet
cargo +1.98.1 check --manifest-path vendor/taffy/Cargo.toml --no-default-features --locked --offline --quiet
cargo +1.98.1 check --manifest-path vendor/taffy/Cargo.toml --no-default-features --features alloc,taffy_tree,flexbox --locked --offline --quiet
cargo +1.98.1 check --manifest-path vendor/taffy/Cargo.toml --all-features --locked --offline --quiet
```

Suite counts overlap. Temporary vendor test settings do not change production
settings. Existing retained-only SpatialUpdate unused warnings and dependency
linker messages remain; Clippy claims framework library scope. Public style/result
payload grows; ABI padding/peak-memory, dispatch and timing are unprofiled.
Common internal Style/Layout and GPU/packet/cache formats do not grow. Automatic
bar appearance/native chrome, broad CSS/writing-mode, general RenderBox and
dependency scheduling, modern desktop heavy-effects GPU/CPU complete-frame
smoothness/sustained60/120Hz/display-visible tails remain separate gates.
