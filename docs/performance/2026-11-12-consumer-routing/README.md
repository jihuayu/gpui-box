# Submitted-snapshot consumer routing — 2026-11-12

Windows / NVIDIA GeForce GTX 1650 Ti / Vulkan / driver 571.96. Source base is
`05a2a23`. The final full validation and three focused executions each carry
**166 selected source hashes**, all checked against the final working-tree
bytes. [verification.json](verification.json) records the executable hash,
process audits, test counts and scope limits.

The full regression passes **939 hardware scenarios / 904 byte-exact pixel
comparisons**. Each focused execution passes **456 scenarios / 432 pixel
comparisons** before timing. Core tests pass **128**, retained-only tests **110**,
and gallery/font/hardware-click tests **12**. Selected core/gallery/perf
all-target Clippy passes `-D warnings`; the desktop WGPU executable builds.
Logs, validation records, raw phase samples and PNGs are retained in this folder.

## Contract and correctness

The consumer compares actual local primitive inputs against its most recently
successfully submitted immutable snapshot. Shared Quad/Shadow geometry can stay
unchanged under ancestor motion/clip/scroll; spatial updates retain root and
every-32-node resolved anchors. Material, size, radius and shadow-list changes
route independently. Reparenting rebuilds spatial metadata, while order/count/
identity changes use the original preparation fallback. World-instance mode
still includes resolved origin/clip in geometry inputs. Image/glyph paths retain
their existing resource/property updates.

Private normal-producer provenance attests the exact base/current tree,
revision, paint/property roots, removal/order identities and damage. Merely
matching revision counters cannot establish completeness. A missing or modified
proof with a claimed matching base forces complete snapshot resynchronization,
retirement from the authoritative live set and full redraw in **both** A/B modes.
Public delta mutation clears its proof. Weak root references keep no old payload
chain alive. A conflicting authoritative scene at the currently submitted
tree/revision is rejected before equal-frame reuse. The validation cache can
advance on a later-rejected packet without replacing the submitted routing base.

Unit tests cover selective/copy/full provenance, changed roots and headers,
damage/removal/order/delta mutation, old payload release, deep anchors, shared
depth reuse and independent material/size/radius/shadow classification.

New hardware tests pass the same mixed Quad/Shadow/image/glyph packets through
cached and original preparation with both shared/world placement, both owned/
copy publishers and scales 1/1.25/1.5. Cases include fractional translation/clip,
deep anchors, opacity, material, radius, shadow phase/count/removal, size, image/
text changes, reparenting, skipped bases, removal/generation reuse, reset and
mode switching. Deliberately empty deltas/damage and missing removals recover
from the complete scene. Conflicting same-revision roots reject safely; valid
equal retries and following normal deltas still work. The prior hardware suite
is included. Mixed/effect references use a fresh full-size world-instance
consumer with empty caches; pure Scene cases use the original GPUI renderer.
References share primitive shader math and do not provide cross-driver proof.

## Controlled completed-owner measurements

Both modes use the same owned publisher, complete immutable packet API, private
transition verification, shared GPU placement, shader and full root redraw on an
800×600 target. Only cached local input routing versus original delta preparation
changes. This compares two current controls, not separate releases. Earlier
spatial/publication benchmarks explicitly disable the new routing on both sides.

Timing scenes contain quads: ancestor translation updates N descendants; the
sparse control changes one material. Mixed primitives/shadows above have
correctness coverage, but no separate dense shadow timing claim is made here.
Cold cases leave the hit index unused; warm cases build it before timing,
maintain it and query it once each frame.

Three executions each contain three rounds rotating control order, six warmups
and twenty timed frames per mode/round: **180 samples per mode/case**. Every
sample includes owner commit, publication, submission and GPU completion with
at most one frame in flight. Phase samples separate commit, publication
(including the warm query) and submission. Setup/readback/native presentation
are excluded. Round-end reference images run outside timing: **144** reference
checks across the three executions. Percentiles select the nearest sorted
sample, matching the executable; no samples are filtered.

| Workload | Descendants | Hit index | Completed median ms, cached / direct | Change | Completed p95 ms, cached / direct | Submission median µs, cached / direct |
| --- | ---: | --- | ---: | ---: | ---: | ---: |
| Ancestor move | 1,000 | unused | 2.208 / 2.773 | -20.4% | 3.040 / 4.429 | 857.6 / 1248.5 |
| Ancestor move | 10,000 | unused | 20.083 / 23.053 | -12.9% | 26.123 / 28.254 | 7069.2 / 9985.1 |
| Ancestor move | 1,000 | warm | 2.443 / 2.845 | -14.1% | 3.571 / 4.398 | 894.2 / 1287.7 |
| Ancestor move | 10,000 | warm | 20.645 / 23.087 | -10.6% | 24.681 / 28.037 | 6953.7 / 9605.9 |
| Single material | 1,000 | unused | 1.278 / 1.220 | +4.8% | 2.740 / 2.333 | 293.5 / 276.0 |
| Single material | 10,000 | unused | 1.026 / 1.012 | +1.4% | 1.354 / 1.292 | 692.0 / 678.5 |
| Single material | 1,000 | warm | 0.467 / 0.498 | -6.3% | 0.696 / 0.727 | 229.3 / 241.2 |
| Single material | 10,000 | warm | 0.982 / 0.956 | +2.8% | 1.302 / 1.299 | 650.2 / 618.9 |

Ancestor movement improves its completed median in all three executions for
both sizes and hit-index states on this host. At 10,000 descendants, unused-index
submission falls about **29.2%**, with completed time improving **1.15×**; warm
submission falls about **27.6%**, with completed time improving **1.12×**.
Publication remains about **12 ms** on both sides. This stage removes repeated
consumer preparation, rather than descendant extraction or raster work.

For 10,000 descendants both sides still extract/inspect **10,001 items**, move
**156 full paint pages**, copy **64 old page items**, share **10,001 delta items**
and upload **96 spatial bytes / zero Quad bytes**. GPU work and upload volume are
the same. Work counters distinguish what is avoided:

| Preparation candidates per ancestor update | Cached | Direct |
| --- | ---: | ---: |
| Quad items | 0 | 10,001 |
| Shadow items | 0 | 10,001 |
| Spatial items | 1 | 10,001 |
| Complete spatial rebuild items | 0 | 0 |

Shadow counters count candidates, including empty lists in the original path;
this table does not mean 10,001 actual shadows. The sparse case prepares one
Quad in both modes; cached preparation skips its unchanged spatial/shadow inputs.

Sparse updates establish **no general latency gain**. At 10,000 nodes, completed
medians regress 1.4% unused / 2.8% warm, and submission medians are also slightly
higher. The unused 1,000-node case is particularly variable: run-1 is
1.730/0.969 ms (cached/direct), run-2 0.530/1.264 ms, and run-3 1.437/1.340 ms.
Its pooled median and p95 both regress. Even the pooled warm 1,000-node gain
reverses in run-3. All losses, outliers, execution and round medians are kept in
[summary.json](summary.json) and the raw run-1/run-2/run-3 samples.

## Reproduction and limits

```powershell
./tooling/rendering-perf/run-retained-gpu.ps1 -Output target/routing-validation -ValidationOnly
./tooling/rendering-perf/run-retained-gpu.ps1 -Output target/routing-run-1 -RoutingOnly
# Repeat with new output directories for runs 2 and 3.
```

Final validation/run-1/run-2/run-3 have **47/38/39/39** compiler observations at
500 ms intervals, all empty. Each runner builds before launching its executable.
This sampling can miss shorter activity and excludes no unrelated host/GPU work.
Rotating block order is not perfectly balanced; host drift, scheduling and
driver/queue waiting remain possible. Wall-time phase medians do not add exactly
to pooled completed medians. Completion differences are not GPU timestamps.

The cache shares one current pair of immutable roots and holds one depth byte
per paint item in shared cached mode, with temporary routing entries for changed
input classes. Provenance uses Weak root headers. This is logical storage
accounting, not measured RSS or a global resource-budget guarantee.

Dense updates still take about **20 ms**, above a 16.7 ms budget. Full resolved
extraction, composition bounds/topology work, backend allocation-prefix and
property-ancestry scans remain. This does not establish sustained 60/120 Hz,
native presentation/input latency, energy savings, general affine/clip/filter
support, forced device-loss recovery or cross-platform acceptance. The complete
RFC goal remains active in the [progress tracker](../../RETAINED_IMPLEMENTATION_PROGRESS.md).
