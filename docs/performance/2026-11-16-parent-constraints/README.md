# Parent box constraints: validation evidence

Date: 2026-11-16. Branch: `codex/rendering-performance-research`.
Baseline: `982cd0a52bc6bab188b897815e8ebec0f979a4ec`.
Windows / Rust 1.98.1. See the
[public contract and decision](../../RETAINED_PARENT_CONSTRAINTS.md).

## Commands

```powershell
cargo +1.98.1 test --locked --offline -p gpui-box --features retained --lib --quiet
cargo +1.98.1 test --locked --offline -p gpui-box --features retained-text,retained-wgpu --lib --quiet
cargo +1.98.1 test --locked --offline -p gpui-box-example-hello --features retained-wgpu --quiet
cargo +1.98.1 clippy --locked --offline -p gpui-box --features retained-text,retained-wgpu --lib -- -D warnings
cargo +1.98.1 test --manifest-path vendor/taffy/Cargo.toml --offline --lib --quiet
cargo +1.98.1 test --manifest-path vendor/taffy/Cargo.toml --offline --no-default-features --features std,taffy_tree,flexbox,grid,block_layout,float_layout,content_size --lib --quiet
cargo +1.98.1 check --manifest-path vendor/taffy/Cargo.toml --offline --all-features --lib --quiet
cargo +1.98.1 check --manifest-path vendor/taffy/Cargo.toml --offline --no-default-features --features std,taffy_tree,flexbox,content_size --lib --quiet
cargo +1.98.1 check --manifest-path vendor/taffy/Cargo.toml --offline --examples --quiet
```

Final results: retained **228 passed**; text/WGPU **261 passed, 1 ignored**;
hello **14 passed, 1 ignored**; Taffy default **190 passed**, project features
**184 passed**. Clippy `-D warnings`, all-features library, Flex-only library
and vendored examples checks passed. The framework adds
eight tests, hello adds one real font test, and the fork adds four regressions
(actual display algorithms, OOF, invalid bounds and cache mode separation).

## Evidence scope

`layout/box_constraints_tests.rs` verifies hard precedence over style min/max,
aspect ratio and padding; unchanged authored styles; Row/Column wrapping;
minimum-driven grow and wrapping when soft available width is smaller than
hard minimum; child percentage basis and noninheritance of root bounds;
constraint-only invalidation, idle reuse and old frames; isolated subtree
measurement; invalid inputs and hidden roots; independent responsive environment
selection and actual constrained query widths.

Full and local paths render byte-identical CPU images. Assertions also check
red inside the constrained box, transparent outside, and matching hit testing.
This verifies raster/geometry agreement, not CPU frame-rate qualification.

`retained_paragraph_tests.rs` uses the bundled Noto font and real shaping.
Auto height, preferred MinContent height and intrinsic minimum-height with
conflicting authored min-width use actual parent-constrained widths
50/90/140/50. Expected size comes from the paragraph's independent wrapping
entry; baseline and old-frame ownership are checked.

`compute/bounds_tests.rs` exercises actual Flex/Grid/Block containers and leaves,
BorderBox/ContentBox padding larger than the host bound, authored-style
preservation, cache transitions back to unbounded layout, an OOF root and
percentage child, and invalid bounds with zero layout writes. The cache test
separately checks both final-layout and measure modes with changed min/max.

An early vendor fixture allowed default Flex shrinking, so its child correctly
shrunk; it now explicitly sets shrink=0 for the noninheritance assertion. The
extreme padding case exposed a real Flex shortcut which recomposed width as
zero content + full inset; the shortcut now respects the hard maximum. Negative
content/positioning extents floor to zero. These are corrected before final
validation, not excluded failures.

No browser equivalence claim applies to this independent host interval, which
can intentionally override CSS padding floors. Earlier browser references are
historical. No new GPU readback, timing benchmark, visible presentation,
sustained 60/120Hz or input-to-visible measurement is claimed. Existing default
ignored GPU tests remain ignored. Hello runs against the current worktree;
pre-existing retained_demo WIP is excluded from the commit. Existing native
linker informational and retained-only unused SpatialUpdate warnings remain.
