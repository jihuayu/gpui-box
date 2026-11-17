# Owned container actual overflow — 2026-11-17

Windows x64 / Rust1.98.1 locked/offline in the existing research worktree.

```powershell
cargo +1.98.1 test -p gpui-box --features retained --locked --offline --quiet
cargo +1.98.1 test -p gpui-box --features retained-text,retained-wgpu --locked --offline --quiet
cargo +1.98.1 test -p gpui-box --features retained-text,retained-wgpu --locked --offline parent_layout::overflow_tests -- --include-ignored --nocapture
cargo +1.98.1 test -p gpui-box-example-hello --features retained-wgpu --locked --offline --quiet
cargo +1.98.1 clippy -p gpui-box --features retained-text,retained-wgpu --lib --no-deps --locked --offline -- -D warnings
cargo +1.98.1 test --manifest-path vendor/taffy/Cargo.toml --lib --locked --offline -j 1 --quiet
cargo +1.98.1 test --manifest-path vendor/taffy/Cargo.toml --lib --locked --offline -j 1 --no-default-features --features std,taffy_tree,flexbox,grid,block_layout,float_layout,content_size --quiet
cargo +1.98.1 check --manifest-path vendor/taffy/Cargo.toml --locked --offline --no-default-features --quiet
cargo +1.98.1 check --manifest-path vendor/taffy/Cargo.toml --locked --offline --no-default-features --features alloc,taffy_tree,flexbox --quiet
cargo +1.98.1 check --manifest-path vendor/taffy/Cargo.toml --locked --offline --all-features --quiet
git diff --check
```

## Final results

| Check | Result |
| --- | --- |
| gpui-box retained | 366 passed |
| gpui-box retained-text + retained-wgpu | 399 passed, 17 ignored |
| Explicit owned-overflow suite, including actual GPU | 3 passed |
| hello retained-wgpu | 32 passed, 2 ignored |
| Taffy default / project-feature library suites | 216 / 210 passed |
| Taffy no-default / alloc+taffy_tree+flexbox / all-features checks | passed |
| Framework library Clippy, warnings denied | passed |
| git diff --check | passed |

Final suites run sequentially, with one Cargo job for the vendored library
tests. Focused tests overlap regular suites; ignored hardware tests in ordinary
suites are not counted as passed. Existing retained-only SpatialUpdate dead-code
and Windows dependency linker messages remain; no all-targets Clippy is claimed.

## Evidence

Kernel matrix:2 directions x4 host overflow x4 child overflow x3 child
containment x2 content sizes x2 owned protocols =384 controls. Asymmetric
padding3/7/5/11, border2/4/1/2 and scrollbar6 distinguish padding from gutter.
An ordinary Flex reference uses identical child geometry; ConstraintBox loose
bounds intentionally cap assigned child size, so its reference uses that size.
Actual child location/size, final overflow and both maximum scroll offsets match.

Additional explicit numeric checks cover empty hosts, hidden/restored/removed
children, repeat final cache hits, negative custom placement wholly before the
scroll origin and partial overlap. Desired host size never creates content.
The prior baseline control now requires full LayoutResult equality: phantom
right78 becomes the actual right52, matching ordinary Flex.
Finite signed parent baseline cases use a fixed20px host,60px child first3/last55,
offset0/-30 and normal/full containment; explicit query/final metrics and CPU
pixels match ordinary Flex, including negative first and beyond-height last.
Real positioned OOF descendants still extend the final overflow through the
shared OOF pass, matching ordinary Flex in LTR and RTL after the in-flow reset.
The OOF control uses logical-start inset130 and top120 with30x20 size; the final
logical right160/bottom140 is asserted numerically. Its first physical-left-only
RTL fixture was wholly before the logical scroll origin and therefore correctly
excluded; that failed fixture assertion was corrected to test reachable content.

Framework resolved/local flow: authored scroll1000/1000, clipped60x40 host with4px
padding; actual child100x80 ->20x15 ->100x80 ->hidden ->visible. Final overflow,
clamped child origin, layout-cache-retained nested child, hits at five points
and complete CPU pixels match a forced-full ordinary Flex tree each frame.
Old packets remain valid after UI owners drop.

Real bundled Noto Sans CJK SC long ->short ->long at60px content width in a70x50
scrolling host with5px padding: actual wrapped height determines overflow and
effective scroll; host full LayoutResult and every CPU glyph pixel match ordinary
full Flex. Old glyph packet survives UI/font/raster/paragraph owner release.

Explicit GPU test submits child100x80 ->20x15 ->100x80 through the local path and
compares all readback pixels against ordinary full Flex. Numeric overflow/hit/
reclamp tests remain separate from GPU output evidence. Actual adapter:
NVIDIA GeForce GTX1650Ti, Vulkan, driver571.96; three submits/readbacks pass.

## Limits

No wall-time, memory or p99 benefit claim. Finite internal reference controls do
not establish new browser conformance, arbitrary custom/Flex equivalence, modern
GPU heavy-effect qualification, CPU continuous complete frames, sustained60/120Hz
or visible input tails. Focused and full suites overlap; ignored hardware tests
are counted only when explicitly run. Only library Clippy is claimed.
