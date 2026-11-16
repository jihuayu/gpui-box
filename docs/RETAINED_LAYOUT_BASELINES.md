# Published layout baselines

Follow-up 2026-11-17: [first/last baseline sets](RETAINED_LAST_BASELINE.md) adds last-baseline publication, dual-metric leaves/parents and public Flex LastBaseline alignment. The first-only limits below describe the original stage.

Updated 2026-11-16 on `codex/rendering-performance-research`. This completes
first-horizontal-baseline publication for the current layout adapter. The
[complete-runtime goal](RETAINED_IMPLEMENTATION_PROGRESS.md) remains open.

Later same-day stages add [intrinsic min/max](RETAINED_INTRINSIC_LIMITS.md)
and [normal unbounded layout](RETAINED_UNBOUNDED_LAYOUT.md). The test counts
and remaining-work list below describe this baseline-publication stage;
the complete-runtime progress table is authoritative for current status.

## Public contract

`LayoutResult.baseline` is an optional first horizontal baseline, in logical
pixels from the node's border-box top. Its value comes from the final layout
algorithm, including the selected child's local position for containers.
`CustomMeasureSnapshot.baseline` remains content-box local; the leaf adapter
adds resolved top padding and border before it enters layout.

Flex, legacy Grid and Block containers now expose the baseline produced by
their actual algorithms. Their fallback rules can synthesize a baseline from
an eligible child's bottom edge when it has no baseline. An unmeasured leaf,
empty container or hidden subtree can have no baseline. A hidden subtree clears
old results, and restoration uses its current measurement input.

`layout_result` observes the last successful frame publication. Committing a
new measurement alone does not expose a partially updated result. Resolved and
local frame paths both publish it. Intrinsic `measure_node_with_constraints`
reads the same final result from the owned Taffy clone; it changes neither the
published layout nor the frame revision.

## Engine changes and update cost

The project already uses its [vendored Taffy fork](../vendor/taffy/README.gpui-box.md)
through the workspace patch for both the retained owner and GPUI. Its final
`Layout` now stores `Baselines`, populated from `LayoutOutput` at root, Flex,
Grid, Block/float and out-of-flow publication. These values remain valid when
computation caches are evicted or invalidated. Reading the cache after the fact
would conflate computation state with published output and could miss changes
that preserve every box's geometry.

The existing layout observer sees baseline-only changes through Layout equality.
The owner publishes those changed results without new all-tree traversal or
measurement calls. When alignment leaves geometry unchanged, paint pages and
commands remain shared, with zero paint updates or chunk recording. If the
baseline change actually moves aligned items, the normal geometry/subtree
extraction updates painting and hit testing, retaining command identity and old
frame snapshots.

This adds two `Option<f32>` fields to every stored Taffy `Layout`, including its
rounded/unrounded copies and the owner's published copy, while removing the
owner's separate leaf-only `Option<f32>`. This is a correctness and API change;
no memory reduction or measured throughput improvement is claimed.

Grid had a related propagation bug: its preliminary track pass populated a
baseline only when multiple items participated in alignment. A single item or
Start-aligned item could therefore export its bottom edge instead of its real
baseline. With multiple participants, the preliminary value included a margin
already counted in final positioning. Final Grid positioning now provides the
actual child baseline after sizing, while the earlier baseline shim remains
responsible for track sizing/alignment. Scroll-container clamping follows the
existing algorithm's rules.

Taffy rounded consumers also receive baselines rounded at the absolute y
coordinate, then translated back by the rounded box origin. The retained owner
continues to disable rounding; this does not decide the framework's eventual
device-scale rounding policy.

## Verification

The new framework cases cover nested Flex alignment, baseline-only changes
through cached ancestors, resolved/local publication, no-paint-work reuse,
hidden restoration/removing measurements, isolated intrinsic queries, Flex
order and wrap-reverse selection, actual paint/hit movement with immutable old
frames, and legacy Grid/Block/absolute publication. Vendor regressions cover
cache/dirty/hidden behavior, first/last field rounding and Grid with one/two
aligned or Start-aligned children and nonzero top margin.

Validation uses Rust 1.98.1 on Windows, offline with the repository's existing
lockfile for workspace commands:

| Command after `cargo +1.98.1` | Result |
| --- | --- |
| `test --locked --offline -p gpui-box --features retained --lib` | 156 passed |
| `test --locked --offline -p gpui-box --features retained-text,retained-wgpu --lib` | 189 passed |
| `test --manifest-path vendor/taffy/Cargo.toml --offline --lib` | 172 passed |
| `test --manifest-path vendor/taffy/Cargo.toml --offline --no-default-features --features std,taffy_tree,flexbox,grid,block_layout,float_layout,content_size --lib` | 166 passed |
| `clippy --locked --offline -p gpui-box --features retained-text,retained-wgpu --lib -- -D warnings` | Passed |
| `check --locked --offline -p gpui-box-example-hello --features retained-wgpu --tests` | Passed |

Vendor test-only fixes gate the calc pointer test on `calc` and account for
the optional `flexbox_balance` field in Style size assertions, enabling the
actual distribution feature set's tests. The broader `--all-targets` strict
Clippy attempt still reports the existing `items_after_test_module` issue in
`retained/local_ink.rs`; no all-targets clean result is claimed. Retained-only
tests emit the existing non-WGPU unused-field warning. No new GPU pixel or
sustained performance qualification was run for this layout-contract change.

## Remaining framework work

- Last-baseline alignment and vertical writing modes are not implemented by
  this change. Storing Taffy's last field is not evidence those algorithms exist.
- Broad Flex/baseline conformance, RTL/reverse/scroll/containment combinations,
  constraint-dependent custom measurements and richer paragraph semantics need
  further qualification. This test set is not a CSS conformance suite.
- The new framework layout API still lacks public Grid/calc support, Unbounded
  engine semantics, general container-query dependencies and adaptive structures.
- Independent presentation, general effects/primitives, native sustained
  60/120 Hz tail latency and the CPU rendering acceptance work remain tracked
  in the full-runtime goal. This baseline change establishes a backend-neutral
  layout result; it does not qualify those rendering paths.
