# Glyph geometry reuse and delta evidence

This freezes [immutable geometry and glyph deltas](../../RETAINED_GLYPH_DELTAS.md)
on base `ef387b5`. The adapter is GTX 1650 Ti / Vulkan 571.96; real glyph masks
come from Cosmic/Swash and bundled Noto Sans CJK SC. All 46 source SHA256 entries
were checked against the measured tree before freezing.

`validation.json` records 112 passing pixel scenarios, including 29 text cases.
New checks verify partial updates preserve unchanged resources, shared-owner
removal, logical full-resync reuse, hiding/restoring the sole mask owner, and
zero per-glyph work for material/property-only updates. Direct backend checks
also reject mask-ID redefinitions, duplicate/conflicting updates and malformed
geometry, then successfully submit after rejection. An atomic resource swap
retains both masks with zero reupload. These additional assertions run before
the numbered image scenarios. The retained/full images share the same shader;
the independent CPU oracle verifies mask blend/clip/order using packet masks.

The core suite has 45 tests, including cache-source immutability and Arc lifetime;
the shared example has ten tests including both normal-click paths. Clippy with
`-D warnings` passes the combined features, WGPU without text, and the non-WGPU
example. Desktop WGPU builds. No native surface, device-loss or other-platform
qualification is implied.

`glyph-geometry-benchmark.json` is the main same-run comparison. Both modes use
the same shader, real immutable masks, warm atlas, alternating colors, full redraw
and an 800 × 600 target. Retain mode clones the geometry Arc. Recreate mode clones
local glyph records/mask references, constructs/validates another geometry and
rebuilds/uploads its instances. Timers include that work and backend submit.
Both exclude shaping, glyph rasterization, UI transaction/layout/snapshot and
readback. Completed timing additionally waits for GPU completion. Three rounds
alternate mode order, six warmups then twenty measured samples each (60/mode).
Outside-timing readbacks verify identical pixels per mode/round.

| Glyphs | Mode | CPU submit median / P95 | Submit + completion median | Instance upload/frame |
| --- | --- | --- | --- | --- |
| 450 | Retain geometry | 334.6 / 443.2 µs | 626.5 µs | 0 |
| 450 | Recreate geometry/instances | 441.9 / 820.0 µs | 754.7 µs | 21,600 bytes |
| 4,500 | Retain geometry | 276.4 / 412.6 µs | 736.2 µs | 0 |
| 4,500 | Recreate geometry/instances | 739.0 / 874.5 µs | 1,239.2 µs | 216,000 bytes |

The 4,500 case repeats the paragraph ten times at the same positions, deliberately
increasing geometry work and overdraw while keeping mask count unchanged. Median
CPU ratios are 1.32× / 2.67× at 450 / 4,500 glyphs. Round CPU medians for 4,500
retain are 335.0/283.5/238.7 µs, recreate 757.3/682.9/739.0 µs. The direct backend
benchmark exercises chunk retention, not a full legacy application comparison.

Timing variation is substantial: 4,500 retain is faster than 450 retain in pooled
samples despite greater GPU work. An earlier run overlapping local Cargo build
is preserved as `concurrent-build-*-benchmark.json`; its absolute timings are
higher and the 450 completed-work ordering reverses. Primary measurements above
were rerun after local compilation finished. Other OS/GPU load is uncontrolled.
Deterministic zero rebuild/validation counters and upload elimination are stronger
evidence than a universal latency multiplier; no sustained FPS/energy claim follows.

`text-benchmark.json` records the integrated FramePacket path: warm color median
279.2 µs, warm color+clip 388.6 µs, cold backend 1,174.3 µs including GPU completion.
Warm modes rebuild/validate zero glyph instances per frame; cold rebuilds and
admits 450. CPU submit includes full packet validation and draw planning (126.6,
218.4 and 829.6 µs medians respectively). Reset/pipeline construction is excluded.
Large P95/round variance remains. The other benchmark JSON files are regenerated
damage/layer/image workloads, included separately for reproducibility.

This does not remove paint-item scans, composition-order/layer traversal, live draw
ID validation or scene-wide snapshot construction. Initial backend geometry
conversion still scans glyphs once and stores duplicated local records. General
stable chunk IDs, spatial/indexed extraction, physical memory bounds, pressure
recovery and native presentation remain acceptance work.
