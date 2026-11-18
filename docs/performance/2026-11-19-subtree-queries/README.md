# Compact dry measurement: subtree-copy correctness and CPU comparison

Date:2026-11-19. Base:8df4593b538b0890890c5625c743d627aabbd09b.
Worktree:`D:\oss\ggpui-worktrees\rendering-performance-research`.
Branch:`codex/rendering-performance-research`.
[Decision](../../ADR_RETAINED_SUBTREE_QUERIES.md).
Complete RFC goal remains active;99% is figurative, with no statistical gate.

## Scope and controls

Public direct-child size/axis/baseline/batched queries copy a compact union of
queried subtrees, preserving normal solver/measure inputs and live-state isolation.
Complete UI-root queries retain the original whole-clone/warm-cache route.
Final root-layout, measure_node_with_constraints and ordinary full candidate/
responsive preview cloning have not been replaced. Queries themselves still
allocate/copy and run cold scratch caches; this is not a persistent query cache.

Three kernel controls check included context clone counts across1,000 nodes,
overlapping/duplicate roots, shared Style/custom delegate/parent-data identity,
exact-measure/tracked-input capabilities, empty/invalid/dead/reused node IDs,
detached parents, cold scratch records, raw mutable context isolation and invalid
ConstraintBox propagation. One public core control compares warm whole-clone
size/baseline results, overlapping batches/hidden nodes and published geometry;
its local3-node union excludes500 unrelated siblings, and full-root input keeps
the old internal root identity and node count. The existing broad Flex/Grid/
custom parent/constraint/baseline/source rollback suites remain relevant.

Natural160-row Noto list controls use widths100/65/110 at1.25scale, comparing
complete CPU/GPU output with a fresh ordinary full Flex reference. The GPUI
owner host checks actual complete CPU image installation and matching input
scene. These checks establish layout/pixel/host integration, not displayed cadence.

## Measurement method

`subtree-measure-perf` uses the same vendored Taffy solver and contexts on both
paths. A warm normal full layout precedes measurements. Old path clones the whole
tree/cache; compact path starts a cold private subtree union. Each timed region
includes clone, requested direct-child queries and dropping the private tree;
result vectors are checked equal after timing.22 samples alternate execution
order and omit2 warmups. Controls use1,001/10,001/30,001 live nodes, querying
8 middle leaves or the complete root. Raw all-node compact controls remain
reported even though the framework keeps whole cloning for UI-root queries.

These are release CPU query measurements, not full UI/source/producer/raster/
GPU/native presentation measurements. Small direct-child leaf controls are
deliberately simple to expose isolation-copy scaling, not a general application
workload. Complex cold subtree layout may dominate or regress; no universal
speedup or hard execution budget is claimed. The kernel can only bound copied
coverage; user contexts/callbacks may still be expensive.

Environment: AMD Ryzen7 4800H, Windows MSVC, Rust/Cargo1.98.1, locked/offline
standard release. Final independent repeats run the already-built binary after
compiler/link jobs finish. Warmup/build-concurrent exploratory timings are not
used as final timing evidence. Raw repeat summaries accompany this file.

Final [repeat1](perf-r1.json) and [repeat2](perf-r2.json), p50 microseconds:

| Live nodes | Queried union | Whole clone | Compact union |
| --- | --- | --- | --- |
| 1,001 | 8 leaves | 416.3–594.5 | 11.2–15.6 |
| 10,001 | 8 leaves | 4783.9–5178.5 | 35.3–45.8 |
| 30,001 | 8 leaves | 13333.6–14005.7 | 33.1–34.6 |
| 1,001 | entire root | 2636.7–2639.2 | 2808.9–2829.7 |
| 10,001 | entire root | 27923.3–32465.3 | 32339.8–39177.9 |
| 30,001 | entire root | 91676.1–109915.6 | 103953.0–130890.2 |

Whole-root compact rebuilding regresses; the public framework path therefore
keeps the original whole clone for complete UI-root queries. The benchmark
reports both primitives explicitly rather than hiding that control. Variability
in repeated large-array allocation and system clocks remains visible. The eight
leaves have numeric pure content, not representative text/custom-policy cost.
Binary SHA256:`890b88950726396fa73683a4d947e9ef8ad46d17e0c37d6a657fa84a2357aecd`.

Final kernel232passed, alloc/no-std check passed. Framework retained-only787passed/
1ignored and text/WGPU822/69, each15compiler doctests. Hello94/38. Explicit measured
Noto GPU1passed/three complete images at1.25scale on GTX1650Ti/Vulkan/571.96;
explicit owned-parent Noto GPU1passed, exercising dry baseline and full image
agreement through the same adapter. Library/harness Clippy-Dwarnings, CPU
desktop/retained-gpu harness checks, scoped format/diff,1,165local links across
302docs and five inherited WIP hashes pass. Ignored is not hardware execution.

## Reproduction

```powershell
cargo +1.98.1 run -p gpui-box-rendering-perf --bin subtree-measure-perf --features retained-runtime --release --locked --offline
cargo +1.98.1 test --manifest-path vendor/taffy/Cargo.toml --no-default-features --features std,taffy_tree,flexbox,grid,block_layout,float_layout,content_size,affine --lib --locked --offline
cargo +1.98.1 test -p gpui-box --features retained --locked --offline
cargo +1.98.1 test -p gpui-box --features retained-text,retained-wgpu --locked --offline
cargo +1.98.1 test -p gpui-box-example-hello --features retained-wgpu --locked --offline
cargo +1.98.1 test -p gpui-box-example-hello --features retained-wgpu --locked --offline hardware_measured_virtual_list_real_noto -- --ignored --nocapture
```

Remaining work: granular measurement invalidation, bounded reusable query cache,
almost-root coverage/cold-cache trade-offs, full candidate/preview copies,
source/producer work and independent native presentation. Logs are in
`target/subtree-query-validation/`.
