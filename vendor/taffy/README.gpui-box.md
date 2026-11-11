# Vendored Taffy 0.15.0

This directory preserves the cached crates.io package for `taffy =0.15.0`.
`UPSTREAM.json` records its registry checksum, Git source revision and original
per-file SHA256 values. `LICENSE` was fetched from that exact source revision;
its source URL and SHA256 are recorded separately because the published package
did not include it. Taffy is MIT licensed. The original manifest, source and
examples are retained. The distribution workspace uses a crates.io patch to
select this source for both the persistent owner and GPUI, retaining the pinned
version and enabled layout algorithms. Its standalone Cargo.lock/target are
local test artifacts and ignored.

The only modified upstream file is `src/tree/taffy_tree.rs`. Its internal view
adds a generic, statically dispatched callback at `set_unrounded_layout` and a
new `compute_layout_with_measure_and_observer` entry point. The existing entry
point supplies a no-op callback. Algorithms, constraints, caches and rounding
are unchanged. Observations include equal and transient writes, and run before
the unrounded layout is stored. Callbacks must not panic or mutate the tree.
Consumers must deduplicate IDs and compare final layouts after computation.
When rounding is enabled these observations are not final rounded results.
The retained owner explicitly disables rounding, as before this change.

```powershell
mise exec -- cargo test --manifest-path vendor/taffy/Cargo.toml --offline --lib
```

All 169 library tests pass with default features. With default features disabled
and the distribution's `std,taffy_tree,flexbox,grid,block_layout,float_layout,content_size`
set, the upstream test module calls `CompactLength::calc` without gating that
test on `calc`, producing a compile error. That upstream test was not changed.
The distribution's retained core/example tests compile and validate the actual
enabled feature set, including the observer versus original layout entry point,
selective/full publication and real hardware pixels.
