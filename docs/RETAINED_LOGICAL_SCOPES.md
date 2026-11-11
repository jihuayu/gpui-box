# Persistent logical scopes and keyed render ownership

`retained::logical::LogicalUi` adds a logical lifetime above the persistent
render tree. `ScopeId` has its own runtime identity, slot and generation; it is
independent of the render `NodeId`. A scope may own no render output, several
siblings, or output in a different render host. The gallery now stores its
reactive state in scopes and exposes a normal-click keyed reorder example.

This is another stage of the [complete-runtime goal](RETAINED_IMPLEMENTATION_PROGRESS.md),
with [the source RFC](https://app.notion.com/p/415d83088d4e818290cddca2f104e65c)
as its design contract.

## Identity and lifetime

`reconcile_keys(parent, keys)` matches only direct children of the logical
parent. Reordering retains their scopes, typed named state, bindings, render
handles and existing local paint commands. Missing keys retire the entire
logical subtree. Inserted keys create empty scopes; constructing their render
output is explicit. Duplicate keys and foreign/stale handles are rejected.

`state(scope, key, init)` initializes once during the scope lifetime. Reusing
the key with a different Rust type is rejected. Removing a scope disposes its
owned signals and all their bindings, while a shared source owned by a surviving
ancestor remains alive. Render removal also detaches consumers of that output.
Reused scope/source slots have a different generation. Already published frames
keep their owned data after those live identities have retired.

Logical and render parentage are separate. `append_render` takes both an owner
scope and an explicit render parent, allowing fragments and portals. Logical
preorder controls sibling painting and hit order within each render host;
portal output stays in that host. `sync_render_order` explicitly synchronizes
new output; reconciliation also performs this synchronization. Existing trees
can be adopted into the root scope without replacing render handles.

Removing a scope that owns a render host used by a surviving scope returns
`ScopeInUse` before mutation. The caller must also remove the dependent owner
or keep the host alive. This prevents silent deletion of a surviving portal.
Scope removal releases its output even when that output is outside its logical
ancestor's render subtree.

## Verification

- 24 retained-WGPU core tests pass, including keyed identity/chunk reuse,
  named-state type checking, nested disposal, shared-source survival,
  duplicate/foreign handles and live portal host rejection.
- Both normal-click tests pass, including the explicitly enabled hardware
  test. The new gallery button reverses three keyed components and checks
  preserved identities and commands after the actual event/paint chain.
- The hardware suite passes 50 pixel scenarios on GTX 1650 Ti/Vulkan. Six new
  cases cover zero/multiple outputs, portals, transparent sibling reorder,
  retained state after reorder, portal retirement, generation reuse and 32
  queued reorders with at most three submissions in flight. Every final image
  matches the full GPUI reference. Updating one retained source after reorder
  uploads 160 instance bytes.

[Frozen evidence](performance/2026-11-11-logical/README.md) includes the complete
validation result, source hashes, device identity and six new images.

## Remaining boundaries

Reconciliation currently scans scopes/hosts and sorts affected render siblings;
it is not a sublinear structure-update implementation. Render insertion/removal
and host reorder may create multiple internal revisions. The host requests one
frame after its event, but a fully atomic structural transaction remains open.
There are no user callbacks during reconciliation; returned validation errors
for duplicate keys and dependent hosts are checked before mutation.

Events, focus, accessibility ownership, legacy islands and portal reparenting
are still pending. Scopes do not yet invoke arbitrary component setup/effects.
The retained primitive remains a rectangle, and the existing native GPU
presentation/resource limits still apply. This stage does not complete the
full RFC goal or establish a new performance speedup for keyed reconciliation.
