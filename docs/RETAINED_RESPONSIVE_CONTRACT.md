# Bounded responsive styles and an explicit Flex subset

`retained::responsive` adds validated Flex style values, configurable width
thresholds and a bounded responsive resolver. The normal gallery prepaint path
now uses this resolver; actual window resize switches a stable pair of tiles
between column and row without reconstructing their scopes or paint commands.
This stage follows RFC sections 5–6 and 25 within the
[full acceptance table](RETAINED_IMPLEMENTATION_PROGRESS.md).

## Public subset and environment

`FlexStyle` opts into a smaller contract than the raw pinned Taffy Style. Row and
column constructors keep border-box and grow/shrink/basis defaults of 0/1/auto.
Validated styles support row/column reverse, nowrap/wrap, gap, alignment,
length/percentage/auto dimensions, min/max, aspect ratio, RTL, ordinary flow and
absolute positioning. Negative margin/inset remain legal; nonfinite values,
negative gap/padding/border and invalid flex factors fail validation.

Grid/block/float, wrap-reverse, sticky/fixed positioning, containment and
intrinsic-size keywords are not exposed by this initial subset. Style overflow
is also rejected here until its painting/scroll contract is connected; existing
explicit retained clip/scroll properties remain available. The lower-level
raw Taffy Style is still accessible and does not become a claim of complete CSS
support. Alignment and reverse helpers use the pinned engine's semantics.

`LayoutEnvironment` separates logical layout viewport, device scale, application
zoom, scalar text scale and direction. Touch/pointer/reduced-motion can be
unknown. The host supplies already resolved logical viewport units; the resolver
does not multiply layout by device scale or choose a device category. This
initial style selector reads viewport width and direction. Text scale, zoom and
device scale are carried independently for their consumers, not automatically
applied to content. Safe areas, visual viewport, occlusion and nonlinear font
policy remain future environment inputs.

## Query and lifetime rules

`BreakpointStyles` accepts strictly increasing nonnegative finite thresholds.
Width >= threshold selects the next complete style. There are no global 600/840
constants. A rule owns the production of an existing scoped `Signal<Style>`;
that source synchronizes one target's typed Style sink. Each target/source can
be registered only once. Responsive updates compare final Style values before
opening a transaction. Resizing within the same interval still enters normal
layout, but emits no redundant discrete style patch. Idle resolution requests
no additional layout or GPU work.

`ViewportInline` reads logical viewport width. `ContainerInline(host)` reads the
actual resolved host content-box width: border-box width minus resolved padding,
border and allocated scrollbar. It never substitutes the parent constraint.
The first supported host must have length-valued border-box width/height with
the same fixed min/max, zero grow/shrink, auto basis, and fixed padding/border
which fit that box. The target must be a strict render descendant. These checks
exclude auto sizing and content-dependent minimums that could feed back into
the queried inline size.

The resolver first applies viewport styles and lays out the host, then applies
container styles and performs at most one further layout pass. It rejects a
container-query target that is itself a query host or an ancestor of another
query host, rather than iterating nested dependencies until they happen to
stabilize. There is no implicit ConstraintBuilder or structural branch builder.
Illegal hosts are rejected before style/frame changes. Removing a source or its
render target removes the rule, including a shared signal that outlives a target.

Two-pass container resolution can publish an intermediate internal packet;
the caller consumes only the returned final frame. Its delta may require a full
renderer resync when the intermediate base was not submitted. `ResponsiveFrame`
reports total layout/measure work across both passes. These are bounded passes,
not one atomic structural transaction or sublinear rule lookup.

## Validation and remaining scope

Seven responsive cases pass within 37 retained-WGPU core tests. They cover exact
threshold selection, same-interval equality, idle frame storage, independent
scale fields, RTL positions, fixed content-box queries, foreign/unsafe hosts,
invalid styles/thresholds, nested dependency rejection, source/target retirement
and `display:none` subtree removal/restoration with unchanged paint commands.
Both normal-click tests pass, including hardware execution and actual window
resize callbacks at 500 and 800 logical pixels. Core/gallery/runner Clippy checks
pass with warnings denied, and the desktop example builds.

The GPU suite passes 61 pixel scenes. Seven new references cover compact layout,
the exact threshold, resize within the same interval, RTL, scope retirement and
a content-box query whose 120px content width differs from its 140px border box,
and hidden subtree removal from GPU output.
Unchanged input submits no GPU work. The pictures compare renderer output;
they do not establish independent browser/CSS conformance.

[Frozen evidence](performance/2026-11-11-responsive/README.md) records these
sources, hardware results and new images. No responsive performance speedup is
claimed. Rule scanning currently examines all registered rules. Full environment
subscriptions, constraint builders, auto-height/nested safe query graphs,
responsive structural adaptation, shared-boundary device rounding, broad Flex
conformance and the rest of the full RFC remain open.
