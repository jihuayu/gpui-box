# Private lazy ink translation proof

Status: implemented inside the experimental, default-disabled local tight index.
Original per-item ink recomputation remains a consumer control. Full goal active.

The previous local tight-layer experiment spends O(descendants) CPU work to
derive new world ink on ancestor movement. This can erase raster savings. Keep
the immutable local snapshot as content authority and move aggregate ink only
when its exactness is provable without visiting descendants each frame.

## Decision

The consumer's segment index retains clipped ink, a conservative uncut geometry
envelope, integer arithmetic eligibility, and lazy translation per aggregate.
The uncut envelope includes invisible boxes because ancestor origins still
participate in descendant arithmetic. Eligibility initially excludes images and
glyphs, fractional layout/translation/scroll/clip or noninteger shadow geometry,
and coordinates beyond 65,536 logical units. These restrictions keep additions,
subtractions and min/max exact in the bounded integer regime. The feature does
not infer correctness from a near-equal floating-point result.

The private snapshot verifies the exact predecessor identity, unchanged topology
and payload, and exactly one changed placement within the selected subtree: its
root translation only. Opacity, scroll, clips and other placement fields must be
equal. More than 64 changed placements bypass proof attempts, bounding sparse
frontier inspection and avoiding quadratic dense selection. The root ancestry
is resolved exactly; the current inherited clip must contain the uncut envelope
at both old and new origins. Thus no previously clipped content can be exposed
by moving a truncated union. Relevant external clipping falls back to recompute.
Private snapshot provenance, not an application dirty range, supplies authority.

Eligible motion updates O(log N) segment records and does not resolve descendants.
Later partial queries accumulate lazy offsets. A subsequent child edit pushes
lazy offsets before replacing leaf records, preserving current subtree unions.
Fallback computes compact geometry records, not a retained vector of paint items.
Full recovery rebuilds all proof metadata. Missing proof never skips validation.

The prototype keeps full GPU spatial validation and full backend layer content
comparison. It optimizes target-bound derivation only, not the remaining GPU
content proof, image/glyph sampling, native present, owner sink dispatch or dense
independent changes. Its additional raw/exact/lazy index arrays increase CPU
memory and cold-build work; they are not charged to the GPU texture payload cap.

## API implication and controls

No new application state/chunk API is required. An immutable snapshot can retain
content-local placement facts and select equivalent consumer algorithms privately.
`set_local_ink_translation_enabled(false)` selects recomputation for measurement;
the outer `set_local_tight_layer_bounds_enabled` remains false by default. It is
not yet justified to enable a general API contract that every ancestor move has
constant work: arbitrary fractional arithmetic, clips, scrolling, resources and
independent descendants still need different handling and measured admission.

## Evidence and remaining work

Core tests compare lazy queries with complete geometry after forty alternating
integer moves, then fractional child edits, parent movement, clip and scroll
changes. Mixed hardware controls retain exact text/image/fractional fallback.
Alternating 10,000-leaf integer captures compare the same executable with/without
lazy ink. Ancestor/effects submission medians improve 56.1/69.8%, completed
medians improve 60.7/51.5%, with zero resolved ink items and 10,011 translated
nodes. Dense-independent/effects cannot qualify and submission loses 4.2/7.7%;
its second completed median also loses 9.2%. Preserve these losses and scope.
This fixture deliberately fits uncut integer content within the viewport and
differs from the older fractional/full-viewport-boundary benchmark. Do not
attribute differences between those fixtures to this algorithm.

Sustained independent 60/120 Hz arrivals, tail/max and source deadline accounting
are recorded separately. They measure owner transaction/publication/submission
through synchronous GPU completion, not native display or physical input.
Modern desktop hardware, fractional proof, general persistent local content,
resource epochs, native visible performance and CPU fallback integration remain
open. See [raw evidence](performance/2026-11-13-ink-motion/README.md).
