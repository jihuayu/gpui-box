# Content-independent Row allocation for builders and queries

Status: experimental framework capability implemented, 2026-11-17. The complete
RFC goal remains active. Extends [ordinary inline builders](RETAINED_INLINE_ALLOCATION_BUILDERS.md)
and [ordinary content-width queries](RETAINED_ORDINARY_CONTAINER_QUERIES.md).
No frame-time, tail-latency or sustained-frame improvement is claimed.

## Public behavior

Existing inline builders and ContainerInline rules now cross ordinary Row and
RowReverse edges, including Wrap and WrapReverse, when **all direct in-flow
items have content-independent horizontal allocation inputs** and the parent's
own inline allocation has a complete independent proof. A common supported case
is flexible columns with `flex_basis(Px(0))`, `min_size(Px(0), Auto)` and numeric
grow factors. Content, real paragraph height and following rows can change
without retroactively changing the already assigned column width.

```rust,ignore
let column = LayoutStyle::column()
    .flex_basis(LayoutDimension::Px(0.0))
    .flex_grow(1.0)
    .min_size(LayoutDimension::Px(0.0), LayoutDimension::Auto);
// Under an independently sized Row:
let host = ui.append_render_layout(scope, row, column, 0)?;
// Existing inline_allocation_builder and ContainerInline use the same proof.
```

There is no new public style field, builder method, query enum or opt-in trust
flag. Existing Tight ConstraintBox and independent custom width phases compose
with these edges. Incoming builder fields retain their border-box meaning;
ContainerInline still reads final usable content width. The external-inline
anchor guard continues rejecting unanchored percentage/stretch roots in
Unbounded/MinContent/MaxContent input.

## Proof from Flex allocation inputs

For every visible in-flow direct item and every reachable authored alternative:

- Flex basis is a finite nonnegative pixel value, or Auto with a finite
  nonnegative pixel preferred width.
- Minimum width is explicitly a finite nonnegative pixel value, avoiding the
  content-based automatic minimum. Maximum width is Auto or a finite nonnegative
  pixel value.
- Preferred width is not an intrinsic sizing keyword; effective aspect ratio is
  absent. The proof inspects the same effective style used by layout, including
  a decoded image's derived ratio, rather than only the authored style.

Under an independently allocated parent width, numeric main basis/min/max and
width-resolved insets/margins determine hypothetical main sizes, line membership
and grow/shrink allocation without measuring descendant content. Cross sizes
still use normal content layout. Wrap can change line height and placement, but
not its main allocation inputs. Parent aspect feedback and Column wrapping
remain outside this proof.

Content/auto/percentage basis, automatic or intrinsic main minima, intrinsic main
maxima, intrinsic preferred widths and effective ratios reject conservatively.
This is not a claim that every excluded case is impossible; broader percentage
definiteness and ratio/automatic-min proofs remain work. Hidden/out-of-flow items
are skipped for allocation proof as in Flex, but are still in conservative
structural observations when present. The layout algorithm and output semantics
are unchanged; there is no special two-column solver or cached-width guess.

## Sibling dependencies and publication

Row allocation uses every direct sibling's item inputs. Shared
InlineDependencies now capture each Row ancestor's layout-ordered child list and
all effective child styles in addition to existing custom algorithm snapshots.
Changing a sibling input or order after builder/query observation rejects at
final validation even if allocation happens to remain numerically equal.
Provisional topology, removal/reordering and decoded-image ratio changes cannot
silently invalidate a prior observation. Callback construction below the
observed host remains free to change because it is not a direct allocation item
of that ancestor.

Responsive DAG construction includes direct sibling style producers for every
Row ancestor and every authored Row alternative. Sibling grow/basis/order rules
execute before host-width consumers even when registered later. A computed sink
from a host-query source back to an allocation sibling forms a dependency cycle
and rejects. Current equal values do not remove that edge. Ordinary Column and
existing custom phase rules keep their established dependencies.

Source, binding, keyed state/effect, layout and complete local/resolved frame
publication keep the existing atomic boundary. Proof or final dependency failure
leaves published state unchanged. Ordinary next-frame cache reuse and P0 typed
value semantics remain; calling a builder still has its existing preview and
forced observation costs. There is no callback replay or iterative solver.

```mermaid
flowchart LR
    A[Independent parent inline allocation] --> B[All Row item numeric bases and limits]
    B --> C[Normal Flex lines and grow/shrink widths]
    C --> D[Builder incoming width or query content width]
    D --> E[Construct or select descendant content]
    E --> F[Normal dependent heights and complete output]
    F --> G[Final sibling styles order and input validation]
    G --> H[Atomic publication]
```

## ADR and costs

Decision: extend the shared dependency proof over content-independent Row inputs,
including all sibling alternatives and final observations. Reuse normal Flex
allocation and the existing source/query DAG. The public API does not expose the
layout kernel or require a special split-pane component.

Always rejecting Row forces unnecessary containment/custom wrappers for ordinary
application layouts. Trusting a fixed current host width ignores sibling grow,
shrink, line membership and automatic minima. Specializing to a fixed item count
would not establish the general dependency. Replacing layout with guessed equal
fractions would diverge on padding/min/max/wrap. Those alternatives were rejected.

Costs: proof scans now visit direct items and reachable sibling style branches at
Row ancestors; observation records copy effective Style values and ordered lists.
This can be O(sum of ancestor sibling counts), with repeated scans across hosts
and rule graph construction. Final comparisons also inspect these records, and
full private candidate/previews remain. No persistent node/style/cache/GPU/frame
field, extra Taffy layout pass or new kernel algorithm is introduced. Memory
payload/peak allocations and wall time were not profiled. No speedup or complexity
improvement follows from adding capability.

See [evidence and reproduction](performance/2026-11-17-row-inline-allocation/README.md).
General content-dependent bases/minima, percentage/ratio/intrinsic/writing-mode
proofs, full-axis feedback, automatic builder/query scheduling, complete RenderBox,
incremental candidates/proof indexes and global budgets remain open. Broader
Flex/CSS and primitive/effect/native lifecycle, modern desktop heavy-effects GPU,
CPU smooth complete-frame delivery, sustained60/120Hz and visible input tails
still require independent acceptance.
