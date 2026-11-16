# Flex geometry reference, 2026-11-16

The [standalone fixture](browser-reference.html) computes CSS geometry without
calling framework or Taffy code. Open it in Chromium; the page title indicates
PASS/FAIL and the page prints `window.flexReference`. The checked-in
[result](browser-reference.json) was captured with headless Chromium
145.0.7632.6 on Windows: **24 passed, 0 failed**.

The cases cover min/max/fit-content/stretch widths, column wrapping with a
minimum greater than maximum, max-width and padding, aspect-ratio transfer,
and root/nested percentage widths/heights against intrinsic and definite sizes.
Chromium reports `CSS.supports('width', 'fit-content(50px)') == false`; numeric
fit-content functions are covered by framework regressions, not this browser
reference. This fixture is a small independent geometry check, not a WPT run,
rendering pixel comparison or performance benchmark.

Library validation commands (Rust 1.98.1):

```powershell
cargo +1.98.1 test --locked --offline -p gpui-box --features retained --lib
cargo +1.98.1 test --locked --offline -p gpui-box --features retained-text,retained-wgpu --lib
cargo +1.98.1 clippy --locked --offline -p gpui-box --features retained-text,retained-wgpu --lib -- -D warnings
cargo +1.98.1 test --manifest-path vendor/taffy/Cargo.toml --offline --lib
cargo +1.98.1 test --manifest-path vendor/taffy/Cargo.toml --offline --no-default-features --features std,taffy_tree,flexbox,grid,block_layout,float_layout,content_size --lib
cargo +1.98.1 check --manifest-path vendor/taffy/Cargo.toml --offline --no-default-features --features std,taffy_tree,flexbox
cargo +1.98.1 test --locked --offline -p gpui-box-example-hello --features retained-wgpu
```

Results: framework **174 / 207**, Taffy **174 / 168**, Clippy and flex-only
check passed; hello **11 passed, 1 hardware case ignored**. Existing diagnostics
and scope limits are recorded in the
[contract](../../RETAINED_FLEX_SIZING_QUERIES.md). No latency or sustained fps
claims are made for this change.
