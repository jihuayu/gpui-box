# Two-axis allocation builders under ordinary Flex

Status: experimental Rust framework capability, 2026-11-18. Full RFC remains
active. This extends the existing allocation_builder and LayoutBuildKind::Allocation;
it adds no new public input type, source schema, Flex solver or rendering path.

## Purpose and actual input

A component may choose structure from the space actually assigned by its
parent. Existing inline_allocation_builder supports common width-dependent
content. Full allocation_builder previously required independent custom parent
edges or a Size+Layout containment cut. Ordinary desktop Row/Column panes with
known width and height could not use it even when allocation was independent of
the constructed content.

This stage adds a proof for ordinary Flex chains and reads the kernel's actual
final incoming LayoutInput. It preserves sizing_mode, known_size,
known_size_is_definite, hard bounds, soft available space and percentage bases.
Known size is border-box allocation, not content size, viewport size or a hard
interval. For a typical flex item both dimensions are known while hard bounds
remain unbounded and sizing mode is Content. Root input retains its separate
external Inherent contract.

```rust,ignore
edit.allocation_builder(pane, |input, edit| {
    let width = input.known_size.width.ok_or(Error::UnsafeConstraintBuilder)?;
    let height = input.known_size.height.ok_or(Error::UnsafeConstraintBuilder)?;
    build_keyed_pane(edit, width, height)
})?;
```

The same builder remains available to once-only LayoutBuildPlan and persistent
LayoutProducer registrations. It runs once, outside intrinsic measurement.
Changing content can overflow its assigned box; overflow clipping, scrolling,
text wrapping and hit publication remain the ordinary kernel/frame contracts.
No containment is silently inserted into authored styles.

## Dependency proof

Existing independent-parent and containment proofs are tried first, preserving
their broader outer-ancestor domain and loose/content-sized inner slots. The
new proof is additional:

1. The host-to-root chain uses in-flow ordinary Flex and no custom parent
   algorithm on that chain. Both root axes use numeric preferred size, or
   percentage/linear Calc/Stretch with a live definite external axis or tight
   root bounds. Auto/intrinsic outer axes do not qualify.
2. Every visible in-flow item on each ancestor edge has a numeric main basis
   (or Auto basis with numeric preferred main size), an explicit numeric main
   minimum, and Auto/numeric limits. Percentages and linear Calc are evaluated
   by normal layout against the proven definite parent sizes. Auto minima,
   intrinsic keywords, Content basis and aspect-ratio feedback reject.
3. In NoWrap a child's cross axis is numeric or stretches to the parent's known
   single-line cross size, without cross-axis auto margins. Normal and Stretch
   alignment use their existing Flex semantics. All four Flex directions work.
4. In Wrap/WrapReverse every visible in-flow item has numeric cross size.
   First/last baseline alignment rejects: content baseline ascent/descent can
   enlarge a line even when item border boxes are fixed. Line formation and
   distribution otherwise remain the normal Flex algorithm. RTL, insets,
   gaps, box sizing and reserved scrollbar gutters use existing calculations.

This proves a dependency direction, not numeric convergence. Full ancestor
styles, ordered direct-child identities and effective item styles are observed.
Image-derived ratio/replaced metadata participates in the latter. After the
callback, replacing a sibling policy, changing order/ancestry or invalidating
the proof rejects even when this scene's final numeric allocation is equal.
Host descendants may change because the proof cuts their size contribution
without adding an authored containment boundary.

The frame environment is captured before proof construction; no stale previous
window size is used as an external anchor. Source-responsive changes settle
before the builder and are revalidated with the normal final observations.

## Atomicity, cache and costs

The existing opt-in tracked input records now also serve this full-axis proof.
The private preview opts in before layout. The final ordinary candidate opts in
for every recorded full/inline input, invokes exact cache/layout validation and
compares complete final input plus structural dependencies before adoption.
Temporary input records retire at the transaction boundary. A cached old frame
cannot satisfy a new observation without the normal input recording pass.

Source values, keyed lifecycle, producers, layout and complete output still
adopt together. Failure retains old sources/stats/frames; old glyph/paint
packets remain usable after owner retirement. Invocation-local caller effects
remain subject to the existing builder side-effect contract, outside rollback.

Costs remain substantial: proof walks ancestry and sibling lists, retains
immutable style/identity observations, uses the ordinary private preview plus
final candidate, and runs final complete geometry checks. Trying the existing
proof before this branch can repeat ancestry work. The underlying tracked-input
and exact-cache machinery already exists; no new per-node kernel/GPU record or
vendor algorithm change is introduced. This is capability work with no claimed
FPS, latency or memory improvement. "99%" remains figurative user direction.

## Evidence and open scope

See [tests and reproduction](performance/2026-11-18-full-flex-allocation/README.md).
Numeric direct Flex references and real Noto controls cover ordinary panes,
nested Row→Column, automatic producer source updates/resize, all directions,
wrap/reverse, RTL, percentage sizing/insets, gutters, rollback and immutable
frame retirement. Hardware tests compare complete output pixels.

General content/intrinsic/ratio feedback, automatically measured wrapped cross
sizes, Grid/Block full-axis allocation proofs, mixed custom/ordinary edges
outside the existing containment domain and complete RenderBox remain open.
Independent presentation, component/source integration and resource budgets
are separate active work. Finite correctness cases do not prove native fluidity
or sustained GPU/CPU cadence.
