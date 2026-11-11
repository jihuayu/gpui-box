# Retained decoded images and resource updates

This stage implements a decoded image path for the
[RFC](https://app.notion.com/p/415d83088d4e818290cddca2f104e65c).
It builds on the existing intrinsic measurement, property, damage, isolated
opacity, completion and frame-queue paths. It does not complete the whole RFC.

`ImageResource::rgba8` accepts owned, immutable, straight-alpha RGBA8 bytes.
Clones share a globally unique resource identity; a new resource gets a new
identity even when its bytes are identical. A resource has nonzero dimensions,
exactly width × height × 4 bytes and a 64 MiB individual size guard. Layout,
paint and queued frames hold no decoder, App, Window or application closure.
The caller resolves decoding, orientation and color conversion before attaching
the resource. One texel maps to one logical pixel for natural measurement.

`Transaction::image` and `Property::image` attach a resource to a leaf render
node, above its background/decorated quad. Its intrinsic image measurement is
owned by that attachment. Explicit measurement writes while the final image
is present are rejected before any transaction mutation; attaching images to
containers is rejected. Removing an image removes its intrinsic measurement,
unless that transaction supplies an explicit replacement measure. Final-value
image restores are no-ops. Logical subtree cleanup disposes its bound sources
and nodes while old immutable packets retain their resources.

Same-size image replacement produces `ResourcePatch` with
`ResourcePixelsChanged`, updates paint-item resource identity and damage, and
does zero layout and zero command recording. A changed intrinsic size also
produces `Relayout`. Resource changes remain independent of background color
and shared property changes. `content_bounds` stores resolved content-box
geometry separately from the border/background bounds.

The current retained WGPU image contract is content-box stretch, nearest texel
sampling, rectangular inherited clipping, straight-alpha upload and
premultiplied blending into the existing non-sRGB output. It uses an ordered
image draw following the node quad and before subsequent render nodes. Uniform
rounded borders decorate the background; they do not round-clip the image.
The shared effect planner places images in the same isolated opacity groups as
quads. A resource or geometry change invalidates that group's cached contents;
opacity-only updates reuse the rasterized image content.

The WGPU cache uploads one texture for each live resource identity, including
identities shared by multiple nodes. Moving/resizing/clipping an existing image
updates a 48-byte geometry uniform without texture upload. Replacing a resource
rebinds its texture. An image-free frame evicts its unused texture and draw
records. A live unique-image guard is 256 MiB; this is not a physical memory
budget including old in-flight textures, output targets and layer textures.
Submission leases retain textures, uniform buffers and bindings through GPU
completion. Reset constructs a fresh device allocation epoch and reuploads
resources from the complete owned packet. Dropped visual packets require no
separate resource-release event because ownership is carried by their Arcs.

The normal gallery has an image replacement button and typed signal sink.
Without the WGPU feature, it maintains a resource-to-legacy-RenderImage cache,
converts RGBA to GPUI's BGRA once, paints through the regular window atlas and
drops replaced atlas entries. With WGPU enabled, images and isolated groups use
the existing GPU readback → window atlas bridge so the visible window consumes
the same retained result. That bridge waits and uploads an output image; it is
not direct native texture presentation. Legacy GPUI image filtering is its own
backend behavior; nearest retained output is not claimed identical to it.

Validation covers resource/measurement routing, atomic rejection, final-value
restore, old packet ownership, logical source disposal, real click dispatch,
atlas entry retirement and hardware output. The hardware suite compares image
frames with a fresh full retained renderer (same image shader, empty cache).
Independent channel/alpha, clip, content-inset and quad-occlusion samples add an
analytical pixel oracle; these comparisons are not a wholly independent image
renderer. CPU and GPU counters verify deduplication, zero texture upload on
motion/opacity, layer invalidation on replacement, skipped-base resync,
bounded overlapping replacements, weak-owner release and device reset/scale.

[Frozen evidence](performance/2026-11-11-images/README.md) includes complete
validation, source hashes, adapter identity and a texture reuse/reupload A/B.
Both modes draw the same decoded pixels and translated geometry with full
redraw. Timing measures submit plus GPU completion and excludes decoding,
UI transaction/snapshot extraction, readback and window presentation. It also
includes replacement allocation/binding cost, not just PCIe transfer time.

Remaining image work includes decode jobs/cancellation/errors, source asset
keys and explicit invalidation epochs, DPI-aware natural dimensions, fit modes,
filtering/mips, color profiles/HDR, animation frames, rounded image masks,
atlas packing, batched image instance buffers, admission/LRU and a physical
cross-frame resource budget. Image planning/validation still scans the complete
snapshot, and one image uses one binding/draw. Forced device loss and native
surface presentation remain open. Retained paragraphs, glyph atlases, paths
and shadows are separate incomplete acceptance items in the
[implementation table](RETAINED_IMPLEMENTATION_PROGRESS.md).
