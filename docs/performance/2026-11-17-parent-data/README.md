# Parent data validation — 2026-11-17

Windows x64, Rust 1.98.1, locked/offline, existing research worktree. This stage
extends an input/dependency API; it contains no timing or speedup result.

## Reproduction

```powershell
cargo +1.98.1 test -p gpui-box --features retained --locked --offline --quiet
cargo +1.98.1 test -p gpui-box --features retained-text,retained-wgpu --locked --offline --quiet
cargo +1.98.1 test -p gpui-box --features retained-text,retained-wgpu --locked --offline parent_data_tests -- --include-ignored --nocapture
cargo +1.98.1 test -p gpui-box-example-hello --features retained-wgpu --locked --offline --quiet
cargo +1.98.1 test --manifest-path vendor/taffy/Cargo.toml --lib --locked --offline --quiet
cargo +1.98.1 test --manifest-path vendor/taffy/Cargo.toml --locked --offline --no-default-features --features std,taffy_tree,flexbox,grid,block_layout,float_layout,content_size --lib --quiet
cargo +1.98.1 check --manifest-path vendor/taffy/Cargo.toml --locked --offline --no-default-features --quiet
cargo +1.98.1 check --manifest-path vendor/taffy/Cargo.toml --locked --offline --no-default-features --features alloc,taffy_tree,flexbox --quiet
cargo +1.98.1 check --manifest-path vendor/taffy/Cargo.toml --locked --offline --all-features --quiet
cargo +1.98.1 clippy -p gpui-box --features retained-text,retained-wgpu --lib --no-deps --locked --offline -- -D warnings
git diff --check
```

Final results:

| Check | Result |
| --- | --- |
| Framework retained | 351 passed |
| Framework retained-text + retained-wgpu | 384 passed, 14 ignored |
| Explicit parent-data suite, including actual GPU | 9 passed |
| Hello retained-wgpu | 29 passed, 2 ignored |
| Taffy default library / project-feature library | 207 / 201 passed |
| Taffy no-default, alloc/Flex and all-feature checks | All passed |
| Framework library Clippy, warnings denied | Passed |
| git diff --check | Passed |

The focused nine-case run overlaps the ordinary suite and explicitly enables
one normally ignored GPU test. It must not be added as nine new distinct passes.

## Coverage and interpretation

Framework client: two content-sized children receive tight inline/unbounded
block slots from authored weights. Outer widths90/180/120/90 and changed weights
reflow three20x10 wrapping items per host. Both resolved and local CPU complete
images match a fresh ordinary full Flex tree with basis0/grow/min-width0; no
custom algorithm or builder is present in that reference. Old frames survive
owner drop. Actual GPU local submissions at90/180/90 match the same full tree.
Adapter: NVIDIA GeForce GTX1650Ti, Vulkan, driver571.96. Offscreen pixels are not
visible presentation, modern-GPU heavy-effects or sustained-frame evidence.

Source/transaction cases cover same-handle skips, different-handle equality,
intermediate writes restored to the original, typed sinks invalidating cached
parent allocation, reordered node metadata, measured-only metadata reads,
reparent without rewrite, wrong/missing/invalid data atomic rejection, late
same-number sibling/host metadata replacement rejection, and same-edit keyed
creation/builder/retirement. A weak token verifies metadata releases when render
owners retire while old packets retain valid pixels. An indirect computed
metadata sink added after container rules joins dependency sorting in ordinary
and structural responsive paths.

Vendor regression covers exact-cache hits, clone independence, hidden retention
and restoration, invalidation on new data, reparent, missing data failure,
deletion/generation reuse, invalid handles and final release across clear/clones.
Metadata reads and independent allocation reject out-of-range child indices.

Real bundled Noto Sans CJK SC text uses typed ParagraphFraction metadata for
width60/120/90/60, keyed paragraph/footer branch selection, real wrapping and
footer positions. CPU complete images/hits equal ordinary forced-full Flex;
glyphs remain consumable after UI/font/raster/paragraph owners drop. This is
rendering correctness, not an independent browser conformance expansion.

Costs/limits: [public contract and ADR](../../RETAINED_PARENT_DATA.md). Existing
retained-only SpatialUpdate unused-field and Windows linker notices remain.
Ignored adapter tests are not counted as passed; focused suite overlaps the
full suite. All-targets Clippy is not part of this stage's pass claim.
