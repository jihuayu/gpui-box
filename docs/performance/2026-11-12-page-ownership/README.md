# Owned page publication and delta views — 2026-11-12

Windows / NVIDIA GeForce GTX 1650 Ti / Vulkan / driver 571.96. Source base is
`fc0a4fc`. Final full validation and three focused executions each carry **163
selected source hashes**, all verified against final working-tree bytes. The
final executable hash, process audits and test counts are in
[verification.json](verification.json).

The full regression passes **483 hardware scenarios / 472 byte-exact pixel
comparisons**. Each focused execution passes **84 scenarios / 78 pixel
comparisons**, including six mutation/retry groups, before timing. Core tests
pass **124**, retained-only tests **110**, and gallery/font/hardware-click tests
**12**. Selected core/gallery/perf all-target Clippy checks pass `-D warnings`;
the desktop WGPU executable builds. Logs, hardware records and PNGs are preserved.

## What is verified

Core cases compare default owned publication, original selective copying and
forced full extraction for dense movement, mixed layout/property/material edits,
ordered deltas, images/shadows, damage, hot hit indexes and snapshots retained
after tree drop. Whole changed pages move without cloning old payloads; partial
pages retain COW. Palette-only edits reuse their already unique dirty set, and
delta indices are owned by the already shared delta header.

Private indexed deltas borrow actual authoritative PaintItems. Exact root Arc
identity permits skipping duplicate delta payload comparison; authoritative
items/spatial/property/header/removal/order validation remains. Detached equal
roots use checked comparisons. Mutation materializes an independent list, loses
the shortcut and rejects mismatches/duplicates without poisoning valid retry.
The identity shortcut proves delta-to-snapshot agreement, without adding a
general untrusted-delta completeness guarantee.

Hardware scenes contain 150 children plus mixed text, alpha images and shadows.
Both selective modes run at scale 1/1.25/1.5 through dense/sparse color changes,
page-boundary batches, simultaneous subtree/local/layout edits, clipped group
opacity, reorder, skipped bases, removal/generation reuse, hit-vs-linear checks,
old-packet rendering after producer drop and mutation/equal-frame retry. Normal
gallery clicks use the owned path. Pure quad/shadow scenes use the original full
GPUI Scene reference; mixed/effect scenes use a fresh full-size consumer with
empty caches, as in the preceding stages. References share primitive shader math.

## Why publication changed

The historical `phase-probe` directory records the actual pre-change investigation
with 160 source hashes. At 10,000 descendants, shared placement spent about
**18.2 ms publishing**, **11.8 ms submitting**, and **7 µs committing**. Its source
manifest intentionally predates ownership changes and is separate from the four
final audits. This probe identified where to work; benefits below use the
controlled final implementation rather than comparing unrelated runs.

The original selective path stores dense candidates in a BTreeMap, clones old
pages, clones changed payloads into pages and also builds an independent delta
list. `SelectiveCopy` retains that strategy under the current packet API. The
default uses contiguous candidates, complete-page moves and a private indexed
delta over the authoritative root. Both controls share the current API and its
checked fallback; this is not an older release/executable comparison.

For the 10,000-descendant parent case, both modes extract **10,001 nodes**, upload
the same **96 property bytes / zero geometry bytes**, and redraw the full root.

| Logical work per update | Owned | Copy control |
| --- | ---: | ---: |
| Old page items cloned | 64 | 10,002 |
| Complete pages replaced by move | 156 | 0 |
| Updated items moved into selective pages | 10,001 | 0 |
| Delta payload items cloned | 0 | 10,001 |
| Delta items compared independently | 0 | 10,001 |
| Delta items shared with exact snapshot | 10,001 | 0 |

The first page contains the unchanged root, so it remains a 64-item COW copy.
This build's PaintItem is **368 bytes**: a 10,001-item delta has 3,680,368 logical
payload bytes versus 80,008 logical index bytes. Vec reserve capacity, root/page
directories, allocator overhead and shared resource payloads are excluded. These
counts do not measure RSS, memory traffic or driver allocations.

## Owner-to-GPU-completion measurements

Both modes keep the same tree/content, shared GPU representation, shader and
full root redraw on an 800×600 target. Ancestor translation updates N descendants;
the sparse control changes one material with zero recording/layout and uploads
160 quad bytes in both modes. Cold cases leave the hit index unused. Warm cases
build it before timing, maintain it through updates and query it once per frame.

Three executions each contain three rounds rotating control order, six warmups
and twenty timed frames per mode/round: **180 samples per mode/case**. Each frame
includes owner commit, publication, submission and GPU completion with at most
one in-flight frame. Phase samples separate commit, publication (including the
warm query when selected) and submission. Setup/readback/native presentation
are excluded; round-end reference images happen outside timing. Percentiles use
the nearest sorted sample, matching the executable, without filtering.

| Workload | Nodes | Hit index | Completed median ms, owned / copy | Change | Completed p95 ms, owned / copy | Publication median µs, owned / copy |
| --- | ---: | --- | ---: | ---: | ---: | ---: |
| Ancestor move | 1,000 | unused | 2.593 / 3.177 | -18.4% | 3.308 / 4.011 | 530.8 / 942.7 |
| Ancestor move | 10,000 | unused | 23.269 / 32.640 | -28.7% | 27.076 / 38.096 | 11598.3 / 18271.2 |
| Ancestor move | 1,000 | warm | 2.652 / 3.239 | -18.1% | 4.519 / 4.571 | 576.1 / 921.4 |
| Ancestor move | 10,000 | warm | 23.259 / 31.204 | -25.5% | 38.684 / 49.169 | 12045.5 / 17424.2 |
| Single material | 1,000 | unused | 1.115 / 1.256 | -11.2% | 3.039 / 2.575 | 18.4 / 19.4 |
| Single material | 10,000 | unused | 1.139 / 1.110 | +2.6% | 1.675 / 1.699 | 22.6 / 23.5 |
| Single material | 1,000 | warm | 0.515 / 0.533 | -3.5% | 0.918 / 0.902 | 19.2 / 19.8 |
| Single material | 10,000 | warm | 1.081 / 1.065 | +1.5% | 1.648 / 1.591 | 28.2 / 29.6 |

The ancestor cases consistently improve their per-execution medians on this
host. For 10,000 descendants, unused-index publication falls from about 18.27 to
11.60 ms and submission from 11.73 to 9.60 ms; completed median improves **1.40×**.
With a maintained hot index the completed median improves **1.34×**. Dense
extraction remains; the gain comes from removing duplicate publication and delta
work within the current complete resolved-snapshot contract.

Sparse material updates do **not** establish a general completed-time gain.
At 10,000 nodes, pooled completed medians regress about 2.6% without hits and 1.5%
with warm hits despite slightly smaller publication phases. Keep those losses.
The 1,000-node cold case is strongly variable: run-1 reports about 0.435/1.355 ms
but run-3 about 1.413/1.319 ms (owned/copy), and its completed p95 is worse with
ownership. No broad small-update latency benefit follows from the pooled median.

The slow warm 10,000-descendant block in run-3 is also retained: 32.555/42.202 ms,
with p95 41.621/52.654 ms. It raises the pooled warm p95 substantially in both
modes. [summary.json](summary.json) keeps every execution and round median;
run-1 through run-3 retain all raw paired phase/CPU/completed samples. Host drift
and driver/queue waiting remain possible. Differences between wall-time medians
are not isolated GPU execution times or GPU timestamps.

## Reproduction and limits

```powershell
./tooling/rendering-perf/run-retained-gpu.ps1 -Output target/page-validation -ValidationOnly
./tooling/rendering-perf/run-retained-gpu.ps1 -Output target/page-run-1 -PublicationOnly
# Repeat with fresh output directories for runs 2 and 3.
```

The final validation/run-1/run-2/run-3 have **23/21/21/23** compiler observations at
500 ms intervals, all empty. Each runner builds before launching the measured
executable. Observations can miss shorter compiler activity and do not exclude
other host/GPU work. All four selected-source manifests were checked again when
freezing these artifacts.

The experimental packet API changes from `Arc<[PaintItem]>` to
`Arc<PaintUpdates>`; contiguous callers use `to_vec`, and explicit item lists use
`Arc::new(PaintUpdates::from(...))`. Integer indexing, iteration and COW mutation
remain. See the [implementation contract](../../RETAINED_PAGE_OWNERSHIP.md).

Dense publication is still about 12 ms and submission about 10 ms for this
workload. Complete updates remain above a 16.7 ms budget; this does not establish
sustained 60/120 Hz, native presentation latency or energy benefits. Backend
allocation/dependency scans, full resolved subtree extraction, global physical
budgets/device loss, complex primitives and cross-platform/native-framebuffer
qualification remain in the [active full-goal tracker](../../RETAINED_IMPLEMENTATION_PROGRESS.md).
