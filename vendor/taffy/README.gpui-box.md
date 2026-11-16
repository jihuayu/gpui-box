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

## Local changes

- `src/tree/taffy_tree.rs` adds a generic, statically dispatched callback at
  `set_unrounded_layout` and a `compute_layout_with_measure_and_observer` entry
  point. The existing entry point supplies a no-op callback. Observations
  include equal and transient writes, and run before the unrounded layout is
  stored. Callbacks must not panic or mutate the tree. Consumers must
  deduplicate IDs and compare final layouts after computation. When rounding
  is enabled these observations are not final rounded results. The retained
  owner explicitly disables rounding.
- `src/tree/layout.rs` adds `Layout.baselines`, populated from the final
  `LayoutOutput` by root, Flex, Grid, Block/float and out-of-flow publication.
  Hidden layouts clear it. It stores horizontal baselines relative to the
  border-box top, independent of cache lifetime. `compute/mod.rs` rounds each
  baseline in absolute coordinates before subtracting the rounded box origin.
  This adds two `Option<f32>` fields to each final layout; the framework removes
  its separate leaf-only published baseline. No extra tree traversal or cache
  lookup is added to publication.
- Grid final positioning now passes its child's actual final first baseline
  to container propagation. Previously, rows with zero or one baseline-aligned
  item skipped the preliminary baseline pass and could incorrectly substitute
  the child's bottom edge; rows with multiple participants could count the top
  margin twice. Track sizing and alignment shims retain their existing roles.
- Two test-only feature assumptions are corrected: the calc pointer test is
  gated on `calc`, and Style size expectations account for `flexbox_balance`.
  Runtime features, dependency versions and cache algorithms are unchanged.
- In-flow root sizing keywords are resolved before final layout, preserving
  constraints, the other axis, box-sizing and percentage-height definiteness.
  Block automatic width stretch no longer overrides intrinsic width. Flex
  ContentSize ignores the measured container's own min/max until the caller
  applies them; final used main dimensions constrain greedy/balanced wrapping
  independently of descendant percentage definiteness. These are algorithm
  changes, not only publication instrumentation. Focused regressions and a
  small independent Chromium reference accompany the framework
  [sizing/query contract](../../docs/RETAINED_FLEX_SIZING_QUERIES.md).

The framework publishes only the first horizontal baseline. Retaining Taffy's
`Baselines.last` field does not implement last-baseline alignment, vertical
writing modes or full CSS baseline conformance. See the
[framework contract and limits](../../docs/RETAINED_LAYOUT_BASELINES.md).

## Verification

```powershell
mise exec -- cargo test --manifest-path vendor/taffy/Cargo.toml --offline --lib
mise exec -- cargo test --manifest-path vendor/taffy/Cargo.toml --offline --no-default-features --features std,taffy_tree,flexbox,grid,block_layout,float_layout,content_size --lib
```

Validation counts and exact toolchain for the current change are recorded in
the framework contract above. Historical pixel and performance experiments
predating these changes remain historical evidence; this baseline publication
change does not claim a measured speedup or complete Flex conformance.
