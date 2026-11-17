# Independent allocation item styles — 2026-11-17

Windows x64 / Rust1.98.1, locked/offline in the existing research worktree.

```powershell
cargo +1.98.1 test -p gpui-box --features retained --locked --offline --quiet
cargo +1.98.1 test -p gpui-box --features retained-text,retained-wgpu --locked --offline --quiet
cargo +1.98.1 test -p gpui-box --features retained-text,retained-wgpu --locked --offline parent_layout::allocation_style_tests -- --include-ignored --nocapture
cargo +1.98.1 test -p gpui-box-example-hello --features retained-wgpu --locked --offline --quiet
cargo +1.98.1 clippy -p gpui-box --features retained-text,retained-wgpu --lib --no-deps --locked --offline -- -D warnings
cargo +1.98.1 test --manifest-path vendor/taffy/Cargo.toml --lib --locked --offline -j 1 --quiet
cargo +1.98.1 test --manifest-path vendor/taffy/Cargo.toml --lib --locked --offline -j 1 --no-default-features --features std,taffy_tree,flexbox,grid,block_layout,float_layout,content_size --quiet
cargo +1.98.1 check --manifest-path vendor/taffy/Cargo.toml --locked --offline --no-default-features --quiet
cargo +1.98.1 check --manifest-path vendor/taffy/Cargo.toml --locked --offline --no-default-features --features alloc,taffy_tree,flexbox --quiet
cargo +1.98.1 check --manifest-path vendor/taffy/Cargo.toml --locked --offline --all-features --quiet
git diff --check
```

## Results

| Check | Result |
| --- | --- |
| Framework retained | 388 passed |
| Framework text/WGPU | 421 passed, 20 ignored |
| Explicit allocation-style suite, including actual WGPU | 7 passed |
| Hello retained-wgpu, including actual shaped glyph allocation | 35 passed, 2 ignored |
| Taffy default library suite | 218 passed |
| Taffy project-feature library suite | 212 passed after the compile-memory retry below |
| Taffy no-default / alloc+tree+Flex / all-features checks | Passed |
| Framework library Clippy with warnings denied | Passed |
| New allocation-style test file rustfmt / git diff check | Passed |

The initial project-feature test compile failed with LLVM out-of-memory before
running tests, even with one Cargo job. Retrying only the failed suite and the
remaining feature checks succeeded with these temporary test-profile settings:

```powershell
$env:CARGO_PROFILE_TEST_DEBUG = '0'
$env:CARGO_PROFILE_TEST_INCREMENTAL = 'false'
$env:CARGO_PROFILE_TEST_CODEGEN_UNITS = '256'
cargo +1.98.1 test --manifest-path vendor/taffy/Cargo.toml --lib --locked --offline -j 1 --no-default-features --features std,taffy_tree,flexbox,grid,block_layout,float_layout,content_size --quiet
```

No runtime implementation or checked-in production build configuration changed
for this retry. The failed compile is not counted as a passing run.

## Evidence

A client independent allocator reads direct-child grow/basis/hidden values,
assigns enforced complete border-box slots and builds ordinary wrapped Flex
descendants. Width/weights90(1,2) →180(2,1) →120(1,2) →90(1,2), resolved/local
outputs, complete host LayoutResult, hits and all CPU pixels compare with an
ordinary forced-full Flex reference. Old packets survive UI retirement.

Both atomic output variants reject late host/sibling self-alignment changes,
image intrinsic/replaced inputs, decoded image installation and changed grow.
Alignment/image cases preserve the same numerical slot but must still invalidate
the observed dependency. Sources, stats and previous frame remain unchanged.
Two-level allocator tests separately change an inner peer and an outer peer.
Restored style writes coalesce; builder-owned descendants remain changeable.
Typed style updates invalidate cached parents, and restored P0 writes avoid layout.

Computed style sinks registered after responsive rules change a sibling custom
allocator's basis20→100. The outer ordinary Flex query host becomes200px and its
consumer remains below250, for ordinary responsive output and structural edits.
This checks indirect layout-target discovery and query stage ordering. It does
not qualify query hosts below arbitrary full allocators.

Hidden/reverse order/restoration/reparent follow node identity. The vendor cache
test reads grow in the independent phase and validates width30/50/30 allocation.
Bundled Noto glyph basis60/140/90/60 drives the independent phase; complete
host/leaf/footer results, explicit query and all CPU glyph pixels compare with
ordinary full Flex. Old owned glyph packets survive owner retirement.

An explicit hardware test submits local packets at width90/180/90 with changing
weights, then compares every readback pixel with a fresh ordinary full Flex
reference. It passed on NVIDIA GeForce GTX 1650 Ti, Vulkan, NVIDIA driver571.96.
This is offscreen correctness and frame-consumption evidence; it does not
establish display-visible presentation or frame timing.

## Limits

No timing/memory/p99 improvement or sustained-presentation claim. Same-kernel
finite controls do not prove broad CSS conformance or arbitrary client policy
correctness. Focused/full suites overlap, ignored hardware cases only count as
passed when run explicitly, and only library Clippy is claimed.
