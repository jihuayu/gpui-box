# Atomic responsive evaluation: validation evidence

Date: 2026-11-16. Branch: `codex/rendering-performance-research`.
Baseline: `37ad10a3fb89fd9823c7e73c008b9937bab5f6be`.
Windows / Rust 1.98.1. Contract and decisions:
[atomic responsive evaluation](../../RETAINED_ATOMIC_RESPONSIVE.md).

## Commands and results

```powershell
cargo +1.98.1 test --locked --offline -p gpui-box --features retained --lib --quiet
cargo +1.98.1 test --locked --offline -p gpui-box --features retained-text,retained-wgpu --lib --quiet
cargo +1.98.1 test --locked --offline -p gpui-box --features retained-text,retained-wgpu --lib hardware_local_responsive -- --ignored --nocapture
cargo +1.98.1 clippy --locked --offline -p gpui-box --features retained-text,retained-wgpu --lib -- -D warnings
cargo +1.98.1 check --locked --offline -p gpui-box --features retained-text,retained-diagnostics --lib --quiet
cargo +1.98.1 test --locked --offline -p gpui-box-example-hello --features retained-wgpu --quiet
```

Results: retained **220 passed**; text/WGPU **253 passed, 1 ignored**;
the explicit ignored WGPU test **1 passed**; hello **13 passed, 1 ignored**;
Clippy `-D warnings` and the diagnostics feature check passed. The explicit WGPU
test reported **NVIDIA GeForce GTX 1650 Ti**, discrete GPU, Vulkan, NVIDIA driver
571.96. This is readback evidence on that device, not visible presentation,
timing performance or broad modern desktop GPU qualification.

## Scope of evidence

Ten added normal tests live in
`crates/gpui-box/src/retained/responsive/layout_queries/atomic_tests.rs`.
They cover later computed failures for resolved/local paths; one final source
transaction/effect observation; independent environment/layout/paint widths;
zero-layout sparse placement and idle output; read-only sources and exact
writer ownership; late-installed computed layout sinks; feedback rejection;
invalid opacity and final revision overflow; full/local CPU raster equality,
geometry/hits and old frames; derived material restoration across stages; and
a viewport-computed branch invalidating an established host proof.

The WGPU regression uses responsive environment widths 100→200 with actual
layout width 100 and Unbounded height. Local responsive output changes a red
rectangle from 10×10 to 20×20. It reads the GPU image and compares every byte
against a fresh full-render reference, explicitly checks red at (15,15), then
checks sparse translation equivalence and that idle output submits no frame.
This supplements CPU pixel evidence; it does not qualify newer desktop GPUs,
effects under load or an on-screen compositor.

An existing sibling-allocation test now expects three layout entries instead
of two: private dependency previews plus final layout. This overhead and the
O(tree) candidate copy are documented; no benchmark or speedup claim is made.
Vendor algorithms and public Flex sizing semantics are unchanged this stage,
so previous browser/Taffy reference counts are historical evidence, not new
runs. Sustained 60/120Hz and input-to-visible tail latency remain unqualified.

The hello test runs against this worktree, including pre-existing
`examples/hello/src/retained_demo.rs` WIP excluded from the commit. The existing
retained-only `SpatialUpdate` unused-field warning and Windows native linker
informational warnings are not failures.
