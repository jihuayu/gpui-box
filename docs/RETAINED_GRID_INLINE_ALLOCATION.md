# Content-independent Grid inline allocation

Status: experimental framework capability implemented, 2026-11-17. Extends
[public Grid](RETAINED_PUBLIC_GRID.md) and the shared ordinary inline builder /
ContainerInline dependency proof. Full RFC goal remains active. No timing,
peak-memory or sustained-frame gain is claimed.

## Public behavior

Existing `inline_allocation_builder` and uncontained `ContainerInline` can now
read ordinary Grid item widths when every possible column allocation is
independent of descendant content and the complete parent chain supplies a
definite inline extent from a live external root anchor. This composes with
nested Grids, ordinary Column and Row/percentage-basis descendants. No new public
type, method, trust flag, alternate Grid solver or Taffy pass is introduced.

```rust,ignore
let track = GridTrack::minmax(GridMinTrack::Px(0.0), GridMaxTrack::Fr(1.0));
let grid = LayoutStyle::grid(
    GridStyle::default()
        .columns(vec![track.into(), track.into()])
        .auto_columns(vec![track]),
);
// Under a definite independently allocated Grid width, an ordinary stretched
// Column item can use inline_allocation_builder / ContainerInline.
```

All reachable explicit and implicit columns need finite definite minima
(Px/Percent/owned Calc) and finite definite or fractional maxima. Explicit
implicit-column patterns are required even if a current placement happens not
to use them; default Auto cannot prove independence for future reachable spans.
Single tracks, Count and AutoFit/AutoFill column fragments qualify. The normal
kernel computes remaining-space fr values, min/max floors, spans, negative lines,
dense flow, RTL, gutters and line placement. Auto/fr convenience tracks with an
automatic minimum and all intrinsic maximum tracks reject.

Explicit row counts are fixed by Singles/Count; row sizing can remain Auto and
content-driven. Auto-repeated rows reject because content-derived height can
change row count and therefore horizontal placement under column flow. Named
areas/lines and named item placement remain outside the proof. All visible
in-flow direct siblings and every reachable authored branch must have a numeric
preferred inline width or supported Auto+Normal/Stretch assignment, Auto/numeric
inline limits and no effective aspect ratio. Unresolved shrink-wrap widths,
intrinsic limits and image-derived ratios reject. Unsupported nondefault
stretch safety combinations reject conservatively rather than infer stretching
that the current kernel does not perform.

The separate definiteness predicate now crosses proved Grid item assignments;
independence is still checked by the complete proof. A fixed measured result
alone does not prove either property. Numeric root width, normal Definite input
or hard-tight root width supplies the existing live anchor. Paint/environment
viewports and previous shrink-wrap geometry cannot replace it. The older
unconditional used-width proof does not acquire this conditional Grid permission.
Custom/box-protocol percentage paths retain their existing conservative scope.

## Publication and scheduling

Grid observations include the full parent Style (tracks/flow/row-count policy,
alignment and insets), layout-ordered direct children and every effective item
Style, including decoded-image metadata. Changes after a builder/query reads its
input reject at final validation even if the particular width remains equal.
Builder descendant content can change heights; it cannot change observed parent
placement inputs. Sources, keyed output, tree geometry, effects and complete
local/resolved frame publish through the existing atomic boundary.

Responsive DAG edges now include sibling producers below Grid ancestors and
all authored Grid alternatives. Later-registered sibling placement rules settle
before width consumers. A computed query source feeding a sibling allocation
style forms a cycle even if its current output is equal. Captured scope-owned
sessions automatically resettle ordinary sibling source transactions. No user
callback is replayed and no threshold loop seeks a fixed point.

```mermaid
flowchart LR
    A[Live definite independent parent inline chain] --> B[Definite column minima and definite or fr maxima]
    B --> C[Normal Grid placement and column allocation]
    C --> D[Item incoming width or usable content query]
    D --> E[Construct or select descendant content]
    E --> F[Content-driven rows and complete geometry]
    F --> G[Final tracks sibling styles order and source validation]
    G --> H[Atomic complete frame publication]
```

## ADR and cost

Decision: add a shared dependency proof over the existing Grid algorithm.
Always rejecting Grid items unnecessarily prevents content-driven cards and
responsive descendants under fixed/fractional columns. Trusting current finite
width loses implicit-track, auto-placement and sibling dependencies. Duplicating
equal-column arithmetic would not preserve spans/minmax/dense semantics.

Proof scans inspect all ancestor alternatives and direct siblings, and the
definiteness walk may repeat across nested ancestors/hosts/plans. Observations
copy full Grid parent Style and effective sibling Styles/order; engine track
vectors are copied by Style clone. The parent Style is optionally boxed so
existing Row/custom records carry only an extra optional pointer, while Grid
observations allocate the owned Style separately. This adds temporary bytes and
comparison work. Existing full candidate/preview costs remain. No new persistent
node/style/kernel/GPU/frame field or Taffy layout pass; no payload/peak/wall-time
measurement or complexity improvement is claimed.

Follow-up: [shared layout styles](RETAINED_SHARED_LAYOUT_STYLES.md) replaces the
boxed Grid style and inline sibling style copies with shared immutable handles,
also covering full-axis allocator observations and private tree/kernel clones.
Final comparisons remain value-based and proof/order scans remain. The linked
stage has its own CPU measurements; the original browser/pixel evidence here
does not imply any timing or peak-memory gain.

General intrinsic/automatic columns, content/ratio feedback, implicit-Auto
reachability analysis, auto-repeated rows, named placement, full-axis builder
proofs, custom Grid placement input views, automatic builder/query scheduling,
complete RenderBox, incremental candidates/indexes/global budgets and broad
CSS/vertical/baseline conformance remain open. Modern desktop GPU heavy-effects,
CPU sustained complete-frame smoothness, native visible presentation/input tails
and sustained60/120Hz remain separate acceptance. See
[evidence](performance/2026-11-17-grid-inline-allocation/README.md).
