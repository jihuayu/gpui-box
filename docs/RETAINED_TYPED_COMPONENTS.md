# Typed mount-once components over logical scopes

Status: experimental explicit Rust framework API,2026-11-18. Complete RFC stays
active. Extends [Content producers](RETAINED_CONTENT_PRODUCERS.md),
[ReadProp](RETAINED_READ_PROPS.md) and [input batches](RETAINED_PROP_BATCHES.md).
No new layout solver, renderer or source schema; UI input schema remains6.

## Component contract

`logical::Component` declares `type Props: PartialEq + 'static` and
`mount(ReadProp<Self::Props>, &mut ComponentContext) -> Result<(), Error>`.
`LogicalEdit::components::<C>(parent_scope, render_host, keyed_items)` declares
one homogeneous keyed component site. The site owns the parent's complete direct
logical child list. Keys are explicit u64 values, not inferred indices. Duplicate
keys reject before mount code. [Independent regions](RETAINED_COMPONENT_SITES.md)
now compose several such lists/selected types under one parent. Other frontend
key encodings remain separate work rather than being silently hashed into this API.

```rust,ignore
struct StatusRow;
impl Component for StatusRow {
    type Props = RowData;
    fn mount(props: ReadProp<RowData>, cx: &mut ComponentContext<'_, '_, '_, '_>)
        -> Result<(), Error>
    {
        let draft = cx.state(0, || String::new())?;
        let color = cx.computed(1, &[props.dependency()], move |i| {
            Ok(i.read_prop(props)?.color)
        })?;
        let node = cx.append_layout(LayoutStyle::row(), 0)?;
        cx.bind(color, Property::color(node))?;
        // Retain draft/props handles in owner-local handlers or producers.
        Ok(())
    }
}
// A Content producer can use this declaration without rerunning StatusRow::mount.
edit.components::<StatusRow>(site_scope, host,
    rows.into_iter().map(|row| (row.id, row)))?;
```

Mount initializes an instance once. Later same-key/type/host declarations replace
the existing live payload. Computed values, bindings and registered producers
update output without rerunning initialization. Plain values read during mount
remain snapshots; changing output must use ReadProp/Signal dependencies or a
producer. No arbitrary Rust closure capture analysis or implicit reactivity is
claimed.

ComponentContext is an explicit borrowed initialization capability with scoped
state/computed/effect keys, read/bind, owned LayoutStyle output, normal properties,
nested components and producer registration. `scope()`/`host()` provide identity
for deferred owner-local producers. It does not insert a layout/render wrapper,
use ambient TLS, cross await or transfer the mutable owner to a worker. A compile
test verifies a borrowed context cannot escape to a thread. Effects, including
mount side effects and cleanup, use the existing post-commit API.

The aggregate Props payload uses the reserved source key u64::MAX. Context state/
computed reject that key; other source keys remain local to the component. Effects
have their existing separate namespace. This experimental reservation is public
so generated frontend code can avoid it. Low-level LogicalEdit users must also
respect the scope's declared ownership.

## Identity, reconciliation and final state

Logical Scope adds an optional boxed identity containing Rust TypeId and render
host. ScopeId/key/generation remain the existing source/output lifetime truth.
Zero-output components therefore retain identity without inspecting render count.
Components can emit several sibling outputs, and logical child ownership stays
separate from their physical render parentage.

- Same key, type and host: retain scope, local state, payload handle, derived
  sources, effects, registrations and outputs; replace the payload.
- Reorder: retain instances and apply the ordinary logical-preorder output sort.
- New key: allocate a fresh scope/input, then run mount once.
- Removed key: retire the normal scope subtree, source graph/effects and pending
  producers/output. Re-entry after an accepted removal creates a new generation.
- Different component type at the same key: fresh scope and mount; the old type
  retires. Equal Props types do not imply component identity.
- Changed render host: currently fresh instance, not portal migration. Explicit
  lower-level reparenting keeps its separate existing contract.

All retained/new payloads are declared in one props batch before new mounts.
Final ordinary source settlement, lifecycle/ownership checks, layout and geometry
validation adopt together. A failed candidate keeps the original instances,
state/stats and frame. Already created effects do not run before successful
adoption. An Rc failure guard poisons the outer edit if construction returns an
error or unwinds, even if user code catches it. Validation runs before and after
automatic producer settlement; this is not a replay or partial component commit.
Public Error adds InvalidComponent; exhaustive matches need migration.

The site uses the same logical reconciliation path as manual scopes, with an
additional expected component identity to decide which keys can be retained.
Manual reconcile_keys keeps its existing untyped reuse semantics. Do not use
multiple independent writers for one site's child list. Typed site metadata does
not convert arbitrary manual scopes into components or enforce general writer
ownership. The follow-up [region API](RETAINED_COMPONENT_SITES.md) supplies stable
site placement and independently declared content, including explicit selected
component types/arm keys. Automatic Branch/Match factories and macros remain.

## Costs, verification and remaining scope

See [controls and reproduction](performance/2026-11-18-typed-components/README.md).
Tests cover payload/reorder/retirement, new type/host, zero/multiple outputs,
nested mount/Content producers, effect cleanup, failure poisoning and final
rollback. Independent numeric full Flex and real Noto references compare complete
CPU/GPU output and layout/baselines; old frames outlive their producing owners.

One optional pointer is added per logical Scope; component scopes allocate a
small boxed TypeId/host identity. Each declaration collects rows/keys, scans the
old direct children, stores retained identities and runs the ordinary props batch.
Reconciliation creates sorted maps/sets and may repeat scope retirement scans;
global logical output ordering and full candidate validation remain. New mounts
are ordinary owner-local Rust calls, with no preemptive time/byte budget. Existing
producer preview/DAG and source graph costs still apply. No timing/FPS or memory
improvement is claimed.

This delivers explicit typed initialization/keyed payload/lifetime wiring.
Independent regions are now explicit; automatic typed Branch/Match, heterogeneous
cross-site input batches, KeepAlive, use_* hooks/TLS, component/view macros, providers/Store/Command,
component/source input unification, event/semantics integration, output ownership,
full RenderBox, resource budgets and independent presentation remain active.
"99%" remains figurative common fluid/intuitive interaction, not a numeric gate.
