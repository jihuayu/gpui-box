# Indexed reuse of accepted producer plans

Status: experimental framework optimization, 2026-11-18. Extends
[persistent producers](RETAINED_LAYOUT_PRODUCERS.md); full RFC remains active.

## Decision

The first persistent producer implementation invoked the entire layout plan on
every transaction even when all content callbacks reused their observations.
That still cloned draft trees, validated the DAG and measured every builder,
then enabled tracked final layout. A business source with no producer readers
could therefore trigger layout construction work unrelated to its output.

The owner now maintains a generation-safe reverse index from observed Signal
dependencies to `(ScopeId,key)` registrations. Index entries come from actual
successful callback reads, including conditional changes. A new index is built
only when the registry/observations change and is installed after successful
complete validation, together with registry/cache/frame/source state. Failed
construction cannot install tentative dependencies. Index keys do not own old
source values; the existing observations still do. Observations now share an
immutable Rc map across registry/task copies; recording a new callback read
uses copy-on-write. The first unequal candidate ends eligibility checking,
since any mismatch already requires the full plan.

After automatic responsive settlement, the scheduler may reuse the **entire
previously accepted plan**, with no per-builder previews/observations, only when:

- The registry has no tentative changes or invalidations and responsive
  declarations are unchanged.
- Logical scopes/order are unchanged, and frame constraints/viewport equal the
  last producer session.
- The actual TreeEdit has no staged node/structure/property/pixel-policy edits,
  the accepted revision matches, and the owner has no pending layout/full
  extraction invalidation.
- No new/detached binding or changed source with property bindings can synchronize
  unseen output. Changed-source-local binding sets provide this certificate,
  without scanning all live bindings.
- The reverse index selects readers of final changed source identities and
  retirements, and every selected registration's complete observed source values
  still match typed equality. Unchanged computed outputs and restored final
  values are already excluded by ordinary transaction settlement.

These are internal facts, not caller dirty flags, width guesses or hashes. No
callback can run on this branch and introduce an unobserved output write. The
same source candidates are rechecked after final responsive/source retirement.
The complete candidate/frame geometry validator and source/effect publication
remain. All old observations were accepted against this exact unchanged tree
and input; rerunning Taffy proofs cannot reveal a new structural fact here.

Any uncertainty runs the ordinary complete plan. Explicit equal property writes
also use that path; this certificate intentionally does not interpret staged
patch equality. Any related source change schedules all registrations because
producer callbacks may write outside their host subtree. This is whole-plan
reuse, not partial producer filtering or output ownership inference. Resize,
topology/style, responsive output changes, registration replacement/invalidation
and background binding changes retain ordinary measurement/proof/order checks.
Convenience local_frame(viewport) follows that same session-aware entry.
Internal standalone resolver access explicitly invalidates producer acceptance
before exposing the mutable reactive owner, even if eventual source adoption
does not change paint revision. The next logical edit runs a complete plan.

## Public diagnostics and costs

LayoutProducerStats adds plans_skipped, source_candidates and planning_previews.
Skipped plans count callbacks_reused for all retained registrations and zero
considered, since no builder input was evaluated. planning_previews counts only
the scheduler's preview calls; it excludes responsive, builder and final work.
source_candidates describes the index eligibility check, not all source scans.

The index costs a BTreeMap entry per observed generation and BTreeSet memberships.
Registry/observation adoption rebuilds it in O(total observed reads); this can
increase changed/dense-update cost. Unrelated-source checks cost changed-source
lookups plus selected-reader equality, but ordinary owner candidate cloning,
source/binding collection, final geometry checks and responsive settlement remain.
This does not make complete UI transactions O(1). No public dirty hint, source
schema, layout solver, GPU instance or producer callback signature changes.

Alternatives: partial filtering would need a complete callback output-domain
and sibling writer dependency contract; the existing arbitrary LogicalEdit
capability does not supply one. Comparing a projected full tree would consume
much of the work being avoided. Unconditionally skipping based only on input
width would lose property/topology/lifecycle edits. The strict internal negative
certificate gives useful reuse now and leaves those broader contracts explicit.

## Evidence and open scope

See [release controls, correctness and reproduction](performance/2026-11-18-producer-invalidation/README.md).
Tests cover source-conditional indexing, final equality, binding synchronization,
responsive output, explicit patches, resize, retirement/generation, rollback,
and real glyph CPU/GPU output against independent full references.

Related updates still incur complete plan work and index rebuild. Per-output
ownership, partial plan scheduling, general content feedback/full RenderBox,
global memory/time budgets, native independent presentation, common input p99,
modern desktop effects and continuous complete CPU frames remain open. Finite
offscreen correctness or CPU measurements do not establish99%fluency or sustained
native60/120Hz.
