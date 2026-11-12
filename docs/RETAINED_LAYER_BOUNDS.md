# Bounded isolation textures

Isolation layers now default to the clipped content union instead of allocating
one surface-sized texture per effect. `WgpuFrameRenderer::set_tight_layer_bounds_enabled(false)`
keeps a full-viewport texture control using the same renderer and draw pipeline.
This extends [group compositing](RETAINED_GROUP_COMPOSITING.md); the
[complete implementation goal](RETAINED_IMPLEMENTATION_PROGRESS.md) remains active.

## Content bounds and publication

The composition cache stores a nearest-effect ink union per immutable paint
page. Unchanged pages retain these maps. Changed pages inspect their items;
`CompositionStats::bounds_items` reports that work. Page maps merge by effect,
then postorder child unions propagate into their parent groups. This includes
clipped quad/image bounds and grayscale glyph ink extending outside layout boxes.
Own or ancestor opacity zero does not remove contents from the union.

The consumer multiplies by device scale, conservatively adds one physical pixel,
floors/ceils the edges and clamps them to the target surface. Empty/offscreen
unions use a cleared 1×1 dummy allocation and skip composition. A clipped-out
layer may therefore report four payload bytes. Backend bounds validation uses
checked addition and rejects out-of-surface rectangles before allocation.

Per-page reuse avoids another complete item scan for opacity-only changes. The
page directory/maps and effect records are still merged/scanned per submission;
this is not a sublinear complete-composition claim. A global persistent effect
bounds index and selective backend invalidation remain possible follow-up work.

## GPU coordinates, invalidation and ownership

Every target owns immutable camera and quad-global buffers containing its
physical origin and size. Quad vertices subtract the origin; the quad fragment
shader restores it before gradient/SDF evaluation. Image and glyph shaders use
the same target camera and recover world coordinates for clipping/materials.
Nested composition draws the child's physical rectangle into the parent's
camera and loads texels relative to the child's origin. Each pipeline switch
binds its corresponding camera/instance group explicitly.

The legacy GPUI globals ABI grows from 16 to 32 bytes. Legacy initialization
always supplies zero origin and retains its original viewport and alpha policy.
Only retained layer targets use nonzero origins. Existing image/glyph resource
record ABIs and delta accounting remain unchanged.

Changing the physical origin or extent allocates a replacement layer and
invalidates parent contents. Stable bounds preserve the texture. Own opacity
still updates a 16-byte uniform while reusing its raster. Leases explicitly keep
replaced textures, opacity buffers, camera/global buffers and bindings through
GPU completion; root camera leases also survive target replacement. Reset
retains the bounds-mode preference and starts a fresh allocation epoch.

`QuadSubmission` reports current live layer texture payload, this submission's
allocated texture payload/count, and raster target pixels. These are logical
RGBA payload/target-area counters, not measured driver-resident memory, actual
covered fragments or peak physical allocation. They exclude texture alignment,
buffer overhead, main target, resources and retired epochs. The 256 MiB guard
now sums actual live layer extents (including dummy pixels); it still does not
provide cross-frame physical budgets, admission/LRU or over-budget fallback.

## Correctness and timing evidence

Hardware reference rendering forces full-viewport layer bounds with a fresh
cache. The analytical overlap/nested-opacity checks remain independent of both
texture-size modes. New mixed cases cover nonzero origins, nested groups,
rounded borders, alpha images, glyph ink overflow, clipping, negative movement,
complete offscreen/empty contents, hidden edits/restoration, fractional scales
1/1.25/1.5, queued replacements, and reset. Malformed public backend bounds and
actual over-budget bounded layers reject before allocation with a valid retry.

[Frozen evidence](performance/2026-11-12-layer-bounds/README.md) records 208
hardware scenarios (207 byte-exact pixel comparisons plus backend preflight),
114 core tests, 12 gallery/font/hardware-click tests, all-target Clippy and the
desktop build. Three repeated timings show 1.60–1.85× median speedup at 1080p and
1.96–2.01× at 4K for small groups. The full-surface control shows no consistent
benefit (4K pooled tight mode is about 3.3% slower). All four 148-source manifests
match final source bytes and no compiler was observed during the executions.
The same-renderer control changes texture bounds only. Both modes perform full
root redraw and wait for each frame's GPU completion; timings include owner
commit/publication/submission and exclude readback/native presentation. Small
cached-opacity and material-raster cases use eight 96×64 groups with four
overlapping quads each. A full-surface group is the unchanged-area control.
Three rounds rotate mode order, with six warmups and twenty measured frames per
mode per round. All raw per-frame CPU/completed samples are preserved.

This does not complete native-surface presentation, affine property trees,
complex primitives, adaptive cache admission, global physical resource budgets,
60/120 Hz input/energy validation or cross-platform acceptance.
