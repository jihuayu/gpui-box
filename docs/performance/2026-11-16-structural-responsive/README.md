# Structural responsive validation — 2026-11-16

Windows x64 / Rust 1.98.1 / locked offline. This stage adds explicit responsive
evaluation inside atomic keyed edits. The vendored Taffy source is unchanged;
previous vendor suite counts are not new results from this stage. No timing
benchmark, sustained frame rate or visible presentation measurement was run.

```powershell
cargo +1.98.1 test --locked --offline -p gpui-box --features retained --lib --quiet
cargo +1.98.1 test --locked --offline -p gpui-box --features retained-text,retained-wgpu --lib --quiet
cargo +1.98.1 test --locked --offline -p gpui-box-example-hello --features retained-wgpu --quiet
cargo +1.98.1 clippy --locked --offline -p gpui-box --features retained-text,retained-wgpu --lib -- -D warnings
cargo +1.98.1 test --locked --offline -p gpui-box --features retained-text,retained-wgpu --lib structural::tests -- --include-ignored --nocapture
```

Results: retained **290 passed**; text/WGPU **323 passed, 5 ignored**;
hello **20 passed, 1 ignored**; Clippy `-D warnings` passed. The explicit focused
run including hardware passed **10 tests** (nine normal + one hardware case).
Default ignored cases are not counted as executed by the ordinary library run.

Nine new regular framework regressions cover:

1. Viewport host width 70/130, ContainerQuery child padding 0/5, and keyed inline
   content height 20/40, in both resolved and local atomic outputs. Environment
   widths 100/100/200/200/100 differ from actual layout width 180. Child slots
   are 70/120, following footer positions 20/50. Each edit is one transaction,
   effects observe only three final transitions, and old frames outlive owners.
   Complete CPU pixels match independently authored forced-full ordinary trees.
2. A later host-width change rejects the prior observation atomically; app and
   rule sources, statistics, logical scopes and old local snapshot remain;
   escaped provisional output stays invalid.
3. Two levels of draft-created computations, a new color binding and new effect
   settle before publication. The effect sees green only. The next ordinary
   transaction follows the newly published reverse edges and observes red once.
4. A computed error in a later container stage can be caught without leaking an
   earlier viewport source change; an unrelated translation can still publish.
5. A source changed in the initial transaction and restored by responsive
   selection causes no source/effect/paint publication. The next translation
   keeps local sparse output.
6. Replacing the exact writer, removing a registered target or invalidating a
   query proof at equal numeric width rejects before publication.
7. A newly created computed sibling layout sink after selection introduces fluid
   Row allocation feedback; final dependency validation rejects it.
8. Nested rules registered dependent-first execute in dependency order after an
   earlier staged reparent; the builder observes the final 70-wide child slot.
9. Calling without an atomic frame environment, or with invalid environment
   values, preserves sources and statistics.

The new hello test uses bundled Noto Sans CJK SC with real shaping and glyph
rasterization. Viewport rules select an outer width of 60/150, ContainerQuery
selects padding 0/5, and the builder observes 60/140. Repeated narrow/wide keyed
construction produces real paragraph-driven height and footer placement;
complete CPU pixels equal an independent forced-full paragraph tree, with
explicit cyan glyph coverage. Old output keeps its pixels after dropping
UI/font/raster owners.

Explicit hardware: **NVIDIA GeForce GTX 1650 Ti / Vulkan / driver 571.96**.
Viewport → ContainerQuery padding → inline keyed branches run across environment
100/200/100/200. All actual GPU pixels equal a fresh ordinary forced-full tree.
The test separately checks red content coverage, hit testing and the green
footer at y=20/50. It uses actual local submissions and completes the renderer.
This verifies this adapter's output, not modern desktop GPU heavy effects,
display-visible presentation or input-to-visible tail latency.

Costs remain explicit: full atomic candidates and per-stage previews; prepared
source-map savepoint copies; source scans/sorts for live and draft computations;
binding/ancestor/DAG validation; cloned rules and final observation checks. No
new speed, allocation reduction, p99 or CPU smooth-delivery claim follows from
passing correctness tests. Existing retained-only SpatialUpdate unused fields
and Windows macro linker informational warnings remain.

See [contract and ADR](../../RETAINED_STRUCTURAL_RESPONSIVE.md) and the
[full progress table](../../RETAINED_IMPLEMENTATION_PROGRESS.md) for current
scope, remaining API work and performance acceptance.
