# Content versus inherent sizing — 2026-11-17

Windows x64 / Rust1.98.1, locked/offline in the original research worktree.

```powershell
cargo +1.98.1 test -p gpui-box --features retained --locked --offline --quiet
cargo +1.98.1 test -p gpui-box --features retained-text,retained-wgpu --locked --offline --quiet
cargo +1.98.1 test -p gpui-box --features retained-text,retained-wgpu --locked --offline parent_layout::content_sizing_tests -- --include-ignored --nocapture
cargo +1.98.1 test -p gpui-box-example-hello --features retained-wgpu --locked --offline --quiet
cargo +1.98.1 clippy -p gpui-box --features retained-text,retained-wgpu --lib --no-deps --locked --offline -- -D warnings
cargo +1.98.1 test --manifest-path vendor/taffy/Cargo.toml --lib --locked --offline -j 1 --quiet
$env:CARGO_PROFILE_TEST_DEBUG = '0'
$env:CARGO_PROFILE_TEST_INCREMENTAL = 'false'
$env:CARGO_PROFILE_TEST_CODEGEN_UNITS = '256'
cargo +1.98.1 test --manifest-path vendor/taffy/Cargo.toml --lib --locked --offline -j 1 --no-default-features --features std,taffy_tree,flexbox,grid,block_layout,float_layout,content_size --quiet
cargo +1.98.1 check --manifest-path vendor/taffy/Cargo.toml --locked --offline --no-default-features --quiet
cargo +1.98.1 check --manifest-path vendor/taffy/Cargo.toml --locked --offline --no-default-features --features alloc,taffy_tree,flexbox --quiet
cargo +1.98.1 check --manifest-path vendor/taffy/Cargo.toml --locked --offline --all-features --quiet
git diff --check
```

The project-feature vendor test uses the lower-memory temporary test profile
established after the previous stage's LLVM OOM. No checked-in production
compiler configuration changes.

## Verified results

| Check | Observed result |
| --- | --- |
| retained full suite, after the final added planner test |400 passed |
| retained-text + retained-wgpu full suite |433 passed,22 ignored |
| explicit content-sizing suite including hardware |7 passed |
| hello retained-wgpu, including actual Noto glyph cases |39 passed,2 ignored |
| framework library Clippy with `-D warnings` | passed |
| vendored Taffy default library suite |219 passed |
| vendored Taffy project-feature library suite |213 passed |
| vendor no-default / alloc+tree+Flex / all-features checks | passed |
| new framework/glyph test rustfmt and `git diff --check` | passed |

Suites overlap; explicit hardware cases are counted only when actually run.
The first retained-only pipeline compiled before the final planner test and
reported399 passed; the final rerun above includes all400. The feature suite
already included the final test. Both vendor profiles and all following checks
completed successfully in the same fail-fast pipeline.

## Hardware correctness

The explicitly run seven-case content-sizing suite includes an actual NVIDIA
GeForce GTX 1650 Ti, Vulkan, NVIDIA driver571.96. Three local submit/readbacks
switch Inherent→Content→Inherent and compare every pixel with independently
constructed ordinary forced-full Flex controls. This verifies this finite
offscreen consumption path, not display-visible presentation or frame timing.

## Evidence

Before the fix, a raw ContentSize+MaxContent query of an authored min-content
leaf returned20 instead of100. The common child entry now skips own preferred
keyword resolution for ContentSize. Framework and standalone vendor tests cover
size/baseline queries and Content→Inherent→Content transitions, with20/100 distinct.

Framework query matrices cover leaf, ordinary Flex, ConstraintBox and owned
parent: own preferred200×70, min150×50, max250×90, aspect2, padding3 and border2.
Content Min/MaxContent returns30×20 /110×20; selected width60 returns60×20; tight
25×15 remains25×15. Descendant authored40×12 remains effective, yielding50×22.
Owned callbacks and inline/full planners read the actual mode on repeated query
transitions. Exact full allocation readback/builder mode and conflicting request
enforcement are asserted.

Normal typed parent snapshot mode switches Inherent→Content→Inherent→Content
change a child from200×70 to110×20. Resolved/local complete child/leaf LayoutResult
and CPU pixels compare with ordinary forced-full Flex controls whose own queried
size/limits are removed explicitly. Source restoration coalesces to0 layout,
keyed construction compares complete CPU pixels and old packets survive owner
retirement. Controls are same-kernel finite cases, not broad CSS conformance.

Actual bundled Noto glyphs use selected widths60/140/90/60 under measured and
independent parent policies. Content mode ignores the queried text node's
authored preferred/min300px height and180px width, retaining actual wrapping;
full host/leaf/footer geometry, explicit queries and all CPU glyph pixels compare
with ordinary Flex controls without those own limits. Old glyph packets survive
owner retirement.

## Limits

Ignored hardware cases count only when run explicitly; suites overlap. Only
framework library Clippy is claimed. This adds capability and fixes an entry
semantic; no time/memory reduction, p99, sustained complete CPU frames or visible
60/120Hz result is claimed. Temporary public/callback payload grows; persistent
kernel layout cache, node, GPU and FramePacket formats do not grow.
