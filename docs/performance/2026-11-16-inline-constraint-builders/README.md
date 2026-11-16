# Inline construction and content-height frame evidence

Date: 2026-11-16, Windows / Rust 1.98.1, original research worktree and branch.
This extends the [full-input builder stage](../2026-11-16-constraint-builders/README.md).
The full RFC goal remains active. No timing benchmark was run.

## Regressions

Nine new framework regressions verify:

1. InlineSize host under a fluid Column chooses keyed 20/60-high content at
   widths 80/80/140/140/80. Host/root auto height, following sibling geometry,
   hit testing, effect retention/cleanup and old frames remain consistent.
   CPU pixels equal an independently built forced-full tree at each width.
2. Nested inline boxes transfer actual width minus padding to a wrapping Flex
   row. Child 110x40, inner/outer/root 120x50 publish together.
3. Full-input builders still reject InlineSize-only hosts. Unproven
   Row/wrap/aspect/intrinsic-width ancestor chains reject before callback.
4. A later invalidation of the ancestor proof rejects even when numeric inline
   width happens to match; the old complete frame remains published.
5. A late width change rolls back settled source values and provisional branch
   state/effect publication.
6. A Loose slot max of 120 can produce a 20-wide child; 50-high content changes
   host height without becoming an inline structural dependency.
7. Exact width/min/max 80 with numeric insets supports a content-height host
   under a wrapping Row with parent aspect ratio. The parent may limit available
   block space (50/55/50-high host) while child inline size remains 70.
8. Fixed border-box width cannot terminate the proof when horizontal insets
   depend on an unproven ancestor width, or when a hard parent box can override
   the authored width.
9. Responsive ContainerQuery uses the same proof for a fluid query host inside
   a Tight inline box. Widths 80/140/80 select correct heights; a Loose edge
   rejects registration and subsequent invalid proof rejects new publication.

The tenth framework test is a hardware test, ignored by default and explicitly
run here. NVIDIA GeForce GTX 1650 Ti, Vulkan, driver 571.96. Four alternating
80/140-width branch retirements update content height and footer placement.
Every GPU output pixel equals a fresh independent full-tree reference render;
red content and green footer pixels are separately asserted. This proves the
checked output on this adapter; it is not a newer-GPU/effect/latency result.

One new hello test uses bundled Noto Sans CJK SC with real shaping, glyph
rasterization and CPU rendering. At final widths 50/90/140/50, paragraph size,
baseline, host height and following footer position match real line metrics.
All CPU pixels match a fresh forced-full tree, visible glyph coverage is
asserted, and an old frame still renders its original pixels after destruction
of the UI and font owner. Existing unrelated retained_demo WIP was present for
hello tests and excluded from this commit.

## Commands

```powershell
cargo +1.98.1 test --locked --offline -p gpui-box --features retained --lib --quiet
# 262 passed

cargo +1.98.1 test --locked --offline -p gpui-box --features retained-text,retained-wgpu --lib --quiet
# 295 passed, 2 ignored

cargo +1.98.1 test --locked --offline -p gpui-box --features retained-text,retained-wgpu --lib hardware_inline_branches_match_fresh_full_pixels -- --ignored --nocapture
# 1 passed; adapter printed as GTX 1650 Ti / Vulkan / 571.96

cargo +1.98.1 test --locked --offline -p gpui-box-example-hello --features retained-wgpu --quiet
# 17 passed, 1 ignored

cargo +1.98.1 clippy --locked --offline -p gpui-box --features retained-text,retained-wgpu --lib -- -D warnings
# passed
```

Retained-only builds retain pre-existing unused SpatialUpdate fields; Windows
macro dependencies emit informational linker warnings. Vendor Taffy source is
unchanged in this stage, so previous standalone vendor results are not restated
as newly executed tests. No new browser comparison was run.

## Costs and open work

See [contract and ADR](../../RETAINED_INLINE_CONSTRAINT_BUILDERS.md). Full
candidate and per-builder preview copying remains, plus style clones and
ancestor proof traversal; no persistent proof/dependency index is introduced.
Preview now drops before calling build so nesting does not retain one full
preview per depth. No Style/GPU format or per-layout-node storage change. Final
FramePacket layout counters still exclude previews. No measured speedup,
memory reduction or p99 improvement claim follows.

General full-axis content-sized builders/custom layout, automatic scheduling,
combined ContainerQuery structural plans, atomic local output, incremental
planning, broad Flex/cyclic/percentage/baseline/rounding conformance and complete
native delivery remain open. Newer desktop GPUs, heavy effects, sustained
60/120Hz, visible presentation/input-to-visible tails and CPU smooth delivery
are not qualified by these correctness checks.
