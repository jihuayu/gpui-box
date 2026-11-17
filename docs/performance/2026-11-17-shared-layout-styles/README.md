# Shared layout style evidence

Capture:2026-11-17; documentation finalized2026-11-18 (Asia/Shanghai).
Worktree:`D:\oss\ggpui-worktrees\rendering-performance-research`.
Branch:`codex/rendering-performance-research`.
Baseline source:`b3088e260f35a65717664ed4ee812997b5e5094c` plus the standalone
`baseline-benchmark.rs` in this directory and its `#[cfg(test)]` module declaration.
Shared variant is the implementation accompanying this evidence, with identical
timed operations/scene. See [ownership and ADR](../../RETAINED_SHARED_LAYOUT_STYLES.md).
The complete RFC goal remains active.

## Workload and measurement scope

Windows11 build26100, Ryzen7 4800H (8cores/16threads), Rust/Cargo1.98.1 MSVC,
release opt-level3, `gpui-box --features retained`, locked/offline dependency
closure. No GPU/presentation is involved in these timings. No CPU pinning,
fixed-frequency/thermal control or allocator/RSS profiling was performed.
Final captures ran after the build/test processes ended. Preliminary concurrent
build captures were excluded from these results.

Both scenes have2,177nodes: one Column root,128groups and16fixed8×8leaves per
group. Flex groups are Rows; Grid groups have16fixed12px explicit columns. Grid
styles therefore contain actual owned track vectors. Different nodes are not
interned. Both variants start with the same public owned style construction and
normal transaction/measurement paths.

| Operation | Included work |
| --- | --- |
| clone | Actual identity-preserving framework candidate clone and destruction, including retained slots and the Taffy tree |
| dry_subtree | Isolated `measure_node_with_constraints` of one17-node group; current implementation clones the whole kernel tree before measuring |
| atomic_layout | Actual private validated edit: clone framework candidate, replace one leaf's8/9px width, publish draft properties, isolated root measurement to validate, adopt candidate and destroy previous owner |
| dry_root | Isolated `measure_with_constraints`, alternating500/501px root input; includes kernel clone and layout, not live-owner layout throughput |
| create_layout | Construct the complete tree via public styles and TreeEdit, perform one isolated root measure, and destroy it |

Scene setup makes a disposable root measure; this does **not** warm live owner
layout caches. Every operation has12untimed warmups, then200timed samples per
process. Temporary result/candidate destruction is inside each timed closure.
The test harness black-boxes outputs. There is no frame extraction, input event,
software raster, GPU upload/execute or native present in these timed paths.

Four process pairs run in order A/B,B/A,A/B,B/A. Each process contains all ten
operations; operation order is unchanged, not randomized. The table reports the
median of four per-process p50s and separately the median of four p95s. These
are **not** pooled percentiles, confidence intervals or end-to-end tails. The
original baseline used `_relayout` for the identical isolated root-measure call;
`compare.py` normalizes that inaccurate label to `_dry_root`. Saved harness and
current benchmark body are identical after this label normalization. Formatting
of the saved source is normalized; binary hashes identify the actual programs.

## Actual CPU comparison

Milliseconds; percentages compare medians of per-process p50s.

| Operation | Old p50 | Shared p50 | Reduction | Old p95 | Shared p95 |
| --- | ---: | ---: | ---: | ---: | ---: |
| Flex candidate clone/drop |3.042|1.352|55.5%|3.715|1.707|
| Flex isolated subtree measure |1.715|0.774|54.8%|2.084|1.061|
| Flex atomic edit + layout validation |7.789|5.308|31.9%|8.911|6.491|
| Flex isolated root measure |4.619|3.892|15.7%|6.162|4.708|
| Flex create + measure + drop |15.855|11.407|28.1%|19.713|14.549|
| Grid candidate clone/drop |3.094|1.100|64.5%|3.919|1.557|
| Grid isolated subtree measure |1.763|0.861|51.2%|2.334|1.300|
| Grid atomic edit + layout validation |9.329|6.545|29.8%|10.703|8.229|
| Grid isolated root measure |6.441|5.314|17.5%|7.703|6.769|
| Grid create + measure + drop |15.925|11.706|26.5%|19.427|15.024|

All four per-process p50s improved for every operation. Example ranges: Flex
clone baseline2.996–3.146ms/shared1.324–1.527ms; Grid clone3.072–3.113/
1.067–1.231ms; atomic Flex7.724–7.919/4.987–5.422ms, Grid9.238–9.571/
6.476–6.649ms. Root measurement has a smaller improvement because layout still
does normal container/child work. Creation also improved in this workload despite
the new per-style allocation; this is not proof of every cold/first-frame or
small-tree workload improving. No isolated direct live-owner relayout benchmark
was added, so it remains an explicit measurement gap.

Raw output:[eight process logs](1-baseline.txt), [per-process CSV](samples.csv),
[summary and ranges](summary.json), [machine/binary metadata](metadata.json).
Logs normalize line endings and trailing blank lines only.
Each corresponding `1-` through `4-baseline.txt`/`shared.txt` is preserved. Logs
contain p50/p95/max only; individual observation times are not retained, so no
new p99 or pooled-tail claim can be reconstructed from these files.

## Static size and costs

Same64-bit core retained build, measured by each executable:

| Type | Old | Shared |
| --- | ---: | ---: |
| Style |776bytes|776bytes|
| Framework Node |1,168bytes|400bytes|
| Framework Slot |1,176bytes|408bytes|

These exclude the shared heap Style, Arc control block, allocator overhead and
track vectors. One normal node now shares its style across framework and kernel;
image effective metadata can require a second snapshot. Candidates share old
values until replacement/drop. Do not translate the Node reduction directly into
total heap/peak/RSS savings. Atomic reference counts, heap allocation, indirection,
full slot/cache/topology cloning and changed-style lowering remain costs.

## Correctness and checks

| Check | Actual result |
| --- | --- |
| retained complete suite |489passed,1ignored manual benchmark |
| retained-text + retained-wgpu complete suite |522passed,37ignored including manual benchmark |
| hello retained-wgpu complete suite |52passed,6ignored |
| explicit Grid inline GPU |1passed,4full-pixel submissions |
| explicit real Grid item Noto glyph GPU |1passed,8full-pixel submissions |
| standalone vendor default lib |232passed |
| vendor affine+serde lib |237passed |
| vendor project features + affine lib |228passed |
| vendor alloc+tree+Flex+affine and all-features checks |passed |
| framework library Clippy `-D warnings` |passed |
| scoped rustfmt/diff |passed |

Three regular framework and three vendor regressions were added. They exercise
shared identities/lifetimes, old clone independence, Grid→Flex replacement,
equivalent-style P0, authored/effective image metadata, failure after candidate
layout, context/parent constructors, visibility/containment indexes and equal
engine setter invalidation. Existing full suites include typed/keyed/query/
builder rollback, images, real text and CPU/reference pixels. Explicit hardware:
GTX1650Ti/Vulkan/driver571.96. The final tiny helper/type-alias Clippy follow-up
was covered by repeated complete framework suites and both explicit GPU tests.

No new browser conformance capture is claimed for this storage-only change.
The existing browser reference fixtures are still exercised by regular Rust
tests. Suites overlap; ignored hardware cases only count when explicitly run.
Existing retained-only unused SpatialUpdate fields/dependency linker messages
remain. Clippy claim is framework library scope. Initial test compile referenced
a nonexistent revision method; corrected to the private field before successful
runs. Initial Clippy rejected the new complex tuple and Arc of non-Send Style;
named alias and documented ownership helper resolve them without unsafe trait
assertions or broad warning suppression.

## Reproduction and remaining work

```powershell
cargo +1.98.1 test -p gpui-box --features retained --release --locked --offline --quiet style_snapshot_benchmark -- --ignored --nocapture --test-threads=1
python docs/performance/2026-11-17-shared-layout-styles/compare.py target/style-snapshot-baseline/baseline.exe target/style-snapshot-baseline/shared.exe --samples 200 --rounds 4 --out target/style-snapshot-recheck
```

Keep a baseline executable before rebuilding. To reconstruct baseline in a
separate checkout of the source revision above, copy `baseline-benchmark.rs` to
`crates/gpui-box/src/retained/style_snapshot_tests.rs`, declare its test-only module
in retained.rs, and run the same release command. Binary paths and hashes in
metadata describe this capture; fresh compiler/output paths can change hashes.
No generated executable or build cache is committed.

This removes a measured duplicated-ownership cost across normal framework
layout/query paths without scene-specific algorithms. It does not remove
whole-tree candidates, full topology/cache cloning, dependency/alternative
scans, guarantee allocation-free writes, or make raw Taffy Style transferable. Incremental
planning, compact/typed storage, global budgets, broad conformance, full-axis
feedback protocols and automatic builder/query scheduling remain open.
Native visible presentation, modern desktop heavy-effects120Hz, CPU sustained
complete-frame smoothness and full input-to-presentation tails remain separate
requirements of the active goal.
