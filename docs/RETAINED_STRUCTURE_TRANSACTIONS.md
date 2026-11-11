# Atomic existing-tree topology and portal moves

`StructureTransaction` supplies final complete child lists for affected render
parents. `UiTree::commit_structure` and `ReactiveUi::commit_structure` validate
the final topology before touching either retained state or persistent Taffy:
every handle belongs to the live tree/generation, each non-root has exactly one
parent, the root has none, all live nodes remain root-reachable, and measured
leaves acquire no children. Cycles, duplicate/missing parents, disconnected
subtrees, foreign/stale IDs and revision overflow reject the complete batch.

All changed parents detach before final linking, allowing graph rotations that
would create a transient cycle if applied as independent moves. The owning
Taffy IDs are already validated; a Taffy error at that point is an internal
invariant failure. Returned user validation errors publish nothing. The batch
advances one render revision and emits `ReconcileStructure` with
`RenderTopologyChanged`. It retains NodeIds, Taffy IDs, intrinsic snapshots,
paint commands, resources and old immutable FramePackets. Final-equal lists
produce no revision/frame/layout work. Layout and extraction still run through
the existing full structure-change path after a changed commit.

`LogicalStructureTransaction` combines existing-sibling scope permutations with
portal render moves. `reorder(parent, scopes)` requires exactly the existing
scope set; `reparent(node, host)` changes render parentage without transferring
logical scope, named sources, computed edges, effects or bindings. Final host
lists are sorted by final logical preorder. Equal-rank siblings retain their
current order; moved same-owner output is appended to its new host before the
stable sort. No index-placement API is implied. The combined render batch must
validate before logical child lists publish. `reparent_render` is the single
move convenience API. `sync_render_order` and reorder-only `reconcile_keys`
now use one atomic render commit across every affected host.

The removal follow-up adds `StructureTransaction::remove(node)` for current
subtree disposal. Duplicate/overlapping roots coalesce. Omitted parent lists
retain surviving children; explicit lists cannot include a removed node, and
reparenting cannot rescue a descendant from a removed current subtree. Reactive
commit detaches dead bindings after the validated topology publishes. Mixed key
reconciliation now prepares removal plus final surviving host order before
running any effect cleanup. It then publishes one render revision, releases
removed dependencies, advances scope generations and creates empty scopes for
new keys. Surviving source/effect dependencies and portal hosts still reject
before preparation/cleanup. Cleanups remain infallible and non-reentrant.

The gallery's normal "Move retained portal" button uses the same logical API.
Its output retains a named color source and owner while moving between two
visible hosts. Ordinary and hardware click tests check one revision, changed
geometry, shared commands, stable ownership, actual changed GPU pixels and
equality with fresh full rendering. Hardware scenarios additionally combine
scope reorder and two portal moves, reject a cycle without publishing its valid
logical permutation, evacuate surviving outputs, retire the former host scope,
and update retained color/inherited translation afterwards.

## Performance and remaining scope

Validation uses slot-indexed temporary arrays for parent/reachability checks.
It scans all arena slots and live edges; logical planning also scans scopes and
hosts. This is an atomicity/identity implementation, not sublinear structural
work. The benchmark compares sequential per-host reorder, one existing property
transaction containing every reorder, and the general final-topology batch.
All produce the same final node order and zero command recording; the first
advances one revision per changed host and the others advance one. Three rotating
rounds contain ten warmups plus one hundred measured samples per mode/round.
Timing includes transaction construction/list clones/commit, excludes layout,
extraction and GPU. [Frozen evidence](performance/2026-11-12-structure/README.md)
records samples and hardware validation without attributing unrelated controls.
[Removal evidence](performance/2026-11-12-structure-removal/README.md) freezes the
subsequent mixed-key retirement cases and regenerated timing controls.

This API operates on existing nodes and existing sibling scope sets. Render
construction, source registration and property writes are not combined into
this transaction. Mixed key removal/insertion now uses one preflighted render
commit; new scopes start empty and later output construction is still explicit.
Atomic construction/property/lifecycle batches, event/focus/semantics lifetime, legacy
islands, indexed planning and other RFC requirements remain in the
[active goal](RETAINED_IMPLEMENTATION_PROGRESS.md).
