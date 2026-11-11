# Unified keyed scope and reactive lifetimes

`LogicalUi::edit` now combines keyed scope reconciliation, named writable and
computed state, effect registration/retirement, bindings, render construction,
topology and property changes behind the same validation/publication boundary.
This extends [render edit transactions](RETAINED_EDIT_TRANSACTIONS.md); it uses
the same source-settlement phase followed by an owner-local construction phase.

`LogicalEdit::reconcile_keys(parent, keys)` returns the final ordered child scope
IDs. Existing keys retain their scope, state, effects and render identities; new
keys reserve provisional scopes; missing keys retire their final logical
subtrees. Within the same draft, callers can initialize new scopes through
`state`, `computed`, `effect`, `append` and `bind`, or move surviving portal output
out of a retired host. The final sibling output is sorted by logical preorder,
retaining insertion order for output of the same scope and explicit portal hosts.
Repeated reconciliation follows the current draft lists: removing then
reintroducing a key constructs a new lifetime. There is no logical reparent API.

```rust,ignore
let result = ui.edit(
    |sources| {
        sources.set(theme, next_theme)?;
        Ok(())
    },
    |(), draft| {
        let scopes = draft.reconcile_keys(parent, &[10, 20])?;
        for scope in scopes {
            let color = draft.state(scope, 0, || 0x4488ffffu32)?;
            let material = draft.computed(
                scope, 1, &[color.dependency(), theme.dependency()],
                move |inputs| Ok(*inputs.read(color)? ^ *inputs.read(theme)?),
            )?;
            // Reuse existing outputs explicitly when retaining an existing key.
            let node = draft.append(scope, render_host, style.clone(), 0)?;
            draft.bind(material, Property::color(node))?;
            draft.effect(scope, 0, &[material.dependency()], move |inputs| {
                observe(*inputs.read(material).expect("declared input"));
                None
            })?;
        }
        Ok(())
    },
)?;
```

## Draft isolation and retirement

Sources, computations and effects remain in sparse owned maps until render and
lifetime preflight succeeds. New computations read settled existing source
values and previously constructed draft sources. Static dependency registration
only references existing identities, so new edges cannot introduce a cycle.
Named state/effect key reuse validates type/dependency identities and retains the
original initializer/callback. Pure initializer/computation errors leave all
published output intact. Initializers/computations must avoid external side
effects because arbitrary callback effects cannot be rolled back.

Whole logical subtree retirement collects every source/effect/output before
publication. Surviving existing or newly created computations/effects may not
depend on a retired source. A surviving foreign-owned portal may not remain
under a retired render host; move it out or explicitly retire its output too.
Failures reject before cleanup, source values, bindings, logical child lists or
Taffy change. Provisional bindings into automatically retired scope output are
cancelled; existing sinks into dead output/sources are skipped and detached.

After successful validation, source/effect dependency edges retire, surviving
draft sources and bindings install, source values publish, then logical scopes,
named state/effect maps and render-owner lists publish. Only after all those
owning maps are consistent do retired effect cleanups run, followed by new and
changed effect observations, deduplicated once per effect. Retired effects never
rerun for source writes in their removal event. Dependencies are disposed in
descending computation rank. Effect callbacks remain synchronous and read-only.
Returned cleanup counters include actual retired observer cleanups.

`ReactiveEdit` also exposes `signal`, `computed`, `effect`, `dispose_signal` and
`dispose_effect` for an owner without logical scopes. Disposal is validated as a
set, allowing a source and all its dependents to retire in one edit. Detaching a
source retires its sinks and makes endpoints available for replacement. Creating
then disposing a source/effect in one draft publishes no lifetime or initial
callback. Normal `ReactiveUi::try_signal` returns generation-exhaustion errors;
the compatibility `signal` API retains its infallible shape and panics only on
that exhausted lifetime space.

Failed/cancelled scope/source/effect handles cannot become valid in later
allocations. Scope and source generation tokens are monotonically reserved per
owner; binding/effect serials are likewise never reused. Aborted reservations
consume tokens only. Free lists are copied lazily on first construction; retired
live slots become reusable only after publication. Pending background senders
remain live after failed removal, expire after successful source retirement and
cannot target replacement sources. Old queued values drain as retired input.

Only changed render output advances the render revision and requests a frame.
Creating/discarding zero-output scopes, equal state reuse or cancelled entire
lifetimes can publish logical/reactive ownership while keeping frame items
shared and the host idle. A changed mixed lifetime/render event requests one
frame and advances one render revision.

## Callback failure and current boundary

User callbacks are infallible at the API level; a panic after publication cannot
undo committed state. The logical wrapper defers all callbacks until its owning
maps publish, so a caught callback/cleanup panic leaves coherent scopes, state,
bindings and render ownership. A panic can prevent remaining callbacks from
running; no panic recovery/retry scheduler is implied. Returned error rollback
and post-commit panic semantics are separate contracts.

Source writes settle in the first closure; newly constructed sources initialize
from values in the second closure. This API does not expose arbitrary writes to
new sources after downstream computations have already been constructed. Use
the final initial value, or a subsequent event, rather than expecting dynamic
reactive settlement during construction. Explicit dependency sets and keyed
lifetimes remain static. General asynchronous effects, dynamic dependencies,
event/focus/semantics lifetime and legacy islands remain in the full RFC goal.

## Verification and performance scope

Core cases exercise mixed keyed construction/retirement, cross-scope DAG/effect
cleanup order, cancelled lifetimes, failed reservations and slot reuse, stable
key state/chunks/order, provisional binding cancellation, source/effect/revision
exhaustion, surviving/new dependency rejection, portal evacuation, zero-output
idle behavior, background sender expiry and caught post-commit callback panics.
The gallery's existing toggle now creates and destroys a nested keyed scope with
local state, a computed material, an effect/cleanup and bound visible output;
ordinary and hardware clicks verify the full lifecycle and one render revision.

Hardware scenarios compare initial keyed construction, failed replacement with
no submit/cleanup, successful mixed replacement with one cleanup and final
retirement against full renderer pixels. The new lifecycle benchmark replaces
1/16/128 scopes beside 512 stable render nodes. Each scope owns writable state,
a computed dependency on root state, an effect/cleanup and one bound output.
It compares sequential existing APIs with a single lifecycle edit, generates
one frame per event for both, checks final geometry/material/live counts/callback
counts, and records commit and commit-plus-frame samples in three rotating
rounds (ten warmups plus forty measured events per mode/round).

This remains sparse draft construction with full logical rank, render topology,
binding retirement and frame extraction scans. It establishes atomic lifecycle
semantics, not a sublinear update or throughput improvement. Native-surface,
cadence, input-to-present latency and energy qualification remain open.
[Frozen results](performance/2026-11-12-lifecycle/README.md) include commands,
source hashes, raw rotating-round samples and hardware lifecycle pixels.
The [repeated-scan follow-up](performance/2026-11-12-lifecycle-scans/README.md)
marks pending topology patches, indexes new binding endpoints and uses per-scope
ordered ranges for draft retirement. Saved old/new executables were compared
in A/B/B/A order; full validation and extraction remain, and host-load drift is
reported alongside the observed CPU improvement.
