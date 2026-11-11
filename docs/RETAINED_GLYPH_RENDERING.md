# Retained grayscale glyph rendering

This stage connects the owned paragraph measurement path to actual retained
text painting for the [RFC](https://app.notion.com/p/415d83088d4e818290cddca2f104e65c).
The subsequent [glyph material update](performance/2026-11-11-glyph-materials/README.md)
separates node colors from glyph geometry. The full implementation goal remains open in the
[acceptance table](RETAINED_IMPLEMENTATION_PROGRESS.md).
The [glyph property update](performance/2026-11-11-glyph-properties/README.md)
also separates node origin/scale/clip, reusing local geometry on translation.

`GlyphRasterizer` captures the real platform `TextSystem`. It prepares immutable
`TextResource` values from a `ParagraphSnapshot`, rasterizing the actual shaped
font/glyph IDs with grayscale antialiasing, zero dilation and subpixel variant
zero. The cache key includes font epoch, font ID, logical font size and device
scale. The rasterizer holds weak entries; resources and packets own the pixels.
Two paragraphs from the same font system share live bitmap resources. A stable
text-system identity prevents equal font IDs from separate systems from aliasing.
Foreign systems and stale paragraph epochs are rejected before rasterization.
Adding fonts requires the owner to shape and publish another paragraph.

The initial paint contract supports the existing uniform-font, monotonic LTR
paragraph subset, hard newlines, emergency wrapping and color/alpha. Raster
baselines are rounded to integer device pixels before applying the bitmap offset.
The prepared scale must exactly match the consumer's scale; the normal gallery
reprepares its resource on a scale change. Each glyph mask is at most 1024 square,
device em is at most 1024 and each text resource has a 32 MiB unique mask guard.
Empty glyphs contribute advance without a paint instance. Emoji/color glyphs,
BGRA/LCD masks and reordered bidi text remain unsupported.

`TextResource::chunk(width)` retains local glyph positions and bitmap Arcs for
four recent content widths. The positions use the actual owned wrapped runs,
their glyph offsets and half-leading baseline. Pure wrapping and chunk creation
continue after the App, Window and rasterizer are gone. Conservative ink bounds
include one device-pixel fringe for snapping, and participate in old/new damage
even when the ink overflows a fixed-height layout box.

`Transaction::text` and typed `Property::text` attach the text paint to a leaf
render node above its background. The attachment owns its paragraph measurement.
Explicit measurement while final text is attached, text plus image on one node,
and attaching to a container are rejected atomically. A transaction may switch
between image and text by clearing the old attachment; final attachment
measurement takes precedence. Detaching clears the owned measurement unless an
explicit replacement measure is supplied. Source equality skips sink dispatch.
Changing color with the same resource produces `ResourcePatch` with no layout
and no quad-command recording, reusing the same glyph chunk. The current backend
writes a 16-byte per-node material record without rewriting glyph geometry.
The initial frozen evidence retains its earlier inline-color upload counts.

The WGPU backend packs unique masks into 1024-square R8 atlas pages and uses one
48-byte local instance per visible bitmap (initially 64 bytes). Per-node ranges persist, grow geometrically
and return to a coalescing free list when no longer live. Adjacent dirty ranges
merge before buffer upload. Consecutive glyphs on the same atlas page use one
instanced draw; switching pages preserves glyph paint order. Node glyph draws
follow their quad and participate in the existing property/effect composition.
Changing color, translation, clip or wrapping reuses uploaded masks. Changing
only isolated opacity reuses both instances and cached raster content.

Changed resource IDs invalidate cached layers even if a freed atlas tile is
reused at the same coordinates with identical geometry. Unused tiles/pages and
draw records are evicted. Queue ordering protects submitted work during tile
and instance reuse; submission leases retain buffers, page textures and bindings
until GPU completion. Skipped-base packets resynchronize from their full owned
snapshot. Reset reuploads the exact owned masks from that snapshot.

Instances reference stable per-node color slots in a separate storage buffer.
Color writes merge adjacent material ranges and independently invalidate layer
contents. Slot reuse is queue ordered, and old material buffers/bindings remain
leased through completion after growth. Color-only changes write no geometry;
clip/translation write independent 32-byte node property records. The shader
resolves integer device-pixel baseline placement using the same halfway-away-from-zero
rounding as the CPU oracle, including negative coordinates. Rewrap writes affected
local geometry. Property changes independently invalidate raster content and merge
adjacent writes; property buffers/bindings also remain leased through completion.
This separation reduces upload size while full CPU glyph planning still remains.

Live guards cover 32 MiB of unique bitmap data, 64 atlas pages and the device
storage-buffer limit. They do not form a physical cross-frame budget. Packing
fragmentation can exhaust pages/buffer capacity before those live guards; a
preparation failure requires renderer reset. Compaction, admission/LRU, advance
reservation and graceful cache-pressure fallback remain open.

The normal gallery has draw-text and change-color buttons on the scoped signal
path. With retained WGPU it presents the offscreen result through the existing
GPU readback → window atlas bridge. Without WGPU it paints the stored font/glyph
positions through `Window::paint_glyph`; the legacy backend's AA/subpixel policy
is separate and is not claimed pixel-identical to the retained integer-origin
contract. Direct GPU texture import/presentation remains required.

Validation includes 44 core tests, ten shared-example tests including real click
dispatch and real Cosmic/Noto shaping/rasterization, plus 107 hardware pixel
scenarios. Twenty-four hardware cases cover text: Unicode/combining characters,
color/alpha, damage translation, clip, rewrap, shared masks, isolated opacity and
cache reuse, resource replacement, skipped base, explicit reset, device scale,
quad occlusion, removal/retirement, overlapping in-flight replacements, multiple
atlas pages, material cache invalidation, material slot reuse and five-node
adjacent upload merging, signed half-pixel/fractional placement and independent
property invalidation. Five-node color writes 80 material bytes in one range,
five-node movement writes 160 property bytes, both with zero geometry/atlas upload;
initial inline colors wrote 2,560 instance bytes.

The incremental/full comparison uses a fresh retained renderer and the same
glyph shader. A separate CPU full-mask blender verifies supported non-effect
frames with at most two byte values per channel tolerance; it shares published
glyph positioning and bitmap bytes, so it independently checks sampling, clip,
alpha blending and draw order rather than shaping or platform raster quality.
CPU cases also check weak ownership, four-width eviction, font-system/epoch and
scale rejection, final-value routing, overflow damage and atomic image/text swaps.

[Frozen measurements](performance/2026-11-11-glyphs/README.md) compare warm reuse
against a cold GPU cache for the same 450-glyph full redraw. They include submit
and GPU completion, not shaping, mask rasterization, transaction/extraction,
reset construction, readback or window presentation. Cache reuse saves measured
work, but full scene validation, glyph planning and instance comparison still
scan the packet. No sublinear text-update or end-to-end FPS claim follows.

Remaining text acceptance includes rich text/font runs and decorations, full
bidi and alignment contracts, editing/selection/caret/IME, color glyphs, LCD and
subpixel variants, automatic font subscriptions, async preparation/cancellation,
general shared GPU property trees, general/rich-text materials, indexed chunk deltas, robust pressure handling,
cross-backend validation and native presentation. Paths and shadows remain
separate unimplemented primitive classes.
