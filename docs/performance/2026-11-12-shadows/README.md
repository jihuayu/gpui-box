# Rounded-box shadows and typed storage — 2026-11-12

Windows / NVIDIA GeForce GTX 1650 Ti / Vulkan / driver 571.96. Source base is
`fd86f7f`; the final full validation and three focused executions each contain
154 selected source hashes, all checked against the final working-tree bytes.
The final executable hash, compiler observations, test counts and overlapping
submission records are in [verification.json](verification.json).

The full hardware regression passes **285 scenarios**, including **281 byte-exact
pixel comparisons**, one independent analytical origin-only case and three
backend preflight/retry groups. The focused shadow suite passes **61 scenarios**
per execution, including **60 pixel comparisons** and its preflight group. Core
tests pass 119 with text/WGPU, the retained-only feature set passes 107, and
gallery/font/real-click tests pass 12. Selected core/gallery/perf Clippy targets
pass `-D warnings`; the desktop WGPU executable builds. Test logs and complete
hardware JSON/PNGs are preserved beside this file.

## Correctness and actual lifetime coverage

Immutable typed shadow lists propagate through normal owner transactions and the
gallery's existing rounded-border control. Core tests cover atomic draft edits,
rollback, equal/final-restored values, direct Arc sharing, no layout/measurement,
old-packet immutability, clipped overflow damage and unchanged body hit geometry.
The consumer rejects shadow deltas that disagree with their authoritative paint
snapshot, including when immutable-page validation reuse is enabled.

At scales 1/1.25/1.5, mixed rounded quads, images and real glyphs exercise shadow
color and quad color independently, translation, ancestor clipping, cropped
isolated opacity, negative spread, inset phase changes, list growth/shrink,
detach/restore, skipped frames, reset, removal and generation reuse. Single
shadow color changes assert **112 shadow bytes / zero quad bytes**; quad color
changes assert **160 quad bytes / zero shadow bytes**. Thirty-two adjacent shadow
changes assert **3,584 bytes in one range**. Phase/count changes rebuild draw
topology; material changes reuse it while invalidating raster content.

Independent expected pixels check red drop coverage, blue fill and green inset
coverage. A tail-only clip catches Scene culling against only the shadow core;
a collapsed inset hole still paints the element. Both compare against the full
GPUI renderer after the shared Scene paint-bounds correction. A reproduced
fractional zero-width clip originally painted a green line instead of black;
[regression-before-fix/stderr.log](regression-before-fix/stderr.log) preserves the
failed assertion and that run's original metadata. Those historical hashes are
intentionally separate from the four final audits. Six final cases check empty
width/height masks for quad, drop and inset, with expected black pixels and full
reference equality after vertex geometry is collapsed for empty masks.

The buffer-growth workload submits 64 large overlapping shadows, then grows to
128 without waiting/readback between submits. All four final executions observe
**two completion-held submissions**, prior serial **1**, completed serial **0**,
and shadow buffer generation **2 → 3**; the quad buffer also grows. Old CPU
packets retain 64 shadows, replaced typed GPU buffers/bindings retain explicit
leases, and final pixels match the full renderer. Five malformed records
(negative blur/size, NaN alpha/radius, finite-but-overflowing expanded blur) plus
an out-of-range draw reject without advancing serial; valid retry preserves the
old image with zero new shadow upload.

Pure quad/shadow scenes use the original complete GPUI Scene/render path as the
reference. Mixed image/text/isolated scenes use a fresh retained consumer with
full-viewport targets and empty caches. References share shader math; independent
analytical pixels provide additional checks. Normal-click validation checks
published shadows and WGPU pixels, without claiming a final native framebuffer
comparison. Ordinary isolated window paint retains the readback-to-atlas bridge.

## Completed-frame transport control

This backend microbenchmark changes one shadow color among N instances on an
800×600 surface. Both controls keep the same persistent CPU mirror, validation,
single instanced shadow draw, shader and full root redraw. Delta sends one
112-byte record; full upload sends **the declared N-record prefix**. Geometric
spare capacity and the unused quad placeholder are excluded from full upload.
This controls upload transport rather than rerunning all owner/UI work.

Each execution has three rounds with rotating mode order, six warmups and twenty
timed updates per mode/round. Three executions yield 180 samples per mode/case.
Each timed span includes backend validation, submit and GPU completion with
maximum in-flight count one. CPU fields stop after submit; completed fields stop
after `finish()`. Readback/reference comparison happens outside timing after
each mode/round. UI transactions, Taffy, paint recording and native presentation
are excluded. Percentiles below select the nearest sample in sorted pooled data,
matching the benchmark's percentile rule; samples are not filtered.

| Shadows | Delta / full bytes per update | Completed median µs, delta / full | Median time change | Completed p95 µs, delta / full | CPU submit median µs, delta / full |
| ---: | ---: | ---: | ---: | ---: | ---: |
| 1,000 | 112 / 112,000 | 189.6 / 221.6 | −14.4% | 449.8 / 446.9 | 71.8 / 100.0 |
| 10,000 | 112 / 1,120,000 | 386.9 / 692.7 | −44.1% | 654.7 / 869.9 | 97.2 / 363.8 |

For 10,000 instances the pooled median improves **1.79×**; per-execution medians
are 420.1/714.2, 379.7/714.6 and 398.6/619.7 µs (delta/full). The smaller case
improves its pooled median **1.17×**, but has no pooled p95 improvement and the
third execution's delta p95 is worse (496.2 versus 359.7 µs). Round-level medians
also retain losses, such as 259.9 versus 242.1 µs for 1,000 instances. Host drift
is substantial; these results support reduced upload cost at larger N, without
establishing a general small-scene latency improvement or whole-UI/FPS gain.
Completed mean times are also retained: 240.2/255.8 µs at 1,000 and 435.7/697.4 µs
at 10,000. [summary.json](summary.json) includes all per-execution/round summaries;
run-1 through run-3 retain every CPU/completed sample and pixel checks.

## Reproduction and evidence boundaries

```powershell
./tooling/rendering-perf/run-retained-gpu.ps1 -Output target/shadows-validation -ValidationOnly
./tooling/rendering-perf/run-retained-gpu.ps1 -Output target/shadows-run-1 -ShadowsOnly
# Repeat with fresh output directories for runs 2 and 3.
```

Each runner builds before measurement. Final validation/run-1/run-2/run-3 record
11/4/3/3 compiler observations at 500 ms intervals, with no observed cargo,
rustc or clippy process. Shorter processes and unrelated host/GPU activity are
not excluded. The selected-source hashes were rechecked when freezing evidence.
Earlier development runs that uploaded geometric spare slots used a different
baseline and are not combined with this declared-prefix comparison.

The shared Scene change and WGPU crop/empty-clip shaders have Vulkan evidence;
native DirectX/Metal framebuffers remain unqualified. Typed storage currently
serves Quad and Shadow; shared GPU property/material trees, arbitrary shapes,
filters, global resource budgets/compaction, native surface presentation, forced
device loss and sustained cross-platform 60/120 Hz/input/energy qualification
remain in the [active full-goal tracker](../../RETAINED_IMPLEMENTATION_PROGRESS.md).
