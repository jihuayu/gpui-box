# Hard parent box constraints

Updated 2026-11-16 on `codex/rendering-performance-research`. This extends
[independent layout and paint inputs](RETAINED_UNBOUNDED_LAYOUT.md) and
[atomic responsive evaluation](RETAINED_ATOMIC_RESPONSIVE.md). The full RFC
goal remains active.

## Public contract

`BoxConstraints` is a framework-owned hard border-box interval. It has `min`
and `max` physical sizes, `new([min_w,min_h], [max_w,max_h])`, `tight(size)`,
`loose(max)` and `UNBOUNDED`. Minima are finite and nonnegative. Maxima must
be ordered and nonnegative; positive infinity explicitly means unbounded.
NaN, negative values, infinite minima and inverted intervals return
`Error::InvalidConstraints` before owner mutation. `constrain(size)` validates
and clamps a finite nonnegative size for callers using the same contract.

`LayoutConstraints::new` and `viewport` retain their previous soft available
space semantics. The new `bounds` field defaults to UNBOUNDED;
`with_bounds` attaches a hard interval. Direct struct literals must now supply
the field; constructors are the preferred migration path.

```rust,ignore
let input = LayoutConstraints::new(
    LayoutAvailableSpace::Unbounded,
    LayoutAvailableSpace::Unbounded,
).with_bounds(BoxConstraints::new(
    [80.0, 0.0],
    [80.0, f32::INFINITY],
));
let frame = ui.local_frame_in_viewport(input, [500.0, 300.0])?;
```

This makes the root exactly 80 logical units wide while letting its height
follow content. The paint viewport clips output and does not become a sizing
or percentage basis. The authored LayoutStyle remains unchanged.

The interval applies to the root of each layout invocation, including
`measure_node_with_constraints` on an isolated subtree. Ordinary resolved and
local UiTree/ReactiveUi/LogicalUi publication and public responsive methods
share it. Container queries read actual constrained content-box geometry;
viewport rules continue to read the independent LayoutEnvironment.

## Precedence and layout behavior

Parent bounds are stronger than authored preferred size, style min/max,
aspect ratio and the CSS padding/border size floor. Authored min-over-max
precedence still applies within the parent interval. Margins are outside the
border box and are not subtracted from the hard interval. Hidden roots remain
zero-sized, including under a nonzero minimum.

Available space continues to supply the containing-block percentage basis
for the root's own percentages and insets. For normal layout its wrapping
space is clamped to the hard interval; an Unbounded axis with a finite maximum
can wrap within that maximum. Explicit intrinsic MinContent/MaxContent modes
remain intrinsic queries whose chosen used size is constrained afterward.
Loose maxima alone do not manufacture a definite percentage-height basis.
An equal min/max axis is a definite parent assignment; children can resolve
percentages against the resulting content box. Constraints do not propagate
as identical bounds to every descendant: Flex/Grid/Block still allocate their
children through their existing algorithms.

If padding/border exceeds a hard maximum, the content and positioning area
has a zero floor; the border box does not expand to evade the maximum.
Authored insets are preserved and may overflow. Clipping remains an explicit
paint/overflow decision. This does not implement Flutter's RenderPadding
child-deflation algorithm or replace all CSS box-model behavior.

Used width participates in Flex line collection, shrink/grow and final text
measurement. Intrinsic height and height-limit queries retain the constrained
other-axis width even when authored min-width exceeds it. A shaped paragraph
therefore wraps at the chosen width before its height and baseline publish.
The interval is not a final FramePacket bounds clamp: children are laid out
against the actual constrained box.

This interval constrains logical unrounded layout. The retained owner disables
Taffy rounding. A separate low-level caller enabling Taffy's pixel rounding can
observe a rounded edge outside a fractional bound; this stage does not define
the framework's future device-pixel rounding policy.

## Decision: extend the owned layout kernel

Taffy's soft AvailableSpace and authored min/max cannot express a hard host
interval: CSS permits min-size and padding floors to exceed available space.
Rewriting root style temporarily would mix two different constraint owners,
lose intrinsic semantics and repeatedly dirty ancestors. A synthetic wrapper
would also introduce visible identity, baseline, margin, containing-block and
ordering questions. Clamping only final output would leave wrapping, child
percentages, hit testing and paint inconsistent.

The project-owned fork now carries node-local `SizeBounds` in `LayoutInput`
and exposes `compute_root_layout_with_bounds` and
`compute_layout_with_bounds_and_measure_and_observer`. Root dispatch and the
actual leaf/Flex/Grid/Block/out-of-flow paths combine host bounds with their
style limits before layout. Child calls reset bounds; same-node refinements
preserve them. The high-level Taffy entry validates bounds and node identity
before writes. The low-level entry requires valid bounds, as its documentation
states. Existing entry points pass `SizeBounds::NONE`.

This is an explicit fork API change: complete LayoutInput literals need
`size_bounds`, custom trees must preserve/reset it deliberately, and exhaustive
TaffyError matches must handle `InvalidSizeBounds`. The retained framework
continues to hide Taffy representation behind its own public types.

### Cache and costs

The cache stores one SizeBounds mode per node, rejecting and clearing entries
when that mode changes. This avoids four extra float values in every one of
the ten entry keys. It adds four floats of payload per node cache (16 bytes,
excluding structure alignment) and per LayoutInput, including default-unbounded
nodes. A LayoutConstraints value also gains four floats of bounds payload.
No memory reduction is claimed. Bounds changes may evict useful intrinsic
measure entries; shared paragraph memoization and measure-only tree clone costs
remain. There is no new whole-tree pass, wrapper node or general-purpose user
callback added by the bounds mechanism, and no measured throughput improvement.

LayoutConstraints equality includes bounds, so changing only the interval
invalidates layout while repeated identical inputs reuse output. Paint viewport
changes keep their existing zero-layout behavior. Existing atomic responsive
drafts receive the same interval during private previews and final publication.

## Validation and remaining work

[Evidence and commands](performance/2026-11-16-parent-constraints/README.md)
cover tight/loose/conflicting limits, extreme padding, aspect ratio,
Row/Column wrap and grow, percentages, hidden nodes, invalid inputs,
constraint-only cache changes, independent measurement, old snapshots,
resolved/local CPU pixels and hit testing, computed responsive previews and
bundled Noto paragraph wrapping at widths 50/90/140. Fork tests run actual
Flex, Grid, Block, ContentBox and out-of-flow roots and check cache separation.
The public Grid API remains absent even though the compatibility kernel is
tested.

This stage provides root/isolated-subtree host bounds. The subsequent
[nested constraint-box stage](RETAINED_NESTED_CONSTRAINT_BOX.md) adds an explicit
single-child parent protocol with tight/loose content allocation. The subsequent
[builder stage](RETAINED_CONSTRAINT_BUILDERS.md) adds Size-contained parent-input
construction and atomic keyed frames. General content-sized builders/custom
layout and a full Flutter RenderBox API remain open. ContainerQuery still means actual
size, not the supplied max width.
Broader Flex/percentage/baseline conformance, native rounding, effects,
sustained 60/120Hz, full-chain tail latency, newer desktop GPU qualification
and CPU rendering smoothness are not established by these correctness checks.
