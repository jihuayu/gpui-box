# Producer invalidation evidence, 2026-11-18

Worktree: `D:\oss\ggpui-worktrees\rendering-performance-research`.
Branch: `codex/rendering-performance-research`.
Base: `d38d5007c09dc90d641a23c055b6c8c0ce428c86`.
See [contract and ADR](../../RETAINED_PRODUCER_INVALIDATION.md).
Full RFC remains active. This is whole-plan reuse for certified unchanged
output, not arbitrary partial producer filtering or native fluency acceptance.

## Correctness controls

Six new regular cases, alongside the nine existing producer cases, cover:

- Unread Signal writes, computed equal outputs and A-to-B-to-A restored final
  values reuse the entire plan, report0scheduler previews/0considered inputs and
  preserve exact LocalFrame identity. An actually changed computed value selects
  its reader and invokes the full plan.
- Conditional read switching updates the reverse index only after successful
  final geometry validation. Invalid tentative branch output leaves old reads,
  stats, source/frame and the accepted index installed. Former condition reads
  cease to select candidates after successful adoption.
- Initial binding synchronization, changed bound sources, explicit equal
  properties, changed constraints/viewport and explicit invalidation keep the
  complete plan. The binding output stays correct; a following idle edit skips.
- Responsive rules settle before the certificate. Metadata changes that keep
  the selected output equal may skip; crossing a breakpoint updates actual
  host width and invokes the full builder path.
- Surviving cross-scope source dependencies reject scope retirement. Removing
  producer/source together rebuilds an empty index, and a new scope/source
  generation does not inherit retired memberships.
- The convenience local_frame(viewport) entry drives producer resize with only
  a producer session, then reuses the accepted plan at equal size. Internal
  standalone resolver mutable access invalidates acceptance even if the paint
  revision remains equal, preventing later empty transactions from trusting
  producer observations predating that source/render adoption.

The real Noto fixture asserts zero planning previews on equal post-resize source
transactions and compares complete LayoutResult, baseline, wrap height and all
CPU pixels at five states with a direct ordinary forced-full tree. Its explicit
GPU control uses4submits/1skip and five full-image comparisons. Prior ephemeral
nested-Flex GPU control keeps3submits/1skip and four complete images. These are
finite offscreen results, not displayed frames/input latency evidence.

## Release A/B protocol

build.py archives the baseline source and copies the candidate into independent
source/target trees. Both use explicit paired Rust/Cargo1.98.1 MSVC, the same
unmodified project-owned Taffy, retained-only release opt-level3 and an identical
public-API harness. Distinct executable hashes are required.

Ordinary Column Flex root contains8or32producer hosts, each with4fixed12px leaves:
41or161nodes. Each producer reads the same tracked Signal and performs no output
writes. The harness uses240px inline constraint/unbounded height and240x600paint
viewport. Operations are changed unread source, equal tracked source, or changed
tracked source (full-plan control). Each fixture has16warmups, then100samples;
four alternating process pairs retain4,800total raw observations. Timing includes
transaction/source settlement, any full plan/preview/proof/layout/geometry work,
index/registry adoption and transient output release. It excludes fixture setup,
resolve, raster, GPU/native presentation. Builds/tests finish before capture;
no CPU affinity/frequency/thermal controls. Percentiles use nearest rank;
reported aggregate is median of process percentiles, not pooled/application tail.

## Actual release results and regressions

Windows11 build26100 / Ryzen7 4800H / Rust-Cargo1.98.1 MSVC, same harness and
Taffy. Values below are milliseconds, medians of four process percentiles from
[final-capture](final-capture/summary.json). Every observation is retained.

| Producers / operation | Old p50 | New p50 | Old p99 | New p99 |
| --- | ---: | ---: | ---: | ---: |
| 8 / unread source | 0.6351 | 0.0332 | 1.1297 | 0.0591 |
| 8 / equal source | 0.4631 | 0.0324 | 0.6291 | 0.0468 |
| 8 / observed source | 0.4388 | 0.5570 | 0.6837 | 0.8717 |
| 32 / unread source | 4.8068 | 0.0604 | 6.1396 | 0.1238 |
| 32 / equal source | 4.6534 | 0.0544 | 6.1341 | 0.1220 |
| 32 / observed source | 4.6162 | 4.5389 | 5.6270 | 6.1274 |

The four unread/equal rows improve both p50 and p99 in all4pairs, with aggregate
p50 reductions93.0–98.8%. Related8producer work regresses p50/p99 in all4pairs:
aggregate p50+26.9%, p99+27.5%. At32producers related p50 improves3/4pairs, but
p99 improves only2/4and aggregate p99 worsens8.9%. Candidate maxima include
0.3489ms for equal-source32and7.1944ms for related32; these are not removed.
This change does not establish related-update/tail improvement or120Hz acceptance.

The [first capture](capture/summary.json) used unshared source-observation maps
and checked every selected candidate after detecting a mismatch. It showed the
same unread/equal gains, but related p50 regressed29.0% at8and1.5% at32producers.
The final candidate shares immutable observation maps across registration/task
copies and stops eligibility at the first mismatch. First/final each retain
4,800samples (9,600total), distinct patches/hashes and metadata. The smaller
related regression persists and is reported rather than hidden by more trials
or scene-specific thresholds. Full-plan/index/diagnostic/code-generation and
timing noise costs are not isolated by this aggregate A/B.

[initial production patch](initial-candidate.patch) /
[initial source hashes](initial-source.json) and
[final production patch](candidate.patch) / [final source hashes](source.json)
identify the measured candidates. Final source hash snapshots may precede
test-only additions; non-test source and common harness are verified against the
build directories. No capture occurred with build/test processes active.

The initial candidate retains4,800samples in `capture/` and its exact patch/
hashes in initial-candidate.patch/initial-source.json. Its four no-op/unread
rows reduce aggregate p50 by93.0–99.1%, but the observed-source control regresses
28.97% at8producers and1.47% at32. These results were retained, not treated as
universal performance success. Inspection found repeated source-observation map
copies in registry/task clones and continued candidate equality checks after
the first confirmed mismatch. The final candidate shares immutable Rc observation
maps and stops eligibility checking at the first mismatch, retaining typed read
and complete final validation semantics. Final source/patch and a separate
capture are recorded rather than replacing the first samples.

```powershell
python docs/performance/2026-11-18-producer-invalidation/build.py
# Use the printed target/producer-invalidation-*/build.json.
python docs/performance/2026-11-18-producer-invalidation/compare.py <build.json> --out docs/performance/2026-11-18-producer-invalidation/capture
```

## Verification history and scope

The first compile called a private responsive settlement method from a sibling
module; its visibility was narrowed to the logical module and compilation then
passed. An initial shell lint invocation was truncated to a nonexistent cargo
command; the explicit complete Clippy command below was rerun successfully.
No lint suppression or semantic restriction was relaxed.

Five invalidation cases plus nine existing producer cases first passed14focused
tests. Complete retained553/1ignored, text/WGPU586/43 and hello58/11passed; real
Noto GPU5images(4submits/1skip) and ephemeral Flex4images(3submits/1skip) passed.
After sharing observations, those complete suites and GPU controls passed again,
with final framework library Clippy. A sixth new case then covered convenience
local-frame resize and internal resolver invalidation; final15focused tests and
strict framework library Clippy passed. Final retained full regression includes
that case; text/GPU/hello complete suites preceded the two entry-point fixes,
which the final15focused cases and retained full run cover. Counts overlap;
default-ignored cases are evidence only when explicitly run.

| Check | Actual result |
| --- | --- |
| final producer focused cases | 15 passed |
| final retained complete suite | 554 passed, 1 ignored |
| text + WGPU complete suite before final entry-point fixes | 586 passed, 43 ignored |
| hello complete suite | 58 passed, 11 ignored |
| explicit persistent Noto GPU | 1 passed, 4 submits / 1 skip, 5 full images |
| explicit ephemeral Flex GPU | 1 passed, 3 submits / 1 skip, 4 full images |
| final framework library Clippy `-D warnings` | passed |
| scoped format/diff/document links | passed, 259 local links |

GPU: GTX1650Ti / Vulkan / driver571.96, Rust/Cargo1.98.1 Windows MSVC,
locked/offline debug correctness checks. Release timing uses the same host and
explicit paired toolchain but excludes GPU/native display.

No vendor/features/schema changed, no new browser/vendor result claimed. Existing
retained-only unused SpatialUpdate fields and dependency linker messages remain.
Production changes preserve unrelated worktree files. All raw CSV percentiles
and build/source/harness hashes were checked; scoped formatting/diff/local docs
checks apply to the touched files, not every vendor crate.

## Costs and remaining acceptance

Index rebuild adds O(total observed reads) work and per-source membership memory
on changed registration/observation adoption. Shared-source dense updates still
run the entire plan. Sparse scheduling of only selected producers needs a real
output-write/dependency contract; arbitrary LogicalEdit callbacks cannot be
treated as host-local. Private full candidate cloning and final geometry scans
remain even when plan work skips. Time/heap budgets, native independent
presentation, representative interaction p99, CPU continuous complete frames and
modern desktop effects/sustained60/120Hz remain open. Report all regressions and
slow samples; no99%fluency claim follows from these controls.
