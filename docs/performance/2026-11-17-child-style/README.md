# Measured parent item styles — 2026-11-17

Windows x64 / Rust1.98.1, locked/offline in the existing research worktree.

```powershell
cargo +1.98.1 test -p gpui-box --features retained --locked --offline --quiet
cargo +1.98.1 test -p gpui-box --features retained-text,retained-wgpu --locked --offline --quiet
cargo +1.98.1 test -p gpui-box --features retained-text,retained-wgpu --locked --offline parent_layout::child_style_tests -- --include-ignored --nocapture
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
| Framework retained suite | 382 passed |
| Framework retained-text + retained-wgpu suite | 415 passed, 19 ignored |
| Explicit child-style suite, including the ignored GPU case | 7 passed |
| Hello retained-wgpu suite | 34 passed, 2 ignored |
| Taffy default / project-feature library suites | 218 / 212 passed |
| Taffy no-default / alloc+taffy_tree+flexbox / all-features checks | Passed |
| Framework library Clippy with `-D warnings` | Passed |
| `git diff --check` | Passed |

## Evidence

Application-owned measured wrap policy reads each direct child's unresolved
margin, basis and box sizing, then delegates actual nested Flex to the common
kernel. LTR/RTL, resolved/local and five widths/style/hidden/reorder steps compare
host, children and footer geometry and all CPU pixels against a separate ordinary
full Flex tree. Child metadata margins are asserted zero for the custom protocol
and normalized only for that comparison. Other geometry tolerates0.0001 logical
units; pixels compare exactly. Warm unchanged frames skip the delegate. Old
packets remain consumable after owners drop.

An early auto-width client did not implement ordinary Flex's shrink-wrap rules;
the output differed and failed the comparison. The finite reference fixture
explicitly authors host width100/65/100/65/100 and does not qualify that auto-size
case. A subsequent failure found real zero-vs-resolved margin metadata semantics;
it remains documented, not changed to hide the distinction. A RTL overflow
comparison differed86.05 vs86.049995; geometric tolerance is explicit.

Field coverage retains all supported dimension keywords, Content basis, all
self alignment keywords and Safe/Unsafe, percentage/auto/negative edges,
direction, relative position, containment, overflow and effective image
classification. Raw scrollbar/paint containment and reparent/query lifetime are
checked. Unsupported raw Sticky styles reject without replacing live geometry,
cache/dirty state or old frame. Typed styles retain restored-value P0 behavior,
atomic source/layout/frame rollback on application policy error and recovery.
Measured style reads cannot override enforced independent slots and cannot
authorize an unsafe feedback builder. The vendor test changes grow1/3/1 and
checks actual30/50/30 allocation plus final cache behavior.

Bundled Noto Sans CJK SC width60/140/90/60 is selected from the real child's basis.
Actual shaped metrics, complete host/leaf/footer results, dry query and all CPU
glyph pixels compare with ordinary full Flex. UI/font/raster/paragraph owners may
drop while the old packet remains valid.

Actual NVIDIA GeForce GTX1650Ti / Vulkan / driver571.96: local RTL width
100/65/100 and a child size update submit/readback exactly match ordinary full
Flex reference pixels. The geometry/margin/tolerance checks are separate CPU
assertions; the GPU comparison does not conceal their limits.

## Limits

No timing/memory/p99 or sustained-presentation claim. Same-kernel controls do not
establish new independent browser conformance or arbitrary client policy
correctness. Explicit hardware pixels are offscreen correctness, not visible
presentation. Full suites and focused suites overlap; ignored cases count as
passed only when explicitly run. Only library Clippy is claimed.
