# Persistent property snapshots

`PropertyTrees` now owns four ordered `PropertyMap<T>` stores for transform,
clip, scroll and effect chains. Each store is an AVL tree of immutable shared
nodes. Cloning a property snapshot clones four root Arcs, independent of the
number of records. Insert/remove copies shared search and rotation paths;
exclusive paths mutate in place. Equal insertions and missing removals preserve
root identity. Keys retain the complete tree/slot/generation identity. Previous
frames own their original roots and values, without a linked frame history.

The selective extractor sets or removes each current property record directly.
It no longer deletes every subtree record before inserting the current values.
An ancestor movement can consequently reuse unchanged descendant values and
chains. Resetting a property to its default removes that record and reconnects
descendants to the inherited chain. Existing disjoint-subtree extraction and
paint-page copying remain in place; layout/visibility/order changes still
rebuild the full snapshot. Full extraction collects records, sorts by identity
and constructs balanced trees in one pass after sorting, avoiding incremental
lookup/rotation overhead during a complete rebuild.

`PropertyMap` preserves the used ordered map operations: lookup, indexing,
iteration, length, containment, insertion, removal, clear and value equality.
The public field types change from `BTreeMap` to `PropertyMap`; arbitrary
`BTreeMap` APIs such as entry/get_mut/range are not provided. Callers mutate
through insert/remove so published roots remain immutable. Lookup and edits are
O(log P); cloning is O(1), iteration and final release of an exclusive complete
map are O(P). Batch path copies share already-private ancestors, but this is not
a general GPU property buffer or a constant-time complete UI update.

The consumer compares its owned effect root separately from the outer
`PropertyTrees` Arc. A translation/clip/scroll change which retains that root
needs effect-reference checks only on changed paint pages. Composition also
reuses the identical effect root before its existing topology comparison.
Effect additions/removals/value changes still run complete effect validation
and may scan the scene; malformed maps cannot claim reuse through a revision
number. Existing item/header/delta validation remains intact.

Validation covers 8,000 deterministic mixed map edits against `BTreeMap`, AVL
ordering/balance, independent snapshots, bulk construction, removal to empty,
bounded record copies for a 10,000-entry snapshot edit and eventual root
release. Integrated tests cover independent map sharing, inherited parent
replacement, resets, hidden nodes, overlapping/disjoint dirty roots, node
generation reuse, old frames and malformed effect-map rejection. The combined
core suite passes 108 tests and the gallery/font suite passes 12, including an
ordinary click reaching real GPU pixels. Clippy passes `-D warnings`.

The hardware suite passes 162 pixel scenarios on Windows / GTX 1650 Ti Vulkan.
Five additions check dense-property translation, opacity, reset-to-default,
retired generation replacement and resubmission of an independently retained
old snapshot. Full-vs-selective item/property/damage equivalence is checked by
the CPU benchmark outside timing. Hardware references follow the existing
contract: original full GPUI renderer for plain quads, fresh uncached retained
renderer for isolated effects or mixed resources.

[Frozen performance evidence](performance/2026-11-12-property-maps/README.md)
compares the old full-map-clone executable with the final path-copy executable
using an unchanged CPU harness. It includes raw samples and full-extraction
controls, whose additional cost remains a tradeoff. GPU submission, draw,
presentation, input latency, sustained cadence and cross-platform acceptance
are excluded from the CPU result. General chunk IDs, affine/shared GPU property
trees, selective layout extraction and backend whole-scene scans remain in the
[full implementation goal](RETAINED_IMPLEMENTATION_PROGRESS.md).
