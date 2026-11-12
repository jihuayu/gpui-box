# Owned paint pages and shared delta views

Normal selective publication now moves complete changed pages into the next
snapshot and shares authoritative paint items with its delta. This extends
[paged paint storage](RETAINED_PAINT_STORE.md),
[subtree extraction](RETAINED_SUBTREE_EXTRACTION.md) and
[shared GPU placement](RETAINED_GPU_SPATIAL.md). The
[complete RFC goal](RETAINED_IMPLEMENTATION_PROGRESS.md) remains active.

## Measured motivation and update flow

A phase probe found that a 10,000-descendant parent update spent about 18.2 ms
publishing the CPU frame and 11.8 ms submitting it; commit itself was about 7 µs.
Reducing GPU uploads alone had left publication as the largest phase. The old
publisher inserted an ordered subtree into a BTreeMap, cloned every affected old
page, and cloned updated PaintItems into both pages and a separate delta list.

The default selective path keeps subtree candidates contiguous, uses sorted
disjoint ranges and a sparse point set to suppress overlapping extraction, then
sorts candidates once into preorder. Layout, paint and property edits retain the
existing precedence: subtree extraction covers its geometry/material edits, and
geometry extraction covers local paint edits at the same node.

A completely replaced page takes ownership of its extracted items. A partial
page clones its old items once and moves changed payloads into it. Unchanged
pages retain their Arc identity. Existing persistent hit-index bounds update
against the resulting pages; equal bounds still reuse index paths. Damage and
property-tree publication use the same old/new geometry and effect semantics.

`FrameBuildPolicy::SelectiveCopy` preserves the original candidate map, old-page
copying and independent delta payloads as a control. `Full` remains the forced
full-extraction control. Switching policy affects the next dirty publication;
equal transactions still request no frame.

## Immutable deltas and checked mutation

`FramePacket::updates` is now `Arc<PaintUpdates>`. The default representation owns
the exact immutable `Arc<PaintItems>` root and a private sorted unique index list.
Iteration and integer indexing borrow authoritative items directly, eliminating
per-item delta payload/resource-handle clones. The packet remains owned after its
tree is edited or dropped, and old packets retain their original pages/resources.
It does not create a chain of previous snapshot roots.

Only private indexed construction can establish this representation. Consumer
delta validation skips duplicate item comparisons only when actual root Arc
identity matches the current authoritative snapshot. Equal values or a revision
number do not establish that identity. Header, item, spatial/property, removal
and order checks still run normally. This proves view-to-snapshot agreement,
without a general caller-supplied delta-completeness claim.

Public COW mutation materializes an independent owned item list. Mutating either
the authoritative root or the delta therefore loses the identity shortcut and
uses the checked path, including mismatch/duplicate rejection. Tests verify that
failed mutations do not poison valid retry, and detached but equal roots are
checked rather than treated as shared views.

This is an experimental API change from a contiguous `Arc<[PaintItem]>`.
`iter`, `len`, `is_empty`, `get`, integer indexing and `Arc::make_mut(...)[index]`
remain available. Callers needing contiguous items use `to_vec()`. Explicit
packet construction uses:

```rust,ignore
frame.updates = Arc::new(PaintUpdates::from(vec![item]));
```

## Work counters and validation

`FrameBuildStats::copied_pages/copied_items` continue to count cloned old page
payloads. `replaced_pages` counts complete page moves; `moved_items` counts
selective item moves, and `delta_cloned_items` counts independent delta copies.
`ValidationStats::validated_delta_items/shared_delta_items` distinguish checked
payloads from items supplied by an exact snapshot view. These are logical work
counts, excluding root-directory copies, allocator metadata and shared resources.

For a parent with 10,000 descendants, both modes extract 10,001 updated nodes.
The original selective control clones 10,002 old-page items and 10,001 delta
items. The default clones only 64 old items in the partially changed first page,
moves 10,001 items, replaces 156 complete pages and clones no delta payloads.
Both modes upload the same 96-byte GPU property and redraw the same full root.

Core tests compare owned, copying and forced full output across dense/sparse and
mixed layout/property/material changes, sorted deltas, images/shadows, damage,
warm hits and snapshot lifetime after tree drop. Hardware cases exercise both
selective modes at scale 1/1.25/1.5 with actual text, alpha images, shadows,
isolation, reorder, skipped frames, generation reuse and mutation rejection.
Normal gallery/hardware clicks continue through the default owned path.

## Performance scope and remaining work

The [frozen evidence report](performance/2026-11-12-page-ownership/README.md)
records complete owner-to-GPU-completion A/B with phase samples, original copying
controls, cold/warm hit-index cases, ancestor motion and single material edits.
Both controls share the current packet API, shader, shared GPU placement and full
root redraw; this is not an older executable comparison. Setup, readback and
native presentation remain outside timing. Preserve losses and host variation.

Dense resolved extraction, sorting sparse candidates, page-directory copying,
consumer authoritative checks and backend allocation/dependency scans remain.
The change reduces duplicate work rather than establishing a sublinear complete
ancestor update. Additional GPU property adoption/materials, native presentation,
resource budgets, device loss and sustained cross-platform 60/120 Hz/input/energy
qualification remain in the full-goal tracker. The
[architecture decision](ADR_RETAINED_PAGE_OWNERSHIP.md) records alternatives and
the API/ownership trade-offs.
