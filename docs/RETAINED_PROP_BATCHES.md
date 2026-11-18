# Atomic component input batches

Status: experimental Rust framework contract,2026-11-18. Complete RFC remains
active. Extends [ReadProp](RETAINED_READ_PROPS.md) using the same scoped source
graph and ordinary atomic UI publication. Source schema remains6.

Follow-up: [typed components](RETAINED_TYPED_COMPONENTS.md) batches each keyed
site's aggregate ReadProp payloads before mounting new instances. Explicit
ComponentContext wires existing scoped state/bindings/effects/producers; same
key/type/host avoids remounting. Full component frontend remains active.

## Why a batch is part of the API

A child may require related inputs to be coherent. Updating a valid interval
from(10,20)to(30,40) one field at a time exposes(30,20)to derived validation.
That temporary tuple is not an intended component state. Independently settling
every keyed row input also repeats source-map copying and derived-graph scans.

`LogicalEdit::props` supplies a restricted `PropBatch` with only
`prop(scope,key,value)`. It returns the same ReadProp handles as the existing
single declaration. All heterogeneous declarations are collected before one
ordinary source settlement. No reading, computed creation, layout, mounting,
producer registration, effect or arbitrary LogicalEdit call is exposed inside
the batch. Consumers resume after it succeeds, with one settled final tuple.

```rust,ignore
let (lower, upper, label) = edit.props(|props| {
    let lower = props.prop(scope, 0, next.lower)?;
    let upper = props.prop(scope, 1, next.upper)?;
    let label = props.prop(scope, 2, next.label)?;
    Ok((lower, upper, label))
})?;
// Existing mount-created computations/bindings see all final inputs together.
// Newly mounted child code can now create derived inputs and bind outputs.
```

Allocate/reconcile scopes before opening the input batch. A parent can declare
all rows' inputs in one props call, then mount only new rows afterwards. This
reuses existing scope/key identity, read-only input handles and mount behavior;
it adds no list-specific layout or parallel component state store.

## Final values and errors

Repeated declarations use sequential typed equality, with the final value for
each input used for derived invalidation. A source restored to its **batch entry**
value does not reevaluate dependent computations. The shared source staging
helper now filters such restored roots for responsive and prop callers alike.
Dirty derived values still settle rank-first once each per batch, with ordinary
equality cutoff; only final successful values drive sinks and post-commit effects.
The single prop API retains its explicit immediate-checkpoint behavior.

Existing scope/runtime/type/kind checks apply. New inputs reserve monotonic
source generations and remain owned by a private pending scope-source map until
successful settlement. A declaration error poisons the batch: propagating it
returns the original error; catching it and returning Ok still returns
InvalidPropBatch. A caller can handle that batch error outside and continue the
same LogicalEdit. Arbitrary closure errors and fallible derived errors likewise
leave previous prepared values/counters and scope declarations unchanged.

A drop guard removes only newly allocated input sources and restores the
source free-slot/directory checkpoint on failure or caught panic, including
panic during derived settlement. Previous successful allocations/values in the
outer draft remain. Consumed generations are never restored: leaked failed
ReadProp identities cannot become a later input. External side effects and
PartialEq/Drop behavior remain the existing immutable/pure caller discipline.

After a batch succeeds, ordinary producer observations/order and final
source/scope/layout/frame checks still run. A late input write cannot bypass
earlier reader validation; final invalid render output rejects the complete
outer edit, including previously successful batches. This is an input savepoint,
not a second publication boundary or partial visible frame.

## Implementation and costs

PropBatch borrows the existing LogicalEdit and privately stores erased pending
values, newly allocated source identities/declarations and one allocation
checkpoint. SourceValues is internal; no caller-authored dirty hint or unchecked
Signal setter is exposed. No field is added to persistent source/node/layout/GPU
records and no vendor/kernel/renderer algorithm changes.

For K declarations in one call, prepared map copying and sorted derived scan
occur once rather than once per replacement. Restored-root comparison adds
one typed comparison per changed source. Batch scratch still includes a values
vector, new declaration map, allocation list, copied free slots and retired scope
set. Lookup/key validation and Rc allocations remain per declaration. Derived
scanning remains whole-graph, and ordinary planner/candidate/scope-order/frame
costs remain. Related input changes do not imply sublinear end-to-end work.
Structural work counts are verified; no wall-time/FPS/memory benefit is claimed.

## Evidence and remaining direction

See [verification and reproduction](performance/2026-11-18-prop-batches/README.md).
Controls cover coherent interval changes, restored values, declaration poisoning,
fallible/panicking derived work, creation/abort/unwind, outer validation rollback,
foreign/retired scopes and natural keyed lists. New and existing computations
share the same source machinery. CPU/GPU complete images and real Noto inputs
match independent full references; mount/key/frame lifetime remains intact.

Component mount contexts, typed Branch/Match/KeyedList sites, KeepAlive, macros,
Store/Command, component/source integration, incremental source scheduling,
writer/output ownership, full RenderBox, budgets and independent presentation
remain active. The batch is the input foundation for these APIs, not completion
of those features. "99%" stays figurative common fluid/intuitive behavior.
