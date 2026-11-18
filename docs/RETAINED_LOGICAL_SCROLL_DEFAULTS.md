# LogicalUi wheel and focus-reveal defaults

Status: experimental explicit Rust API, 2026-11-18. Extends
[scoped events](RETAINED_SCOPED_EVENTS.md). The complete RFC remains active.

`LogicalUi::apply_input_scroll(&effects)` applies default wheel scrolling and
keyboard/programmatic focus reveal from the existing `UiInputRouter`.
`edit_input_scroll(effects, sources, render)` joins application source writes,
component/layout producers and render edits with those defaults in one candidate.
`LogicalScrollCommit` returns the ordinary `ReactiveEditCommit` and per-wheel
consumed/remaining logical pixels after successful adoption.
`UiInputScene::node_id(render_node)` returns that exact publication's input
identity for programmatic focus without exposing render arena slots/generations.

```rust,ignore
let effects = router.route(input)?;
for event in &effects.dispatches {
    ui.dispatch_event(event)?;
}
let scroll = ui.apply_input_scroll(&effects)?;
let frame = ui.published_local_frame().expect("complete adopted frame").clone();
// Host installs the matching image, then adopts its input scene.
router.adopt(ui.input_scene()?, UiInputEvidence::ImageInstalled)?;
```

Callbacks and defaults are explicit separate transactions in this simple host
flow. Defaults validate the latest candidate after callbacks. A callback that
removes, reparents or disables the target causes stale defaults to reject, without
rolling back an already committed callback. `edit_input_scroll` instead provides
one boundary for coordinated source/render edits and default actions. It does not
dispatch the supplied callbacks, apply style feedback or install a native image.

## Shared algorithm and final geometry

The existing UiDocument wheel planner and post-layout nearest reveal algorithm
are now shared internal helpers. UiDocument keeps its ordinary exact-base batch
adapter; LogicalUi uses its existing source/component/layout candidate. There is
no second wheel or reveal solver and no temporary UiDocument tree.

The final candidate validates complete local geometry, builds its input scene,
then validates request runtime/epoch/tree, non-future revision, generation-safe
target eligibility and exact render ancestry. Merely moving a valid target does
not retarget an installed wheel position to another element. Disabled/hidden,
removed or reparented targets reject the entire default transaction. Invalid
positions/deltas reject with `Error::Input(UiInputError)`.

Wheel deltas accumulate from the **current candidate's clamped offsets and final
layout ranges**, including changes staged by the caller. Inner scroll hosts consume
each axis first; representable unconsumed distance chains to outer hosts, then is
returned for host behavior. Multiple requests in one call accumulate in order;
reusing old installed targeting for subsequent physical wheel events reads current
offsets rather than replaying an absolute old plan. Effects are not deduplicated
event IDs: the host submits each physical wheel once.

Focus reveal runs after final source/style/layout settlement and wheel offsets,
using current owner-local geometry before pixel projection. It scrolls the nearest
eligible ancestor only as far as needed, then propagates the actually visible
portion outward. Scroll and Hidden overflow allow reveal; only Scroll accepts
wheel. Clip/Visible do not acquire scrolling. Existing oversize-target, per-axis,
explicit clip and nested viewport behavior remains the same as UiDocument.
Repeated reveal of an already visible target does not request a visual frame.

## Application ownership and publication

A signal-bound `Property::scroll(node)` is application-controlled. Default wheel
and reveal skip that entire two-axis endpoint; residual wheel distance may chain
to an ancestor. New bindings and unbindings in this same candidate take effect
before defaults. The default never rewrites the source or silently detaches its
binding. Direct property writes are an initial offset for this transaction; bind
the endpoint when the application intends continued exclusive control.

The candidate owns offset mutations, complete LocalFrame extraction, route
diagnostics and source adoption. Final layout/geometry/request failure discards
them together and runs no reactive effects. Effects run only after the final
logical maps and complete output are adopted. Nested scroll commits merge their
routes/decisions into the returned ordinary commit, retaining the final revision.
Old snapshots remain immutable and consumable after owner disposal.

Input publication accepts a revision-matching complete LocalFrame even when the
older resolved FramePacket cache is dirty. It still rejects pending layout,
topology/extraction, geometry changes or mismatched frame revision. This fixes
sequential local property/scroll updates that otherwise rejected a second input
despite a current local publication.

## Costs and remaining work

The later [style-feedback stage](RETAINED_LOGICAL_STYLE_FEEDBACK.md) adds
apply_input_effects/edit_input_effects to join ordinary interaction-state feedback
with these same defaults. Appearance-only StyleResolution bindings preserve scroll;
the full style endpoint is application-controlled. Explicit scroll APIs here retain
their behavior and do not apply feedback by themselves.

This API reuses current host constraints/viewport and requires an existing complete
input publication. It builds the candidate's full input graph/maps and scans bound
scroll endpoints. The sparse event registry is cloned for final policy validation;
ordinary source/producer/candidate/layout costs remain. Reveal can publish geometry
for multiple ordered targets; no FPS, latency or memory improvement is claimed.

A combined callback/feedback/defaults host pump, prevent-default semantics,
semantics/accessibility, IME,
editing, multipointer, native focus/capture and OS host binding remain open.
UiDocument and LogicalUi retain separate source models. No Taffy solver, GPU format
or source schema 6 change. `Error` adds `Input(UiInputError)`.

See [verification and reproduction](performance/2026-11-18-logical-scroll/README.md).
“99%” remains a description of common fluid/intuitive behavior, not a numerical
success ratio or p99 acceptance gate.
