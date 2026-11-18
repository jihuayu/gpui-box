# Grid item metadata in public parent policies

Status: experimental public layout contract,2026-11-18. The complete RFC goal
remains active. This extends the existing
[child-style view](RETAINED_CHILD_STYLE.md) and
[independent allocation view](RETAINED_ALLOCATION_STYLES.md).

## Contract

`ChildLayoutStyle` adds framework-owned `grid_column: GridAxisPlacement`,
`grid_row: GridAxisPlacement` and `justify_self: LayoutAlignment<LayoutAlignSelf>`.
The same unresolved values are available through `ParentLayoutChildren::style`
in measured/full/baseline phases and `ParentAllocationChildren::style` during
independent allocation. Positive/negative line numbers, Auto, Span and the
alignment's Default/Safe/Unsafe policy are retained. The existing align_self
conversion shares the same helper, including Auto/Normal/first/last baseline.

No item location, resolved track size, child container internals, arbitrary
descendant or renderer type is exposed. The metadata is input to a client policy,
not automatic CSS placement inside every custom parent. Ordinary Grid continues
using the real Taffy algorithm. Custom physical origins and allocation rules
remain the parent's responsibility, including its declared supported subset.
Child style reads do not authorize new builder feedback chains or bypass
installed independent slots.

The fork's `CustomChildStyle` preserves raw owned Grid placements, including
names. It therefore becomes Clone rather than Copy when Grid is enabled. The
framework view remains Copy and translates only the existing numeric public
placement domain. NamedLine/NamedSpan, line0 and span0 reject explicitly with
InvalidParentLayout when the view is read; they are not silently reduced to Auto
or a different span. Existing ordinary low-level Grid behavior is unchanged.
Adding public fields also requires users of complete struct literals to adapt;
there is no stable ABI promise for this experimental interface.

Fork alignment fields use the same feature guards as actual Style:
align_self under Flex/Grid/Block, justify_self under Grid/Block and placements
under Grid. Grid-only and Block-only allocation adapters are no longer blocked
from observing the corresponding alignment by an accidental Flex-only guard.

## Invalidation and publication

These inputs already belong to the authoritative lowered node Style, changed
through ordinary typed styles and transactions. Child writes dirty the child and
ancestors and invalidate parent exact queries using existing mechanisms. Builder
observations retain effective full immutable Styles and child order, so all
new metadata is already covered by final value comparison. A later sibling
placement change must reject even if its assigned width is unchanged. There is
no new per-field dependency tracker, persistent mirror or layout pass.

Failure uses the existing candidate boundary; source values, geometry, revision
and published frame remain unchanged for atomic edit failures. P0 checks remain
value-based. The raw kernel still requires caller isolation for fallible custom
layout; this addition does not make direct low-level writes transactional.

## ADR: complete the item input model rather than duplicate parent data

Context: public normal Grid already accepts numeric placement and inline self
alignment, while custom parent policies could read only Flex item controls and
the common box model. Clients would otherwise duplicate placement in opaque
ParentData and keep two independent configurations consistent.

Decision: extend the existing unresolved child-style model with the same owned
public placement/alignment types and use the existing conservative invalidation
and final observation checks. Preserve names at the kernel boundary; reject
unsupported public conversion. Do not invent a special equal-column runtime
algorithm or expose the complete Taffy Style publicly.

Costs: every style-view return copies/converts three additional fields. The
kernel may clone strings for legacy named placements; the framework rejects
these after conversion begins. No new persistent node/style/frame/GPU field,
track vector or proof state is added. This is not a wall-time, peak-memory,
sustained120Hz or complete RenderBox improvement claim.

## Evidence and remaining scope

See [test record](performance/2026-11-18-grid-item-style-view/README.md).
A small test-only explicit-cell client reads numeric item declarations to assign
slots and positions, compared with full ordinary Grid. This finite client is
not a general Grid solver or CSS conformance test. Real bundled Noto tests
exercise resulting percent-width wrapping and complete glyph pixels against
ordinary Grid. Existing normal suites continue to exercise Flex/measurement/
baseline/typed/keyed/query behavior. Kernel tests preserve names, signed lines,
spans, alignments, clones and owned view lifetime.

General content-dependent Grid allocation proofs, named public styles/subgrid,
automatic cross-builder/query scheduling, complete RenderBox/full-axis feedback,
incremental candidates/indexes/budgets and broad CSS/vertical conformance remain
open. Modern desktop effects, CPU sustained complete-frame smoothness, native
visible presentation, continuous60/120Hz and full input tails remain independent
requirements of the still-active goal.
