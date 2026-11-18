# Independent component regions

Status: experimental explicit Rust framework API,2026-11-18. Complete RFC remains
active. Extends [typed components](RETAINED_TYPED_COMPONENTS.md) with independently
addressable logical regions. Uses the existing scope/key/source/layout/render
lifecycle; source schema stays6. No new Flex solver or renderer algorithm.

## Region ownership and ordering

`LogicalEdit::component_sites(parent, keys)` declares the parent's **complete
ordered direct region list** and returns opaque `ComponentSite` handles in that
order. This is a structural declaration, not an append operation. Stable region
keys retain their scopes; omitted keys retire their normal scope subtrees.
Duplicate region keys reject before changes can adopt.

`LogicalEdit::components_at::<C>(site, host, items)` updates only that region's
complete homogeneous keyed content. It delegates to the ordinary typed component
API. No sibling region must be redeclared for a content-only update. Different
regions have independent item-key namespaces: row7 in a list and row7 in another
list never share state. Region keys and row keys are in different scopes, so no
reserved range or implicit hashing is needed.

```rust,ignore
// Declare placement once during initialization, or reconcile it when structure
// changes. Both regions render directly into host, with no extra Flex box.
let sites = edit.component_sites(owner, &[10, 20])?;
edit.components_at::<StatusRow>(sites[0], host,
    rows.into_iter().map(|row| (row.id, row)))?;
edit.components_at::<Summary>(sites[1], host, [(0, summary)])?;

// A later update may touch just the list. Summary retains state and output.
edit.components_at::<StatusRow>(sites[0], host, next_rows)?;

// Placement changes independently from content; both site handles survive.
assert_eq!(edit.component_sites(owner, &[20, 10])?, [sites[1], sites[0]]);
```

Regions add logical scopes but **no render node, source, producer or layout box**.
Normal logical preorder keeps each region's zero/multiple outputs together at
their explicit render hosts. Flex/Grid see ordinary physical children and use
the same layout engine. Existing portal/host rules remain: changing a component's
host remounts its identity; the region handle itself carries no host.

ComponentContext exposes `component_sites(keys)` and `components_at(site,host,
items)` during mount. The latter checks the site is a direct child of this
component. A borrowed context cannot write an unrelated component's region.
`ComponentSite::scope()` exposes identity for inspection and explicit low-level
operations; LogicalEdit remains a low-level owner capability, not a general
exclusive-writer proof. Do not combine multiple writers of one child list, or
mix `components(parent,...)` and `component_sites(parent,...)` expecting both
complete-list declarations to survive.

## Selected arms, retirement and transactions

Sites can contain different component types at different times. Explicit Rust
`if`/`match` inside a Content producer can choose which typed declaration to call:

```rust,ignore
match mode {
    Mode::Details(data) => { edit.components_at::<Details>(site, host, [(0, data)])?; }
    Mode::Error(data) => { edit.components_at::<ErrorPanel>(site, host, [(1, data)])?; }
    Mode::Empty => { edit.components_at::<Details>(site, host, [])?; }
}
```

Component TypeId and arm/item key both participate in instance identity. Give
different arms explicit different keys even if they use the same component type.
Changing arms unmounts the old instance. Re-entering it after retirement creates
fresh state; no implicit KeepAlive or state sharing by component name. Empty
content preserves the site scope while retiring its children. Removing the site
itself retires all descendants, sources/effects/producers and output through the
same final candidate. A stale/foreign handle cannot revive or alias a new region.
Existing manual/typed scopes with the same region key are replaced, not adopted
as a region: the existing optional identity now distinguishes Component and Site.

All operations use the component failure guard. Errors/unwind poison the outer
edit even when caught; final source/lifecycle/layout/geometry validation still
adopts one complete result. Failed candidates keep old sites, payloads, output,
effects and published frame. Effects/cleanup run after successful adoption.

Payload batching remains **per components_at call**. Independent regions can
have different Props types, but this does not provide an aggregate heterogeneous
cross-site input checkpoint: derived calculations must not rely on seeing all
later site declarations before they run. Related fields should remain one Props
payload or use the existing explicit input batch contract. The follow-up
[ComponentPlan](RETAINED_COMPONENT_PLANS.md) now supplies one shared checkpoint
for heterogeneous region declarations; separate components_at calls retain their
individual checkpoints. Arbitrary external mount side effects remain outside rollback.

Sites are not new scheduler hosts. Content registration still requires an owned
render host and one writer per host, with ancestry/explicit/inferred prerequisites
and final executed-order validation. A common parent producer can update several
regions under one physical host; nested component-owned hosts can have their own
producers. This change does not infer arbitrary callback outputs or install a
per-region scheduler. The follow-up [component plan](RETAINED_COMPONENT_PLANS.md)
adds automatic typed selection and heterogeneous input batching through one
ordinary Content producer. Component/view macros remain separate work.

## Costs, verification and remaining scope

See [controls and reproduction](performance/2026-11-18-component-sites/README.md).
Tests exercise separate regions with equal row keys, content-only updates,
independent region/list reorders, type/arm replacement, empty selection, stale
handles, context ownership, duplicate keys, final rollback and nested producer
retirement. Real Noto labels compare full LayoutResult/baseline, wrapping and
complete CPU/GPU output against independent numeric full Flex. Old frames remain
consumable after the UI/source/font owners retire.

Each region consumes an ordinary logical scope/slot plus its small boxed identity.
The optional per-scope pointer already exists; Component/Site are variants of that
record. Structural region reconciliation allocates the existing maps/sets/order;
each content call additionally validates handle identity and delegates to normal
component reconciliation/props/retirement. Full logical sorting, source/derived
scans, candidate copying, layout/geometry and producer/DAG work remain. No
wall-time, FPS, memory reduction or modern-GPU/native fluency claim follows.

The follow-up plan supplies typed automatic Branch/Match/list selection and
heterogeneous cross-site input batches. Remaining: KeepAlive, hooks/macros,
providers/Store/Command, source-model unification,
event/semantics integration, general writer/output ownership, complete RenderBox,
resource budgets and independent native presentation. The complete RFC remains
active. "99%" continues to mean ordinary fluid/intuitive interaction, not a
numerical success rate or p99 acceptance gate.
