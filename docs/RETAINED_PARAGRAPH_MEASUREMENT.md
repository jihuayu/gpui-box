# Owned shaped paragraph measurement

This stage adds `retained-text` and `MeasureSnapshot::Paragraph` to the
[RFC runtime](RETAINED_IMPLEMENTATION_PROGRESS.md). It implements owned
constraint-dependent measurement, not retained glyph rendering.

`ParagraphSnapshot::shape` consumes a real GPUI `WindowTextSystem`, text, font,
font size and line height on the owner. It shapes each hard line once, retains
owned `ShapedLine` values and captures the font database epoch. Width-dependent
layout then wraps these shaped runs without an App, Window, text system or
application closure. A small GPUI extension exposes `ShapedLine::wrap_owned`,
using the same existing cluster-aware line-break algorithm. No new per-character
width approximation or duplicate line breaker is introduced.

The first contract is uniform-font LTR glyph order, LF hard lines, GPUI soft
wrapping with emergency cluster breaks, no clamp and a fixed positive line
height large enough for ascent + descent. Empty text and trailing hard lines
each retain an empty line. Tabs, CR, nonmonotonic visual/byte glyph order,
invalid metrics and oversized input are rejected explicitly. This is not full
Unicode bidi, CSS text/min-content or rich-text conformance. The declared
min-content policy is the largest indivisible shaped cluster advance under
this emergency-wrap contract. The public `min_content_width` exposes that
policy; normal word-only min-content remains a separate future contract.

The immutable semantic snapshot memoizes four recently used exact widths,
including no-wrap, with a mutex. Results share an `Arc<ParagraphLayout>`;
evicting one does not invalidate consumer-held layouts. Repeated width queries
return the same Arc. This bounded memoization does not access UI state, request
frames or change metrics. Layout can run after its source context is dropped
and can transfer to another thread. Font IDs remain meaningful only to their
source font system for future painting; this stage does not claim cross-system
glyph reuse or font-resource leases.

Persistent Taffy's content measurement now receives its actual available width.
Definite width wraps at that content width; min/max-content select the declared
cluster minimum or unwrapped width. The existing Taffy box algorithm applies
known dimensions, padding, border and min/max constraints. Paragraph first
baseline includes half-leading, ascent and resolved top padding/border. Changed
text/snapshot enters `Relayout`; same snapshot identity is a no-op, preserving
Taffy cache and command chunks. The caller reuses snapshots across unchanged
text/font/size inputs; constructing an identical new snapshot creates a new
identity and is not automatically deduplicated.

`TextSystem::font_epoch` increments through the existing font installation path.
New snapshots capture it; an epoch change during shaping rejects publication.
Existing owned snapshots remain immutable and continue to measure against the
old glyphs. The owner must compare epochs and supply a new snapshot after font
installation. Automatic subscriptions, fallback changes and rendering-resource
epoch management remain pending.

The normal gallery's paragraph button shapes in its event handler and writes
the existing typed measure sink. The resulting geometry is represented by a
colored rectangle and reaches the persistent WGPU path with one layout and
zero command recordings. The UI labels this measurement behavior. It does not
paint the paragraph's glyphs. The click test now uses an actual Cosmic text
system and the bundled Noto Sans CJK SC font instead of the GPUI no-op text
implementation, and checks hardware geometry against full output.

Four real-font tests compare all wrapped boundaries/glyph runs with GPUI for
Latin ligatures/combining characters, Chinese, empty lines, trailing LF and
widths from zero to wide. They verify content insets, sibling reflow, automatic
minimum width, first-baseline alignment, same-input idle behavior, chunk reuse,
new text, epoch replacement, cache eviction, old metrics and cross-thread
context-free measurement. With core tests, both click tests and hardware
regressions, current verification is 44 core tests, six shared-example tests
and 83 GPU pixel cases. Desktop build and Clippy pass.

[Frozen CPU evidence](performance/2026-11-11-paragraph/README.md) includes both
cache hits and distinct-width misses, GPUI's warm layout cache and fresh window
cache. A cache miss still walks shaped glyphs and allocates wrapped layouts;
this is not constant-time paragraph layout. Subsequent work includes owned
text paint items, glyph resources/atlas epochs, retained GPU glyph instances,
matching ink/damage/hit geometry, rich runs, bidi, selection/editing and
automatic font invalidation. Text rendering acceptance remains open.
