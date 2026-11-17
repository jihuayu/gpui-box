# Public Grid evidence, 2026-11-17

Worktree: `D:\oss\ggpui-worktrees\rendering-performance-research`.
Branch: `codex/rendering-performance-research`.
Base: `45a49ad52f8515efcb044cbf26b720357ebf6a7f`.
See [contract and ADR](../../RETAINED_PUBLIC_GRID.md).
Full RFC goal remains active. No timing/memory/sustained-frame gain is claimed.

## Independent browser matrix

Chromium145.0.7632.6/Windows,1,152cases all match within0.06logical px with no
exclusions. Six templates (minmax-fr, percentage+fr, fixed-repeat, auto-fit,
auto-fill, intrinsic+fit-content) × four flows × LTR/RTL ×160/260 widths ×
explicit placement on/off × stretch/start/center × two content-height sets.
Rows include minmax/Auto and fixed tracks; implicit columns/rows use repeating
patterns. Explicit placement includes span2, negative lines, row span2, item
alignment overrides and order. Compare root size and each item's x/y/width/height.

Initial48mismatches all involved intrinsic templates: the fit-content track's
minimum contribution could fall below item padding+border with explicit min0.
The fork now floors that outer contribution. These failed runs are not counted
as passes; all original cases pass after the kernel fix. A standalone Taffy
regression covers9padding/border combinations in an overflowing zero-width Grid.

```powershell
node docs/performance/2026-11-17-public-grid/capture.mjs D:/Scoop/persist/nodejs/bin/node_modules/agent-browser/node_modules/playwright-core/index.mjs
```

## Framework controls

- Typed source updates change2→3columns, span/order and Grid→Flex→Grid through
 both resolved/local atomic frames; complete CPU pixels match fresh full trees.
 Final-value coalescing retains revision, next unchanged frame has0layout, old
 packets remain consumable after owner drop.
- Invalid numeric tracks, repeats, auto-repeat grammar, zero lines/spans,
 fit-content bounds, alignment safety and Flex-axis gaps reject atomically,
 preserving source/stats/published revision.
- Fixed-width auto-fit Grid root query uses captured session and ordinary typed
 source160→240 to select descendant10→30height; root160×23→240×30.
- A stretched Grid host below Column reads180px in an inline builder and builds
 two independent tracks with30/50content heights, giving180×50.
- Hard tight bounds override preferred size; isolated Min/MaxContent subtree
 queries preserve published frame, and Size containment uses explicit12px
 intrinsic fallback rather than descendant height.
- Ordinary content-dependent Grid item queries reject. Keyed insertion/removal/
 reorder retains per-key state and complete CPU equality against fresh trees.
- Real bundled Noto paragraphs use1fr/2fr tracks over180/260/120/180 widths,
 including a content replacement. Independently calculated widths determine
 paragraph wraps/baselines/root height and absolute-positioned CPU glyph
 reference pixels. Local/resolved and retained old packets are covered.

## Checks

| Check | Actual result |
| --- | --- |
| retained complete suite |473 passed |
| retained-text + retained-wgpu complete suite |506 passed,34 ignored |
| hello retained-wgpu including actual Noto |50 passed,4 ignored |
| explicit Grid update GPU |1 passed,4 complete-pixel submissions |
| explicit real Grid glyph GPU |1 passed,8 complete-pixel submissions |
| vendored Taffy default / project-feature lib tests |229 /223 passed |
| vendor no-default / alloc+tree+Flex / all-features checks |passed |
| independent browser |1,152cases all match,no exclusions |
| framework library Clippy `-D warnings` |passed |
| scoped framework/glyph rustfmt and diff check |passed |

All explicit GPU work used NVIDIA GeForce GTX1650Ti / Vulkan / driver571.96.
Grid update submissions cover2→3columns/spans/order→Flex→Grid. Actual glyph
submissions cover180/260/120/180 widths in each local/resolved mode against
independently calculated absolute-placement references, including text replacement.
Seven regular framework tests, one standalone vendor test with9combinations and
one regular glyph test were added. The final473core count includes the last
keyed-topology regression; earlier472does not. Text/WGPU506includes it.

Vendor tests use the existing temporary low-memory test-profile controls, not
production profile changes. Suites overlap; ignored hardware only counts when run.

```powershell
cargo +1.98.1 test -p gpui-box --features retained --locked --offline --quiet
cargo +1.98.1 test -p gpui-box --features retained-text,retained-wgpu --locked --offline --quiet
cargo +1.98.1 test -p gpui-box --features retained-text,retained-wgpu --locked --offline --quiet hardware_public_grid -- --ignored --nocapture
cargo +1.98.1 test -p gpui-box-example-hello --features retained-wgpu --locked --offline --quiet
cargo +1.98.1 test -p gpui-box-example-hello --features retained-wgpu --locked --offline --quiet hardware_public_grid -- --ignored --nocapture
cargo +1.98.1 clippy -p gpui-box --features retained-text,retained-wgpu --lib --no-deps --locked --offline -- -D warnings
```

Initial framework fixture compile used ambiguous glob imports and returned a
bool from a unit frame callback; corrected with explicit types/return mapping.
Existing retained-only unused SpatialUpdate fields and dependency linker
messages remain. Library Clippy scope is separate from example/test linting.
Finite browser/CPU/offscreen GPU correctness does not establish native visible
presentation, heavy-effects120fps or sustained CPU fallback smoothness.
