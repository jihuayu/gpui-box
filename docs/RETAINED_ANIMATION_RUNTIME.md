# Render-owned animation timelines

Status: experimental Rust API, 2026-11-19. Full RFC remains active.
See [decision and costs](ADR_RETAINED_ANIMATION_RUNTIME.md).

`retained::animation::AnimationRuntime` owns animation controllers over a complete
immutable `LocalFrame`. It contains no LogicalUi, Signal, Window, font service or
frontend callback. A host can move it to a render worker, accept ordered controls,
sample a monotonic target presentation time, and send the complete sampled frame
to the existing GPU or software consumer. No per-frame Store/source write or
Flex recomputation is required for translation or opacity.

```rust,ignore
let mut runtime = AnimationRuntime::new(source, AnimationLimits::default())?;
runtime.apply(AnimationBatch {
    scene: runtime.scene(),
    sequence: runtime.next_sequence(),
    controls: vec![AnimationControl::Animate(AnimationSpec {
        id: 1,
        node,
        destination: AnimationValue::Translation([120.0, 0.0]),
        curve: AnimationCurve::Spring(Spring {
            mass: 1.0, stiffness: 180.0, damping: 24.0,
        }),
        duration: Duration::from_millis(700),
        start: AnimationStart::Now,
        velocity: RetargetVelocity::Preserve,
    })],
}, now)?;
let output = runtime.sample(target_time)?;
gpu.submit_local(output.frame(), physical_size, scale)?;
// Install the complete image before adopting output.input_scene(&source_scene).
for notice in runtime.drain_notices() { /* deliver outcome to the UI */ }
```

## Values, curves and controller lifetime

One controller owns each `(NodeId, AnimatedProperty)` pair. Translation is an
absolute local property value, not a delta from the authored translation. Opacity
is the existing isolated group property. Layout size, scroll, clip, scale,
rotation, color and resource preparation are not animation properties here.
Opacity sampling preserves the normal composition path; it is not guaranteed to
be a cheap texture composite.

IDs are nonzero and strictly increasing per runtime; batch sequence must equal
next_sequence, and each batch names the current authored tree/revision. A failed
batch adopts no controls, IDs, sequence, time or outcomes. There is no replay
acknowledgement protocol: producers handle a stale-sequence result explicitly.
All accepted controls are processed in order; a visual mailbox must not carry
these controls or terminal outcomes.

Start is admission time (`Now`) or explicit process-local monotonic `At(Instant)`.
A future start holds the captured current value; a past start samples its elapsed
position. Replacing a controller captures its current sampled value at admission,
emits Replaced for an unfinished predecessor, and optionally carries its physical
velocity into a new spring. Linear/eased/keyframe curves use their defined slope;
preserved velocity is consumed by spring curves only. No arbitrary user easing
callback runs during sampling.

Curves are Linear, finite unit-square CubicBezier, up to 256 ordered progress/
eased-progress Keyframes, and analytic under/critical/overdamped Spring. Keyframes
control interpolation progress, not arbitrary property values. Spring mass and
stiffness must be positive, damping nonnegative, finite natural frequency/damping
rates no larger than 1e6. Bezier inversion uses 40 bounded bisections; springs
do not numerically integrate missed ticks. Duration is positive and at most one
day. It is a hard end for springs: the exact destination is installed then,
regardless of convergence. Opacity clamps spring overshoot to its physical 0..1
range and exposes zero velocity while clamped; translation can overshoot while
final geometry remains finite.

Finish jumps to destination and emits Finished. Cancel supports Hold or Release;
Release of a property returns it to the latest authored value. Finished/cancelled
held controllers continue owning the value until Release or replacement, so an
unrelated UI update cannot reset an animation to a stale base. Held tracks count
toward max_tracks and retain their last ID. Removal of an unfinished target emits
TargetRemoved and deletes it; a new slot generation cannot inherit its controller.
Terminal results occur once. Destroying a runtime is a host-owned shutdown boundary;
drain/cancel accepted work before destruction if outcomes must be delivered.

## Scene, sample and input identity

`set_scene` accepts a complete newer source from the same authored tree and keeps
ongoing controller trajectories. It does not reconstruct controllers from UI base
values. New layout/content/resources appear in the next complete sample; removed
exact targets retire. Same source identity is a no-op. Monotonic time cannot move
backwards. Current sampling is atomic on invalid composed geometry; the host retains
its previous complete installed image.

`FramePacket::stream()` / `LocalFrame::stream()` separate submission ordering from
node/tree identity. Ordinary UI frames use their tree as stream. Each runtime gets
a distinct stream and private monotonically advancing sample revisions. WGPU
ordering, provenance and latest-frame mailboxes use stream/revision; node validation
and resource/geometry ownership continue to use authored tree/generation. Use one
runtime stream per consumer; do not interleave raw source frames with its samples.
Reset the renderer to recover GPU resources, rather than replacing the runtime.

An `AnimationFrame` retains both exact authored source and sampled output.
`input_scene(&source_scene)` requires exact source LocalFrame identity and shares
its immutable policy, routes, handlers and resource ownership, replacing only
geometry. `UiInputSceneId.presentation` adds an optional stream/sample revision,
while source_revision/tree/frame_revision retain their authored meanings. Router
adoption accepts ordered samples of the same source and rejects a backward sample
or raw unsampled reversion. Install a matching complete image first; this still
does not prove OS display completion. Delayed events retain their sampled scene
identity and current-owner generation/route/handler validation.
Wheel and focus-reveal route checks compare authored versions rather than the
optional presentation ID; their path/eligibility and current-offset checks remain.

## Bounds and costs

Default max_tracks=4,096, max_notices=8,192 and max_controls_per_batch=4,096.
Admission reserves room for one terminal result per unfinished track, in addition
to queued notices. If replacement plus its outcome would exhaust capacity, the
entire batch rejects and prior work continues. Sampling can finish all admitted
tracks without discarding outcomes, even while the UI is unavailable. Hosts drain
notices and provide their own bounded reliable delivery; no worker/timer/channel
or priority scheduler is created by this API.

Immutable topology/material resources are shared. Placement pages are copied only
where overridden; predecessor identity allows existing GPU spatial updates for
translation. Opacity changes and changed source payloads use ordinary full recovery;
skipped sample bases also recover from the latest complete output. Unchanged
placement output reuses the preceding LocalFrame and does not submit again.
The page directory is O(scene/64); changed-page comparison has up to 64 entries per
page, and complete geometry validation still walks the scene. Controls/source
adoption clone bounded controller/outcome tables to preserve rollback. Sampling
evaluates tracks for preparation and completion and allocates temporary overrides.
This is not constant-time animation or a measured FPS improvement.

PixelSnap::DevicePixels rejects: that output has baked authored translation/scroll
into projected geometry, so overriding it as if still authored would be wrong.
Use the default continuous logical geometry. Supporting snap-aware render sampling
requires a separate explicit projection contract; silent double projection is
not allowed. This does not change the existing ordinary pixel-snap UI path.

Follow-up: [split-owner host](RETAINED_SPLIT_PRESENTATION.md) uses
`activate(source, batch, now)` to stage source adoption plus its ordered animation
controls atomically. UiInputScene::frame exposes the exact immutable geometry to
that presentation owner; no mutable UI crosses threads.

## Runnable host and evidence boundaries

The ordinary owner example accepts `--animate`. Clicking the count button submits
one spring target, and repeated clicks redirect from current presentation. GPU and
CPU use the same sampled frame and projected input scene. The CPU host still has
one active plus one latest pending job and installs complete pictures. Its job is
not restarted every tick, but may display lower cadence. The example samples in
GPUI prepaint and requests animation frames only while running:

```powershell
cargo +1.98.1 run -p gpui-box-example-desktop --features retained-wgpu --locked --offline -- --retained-owner --animate
cargo +1.98.1 run -p gpui-box-example-desktop --locked --offline -- --retained-owner --animate --cpu
```

Native diagnostic modes can also take `--animate`; they settle each submitted
timeline before comparing the full framebuffer and then exercise the next event:

```powershell
target/debug/gpui-box-example-desktop.exe --retained-owner-check target/animation-native-gpu --animate
target/debug/gpui-box-example-desktop.exe --retained-owner-check target/animation-native-cpu --animate --cpu
```

Build with `--features native-check` for those diagnostics. Its explicit native
draw/readback and app-internal events are correctness evidence, not OS injection
or displayed cadence. The ordinary CPU Exact backend explicitly rejects box
shadows; the example does not attach shadows or silently request omission.

**Native final presentation remains GPUI callback-driven.** A worker control proves
sampling while the owner is parked; it does not prove a visible window keeps
animating during that stall. Independent native presentation, render-domain direct
scroll/drag, prepared pending scenes, reduced-motion host subscription, control
transport/acknowledgement and stable cross-language animation ABI remain open.
See [validation](performance/2026-11-19-animation/README.md). “99%” is figurative;
there is no numerical success-rate or p99 gate.
