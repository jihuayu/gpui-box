# Retain identical local output after checked layout, 2026-11-18

Worktree: `D:\oss\ggpui-worktrees\rendering-performance-research`.
Branch: `codex/rendering-performance-research`.
Base: `ced3248359a4a961197222943acdda24a5887b4c`.
The full RFC remains active. This is a general output-reuse improvement, not
unchanged-builder scheduling, a new layout algorithm or native FPS acceptance.

## Decision and boundaries

Final inline-allocation input tracking intentionally marks the host/ancestors
dirty so the ordinary final layout produces authoritative input observations.
Previously even a repeated equal-width edit wrapped its unchanged resolved
FramePacket in a newly allocated LocalFrame, rebuilding topology and defeating
the GPU consumer's exact-snapshot duplicate-submit skip.

The local fallback now runs the same complete publisher first. It retains the
existing LocalFrame only when all four conditions hold:

1. Local and complete output carry the same source revision.
2. Their pixel-snap policies match.
3. The previous local owner-items Weak reference points to the exact published
   immutable PaintItems allocation.
4. Local template properties share the exact complete PropertyTrees allocation.

The selective publisher already keeps these allocations only when its candidate
comparison found no output change. No hash, dirty hint, per-item equivalence walk
or application-provided equality certificate is introduced. Revision alone is
insufficient: actual kernel output can change during internal remeasurement.
Empty deltas relative to an old resolved baseline also cannot certify the latest
sparse local state. If source revisions or storage differ, the existing complete
local construction remains. Full reference extraction can continue rebuilding.

Callbacks, graph/draft previews, layout, final geometry/input/proof/order checks,
source/lifecycle adoption and effects are unchanged. A failed final check still
rejects the complete draft. No persistent registration, skip-callback cache,
public API, schema, vendor or GPU record changes are added.

Tradeoff: a few pointer/revision comparisons on the complete-local fallback;
the unchanged case avoids local snapshot/topology/placement allocation, resolved
recovery-cache setup and downstream submission. CPU software consumers receive
the same complete immutable frame; source-new-but-pixel-equal outputs still
carry a new publication. This does not remove private tree/layout cloning.

## Correctness controls

Three new regular cases check:

- A dirty same-revision layout with unchanged actual output retains exact local
  identity and clears layout work. An input style change with equal final box
  still carries its new source revision. Actual kernel geometry change at the
  same source revision replaces the snapshot and leaves old geometry immutable.
- A prior sparse local translation/clip/opacity/material update over an older
  resolved baseline cannot be replaced by that stale baseline during layout
  revalidation; complete output matches the latest local result. Subsequent idle
  reads retain the newly established baseline.
- Repeated ordinary inline builders still execute every requested callback,
  preserve the exact snapshot after checked final layout and keep tracking
  transaction-local. A later root-width change invalidates the observed input;
  source writes and the published snapshot roll back together.

The existing scheduled nested-Flex GPU fixture exercises widths80/140/140/80.
It compares four complete images against independent forced-full references;
the repeated140px frame now requires only3submissions and1skip. The real Noto
case adds a repeated final180px state: five complete-image comparisons require
four GPU submissions and one skip, while all requested callbacks still execute.
These offscreen controls are distinct from displayed frames and latency data.

## Release control protocol

`build.py` archives the baseline framework source and copies current candidate
source into isolated temporary trees, with separate target directories and
explicit Rust/Cargo1.98.1. Both use the same unchanged project-owned Taffy and
the same public-API harness. No GPUI/GPU dependencies are needed for CPU timing.
Distinct executable hashes are checked before sampling.

The harness constructs ordinary nested Column Flex with64or512fixed leaves,
two reverse-registered InlineAllocation tasks, a240px inline constraint and a
240x600paint viewport. `unchanged_plan` executes the same callbacks/proofs with
no output writes. `leaf_color` alternates one real color after the same plan as
a changed-output control. Counts include root and two hosts:67or515nodes.
There are16warmup operations per fixture, then100samples per operation/process,
four alternating process pairs. Timing includes edit_local_frame, construction
plan/preview/final checks, output creation and release of previous local output.
It excludes initial fixture, resolve, raster, GPU and native presentation.

All samples, p50/p95/p99/max and hashes are retained. Process-percentile medians
are not pooled percentiles or whole-application tails. Run capture after builds
and tests finish; no CPU affinity/frequency/thermal control is imposed.

Actual release capture: Windows11 build26100 / Ryzen7 4800H / MSVC1.98.1,
four alternating pairs with3,200total observations. The unchanged plan does not
show a stable CPU improvement; allocation/consumer savings are small relative
to the common source/graph/tree/layout work and measurement noise.
Values below are milliseconds; p50/p99 are medians of process percentiles.

| Tree / operation | Old p50 | New p50 | Old p99 | New p99 |
| --- | ---: | ---: | ---: | ---: |
| 67 nodes / unchanged plan | 0.6035 | 0.6076 | 1.0060 | 1.1056 |
| 67 nodes / leaf color | 0.5547 | 0.5162 | 0.8612 | 0.7895 |
| 515 nodes / unchanged plan | 4.0065 | 4.0463 | 6.4473 | 6.4450 |
| 515 nodes / leaf color | 3.7128 | 3.7235 | 5.4107 | 5.0528 |

Unchanged p50 regresses0.7%/1.0%; p99 improves in only2/4pairs for both scales.
Leaf-color p50 wins3/4small and2/4large pairs; this is a changed-output control,
not an expected direct beneficiary. No stable owner-CPU or tail gain is claimed.
All observations including candidate unchanged maxima1.4014ms/9.5580ms remain
in [capture](capture/summary.json). [Source identity](source.json) includes paired
toolchain versions, identical harness and candidate hashes;
[the exact production change](candidate.patch) records the measured candidate.

```powershell
python docs/performance/2026-11-18-local-revalidation/build.py
# Use the exact build.json path printed under target/local-revalidation-*.
python docs/performance/2026-11-18-local-revalidation/compare.py <build.json> --out docs/performance/2026-11-18-local-revalidation/capture
```

## Actual regression checks

| Check | Actual result |
| --- | --- |
| retained complete suite | 539 passed, 1 ignored |
| text + WGPU complete suite before extra consumer-only ignored fixture | 572 passed, 42 ignored |
| hello complete suite | 57 passed, 10 ignored |
| repeated-width real Noto CPU follow-up | 1 passed |
| scheduled nested Flex GPU | 1 passed, 3 submits / 1 skip, 4 full images |
| repeated-width real Noto GPU follow-up | 1 passed, 4 submits / 1 skip, 5 full images |
| framework library Clippy `-D warnings` | passed |

Production code was fixed before all these complete runs. Only example test
coverage was extended afterward to include repeated text width; its CPU/GPU
focused reruns pass. The additional ignored consumer cost fixture is described
separately below. Counts overlap; ignored tests are hardware evidence only when
explicitly run. Existing retained-only SpatialUpdate unused-field and dependency
linker messages remain. No vendor/features/schema changed, no fresh vendor or
browser run claimed. Other worktree changes are preserved.

## Consumer cost control

An explicit ignored hardware fixture isolates the avoided consumer work. The
same complete81-node immutable packet at300x200feeds one WGPU renderer. Rebuilt
mode wraps it in a fresh LocalFrame per operation, as the prior fallback did;
retained mode clones the same certified snapshot. Four alternating pairs have
16warmups and100samples per mode, with initial setup outside timing. Timing
includes local reconstruction, submit, finish/completion observation and local
release; owner callbacks/previews/layout and readback are excluded. Each mode
finishes with full reference-pixel comparison. Every rebuilt operation must
submit; every retained operation must skip. This is a same-runtime debug GPU
consumer control, separate from the release owner A/B and native display timing.

Actual GTX1650Ti/Vulkan/driver571.96 debug run passes, with800retained timing
samples and8complete reference-image comparisons. All400rebuilt samples submit;
all400retained samples skip. Values below are milliseconds, medians of the four
within-process round percentiles (not four independent processes or pooled
percentiles):

| Consumer mode | p50 | p95 | p99 |
| --- | ---: | ---: | ---: |
| Rebuild identical LocalFrame, submit, finish | 1.4655 | 1.8120 | 2.0664 |
| Retain exact LocalFrame, skip, finish | 0.00865 | 0.0090 | 0.0108 |

All four rounds improve these percentiles. The maximum individual retained
sample is0.0556ms versus2.3347ms rebuilt. These figures show avoided duplicate
consumer work on this debug configuration; they do not imply a corresponding
whole-application release speedup. Do not add them to release owner percentiles
or treat GPU completion as display completion. Raw [consumer samples](consumer.csv)
and [round summaries/hash](consumer-summary.json) remain available.

```powershell
$env:TEMP='D:/oss/ggpui-worktrees/rendering-performance-research/target/style-ir-temp'
$env:TMP=$env:TEMP
$env:GPUI_LOCAL_REVALIDATION_CSV='D:/oss/ggpui-worktrees/rendering-performance-research/docs/performance/2026-11-18-local-revalidation/consumer.csv'
cargo +1.98.1 test -p gpui-box --features retained-text,retained-wgpu --locked --offline --quiet hardware_identical_local_output_consumer_control -- --ignored --nocapture
```

The benchmark source hashes describe the exact release-build snapshot before
the later ignored consumer-only test was added. The production patch and all
non-test source files were unchanged across those captures and final checks.

## Remaining scope

The full once-only planner still reconstructs its DAG and private source draft
after every callback, and individual builder/final layout previews remain.
Persistent invalidation, reusable producers and unchanged callbacks require a
separate lifecycle contract. Common native input/presentation p99, modern GPU
effects and continuous complete CPU frames remain unqualified. No99%fluency or
sustained60/120Hz guarantee follows from these finite correctness/timing controls.
