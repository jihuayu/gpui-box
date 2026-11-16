# Nested parent constraint protocol evidence

Date: 2026-11-16. Branch: `codex/rendering-performance-research`.
Windows / Rust 1.98.1, locked/offline workspace dependencies. This is API and
correctness work; no timing or sustained-presentation benchmark was run.
See [contract and decisions](../../RETAINED_NESTED_CONSTRAINT_BOX.md).

## Coverage

- Thirteen framework regressions in `layout/constraint_box_tests.rs`: nested
  Tight/Loose policies, finite/unbounded/empty, padding/hard maxima, Flex sibling
  allocation and baseline, invalid append/property/structure/edit rollback,
  one-edit protocol/topology switch, real local/full CPU pixels and hit testing,
  old snapshots, isolated measure, RTL/hidden and nested OOF coordinates,
  preferred intrinsic keywords, logical typed signal updates and fixed queries,
  min-selected inline size before percentage/aspect auto height, explicit Tight
  assignment vs percentage basis, and conservative fluid-query rejection.
- The CPU comparison uses widths 80/40/110/40, a 160x100 target, complete byte
  equality with Full publication, explicit red-inside/transparent-outside
  assertions and leaf hit-test checks. It is not merely two geometry counters.
- One bundled Noto paragraph regression in hello checks actual line heights,
  child/wrapper sizes and baseline at content widths 50/90/140/50.
- Two fork regressions check high-level cache reuse, indexed structural
  rejection before any observer writes, detached-tree isolation/retirement/clear,
  and distinct MinContent/MaxContent/Unbounded measurement.
- Existing core, mixed renderer/resource, responsive, lifecycle and Flex tests
  run with the new algorithm available. Default ignored hardware tests remain
  ignored; no new GPU pixel readback or browser-equivalence claim is made.

## Commands and results

```powershell
cargo +1.98.1 test --locked --offline -p gpui-box --features retained --lib --quiet
# 241 passed

cargo +1.98.1 test --locked --offline -p gpui-box --features retained-text,retained-wgpu --lib --quiet
# 274 passed, 1 ignored

cargo +1.98.1 test --locked --offline -p gpui-box-example-hello --features retained-wgpu --quiet
# 15 passed, 1 ignored

cargo +1.98.1 clippy --locked --offline -p gpui-box --features retained-text,retained-wgpu --lib -- -D warnings
# passed

cargo +1.98.1 test --manifest-path vendor/taffy/Cargo.toml --offline --lib --quiet
# 192 passed

cargo +1.98.1 test --manifest-path vendor/taffy/Cargo.toml --offline --no-default-features --features std,taffy_tree,flexbox,grid,block_layout,float_layout,content_size --lib --quiet
# 186 passed

cargo +1.98.1 check --manifest-path vendor/taffy/Cargo.toml --offline --all-features --lib --quiet
cargo +1.98.1 check --manifest-path vendor/taffy/Cargo.toml --offline --no-default-features --features std,taffy_tree,flexbox,content_size --lib --quiet
cargo +1.98.1 check --manifest-path vendor/taffy/Cargo.toml --offline --no-default-features --lib --quiet
cargo +1.98.1 check --manifest-path vendor/taffy/Cargo.toml --offline --examples --quiet
# passed
```

The default fork full test run used the final runtime algorithm before only the
non-balance Style size expectations were adjusted; its size test was rerun after
that adjustment. The project-feature run verified the corrected 560-byte
String/528-byte Arc-str layout. Earlier
assertion failures identified the two-byte Display payload and the eight-byte
non-balance Style growth; they were implementation-cost checks, not suppressed
functional failures. Balance-enabled Style size remains unchanged.

Existing retained-only unused SpatialUpdate field warnings and Windows linker
informational messages remain. Hello runs used the current worktree, including
the pre-existing `retained_demo.rs` WIP, which is excluded from this change.

## Cost and acceptance limits

The fork adds a sparse box index and O(B x depth) entry validation. A box whose
width is initially unknown can measure its child again after selecting the
inline size. Intrinsic probes can evict cache entries. Display grows by one
byte; the tested non-balance Style grows by eight bytes. No reduced allocation,
O(1) update or lower p99 claim is made.

ConstraintBuilder, pre-layout structural branch planning, custom layout and
full Flutter RenderBox remain open. Existing broader percentage/cyclic/Flex
differences, native device rounding, complex effects, modern desktop GPU
qualification, continuous 60/120Hz/input-to-visible tails and CPU smoothness
are unaffected acceptance items. The complete goal remains active.
