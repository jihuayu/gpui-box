# Flex alignment keywords and overflow position

Status: experimental public contract, 2026-11-17. This extends the framework-owned
LayoutStyle, not the legacy FlexStyle adapter. Complete CSS alignment, baseline,
writing-mode/Flex conformance and the full RFC remain open.

## Public API

```rust,ignore
LayoutStyle::row()
    .flex_direction(LayoutFlexDirection::RowReverse)
    .justify_content(LayoutJustifyContent::FlexStart)
    .align_items(LayoutAlignItems::Center.safe());

LayoutStyle::column()
    .align_items(LayoutAlignItems::SelfStart)
    .align_content(LayoutAlignContent::FlexEnd.unsafe_overflow());
```

Existing enum spellings and builder calls such as align_items(Center) remain.
The public Alignment<T> pairs a position keyword with AlignmentOverflow:
Default, Safe or Unsafe. The retained root reexports LayoutAlignment and
LayoutAlignmentOverflow. Position enums add Normal/FlexStart/FlexEnd; AlignItems
and AlignSelf additionally add SelfStart/SelfEnd. JustifyContent adds Stretch.
AlignSelf::Auto continues to inherit the parent alignment including its modifier.

Start/End follow the container's writing direction; FlexStart/FlexEnd follow its
flex main/cross start including reverse direction or wrap-reverse. SelfStart/
SelfEnd follow the child's own direction where the cross axis is inline. Current
writing mode remains horizontal-tb; vertical writing is not inferred. Safe
positional alignment falls back to logical start on negative free space; Unsafe
keeps the requested alignment. Default delegates fallback to the actual layout
context, including out-of-flow rules. These types describe alignment, not style
overflow clipping or a scrolling command. Auto margins still take precedence.

Normal cross alignment stretches eligible auto-sized items; Normal content
alignment stretches lines. In Flex main alignment, Normal and Stretch both behave
as FlexStart and do not resize auto-sized items. Positive free space is still
consumed by grow factors and auto margins before alignment. Existing defaults
remain Stretch/Auto/Stretch/Start for items/self/content/justify respectively;
the framework's default justify Start differs intentionally from CSS Normal on
reverse axes, so browser fixtures explicitly author it.

Position enums offer .safe() and .unsafe_overflow(). Alignment::with_overflow
allows explicit reconstruction. Both modifiers are accepted only on Start, End,
FlexStart, FlexEnd, Center and item-level SelfStart/SelfEnd. Applying them to
Normal, Stretch, Baseline, Auto or distribution keywords rejects with InvalidStyle
before tree/source transactions publish. Raw Taffy structs retain their lower-level
behavior; they do not bypass validation of framework LayoutStyle values.

**Migration:** LayoutStyle's four public alignment fields now hold Alignment<T>.
Builder call syntax accepts enums or complete values via Into. Direct struct
literal/field assignment needs `.into()`, e.g.
`style.align_items = LayoutAlignItems::Center.into()`, and readers inspect
`.position` / `.overflow`. New enum variants require updating exhaustive matches.
This remains an experimental source API, without a stable binary/foreign ABI.

## Owned-kernel corrections

The project-owned Taffy kernel already carries independent keyword/safety fields.
Public lowering alone exposed two actual Flex errors, fixed here:

1. Explicit justify-content:stretch must resolve to flex-start just like Normal,
   including reverse main axes. Passing Stretch into generic distribution math
   had placed reverse items at logical start instead.
2. Content-distribution Stretch falls back to flex-start on overflow **without**
   gaining implicit safe semantics. Previously it shared SpaceBetween's safe
   fallback and incorrectly moved oversized wrap-reverse lines to logical start.
   The common helper is shared with Grid; both use this keyword fallback.

References: [Flex main distribution](https://www.w3.org/TR/css-align-3/#distribution-flex),
[Flex align-content stretch](https://www.w3.org/TR/css-flexbox-1/#valdef-align-content-stretch),
[Box Alignment stretch fallback](https://www.w3.org/TR/css-align-3/#valdef-align-content-stretch),
[Flex space-between](https://www.w3.org/TR/css-flexbox-1/#valdef-justify-content-space-between).

### Explicit browser deviation

Chromium145 still places overflowing SpaceBetween at flex-start on reversed
main axes / wrap-reverse lines. Current Flexbox specifies **safe flex-start**,
whose negative-free-space fallback is logical start. The existing owned kernel
already follows the latter; this stage preserves it. Twelve such cases are
retained and separately asserted against the exact expected offset difference.
They are excluded from the 5,364 browser-compatibility passes. There is no
browser-version conditional or hard-coded scene specialization in production.

## Transactions, publication and resources

The complete Alignment value participates in existing LayoutStyle equality,
typed source bindings, breakpoint styles, previews and final proof validation.
Changing only its overflow modifier is a layout change; an equal source/style
does not advance revision or request another frame. Atomic invalid styles preserve
sources, layout, stats and the published frame. No independent rendering path or
second geometry truth is introduced.

Actual layout output drives existing resolved/local placement, damage and hit
publication. Real paragraph wrapping stays at its assigned width; alignment
moves the text node's origin without changing width-dependent glyph geometry.
Frame ownership and resource lifetime remain the existing snapshot contracts.

## ADR: keep alignment keyword and overflow policy separate

**Decision:** retain the existing per-property keyword enums and add a generic
owned value wrapper. Builder methods accept either plain enums or that wrapper.
Lower both fields to the owned kernel and validate legal combinations first.

**Alternatives:** adding SafeCenter/SafeEnd/UnsafeCenter and every combination to
four enums expands the variant surface and conflates independent concepts.
Boolean safe flags on LayoutStyle permit the same invalid combinations without
making the association obvious. Exposing Taffy alignment structs directly would
bind public framework code to the algorithm kernel. Changing default semantics
to CSS Normal would silently move existing reverse layouts; defaults stay stable.

**Costs:** each public alignment value carries an additional small enum; padding
and total LayoutStyle size are compiler/target-dependent. Copies/equality and
responsive style snapshots include it. Validation adds four bounded checks and
lowering maps the two fields; layout math reuses the existing kernel. No new
tree-node/GPU records, scene scan, per-frame heap allocation or renderer pipeline
is added by alignment itself. Existing full previews/layout and publication
costs remain; no timing, memory saving or smoothness improvement is claimed.

## Evidence and open work

See [validation record](performance/2026-11-17-flex-alignment/README.md).
Independent Chromium geometry covers 5,376 cases across all four flex directions,
LTR/RTL, three wrap modes, child direction, items/self/content/main alignment,
positive/negative free space and modifiers. 5,364 match within 0.02px (Chromium's
fractional distribution uses 1/64px), with twelve explicit spec/browser differences.
Pixel controls use independently positioned ordinary trees; real Noto shaping
checks glyph identity, CPU output and old-frame lifetime. Explicit GPU submission
checks actual pixels but does not establish visible presentation/frame timing.

Still open: last baseline, baseline content distribution, left/right keywords,
vertical writing modes, broader out-of-flow/alignment combinations and device
precision; broad Flex/intrinsic/percentage/cyclic/Grid/Block conformance, complete
RenderBox, automatic builder/query/environment scheduling and incremental previews.
Modern desktop GPU effects, CPU continuous complete-frame delivery, sustained
60/120Hz and display-visible input tails retain their separate acceptance gates.
