# Intrinsic Flex sizing and staged container queries

Updated 2026-11-16 on `codex/rendering-performance-research`. This stage extends
the framework-owned layout contract and fixes algorithms in the project-owned
Taffy fork. The full RFC remains partially implemented.

Subsequent same-date [size containment and fluid queries](RETAINED_SIZE_CONTAINMENT.md)
extend the fixed-inline proof below with explicit boundaries, fluid external
allocation, safe auto-height column chains and sibling-allocation dependencies.
The validation counts in this document describe the preceding sizing stage;
the linked follow-up records current combined counts and API differences from
browser `container-type`.

## Public dimensions and gap axes

`LayoutDimension` now supports `MinContent`, `MaxContent`, `FitContent`,
`FitContentPx(limit)`, `FitContentPercent(fraction)` and `Stretch` for preferred
width/height and flex basis. `Content` is accepted only for flex basis: it sizes
from content instead of the preferred main size. Limits must be finite and
nonnegative. `Content` in width/height is rejected before a property transaction
can publish, even when a later write overwrites the invalid value.

These are preferred-size styles, separate from `LayoutAvailableSpace` intrinsic
measurement constraints. Min/max fields still use `LayoutLength`, which does
not accept intrinsic keywords. `Unbounded` remains explicitly unsupported.
The numeric fit-content functions use Taffy's existing semantics; the browser
reference below does not support `width:fit-content(50px)` and does not validate
those functions as browser-compatible.

The documented `gap(row, column)` contract is now implemented correctly:
`gap.width` is the horizontal column gap and `gap.height` the vertical row gap.
Neither swaps when Flex direction changes. `row_gap` and `column_gap` modify
the corresponding physical axis. `main_cross_gap(main, cross)` remains an
explicit logical-axis override, lowered using the final Flex direction.
This corrects previous behavior: callers compensating for the old swapped
physical gap must remove that compensation.

## Why the Taffy fork changed

An in-flow root has no parent Flex/Grid algorithm to resolve its size keywords.
The old path could ignore them although the same style worked as a Flex item.
The root now resolves exact styles on the other axis, padding/border,
box-sizing and constraints, obtains the keyword content contribution, applies
outer constraints, then performs final layout with the used size. Block's
automatic width stretch no longer replaces an explicit intrinsic width.
Roots with no size keyword take the existing path.

Flex `ContentSize` measurement previously applied the measured container's own
min/max too early, unlike leaf measurement. For example, max-content width
70 with max-width 55 and padding could wrap during the intrinsic query and
return the largest child instead of the unconstrained contribution. Its own
constraints are now applied by the caller after the content contribution;
descendant constraints remain part of descendant layout. This affects the
general Flex algorithm, including content flex-basis measurement, rather than
adding an adapter exception for one tree shape.

The final used size and percentage definiteness are distinct. Content-derived
column heights still constrain final line breaks, including the balanced
algorithm when enabled, while remaining indefinite for percentage heights.
Root percentage widths resolve against the final used inline extent. Tests
cover a 45px column with 30px/40px children wrapping into two columns, and a
30px content-derived height with a 50% child retaining its 30px content height.
Definite 80px height instead resolves that child to 40px. Aspect-ratio transfer
from a definite other axis and ContentBox padding are covered separately.

Intrinsic resolution can add size queries. Switching sizing modes may evict
Taffy cache entries; this change does not claim a cache or performance gain.
There is no new global traversal for ordinary non-keyword roots. Broader
intrinsic interactions, writing modes and CSS conformance remain unqualified.

## Container query dependency contract

`LayoutResponsiveBindings::bind_container` now requires a strict render
descendant target and a provably independent host inline extent. Use
`LayoutStyle::fixed_inline_size(width)` to set width/min-width/max-width to
the same logical length, preserving independently authored height, including
`Auto`. It does not set alignment: normal Flex stretch can still give an
auto-height host the parent's cross size; use `align_self(Start)` when content
height is intended in a row parent.

The current proof accepts BorderBox Flex/None, equal finite nonnegative fixed
width/min/max, fixed horizontal padding/border and a finite nonnegative
scrollbar gutter fitting the box. It does not require fixed height or zero
grow/shrink: the locked min/max determine the inline extent. If another
responsive rule controls the host, its current source and every breakpoint
branch must satisfy the same proof, regardless of registration order. A fixed
preferred width alone is insufficient because default Flex shrink can change
it. Unrelated sibling targets are rejected.

Before updating viewport sources, each frame prunes retired sources/targets,
revalidates remaining hosts/topology and builds a dependency graph. A container
rule waits for any rule targeting its host or an ancestor. The resolver then:

1. Applies viewport styles in one transaction and lays out the tree.
2. Reads actual host content width for every rule in the next independent
   stage, subtracting resolved padding, border and allocated scrollbar width.
3. Applies that stage in one transaction and lays out only if values changed.
4. Advances to the next stage and returns the final packet.

No convergence loop is used. There are at most one initial layout entry plus
one per changed dependency stage; actual `layout_runs` may be zero when cached.
Hidden hosts query zero width and restore through the same stages. Stale hosts
return an error rather than silently dropping a surviving rule. Unsafe host
styles/reparenting fail before viewport source or published-frame changes.

`ResponsiveFrame.layout_runs` and all `layout_stats` fields sum work over every
pass, including intermediate layouts. `rules_examined` counts registered
rules, not graph comparisons or layout-node visits. Graph construction checks
all rule pairs with render ancestry walks: O(R² × D) time and O(R + E) memory;
topological scheduling is O(R + E). It is rebuilt each frame. Rule validation
also scans producers/branches. This is bounded dependency execution, not
indexed/sublinear dispatch or a measured throughput optimization.

Stages can publish intermediate owner packets. Consumers should submit only
the returned final packet; the existing base-revision recovery is necessary
when intermediate bases were not submitted. This is not one atomic multi-stage
structural transaction. A later stage failure does not roll back earlier
successful transactions. The public resolver currently returns resolved
FramePacket output; a public local-placement responsive entry remains open.
Legacy `ResponsiveBindings` retains its older fixed-width/height, two-pass,
nested-dependency-rejection contract.

## Verification

On Windows with Rust 1.98.1, locked/offline workspace builds:

- `gpui-box --features retained --lib`: **174 passed**.
- `gpui-box --features retained-text,retained-wgpu --lib`: **207 passed**.
- Vendored Taffy default `--lib`: **174 passed**.
- Vendored Taffy distribution feature set `std,taffy_tree,flexbox,grid,block_layout,float_layout,content_size`:
  **168 passed**.
- Combined text/WGPU **library** Clippy with `-D warnings`: passed. This is not
  an all-targets clean claim; the existing `local_ink` test-module ordering
  issue remains outside this stage.
- Hello example with `retained-wgpu`: **11 passed, 1 hardware test ignored**.
  This ran against the current workspace, including its pre-existing example
  edits, which are not part of this change.
- Flex-only Taffy library check passes; its existing print helper warns about
  an unused `writeln!` result. Retained-only tests retain the existing unused
  spatial transport field warning.

Ten new framework sizing regressions cover root/nested wrapping, all four
physical gap directions, content basis, other-axis preservation, min/max
precedence, padding/ContentBox, aspect ratio, percentages, hidden restoration,
measure-only isolation and invalid transaction rejection. Eight container-query
regressions cover dependency order, content-driven height, idle sharing,
registration-order-independent proofs, exact thresholds, viewport-driven
hosts, hidden restoration, actual scrollbar gutter, foreign/stale hosts and
runtime style/topology invalidation.

The standalone [browser fixture](performance/2026-11-16-flex-contract/browser-reference.html)
and [captured output](performance/2026-11-16-flex-contract/browser-reference.json)
record **24/24** geometry checks on Chromium **145.0.7632.6**. They independently
check keyword widths, min-over-max column wrapping, max-width/padding,
aspect-ratio transfer and root/nested percentage width/height behavior.
This is a small geometry reference, not WPT coverage or GPU pixel validation.

No new sustained 60/120Hz, GPU pixel, visible-presentation or CPU-raster
performance qualification was performed for this stage. General query hosts
with flexible/content-dependent inline sizes, public intrinsic min/max,
Grid/calc, structural adaptation, last-baseline/vertical writing and shared
device rounding remain open alongside the full RFC acceptance matrix.
