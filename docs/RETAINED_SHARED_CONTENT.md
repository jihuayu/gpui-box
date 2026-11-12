# Shared GPU placement for images and text

Normal WGPU rendering now shares the existing hierarchical placement buffer with
image and glyph draws. Moving, clipping or scrolling an ancestor can update its
96-byte spatial node without rewriting per-image 48-byte geometry or per-text
32-byte properties. Glyph instances, masks and image textures remain unchanged.
Local size/insets, resource, text shape or color changes retain their normal
update paths.

Published items own separate border/padding components, preserving the producer's
f32 addition order and old snapshots. Equal current content rectangles still
publish changed components. A common vertex resolver computes global content
origin and clip; flat fragment inputs preserve pixel-center behavior with cropped
targets. Glyph baseline rounding remains global before camera subtraction.

Private exact-base routing suppresses unchanged image/glyph descriptors. Backend
inverse memberships still invalidate affected cached layers, and projected
reference/finite-arithmetic checks include retained draws with no descriptors.
Mode/capacity/scale changes, removals/replacements, rejected preflight retry and
completion-held bindings preserve the existing lifecycle rules. See the
[decision record](ADR_RETAINED_SHARED_CONTENT.md).

`WgpuFrameRenderer::set_shared_content_enabled(false)` keeps per-draw world
placement as a control while leaving Quad/Shadow spatial sharing enabled. The
option defaults to true, takes effect with shared spatial placement, forces
resync on changes and survives reset. Raw `QuadImage` and `GlyphBatch` callers
set `spatial: None` for original absolute semantics or `Some(SpatialContent)` for
shared semantics. Shared glyph scale must equal spatial scale. Disabling the
spatial system requires removing or replacing all shared draw bindings in the
same batch.

```powershell
./tooling/rendering-perf/run-retained-gpu.ps1 -Output target/content-validation -ValidationOnly
./tooling/rendering-perf/run-retained-gpu.ps1 -Output target/content-run -ContentSpatialOnly
```

[Frozen evidence](performance/2026-11-12-shared-content/README.md) records pixels,
upload counts, actual completed-owner phases, raw samples and scope limits.
Existing image/glyph/spatial benchmark controls explicitly select the old content
transport so their comparisons retain their earlier meaning.

The full RFC remains active. Complete resolved subtree extraction, resource/order/
layer/lease scans, measured adaptive cost policy, general primitives and layout,
physical budgets/device loss, native surfaces and cross-platform qualification
remain in the [progress tracker](RETAINED_IMPLEMENTATION_PROGRESS.md).
