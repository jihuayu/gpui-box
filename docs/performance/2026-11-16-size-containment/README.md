# Size containment validation, 2026-11-16

This is framework/layout correctness evidence, not a performance benchmark.
See the [contract and limits](../../RETAINED_SIZE_CONTAINMENT.md).

`browser-reference.html` runs independent browser layout and native CSS
container queries. `browser-reference.json` records Chromium 145.0.7632.6 on
Windows: **23 passed, zero failed**. It covers Normal/InlineSize/Size box
contributions in row/column parents, fluid query thresholds, image natural vs
authored ratios and layout-contained absolute placement across static parents.
One case intentionally records current browser `container-type:size` alone
using the viewport positioning block. The framework's stronger boundary uses
layout containment explicitly; following placement cases compare that combination.

Reproduce with an installed Playwright core and Chromium:

```powershell
node docs/performance/2026-11-16-size-containment/capture.mjs <absolute-path-to-playwright-core/index.mjs>
```

Native checks on Rust 1.98.1, Windows:

```powershell
cargo +1.98.1 test --locked --offline -p gpui-box --features retained --lib --quiet
cargo +1.98.1 test --locked --offline -p gpui-box --features retained-text,retained-wgpu --lib --quiet
cargo +1.98.1 test --manifest-path vendor/taffy/Cargo.toml --offline --lib --quiet
cargo +1.98.1 test --manifest-path vendor/taffy/Cargo.toml --offline --no-default-features --features std,taffy_tree,flexbox,grid,block_layout,float_layout,content_size --lib --quiet
cargo +1.98.1 clippy --locked --offline -p gpui-box --features retained-text,retained-wgpu --lib -- -D warnings
cargo +1.98.1 test --locked --offline -p gpui-box-example-hello --features retained-wgpu --quiet
cargo +1.98.1 check --manifest-path vendor/taffy/Cargo.toml --offline --no-default-features --features std,taffy_tree,flexbox,content_size --lib --quiet
```

Results: retained **186**, text/WGPU **219**, Taffy **177/171**; all passed.
Strict library Clippy and Flex-only check passed. Hello **11 passed / 1 ignored**,
using the current workspace including pre-existing example WIP outside this change.
After proof memoization the focused responsive suite also passed **24 tests**.

The new out-of-flow regression includes selective/full packet equivalence,
published-coordinate stability before frame publication, static-ancestor motion,
containing-block changes, nested positioning, local resolution/hit tests, old
snapshots, reparenting, append/remove index maintenance, and equal complete CPU
raster pixels with an explicit expected red pixel. No new hardware GPU pixels,
sustained refresh-rate or visible-present latency measurements were taken.
