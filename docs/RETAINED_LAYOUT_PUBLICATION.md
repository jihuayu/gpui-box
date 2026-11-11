# Selective publication from actual layout outputs

Layout updates now reuse the existing Paint Store when render identity, child
order, visibility and viewport stay stable. Taffy still computes the complete
dependency-correct layout through its persistent caches. The owner observes
actual unrounded result writes, deduplicates changed identities and compares
final results with each node's last successfully published layout. A style
input change whose final geometry is unchanged can publish zero extracted
items, preserving complete paint/property snapshots.

This is an extension of [selective property extraction](RETAINED_SUBTREE_EXTRACTION.md)
and [persistent property maps](RETAINED_PERSISTENT_PROPERTIES.md). It is not a
replacement layout algorithm, independent layout islands, or proof of
constant-time complete UI updates.

## Layout output to paint dependency contract

- A relative location change dirties the complete visible subtree: descendants
  may have unchanged cached relative layouts but different world positions.
- A change to the clamped scroll offset also dirties the subtree, including
  changed overflow caused by another node's layout. The background stays at its
  own position while descendants use the current scroll extent.
- A size, padding or border change without location/scroll movement extracts
  that node alone. It updates quad bounds, image destination, paragraph width
  and glyph chunk; any children whose relative geometry changed are observed
  separately. Unchanged descendants retain their pages.
- Property dirty roots and layout movement roots share the existing sorted,
  disjoint preorder ranges. Single-node geometry candidates contained inside a
  dirty subtree are skipped. Independent material/resource edits join the same
  publication. Old/new ink and effect damage and GPU deltas retain their prior
  semantics.
- Layout result fields unused by paint, such as an unchanged box's overflow
  when its clamped scroll does not move, need no geometry extraction. The final
  layout baseline still advances after publication for the next comparison.
- Visibility/display changes, append/remove/reparent/reorder, viewport changes
  and first publication conservatively rebuild the complete snapshot and
  indices. Changed hidden nodes have no visible indexed candidate. Restoring
  visibility rebuilds from current final styles/resources and identities.

The owner keeps a generation-safe Taffy-ID→render-ID map and removes entries
with their layouts. Failed provisional IDs are never registered. Removal clears
pending observed IDs, preventing reuse from targeting an old render lifetime.
Published layout baselines update only after successful frame extraction; old
FramePackets remain independent immutable snapshots. Existing failure behavior
for user measurement/resource validation remains unchanged.

## Fixed-version Taffy observer

The workspace now patches `taffy =0.15.0` to the [vendored source](../vendor/taffy/README.gpui-box.md).
Its original package hashes, registry checksum, exact Git revision and MIT
license provenance are retained. Only `tree/taffy_tree.rs` changes: a generic
observer receives each unrounded write's node/old/new result. Original APIs
pass a statically dispatched no-op. Layout algorithms, cache behavior and
rounding remain unchanged. The retained owner disables rounding, as before.
Rounded-layout consumers cannot treat these writes as final rounded output.

`LayoutStats` adds `layout_writes`, `changed_layout_writes` and
`net_changed_nodes`, alongside actual leaf-layout/measure callbacks. Writes
include equal or transient repeated values; net changes compare unique final
results with the published baseline. These counters are not complete node
visits or cache-hit counts. Responsive two-pass frames aggregate each pass's
work, so their net-change total is a sum across publications, not unique nodes
over the whole event.

## Verification and limits

Five new core cases compare selective/full output, including forty rounds of
Flex movement, nested clipping/effects/scroll, intrinsic inputs and materials;
sixty-four rounds combine Grid, Block, percentages, min/max, margins, absolute
positioning and owned baselines. Tests also cover one-item parent/leaf resizing,
old page sharing, equal final geometry, hidden updates, generation reuse,
structure/viewport fallback and observed versus original Taffy entry points.
The core suite passes 113 combined-feature tests, gallery/font passes 12 with
hardware ordinary-click assertions for selective measurement/paragraph updates,
and Clippy passes `-D warnings`. Retained-only passes 104 tests, and the desktop
WGPU executable builds. Taffy's default-feature library suite passes
169 tests. Its no-default-feature test module has an existing ungated `calc`
test compile error, documented in the vendor readme; distribution tests validate
the actual selected feature set.

The hardware suite passes 171 pixel cases on Windows / GTX 1650 Ti Vulkan.
Nine additions cover one-size update, dependent siblings, wrap/scroll, changed
scroll clamp, inherited movement/material, decoded-image attachment/resize,
visibility fallback and real glyph wrapping that moves a sibling. Existing
click, damage, skipped-frame, resource and completion-lifetime checks pass.

[Frozen CPU evidence](performance/2026-11-12-layout-output/README.md) compares
selective publication with forced full extraction on the same persistent tree
and Taffy execution. It includes layout recomputation, observer/deduplication,
extraction and producer snapshot retirement. It excludes GPU/consumer/window
presentation. Real dependency movement may touch a large fraction of a scene:
the benchmark intentionally keeps that case visible. Paint-page directories,
Taffy's own ancestor/sibling work, remaining backend scans, rounded behavior,
native presentation and cross-platform latency remain in the
[full acceptance goal](RETAINED_IMPLEMENTATION_PROGRESS.md).
