# Framework-owned Grid layout contract

Status: experimental numeric Grid API implemented, 2026-11-17. The complete RFC
goal remains active. No frame-time or sustained-frame benefit is claimed.

## Public model

`LayoutStyle::grid(GridStyle)` selects `LayoutDisplay::Grid`. Tracks, placement
and alignment use framework-owned types; application code needs no Taffy type.
Flex and Grid containers can be items in one another and use the same persistent
layout tree, typed style property, source transactions, keyed scopes and frame
publication. The renderer continues consuming layout output without a Grid mode.

```rust,ignore
let columns = GridStyle::default()
    .columns(vec![
        GridTrack::px(180.0).into(),
        GridTrack::minmax(GridMinTrack::Px(0.0), GridMaxTrack::Fr(1.0)).into(),
    ])
    .auto_rows(vec![GridTrack::auto()]);
let page = LayoutStyle::grid(columns)
    .size(LayoutDimension::Percent(1.0), LayoutDimension::Auto)
    .gap(LayoutLength::Px(12.0), LayoutLength::Px(16.0));
let header = LayoutStyle::row()
    .grid_column(GridPlacement::Line(1), GridPlacement::Line(-1));
let cards = GridStyle::default().columns(vec![GridTemplate::repeat(
    GridRepetition::AutoFit,
    vec![GridTrack::minmax(GridMinTrack::Px(160.0), GridMaxTrack::Fr(1.0))],
)]);
```

Track minimum: Px, Percent, Auto, MinContent, MaxContent. Track maximum adds Fr
and FitContentPx/Percent. Separate min/max types exclude fractional minima.
Convenience `GridTrack::fr(n)` has CSS automatic minimum; `minmax(Px(0), Fr(n))`
allows content to overflow a smaller allocation. Fit-content is a whole track
function with Auto minimum; incompatible manually assembled bounds reject.

Templates contain single tracks or flat repeated fragments: Count, AutoFill,
AutoFit. Implicit row/column patterns and Row/Column/Dense flow share the normal
kernel auto-placement algorithm. Item placement accepts Auto, one-based signed
Line, positive Span and separate start/end per axis. Negative lines count from
the explicit grid's end. CSS conflict normalization and the kernel's finite
track limits apply. No named lines/areas, subgrid, masonry or CSS parser yet.

`align_items`/`align_self` are the Grid block-axis controls. New
`justify_items`/`justify_self` use the same framework alignment/safety types for
the inline axis. Content alignment and physical gaps are shared. Flex ignores
the Grid item controls. Flex-axis `main_cross_gap` is rejected on Grid; callers
must use physical row/column gaps. Public enum exhaustiveness and full struct
literals need adaptation; Default-based constructors keep their prior values.
Horizontal writing-mode and the kernel's supported alignment/baseline subset
remain the scope, not complete CSS alignment conformance.

## Integration and dependency boundaries

Track edits, span/order changes and Grid/Flex display changes go through the
normal typed `layout_style` sink. P0 equality/coalescing, validation rollback,
selective geometry/full structural recovery, hit geometry, old packets and GPU
leases retain their existing contracts. Stable framework order is used for
Grid placement and paint while logical child order remains distinct. Keyed
insert/remove/reorder preserves existing scope/state identity.

A Grid host with externally proved inline allocation can run an ordinary inline
builder to construct its items; content heights still size the Grid. Fixed-width
or full Size-contained Grid query hosts use existing safe query boundaries and
captured sessions. The shared proof accepts Grid as a host, not a general Grid
parent edge. Ordinary item queries/builders inside content-dependent Grid tracks
remain rejected. Track dependency analysis and custom item-style access to Grid
placement are future work; this does not silently inherit Row-only proofs.

```mermaid
flowchart LR
    A[Owned GridStyle and item placements] --> B[Validate typed LayoutStyle]
    B --> C[Existing atomic source or keyed edit]
    C --> D[Persistent project-owned Taffy Grid]
    D --> E[Shared LayoutResult and geometry publication]
    E --> F[Existing CPU or GPU frame consumption]
```

## Kernel correctness fix

Independent browser references exposed48 mismatches among1,152cases. A
fit-content track with an explicit zero border-box item minimum could use a
minimum contribution below that item's padding+border, shifting following
columns. Minimum contribution is an outer border-box extent: the project-owned
Taffy fork now floors it at the resolved padding+border sum on the relevant axis.
This applies to explicit and automatic zero minimums without changing content
measurement or introducing a special Grid shape. A standalone9-combination
padding/border regression and the complete browser matrix cover it. All1,152
browser cases match after the fix; no cases were excluded or widened past0.06px.

## ADR and costs

Decision: expose owned Grid data over the existing persistent kernel and share
normal transaction/render output. Reusing raw Taffy types would leave the public
layout boundary incomplete; implementing a separate equal-column solver would
lose intrinsic, minmax, spans, implicit tracks and ordering semantics.

`LayoutStyle.grid` is an optional `Arc<GridStyle>`. Cloning styles shares the
immutable numeric track snapshot; Flex defaults carry no track vectors or
allocation. Explicit mutation uses a replacement snapshot or Arc copy-on-write.
Equality is value-based, retaining same-value P0 even for a new equivalent Arc.
Lowering validates/scans and allocates engine track vectors; existing Taffy Style
clones still copy its Grid vectors, and private full-tree candidates/previews
remain. Alignment and placements add inline public fields. Actual ABI bytes,
peak memory, dense Grid resize cost and wall time were not profiled. Do not infer
performance from API reuse, cache tests or finite geometry/pixel checks.

Auto-repeat grammar errors, zero repeats/empty fragments, nonfinite/negative
numbers and zero lines/spans reject before state mutation. Broad CSS/cyclic
percentages, Grid last-baseline/vertical writing, named tracks/areas, general Grid
builder/query proofs, independent host presentation, CPU smooth complete-frame
delivery, modern desktop heavy-effects120Hz and global cache/resource budgets
remain open. See [actual evidence](performance/2026-11-17-public-grid/README.md).
