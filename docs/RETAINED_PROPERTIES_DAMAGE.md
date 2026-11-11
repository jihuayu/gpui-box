# Clip, scroll and damage redraw

The retained update flow now supports rectangular subtree clipping and scroll
offsets without layout or local command recording. The offscreen GPU consumer
can preserve its target and redraw a conservative damage rectangle. Hardware
verification ran on Windows, GTX 1650 Ti, Vulkan. This is an incremental stage
of the [active complete-runtime goal](RETAINED_IMPLEMENTATION_PROGRESS.md).

## Shared property state

Persistent nodes own local translation, optional border-box clip and requested
scroll offsets. FramePacket owns sparse transform/clip/scroll chains keyed by
generation-checked node identity. Descendants share inherited property IDs.
Clips intersect through ancestry and with the viewport. Scroll offsets clamp to
the current Taffy content extent and affect descendants; the viewport background
stays stationary. Painting, GPU masks and hit testing read the resolved geometry
in the same packet.

Transactions validate finite, nonnegative scroll offsets and valid clip bounds
before applying changes. Equality is checked after field coalescing. Clip and
scroll changes use the property route, preserve shared command chunks and skip
Taffy. A request clamped to the existing offset can publish a semantic property
change without a visible damage region or GPU draw.

The property maps describe shared ancestry. Geometry is still resolved on the
CPU and uploaded into affected instances. Full affine transforms, a separate
effect tree and GPU property indirection remain pending.

## Damage and preservation

Changes union the old and new clipped ink bounds, covering movement, shrink,
clipping and alpha changes. Initial frames, viewport/structure changes, skipped
bases, scale changes and renderer reconstruction conservatively redraw fully.
The backend preserves its private texture, loads it for partial updates, draws
an opaque black clear quad only inside the scissor and replays all instances in
the original order. A one-device-pixel fringe covers rectangular edge AA.
An independent reference renderer leaves the live target untouched.

`CompositorPolicy` offers `Full` (default), `Damage`, and `Auto` with an explicit
maximum damage-area fraction. Auto chooses full redraw for larger damage and
skips drawing when no visible pixels changed. The threshold is a configurable
area heuristic, not a measured adaptive cost model. All ordered instance ranges
are still submitted for partial redraw; the scissor reduces fragment work, not
vertex processing or draw calls. Raster-cache layers and budgets are pending.

These preservation rules apply to the owned offscreen texture. Native swapchain
buffer age, surface presentation and recovery need separate implementation.

## Completed-work A/B

The runner compares Full, Damage and Auto (35% area threshold) in rotating order:
three rounds, eight warmup updates and forty measured updates per round/policy,
up to three frames in flight. Each update changes the same 24×24 translucent
rectangle. Final completion is awaited without a texture readback. Timings
include transaction commit, frame generation, submission, backpressure and final
drain. They exclude screenshot readback/encoding and native presentation, and
are wall-clock throughput measurements rather than GPU-only timestamps.

Frozen final run, mean milliseconds per completed frame:

| Scene | Full | Damage | Auto | Damage speedup |
| --- | ---: | ---: | ---: | ---: |
| 576 grid rectangles, 1920×1080 | 0.620 | 0.666 | 0.707 | 0.93× |
| 32 full-screen translucent layers, 1920×1080 | 5.261 | 0.120 | 0.112 | 43.77× |
| 32 full-screen translucent layers, 3840×2160 | 20.428 | 0.181 | 0.155 | 112.89× |

All paths upload 160 bytes per color change. Partial redraw covers 676 device
pixels (26×26 including the fringe) and adds one clear draw. Final pixels match
the original complete GPUI render path for every benchmark round and policy.
The grid is dominated by CPU snapshot work and shows no dependable gain; in the
final run partial redraw was slower. The transparency cases deliberately stress
fill rate and must not be read as ordinary application-wide speedups. Small
sub-millisecond timings varied between runs; raw per-round values are retained.
These results support keeping Full as the default until a broader cost model
can avoid unhelpful partial redraws.

## Verification and reproduction

The runner passed 26 whole-image pixel scenarios: the preceding 16 lifecycle
cases plus clip setup, color damage, scrolling, clip shrink, layout shrink,
movement covering old/new ink, a clamped scroll with zero drawing, fractional
1.25× scale, adaptive full redraw and adaptive small damage. The normal GPUI
gallery now has a scroll button; ordinary click tests and explicit hardware
tests cover click → scroll transaction → FramePacket → masked paint/GPU output.
The hardware click case also enables damage redraw for its scroll update.

Fifteen core tests and both normal-click tests passed. Core, gallery and runner
Clippy checks passed with `-D warnings`. The desktop executable built with the
retained WGPU feature; the pre-existing GPU benchmark also checked with retained
support disabled. Explicit `poll` and `finish` APIs release completed leases
without requiring image readback; host idle-poll scheduling remains pending.

```powershell
./tooling/rendering-perf/run-retained-gpu.ps1 -Output target/retained-properties-gpu
mise exec -- cargo test --locked --offline -p gpui-box --features retained-wgpu
mise exec -- cargo test --locked --offline -p gpui-box-example-hello --features retained-wgpu normal_click -- --include-ignored
```

[Frozen evidence](performance/2026-11-11-properties/README.md) contains source
hashes, adapter details, all 26 screenshots, validation JSON and timing data.
Group opacity/isolation, text/images, raster caches and the remaining full-goal
items continue in the acceptance table.
