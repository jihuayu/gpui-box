# Nested parent constraint boxes

Updated 2026-11-16 on `codex/rendering-performance-research`. This builds on
[hard root bounds](RETAINED_PARENT_CONSTRAINTS.md). The full RFC goal remains
active. ConstraintBuilder and layout-dependent structural construction are
still separate, unfinished capabilities.

## Public protocol

`LayoutStyle::constraint_box(width_fit, height_fit)` selects a framework-owned
single-child layout algorithm through `LayoutDisplay::ConstraintBox`.
`ConstraintFit::{Loose,Tight}` controls each physical axis. A finite content
axis supplies a hard child maximum; Loose uses a zero minimum and Tight assigns
the axis exactly. An unbounded axis stays loose even under Tight. Explicit
MinContent and MaxContent queries remain intrinsic modes, rather than becoming
infinite finite allocations.

```rust,ignore
let host = ui.append_render_layout(
    scope,
    parent,
    LayoutStyle::constraint_box(ConstraintFit::Tight, ConstraintFit::Loose)
        .size(LayoutDimension::Px(100.0), LayoutDimension::Auto)
        .padding(LayoutEdges::all(LayoutLength::Px(10.0))),
    0,
)?;
let row = ui.append_render_layout(
    scope,
    host,
    LayoutStyle::row().flex_wrap(LayoutFlexWrap::Wrap),
    0,
)?;
// row receives exactly 80 units of width. Its height follows its content.
// Normal Flex children inside row retain their CSS allocation semantics.
```

The boundary is a real retained render/layout node with normal identity,
material and visual properties. It supports zero or one direct in-flow child.
It can sit inside a Flex container, where the parent still owns sibling
grow/shrink allocation and placement; the boundary then passes its allocated
content constraints to its own child. Boundaries can nest. No synthetic extra
nodes, temporary style rewrites, arbitrary callbacks or final paint-only clamps
are used.

Padding, borders and scrollbar gutters deflate the content interval with a
zero floor. A finite parent hard bound still overrides padding floors and
authored child min/max/aspect suggestions. Wrapper preferred/min/max,
ContentBox sizing and intrinsic limits use the existing common box model.
Child preferred intrinsic keywords are resolved before selecting a constrained
used size, keeping the other-axis constraint for text wrapping.

Width is selected before dependent height. When the wrapper initially has an
unknown width, one additional width-constrained measurement supplies the final
inline percentage basis and wrapping width before choosing auto height. This
is a fixed protocol order; it is not iteration until a layout stabilizes.
An auto/content-derived height does not become a percentage basis solely
because its used size is finite. An explicitly Tight finite child allocation
does establish a definite assigned dimension. Cyclic percentage sizing and
full browser equivalence are not claimed.

## Placement and compatibility

The direct child is placed at content start: left/top in LTR, right/top in RTL.
The parent owns placement, so this edge deliberately does not consume child
margins, flex item alignment/grow/shrink/order, auto margins or relative insets.
Use a normal Flex child for those rules inside the content area. The wrapper's
own item metadata remains available to its CSS parent. A direct absolute/fixed
child is rejected; descendants inside the child's normal CSS tree can still
be out of flow. Their candidates bubble through the boundary to their actual
containing block with normal render-parent coordinate publication.

The boundary propagates the child's first/last horizontal layout baselines
with its vertical content offset, unless layout containment suppresses them.
The public LayoutResult still exposes only the first baseline. Hidden children
have zero layout, including padding/border/gutter and baseline. Scrollable
overflow uses the existing child overflow/containment rules. Paint clipping,
scroll, isolated opacity, hit testing and old immutable frames keep the same
geometry ownership as ordinary nodes.

Framework append, property transactions, structural transactions and unified
edits validate arity and direct child position before publication. A box cannot
also be a measured leaf/image/text node. `Error::InvalidConstraintBox` rejects
these inputs without publishing partial source, tree or frame changes.
`LogicalEdit::append_layout` now constructs framework layout nodes in the same
keyed logical/render/source draft. A valid edit can remove an extra child and
switch the parent protocol atomically.

Typed `Property::layout_style` updates change the policy through the normal P4
route; equal final values retain the existing no-op behavior. Resolved, local,
independent measure and responsive preview paths all dispatch the same kernel.
Fixed-inline and full-size-contained query hosts may be constraint boxes.
The existing fluid Column proof requires a real Flex parent, so unused column
style fields on a box cannot accidentally authorize an unsafe container query.
General fluid box query proofs are not implemented.

## Decision: extend the owned kernel with an explicit algorithm

Taffy remains suitable as a persistent CSS Flex/Grid/Block algorithm kernel,
not as the owner of the entire framework protocol. Its original soft available
space and CSS item rules cannot express this edge contract. Root-only hard
bounds also cannot transfer a nested Flex allocation into a constrained child.
Reusing Flex and temporarily rewriting child min/max would mix style authorship
with parent allocation, while a final output clamp would leave wrap and
descendant percentages wrong. An explicit non-CSS algorithm composes the two
models while keeping the existing Flex behavior.

The project-owned fork adds `Display::ConstraintBox`, per-axis `ConstraintFit`
and `compute_constraint_box_layout`. The generic low-level algorithm requires
valid arity and an in-flow direct child. High-level Taffy setters retain their
draft-like behavior, permitting multi-node changes in any order; the layout
entry validates indexed boxes in the queried subtree before observer writes.
The retained owner additionally validates at its transaction boundary.

A sparse Taffy index tracks actual box nodes, is cloned/cleared/retired with the
tree and starts without preallocated capacity. Validation costs O(B × depth)
for B indexed boundaries, including ancestry checks for unrelated detached
boxes; ordinary nodes are not enumerated by this check. It adds tree-level map
bookkeeping and sparse entries. `Display` grows from one to two bytes on the
tested 64-bit build. String Style is 560 bytes and Arc-str Style is 528 bytes:
without flexbox_balance each grows by eight bytes (552/520 previously), while
the balance-enabled configuration fits its existing size. No memory reduction
or throughput improvement is claimed.

Unknown-width boxes can perform an extra measure pass. Intrinsic child probes
can switch cache mode/bounds and evict useful entries; final layout and real
paragraph memoization still have their normal costs. The implementation
materializes a temporary common Style to reuse the existing leaf box model;
it does not retain a second style or callback per node. No new timing or
sustained frame-rate result follows from these correctness tests.

This adds public enum variants: downstream exhaustive LayoutDisplay/Display
and Error/TaffyError matches need migration. CSS parsing does not expose a
constraint-box keyword. Low-level custom trees can call the algorithm and must
validate the documented preconditions. Other display algorithms are unchanged.

## Validation and unfinished work

[Evidence](performance/2026-11-16-nested-constraint-box/README.md) covers nested
policies, padding, real Flex allocation, baseline, bounded/unbounded and
intrinsic queries, hard overflow, percentages, OOF candidates, RTL/hidden,
property/topology failure isolation, logical typed bindings, responsive query
proofs, independent measurement, old snapshots and complete CPU pixels.
A real bundled Noto paragraph wraps at allocated widths 50/90/140/50.

Still open: ConstraintBuilder receiving pre-layout parent inputs, conditional
keyed structural adaptation within one atomic frame plan, general custom
layout, broader percentage/cyclic sizing and Flex conformance, device rounding,
general query proofs and incremental planning. This protocol is not a complete
Flutter RenderBox implementation. Full native/GPU/CPU acceptance, effects,
sustained 60/120Hz and input-to-visible tail latency remain open.
