# Paged paint snapshots and selective publication

The [page ownership follow-up](RETAINED_PAGE_OWNERSHIP.md) now moves complete
changed pages and shares authoritative items with delta views. Partial pages
still copy on write; `SelectiveCopy` preserves the original selective-copy
control described by this historical stage's measurements.

The persistent owner now keeps a generation-checked node-to-paint index and
publishes immutable `PaintItems` as 64-item pages. Local material/paint changes
extract only dirty visible nodes and copy only touched pages. Old `FramePacket`
values keep their page Arcs and their original items, commands and resources.
The directory still copies N/64 page pointers on a changed snapshot; this does
not make the complete update constant time.

Color, decoration and same-layout image/text changes reuse the published
geometry and property trees. Final values are compared with the previous item,
and damage unions only changed old/new ink. Repeated commits coalesce dirty node
IDs until publication. Hidden-only material changes publish no visible patches
and retain the items Arc. Layout, visibility, order and visual property changes
currently rebuild the snapshot and its index. Node generations prevent a reused
slot from patching an old node's page position.

`FrameBuildStats` reports extracted nodes, copied pages/items and full rebuilds.
`FrameBuildPolicy::Full` forces the original extraction on the same persistent
tree, commands and transaction path for correctness and timing comparisons.
This control changes publication work without changing rendered behavior.

Three regression tests cover cross-page batches, repeated selective/full
equivalence, old snapshot ownership, hidden material updates, geometry fallback,
reorder/index rebuilding, slot-generation reuse and eventual page retirement.
The combined core suite passes 48 tests; retained-only passes 45. Combined and
feature-specific Clippy checks use `-D warnings`; the shared example has ten
tests and the desktop WGPU build passes. The hardware runner passes 115 pixel
scenarios, including two-page colors, decoration and a selective update after
ancestor movement. The two-page case writes 320 GPU bytes without a full resync.

[Frozen performance evidence](performance/2026-11-11-paint-store/README.md)
contains paired publication timings, raw samples, pixel checks and 48 verified
source hashes. A 10,001-node single-color change extracts one node and copies
64 items; median publication is 3.2 µs versus 9,910.7 µs for full extraction.
These are CPU publication measurements, excluding renderer/GPU/presentation;
they do not establish an end-to-end frame-rate multiplier.

Remaining work includes selective property/layout extraction, general stable
chunk IDs, indexed hit testing and eliminating consumer snapshot/order scans.
This is an immutable paged snapshot store, with the existing node identity as
the extraction key; it does not yet provide a general primitive chunk registry.
