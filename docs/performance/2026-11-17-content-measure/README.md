# Owned content measurement — 2026-11-17

Windows x64 / Rust1.98.1, locked/offline in the existing research worktree.

```powershell
cargo +1.98.1 test -p gpui-box --features retained --locked --offline --quiet
cargo +1.98.1 test -p gpui-box --features retained-text,retained-wgpu --locked --offline --quiet
cargo +1.98.1 test -p gpui-box --features retained-text,retained-wgpu --locked --offline measure::content_tests -- --include-ignored --nocapture
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
| Framework retained suite | 376 passed |
| Framework retained-text + retained-wgpu suite | 409 passed, 18 ignored |
| Explicit content-measure suite, including the ignored GPU case | 11 passed |
| Hello retained-wgpu suite | 33 passed, 2 ignored |
| Taffy default library suite | 217 passed |
| Taffy project-feature library suite | 211 passed |
| Taffy no-default / alloc+taffy_tree+flexbox / all-features checks | Passed |
| Framework library Clippy with `-D warnings` | Passed |
| `git diff --check` | Passed |

The retained, hello and library Clippy checks were repeated after the final
nonfinite-opportunity guard. The text/WGPU and explicit content suites include
that guard; the kernel checks cover the final unchanged vendor sources.

## Evidence

Client wrapping cells use natural10x8 content,12 cells, baseline6. Selected
border widths36/76/116/36, padding2 and border1 give true content widths30/70/110/30.
Normal resolved/local output compares complete host/footer LayoutResult, hit
and all CPU pixels with an ordinary full Flex tree of independently authored
fixed children. Old packets remain valid after UI owners drop.
An additional16 controls place the generic leaf under ordinary Row/RowReverse/
Column/ColumnReverse parents, LTR/RTL and two extents with a fixed rail and real
grow/shrink/basis allocation; complete host/rail results and CPU pixels compare.

Explicit intrinsic MinContent/MaxContent/Unbounded query answers are16x102 /
126x14 /126x14. A loose46px hard max selects40px content width and returns46x30.
Size and complete-baseline query answers agree without changing live kernel
geometry/cache/dirty state or published frame. Diagnostic callback logs are
shared by snapshots and are deliberately excluded from the state equality
claim; logging/memo warming is not rollback.

Mode-sensitive client outputs10/20/30px for MinContent/MaxContent/Unbounded block
space at the same20px width. Repeated normal frames must preserve these distinct
answers; fixed20x5 used box keeps40px natural content overflow and last baseline43.
Pure content baselines may be signed and outside the used host box.

30 finite style controls cover2 directions x3 containment modes x5 preferred/
intrinsic limits, percentage insets, bounded heights and sizing modes, compared
against actual ordinary Flex. Typed measure sources, restored writes, nested
custom parents, hidden/restoration, direct removal and old-packet ownership use
the normal update paths. Five invalid/error outputs in resolved/local atomic
edits must preserve source, geometry and old frame; repeat failed normal frames
and all three explicit query types must not poison live caches.

An additional callback succeeds during initial size selection and fails only
at selected20px width under a custom parent. Its sticky InvalidMeasure takes
precedence over secondary parent errors, preserving live kernel state, published
geometry and frame. Raw scrollbar6 tests in both directions assert60x20 used
box,48x8 deflated content opportunity, overflow right44/bottom28 and baseline9/25.

Bundled Noto Sans CJK SC at60/140/90/60 content widths, including short content
replacement, supplies true owned paragraph metrics through ContentMeasure. Full
leaf/footer LayoutResult, explicit query, hit and all colored-box CPU pixels
match the existing built-in Paragraph variant. This is real-font metric evidence,
not a new glyph-paint or rich-text implementation.

Actual NVIDIA GeForce GTX1650Ti / Vulkan / driver571.96: four local submissions
at border widths36/76/116/36 compare all readback pixels with ordinary full Flex.
This is offscreen output correctness, not native visible presentation evidence.

## Limits

No timing/memory/p99 improvement claim. Internal same-kernel controls do not
establish broad browser conformance, arbitrary content algorithm correctness,
modern GPU heavy effects, CPU continuous complete frames, sustained60/120Hz or
visible input tails. Focused and full suites overlap; ordinary ignored hardware
tests count as passed only when explicitly run. Only library Clippy is claimed.
