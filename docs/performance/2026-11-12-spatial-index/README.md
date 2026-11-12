# Indexed backend spatial processing — 2026-11-12

Windows / NVIDIA GeForce GTX 1650 Ti / Vulkan / driver 571.96. Source base is
`72dec75`. Final full validation and three focused executions each carry
**169 selected source hashes**, verified against final working-tree bytes.
[verification.json](verification.json) records the executable hash and audits.

Final regression passes **1399 hardware scenarios / 1336 byte-exact pixel
comparisons**. Each focused execution passes **460 scenarios / 432 pixel
comparisons** before timing. Core tests pass **134**, retained-only tests **110**,
and gallery/font/hardware-click tests **12**. Selected core/gallery/perf
all-target Clippy passes `-D warnings`; the desktop WGPU executable builds.
Final logs, metadata, raw samples, validation records and PNGs are retained.

## Backend state and failure coverage

The backend retains shader-parent adjacency and memoized resolved placement,
plus independent Quad/Shadow inverse property memberships. Stable sparse spatial
changes validate affected old descendants against the final projected graph and
resolve by bounded depth. Unchanged primitive references use a committed maximum
bound; changed references and invalidated prefixes are checked explicitly.
Selected property users produce coalesced raster dirty ranges.

The addressable prefix includes unreferenced slots. Scale/capacity/mode changes
revalidate actual retained mirror values, including newly exposed old values.
Overlapping writes use final-write projection, consistent with storage transport.
Existing typed field/range checks remain. Authoritative CPU plans commit only
after all image/glyph/layer/damage preflight checks succeed. Rejected inputs may
touch scratch stamps but cannot replace the actual applied base.

The actual pure backend implementation is compiled into locked core tests.
Six added cases compare 600 graph edits against an independent full ancestry
walk, check 200 inverse-reference edits against full prefix selection, verify
child edges/cycles/overflow/scale, dropped plans, capacity changes, reexposed
references and dense-to-sparse ancestor reconstruction.

Hardware compares the same mixed Quad/Shadow/image/glyph frames through indexed
and original full backends, both publisher/placement controls and scales
1/1.25/1.5. It retains the previous regression and covers clips, opacity,
material/size/radius/shadow changes, deep anchors, reparenting, skipped bases,
missing deltas/removals, slot reuse, reset and mode changes. Raw backend cases
reject malformed nodes/references, unreferenced descendant overflow and later
layer/damage failures before serial/state changes. Valid zero-write retries keep
pixels unchanged. Shrink/grow reexposes real mirror values. Dense fallback still
rejects unchanged bad children; a following sparse overflow uses the actual
updated ancestor rather than a stale memo. Mixed references use fresh full-size
world-instance consumers; pure Scene references use original GPUI rendering.
Primitive shader math is shared, so these are not independent-driver proofs.

## Preserved losing prototype and dense fallback

The first unconditional index kept every resolved memo current and used inverse
users on dense ancestor changes. Its separate three-execution
[probe summary](unconditional-probe/summary.json) shows 10,000-descendant completed
medians **20.372/19.291 ms (+5.6%)** unused-index and **20.417/18.816 ms (+8.5%)**
warm, despite large sparse gains. Raw timing/validation/process records, metadata,
changed runtime/protocol [source snapshots](unconditional-probe/source-snapshot.zip)
and [tracked source patch](unconditional-probe/source.patch.zip) are in
`unconditional-probe`. Its four 169-source manifests were checked against the
then-current bytes before changing the implementation. Probe PNGs remain in
their original target directories; final PNGs are archived here.

The default now stops frontier construction above `max(64, capacity / 4)` nodes
and uses original full projected spatial validation and ancestry invalidation.
Reference checks still reuse their valid bound. Dense commit changes topology
and invalidates resolved memos without rewriting all values. Following sparse
spatial updates read needed ancestors from the actual mirror with a bounded
walk; complete rebuild restores memo validity. This is an experimental fixed
heuristic, not a measured general cost model. It reduces dense bookkeeping but
does **not** establish a dense completed-time improvement, as the results below
show. The initial probe and final implementation are separate source states;
only their own paired controls support their respective timing claims.

## Final controlled owner-to-completion measurements

Both modes use the same owned publisher, private transition provenance, cached
consumer input routing, shared GPU layout/shader and full root redraw on an
800×600 target. Only the adaptive backend index versus original complete scans
changes. Earlier spatial/publication/consumer-routing benchmarks explicitly
disable this backend index on both sides to preserve their comparison scope.

Timing scenes contain quads. Dense movement translates a parent of N descendants;
sparse work changes one material. Mixed primitive/shadow scenes have correctness
coverage rather than separate throughput claims. Cold cases leave the hit index
unused; warm cases prebuild, maintain and query it once per frame.

Three executions each contain three rounds rotating control order, six warmups
and twenty timed frames per mode/round: **180 samples per mode/case**. Every
sample includes commit, publication, submission and GPU completion, with at most
one frame in flight. Phase samples separate the three CPU phases, including the
warm hit query in publication. Setup/readback/native presentation are excluded;
**144** round-end reference checks run outside timing. Percentiles select the
nearest sorted sample, matching the executable; no samples are filtered.

| Workload | Descendants | Hit index | Completed median ms, indexed / full | Change | Completed p95 ms, indexed / full | Submission median µs, indexed / full |
| --- | ---: | --- | ---: | ---: | ---: | ---: |
| Ancestor move | 1,000 | unused | 2.039 / 2.144 | -4.9% | 3.170 / 3.323 | 800.0 / 805.3 |
| Ancestor move | 10,000 | unused | 20.198 / 18.976 | +6.4% | 24.565 / 22.774 | 6951.7 / 6649.6 |
| Ancestor move | 1,000 | warm | 2.188 / 2.058 | +6.3% | 3.426 / 3.149 | 797.9 / 782.9 |
| Ancestor move | 10,000 | warm | 19.125 / 18.905 | +1.2% | 22.781 / 22.261 | 6415.2 / 6550.5 |
| Single material | 1,000 | unused | 1.297 / 1.212 | +7.0% | 2.494 / 2.314 | 241.5 / 263.6 |
| Single material | 10,000 | unused | 0.602 / 1.091 | -44.8% | 1.376 / 2.073 | 219.4 / 681.7 |
| Single material | 1,000 | warm | 0.507 / 0.511 | -0.8% | 0.904 / 0.896 | 186.8 / 219.7 |
| Single material | 10,000 | warm | 0.561 / 1.042 | -46.2% | 0.846 / 1.584 | 213.7 / 656.3 |

At 10,000 nodes, sparse completed medians improve in all three executions and
both index states: **1.81×** unused and **1.86×** warm. Submission medians fall
about **67.8% / 67.4%**. GPU uploads remain **160 Quad bytes / zero spatial bytes**
and full-root raster work is unchanged. Producer work remains one extracted item,
64 copied old page items and zero recorded chunks. The backend difference is:

| Sparse update checks, 10,000 descendants | Indexed | Original full |
| --- | ---: | ---: |
| Spatial edges / resolved nodes | 0 | 10,002 |
| Quad prefix references | 1 | 10,002 |
| Shadow references | 0 | 0 |

The sparse benefit is skipping unchanged validation, not decreasing GPU work.
Sparse 1,000-node completed time has no stable benefit: unused-index pooled
median regresses 7.0%, and the small warm gain reverses in run-1.

Dense ancestor movement takes the fallback in every timed indexed sample. Both
sides validate **10,002 spatial nodes**, invalidate **10,001 Quad users**, extract
**10,001 items** and upload **96 spatial bytes / zero geometry bytes** at N=10,000.
The indexed side skips unchanged reference-prefix checks, but that alone does
not establish an overall gain. Unused-index submission regresses about 4.5%,
while warm submission improves about 2.1%; completed medians regress 6.4% and
1.2%. In run-1, unused-index publication also varies 12.291/10.970 ms despite the
same publisher. Run-3 reverses the dense unused completed result to
18.420/19.461 ms. Keep the losses and host/block variability rather than claim
that fallback eliminates dense regressions.

Run-3's unused sparse 10,000-node block is slower in both modes than earlier
runs: **0.923/1.538 ms** versus run-1 **0.506/1.034 ms**. All execution/round
medians and raw samples are retained in [summary.json](summary.json) and run-1
through run-3. Phase medians do not add exactly to pooled completed medians.

## Reproduction and open acceptance

```powershell
./tooling/rendering-perf/run-retained-gpu.ps1 -Output target/spatial-index-validation -ValidationOnly
./tooling/rendering-perf/run-retained-gpu.ps1 -Output target/spatial-index-run-1 -SpatialIndexOnly
# Repeat with new output directories for runs 2 and 3.
```

Final validation/run-1/run-2/run-3 have **72/37/38/37** process observations at
500 ms intervals, all without compiler processes. Each runner builds before
launching. This sampling can miss shorter activity and excludes no unrelated
host/GPU work. Rotating block order is not perfectly balanced. Wall time includes
host scheduling and driver/queue waits; differences are not GPU timestamps.

The index adds CPU adjacency/positions, resolved values, scratch stamps,
references and inverse memberships; it adds no GPU records. This is not RSS or
global-budget accounting, and high-water memory/compaction remain open. Sparse
spatial changes after dense frames have correctness coverage but no independent
latency measurement here; dense color/scatter and other workload shapes also
need broader qualification.

Dense updates still cost about 19–20 ms. Producer extraction, composition/
effect-reference, resource/layer/lease scans, measured adaptive cost policy,
general primitives/layout, physical budgets/device loss, native surfaces and
cross-platform/sustained 60/120 Hz/input/energy acceptance remain open in the
[full-goal tracker](../../RETAINED_IMPLEMENTATION_PROGRESS.md).
