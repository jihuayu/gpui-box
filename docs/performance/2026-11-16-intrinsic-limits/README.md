# Intrinsic minimum/maximum evidence

Date: 2026-11-16. Windows, Rust 1.98.1, offline dependencies. This records
layout contract validation, not a rendering performance benchmark.

## Independent browser reference

[browser-reference.html](browser-reference.html) uses a separate DOM/CSS
fixture. [capture.mjs](capture.mjs) launches installed Playwright Chromium and
writes [browser-reference.json](browser-reference.json). The recorded browser
is Chromium **145.0.7632.6**; **47 assertions pass, zero fail**.

Coverage: root/nested Row/Column width limits and wrapping (20), Flex shrink
(3), box-sizing/containment (6), small authored-height caps (2), auto-height
percentage definiteness (4), actual Grid/Block hosts (8), and preferred keyword
or ratio interactions (4).

Two cases in the JSON's `deviations` array explicitly record different browser
and framework outputs for fixed preferred height plus intrinsic limits and
percentage descendants. They are **not counted as passing compatibility**.
Fit-content numeric functions are unsupported by this Chromium and are covered
only by native tests. Fixed div children do not establish font/shaping parity.

```powershell
node docs/performance/2026-11-16-intrinsic-limits/capture.mjs D:/Scoop/persist/nodejs/bin/node_modules/agent-browser/node_modules/playwright-core/index.mjs
```

The installed module entry is an environment path; another machine must pass
its own Playwright-compatible entry and install the browser separately.

## Native checks

```powershell
cargo +1.98.1 test --locked --offline -p gpui-box --features retained --lib --quiet
cargo +1.98.1 test --locked --offline -p gpui-box --features retained-text,retained-wgpu --lib --quiet
cargo +1.98.1 test --manifest-path vendor/taffy/Cargo.toml --offline --lib --quiet
cargo +1.98.1 test --manifest-path vendor/taffy/Cargo.toml --offline --no-default-features --features std,taffy_tree,flexbox,grid,block_layout,float_layout,content_size --lib --quiet
cargo +1.98.1 clippy --locked --offline -p gpui-box --features retained-text,retained-wgpu --lib -- -D warnings
cargo +1.98.1 test --locked --offline -p gpui-box-example-hello --features retained-wgpu --quiet
cargo +1.98.1 check --manifest-path vendor/taffy/Cargo.toml --offline --examples --quiet
cargo +1.98.1 check --manifest-path vendor/taffy/Cargo.toml --offline --no-default-features --features std,taffy_tree,flexbox,content_size --lib --quiet
cargo +1.98.1 check --manifest-path vendor/taffy/Cargo.toml --all-features --lib --quiet
```

Results: retained **198**, combined text/WGPU **231**, Taffy default **181**,
Taffy project configuration **175** passed. Strict library Clippy and all listed
checks passed. The all-feature check needed an initial online dependency fetch;
the other listed checks ran offline. Hello **12 passed, 1 ignored**.
Detailed limitations are recorded in the [contract](../../RETAINED_INTRINSIC_LIMITS.md).
The hello suite uses the current
worktree; unrelated pre-existing `retained_demo.rs` edits are excluded from this
stage's commit. Its hardware GPU test retains its existing ignored status.

No timing samples or new GPU/CPU frame-delivery qualification were collected.
Extra intrinsic queries and cache-mode changes are correctness costs pending
profiling, not a demonstrated speedup.
