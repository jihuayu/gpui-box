# Logical text inheritance and ordinary Text components

Status: experimental Rust API, 2026-11-19. The complete RFC remains active.
See [decision and costs](ADR_RETAINED_TEXT_COMPONENTS.md).

`retained::text_style` is available without a GPU or text backend. `TextStyle`
contains the explicitly inheritable properties: font family, integer weight
(100–900), slant, logical font size, line height and packed RGBA foreground.
Each property is `TextValue::Inherit` (also the omitted default), `Initial`, or
`Value(T)`. `Initial` resets just that property to the framework default. Defaults
are system UI family, weight 400, normal slant, size 16, line-height multiplier 2
and white foreground. Applications should establish their normal text defaults.

`TextLineHeight::Px` inherits an absolute logical length; `Multiple` inherits a
factor that resolves against the final consumer font size. Family, weight and
slant inherit independently; setting bold does not discard an outer font family.
Nonfinite/nonpositive sizes and line heights, overflowing factors, empty/NUL or
oversized family names, and weights outside the accepted range reject. The
existing paragraph backend also validates actual font ascent/descent against
line height; this initial backend rejects lines that cannot fit.

`TextStyle::resolve` is a pure data operation. `LogicalEdit` and
`ComponentContext::resolve_text_style` add ordinary read-only computed projections
against the nearest logical `ThemeGroup<TextStyleKey>`. A missing provider uses
the defaults. Provider identity/absence stays fixed for that consumer lifetime,
as [ordinary providers](RETAINED_SCOPED_PROVIDERS.md) do. Portals inherit from
logical ancestors. Layout, backgrounds, opacity and interaction never inherit.

`TextStyleScope<C>` resolves its partial style against the outer text group and
uses an ordinary transparent `ThemeScope<TextStyleKey,C>` for the resolved group.
It owns no render node, layout box or layout producer. This creates ordinary
logical scopes/sources; it is not free metadata. Child Props require Clone.
Same key/type/host updates preserve the child and its state. Full theme-group
replacement still resets omitted fields to defaults; partial text declarations
are a distinct operation and do not change that theme rule.

With `retained-text`, `retained::text::Text` is an ordinary typed component:

```rust,ignore
let preparation = TextPreparation::new(GpuiTextPreparer::new(
    window.text_system().clone(),
));
let mut label = TextProps::new("保存", preparation);
label.scale = window.scale_factor();
label.layout = LayoutStyle::column()
    .size(LayoutDimension::Px(120.0), LayoutDimension::Auto);
label.style.font_weight = TextValue::Value(700);

let props = TextStyleScopeProps::<Text> {
    style: TextStyle {
        font_family: TextValue::Value("Noto Sans CJK SC".into()),
        font_size: TextValue::Value(18.0),
        color: TextValue::Value(0xe8edf5ff),
        ..Default::default()
    },
    child: label,
};
edit.components::<TextStyleScope<Text>>(parent, host, [(key, props)])?;
```

There are no manual paragraph/measure/color bindings in the component's caller.
Text prepares its resource on the owner, then uses the existing text sink,
paragraph measurement, Taffy Flex, glyph wrapping, baselines and immutable frames.
The layout source is independent from text content. Width changes reuse the
paragraph/glyph resource and let normal layout recompute wraps. A separate
typography request excludes foreground: equal typography/content/preparation/
scale/font revision avoids reshaping on color changes, including masked outer
styles. Paint changes still go through the normal backend path.

`TextPreparer` is the explicit owner-local font resource adapter. `TextPreparation`
shares its Rc identity; changing identity invalidates resource preparation.
`GpuiTextPreparer` uses the existing WindowTextSystem, ParagraphSnapshot and
GlyphRasterizer and exposes resource/rasterization counters for diagnostics.
No Window borrow enters layout/render work. Only the immutable prepared resource
crosses the frame boundary. A custom adapter must return a resource at the
requested scale; the Text component rejects a mismatch. Other shaping fidelity
is the adapter's responsibility.

`scale` is glyph raster scale, separate from logical font size and layout zoom.
The host updates it explicitly for DPI changes. `font_revision` is passed to
the preparation adapter as well as participating in request equality; custom
adapter caches must account for it. `font_revision` is a host input
invalidation after font database/fallback changes; increment it (or replace the
preparer) to request fresh shaping. It is not an automatic platform subscription
or the renderer resource epoch. Host changes must be captured consistently with
the normal UI transaction. Existing prepared frames own their old resources.

Once text-theme lookup/provider state is adopted and a frame establishes its
constraints/viewport, later source/Props edits validate complete frames before
adoption, including standalone Text with no provider. Invalid style, unsupported
text, failed preparation or final frame failure preserves previous source/Props,
scopes and complete output. Resource memoization/counters can advance during a
failed speculative attempt; external adapter side effects are not rolled back.

This is the uniform text/inheritance contract, not a rich-text or CSS inline
formatting implementation. Font feature/fallback lists, letter/word spacing,
text alignment, ellipsis, selection, editable text/IME, color emoji, complex bidi,
vertical text, portable font resources and semantic UI IR remain open. Existing
ParagraphSnapshot restrictions on tabs/CR, shaped ordering and glyph resources
still apply. Low-level already-prepared TextPaint and UiContent::Text retain
their explicit values and do not acquire implicit style inheritance.

Text preparation remains synchronous owner work and can be expensive for large
content or many changed consumers. There is no new persistent shaping cache,
budgeted PendingScene, OS subscription or independent native presentation.
See [verification](performance/2026-11-19-text-inheritance/README.md). No new
FPS/latency or numerical success-rate claim is made.
