# Incoming allocation and content-sized builder validation — 2026-11-17

Windows x64 / Rust 1.98.1 / locked offline, on the existing research worktree.
This is framework capability/correctness evidence. No timing benchmark, memory
reduction, modern desktop GPU/effect or sustained-frame-rate qualification ran.

```powershell
cargo +1.98.1 test --locked --offline -p gpui-box --features retained --lib --quiet
cargo +1.98.1 test --locked --offline -p gpui-box --features retained-text,retained-wgpu --lib --quiet
cargo +1.98.1 test --locked --offline -p gpui-box --features retained-text,retained-wgpu --lib allocation_tests -- --include-ignored --nocapture
cargo +1.98.1 test --locked --offline -p gpui-box-example-hello --features retained-wgpu --quiet
cargo +1.98.1 clippy --locked --offline -p gpui-box --features retained-text,retained-wgpu --lib -- -D warnings
cargo +1.98.1 test --locked --offline --manifest-path vendor/taffy/Cargo.toml --lib --quiet
cargo +1.98.1 test --locked --offline --manifest-path vendor/taffy/Cargo.toml --no-default-features --features std,taffy_tree,flexbox,grid,block_layout,float_layout,content_size --lib --quiet
cargo +1.98.1 check --locked --offline --manifest-path vendor/taffy/Cargo.toml --no-default-features --lib --quiet
cargo +1.98.1 check --locked --offline --manifest-path vendor/taffy/Cargo.toml --no-default-features --features alloc,taffy_tree,flexbox --lib --quiet
cargo +1.98.1 check --locked --offline --manifest-path vendor/taffy/Cargo.toml --all-features --lib --quiet
```

Final results: retained **321 passed**; text/WGPU **354 passed, 9 ignored**;
focused allocation suite **8 passed**, including its explicitly enabled WGPU
case; hello **24 passed, 2 ignored**; Clippy `-D warnings` passed. Taffy default
**203 passed**, project configuration **197 passed**; all three feature checks
passed. The ordinary suite's ignored GPU cases are not counted as hardware passes.
The pre-existing retained-only SpatialUpdate unused-field warning and Windows
macro linker information remain. Unrelated hello demo/document WIP was preserved.

## Framework and owned kernel

Seven new ordinary framework regressions cover:

1. Loose incoming widths 60/140/60/140 and Unbounded height: allocation_builder
   chooses keyed descendants, actual three-item Flex wraps to heights 30/10/30/10,
   custom root content height and following green footer update. Measured-parent
   requests of tight 999x999 cannot override the allocated intervals. Both
   resolved and LocalFrame complete CPU pixels match a separately authored
   forced-full ordinary Flex tree. Hit geometry and old frames after UI drop hold.
2. Nested incoming builders without containment derive both content width and
   height (30x10 versus 70x20), reading current draft assignments before keyed
   construction. Structural callbacks run once, without convergence retries.
3. Equal-number algorithm replacement, ancestor insertion and sibling layout-order
   change after observation reject. Source value/stats/published revision and
   old pixels remain intact.
4. Ordinary Flex/arbitrary measured-parent edges and preferred intrinsic sizing
   reject before user structural callbacks run.
5. Invalid slot count, NaN minima, invalid max and nonfinite percentage basis reject
   atomically through LocalFrame publication.
6. MinContent/MaxContent/Unbounded soft width remain distinct from hard 25..70 and
   15..40 bounds and separately established 200/300 percentage bases. A 25%/10%
   host uses 50x30, proving the incoming max is not reported as used size.
7. Nested custom parent with constant known width80 and unchanged percentage basis
   receives soft space40/70/40. Its independent child width must change40/70/40;
   pixels match an independent ordinary tree. The old CSS final key could hide
   soft space behind known width; the new complete-input gate resolves it.

Two new vendor regressions cover final-record lifetime and the exact final-input
guard: repeated cached layout, changed slot width with cloned old-tree isolation,
hidden subtree, algorithm removal/reinstall, deletion/generation reuse/tree clear;
ordinary trees allocate no final-input guard, dirty/cache clear/delete/clear retire
guards, and changed soft space with equal known dimensions misses correctly.

The kernel now obtains allocation from the incoming LayoutInput before host
measurement, and enforces it for every measurement and placement across internal
selected-size probes. Only final PerformLayout stores slot observations; raw
Taffy errors may leave internal writes, while framework custom layout candidates
provide publication rollback. This boundary is documented, not hidden.

## Actual fonts and GPU consumers

Bundled **Noto Sans CJK SC** shapes/rasterizes real Latin/Chinese text. Widths
60/140/90/60 flow through incoming full allocation → keyed builder without
containment → real paragraph wrapping → content host/root height → footer.
Every CPU glyph pixel equals fresh ordinary full Flex output; explicit colored
glyph and hit assertions pass. The retained old frame reproduces pixels after
UI/font/raster/text/paragraph owners are dropped.

Explicit GPU: **NVIDIA GeForce GTX 1650 Ti / Vulkan / driver571.96**. Actual
LocalFrame submissions at widths60/140/60/140 match every pixel of independent
fresh forced-full ordinary Flex output. Green footer pixels are checked at the
actual wrap-derived y coordinate. This is this adapter's output correctness;
it is not modern desktop heavy-effects, display-visible presentation or continuous
60/120Hz/input-tail evidence.

## Costs and uncompleted work

See the [protocol and ADR](../../RETAINED_PARENT_ALLOCATION_BUILDERS.md).
Slots allocate/check O(children); final Arc arrays and exact final-input guards
add sparse records, tree clones traverse their maps; snapshot gains another
optional Arc fat pointer. Full-input guards apply to visited final-layout nodes
when any custom algorithm exists. Stricter cache hits can increase work. Builder
observations copy styles/handles and sorted ancestor child lists. Full candidate,
per-builder preview/source savepoint/rollback costs remain; no lower memory or
speedup follows from these tests.

General mixed-Flex/full-axis proof, child-style allocator dependencies, automatic
builder/query/environment scheduling, complete RenderBox, incremental planning,
global cache budgets, broad conformance and full resource/effect/native lifecycle
remain open. CPU continuous complete-frame smoothness, modern GPU effects,
sustained60/120Hz and visible input tails remain separate acceptance gates. The
full research objective stays active.
