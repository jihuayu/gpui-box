# Percentage Row allocation evidence, 2026-11-17

Worktree: `D:\oss\ggpui-worktrees\rendering-performance-research`.
Branch: `codex/rendering-performance-research`.
Base: `ba289ba1e174c53052b5a888a1586aefc96c8c68`.
See [contract and ADR](../../RETAINED_PERCENTAGE_INLINE_ALLOCATION.md).
Full RFC acceptance remains open. No frame-time/peak-memory/FPS gain is claimed.

## Independent browser controls

Chromium145.0.7632.6/Windows reference:432 cases, all pass without exclusions
within0.06 logical px. Row/RowReverse × three wrap modes × LTR/RTL ×120/180/260
widths × percent-basis/Auto-with-percent-width/mixed-Px-percent-basis × two box
sizing models × two content-height sets. Grow1/2/1, percent min/max, padding1%,
inline margin1.5%, border1px and gap4px. Descendant width400 deliberately overflows.
Compare root width/height and each item's x/y/width/height after proving all
three incoming chains. The unconditional used-width proof separately rejects
the same percentage inputs without an external-anchor contract.

```powershell
node docs/performance/2026-11-17-percentage-inline-allocation/capture.mjs D:/Scoop/persist/nodejs/bin/node_modules/agent-browser/node_modules/playwright-core/index.mjs
```

## Normal framework controls

- Percent25% sidebar and flexible sibling under120/240/160 root widths; nested
 50% Row under the sidebar gives12.5% final width. Builders observe actual
 known-width/definiteness/percentage-basis fields, select keyed20/60 content
 height and overflow400px. Local/resolved complete CPU pixels match freshly
 constructed full ordinary Flex references. Old packets survive owner drop.
- A later-registered sibling percentage/grow producer runs before host query.
 Root160→240→180→160 yields host80→75→56.25→80 and20/60 content selection.
- A scope-owned query automatically resettles after a typed sibling source
 update in a captured session:180px root gives90×60 then56.25×20. Repeating the
 source value keeps the frame revision.
- Unresolved percent roots in normal Unbounded/MinContent/MaxContent reject
 before builder callbacks; unanchored query evaluation rejects without publishing.
 Numeric180px and hard-tight180px roots remain usable in normal Unbounded space.
- ConstraintBox, intrinsic-root and custom independent width-phase ancestry
 conservatively reject the new percentage proof. An unsafe reachable sibling
 branch rejects registration. Computed query→percentage sibling feedback rejects.
- A late overridden preferred-width edit preserves numerical basis/allocation
 but invalidates observed sibling inputs; source/stats/published revision remain.
- Real bundled Noto glyphs reuse the Row builder/query composition test with25%
 bases. Padded host allocation is width/2, content width/2−6; normal/50%-width
 text selection matches fresh complete Flex LayoutResult/baselines and all CPU
 glyph pixels, both local/resolved, with retained old-frame consumption.

## Actual checks and reproduction

| Check | Actual result |
| --- | --- |
| retained complete suite |466 passed |
| retained-text + retained-wgpu complete suite |499 passed,33 ignored |
| explicit percentage builder WGPU case |1 passed,3 complete-pixel submissions |
| hello retained-wgpu, including actual Noto glyphs |49 passed,3 ignored |
| independent browser geometry |432 cases all match,0 exclusions |
| framework library Clippy `-D warnings` |passed |
| scoped rustfmt and git diff check |passed |

Explicit hardware: NVIDIA GeForce GTX1650Ti / Vulkan / driver571.96. Root widths
120/240/160px produce local frames, each submitted/read back and compared against
all full ordinary-Flex reference pixels. This is finite offscreen correctness;
modern desktop heavy effects, native presentation and sustained qualification
remain open. Ten regular framework tests and one real-glyph test were added.

The stage changes no vendor code/manifest/features, so no new vendor run is
claimed. Suites overlap; ignored tests are hardware evidence only when run.

```powershell
cargo +1.98.1 test -p gpui-box --features retained --locked --offline --quiet
cargo +1.98.1 test -p gpui-box --features retained-text,retained-wgpu --locked --offline --quiet
cargo +1.98.1 test -p gpui-box --features retained-text,retained-wgpu --locked --offline --quiet hardware_percentage_inline -- --ignored --nocapture
cargo +1.98.1 test -p gpui-box-example-hello --features retained-wgpu --locked --offline --quiet
cargo +1.98.1 clippy -p gpui-box --features retained-text,retained-wgpu --lib --no-deps --locked --offline -- -D warnings
```

Initial compilation used an incorrect public ConstraintFit alias and a shadowed
test helper; both corrected to the actual API. Existing retained-only unused
SpatialUpdate fields and dependency linker messages remain. Costs and broader
acceptance boundaries are in the ADR; browser geometry/CPU/offscreen GPU do not
establish native visible or sustained modern desktop/CPU smoothness.
