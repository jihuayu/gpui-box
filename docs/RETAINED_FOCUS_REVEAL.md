# Atomic post-layout scroll into view and focus reveal

Status: experimental Rust contract, 2026-11-18. Extends
[adopted-frame input](RETAINED_UI_INPUT_ROUTING.md),
[wheel planning](RETAINED_WHEEL_SCROLLING.md) and
[automatic viewports](RETAINED_OVERFLOW_VIEWPORTS.md).
The complete RFC and native common-interaction p99 acceptance remain active.

## Public entry and defaults

`UiOperation::ScrollIntoView { node }` is a post-layout intent in an ordinary
UiBatch. It minimally reveals that node's border box through unowned Hidden or
Scroll ancestors, nearest first and independently on each physical axis. It does
not assign focus. Direct requests can name any enabled/displayed node, including
one created/reparented in the same batch. Final hidden/disabled/missing targets
reject the complete publication with IneligibleReveal.

Tab/Shift-Tab and explicit `UiInput::Focus(Some(id))` additionally emit owned
UiFocusReveal records with adopted scene/evidence, target and target-first render
route. `plan_input_scroll` handles these alongside wheel deltas and StyleState
feedback. It checks runtime/epoch/tree/nonfuture revision, current focusability
and identical ancestry, then adds ScrollIntoView to the exact-base batch.
Pointer focus, Focus(None), adoption and activation keys add no reveal request.
An explicit focus request can reveal an already-focused node after content
changes. Repeated visible requests do not request a new visual frame. Hosts can
omit focus_reveals for a prevent-scroll policy.

```rust,ignore
let effects = router.route(UiInput::Tab { backwards: false })?;
let plan = document.plan_input_scroll(&effects)?;
let output = document.apply(plan.batch, resources, capabilities, constraints, viewport)?;
// Install the complete output, then adopt its matching input scene.
let adoption = router.adopt(output.input, UiInputEvidence::ImageInstalled)?;
```

Hosts using only effects.style_operations still receive only style operations;
use the planner for scrolling/reveal. Process events/effects once and in order.
Router focus changes when routing: no event-ack protocol or router/source two-way
rollback is introduced. If apply fails the host must handle that failure and
reconcile ordered interaction state. New hit geometry appears only on adoption,
never from router prediction. Evidence labels do not prove display.

## Final-layout and nearest semantics

Ordinary source setters first settle final values, topology, styles, resources
and layout. Reveal intents then run in original relative order, each against the
current private candidate after preceding reveals. Their location among ordinary
setters does not change this post-layout phase. Focus styles can change height
or wrap width in the same batch; reveal reads that final layout. Explicit Scroll
values settle before all reveals. Snapshot recovery stores resulting offsets
without reexecuting the intent.

Leave a fully visible target unchanged; also leave an oversized target spanning
the viewport unchanged. Otherwise move the nearer edge into the viewport and
clamp to the actual maximum. Carry only the resulting visible portion outward.
Oversized/extent-limited targets can remain partly visible. The viewport is the
automatic padding/gutter box intersected with the host's explicit clip. Clip and
Visible axes do not scroll. Style-owned Scroll (including Initial) is untouched;
a fixed clipping ancestor cannot be bypassed by scrolling an outer host toward
invisible content. No scroll-margin/padding, smooth animation or alignment modes.

Geometry reads current owner-local logical layout/translation/scroll using the
extractor's root-to-leaf f32 order and common OverflowClip helper. Pixel-snapped
output has baked inherited clips/offsets, possibly an empty clip for an offscreen
target, so it cannot provide reveal geometry. Final output retains its existing
pixel policy. No public Taffy type, new solver or extra compositor truth is added.
Transforms do not enlarge layout extents. Shadow/decoration ink is not the target
rectangle; independent occluding siblings are not visibility-tested.

## Atomic publication and compatibility

All work runs inside the ordinary private TreeEdit candidate. Per-target commits
use existing property routes and complete-frame geometry checks. After all
succeed, tree, authored/effective host offsets, style cache, source revision,
resources and input scene adopt together. Failure preserves previous source/frame;
provisional identity reservations retain their existing non-reuse boundary.
Diagnostics merge routes/decisions and count unique affected nodes. Source accepts
one batch revision/sequence; private render revisions may advance more than once.
Missing-base consumers retain ordinary full resync. No intermediate candidate is
sent to host or renderer.

Narrow offsets update authored/effective records only for hosts without any
authored StyleDeclaration::Scroll. Other properties/styles stay intact. Exact
replay returns the accepted result without rerunning reveal; stale batches reject.

UI_SCHEMA_VERSION advances4→5 for ScrollIntoView; schemas1/2/3/4 reject and
constructors select5. UiInputEffects gains focus_reveals. Exhaustive operation
matches/Effects literals need migration. These remain experimental Rust records
without a stable wire/foreign ABI. Taffy, GPU records and renderer algorithms do
not change in this stage.

## Costs and remaining acceptance

Without reveal, ordinary staging retains its existing path. With reveal, an
inverse identity map is built once; each target allocates an ancestry/geometry
path and scans ancestor styles for Scroll ownership. A changed offset commits a
transaction and obtains/validates a complete LocalFrame; an already-visible
request skips that empty commit and second validation. Multiple requests retain normal candidate,
layout/frame/resource costs. No sublinear total transaction, timing, memory or
FPS gain is claimed. Fixed clips/extents/occluders can prevent visibility.

Open: resize/content anchoring, caret/range reveal, margins, smooth scrolling,
keyboard arrow/page scroll, scrollbar chrome, focus traps/OS focus, IME/semantics,
event acknowledgements, independent scrolling under owner stalls and native
input/presentation p99. CPU complete-frame fluency, desktop effects and sustained
60/120Hz are separate gates. See [verification](performance/2026-11-18-focus-reveal/README.md).
