# Owned intrinsic inputs for persistent Taffy

The persistent tree now stores `MeasureSnapshot` values in Taffy's long-lived
node context. Layout callbacks consume those values without capturing `App`,
`Window`, business closures or borrowed element state. A typed measure property
connects normal signals and logical scope cleanup to this same input channel.

This implements the owned-input foundation of RFC sections 5 and 25, within the
[complete-runtime goal](RETAINED_IMPLEMENTATION_PROGRESS.md).

## Supported contract

`ImageIntrinsicSize` owns natural width/height. The adapter marks these leaves
as replaced content and derives aspect ratio when the caller has not supplied
one. Explicit dimensions, percentages and min/max remain Taffy responsibilities.
Changing natural size invalidates the node's Taffy context and ancestor caches.
Resolved image content fits the allocated box, rather than exposing its original
bitmap dimensions as scroll overflow. This stage draws a colored rectangle with
that geometry; image decoding, texture upload and resource versions are pending.

`CustomMeasureSnapshot` owns fixed content width/height and an optional first
baseline. It represents metrics prepared before layout, with no arbitrary user
callback. Top padding/border are included in the baseline used by parent Flex
alignment. Fixed content can still overflow a smaller allocation. Wrapped text
and custom constraint-dependent measurement need additional snapshot types.

`Transaction::measure` and `Property::measure` accept or remove these snapshots.
The transaction validates every input before publication. Dimensions must be
finite and nonnegative; baselines must lie within the fixed content height.
Measured nodes are leaves: attaching a measure to a container or appending a
child to a measured node fails. Equal inputs and final-value restoration avoid
layout and frame requests. Style and measurement changes in one batch use their
combined final values. Inferred image style does not leak into the public Style.

`FramePacket::layout_stats` records actual leaf-layout callbacks and actual
content-measure invocations reached after Taffy's cache. Color/translation updates
and unchanged frames report zero. These counters do not instrument all Taffy
container visits or directly report its internal cache hit count.

## Correctness and integration

Six measurement cases pass within the 30 retained-WGPU core tests. They cover
intrinsic sizing, known-vs-available constraints, percentage and explicit ratios,
zero-size resources, overflow, baseline changes, atomic rejection, retained
commands, affected sibling positions, source retirement and cached siblings.
The normal-click tests update the owned intrinsic input through the UI signal
route and verify layout runs while existing paint commands are reused. The
hardware variant compares the resulting GPU image with full GPUI output.

The complete hardware suite passes 54 pixel scenarios. Four new scenes cover
initial measured geometry, dependent sibling movement, fractional device scale
and 32 queued input updates with no more than three submissions in flight.
Write/restore submits no GPU work. This checks sizing-driven rectangle output;
it is not image-primitive or glyph rendering qualification.

## CPU experiment

`measure-runtime-perf` compares one fixed intrinsic-size update on a persistent
tree against constructing a fresh tree with identical final inputs, performing
its first layout and extracting its frame. Every result's bounds, clips and
commands are checked for equality outside timing. Three rounds rotate execution
order, each with eight warmup and twenty measured changes. Frame extraction is
included; text shaping, GPU work, window rendering and fresh-tree destruction
are excluded. This reference is a fresh retained tree, not the original GPUI
application lifecycle.

Frozen CPU medians on Ryzen 7 4800H/Windows 11, release build:

| Measured leaves | Retained commit | Retained total | Fresh rebuild total | Median ratio |
| --- | ---: | ---: | ---: | ---: |
| 300 | 10.7 µs | 0.592 ms | 1.351 ms | 2.28× |
| 3,000 | 19.9 µs | 5.482 ms | 13.200 ms | 2.41× |
| 30,000 | 22.5 µs | 62.009 ms | 132.817 ms | 2.14× |

Retained total p95 values are 0.759 / 7.265 / 93.207 ms. These are sequential
CPU samples, not throughput under sustained animation or input-to-photon latency.

Actual measure callbacks fall from 4×N in the fresh layout to four for the changed
leaf. Unchanged siblings reuse cached measurements even when their world positions
move. No existing local paint commands are rerecorded. Frame extraction still
visits all nodes, so the complete update is not O(1) and can exceed a frame budget
at large N. See the [frozen samples](performance/2026-11-11-measure/README.md) for
phase timings and tail latency.

```powershell
./tooling/rendering-perf/run-measure.ps1 -Output target/measure-runtime-perf
./tooling/rendering-perf/run-retained-gpu.ps1 -Output target/retained-measure-gpu
```

Owned paragraph shaping/wrapping, font epochs, decoded image resources, custom
constraint-dependent snapshots, proved layout islands and full layout-visit
instrumentation remain unfinished. Native presentation and cross-platform
qualification remain in the acceptance table.
