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

This API operates on existing nodes and existing sibling scope sets. Render and
scope creation/removal, source registration/disposal, property writes and cleanup
are not combined into this transaction. Mixed key insertion/removal still uses
the earlier lifecycle path and can advance several internal revisions. Atomic
construction/removal/property batches, event/focus/semantics lifetime, legacy
islands, indexed planning and other RFC requirements remain in the
[active goal](RETAINED_IMPLEMENTATION_PROGRESS.md).
