# Live read-only component inputs

Status: experimental Rust framework capability,2026-11-18. The complete RFC
remains active. This builds on [Content producers](RETAINED_CONTENT_PRODUCERS.md)
and the existing scoped source/computed/binding graph. It implements explicit
live input handles, not the complete component/Branch/Match/KeepAlive frontend.

## Public contract

`reactive::ReadProp<T>` (also re-exported from logical) is a copyable owner-local
read-only source identity. A prop is a **live input**, while ordinary T passed to
a mount function is a snapshot. It has no public constructor, Signal conversion,
set/dispose/update-sender capability or independent lifetime ownership. Phantom
Rc makes the handle non-Send/non-Sync even if T is Send. Generation and runtime
checks use the ordinary source identity; disposed input returns InvalidSignal.

`LogicalEdit::prop(scope,key,value)` creates or replaces a keyed input in that
scope. Repeating the same declaration preserves the handle, replaces the value
through ordinary typed equality and settles affected derived sources. The
writer is the logical draft's declaration capability, not the ReadProp handle.
The same `(scope,key)` source namespace rejects different T, state/computed
versus prop, and foreign/retired scopes. Source generations consumed by failed
drafts are not rebound by later successful drafts.

Read through `LogicalUi::read_prop`, `LogicalEdit::read_prop`,
`ReactiveUi::read_prop`, `ReactiveTransaction::read_prop` or
`ComputedInputs::read_prop` (including EffectInputs). `prop.dependency()` supplies
the ordinary declared computed/effect input; undeclared reads reject. Producer
reads through LogicalEdit record the same dynamic source observations/index.
`LogicalEdit::bind_prop`, `LogicalUi::bind_prop` and `ReactiveUi::bind_prop` target
the same typed Property sinks as Signals. These are explicit context APIs, not
implicit TLS get() or a frozen foreign ABI. Source schema remains6.

```rust,ignore
let rows = edit.read(model)?.clone();
let scopes = edit.reconcile_keys(list_scope,
    &rows.iter().map(|r| r.id).collect::<Vec<_>>())?;
for (scope, row) in scopes.into_iter().zip(rows) {
    let row = edit.prop(scope, 0, row)?;
    if edit.renders(scope)?.is_empty() {
        // Mount once. Computation/event code captures the live handle rather
        // than the first Row payload. Local state retains this scope's identity.
        mount_row(edit, scope, host, row)?;
    }
}
// In mount_row:
let color = edit.computed(scope, 1, &[row.dependency()], move |inputs| {
    Ok(inputs.read_prop(row)?.color)
})?;
edit.bind(color, Property::color(node))?;
// An event callback with the owner context reads its current adopted UI input:
let latest = ui.read_prop(row)?;
```

Large payloads may use Arc/immutable resource handles. ReadProp itself does not
clone T on reads. Interior mutability or a dishonest PartialEq can still violate
ordinary immutable-value/equality discipline; no deep-freeze or purity guarantee
is added.

## Settlement, ordering and atomicity

Prop writes factor the existing responsive LayoutStyle source staging into a
shared internal erased-value entry. Both paths use a temporary PreparedUpdate,
typed equality, final restore coalescing, rank-ordered derived settlement and
adopt the temporary values/counters only on success. Newly created computations
participate before their live reverse edges exist. There is no second prop graph
or direct mutation of committed Rc payloads.

Multiple prop writes inside one draft can cause multiple pure derived evaluations
at explicit checkpoints, just like responsive stages. Only final values drive
property sinks/effects; post-commit observers execute once with a coherent final
tuple. On derivation error the individual prop replacement leaves its previous
prepared values/counters, allowing an explicit caller to handle it. Final render/
resource/geometry/source validation can still reject the entire outer edit.

The prop writer does not implicitly observe its old value as a producer read:
writing an input is different from depending on its previous payload. A consumer
explicitly reading it records the normal typed observation. Existing ancestry/
after/inferred layout edges order tasks; a later changed prop after an earlier
consumer read rejects at final source validation, without callback replay.
Parent-driven same-key updates and child producer consumption use that graph.
No output-writer inference or enforced prop writer exclusivity is claimed;
competing declarations require deliberate frontend ownership/order.

Same-key payload replacement retains local state, mount-created computations,
bindings, outputs and captured prop identity. Reordering keeps these identities.
Scope retirement disposes props with its other sources, bindings, derived values
and effects. A surviving external derived reader still prevents unsafe source
retirement with SignalInUse; the source is not silently kept alive or detached.
Current delayed events can read the latest payload; disposed/recreated instances
cannot be reached by an old prop generation. This does not implement the event
registry itself or redefine adopted-scene dispatch validity.

## Costs, evidence and remaining scope

See [verification and reproduction](performance/2026-11-18-read-props/README.md).
The handle adds no value allocation beyond an ordinary source; the owner-local
marker has zero payload size. Each input still uses source/scope table storage,
Rc value allocation, reverse dependency memberships and ordinary observations.
Changed input staging clones the prepared value map and scans/sorts all derived
sources, as the existing responsive stage did. Repeated per-row declarations may
therefore amplify work for dense lists. No batched prop transaction, incremental
source scheduler, output ownership or time/byte budget is claimed.

Controls compare same-key payload/geometry/color changes and reorder/removal to
ordinary independent full Flex CPU/GPU references; mounting occurs once per
surviving key. Direct bindings, persistent child reads, effects, error rollback,
new-source repeated writes, retirement, stale/foreign identity and compile-time
setter/thread restrictions are covered. Real Noto paragraph/color replacement
uses one mounted text binding inside Auto Grid and compares complete layout/
baseline and CPU/GPU images, retaining old frames after owners retire.

This is capability work with no timing/FPS/memory reduction claim. Typed component
mount contexts, Branch/Match/KeyedList sites, prop declaration batching, KeepAlive,
macros, Store/Command, component/source integration, full RenderBox/content
feedback, resource budgets and independent native presentation remain active.
"99%" stays figurative common fluid/intuitive interaction, not a numeric gate.
