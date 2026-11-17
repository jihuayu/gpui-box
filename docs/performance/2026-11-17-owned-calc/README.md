# Owned mixed-length evidence, 2026-11-17

Worktree: `D:\oss\ggpui-worktrees\rendering-performance-research`.
Branch: `codex/rendering-performance-research`.
Base: `e78d579292de357a3924f2e94255556133e060c8`.
See [contract and ADR](../../RETAINED_OWNED_CALC.md).
Full goal remains active. No timing/peak-memory/sustained-frame gain claimed.

## Browser reference

Chromium145.0.7632.6/Windows:336cases all match within0.1logical px, no exclusions.
Seven modes (Row/reverse/Column/Grid/Grid-fit-content/Grid-auto-fit/absolute) ×
LTR/RTL ×120/180/260 widths × positive/negative used width × two box-sizing
models × two content heights. Mixed margins/padding/gaps/bases/min/max/track
limits/insets are evaluated through their normal property contexts. Parent
containing block is400×400; root is positioned with160height and mixed padding.

Maximum raw deltas: Row/reverse0.06875px, Column0.07999, Grid0.03075,
Grid-fit0.03890, Grid-repeat0.02635, absolute0.02501. Browser fixed-point rounding
accumulates through nested box sizing. Initial0.07tolerance rejected8Column
cases differing by0.08px; it is explicitly0.1now. These are numerical differences,
not falsely reported exact matching. Raw TSV/reference metadata are committed.

Initial240/240failures used default1280browser containing width for root percent
padding versus400in Rust. Set a positioned400×400body to match. Subsequent
48absolute mismatches used a static Rust host versus positioned browser host;
set Relative in Rust to match containing-block semantics. Neither fixture change
changed runtime algorithms. Failed runs are not counted as passing evidence.

```powershell
node docs/performance/2026-11-17-owned-calc/capture.mjs D:/Scoop/persist/nodejs/bin/node_modules/agent-browser/node_modules/playwright-core/index.mjs
```

## Framework controls

- Public mixed width50%−12 and padding1%+2 over120/240/160/120 constraints match
 independently numeric full-tree CPU pixels, local/resolved; typed px coefficient
 updates−4/−20/−12 also match, P0 final-value restore keeps revision, invalid
 coefficients roll back source/stats/frame. Old packets survive owner drop.
- Mixed Row basis/minimum under an anchored definite chain composes inline
 builder and scope-owned content query; builder reads half-width and selects
20/60content heights without feedback. Unanchored mixed root with0% remains
indefinite and rejects builder, normal layout shrink-wraps content; hard-tight
bounds still override it.
- Mixed fit-content preferred/max limits and loose ConstraintBox inputs match
 numerically resolved controls across three root widths, complete CPU pixels.
- Actual custom parent child-style loop preserves unresolved Calc/FitContentCalc
 dimensions and mixed padding with its ordinary full layout path.
- Real bundled Noto text uses75%−12width and1%+2padding over120/240/180/120widths;
 actual wrap height/baseline and all CPU glyph pixels match numeric references,
 both local/resolved. Old glyph packets remain valid after owner retirement.

## Actual checks

| Check | Actual result |
| --- | --- |
| retained complete suite |478 passed |
| retained-text + retained-wgpu complete suite |511 passed,35 ignored |
| hello retained-wgpu with actual Noto |51 passed,5 ignored |
| explicit mixed-length GPU case |1 passed,4 complete-pixel submissions |
| explicit real glyph GPU case |1 passed,8 complete-pixel submissions |
| vendor default / affine+serde / project-affine lib suites |229 /234 /225 passed |
| vendor no-default / alloc+tree+Flex+affine / all-features checks |passed |
| Chromium independent geometry |336cases pass,no exclusions |
| framework library Clippy `-D warnings` |passed |
| scoped framework/glyph rustfmt and diff check |passed |

Explicit GPU: NVIDIA GeForce GTX1650Ti / Vulkan / driver571.96. The ordinary
case submits120/240/160/120 local snapshots against numeric full references.
Glyphs submit120/240/180/120 in each local/resolved mode against numeric controls.
All pixels agree. Five regular framework tests, two regular vendor tests plus
one serde-feature test, and one regular glyph test were added. Existing child
style tests gained unresolved mixed values. Suites overlap; ignored cases count
as hardware evidence only when explicitly run. Vendor profile uses established
temporary low-memory settings; production settings unchanged.

```powershell
cargo +1.98.1 test -p gpui-box --features retained --locked --offline --quiet
cargo +1.98.1 test -p gpui-box --features retained-text,retained-wgpu --locked --offline --quiet
cargo +1.98.1 test -p gpui-box --features retained-text,retained-wgpu --locked --offline --quiet hardware_owned_calc -- --ignored --nocapture
cargo +1.98.1 test -p gpui-box-example-hello --features retained-wgpu --locked --offline --quiet
cargo +1.98.1 test -p gpui-box-example-hello --features retained-wgpu --locked --offline --quiet hardware_owned_calc -- --ignored --nocapture
cargo +1.98.1 clippy -p gpui-box --features retained-text,retained-wgpu --lib --no-deps --locked --offline -- -D warnings
```

Initial compile fixes: CompactLength wrapper field access, exhaustive public
Length pattern and legacy FlexStyle rejection branch. P0 stats fixture initially
compared against before the successful no-op transaction's accounting; now takes
the rollback baseline afterwards. Static size expectation was corrected from
an estimate to measured776/744bytes. Existing retained-only unused SpatialUpdate
fields and dependency linker messages remain. Finite correct output does not
prove sustained/native/modern desktop120Hz or CPU fallback acceptance.
