# Atomic construction, topology, sources and properties

`UiTree::edit` stages new render nodes, complete child lists, reparenting,
final-subtree removal and ordinary typed property writes. A sparse overlay owns
only touched node copies; the live Taffy tree and arena are not cloned. Final
topology, measured-leaf contracts, property inputs and revision availability
validate before allocating Taffy leaves or publishing changes. A changed edit
advances one revision and returns one frame intent. Equal final output, including
created-then-removed nodes, advances neither, even at the revision limit.

`TreeEdit` exposes `append`, `set_children`, `reparent`, `remove`, `children`,
`root` and `properties`. Complete child lists can stage graph rotations; the
final graph must contain each surviving non-root exactly once and be reachable
from the root. `reparent` resolves current draft child lists rather than stale
committed parent links. Pending `Transaction::reorder` writes settle before
subsequent topology operations and final validation. Inside an edit these lists
participate in final topology, so they can introduce/remove edges. The ordinary
property-only `UiTree::commit` retains its permutation-only reorder contract.

Removal follows the **final draft subtree**: evacuating a child from a removed
host preserves it. This differs deliberately from `StructureTransaction::remove`,
which retires the current subtree. Overlapping removals coalesce. Explicit
property writes or new bindings targeting removed output reject the edit.
Cancelled new output appears in neither returned `inserted` nor `removed` lists.
Old NodeIds, Taffy IDs, commands and immutable FramePackets remain valid for
surviving output and snapshots. No old node is rebuilt just to publish the edit.

Each new NodeId reserves a monotonically issued per-tree generation. A closure
may leak a provisional ID and then fail, but a later reservation of that slot
cannot revive it. Aborted drafts consume generation tokens only. Binding IDs
likewise reserve serials before publication. Resource exhaustion rejects before
live output changes; closure callbacks and computations must avoid external
side effects because arbitrary callback effects cannot be undone.

## Reactive and logical integration

`ReactiveUi::edit` and `LogicalUi::edit` have two closures within one synchronous
owner event. The first uses the existing `ReactiveTransaction` API to stage
source writes, nested savepoints and settle the computed DAG. The second reads
those final values and constructs/edits output or binds existing signals to new
endpoints. Failed source settlement or render validation publishes neither
source/derived values, bindings, stats nor render output, and runs no observer.

```rust,ignore
let owner = ui.root_scope();
let root = ui.root_render();
let result = ui.edit(
    |sources| {
        sources.set(color, 0x4488ffff)?;
        Ok(())
    },
    |(), render| {
        let node = render.append(owner, root, style, 0)?;
        render.bind(color, Property::color(node))?;
        render.reparent(portal, node)?;
        render.remove(previous_host)?;
        Ok(node)
    },
)?;
if result.result.commit.request_frame {
    // Notify the owning host once.
}
```

Existing sinks targeting finally removed output are skipped and their bindings
retired on success. Surviving existing sinks and new bindings write settled
values after manual render-property writes; a bound endpoint has its signal's
final value. New bindings and source values publish before deduplicated
post-commit effects run. Effects remain synchronous, read-only and infallible.
Logical render-owner maps update before the API returns; effects receive source
inputs only, and cannot access/reenter the mutable LogicalUi.

The endpoint lifecycle follow-up adds `ReactiveEdit::unbind` and
`LogicalEdit::unbind`. Removing an existing binding frees its endpoint within
the draft for a later bind. Its changed-source sink is skipped, and the new
writer synchronizes from final settled values. If the edit fails, the old
subscription and source values remain intact. Unbinding alone retains the
current property value. Creating then unbinding a provisional binding cancels
its synchronization and publication; its serial can never identify a later
binding. Equal-value replacement changes subscriptions without advancing the
render revision or requesting a frame. Duplicate/stale/foreign unbinds reject.
[Frozen follow-up evidence](performance/2026-11-12-edit-rebind/README.md) records
the atomic replacement tests and an independent hardware final-writer case.

`LogicalEdit` requires a live existing or provisional owner for every new output. Reparenting
retains logical ownership. Removing a host with another scope's descendants
rejects unless those descendants are moved out or explicitly removed in the
same edit. Explicit removal of a separate output of that foreign owner does
not authorize deleting its unrelated portal. Retiring output leaves the scope,
state and effects alive; later ordinary keyed reconciliation still cleans up
newly constructed output and bindings through recorded ownership.

The gallery's **Toggle output in one transaction** button updates a source,
creates/binds or removes visible output and notifies once. Its status row and
32-pixel canvas have fixed height so status wrapping cannot produce a second
viewport revision. Both ordinary event and hardware event tests exercise this
same path; the GPU pixels must change and match fresh full rendering.

## Performance and verification

Unmodified topology delegates to the existing property commit path. Free-slot
lists are copied lazily on first construction, so equal source events do not
scan the render tree or copy an arena-sized free list. Color-only updates retain
zero layout/recording and the existing material instance route. Changed topology
still validates all slots/live edges, lays out through persistent Taffy and
performs full structure-change extraction. Reparenting also searches draft hosts.
This stage does not establish sublinear structural work or faster GPU execution.

The CPU benchmark replaces 1, 32 or 256 nodes beside 1,024 stable nodes. The
control uses ordinary append, one property batch and individual direct removes;
the draft performs the same construction/material/removal in one validated
revision. Both extract exactly one frame per event and verify final geometry,
material and revision counts outside timing. Three rotating rounds per mode use
ten warmups and sixty measured samples, recording commit and commit-plus-frame
times separately. The extra validation may outweigh batching; raw samples and
limitations are preserved in [frozen evidence](performance/2026-11-12-edit/README.md).

Core cases cover failure atomicity, stale provisional handles/binding IDs,
revision exhaustion, cancelled free slots, final image/container swaps, fixed
measurement, current-draft reparenting, property-only fast routes, removed sinks,
computed values, effects and cross-scope ownership. Real-font coverage compares
draft text geometry with separately constructed text and converts it into a
container. Hardware cases cover mixed source/construction/portal/image/opacity
output against both a fresh full consumer and a separately built final scene,
rejected edits causing no submit, old snapshots and final output retirement.

The [lifecycle extension](RETAINED_LIFECYCLE_EDITS.md) now adds keyed scope
creation/reorder/retirement and named source/computed/effect lifetimes to the same
draft. General asynchronous effects, dynamic dependency changes,
focus/events/semantics lifetime and legacy islands remain outside this API.
The complete [RFC acceptance goal](RETAINED_IMPLEMENTATION_PROGRESS.md) stays open.
