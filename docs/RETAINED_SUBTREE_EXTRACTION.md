# Selective visual-property subtree extraction

Visual property changes now use the [paged Paint Store](RETAINED_PAINT_STORE.md)
instead of rebuilding the complete scene. Full extraction records each visible
node's preorder index and subtree end. Translation, clip, scroll and opacity
commits collect dirty roots; sorting their ranges removes nested duplicates.
Each disjoint affected subtree is extracted once from the persistent Taffy
layouts, alongside any independent dirty materials. The [layout-output follow-up](RETAINED_LAYOUT_PUBLICATION.md) extends selective
publication to stable-visibility/order layout changes. Visibility, viewport
and child-order changes still rebuild the complete snapshot and indices.

The full and selective routes share one extractor. A subtree starts with its
unchanged parent's published origin, intersected clip and property ancestry,
including the parent's clamped scroll offset. Old property entries inside the
subtree are removed before extraction; newly active chains replace them.
Removing a transform, clip, scroll or opacity node correctly reconnects its
descendants to inherited chains. The [persistent-map follow-up](RETAINED_PERSISTENT_PROPERTIES.md)
replaces the original whole-map copy with shared immutable AVL paths. Extraction
sets or removes each current entry directly, retaining equal entries and roots;
GPU property resolution remains separate acceptance work.

Comparison only visits extracted candidates. Paint changes enter the GPU item
delta. Property ancestry changes patch the immutable pages even if geometry is
unchanged; effect-value changes compute old/new damage without rewriting quad
instances. Existing isolated-layer planning receives the complete current
property maps. Simultaneous ancestor/descendant changes extract once, hidden
property changes retain the items/property Arcs until layout restores visibility,
and previously published frames preserve their geometry and property values.

Two new core tests compare 40 rounds against forced full extraction across 381
nodes, including nested/disjoint property writes, parent scroll/clip context,
property removal, opacity transitions, root movement, material mixing, damage,
delta order and hit testing. Hidden subtree restore is covered separately.
The core suites pass 50 combined-feature / 47 retained-only tests. Ten shared
example tests including real-font and hardware normal clicks pass, as do the
desktop WGPU build and combined/feature-specific Clippy `-D warnings`.

The hardware runner passes 121 pixel scenarios. Six added checks exercise nested
fractional movement, clipping/clamped scrolling, nested isolated opacity,
independent material plus leaf movement, removal of parent properties and full
visual restore. Comparisons use the original full GPUI renderer for plain quads
and a fresh uncached retained renderer for isolated opacity. The original text,
image, damage and resource-lifetime regression scenarios also pass.

[Frozen evidence](performance/2026-11-11-subtree-extraction/README.md) includes
51 verified source hashes, raw timings and pixel artifacts. In the 10,101-node
fixture, one leaf translation extracts 1 item (5.4 µs median publication versus
17,420.7 µs full), while a 101-item subtree translation extracts 101 (54.1 versus
15,267.6 µs). These CPU timings exclude GPU validation/planning/presentation and
do not establish application FPS. Property maps, page directories, consumer
validation/order, hit testing and layout still have broader traversal costs.
