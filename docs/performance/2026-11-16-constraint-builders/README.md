# ConstraintBuilder / atomic keyed frame evidence

Date: 2026-11-16. Windows, Rust 1.98.1; existing
`codex/rendering-performance-research` worktree. This is correctness and API
evidence, not a timing benchmark or full RFC acceptance.

## Behavior covered

Framework tests in `retained::logical::constraint_builder::tests` cover:

1. A real Flex-allocated size-contained host chooses narrow/wide keyed
   structures at 100/100/160/160/100. Stable keys retain state/render handles;
   changed keys retire state/output/bindings and clean effects once. Wide adds
   a third blue primitive. Actual complete CPU pixels equal an independently
   built forced-full reference, with explicit green/red/blue/transparent checks.
   A retained old frame still produces its original pixels after replacements.
2. A final extraction with composed finite translations overflowing geometry
   rejects before source, stats, logical lifetime, effect or frame publication.
   Escaped provisional node/scope/source handles cannot become future live IDs.
3. Changing an observed parent allocation later in the same plan returns
   `UnsafeConstraintBuilder`, preserving old geometry/frame.
4. Nested builders observe a newly constructed Flex tree. A fixed 30-wide
   sibling leaves an inner host 130 wide; 5-pixel padding assigns a tight
   120-by-70 child slot. A 900-by-900 preferred child respects it.
5. Settled LayoutStyle sinks apply before input reads. Non-box,
   non-size-contained, hidden hosts and builder calls outside edit_frame reject.
6. Loose max is distinct from the child's used size under finite allocation.
7. Source protocol change + old keyed output retirement + replacement
   construction validate together, including a two-child Flex changing into a
   single-child box without an invalid intermediate publication.
8. Invalid final branch arity rejects without creating scope/effect lifetime.
9. Explicit builder callback runs once per owner plan, including repeated
   input/cache reuse; it never runs for transient intrinsic Flex probes.
10. Selected inline size and indefinite block-axis Unbounded/MinContent/
    MaxContent remain distinct from percentage definiteness in an empty host.
11. A failed replacement preserves old state, binding and effect. The old
    branch can update afterward; a subsequent valid replacement runs old cleanup
    before the new effect and retires the old binding.
12. Reused free slots do not change a scope's render append order in the current
    draft, after publication, or on the next retained keyed build.

The new hello test uses the bundled Noto font and actual platform shaping.
After ConstraintBuilder branch selection, final Flex widths 50/90/140/50
produce the expected paragraph height/baseline and old owned frame geometry.
The unrelated existing `retained_demo.rs` WIP was present for hello checks but
is excluded from this commit.

The new vendor test checks final empty child-slot input, parent cache hits,
hidden ancestor retirement, reappearance, style protocol removal, node removal
and clear. Existing constraint-box and broader retained regressions also pass.

## Commands and results

```powershell
cargo +1.98.1 test --locked --offline -p gpui-box --features retained --lib --quiet
# 253 passed

cargo +1.98.1 test --locked --offline -p gpui-box --features retained-text,retained-wgpu --lib --quiet
# 286 passed, 1 ignored

cargo +1.98.1 test --locked --offline -p gpui-box-example-hello --features retained-wgpu --quiet
# 16 passed, 1 ignored

cargo +1.98.1 clippy --locked --offline -p gpui-box --features retained-text,retained-wgpu --lib -- -D warnings
# passed

cargo +1.98.1 test --manifest-path vendor/taffy/Cargo.toml --offline --lib --quiet
# 193 passed

cargo +1.98.1 test --manifest-path vendor/taffy/Cargo.toml --offline --no-default-features --features std,taffy_tree,flexbox,grid,block_layout,float_layout,content_size --lib --quiet
# 187 passed

cargo +1.98.1 check --manifest-path vendor/taffy/Cargo.toml --offline --all-features --lib --quiet
cargo +1.98.1 check --manifest-path vendor/taffy/Cargo.toml --offline --no-default-features --features std,taffy_tree,flexbox,content_size --lib --quiet
cargo +1.98.1 check --manifest-path vendor/taffy/Cargo.toml --offline --no-default-features --lib --quiet
cargo +1.98.1 check --manifest-path vendor/taffy/Cargo.toml --offline --examples --quiet
# passed
```

Retained-only builds retain the pre-existing unused SpatialUpdate field warning.
Windows dependency macro linking emits informational linker warnings. Ignored
hardware tests were not explicitly executed in this stage.

## Costs and unproven claims

See [contract and ADR](../../RETAINED_CONSTRAINT_BUILDERS.md). Atomic frame edits
make a full private retained/layout candidate. Each builder makes a temporary
preview, scans binding sinks, validates and lays out before final frame work;
peak can include candidate + preview in addition to the published owner. Final
finite geometry checks scan complete paint items. Fork input records add memory
per laid-out constraint box, with map overhead; no new Style size increase.
The existing final FramePacket layout counters exclude builder previews.

No speedup, memory reduction, bounded tail latency, visible presentation,
modern-GPU or CPU delivery smoothness conclusion follows. No new GPU readback,
browser equivalence, sustained 60/120Hz, or timing benchmark was run. General
content-sized builders/custom layout, automatic dependency scheduling, combined
container-query structural planning, atomic local output and incremental
planning remain open. Full RFC goal stays active.
