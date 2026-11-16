# Size containment and fluid container queries

Updated 2026-11-16 on `codex/rendering-performance-research`. This stage builds
on [intrinsic sizing and staged queries](RETAINED_FLEX_SIZING_QUERIES.md).
It extends framework/API semantics and the project-owned Taffy algorithms;
it does not complete the RFC or qualify sustained frame rates.

## Public boundary

`LayoutStyle.container_type` and its builder accept `LayoutContainerType`:

| Value | Intrinsic contribution | Other layout effects |
| --- | --- | --- |
| `Normal` (default) | Content participates normally | No added containment |
| `InlineSize` | Ignore content when sizing the horizontal inline axis | Layout containment |
| `Size` | Ignore content when sizing either axis | Layout containment |

These are explicit sizing boundaries, not fixed-width shortcuts. The outside
Flex algorithm still allocates space, grow/shrink and min/max still apply,
and padding/border/scrollbar gutters remain in the box model. Auto contained
axes behave as empty content, subject to those constraints; they do not retain
an earlier size. Children still receive normal layout and remain paintable.
Local overflow is retained; layout containment prevents descendant overflow
from expanding an ancestor's scrollable overflow beyond the boundary.

Both query types lower to Taffy **layout containment** plus their size bits.
They suppress a container's outward baseline and establish an absolute/fixed
positioning containing block, including when the container is static. This is
an intentional framework contract, stronger than current Chromium
`container-type` alone. Do not assume browser declaration equivalence. The
browser reference compares `contain:layout` explicitly for positioning.
Clip, scroll and paint effects remain independently authored. Full CSS stacking
contexts, paint containment, transforms establishing positioning blocks and
CSS out-of-flow scroll attachment are not implemented by this declaration.

Example: a content-height column page with a fluid query boundary:

```rust
let page = LayoutStyle::column()
    .size(LayoutDimension::Percent(1.0), LayoutDimension::Auto);
let query_host = LayoutStyle::column()
    .container_type(LayoutContainerType::InlineSize)
    .padding(LayoutEdges::all(LayoutLength::Px(5.0)));
```

With a stretched host in this column, a 210px viewport gives 200px of queried
content width. A query can change its child from 10x10 to 1000x80 without
widening the host; the host/page height changes from 20px to 90px. This is a
real content-width query, not a parent maximum-constraint query.

For a row containing a 60px fixed sibling and a `Size` host with `flex_grow(1)`,
the host receives the remaining width. At viewport 270px, 5px padding on each
side again yields a queried width of 200px. Content expansion does not feed
back into either of the host's intrinsic axes. Auto height under full size
containment may collapse; choose `InlineSize` only where the dependency proof
below permits content height, or allocate height externally.

## Algorithm and measurement

The vendored Taffy `Contain` gains `INLINE_SIZE` and `SIZE`. A common helper
resolves contained axes through the zero-content leaf box model. Flex, Grid
and Block use those dimensions before their normal child layout. Legacy
Grid/Block tests exercise this shared implementation; public Grid is still
not exposed. Uncontained ordinary nodes bypass this helper's computation.

Leaf measurement resolves the contained used width before calling the
content callback, so paragraph height sees its actual wrapping constraint.
Measured content still contributes to overflow and uncontained axes. Retained
baseline adaptation respects layout containment rather than reattaching the
custom or paragraph baseline afterward.

A contained image's natural aspect ratio no longer transfers dimensions.
For an 80x40 image with authored width 100 and auto height, Normal gives 100x50,
InlineSize gives 100x40 and Size gives 100x0. An explicitly authored ratio of 2
gives 100x50 in all three cases. The resource still paints in the resulting
content box. `contain-intrinsic-size` fallback dimensions are not implemented.

## Query safety and dependency scheduling

`LayoutResponsiveBindings` retains the existing strict-render-descendant
target rule and fixed-inline proof. It additionally accepts:

1. A Flex/None host with invariant full size and layout containment. Its
   external allocation can be fluid.
2. An inline/layout-contained host whose width follows a provable column chain
   to a root with an explicit length, percentage or Stretch width. Every link
   must be in flow, Flex/None, Column/ColumnReverse and NoWrap without aspect
   ratio feedback. A link uses explicit width or Auto stretching without auto
   horizontal margins; fixed-inline dimensions also qualify.

The proof checks live styles, responsive sources and **all** breakpoint
alternatives of affected ancestors. Switching a host or ancestor to an unsafe
style is rejected before binding/source synchronization or, on a subsequent
frame, before viewport updates and new frame publication. Inline containment
under a Row/wrapping/aspect-feedback chain is conservatively rejected even if
a particular scene happens to settle. No convergence loop is used.

The staged dependency graph now includes potential **sibling allocation**
dependencies for full-size fluid hosts. A producer inside an invariant full
size/layout boundary cannot affect allocation outside it; changing that
boundary itself can. Rules targeting a host or ancestor precede queries of
that host. This handles later-registered producers and nested contained hosts
without reading a transient pre-allocation width. Cycles fail with
`UnsafeContainerQuery` while validating the complete graph.

All stages still use the normal typed property transactions and persistent
layout publisher. Only changed styles cause another layout. Idle repeated
inputs have zero layout runs. Stage counters accumulate all work. Stages are
not one atomic structural transaction; later-stage errors do not roll back
earlier successful publications. Only the final returned packet should be
submitted, with the existing skipped-base recovery.

The resolver still rebuilds its graph and checks rule pairs each frame.
Column proofs clone possible styles and scan relevant producers; their
ancestor results are memoized within an individual proof to avoid multiplying
branch alternatives across depth. With R rules, D depth and B style
alternatives per node, a conservative bound for repeated pairwise proofs is
O(R^3 * D * (B+1)^2), plus O(R^2) graph edges. This is not an indexed or measured
performance improvement. Global proof caching, invalidation and threshold
dispatch remain future work.

## Positioning coordinates and publication

Taffy stores out-of-flow `Layout.location` relative to the positioning
containing block, which may skip static render ancestors. The renderer's
spatial graph and public `LayoutResult.location` require render-parent-local
coordinates. The adapter now normalizes those coordinates after layout and
stores both working and published origins. Reading a result after committing
a new style but before publishing a frame still returns the old location in
its old coordinate system.

An indexed set tracks out-of-flow nodes through append, draft creation, style
changes, removal and topology edits. After a layout pass their normalized
positions are checked even when raw Taffy output was cached: moving a static
intermediate ancestor changes the compensating local offset. Only changed
published offsets dirty extraction. An iterative ancestor walk avoids adding
recursive coordinate traversal; the additional work is O(M * D) for M
out-of-flow nodes, plus the observed layout changes. There is no new retained
slot scan on ordinary/property-only frames. The two extra `[f32;2]` fields add
16 bytes per Node before alignment; the index adds storage for out-of-flow
nodes. This is a correctness cost, not a claimed memory reduction.

Full extraction, selective extraction, local snapshots, hit testing and CPU
raster consume the same normalized origin. Visual translations and scrolling
continue following the render-parent property hierarchy; this does not claim
all CSS fixed/absolute transform and scroll-attachment semantics.

## Validation and remaining work

Rust 1.98.1, Windows, locked/offline workspace builds:

- Retained library: **186 passed**.
- Retained text/WGPU library: **219 passed**.
- Vendored Taffy default library: **177 passed**; distribution feature set:
  **171 passed**.
- Combined text/WGPU library Clippy with `-D warnings`: passed.
- Hello retained-WGPU tests: **11 passed, 1 hardware test ignored** under its
  existing configuration and pre-existing example WIP.
- Flex-only vendored Taffy library check (`std,taffy_tree,flexbox,content_size`):
  passed. Retained-only tests retain the existing unused
  spatial transport field warning. This is not an all-targets Clippy claim.

Twelve added framework tests cover box contributions, content overflow,
measurement/min/max, image ratio, fluid allocation, exact thresholds, nested
queries, sibling producer ordering and unsafe styles. The positioning
regression compares selective and full packets, local resolution, hit tests,
old snapshots, style changes, reparenting and index lifecycle. It also compares
complete CPU raster pixels and explicitly checks the positioned red rectangle.
Three new vendor tests cover shared Flex/Grid/Block sizing, width-sensitive
leaf measurement and absolute/fixed containing-block claims.

The standalone [fixture](performance/2026-11-16-size-containment/browser-reference.html),
[capture script](performance/2026-11-16-size-containment/capture.mjs) and
[recorded JSON](performance/2026-11-16-size-containment/browser-reference.json)
record **23/23** Chromium **145.0.7632.6** geometry checks. They are an independent
small reference, not WPT coverage or GPU pixel validation.

Open: general Row/wrap/aspect-dependent inline query graphs, intrinsic fallback
dimensions, vertical writing, intrinsic min/max, public Grid/calc, Unbounded,
constraint builders and structural adaptation, public local responsive output,
atomic multi-stage failure recovery, complete paint/stacking/OOF semantics and
device rounding. No new GPU pixel qualification, sustained 60/120Hz,
input-to-visible tail latency, effects or CPU performance qualification was
performed here. The full RFC goal remains active.
