# Immutable UI records and incremental style planning,2026-11-18

Worktree:`D:\oss\ggpui-worktrees\rendering-performance-research`.
Branch:`codex/rendering-performance-research`.
Baseline:`f3f1587a7259efd2111d81ea1500ddc9a8242401`.
See [contract, architecture and ADR](../../RETAINED_INCREMENTAL_STYLE_PLANNING.md).
Full RFC remains active. These CPU input-adapter timings do not qualify native
presentation, GPU/CPU raster smoothness or full input-to-presentation p99.

## What changes

UiDocument shares immutable authored/effective node records, caches selected
StylePlan rules and maintains inverse users for five environment inputs. State
changes evaluate only selected-rule readers; overwritten rule dependencies do
not leak through. Definition work counts cache once; shared immutable identities
short-circuit value comparison, while distinct equal definitions retain P0.
Dense effective changes merge ordered records; sparse changes use normal map
replacement. Structural batches still validate the complete source graph;
attribute-only batches reuse accepted topology and validate touched payloads.
Selected style output validates without a second identical effective graph walk.
Resource, ordinary retained candidate and complete geometry validation remain.

Public additions: reusable StylePlan, StyleDependencies and UiPublication.planning
counts. There is no new input schema, layout mode, renderer policy or dirty flag.
No vendor kernel code/features change and no new browser capture/vendor suite is
claimed. Normal Flex/Grid geometry and existing reference fixtures still run.

## Workload and timing scope

Windows11 build26100, Ryzen7 4800H, actual Rust/Cargo1.98.1 MSVC, release
opt-level3, retained-only source library. The standalone build intentionally
avoids the optional GPUI/WGPU dependency closure; both variants use identical
features, compiler, public harness and current unchanged project-owned Taffy.
Hashes are recorded in [source.json](source.json) and capture metadata.

Each tree has2,177nodes: Column root,128groups of16leaves. Groups are Rows or
Grids with16fixed12px columns. Leaves are12×8px with six authored declarations
expanding to nine atoms. All2,048leaves use styles;128read viewport width,
1,920read hover. Each operation has a fresh owner and12warmups followed by100
individual timed calls per process. Four alternating pairs run baseline/candidate,
candidate/baseline, baseline/candidate, candidate/baseline. Builds/tests finish
before capture. No CPU pinning, fixed-frequency or thermal control is imposed.

Operations are single hover, all2,048state writes, width change with128subscribers,
device-scale metadata only and one equal StyleRef-list replacement. Message
construction is outside the timed span; complete UiDocument.apply, source/style
planning, required validation, candidate layout/frame publication, previous owner
record destruction and returned publication drop are inside. No frame.resolve,
CPU raster, GPU work, actual host events, native present or initial snapshot
construction is timed. Parent geometry stays fixed; width conditions change
fill, not layout. These are input-adapter costs under ordinary Flex/Grid trees,
not a live resize-layout or interactive end-to-end benchmark.

## Initial candidate and corrections

The initial correct incremental candidate improved eight sparse/no-op process
median aggregates31.1–39.5%, but dense-hover aggregates regressed3.1%Flex and9.9%Grid.
Grid dense p99 aggregate also regressed. Preserve [all initial samples](capture/summary.json),
[exact production patch](initial-candidate.patch) and [source hashes](initial-source.json).
It is not reported as the final runtime result.

The first follow-up caches immutable definition work counts and shared equality,
skips shared effective records during property comparison and uses ordered map
merge for dense effective edits. These general reductions apply to ordinary UI
input, without a workload-specific mode or changing the dependency contract.
Its [four-pair capture](final-capture/summary.json) still regresses dense medians
7.3%Flex/8.3%Grid and Flex dense p99. The directory was named final-capture before
that failed performance result; it is **not the final retained implementation**.
[Exact production patch](merge-candidate.patch) and [hashes](merge-source.json)
are preserved. The kept follow-up additionally reuses the accepted source
topology proof on attribute-only batches and removes the redundant effective
graph walk. Structural batches still validate the complete source graph;
changed payloads/content-leaf rules, normal retained validation and complete
geometry validation remain. Its measurements are in topology-capture.

Early benchmark setup reused a same-name package output in a shared target
directory, producing identical executable hashes. It was rejected before any
timed A/B capture. All recorded captures use distinct independently built
directories and explicit paired1.98.1 compiler/Cargo. The copy/verification
mistake and earlier stable-compiler diagnostic build are excluded from data.
Fresh reproduction uses [build.py](build.py), which creates independent source
and output directories and checks distinct executable hashes.

## Kept implementation: four alternating release pairs

All values below are milliseconds. p50 and p99 are the medians of four process
percentiles, not pooled percentiles. Every one of the8,000individual observations
is retained in [topology-capture](topology-capture/summary.json), along with binary
hashes and per-process percentiles. Both variants used the same unchanged harness.

| Tree / operation | Old p50 | New p50 | Reduction | Old p99 | New p99 |
| --- | ---: | ---: | ---: | ---: | ---: |
| Flex single hover |11.107|4.235|61.9%|14.662|5.593|
| Flex dense hover |14.482|13.945|3.7%|18.918|17.583|
| Flex128environment users |12.951|4.549|64.9%|16.279|6.158|
| Flex metadata only |12.413|3.519|71.6%|15.512|4.860|
| Flex equal style list |11.325|3.820|66.3%|14.022|5.152|
| Grid single hover |11.405|4.154|63.6%|14.314|5.580|
| Grid dense hover |15.436|13.643|11.6%|22.458|16.761|
| Grid128environment users |11.999|4.854|59.5%|14.707|6.295|
| Grid metadata only |11.761|4.073|65.4%|14.389|5.652|
| Grid equal style list |12.429|3.834|69.1%|14.601|5.074|

Sparse/no-op p50 and p99 improve in all four pairs for all eight rows. Each dense
row improves p50 in3/4pairs; Flex dense p99 improves2/4, Grid4/4. In the losing
Flex dense rounds p99 is14.889→15.560ms and15.921→17.800ms; the raw tails remain.
Dense performance is therefore not uniformly better, and the~13.6–13.9ms dense
owner median still exceeds an8.333ms120Hz whole-frame budget before raster/GPU/
presentation. This is no120Hz qualification or universal no-stutter result.

Source hashes for the kept code are in source.json; earlier candidates have
separate patches/hashes. The table describes the complete accumulated change,
not an isolated speedup attributable solely to topology validation removal.

## Correctness and actual checks

Seven new regular framework cases cover owned compiled winners and dependencies,
untouched record sharing, all five state flags, each environment input and unknown
capabilities, final restored values, hidden base changes, overlay removal,
metadata-only/no-op host adoption, late failure rollback, source/index retirement,
slot generation reuse, recovery/replay and ordered sparse/dense map merge.
Differential controls rebuild complete snapshots and compare every effective
record, complete LayoutResult and every CPU pixel. Existing direct ordinary full
tree tests independently author selected numeric styles, including Flex/Grid/Calc
and real Noto paragraph width/baselines/opacity.

| Final check | Actual result |
| --- | --- |
| retained complete suite |517passed,1ignored manual benchmark |
| text+WGPU complete suite |550passed,40ignored |
| hello complete suite |55passed,9ignored |
| explicit style-condition GPU |1passed,4complete-pixel submissions |
| explicit actual Noto style GPU |1passed,5complete-pixel submissions |
| framework library Clippy `-D warnings` |passed |

Hardware:GTX1650Ti/Vulkan/driver571.96. Suite counts overlap and ignored hardware
is evidence only when explicitly run. Production and seven-case final revision
are covered by these complete suites and GPU reruns. Early test module/private
helper access and one borrow conflict were compilation failures, corrected before
successful checks. Existing retained-only unused SpatialUpdate fields and
dependency linker messages remain. No unsafe added or warning policy broadened.

## Reproduction

```powershell
$env:TEMP='D:/oss/ggpui-worktrees/rendering-performance-research/target/style-ir-temp'
$env:TMP=$env:TEMP
cargo +1.98.1 test -p gpui-box --features retained --locked --offline --quiet
cargo +1.98.1 test -p gpui-box --features retained-text,retained-wgpu --locked --offline --quiet
cargo +1.98.1 test -p gpui-box-example-hello --features retained-wgpu --locked --offline --quiet
cargo +1.98.1 test -p gpui-box --features retained-text,retained-wgpu --locked --offline --quiet hardware_style_conditions -- --ignored --nocapture
cargo +1.98.1 test -p gpui-box-example-hello --features retained-wgpu --locked --offline --quiet hardware_style_ir_real -- --ignored --nocapture
cargo +1.98.1 clippy -p gpui-box --features retained-text,retained-wgpu --lib --no-deps --locked --offline -- -D warnings
python docs/performance/2026-11-18-ui-style-planning/build.py
```

Pass the two resulting binary paths to compare.py with --samples100 --rounds4
and an explicit --out directory. Keep builds/tests stopped during measurement.
Every sample is retained. Percentiles use nearest rank; aggregates are medians
of four process percentiles, not pooled percentiles or whole-application tails.

## Remaining costs and acceptance

Structural source validation, map directories, complete candidate cloning, resource/hit
maps and complete geometry checks remain O(N) work. Shared records/plans and
inverse membership retain memory/refcount/indirection costs; heap/peak/RSS, cold
creation, dynamic definition churn and other devices were not measured here.
No automatic structural builder/query scheduler or complete RenderBox is added.
Themes/inheritance/parts, semantic input/focus/handlers, independent native
presentation, modern desktop heavy effects, CPU continuous complete frames,
sustained60/120Hz and full input tails remain active-goal requirements.

The user's99%fluency direction prioritizes common interaction workloads and
their p99/consecutive-missed-frame evidence, without dropping slow samples or
turning finite CPU adapter results into a universal no-stutter promise.
