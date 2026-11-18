# Measured siblings in full Flex allocation builders

Status: experimental framework capability, 2026-11-18. Full RFC remains active.
Extends [ordinary two-axis allocation](RETAINED_FULL_FLEX_ALLOCATION.md) using
the same allocation_builder, Allocation plan/producer kind and final input
recording. Source schema6 and public types are unchanged.

## Common UI behavior

An ordinary Column can contain an automatically measured title/toolbar and a
body that fills the remaining height. The body can now read its actual assigned
width/height before constructing its content, without forcing the title to a
fixed height or adding size containment. Row/sidebar and reversed directions
use the same rule. Text wrapping and image/content intrinsic sizing remain the
normal layout algorithms; no title-specific allocator is introduced.

The previous full-axis proof required every visible in-flow sibling to have a
content-independent numeric size policy. Now that restriction applies to the
host's path child at each ancestor edge. Other siblings may measure their own
content, including nested Flex, prepared paragraphs, images and pure content
measurement. A tree sibling cannot contain the path child, and the ancestor's
two axes are already independent of the builder's constructed content.

The path proof is unchanged: externally sized root axes, numeric main basis and
explicit main minimum, no aspect/intrinsic feedback, NoWrap cross stretch or
numeric cross size, and wrapped path items with numeric cross size and no
baseline sharing. Numeric sibling policies retain the previous compact input
observation and need no added subtree record.

## Capture settled layout inputs, validate before adoption

For each sibling whose item policy can observe content, the proof additionally
records the complete sibling subtree's layout-relevant immutable inputs:

- Node identity, child identities/order and effective layout style.
- MeasureSnapshot, including actual prepared paragraph identity, image intrinsic
  dimensions and pure ContentMeasureSnapshot handle.
- ParentLayoutSnapshot and ParentData handles for custom algorithms/metadata.

Capture is iterative. Comparisons do not invoke user algorithms or compare
their answers; existing typed equality/immutable identities are authoritative.
Internal paragraph memo warming changes no observation. Styles, insets, order,
resource measurements and metadata must settle before the dependent builder.

The ordinary private preview computes the real incoming full LayoutInput after
capturing these dependencies. Final candidate layout rechecks the same path,
all recorded sibling inputs and complete actual input before any source/keyed/
frame adoption. A late sibling layout-input change rejects even if its final
height or the body's allocation happens to be numerically equal. A new pure
measure handle with the same answer is still a changed dependency. Reusing its
original immutable handle is equal.

Paint inputs are deliberately separate: fill, translation, visual clipping,
scroll offset, group opacity and decoration do not become layout measurements.
Text color with the same shaped paragraph and image pixels with the same
intrinsic dimensions may change after observation. They still use ordinary
paint/property and complete geometry validation. Layout overflow/scrollbar
policy is part of Style and remains a measured dependency.

## Task order, rollback and lifetime

The once-only plan and persistent producers use the existing scheduler. If a
sibling task changes title contents, declare it before the dependent body with
an explicit after prerequisite, or settle its edits before calling the body
builder. This stage does not automatically discover arbitrary callback output
dependencies. An incorrectly ordered plan fails final validation; callbacks are
not replayed to numeric convergence. A failed transaction leaves the prior
sources, registry, keyed lifecycle, statistics and complete published frame.

Measurement observations are transaction-local. Old complete frames retain
their paint/glyph/image data and do not remeasure against current owners.
The existing independent-parent/containment proof still takes precedence;
its loose/content-sized domain is unchanged.

## Costs and evidence

Content-dependent sibling subtrees are visited during capture and final
recapture; records retain cloned immutable style/measure/algorithm/data handles
and child lists for the transaction. Multiple dependent builders can duplicate
observations of the same sibling. No global read index, cache byte budget or
incremental rollback is added. Ordinary candidate/private-preview/final-input
and complete geometry costs remain. This is a capability change with no timing,
memory or FPS gain claim.

[Verification](performance/2026-11-18-measured-flex-siblings/README.md) covers
four directions, changing nested intrinsic content, real Noto wrapping titles,
remaining body space, mixed wrapped lines, pure content-measure identity,
image-pixel versus intrinsic changes, explicit producer order and late task
failure. CPU and explicit GPU compare complete output with numeric full Flex
references; old frames survive owner/resource retirement.

General content-dependent host/main sizing, intrinsic/ratio feedback on the
path, automatic wrapped path cross size, Grid/Block full-axis proofs, inferred
cross-task writer dependencies and complete RenderBox remain open. Component
integration, resource budgets and independent native presentation remain active
work. "99%" continues to mean common fluid/intuitive interactions, not a numeric
ratio or p99 gate; correctness controls do not establish native smoothness.
