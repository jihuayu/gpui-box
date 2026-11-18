# Scope-owned reusable layout producers

Status: experimental Rust framework contract, 2026-11-18. The full RFC remains
active. This builds on [once-only plans](RETAINED_LAYOUT_BUILD_PLAN.md) and
[atomic local edits](RETAINED_ATOMIC_LOCAL_EDITS.md), using their existing four
builder input/proof protocols and final source/lifecycle/frame adoption.

The [indexed invalidation follow-up](RETAINED_PRODUCER_INVALIDATION.md) now
reuses entire accepted plans for internally certified unchanged drafts. It uses
generation-safe reverse source reads and retains the complete planner whenever
there is a possible output/layout/registration change. The original per-callback
reuse contract below still applies on that complete path.

## Public API

`logical::LayoutProducer` owns an owner-local immutable `Rc<dyn Fn>` callback.
It receives `LayoutBuildInput` and `&mut LogicalEdit`, may create/reconcile keyed
content and register descendant producers, and returns `Result<(), Error>`.
It is reusable across transactions and must own its captures (`'static`); it
is not Send and is never invoked on a GPU/render worker or during Taffy probes.
It is distinct from an ephemeral borrowed `LayoutBuildPlan` callback.

`LogicalEdit::layout_producer(scope, key, host, kind, after, producer)` declares a
producer owned by `(ScopeId, key)` and returns `LayoutProducerId`. The host must
be render output owned by that scope (including the root scope's root host).
Exactly one producer claims a host. IDs contain runtime identity and a monotonic
serial; failed transactions consume serials, preventing leaked provisional IDs
from being rebound to another producer. No frozen wire/foreign ABI is added.

Clone an existing LayoutProducer handle for equal keyed redeclaration. Same
handle/host/kind/prerequisites retains the ID and observations. A different
handle or contract invalidates construction while retaining the registration ID.
This makes stale captures explicit; rebuilding a new closure on every parent
run deliberately invalidates the child. An already executed/active registration
cannot be replaced, removed or invalidated in the same edit. Equal redeclaration
remains allowed. `remove_layout_producer(scope,key)` detaches future construction
without destroying the existing content. `invalidate_layout_producer(id)` forces
the next requested construction even if inputs and tracked sources compare equal.

Register within an edit with captured frame constraints/viewport. Successful
nonempty registration retains that frame session for subsequent ordinary
`transaction`, background update drains, binding/append/reconcile/structure APIs
and frame requests. The producer session does not invent a responsive environment;
the existing optional LayoutEnvironment session still owns that metadata.
Removing the final producer drops its implicit frame session while preserving an
independent responsive session. Ordinary read-only layout queries do not run
construction. Stats report only the last successfully adopted edit.

```rust,ignore
let producer = LayoutProducer::new(move |input, edit| {
    let LayoutBuildInput::Inline(input) = input else { unreachable!() };
    let model = edit.read(items)?.clone(); // dynamically tracked signal read
    build_keyed_content(edit, owner, host, input, model)
});
ui.edit_local_frame(constraints, viewport, |_| Ok(()), |(), edit| {
    edit.layout_producer(owner, 0, host,
        LayoutBuildKind::InlineAllocation, &[], producer)?;
    Ok(())
})?;
// Later source writes and frame resize run eligible registered construction.
ui.transaction(|tx| tx.set(items, next_items))?;
```

## Scheduling and selective callback reuse

After the user render closure, before logical retirement/order/final publication,
the runtime puts all surviving producers into the once-only build plan. Current
render ancestry orders ancestors before descendants; explicit `after` IDs order
sibling/source prerequisites. Unknown/foreign IDs, cycles, duplicate hosts and
retroactive dependencies reject as ordinary Invalid/CyclicLayoutBuildPlan.
Ready ordering uses the persistent serial. Each producer has one execution
opportunity per transaction; no replay or numeric fixed point is used.

Before each task, the plan settles scope-owned responsive rules and previews
the current draft. New child registration joins the next graph validation. A
parent may remove a keyed child scope: pending producers owned by that scope
are retired before reading their now-removed hosts. Already executed producers
retain their original final observations even if a later task removes them;
late retirement cannot erase validation. Surviving prerequisites pointing to a
removed registration fail. Host removal without owner retirement fails rather
than silently deleting a surviving declaration. At most 1,024 distinct issued
producer tasks and 16,384 edges per current graph use the existing structural
limits; registration churn cannot evade the task limit by removing pending IDs.

All four builder kinds still calculate their actual input and prove their
dependency direction. Only the content callback can be reused when all hold:

- The incoming Full/Inline payload is equal to the prior successful input.
- Every dynamically observed source value remains equal by normal typed Signal
  equality (Rc identity first). Conditional branches replace the old read set.
- The current draft render revision equals the prior successful owner revision.
- No logical scope creation/retirement or changed logical child list is pending.
- Registration identity and contract have not invalidated the observation.

Render/logical changes conservatively rerun producers even when their numeric
layout input is equal. This prevents an imperative content edit or keyed scope
change from being hidden by callback reuse. It is intentionally coarse and can
rerun unrelated producers; there is no per-subtree output write ownership proof.
Equal final writes and equal computed output can reuse callbacks. Dynamic reads
are observed at the actual read point, so a later responsive source change
cannot be hidden by sampling only the end-of-callback value.

`edit.read` records successful Signal reads; keyed `edit.state`/`computed` access
also records the returned source conservatively. Internal writer validation is
not a semantic source read. Source observations own Rc values with generation
identity; old conditional dependencies disappear after successful reruns. A
surviving producer cannot retain a retired cross-scope source. Final source
checks run after responsive settlement and after scoped source retirement.

Captured arbitrary mutable cells, global state, clocks, resource internals and
OS state cannot be automatically observed. Callbacks must express changing
inputs through Signals/layout or replace/invalidate the producer explicitly.
External side effects/panic/shared memo warming are outside rollback, like the
existing once-only builders. No arbitrary user code runs during reuse checks.

## Atomicity, stats and costs

Registration map, source observations, frame session, stats, source values,
keyed identity/effects and complete frame adopt together only after all final
layout inputs/proofs/order/geometry validate. Failure leaves the live registry,
cached read values, stats and frame unchanged. Caught registration/removal/
invalidation errors poison the outer edit; catching them cannot publish partial
construction. No producer callback can start a nested plan; append descendants
through registration instead.

`LayoutProducerStats` exposes considered/callbacks_executed/callbacks_reused.
These are per successful edit callback decisions, not total source visits,
layout/preview work, bytes or time. The indexed follow-up adds plans_skipped,
source_candidates and planning_previews. Certified whole-plan reuse counts all
callbacks_reused and zero considered/previews; no builder inputs were evaluated.
Any related/uncertain update still enters the complete planner for all registered
producers: full DAG validation, repeated draft preview, existing builder
measurement/proof, final tracked layout and O(N) geometry validation still run.
Registry maps/observations clone on modification;
old Signal values and callback captures remain retained by registrations. There
is now a reverse source index, rebuilt on successful registry/observation
adoption. There is no global memory/time budget or guaranteed sublinear whole
owner update. See the follow-up's measured costs and limited timing scope.

## ADR and remaining scope

Decision: add reusable immutable producer identity and actual Signal read
observations above the existing once-only planner. Retain normal builder proof
and final validation even for reused callbacks; conservatively invalidate on
render/logical changes rather than guessing captured dependencies or writer
ownership. Keep UI-local callbacks separate from transferable scene outputs.

Alternatives: retaining FnOnce is impossible without moving to another owned
producer. Unconditionally invoking every callback would preserve construction
but lose useful same-value/conditional dependencies. Tracking only explicit
manual dependency lists is brittle for conditional reads. Replaying callbacks
to convergence changes lifecycle semantics and still cannot prove layout
independence. Fine-grained output dependency tracking needs a separate measured
contract and should not be inferred from input-width equality.

This provides persistent registration/automatic scheduling and selective content
callback reuse for the proven builder protocols. It does not discover arbitrary
cross-producer source writers: use after prerequisites, and final checks reject
late changes rather than replaying. Pure ParentLayout intrinsic/full-axis feedback,
complete RenderBox, general UI IR component integration/macros, native independent
presentation and representative input/presentation p99 remain open. CPU complete
frames and modern desktop effects/sustained 60/120Hz retain separate acceptance.
See [verification and reproduction](performance/2026-11-18-layout-producers/README.md).
