# Persistent ordered page hit index

`FramePacket.hit_test` now queries a balanced hierarchy of page bounds. Leaves
cover the existing immutable 64-item pages, while internal nodes union their
children's clipped border bounds. Querying right before left preserves reverse
paint order; leaf checks retain the prior half-open rectangle test. Opacity and
transparent fills retain their prior hit semantics. This is a broad-phase index
for rectangular hit bounds, not path/text-shape picking or interaction policy.

The index is lazy: the first query constructs it by scanning current items.
Indexed publication patches only changed page bounds and their ancestor paths.
Unchanged leaf/ancestor bounds reuse their Arcs, including an entire root when
a material-only patch does not change bounds. The index stores page ranges and
bounds, not PaintItems or resources. It reads items from the querying packet's
current pages. Old packets keep their own immutable index root. Public mutable
COW item access invalidates only that packet's lazy index. Full layout/order
publication starts a fresh lazy index; packets never queried do not build one.

`hit_test_with_stats` reports visited bounds, tested items and cold built items.
Two new core tests compare with the prior reverse scan, including 10,001 items,
clipped movement, page sharing, material-only index reuse, old snapshots, COW
mutation, overlay order, half-open clip edges, visibility, generation reuse and
empty/non-finite queries. The 40-round selective/full property tests also compare
hit results. Combined core tests pass 54 cases; retained-only passes 49. Ten
shared-example tests, the desktop WGPU build and feature-specific/combined
Clippy `-D warnings` pass. Hardware regression remains 124 pixel scenarios.

[Frozen evidence](performance/2026-11-11-hit-index/README.md) includes 55 verified
source hashes, warm indexed/linear comparisons and cold construction costs.
In the 10,001-item grid, warm median query is 0.3745 µs versus 20.6553 µs for
linear scan, averaging 74.38 tested items and 13.64 bounds. Cold rebuild/query
median is 54.1 µs. When every item overlaps and the top item hits immediately,
linear scan is faster: 0.0080 µs versus indexed 0.0581 µs. This tradeoff remains
explicit; an adaptive query strategy is not implemented. These are batched CPU
query timings excluding dispatch, update/publication, GPU and presentation.

The hierarchy splits by paint order rather than spatial median to keep update
and ordering simple. A page spanning spatially distant items has loose bounds;
heavy overlap or miss patterns may visit all pages. There is no universal
sublinear worst-case guarantee. Index construction after full publication is
linear, directory publication still copies N/64 pointers, and sparse property-map
updates/consumer composition retain their existing costs. General geometry,
event/focus/semantics and physical resource budgeting remain acceptance work.
