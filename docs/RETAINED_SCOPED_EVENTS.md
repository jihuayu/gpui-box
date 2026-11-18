# Scoped frontend handlers over the shared input router

Status: experimental explicit Rust component/runtime API, 2026-11-18. Complete
RFC stays active. Extends [scoped providers](RETAINED_SCOPED_PROVIDERS.md),
[typed commands](RETAINED_TYPED_COMMANDS.md) and the existing immutable
UiInputRouter. UiDocument keeps its existing separate source writer. This stage
connects LogicalUi components to the same input routing types/algorithm, without
claiming complete logical/source-model unification or native event installation.

## Registration, policy and complete geometry

`EventHandler::new` owns one UI-local immutable callback. LogicalEdit/ComponentContext
register it under scope/key and receive UiHandlerId. Cloning/redeclaring the same
handle retains ID; a different callback allocates a new process-unique monotonic
ID, including failed provisional declarations. An old ID is never rebound to
another callback. Handlers live in a sparse LogicalUi registry, with no callback
or map added to each render node or immutable FramePacket.

`interaction(node, Some(UiInteraction{...}))` associates existing pointer/focus/
activation policy with that scope's render output. All referenced handlers must
belong to the same scope; foreign/unowned references reject. None removes the
explicit policy. A callback replacement must update all policies still referring
to its old ID in the same edit, otherwise final validation rejects. Structural
scope/node retirement removes handlers and/or policies through the existing
candidate; failed adoption preserves the old registry. Legacy manual reconciliation
also clears retired scopes/nodes before returning.

```rust,ignore
let node = cx.append_layout(LayoutStyle::row(), 0xff0000ff)?;
let clicks = cx.state(0, || 0u32)?;
let save = cx.use_command::<SaveDocument>()?;
let document = cx.use_store::<DocumentStore>()?;
let handler = cx.event_handler(0, EventHandler::new(move |event, ui| {
    if event.kind == UiInputEventKind::Activate {
        let count = *ui.read(clicks)?;
        ui.set(clicks, count + 1)?;
        let snapshot = ui.read_prop(document)?;
        let command = SaveDocument {
            expected: snapshot.revision(),
            draft: snapshot.state().clone(),
        };
        let save = save.clone();
        ui.after_commit(move || {
            // Retain ticket in application task state or report Busy/Closed.
            let _result = save.send(command);
        });
    }
    Ok(())
}))?;
cx.interaction(node, Some(UiInteraction {
    focus: UiFocusPolicy::Tab,
    activate_handler: Some(handler),
    ..Default::default()
}))?;
```

`LogicalUi::input_scene()` captures policy, physical render ancestry, enabled /
visible state and exact complete geometry into the **existing** UiInputScene.
Normal hit filtering, capture, hover, Tab, Enter and Space behavior therefore use
the same UiInputRouter as UiDocument, rather than a component-specific click
algorithm. Host explicitly adopts that scene when installing its matching frame:

```rust,ignore
router.adopt(ui.input_scene()?, UiInputEvidence::ImageInstalled)?;
let down = router.route(UiInput::PrimaryDown(position))?;
for event in down.dispatches {
    ui.dispatch_event(&event)?;
}
let effects = router.route(UiInput::PrimaryUp(position))?;
for event in effects.dispatches {
    ui.dispatch_event(&event)?;
}
```

The scene uses generation-checked render IDs, a runtime/tree identity and monotonic
combined visual/policy revision. LocalFrame is reused when current; a current
complete FramePacket can use the existing LocalFrame conversion. Dirty or mismatched
geometry rejects instead of combining current policy with stale node positions.
This is explicit host adoption evidence, not proof of presentation/display.

## Dispatch and post-commit business work

`dispatch_event` first ignores a retired handler without invoking user code. For
live handlers it validates current complete scene identity, enabled/visible target
and current-target policy, render ancestry/route and handler identity through the
existing InputGraph acceptance check. A delayed click cannot invoke a remounted
instance or a replaced callback; reparenting/disable invalidates the old route.
Motion alone may retain the original valid event target instead of re-hit-testing
the position against a newer image. Dispatch values are trusted frontend routing
data, not cryptographic authorization or a wire security capability.

EventContext exposes ordinary source read/read_prop/read_selector/set plus
`after_commit`, without structural LogicalEdit, layout or general properties
access. Handler source writes use the existing transaction; persistent component/
layout producers settle afterward. Dispatch uses the existing `edit_local_frame`
with the current published image's constraints/viewport, so even a UI without a
persistent producer validates and adopts complete geometry before after_commit.
Each dispatch is a separate transaction, so a
later dispatch from the same router result rechecks any preceding structural
changes before running. Bubble/activation selection still belongs to the shared
router, not the registry.

Handler error or unwind discards tentative source writes and queued after_commit
closures. Final source/component/layout/geometry failure likewise preserves old
state/frame and runs no queued business work. Successful adoption runs ordinary
reactive effects, then the event's after_commit closures in order. These can send
owned Command payloads; Busy/Closed is handled as a business-admission outcome
after already committed UI state. Panics in post-commit effects cannot roll back
adopted UI or prior external effects. Arbitrary side effects directly in handlers
or captured interior mutable objects also remain outside rollback.

No callback crosses a thread/await or goes into GPU output. EventContext is a
borrowed synchronous transaction capability. A compiler control rejects obtaining
properties access from it. Async business tasks keep owned data/senders/tickets;
future UI results still need ordinary generation-checked delivery.

## Current boundaries and costs

This stage delivers scope-owned handler registration, interaction policy, complete
input scene construction, shared pointer/focus/key routing, delayed-event rejection,
source transactions and post-commit business actions. UiDocument still owns its
own IR tree; no automatic import/reconciliation between its writer and LogicalUi
is inferred.

Router style feedback, wheel-scroll/default focus-reveal effects are returned as
usual, but their **automatic application to LogicalUi** is not implemented here.
Existing UiDocument scroll/style planners remain separate. IME, selection, editing,
multi-pointer gestures, semantics/accessibility, automatic OS host binding and
task/KeepAlive activation policies also remain. Deactivation cleanup events whose
old target is no longer eligible can be ignored by live validation; handler scope
disposal/effect cleanup remains the lifetime cleanup authority. Native focus /
capture transitions still require host integration.

Registry mutations clone its maps; final lifetime validation scans policies and
handler references. Building and validating a current input scene walks the whole
render tree and allocates maps/tab order/scroll state; each dispatch currently
builds one again and linearly finds its handler. FramePacket-only conversion builds
the existing immutable LocalFrame topology. No input-time complexity/performance
improvement is claimed. Callbacks run on the UI owner with no preemptive budget.
See [controls and reproduction](performance/2026-11-18-scoped-events/README.md).

No layout solver/GPU format/source schema6 change; Error adds InvalidEventHandler.
Full RFC stays active. "99%" means common fluid/intuitive behavior, not a numerical
success ratio or p99 acceptance gate.
