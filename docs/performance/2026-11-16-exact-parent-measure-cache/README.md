# Exact parent measurement cache validation — 2026-11-16

Windows x64 / Rust 1.98.1 / locked offline. This stage changes the project-owned
Taffy measurement cache, not public layout inputs or renderer formats. No timing
benchmark or sustained frame-rate qualification was run.

```powershell
cargo +1.98.1 test --locked --offline -p gpui-box --features retained --lib --quiet
cargo +1.98.1 test --locked --offline -p gpui-box --features retained-text,retained-wgpu --lib --quiet
cargo +1.98.1 test --locked --offline -p gpui-box-example-hello --features retained-wgpu --quiet
cargo +1.98.1 clippy --locked --offline -p gpui-box --features retained-text,retained-wgpu --lib -- -D warnings
cargo +1.98.1 test --locked --offline -p gpui-box --features retained-text,retained-wgpu --lib hardware_custom_parent -- --ignored --nocapture
cargo +1.98.1 test --locked --offline --manifest-path vendor/taffy/Cargo.toml --lib --quiet
cargo +1.98.1 test --locked --offline --manifest-path vendor/taffy/Cargo.toml --no-default-features --features std,taffy_tree,flexbox,grid,block_layout,float_layout,content_size --lib --quiet
cargo +1.98.1 test --locked --offline --manifest-path vendor/taffy/Cargo.toml --lib exact_measurements_are_bounded -- --nocapture
cargo +1.98.1 check --locked --offline --manifest-path vendor/taffy/Cargo.toml --no-default-features --lib --quiet
cargo +1.98.1 check --locked --offline --manifest-path vendor/taffy/Cargo.toml --no-default-features --features std,taffy_tree,flexbox --lib --quiet
cargo +1.98.1 check --locked --offline --manifest-path vendor/taffy/Cargo.toml --no-default-features --features alloc,taffy_tree,flexbox --lib --quiet
cargo +1.98.1 check --locked --offline --manifest-path vendor/taffy/Cargo.toml --all-features --lib --quiet
```

Results: retained-only **281 passed**; text/WGPU **314 passed, 4 ignored**;
hello **19 passed, 1 ignored**;
Taffy default **201 passed**, project configuration **195 passed**; Clippy
`-D warnings` and all four feature checks passed. One explicit hardware case
passed separately.

Six new kernel regressions cover complete input/result equality, bounded
replacement/eviction, clone isolation, lazy ordinary-node allocation, exact-only
dirty propagation, unchanged sibling reuse, context/resource invalidation,
hidden/cache clear/remove/tree clear/generation reuse, and removing the last
custom delegate without losing subsequent child-to-ancestor invalidation.
One actual custom algorithm requests height percentage bases 20/40/20 repeatedly
and preserves complete baselines: only **two distinct ComputeSize leaf calls**
occur across all identical repeated parent passes, producing heights 10/20/10.

Two new framework cases compare one versus twelve identical measurements of a
real wrap Flex subtree. At widths 80/45/100/80, actual `leaf_layouts` and
`measure_calls` are equal between clients; final geometry and complete CPU pixels
are equal. Intrinsic resource replacement changes height and baseline correctly;
local/resolved output matches and old hit geometry remains valid. A separate
direct measured-leaf test checks baseline 7/16/7 through repeated exact hits.
The existing bundled Noto custom parent test now repeats each actual paragraph
measurement nine times, checks full result equality, and still compares real
shaping/rasterization glyph pixels to an independently authored full Flex tree.

Explicit hardware: **NVIDIA GeForce GTX 1650 Ti / Vulkan / driver 571.96**,
custom parent output through width/height changes matches every pixel of a fresh
ordinary full Flex reference, including explicit red-content coverage. This is
GPU output correctness on this adapter, not modern GPU/effect qualification or
visible presentation latency.

ABI probe: **LayoutInput 72 B; LayoutOutput 88 B; record 160 B; cache header
32 B; capacity 9** on this default Windows x64 build. The initial store reserves
1,440 B for records plus header/Arc/table overhead. Ordinary NodeData/Cache fields
are unchanged. Map clones still cost O(visited cached nodes); shared cache hits
do not copy stores, while writes to shared stores copy that node's record list.
No global byte budget or measured memory/frame-time improvement is claimed.

Clippy first encountered a missing build fingerprint directory while another
Cargo command used the shared target; rerunning it alone passed. Existing
retained-only SpatialUpdate unused fields, Windows macro linker information and
vendor print ignored Result warning remain. No unrelated WIP is included.
