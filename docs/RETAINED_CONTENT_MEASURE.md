# Owned constraint-dependent content measurement

Follow-up 2026-11-18: [owned UI layout resources](RETAINED_OWNED_LAYOUT_RESOURCES.md)
adds UiContent::Measure and UiResources::measure to the owned source boundary,
with ordinary atomic layout, resource-kind checks and isolated document queries.
This supplies measurement for a leaf, not a new paint primitive.

Follow-up: [content axis demand](RETAINED_CONTENT_AXIS_MEASUREMENT.md) adds
requested_axis to callback inputs, preserves per-probe intrinsic demand in the
shared leaf box model and keeps complete final/baseline metrics authoritative.
Complete input literals need the new field; ignoring demand keeps old behavior.

Status: experimental framework protocol, 2026-11-17. This extends fixed leaf
metrics and pure parent layout. Full RenderBox/intrinsic semantics and the
complete RFC goal remain open.

## Public contract

```rust,ignore
impl ContentMeasure for MyOwnedContent {
    fn measure(&self, input: ContentMeasureInput)
        -> Result<ContentMeasureOutput, Error>
    {
        // Only immutable captured content and the supplied soft content space.
        // Return natural content extent and first/last horizontal baselines.
    }
}
let snapshot = MeasureSnapshot::Content(ContentMeasureSnapshot::new(content));
transaction.measure(node, Some(snapshot));
```

`ContentMeasureInput::available` contains the resolved **soft content space**,
after padding/border/scrollbar-gutter deflation. Each axis distinguishes Definite,
normal Unbounded, MinContent and MaxContent. A definite opportunity may control
wrapping, but it is neither a hard maximum nor an instruction to fill that size.
The callback returns natural content width/height, both finite/nonnegative, and
optional finite signed content-box baselines. Natural content may exceed the
used host height; fixed intrinsic snapshots retain their older validation.

The callback is content measurement, not a second box model. It cannot read
authored styles, arbitrary ancestors/children, parent percentage bases or final
paint geometry. It does not choose padding, flex placement or child construction.
Those remain the owned kernel's responsibilities. This protocol deliberately
does not claim Flutter RenderBox's full known-size/constraint/intrinsic surface.
An arbitrary content algorithm must define meaningful answers for both intrinsic
modes; the framework does not infer them from a normal unbounded answer.

The snapshot owns `Debug + Send + Sync + 'static` data behind an Arc. Clone
preserves identity; changing any content/configuration requires a new handle.
The callback may run repeatedly or not at all on a cache hit. Mutable captures,
globals or invocation-dependent answers violate purity. The type system does
not freeze interior mutability. Observable user effects, panics, diagnostic
counters and shared memo warming are outside rollback guarantees.

## Sizing, metrics and failure boundary

The adapter reuses `compute_leaf_layout` for preferred/min/max/intrinsic sizes,
percentage box model, aspect ratio, hard border-box intervals, containment and
overflow. It evaluates the pure leaf arithmetic without the size-only early
return, so known-size queries still obtain actual content metrics. No Taffy
tree geometry is published by this arithmetic.

If incoming inline size is unknown, one initial measurement selects the used
width, then a second leaf pass measures dependent height at that selected width.
There is no fixed-point iteration. The parent's block opportunity remains
unchanged; a content-derived auto height does not become another hard callback
input. The final callback of the selected pass supplies overflow and first/last
metrics; host top padding/border is added once, and layout containment suppresses
baseline export. Negative deflated intrinsic opportunities clamp to zero.
Nonfinite definite opportunities reject before user code runs.

```mermaid
flowchart LR
    A[Owned measure property] --> B[Exact kernel invocation]
    B --> C[Common leaf box model]
    C --> D[Select inline size]
    D --> E[Remeasure dependent content]
    E --> F[Validate metrics and composed output]
    F --> G[Private layout candidate]
    G --> H[Publish complete valid geometry and frame]
```

Delegate errors, nonfinite/negative sizes, nonfinite baselines or nonfinite
composed output produce `InvalidMeasure`. The first measure error stays sticky
through nested parent queries and takes precedence over a secondary parent-plan
error caused by its placeholder output. Failed provisional output/caches never
replace the live layout tree. Explicit size/baseline/full-root queries use their
existing private clones and also return this error.

`Transaction::measure` and `Property::measure` use the existing P0 equality and
final-value coalescing. A new handle selects Relayout and invalidates dependent
ancestors; color-only changes do not call content measurement. Content remains a
leaf, with the same image/text ownership conflicts and measured-node arity checks.
Snapshot installation validates the handle without executing user layout code.
`MeasureSnapshot` gains an enum variant; exhaustive downstream matches need an
additional arm. Existing fixed snapshot and parent-layout struct literals keep
their syntax. None of these experimental Rust types defines a stable binary or
cross-language ABI.

Therefore a plain successful transaction can install a callback that later
fails layout; that transaction remains committed. `edit_frame`/`edit_local_frame`
are the atomic source/tree/layout/frame boundaries and roll back such failures.

## Cache and lifetime decision

Ordinary CSS size cache equivalences are too weak for arbitrary content modes.
The vendored kernel adds `set_exact_measure(node, enabled)` backed by a sparse
authored-capability index. Presence enables the existing complete-input,
complete-output bounded measurement cache and final-input guard throughout the
invocation, including ancestors. Definite dimensions cannot mask different soft
space, intrinsic modes or other input metadata. Hidden layout preserves the
authored capability; replacement/removal/clear/generation reuse retire it.

The framework indexes content-measured nodes so fallible normal layout uses a
private owned Taffy clone and staged observer journal. Both public direct removal
and structural removal retire the framework index. Old render packets carry
only resolved paint/resources, not measurement delegates; they remain consumable
after UI and content owners retire.

## ADR: extend content measurement, keep one box model

**Decision:** expose an immutable content-only measurement trait, integrate it
through existing measure properties, and preserve complete cache keys plus an
atomic final layout boundary. Width selection and all box sizing reuse the
project-owned layout kernel.

**Alternatives:** adding one enum variant per application content type would
keep the framework closed to new measurement algorithms. Arbitrary UI closures
inside layout would make caching, lifecycle and failure effects unbounded.
Returning a used border box from a content callback would create competing
padding/constraints/overflow rules. Reusing ordinary size caches could merge
distinct valid queries and lose baselines. A new broad RenderBox API is still
needed for other responsibilities and is not silently substituted by this trait.

**Costs:** Arc allocation/dispatch for authored custom content; a framework
BTreeSet plus kernel sparse capability entry per participating leaf. Existing
exact caches may allocate entries for visited ancestors/unrelated siblings during
that invocation. Selected-width passes and intrinsic-limit probes repeat work;
dirty fallible layouts and public queries still clone the owned tree. There is
no new persistent callback memo, byte budget, GPU record or FramePacket field.
No speedup, lower memory, tail-latency or sustained-frame claim is made.

## Evidence and limits

See [reproduction record](performance/2026-11-17-content-measure/README.md).
Finite client wrapping-cell controls compare host/footer geometry and all CPU
pixels with an independently authored ordinary full Flex tree. These are
same-kernel controls, not independent browser conformance. Actual bundled Noto
paragraph metrics exercise the trait without character-width approximation;
that test draws measured colored boxes, not glyphs through a new paint protocol.
Existing Paragraph/TextPaint rendering remains unchanged.

Complete RenderBox/intrinsic querying, arbitrary child-style allocation,
uncontained ordinary Flex feedback construction, automatic cross-builder/query
scheduling, incremental candidates/proof/rollback/global budgets, broad Flex /
percentage/cycle/Grid/Block/writing-mode/precision and primitive/effect/native
lifetimes remain open. Modern desktop GPU effects, CPU continuous complete
frames, sustained60/120Hz and display-visible input tails remain separate gates.

The [explicit scrollbar allocation follow-up](RETAINED_SCROLLBAR_GUTTERS.md)
removes the raw-kernel style escape hatch from gutter fixtures and aligns public
content_size, normal paragraph baselines, glyph wrap and image/glyph destinations
with those same deflated content opportunities. Automatic scrollbar prediction
remains separate work; the content callback does not decide bar appearance.
