# Automatic overflow content viewports

Status: implemented experimental Rust contract, 2026-11-18. Completes the ordinary
viewport behavior missing from [wheel scrolling](RETAINED_WHEEL_SCROLLING.md).
The complete RFC and native common-interaction p99 acceptance remain active.

## Behavior and API

LayoutStyle overflow now controls both layout sizing and the content viewport.
Each non-Visible axis (Clip, Hidden, Scroll) clips own image/glyph paint and all
render descendants to the padding viewport: border box minus resolved border
and reserved scrollbar gutters, including padding. RTL vertical gutters occupy
the left edge. A Visible axis adds no restriction. Axes are independent; this is
the framework's declared subset, without CSS computed-axis coercion.

The host background, decorated border and drop/inset shadows keep the ordinary
inherited/explicit clip. Overflow does not trim its own outer shadow or erase its
border. An explicit Transaction::clip still clips the whole node/subtree and
intersects the automatic viewport. Empty viewports clip all descendant ink.
Rounded viewport masks, scrollbar chrome and CSS overflow-clip-margin are open.

`PaintItem::clip` remains the node's ordinary paint/hit clip;
`PaintItem::content_clip` is the effective image/glyph/descendant clip.
`SpatialState::overflow_clip: Option<OverflowClip>` carries local padding bounds
and two axis flags. All complete producers start with a finite viewport, which
supplies bounds for unrestricted axes. No sentinel infinity or guessed screen
extent is introduced. Consumer validation rejects automatic overflow without a
finite ordinary clip. These output structs are experimental and not a stable
foreign/wire ABI. Source UI_SCHEMA_VERSION remains4: no source field is added.

The existing sparse PropertyTrees::clips remains the authored explicit-clip
chain. Automatic overflow is derived layout geometry in SpatialState; consumers
must use the complete placement and content_clip rather than interpreting the
explicit property map as all effective clipping. This adds no parallel mutable
layout/scroll truth.

## Publication and renderer integration

Full extraction, selective property/subtree extraction, geometry-only refresh,
LocalFrame resolution/hits/validation and device-pixel projection propagate the
content viewport. Own hit bounds keep the ordinary clip; children inherit the
parent's content clip, so overflow pixels cannot remain clickable. Old frames
retain their original geometry. Transform/scroll retain local paint payloads.

Resolved size/border/gutter changes that alter a viewport dirty its subtree,
even when child relative layout results are cached. Changing overflow policy
also invalidates its subtree even if scalar geometry is equal. RTL direction
changes update viewport propagation. Ordinary non-clipping size changes retain
the previous individual-node publication rule. This is a necessary dependency,
not an alternative layout algorithm or a caller dirty hint.

CPU image/glyph draws use content_clip; quads use ordinary clip. WGPU absolute
content records do the same. Shared GPU traversal computes separate ordinary
and content clips, passes content clip to children and image/glyph shaders, and
leaves Quad/Shadow clipping unchanged. Root and every32nd-depth anchor retain
resolved ordinary clip plus their own local overflow viewport; sparse ancestor
changes update the existing deep anchors. GPU projected validation/indexed
updates follow the same root-to-leaf f32 order. Existing buffer/resource leases,
base revisions and atomic adoption boundaries remain.

## Costs, migration and limits

The internal GPUI WGPU SpatialNode grows96→112bytes: one additional initialized
vec4 with axis flags in existing links. This is16 extra bytes per allocated slot
and per changed spatial record, plus consumer CPU resolved content-clip state.
Content image/glyph record layouts remain48/32bytes. Rust paint/local snapshots
gain optional local overflow and resolved content clips; normal metadata/tree
candidate scans and whole-geometry validation remain. No new layer or texture
is allocated just to express overflow.

Benchmark assertions now derive spatial bytes from actual SpatialNode size.
Historical96-byte measurements stay historical and must not be treated as
current record size or reused as new performance evidence. There is no fresh
wall-time/peak-memory/FPS improvement claim. ABI consumers must rebuild together.
Callers that previously used non-Visible solely for sizing now also get clipping;
use Visible when overflow must paint, and explicit clips for additional bounds.

Programmatic scroll clamping and wheel eligibility retain their existing
contracts: Hidden permits programmatic offsets, Scroll accepts default wheel,
Clip/Visible do not accept wheel. Own text scroll-origin behavior is unchanged
(current scroll property moves descendants). The follow-up
[focus reveal](RETAINED_FOCUS_REVEAL.md) adds atomic nearest post-layout reveal;
anchoring, keyboard/scrollbars, inertia and owner-stall scrolling remain open.
Native presentation p99, CPU complete-frame fluency and sustained60/120Hz remain
separate unpassed gates. See [verification](performance/2026-11-18-overflow-viewports/README.md).
