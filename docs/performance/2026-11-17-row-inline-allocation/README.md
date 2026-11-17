# Content-independent Row proof evidence, 2026-11-17

Worktree: `D:\oss\ggpui-worktrees\rendering-performance-research`.
Branch: `codex/rendering-performance-research`.
See [contract and ADR](../../RETAINED_ROW_INLINE_ALLOCATION.md).
Full RFC acceptance remains open. No timing/peak-memory/sustained-FPS gain is claimed.

## Independent browser allocation matrix

Chromium145.0.7632.6/Windows reference contains288 cases: Row/RowReverse × three
wrap modes × LTR/RTL × widths120/180/260 × zero/numeric basis × BorderBox/ContentBox
× two content-height sets. Items have independent numeric minima/maxima, grow
factors1/2/1, padding2, inline margins2 and gap4; actual descendant width400
overflows deliberately. Each case first proves all three item inline chains,
then compares host dimensions and all item x/y/width/height within0.03px.
All288 cases pass without exclusions after matching the alignment fixture.

The matrix's initial16 mismatches were a fixture error: browser default justify
is flex-start while the framework deliberately defaults to logical Start.
Reverse rows with positive remainder differ under those two values. The Rust
fixture now explicitly uses FlexStart to match the browser; no kernel alignment
behavior was altered. Initial failed references do not count as passes.

```powershell
node docs/performance/2026-11-17-row-inline-allocation/capture.mjs D:/Scoop/persist/nodejs/bin/node_modules/agent-browser/node_modules/playwright-core/index.mjs
```

## Normal framework controls

- Two0-basis/min0/grow1 columns in Row/RowReverse receive120/180/240/120 root
  widths. Builder observes half width, selects keyed20/60 content heights and
  deliberately overflowing400px descendants. Resolved/local complete CPU pixels
  match fresh ordinary Flex trees. Old packets survive owner drop.
- Wrap/WrapReverse with80px bases puts items on one/two lines as width changes.
  Host builder reads actual line allocation, content height changes line height,
  and complete host LayoutResult/CPU pixels match normal full controls.
- A sibling grow producer registered after the host query is ordered first.
  Root160→180→400→160 yields host80→45→100→80 and corresponding20/60 content
  selection. This would select the wrong result without the new sibling edge.
- Content basis, auto min-width, MaxContent max-width, explicit aspect ratio and
  decoded-image effective ratio on a sibling all reject before builder callback.
  Unsafe authored sibling branches reject registration; computed host-query
  sinks changing sibling grow reject as an allocation cycle.
- After observing the host, a sibling preferred-width change with an overriding
  numeric basis, sibling reorder or zero-grow provisional sibling insertion
  rejects even when width is unchanged. Source/stats/revision remain intact.
- Actual bundled Noto text composes inline builder and scope-owned content query
  under a padded0-basis column. Host width is(width+6)/2; content width(width−6)/2
  selects normal/50% text width. Real glyph layout/baselines/height and all CPU
  pixels match complete ordinary Flex trees, resolved/local both. No character
  width approximation. Old glyph frames survive UI retirement.

## Actual checks and reproduction

| Check | Actual result |
| --- | --- |
| retained complete suite |456 passed |
| retained-text + retained-wgpu complete suite |489 passed,32 ignored |
| explicit Row builder WGPU case |1 passed,4 complete-pixel submissions |
| hello retained-wgpu, including actual Noto glyphs |48 passed,3 ignored |
| framework library Clippy `-D warnings` |passed |
| scoped framework/glyph rustfmt and git diff check |passed |

The final456/489 runs include all seven new regular cases, the288-case browser
matrix, wrapped builder and sibling-cycle fixture. Earlier453/486 runs did not
include all later additions and are not the final counts. Explicit hardware used
NVIDIA GeForce GTX1650Ti / Vulkan / driver571.96 and passed120/180/240/120px local
submissions against full ordinary Flex reference pixels. This is finite offscreen
correctness, not display-visible or modern heavy-effects/sustained qualification.

```powershell
cargo +1.98.1 test -p gpui-box --features retained --locked --offline --quiet
cargo +1.98.1 test -p gpui-box --features retained-text,retained-wgpu --locked --offline --quiet
cargo +1.98.1 test -p gpui-box --features retained-text,retained-wgpu --locked --offline --quiet hardware_row_inline -- --ignored --nocapture
cargo +1.98.1 test -p gpui-box-example-hello --features retained-wgpu --locked --offline --quiet
cargo +1.98.1 clippy -p gpui-box --features retained-text,retained-wgpu --lib --no-deps --locked --offline -- -D warnings
```

Suites overlap; ignored cases count as GPU evidence only when explicitly run.
The stage changes no vendor code/manifest/features; no new vendor suite result
is claimed. Existing retained-only unused SpatialUpdate fields and dependency
linker messages remain; Clippy claims framework library scope. One initial
compile attempted a private Taffy is_row method; explicit public enum matches
now classify direction without widening the engine API. The former fixed-host
Row rejection fixture now includes a genuinely content-dependent sibling;
one exact numeric-width item alone is intentionally safe under the new proof.
All broader acceptance gates and extra scan/Style-copy costs are in the ADR.
