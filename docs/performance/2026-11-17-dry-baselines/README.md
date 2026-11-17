# Explicit dry baseline validation — 2026-11-17

Windows x64 / Rust1.98.1, locked/offline, existing research worktree.

```powershell
cargo +1.98.1 test -p gpui-box --features retained --locked --offline --quiet
cargo +1.98.1 test -p gpui-box --features retained-text,retained-wgpu --locked --offline --quiet
cargo +1.98.1 test -p gpui-box --features retained-text,retained-wgpu --locked --offline dry_baseline -- --include-ignored --nocapture
cargo +1.98.1 test -p gpui-box-example-hello --features retained-wgpu --locked --offline --quiet
cargo +1.98.1 test --manifest-path vendor/taffy/Cargo.toml --lib --locked --offline -j 1 --quiet
cargo +1.98.1 test --manifest-path vendor/taffy/Cargo.toml --lib --locked --offline -j 1 --no-default-features --features std,taffy_tree,flexbox,grid,block_layout,float_layout,content_size --quiet
cargo +1.98.1 check --manifest-path vendor/taffy/Cargo.toml --locked --offline --no-default-features --quiet
cargo +1.98.1 check --manifest-path vendor/taffy/Cargo.toml --locked --offline --no-default-features --features alloc,taffy_tree,flexbox --quiet
cargo +1.98.1 check --manifest-path vendor/taffy/Cargo.toml --locked --offline --all-features --quiet
cargo +1.98.1 clippy -p gpui-box --features retained-text,retained-wgpu --lib --no-deps --locked --offline -- -D warnings
git diff --check
```

## Correctness evidence

| Check | Result |
| --- | --- |
| gpui-box retained | 364 passed |
| gpui-box retained-text + retained-wgpu | 397 passed, 16 ignored |
| Explicit baseline suite, including actual WGPU | 6 passed |
| hello retained-wgpu | 31 passed, 2 ignored |
| Taffy default / project feature library suites | 212 / 206 passed |
| Taffy no-default, alloc/Flex and all-features checks | Passed |
| Framework library Clippy, warnings denied | Passed |
| git diff --check | Passed |

Kernel controls compare explicit baseline queries with normal same-input full
layout for72 combinations: LTR/RTL x four Flex directions x three wrapping modes
x Start/first/last alignment. Complete tree Debug state is identical before and
after repeated queries, including warm size/final cache state, hidden entries
and published geometry. A cold fixed50x30 host with2px inset and an actual leaf
baseline3/7 returns5/9 without authorizing a final cache hit; the subsequent real
layout still writes the leaf at y2.
An absolute200x200 sibling is excluded from in-flow metrics and its published
geometry/hoisted state remains intact during the query.

Protocol tests preserve custom child-slot and contained final observations,
cache inputs and geometry; failing custom full plans propagate. Flex,
ConstraintBox and compiled Block/FlowRoot/Grid query results equal their supported
final algorithm's baseline sets. This does not add Grid last-baseline support.

Framework Mirror client calls explicit child baseline queries from both
ParentMeasure and ParentLayout. An ordinary nested fixed50x30 row with3px padding
and baseline3/7 inside an80px host with2px padding yields host8/12. Independent
ordinary full Flex matches all CPU pixels. Enforced inline30/full40 assignments,
typed source nested failure rollback, containment and invalid queries are checked.
Old frames remain independently consumable after owner drop.
An outer ordinary Flex aligns the custom host using both first and last groups;
baseline3/5/3 updates match independent ordinary full location, size, first/last
baseline and CPU pixels. The pre-existing custom-host overflow accumulation
differs from ordinary Flex (the fixture reports right78 versus52), so complete
LayoutResult equality is not claimed; this query does not return overflow.
Follow-up: [actual owned overflow](../2026-11-17-owned-container-overflow/README.md)
fixes the temporary host-sizing extent; that fixture now checks full LayoutResult
equality. The preceding difference is historical evidence for this stage.

Bundled Noto Sans CJK SC text at60/140/90/60 content widths passes through a real
ordinary nested Flex and the custom parent. Pure metrics/final host/footer/hit,
first/last baseline and full CPU glyph pixels equal an ordinary full Flex tree.
An explicit post-publication baseline query leaves geometry/frame unchanged.

Actual NVIDIA GeForce GTX1650Ti / Vulkan / driver571.96: three local submits with
baseline3/5/3 updates match independent ordinary full Flex GPU readback. Baseline
changes do not alter these rectangle pixels, so numeric baseline checks are
separate; this GPU evidence only covers output/lifecycle of the combined path.

## Claim boundaries

No timing benchmark or improvement claim. Queries intentionally bypass caches
and may execute many final descendant plans, while suppressing publication.
Raw mutable NodeContext callbacks can still mutate their own context. Tests are
finite internal correctness controls, not new browser conformance or modern GPU
heavy effects, CPU continuous delivery, sustained60/120Hz or display input tails.
Focused suites overlap full suites. Ignored hardware tests are not implicitly
counted as passed. Only library Clippy is claimed, not all-targets Clippy.
An initial concurrent kernel test compile failed with LLVM out-of-memory before
running its tests; the recorded kernel suites were subsequently run sequentially
with one Cargo job. This is a local build-resource limitation, not a test pass.
