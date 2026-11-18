# Two-axis allocation builders across Grid and Flex

Status: experimental Rust framework capability, 2026-11-18. The complete RFC
remains active. This extends the same `allocation_builder`, full Allocation
task and persistent `LayoutProducer` protocol used by
[ordinary Flex](RETAINED_FULL_FLEX_ALLOCATION.md). Source schema remains6;
there is no new public builder, layout solver, vendor or rendering algorithm.

## Ordinary component composition

A Grid card can contain a Flex column with a naturally measured title and a
remaining-space body. The body can choose its structure from its actual assigned
width and height. Flex can also contain a Grid, and Grid can contain another
Grid. The ordinary persistent kernel remains responsible for track sizing,
placement, Flex allocation, text wrapping and complete frame geometry.

```rust,ignore
let fr = GridTrack::minmax(GridMinTrack::Px(0.0), GridMaxTrack::Fr(1.0));
let cards = LayoutStyle::grid(
    GridStyle::default()
        .columns(vec![fr.into(), fr.into()])
        .rows(vec![fr.into()])
        .auto_columns(vec![fr])
        .auto_rows(vec![fr]),
)
.size(LayoutDimension::Percent(1.0), LayoutDimension::Percent(1.0));

// An ordinarily stretched child of cards, or an independently allocated Flex
// body below that child, uses the existing builder without containment.
edit.allocation_builder(body, |input, edit| {
    let width = input.known_size.width.ok_or(Error::UnsafeConstraintBuilder)?;
    let height = input.known_size.height.ok_or(Error::UnsafeConstraintBuilder)?;
    build_body(edit, width, height)
})?;
```

This is an additional dependency proof, not a promise that every Grid accepts
every builder. Ordinary Grid itself can still lay out content-dependent tracks;
the restriction concerns selecting structure from a full allocation before that
structure exists.

## Direction of dependencies

The existing independent-parent/containment proof retains precedence. The
additional ordinary Flex proof now crosses Grid edges under these conditions:

1. Both root axes have the same live external/numeric anchors required by the
   full Flex proof. Every path item/ancestor remains an in-flow plain Flex or
   Grid box, without custom parent algorithms, aspect-ratio feedback or
   intrinsic size/limit keywords.
2. Both axes of each Grid ancestor have nonempty explicit templates and explicit
   implicit-track patterns. Minima are finite numeric Px/Percent/owned affine
   Calc; maxima are numeric or fractional. Single, Count, AutoFit and AutoFill
   templates use normal kernel semantics. Each ancestor's definite allocation
   supplies percentage and automatic-repeat bases.
3. Named templates/areas and named placements reject. Auto, signed numeric lines
   and spans use the normal auto-placement/order rules. Visible in-flow items
   using first/last baseline alignment reject conservatively because baseline
   shims can introduce content-dependent item assignments.
4. The item on the builder path has numeric preferred size on each axis, an
   explicit Stretch sizing keyword, or Auto size with supported Normal/Stretch
   alignment and no auto margins on that axis. Auto+Normal excludes replaced
   items. Auto/numeric limits are permitted; numeric padding, borders, gaps,
   box sizing and gutters use the existing kernel calculations.

`GridTrack::fr(1.0)` means an **Auto minimum** and therefore does not qualify.
`minmax(Px(0), Fr(1))` is the usual content-independent fraction. Default implicit
Auto tracks also do not qualify, even if the current scene uses only explicit
tracks. This observes a structural contract rather than trusting equal current
measurements. Intrinsic/fit-content tracks and unrestricted automatic feedback
remain separate work.

The ordinary inline Grid proof shares only the numeric-axis/placement helpers.
Its prior scope remains: only columns must be independently sized; explicit row
counts stay fixed and row sizes may remain content-dependent. Full-axis Grid
can accept automatic row repeats when both axes satisfy this stronger proof.

## Actual input, tasks and atomicity

The builder reads the real final incoming kernel `LayoutInput`, including
`sizing_mode`, `known_size`, definiteness, hard bounds, soft space and percentage
bases. For a directly assigned Grid item, sizing mode is Inherent; its percentage
basis is the Grid area, soft space is that area minus item margins, and known
border-box size incorporates preferred sizing, padding floors and limits. Hard
bounds remain separate. Nested Flex inputs retain normal Flex semantics.
No allocation is reconstructed from viewport division or final rounded geometry.

Full Grid parent styles, layout-ordered child IDs and effective direct item styles
are recorded and compared before atomic adoption. Later track, placement or order
changes reject even if the final area would have the same numeric size. Grid
sibling **content** is not a measurement dependency when tracks are independent;
it may change without altering these slots. A measured Flex sibling below a Grid
cell still uses the complete measurement observations and
[inferred task prerequisites](RETAINED_INFERRED_MEASUREMENT_TASKS.md). A title task
therefore precedes its body's full Allocation task regardless of registration
order, while unrelated Grid content does not gain an invented dependency.

Ordinary final tracked input/cache validation, source observations and complete
geometry checks remain. Failure keeps sources, keyed structure, stats and old
frames. A callback runs once rather than replaying to numeric convergence. Old
paint/glyph frames retain their own resources after the producing UI retires.

## Costs, evidence and remaining direction

See [verification and reproduction](performance/2026-11-18-full-grid-allocation/README.md).
Controls cover nested Grid/Flex, LTR/RTL, resolved/local frames, resize, percentages,
spans, implicit rows, Count/AutoFit/AutoFill, preferred half sizes with padding and
margins, source producers, rejection/rollback and real Noto title tasks. Numeric
ordinary full Flex references compare complete CPU/GPU images; nested automatic
repeat cases also compare complete kernel layouts and CPU images.

Proof construction scans ancestors/tracks/direct children and retains immutable
style/order observations. It can run repeatedly in scheduling and validation;
ordinary private previews, candidates and final full geometry checks remain.
There is no new persistent per-node/GPU field or allocation index, no byte/time
budget and no timing, FPS, memory or native smoothness benefit claim.

General intrinsic/automatic-track feedback, Block/mixed custom protocol proofs,
full RenderBox, callback output ownership, component/source integration, global
resource budgets and independent native presentation remain active. Modern
desktop effects and continuous complete CPU output still need representative
experience/performance work. "99%" means ordinary fluid and intuitive interaction;
it is not a required success ratio or p99 gate.
