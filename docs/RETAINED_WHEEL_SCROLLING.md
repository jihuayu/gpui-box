# Adopted-target wheel scrolling and ancestor chaining

Status: experimental Rust contract, 2026-11-18. Extends
[adopted-frame input](RETAINED_UI_INPUT_ROUTING.md) and the ordinary owned UI
batch/retained scroll property path. Full RFC remains active.

The [focus-reveal follow-up](RETAINED_FOCUS_REVEAL.md) adds focus_reveals to the
same planner, with schema5 post-layout ScrollIntoView. The wheel contract below
is unchanged; outcomes still correspond only to wheel events.

## Input, targeting and units

`UiInput::Wheel { position, delta }` carries logical scene coordinates and logical
pixel offset changes. Positive x/y increases the corresponding scroll offset.
The host converts native signs, physical scaling, line/page deltas and modifier
semantics. The GPUI integration fixture uses pixel_delta(line_height) then negates
the native displacement. No platform-wide line height, acceleration or natural
scroll setting is guessed by the framework.

Router validates position/delta before changing pointer state. Wheel updates
pointer hover using the explicitly adopted complete scene, without dispatching
a synthetic pointer-move, focus change, click or activation. Primary capture is
not the wheel target: hit geometry under the wheel position chooses the target.
Disabled foreground subtrees still occlude lower content and produce no eligible
wheel target; pass-through targeting keeps its existing children behavior.

Effects gain `wheel_scrolls: Vec<UiWheelScroll>` with scene/evidence, optional
target, target-first adopted render route, position and delta. No arbitrary
frontend callback runs and there is no cancelable DOM-style wheel handler yet.
No hit is retargeted against a newer tree during default scrolling.

## Complete scroll metadata

Each UiInputScene owns a sparse immutable map of UiScrollState for nodes whose
effective overflow has at least one Scroll axis. `scroll_state(id)` reports
effective clamped offset, maximum offset from the actual published layout and
wheel-enabled axes. It uses the same Taffy scroll_width/height values and node
offsets as extraction/local placement, rather than reconstructing extents from
an unrelated viewport or child count.

Only effective Overflow::Scroll axes accept wheel defaults. Overflow::Hidden
still permits ordinary programmatic UiOperation::Scroll but receives no wheel
delta. Visible/Clip are not wheel scrolling axes. A StyleDeclaration::Scroll in
the authored style composition owns that whole property; wheel defaults skip
both axes and leave the remaining delta for ancestors. Even Initial counts as a
declaration of ownership. This prevents silently writing an authored offset that
styles immediately override. Frontends wanting wheel-managed scroll leave that
property out of StyleRef and use the authored scroll field.

The follow-up [automatic overflow viewport](RETAINED_OVERFLOW_VIEWPORTS.md) clips
own content and descendants on non-Visible axes, preserving host border/shadow.
Explicit clips intersect that viewport; applications no longer need to author
one for an ordinary Scroll host. No scrollbar chrome is added.

## Planning and atomic application

`UiDocument::plan_input_scroll(&effects)` returns UiScrollPlan with an exact-base
ordinary UiBatch and one UiScrollOutcome per wheel event. The plan combines
existing style feedback with narrow `UiOperation::Scroll { node, value }`
updates; it preserves color, clip, opacity, translation, decoration, shadows,
layout and caller-owned selected/disabled feedback fields.

Targeting belongs to the adopted scene; offset arithmetic uses the **current
committed document**. Pending events do not predict hit geometry. For each event,
the planner validates runtime/epoch/tree/nonfuture revision, live enabled/visible
target and identical complete ancestry. Reparent/retirement/recovery rejects the
whole plan rather than retargeting or partially applying it. Geometry-only motion
and intervening scroll commits can retain that adopted route.

Walk the route target-to-root. Each enabled Scroll axis consumes as much signed
delta as fits its current clamped offset and current layout maximum; remaining
x/y components independently continue to ancestors. Per-plan temporary offsets
accumulate multiple events in order; separate successfully applied plans start
from the latest committed offsets even before host adoption catches up. Negative
delta unwinds the nearest container first. Boundary remainder is returned to the
host, with no implicit page navigation, bounce or overscroll effect.

Arithmetic adds in f64 before clamping finite f32 offsets, so huge finite deltas
do not overflow before clamping. Rounding cannot consume past the request and
reverse the residual passed to an ancestor. Actual represented movement is
reported separately from remaining delta; floating-point reporting is not exact
real-number conservation for arbitrary magnitudes.

```rust,ignore
let effects = router.route(UiInput::Wheel { position, delta })?;
let plan = document.plan_input_scroll(&effects)?;
let output = document.apply(plan.batch, resources, capabilities, constraints, viewport)?;
// After the host installs the complete output image/scene:
let adoption = router.adopt(output.input, UiInputEvidence::ImageInstalled)?;
// Process adoption feedback through ordinary ordered input effects as usual.
```

The planner does not mutate document/router or publish offsets. Apply the batch
once; existing exact base/sequence/replay checks prevent an already-built plan
from overwriting newer source state. A stale sequence can return ReplayConflict
when it matches the last accepted sequence with a different body. An exact batch
replay does not execute the delta twice. Rebuilding a new plan from an already
consumed event is a new request; hosts must process wheel effects once and in
order. No event sequence acknowledgement/deduplication protocol is added.

Offsets/feedback and complete frame still adopt through ordinary validation,
layout/property routing, resource lifetime and CPU/GPU publication. Invalid
scroll values/geometry/resources reject without partial document adoption.
At a boundary, an empty visual result keeps the prior frame revision and does
not request a new visual frame, though accepting an empty source batch advances
the source protocol revision/sequence. Host may choose not to apply empty plans.
Hover after motion changes only when the new complete scene is adopted.

## Source/API compatibility and costs

UI_SCHEMA_VERSION advances3→4 because Scroll is a new source operation. Old
schemas1/2/3 explicitly reject; constructors select4. UiInput/Effects gain fields/
variants, so exhaustive frontend matches/struct literals need migration. This
remains experimental Rust data, without a frozen wire or foreign ABI. No Taffy
algorithm, renderer format or extra mutable compositor truth is introduced.

Scroll metadata currently scans effective nodes and builds a new sparse map on
each accepted source publication, including empty batches. Old input scenes
retain their map/resources. Input hit testing remains linear; route planning
allocates paths, an offsets map, outcomes and operations and reads latest source
metadata. Normal document candidate/style validation and source/resource scans
remain. The narrow setter avoids unrelated field replacement but does not prove
sublinear total owner work, bounded latency or sustained FPS. No wall-time or
memory improvement is claimed in this capability stage.

## ADR and remaining scope

Decision: preserve adopted targeting while applying signed deltas to current
committed offsets in one ordinary batch. This prevents lost repeated wheel
movement while retaining visible hit geometry and exact-base transaction rules.
Explicit writer ownership and current extents choose safe chain behavior.

Alternatives: absolute offsets derived repeatedly from the adopted snapshot lose
deltas while publication/adoption lags. Hitting the live document targets content
that may not yet be installed. Predicting or independently mutating a router's
scene needs an acknowledgement/rebase protocol and a separate native compositor
adoption contract. Blindly replacing UiProperties can overwrite concurrent color/
clip/opacity edits. This implementation reuses the existing source truth instead.

Open: gesture/scroll latching, inertia/kinetic touch, elastic overscroll, per-axis
overscroll policy, cancelable wheel handlers, scrollbar/keyboard scroll, scroll
anchoring and advanced reveal policies, frame/event acknowledgements, renderer-domain direct
scrolling during owner stalls, affine presentation hits, native independent
presentation and representative input/presentation p99. Modern desktop effects,
CPU complete-frame fluency and sustained60/120Hz retain separate acceptance.
See [evidence and reproduction](performance/2026-11-18-wheel-scrolling/README.md).
