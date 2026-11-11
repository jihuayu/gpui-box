# Immutable glyph geometry and base-relative updates

This continues [retained glyph rendering](RETAINED_GLYPH_RENDERING.md) and moves
the RFC's chunk/resource and P1/P2 paths beyond upload-only savings. Previously,
every submit expanded every text node's glyphs, revalidated mask identities,
rebuilt instance/page-run arrays and compared them, even when only color changed.
The GPU payload was small but CPU work still scaled with all glyphs.

`GlyphGeometry::new` validates and owns private local glyph records and mask Arcs.
It records unique mask references and local origin extrema. An `Arc<GlyphGeometry>`
is immutable; callers cannot replace its fields. The text chunk lazily converts
to this backend representation once, with a `OnceLock`. It retains the exact CPU
glyph array as the conversion source: subsequent array replacement is rejected,
and the extra array owner prevents in-place mutation after conversion. Backend
geometry retains bitmap bytes independently of the UI chunk, without an Arc cycle.

`GlyphUpdates` has reset, removed IDs and changed node batches. Sequential packets
send only `FramePacket.updates` plus removal IDs; skipped-base/new-tree/reset
submits supply the complete live draw set. A live node with no final text removes
its previous glyph draw. Unchanged backend draws retain geometry, page runs,
instance ranges and material/property slots. A same-geometry update compares
only constant-size node material/properties and reuses the previous draw. It
does not expand, revalidate resources for, or rewrite the local glyph instances.
Extrema validate node transform finiteness without walking every glyph.

On a changed geometry, validation checks unique immutable mask definitions
against cached resources and all incoming definitions before mutation. Draw/mask
counts are computed from replacement/removal deltas and checked against existing
live guards. Mask references count one owner per draw per unique resource;
duplicate glyph occurrences do not multiply mask storage. Preparation releases
old owners, protects all incoming shared owners, then frees zero-reference tiles
and packs new ones. This ordering handles same-transaction resource swaps without
evicting shared textures. Page reclamation scans existing tiles only when some
tile is actually retired. Color/property-only updates do not scan the mask cache.

The existing queue order, completion leases and renderer failure/reset contract
remain in force. Invalid duplicate/conflicting updates or resource redefinitions
are rejected before backend mutation; a valid submit can follow the rejection.
Reset of the graphics backend reuploads masks from immutable packets; a logical
full resync on an intact backend may reuse matching geometry/atlas allocations.
Hidden/removed nodes retire draws and references; restoring a sole hidden owner
reuploads masks rather than relying on stale tiles.

New counters report submitted text node batches, glyphs in newly admitted
geometry validation and reconstructed instances. A color-only update has one
batch and zero glyph validation/rebuild counts. Translation/clip with the same
geometry likewise reuses it. An opacity-only packet needs no text batches.
Changing wrapping/resource identity re-enters geometry validation and planning.

Validation includes 45 core tests, ten real-font/normal-click tests and 112
hardware pixel scenarios (29 text cases). New hardware cases cover partial
updates retaining unchanged draws, shared-owner removal, skipped-base reuse,
display-none retirement and restore. Additional direct backend assertions check
atomic resource swapping, immutable-ID redefinition, duplicate/conflicting
updates, recovery after rejection and malformed geometry constructors. Existing
CPU mask blending, multiple atlas pages, in-flight replacement and scale/rounding
checks continue to pass. WGPU with/without text, the non-WGPU example Clippy and
desktop build pass; Clippy uses `-D warnings`.

[Frozen evidence](performance/2026-11-11-glyph-deltas/README.md) records real-mask
same-run retained/recreated geometry comparisons at 450 and 4,500 glyphs, exact
pixel equality, raw samples and scope. This isolates a backend/local-geometry
cost from shaping and complete UI snapshot construction. It does not establish
an end-to-end frame-rate improvement or reconstruct the full legacy GPUI path.

Remaining scans are explicit: FramePacket publication/validation still walks
paint items, composition planning traverses draw order and layers, glyph-draw
validation forms a live ID set, and completion leases collect live pages. Whole
scene/chunk indexing, selective extraction and general semantic/resource routing
remain required. Geometry conversion duplicates local CPU records in a bounded
four-width cache; retained physical memory, fragmentation fallback/compaction,
shared GPU property trees, rich text and native presentation remain open.
