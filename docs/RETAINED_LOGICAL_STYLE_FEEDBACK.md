# Component interaction inputs and resolved style bindings

Status: experimental explicit Rust API, 2026-11-18. Extends
[scoped events](RETAINED_SCOPED_EVENTS.md),
[default scrolling](RETAINED_LOGICAL_SCROLL_DEFAULTS.md) and
[typed style composition](RETAINED_TYPED_STYLE_COMPOSITION.md).
The complete RFC remains active.

## One state and style path

`LogicalEdit::interaction_state(scope, key, node, disabled, selected)` and its
ComponentContext wrapper register an ordinary scope-owned `ReadProp<StyleState>`.
The node must belong to the scope; a source can serve one node, and a node retains
the same source identity. The key shares the scope's ordinary source namespace.
Redeclaration preserves router-owned hovered/pressed/focus-visible values and
updates caller-owned disabled/selected values. Style disabled remains separate
from the semantic `UiInteraction::disabled` policy.

Components consume this read-only source in existing computed values, producers
or effects. `StylePlan::resolve_values` is now public for hot-path resolution
without winner diagnostics; the existing `resolve` continues providing winners.
These are the same pure condition, override, validation and equality rules as
UiDocument. There is no second style cascade or ambient component execution.

```rust,ignore
let node = cx.append_layout(base_layout.clone(), 0)?;
let state = cx.interaction_state(0, node, false, false)?;
let output = cx.computed(1, &[state.dependency()], move |inputs| {
    plan.resolve_values(
        &base_layout, &base_properties, &environment,
        *inputs.read_prop(state)?,
    ).map_err(|_| Error::InvalidStyle)
})?;
cx.bind(output, Property::style_resolution_without_scroll(node))?;
```

Base layout/properties and environment are explicit caller inputs; applications
can observe their own ReadProps/Store selectors for dynamic bases and environment.
Resolved values are never used as the next authored base. This stage adds no
theme inheritance or automatic global environment injection.

## Property ownership

`Property::style_resolution` writes the complete effective layout and common
visual properties: color, translation, clip, scroll, group opacity, decoration and
shadows. It conflicts with every individual binding it covers, in either binding
order, both immediate and draft paths. Unbind may release the endpoint in the same
edit. Text/image/measure/parent-layout content remains independently bindable.
Layout dependency tracking includes this complete style endpoint, so producer and
responsive ordering cannot treat a width-changing source as paint-only.

`Property::style_resolution_without_scroll` writes the same appearance/layout
while leaving scroll untouched. Its StyleResolution.scroll value is deliberately
not applied. Use this when default wheel/reveal should own scrolling, or bind a
separate scroll source. The full endpoint remains application-controlled for
default scrolling. Two style endpoints on one node still conflict. The full
endpoint and an independent scroll binding conflict; appearance-only plus a
scroll binding can coexist. Conflicts are checked through bounded field lookups,
without scanning all live bindings for each ordinary bind.

Direct property writes remain an explicit low-level escape; the source writer
applies the whole effective output on its next change. Use ordinary bindings and
compose complete rules to avoid competing imperative writers.

## Router feedback and complete adoption

`apply_input_effects` applies style feedback plus existing wheel/reveal defaults.
`edit_input_effects(effects, sources, render)` joins application source/render
edits in the same candidate. Callback dispatch remains separate and explicit;
these methods do not invoke `effects.dispatches` or install native images.

Feedback is matched by runtime/epoch/tree, non-future publication revision and
generation-safe node identity. Foreign/future/retired feedback is ignored. The
latest supplied record per live node wins. The candidate preserves disabled/
selected, stages the three router booleans together, then uses ordinary computed
equality and producer settlement. Before staging it projects current render edits
to check ancestor display/disabled eligibility. Ineligible nodes receive cleared
router booleans; reparenting keeps the same live identity and router adoption
subsequently refreshes hover. UiStyleFeedback has no ancestry route or unique event
serial: hosts must deliver feedback in router order, not replay older states over
newer feedback.

The supplied render closure runs before this final feedback staging so its policy,
display and caller flag edits participate in eligibility. Reads inside that
closure see the previously adopted router state; feedback-dependent construction
belongs in ordinary producers, which run after the input batch settles. New
registrations can consume matching feedback already captured for the same live
node; the API does not replay unrelated historical pointer positions.

All feedback source values settle before layout producers. Wheel/reveal then use
the final candidate, including any feedback-driven width, padding or text wrap.
Failed propagation, layout, final frame or default-route validation discards the
feedback and output together. Ordinary effects run after full adoption. No source
setter escapes through the returned ReadProp; a compiler control enforces this
at the ReactiveTransaction API boundary. Explicit `prop` remains an owner-level
input writer and must not be used to counterfeit router state.

The sparse event registry stores node→scope/source registrations. Successful
scope/node retirement drops the registration alongside existing callback/policy
cleanup; ordinary scope source disposal owns input lifetime. Manual reconciliation
uses the same registry retirement. No field is added to every render node/scope.
Old complete frames remain immutable and consumable after owner disposal.

## Costs and unfinished boundaries

Feedback application constructs current scene maps, projects a disposable render
candidate for eligibility and runs ordinary source/computed/producer/candidate
validation. Registry mutations copy maps and registration alias checks scan the
registered entries. Full style binding creates patches for all covered fields;
normal value comparison selects P0/P1/P2/layout as appropriate. Even equal output
can cost source propagation and allocation. No FPS/latency/memory gain is claimed.

The simple explicit host still dispatches callbacks, applies input effects, installs
the matching complete image, then adopts its input scene. A combined native host
pump, propagation/prevent-default, cross-thread presentation feedback, IME/editing,
multipointer, semantics/accessibility, full source-model unification, task scopes,
KeepAlive, hooks/macros and theme inheritance remain open. Source schema stays 6;
no Taffy solver, GPU format or renderer algorithm changes.

See [verification and reproduction](performance/2026-11-18-logical-feedback/README.md).
“99%” describes common fluid/intuitive behavior, not a numerical success ratio
or p99 gate.
