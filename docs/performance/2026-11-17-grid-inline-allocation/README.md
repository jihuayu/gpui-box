# Grid inline allocation evidence, 2026-11-17

Worktree: `D:\oss\ggpui-worktrees\rendering-performance-research`.
Branch: `codex/rendering-performance-research`.
Base: `39381213ed564d896f27994acf5c15c3163fc60e`.
See [contract and ADR](../../RETAINED_GRID_INLINE_ALLOCATION.md).
Full RFC goal remains active. No wall-time/peak-memory/FPS gain is claimed.

## Independent Chromium reference

Chromium145.0.7632.6/Windows:1,152cases all match within0.06logical px without
exclusions. Six column templates (fixed-min fr, bounded minmax, percentage+fr,
Count repeat, AutoFit, AutoFill) × four Row/Column/Dense flows × LTR/RTL ×
120/180/260 widths × explicit placement on/off × two box models × two content
height sets. Four direct items with400px overflowing content, border1/padding2/
inline margins2; Grid padding3/gap4. Two Auto rows establish a fixed row count;
implicit columns use minmax(20px,1fr). Explicit spans, negative lines, row spans
and item order alter placements. Every item first passes the complete incoming
proof; compare root size and all item x/y/width/height to browser output.

```powershell
node docs/performance/2026-11-17-grid-inline-allocation/capture.mjs D:/Scoop/persist/nodejs/bin/node_modules/agent-browser/node_modules/playwright-core/index.mjs
```

## Normal framework controls

- Minmax0/1fr+20/2fr Grid item receives one-third width; nested identical Grid
 receives one-ninth. Builders across120/240/160/120 choose keyed20/60heights,
 normal rows resize and local/resolved CPU pixels match fresh full trees. Old
 packets survive owner drop. Initial test formula subtracted the20px minimum
 twice; actual normal fr allocation uses it as a floor, corrected to w/3,w/9.
- Mixed Grid→Row percentage-basis composition:240px root gives Grid item80px
 and Row child40px; preferred50%-wide centered Grid item gives40px and20px.
- Later-registered sibling placement producer claims first cell at180px; host
 moves from first to second track before query selection.120/180/240/120 roots
 give host40/120/160/40 and corresponding20/60content height.
- Captured scope-owned session automatically responds to sibling typed placement
 source:180root host60×20→120×60. Repeated source keeps revision.
- Default implicit Auto, automatic-min fr, auto-repeated rows, shrink-wrap item,
 explicit/effective image ratio and unanchored input reject before callback.
 Unsafe reachable authored sibling branches and computed allocation feedback
 reject rather than use current equal results as proof.
- Late sibling height input, Grid row policy or child order changes reject
 even with unchanged numeric width, preserving sources/stats/published revision.
- Real bundled Noto text in a padded Grid item composes builder and scope-owned
 content query. Root120/180/240/120 yields half-width host, content−6; query
 selects normal/50% paragraph width. Layout/baselines/height and complete CPU
 glyph pixels match full ordinary Grid; local/resolved and old frames covered.

## Checks

| Check | Actual result |
| --- | --- |
| retained complete suite |486 passed |
| retained-text + retained-wgpu complete suite |519 passed,36 ignored |
| hello retained-wgpu with actual Noto |52 passed,6 ignored |
| explicit Grid item builder GPU |1 passed,4 complete-pixel submissions |
| explicit real Grid item glyph GPU |1 passed,8 complete-pixel submissions |
| Chromium independent geometry |1,152cases all match,no exclusions |
| framework library Clippy `-D warnings` |passed |
| scoped framework/glyph rustfmt and diff check |passed |

Explicit hardware uses NVIDIA GeForce GTX1650Ti / Vulkan / driver571.96. The
builder case submits120/240/160/120local frames against full ordinary Grid.
Glyphs use120/180/240/120in each local/resolved mode, with actual builder/query
selection against full ordinary Grid references. All pixels agree. Eight regular
framework cases and one regular glyph case were added. No vendor code/features
changed, so no new vendor-suite run is claimed. Suites overlap; ignored cases
only count as GPU evidence when explicitly run. Final observations use an
optional boxed Grid Style to avoid inlining its payload in every Row/custom
record; allocation/copy costs are documented without a measured gain claim.

```powershell
cargo +1.98.1 test -p gpui-box --features retained --locked --offline --quiet
cargo +1.98.1 test -p gpui-box --features retained-text,retained-wgpu --locked --offline --quiet
cargo +1.98.1 test -p gpui-box --features retained-text,retained-wgpu --locked --offline --quiet hardware_grid_inline -- --ignored --nocapture
cargo +1.98.1 test -p gpui-box-example-hello --features retained-wgpu --locked --offline --quiet
cargo +1.98.1 test -p gpui-box-example-hello --features retained-wgpu --locked --offline --quiet hardware_grid_item_real_text -- --ignored --nocapture
cargo +1.98.1 clippy -p gpui-box --features retained-text,retained-wgpu --lib --no-deps --locked --offline -- -D warnings
```

Existing retained-only unused SpatialUpdate fields and dependency linker
messages remain. Clippy claim is framework library scope. Finite CPU/browser/
offscreen GPU does not establish display-visible/native/modern desktop heavy
effects, sustained60/120Hz, input tails or CPU fallback smoothness acceptance.

The final expanded rejection fixture initially used Safe+Stretch, already
invalid in the public alignment API, so setup failed before the intended
builder guard. It now uses legal content-dependent MaxContent preferred width;
the final complete suites include this correction. Early485/518pass-plus-one-fail
runs are not the final results. The optional boxed observation follow-up is
covered by final core/text suites; GPU/glyph behavior was unchanged by boxing.
