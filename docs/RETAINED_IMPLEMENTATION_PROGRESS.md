# Retained rendering implementation progress

The active implementation goal covers the complete runtime requested on
2026-11-11. Work continues in `codex/rendering-performance-research`.

The subsequent [GPU integration stage](RETAINED_GPU_INTEGRATION.md) connects
FramePacket deltas to persistent quad allocations and bounded in-flight submits.
The [clip, scroll and damage stage](RETAINED_PROPERTIES_DAMAGE.md) adds shared
property chains, normal UI scroll events and measured partial GPU redraw.
The [group-compositing stage](RETAINED_GROUP_COMPOSITING.md) adds isolated opacity,
content-cache invalidation and a correct readback bridge for normal window paint.
The [reactive update stage](RETAINED_REACTIVE_UPDATES.md) connects typed source/sink
bindings, nested event transactions, lifecycle cleanup and measured batch dispatch.
The [logical scope stage](RETAINED_LOGICAL_SCOPES.md) separates component identity
from render parentage, retaining keyed state and cleaning up portal output.
The [owned measurement stage](RETAINED_OWNED_MEASUREMENT.md) adds intrinsic image
and fixed custom inputs, baseline dependencies and actual measure-call counts.
The [responsive contract stage](RETAINED_RESPONSIVE_CONTRACT.md) adds validated
Flex styles, actual resize rules and fixed content-box container queries.
The [decorated quad stage](RETAINED_DECORATED_QUADS.md) adds uniform rounded
fills/inside borders and explicit update-route reasons.
The [material patch stage](RETAINED_MATERIAL_PATCH.md) preserves command identity
for supported solid/decorated color changes while updating the owned material.
The [visual frame queue stage](RETAINED_FRAME_QUEUE.md) coalesces pending output,
checks skipped-base resync and verifies old-snapshot completion retirement.
The [decoded image resource stage](RETAINED_IMAGE_RESOURCES.md) connects typed
resource replacement, intrinsic invalidation, shared GPU textures, ordered
image draws and completion-held texture/binding leases.
The [paragraph measurement stage](RETAINED_PARAGRAPH_MEASUREMENT.md) retains
real shaped runs, wraps against Taffy content constraints without UI callbacks,
memoizes four widths and records explicit font epochs.
The [grayscale glyph stage](RETAINED_GLYPH_RENDERING.md) attaches owned platform
masks to typed text properties, reuses packed R8 pages and persistent glyph
instance ranges, and verifies real normal-click text painting and batch uploads.
Its [material follow-up](performance/2026-11-11-glyph-materials/README.md) reduces
color-only writes to one 16-byte record per text node with zero geometry upload.
The [glyph property follow-up](performance/2026-11-11-glyph-properties/README.md)
uses 32-byte origin/scale/clip slots and 48-byte local glyph records, keeping
translation/clip changes out of the glyph geometry buffer.
The [glyph delta stage](RETAINED_GLYPH_DELTAS.md) owns immutable backend chunks,
retains unmodified draws and shared mask references, and skips per-glyph
validation/reconstruction for material/property-only updates.
The [paged Paint Store stage](RETAINED_PAINT_STORE.md) publishes local paint
changes from dirty node indices, copying only affected immutable 64-item pages
and preserving old frame snapshots.
The [subtree extraction stage](RETAINED_SUBTREE_EXTRACTION.md) extends this to
translation, clip, scroll and opacity, using indexed disjoint dirty subtrees and
the same extractor as full publication.
The [persistent property storage stage](RETAINED_PERSISTENT_PROPERTIES.md) shares
immutable AVL roots across packets, copies only changed property paths and
preserves independent transform/clip/scroll/effect identity for consumer reuse.
The [layout-output publication stage](RETAINED_LAYOUT_PUBLICATION.md) observes
actual Taffy writes, compares final results with the published layout and
patches only affected geometry/subtrees for stable visibility and order.
The [validation-cache stage](RETAINED_VALIDATION_CACHE.md) reuses consumer checks
for cache-owned immutable pages and stable node indices, with full fallback for
identity/order/scale changes and complete effect-reference checks on effect-map edits.
The [hit-index stage](RETAINED_HIT_INDEX.md) routes ordinary hit testing through
persistent ordered page bounds, with lazy construction, selective page/path
maintenance and explicit linear-scan/cold/overlap performance controls.
The [image-delta stage](RETAINED_IMAGE_DELTAS.md) retains unmentioned image draws,
updates shared texture ownership by net reference changes and handles logical
resync/resize without needless texture upload.
The [composition-cache stage](RETAINED_COMPOSITION_CACHE.md) retains root draw
order and shared layer draw arrays for stable mixed primitive/effect topology,
with current opacity values and unchanged content-invalidation checks.
The [computed DAG stage](RETAINED_COMPUTED_DAG.md) propagates explicit derived
dependencies in rank order, cuts off equal outputs, settles transaction reads
and atomically publishes derived sources with typed sinks and keyed lifetimes.
The [post-commit effects stage](RETAINED_EFFECTS.md) observes final dependencies
once per successful transaction and owns replacement/scope/drop cleanup.
The [source-delivery stage](RETAINED_SOURCE_DELIVERY.md) coalesces background
Send values before owner transactions, integrates executor readiness/waking and
checks bounded queues, stale sources, actual worker threads and native execution.
The [idle GPU completion stage](RETAINED_GPU_COMPLETIONS.md) wakes the host from
native completion waits, retires snapshots/resources without another frame and
handles subscription closure and device-epoch replacement.
The [existing-tree structural stage](RETAINED_STRUCTURE_TRANSACTIONS.md) validates
final topology atomically, batches multi-host keyed reorders into one revision,
and moves portals while retaining scope/source/render identity.
Its removal follow-up preflights whole current-subtree deletion, final host
order and mixed key retirement before cleanup, publishing one render revision.
The [unified edit stage](RETAINED_EDIT_TRANSACTIONS.md) stages render construction,
final topology/removal, properties, existing source/computed updates and new
bindings behind one validation boundary and revision, with independent logical
ownership, failure rollback, stale provisional handles and real gallery events.
Its endpoint follow-up stages unbinding/rebinding in the same draft, skips
detached sinks, cancels provisional bindings and preserves old subscriptions on
failure; equal-value replacement changes no render revision or frame intent.
The [lifecycle extension](RETAINED_LIFECYCLE_EDITS.md) constructs/retires keyed
scopes, writable/computed sources, effects, bindings and output atomically,
preflights surviving dependencies/portals, cancels draft lifetimes and defers
callbacks until all logical/reactive/render ownership maps publish.
Its measured scan follow-up skips unnecessary property replay during
construction, indexes draft binding endpoints and ranges named state/effect
retirement by scope; full topology and extraction work remain in acceptance.

The [rounded-box shadow stage](RETAINED_SHADOWS.md) adds immutable drop/inset
shadow properties, ordinary typed gallery updates, clipped overflow damage and
persistent 112-byte shadow ranges. Quad and Shadow share typed GPU storage,
geometric growth and completion-held replacement leases; cropped target shaders,
conservative Scene culling and empty fractional clips have hardware checks.

The [shared GPU placement stage](RETAINED_GPU_SPATIAL.md) keeps Quad/Shadow
geometry local and shares hierarchical layout/translation/scroll/rectangular
clip slots, with bounded deep anchors and completion-held property buffers.
It validates local/resolved snapshots, depth-changing reparenting and reuse,
fixes pixel-center clipping across target sizes, and measures the complete
owner commit/publication/submission/completion path against world instances.

The [page ownership stage](RETAINED_PAGE_OWNERSHIP.md) moves complete changed
paint pages instead of cloning their old payloads, keeps dense candidates
contiguous and shares authoritative immutable pages with sorted delta views.
Exact root identity skips duplicate delta comparisons, while public COW mutation
uses checked item lists. Original selective copying remains an A/B control;
phase measurements distinguish commit, publication and submission costs.

The [submitted-snapshot routing stage](RETAINED_CONSUMER_ROUTING.md) reuses
unchanged local Quad/Shadow inputs, updates only changed spatial records and
bounded anchors, and retains original preparation as an A/B control. Private
normal-producer provenance ties complete deltas to the actual submitted base;
unknown or modified transitions force full snapshot recovery, including missing
removals/damage. Conflicting same-revision roots are rejected before equal-frame
reuse. Hardware and phase measurements compare both consumer modes.

The [backend spatial index stage](RETAINED_SPATIAL_INDEX.md) retains validated
shader-parent adjacency/resolved values and inverse Quad/Shadow property users.
Stable updates check affected descendants and projected references, while
capacity/scale changes and invalidated aggregate bounds rebuild from actual
mirrors. CPU plans commit only after all preflight checks pass. Indexed raster
dependencies preserve the original full-scan A/B control, including adversarial
raw batches, unreferenced overflow, prefix reexposure and rejection/retry checks.
The measured unconditional prototype is retained; dense frontiers use full-prefix
validation while sparse frames retain indexed reference checks. The later dense
backend stage replaces repeated full ancestry walks with depth-ordered resolution;
the original walk remains an explicit control.
Resolved memos are invalidated across dense changes, with bounded actual-mirror
ancestor reconstruction on following sparse frames.

The [shared content placement stage](RETAINED_SHARED_CONTENT.md) adopts the same
GPU hierarchy for images and glyphs, preserves ordered immutable content insets,
keeps the existing 48/32-byte records and suppresses unchanged resource inputs.
Inverse property memberships invalidate isolated caches with zero descriptors;
projected checks cover retained content overflow and atomic binding changes.
Fractional padding, surface-only resize, raw rejection/retry, path switches,
producer-independent snapshots and completed mixed-scene controls are verified.

## Current integrated foundation

`gpui-box/retained` owns persistent rectangle nodes and a Taffy tree. Nodes carry
tree identity, slot and generation. Removing a subtree invalidates its handles.
The owner validates property transactions before applying changes, coalesces
field writes and compares final values with committed state. A transaction that
restores the previous value requests no frame.

Color, layout style, translation, clip, scroll and child reordering produce independent update
intents. Style changes enter Taffy through its normal setters. Color and
translation skip layout. Child reorder preserves node identity. Immutable
`FramePacket` values contain owned paint items and shared local command chunks.
Translation and layout reuse existing commands. Hit testing reads the same
published geometry as painting. Root translation propagates to descendants.

The desktop entry `--retained` opens a GPUI gallery. Its normal click handlers
submit transactions, call `notify` when a commit requests output, and paint the
published frame in the GPUI canvas prepaint/paint lifecycle.

```powershell
mise exec -- cargo run --locked -p gpui-box-example-desktop -- --retained
mise exec -- cargo test --locked -p gpui-box --features retained
```

This adapter declares default backend capabilities and uses ordinary GPUI paint.
The earlier WGPU single-quad upload prototype remains a separate experiment.
`InstancePatch` is a routing intent for a consumer declaring that capability.
The normal canvas adapter currently selects `RepaintChunks` for color changes.
With the `retained-wgpu` example feature, the same click and prepaint flow also
submits the frame to a persistent offscreen GPU consumer. The explicit hardware
click test passed pixel comparison at the actual window scale.

## Windows native GPU surface stage

The normal retained WGPU gallery now selects a DX12 device matching the native
D3D11 adapter LUID and paints shared immutable GPU snapshots in the GPUI scene.
The completion-safe pool checks exclusive snapshot ownership plus producer and
native read fences. It preserves cached/old frames, changed extents and explicit
reset. The normal `submit_for_window` path also detects a replaced platform D3D11
identity, advances a consumer epoch, clears GPU-side caches/arenas and replays the
complete owned frame. A matching-DX12 failure selects the existing readback
bridge. Output export avoids CPU readback and sprite upload; it still copies the
complete retained target on the GPU. The original readback mode remains an
explicit control and now handles premultiplied alpha correctly.

See [native qualification](RETAINED_NATIVE_SURFACE.md) and
[the interoperability decision](ADR_RETAINED_NATIVE_SURFACE.md). This stage does
not close visible OS presentation latency, native damage/buffer age, a
driver-induced TDR/physical device removal or cross-platform acceptance.

## Full-goal acceptance tracking

The experimental [local snapshot route](ADR_RETAINED_LOCAL_SNAPSHOT.md) now
publishes persistent local placement overrides and feeds the normal GPU canvas
without resolving every moved descendant. A private predecessor identity
authorizes spatial updates; depth-32 anchors keep shader traversal bounded.
Skipped bases, unsupported capabilities, opacity/layout/material changes and
device/surface recovery use complete resolution. The resolved path remains a
control and vector bridge. Local hit geometry is exact but linear; damage and
isolated layer extents are currently full viewport. Those costs remain in the
performance evidence and require local-space indexing/cache follow-up.
The [two-run local evidence](performance/2026-11-12-local-snapshot/README.md)
shows 80–93% ancestor completed-work median reductions against the complete
route, while 120 Hz misses and small-scene tail regressions remain. Dense
independent updates still exceed 60/120 Hz. This is not visible presentation
or modern desktop GPU acceptance.

The next [dense-page refinement](performance/2026-11-12-dense-pages/README.md)
replaces private local AVL overrides with immutable indexed pages and uses a
contiguous projected mirror for full GPU spatial validation. Alternating
previous/current binaries show 28–32% dense completed-work median improvement,
including losing ancestor-only cases. It preserves generation/snapshot
ownership and backend rejection/retry checks. This does not close the remaining
dense 60/120 Hz, effect raster reuse, native visible or software acceptance.

The [compact transport refinement](performance/2026-11-12-compact-transport/README.md)
removes PaintItem/resource cloning from local spatial updates and uses touched
dirty-bit words for arena uploads. Exact binding lookup hashes identities while
ordered source memberships preserve dispatch. Alternating binaries show 16–19%
dense submission improvement but only 0.4–5.7% completed median improvement;
commit/publication and ancestor/effects regressions remain in the report.
Two sustained runs still fail dense 60/120 Hz: independent/effects 120 Hz p99 is
39.14–39.27 ms, with 1,199 consecutive unsatisfied ticks. Current-source checks
pass 147 core / 11 example tests and 2,011 hardware scenarios / 1,921 pixel
comparisons, plus hidden native delivery/surface/device replacement. This is
partial internal transport work; modern desktop visible 120 fps, software
rendering and the full RFC remain unaccepted.

The [grouped preflight refinement](performance/2026-11-12-grouped-preflight/README.md)
now coalesces and validates one sorted node at a time while retaining the whole
final change list until atomic publication. It avoids the extra dense Pending
array without bypassing overwritten-invalid input checks or last-writer rules.
Alternating exact binaries reduce dense commit medians 21.6–25.2% and completed
medians 6.9–14.5%; an ancestor/effects median regresses 30.6%, kept in evidence.
Two sustained runs produce dense/effects 120 Hz p99 of 29.55–34.33 ms, still
1,199 consecutive unsatisfied ticks, and dense 60 Hz also fails. Hardware checks
again pass 2,011 scenarios / 1,921 pixel comparisons. This does not close visible
presentation, modern desktop hardware or software rendering acceptance.

The [visual admission stage](performance/2026-11-12-visual-admission/README.md)
adds nonblocking owner capacity observation before replaceable source fanout,
normal gallery inbox gating via existing completion wakes, epoch resubscription,
and timestamped covering completion receipts while retaining serial-only awaits.
A pipelined 60/120 Hz harness compares admission before/after construction and
preserves source skips, upper-bound misses, owner receipt delay and flight counts.
Two ordinary 800x600 runs have zero gate waits and show no stable gate benefit;
dense/effects 120 Hz p99 remains 30.08–33.68 ms with 1,199 consecutive unsatisfied
ticks. Owner receipt handling itself can lag another 17–21 ms at p99. These
measurements strengthen the need for bounded owner work and local-space raster
reuse; they do not close visible presentation, CPU rendering or 120 fps.
The one-flight 2400x1800 control records sixteen capacity waits across both
runs, including a losing before-work effects/120 Hz case with an eleven-tick
burst. Final checks pass 150 core / 11 example tests, 2,011 hardware scenarios /
1,921 pixel comparisons, hidden native source delivery and shared surface/device
replacement. Deterministic normal-host saturation, sustained visible presents,
burst/churn and CPU fidelity/responsiveness remain open.

The latest performance priority is modern desktop GPUs at 120 Hz with effects,
especially p99/max and burst stalls; software rendering prioritizes responsiveness.
The [frame-budget decision](ADR_RETAINED_FRAME_BUDGET.md) records API consequences
and explicitly unimplemented parts. Property-only publication now reuses local
payloads and batches dense persistent-map edits, retaining an original extractor
control. The sustained test uses an independent OS worker, the actual bounded
source inbox, owner wake/computed/typed sink transaction, frame queue and GPU
completion at 60/120 Hz. It counts skipped ticks as unsatisfied work and keeps
raw phase/tail samples. Synchronous completion is not visible-window acceptance.
A Windows present-attempt receipt records native status and call boundaries;
compositor/display completion and native sustained receipt correlation are open.
`RenderPreference`/`SoftwareEffects` are experimental policy declarations, not
an implemented software backend. The total goal remains active. The
[two-run evidence](performance/2026-11-12-dense-cadence/README.md) records real
regressions, a 9.26 ms small-scene spike and failed 10,000-node deadlines;
it does not claim sustained visible 120 fps.

The [slot dirty-journal stage](performance/2026-11-13-dirty-journal/README.md)
removes two ordered-set insertions per changed property using private slot-indexed
membership journals. Alternating binaries reduce dense commit medians 30.2–39.4%
and completed medians 2.2–25.2%, with small-scene regressions retained. Two sustained
runs still fail dense/effects 120 Hz: p99 32.59–33.87 ms, max 41.27–41.68 ms
and 1,199 consecutive unsatisfied ticks. All 152 core / 11 example tests,
2,011 hardware scenarios / 1,921 pixel comparisons and hidden native delivery /
shared-output / device-replacement checks pass. No visible or modern desktop
120 fps qualification or software renderer is claimed; the goal stays active.

The [dense spatial backend stage](performance/2026-11-13-dense-validation/README.md)
removes dense ordered-map construction and repeated ancestry walks while
preserving raw-record checks, overlapping last writers, unreferenced-prefix
validation and rejected-plan retry. Alternating exact binaries reduce dense
submit medians 7.0–19.7% and completed medians 2.7–15.9%; the second ancestor/effects
case regresses 33.0% in completed median. Current synchronous sustained dense/effects
120 Hz p99 is 38.80 / 28.24 ms, with 1,199 consecutive unsatisfied ticks in each
run. Same-sample owner work alone has 12.33–12.56 ms medians and every non-initial
consumed 120 Hz dense/effects sample exceeds budget before finish. The new analyzer
preserves correlated longest stalls instead of summing phase percentiles.
154 core tests, 11 ordinary example tests and 2,013 hardware scenarios / 1,921
pixel comparisons pass. All 48 pipelined cases preserve bounded flights, final
reference pixels and lease retirement, with zero capacity/resource waits. Dense/effects
120 Hz completion upper-bound p99 remains 27.37–29.36 ms and owner receipt observation
p99 37.83–39.59 ms; extra receipt handling p99 is 13.10–14.21 ms. Both admission
orders retain 1,199 consecutive unsatisfied ticks, including losing cases. Current
hidden native delivery, 97,792 shared pixels and 3,072 recovery pixels pass.
Placement-independent effects now have a proposed content/placement/raster-environment
contract, including fractional sampling/clip/resource-epoch limits; implementation,
visible modern desktop qualification and actual CPU rendering remain open.

The [dense invalidation study](performance/2026-11-13-dense-invalidation/README.md)
rejects three generic dependency-closure candidates after exact alternating
measurements: unconditional gather/sort regresses ancestor submission 15.7–35.1%;
selective gather/sort has no stable dense/effects benefit; contiguous selection
regresses dense/effects submission 23.0/30.3%. Full raw captures and changed-source
ZIPs remain archived. Runtime restores `634c8ba`, preserving the original dense
ancestry invalidation control and prior sustained baseline. Added isolated Quad
and image/glyph dependency regression cases remain. These results do not close
owner work budgets, effects raster reuse, modern desktop visible 120 Hz or CPU
rendering acceptance. The overall goal stays active.

| Requested area | Current implementation | Remaining acceptance work |
| --- | --- | --- |
| Normal UI update chain | Desktop click → signal → typed property sink → transaction → layout → FramePacket → persistent WGPU; Windows shared GPU snapshots in the native D3D11 scene, preserving normal UI ordering/clipping; the normal `submit_for_window` path detects native device replacement, advances a new consumer epoch and restores from a complete frame, with matching-DX12 construction falling back to the readback bridge | Native visible presentation qualification, driver-induced TDR/device-removed qualification, other platforms and complete general routing |
| P0 equality and transactions | Source equality before fanout, final-value coalescing, explicit computed DAG with ordered propagation/equality cutoff, transaction-consistent reads, atomic source/derived/sink validation, nested savepoints, keyed dependency lifetime checks, synchronous post-commit effects with deduplication and replacement/scope/drop cleanup; bounded background last-value source inbox with executor wake, owner drain and stale-source cleanup; one outer frame intent, real normal-click/worker computed/effect route and idle-render assertions | General asynchronous/reentrant effect scheduling, dynamic dependency tracking, dense graph scheduling costs, generalized producer priority/backpressure/byte budgets and visible-window automatic wake/OS idle-wakeup/energy measurements |
| P0–P5 routing | Solid/decorated-quad color, immutable rounded-box shadow paint with typed shared-list sinks, property, layout, reorder and decoded-image resource intents; reason records; P1 reuses commands with 0 layout/recording and stable instance dispatch; indexed producer patches and immutable-page consumer validation reuse | General primitive/material capabilities, semantic channels, general asynchronous resource invalidation and eliminate remaining full resource/order scanning |
| Logical / Render Tree | Independent generation-checked scopes, zero/multiple outputs and portals, atomic existing-node topology/removal batches and mixed-key reconciliation, multi-host keyed reorders, portal reparenting retaining scope/state/render/chunks, unified sparse render construction/topology/property edits with existing source/computed writes and binding construction/unbinding/replacement plus keyed scope/state/computed/effect construction and retirement in one validated revision, final-cycle/ownership/leaf/revision preflight, provisional ID safety, subtree source/binding/output cleanup; gallery and hardware validation | Dynamic dependency/lifetime changes during construction, asynchronous effects, event/focus/semantics lifetime and legacy islands; eliminate whole-scope/order scans and full topology validation/extraction |
| Persistent Taffy | Long-lived nodes, style/children setters, owned image/fixed/shaped paragraph snapshots, constraint-dependent wrap, baseline/overflow updates, actual leaf/measure-call and layout-write/net-change counts, root cache and resize handling; optional observed output writes from pinned vendored Taffy | General constraint-dependent custom inputs, paragraph conformance, independent islands, full node visits/cache-hit counters |
| Paint chunks / property trees | Shared local fill commands and immutable shadow bundles; immutable 64-item paint pages with owned complete-page replacement, sparse partial-page copying, authoritative indexed delta views and original selective-copy controls; selective local material and disjoint property-subtree extraction, indexed node patches/subtree ranges; persistent ordered page-bounds hit index with selective path updates; immutable cached glyph geometry and base-relative backend updates preserving unmodified draws; persistent sparse transform/clip/scroll/effect maps with inherited state, constant-time snapshot cloning and path copying; bulk construction for full extraction; actual-layout-output selective geometry/subtree extraction with stable visibility/order | General stable chunk IDs/store, full affine transforms, incremental visibility/structure/viewport extraction, tighter/adaptive spatial indexing and general hit geometry |
| GPU arena | Generic CPU allocation table with generations and deferred slot reuse; shared typed WGPU Quad/Shadow/96-byte spatial-node mirrors, geometric growth, range uploads and completion-held replacement buffer/binding leases, including actual overlapping-submission growth checks | General resource/material records, budgets, compaction and other backends |
| Batch GPU updates | Adjacent upload merging; 96-byte ancestor placement patches and bounded deep-anchor patches with unchanged Quad/Shadow geometry; 10,000-object translation and 10,000-signal color pixel verification; CPU per-signal/coalesced/typed-batch comparison; completed-work redraw A/B | Sustained tests now cover ancestor and dense independent updates with raw tail/deadline evidence; remaining work is meeting 120Hz, pipelined native visible qualification, general fragmented order throughput and tuning |
| P2 property / composite | Translation/clip/scroll reuse commands and resolved hit/damage geometry; default Quad/Shadow path shares hierarchical 96-byte GPU placement slots, bounded deep anchors and local primitive geometry with property-driven raster invalidation, stable slot references and correct reparent/removal/reuse; backend-owned indexed shader-parent/finite validation, inverse Quad/Shadow users, transactional preflight and prefix reexposure checks; image and glyph draws share the same hierarchy through ordered content offsets and existing 48/32-byte records, with exact-base input routing, inverse dependencies and retained-bound checks; isolated subtree opacity supports nesting and 16-byte property-only updates | Full affine/complex clips, adoption across other primitive types, remaining resource/order/layer dispatch, other-platform texture import, native presentation/damage qualification and NativeView geometry |
| Text / images / complex primitives | Solid/decorated quads and rounded-box drop/inset shadows with ordinary typed clicks, clipped ink and persistent 112-byte records; immutable decoded RGBA textures, base-relative image draw updates and reference-counted shared texture ownership; owned uniform-font LTR paragraphs and grayscale glyph masks, four-width chunks, system/font/scale identity, packed R8 pages, persistent 48-byte local glyph ranges, independent 16-byte color / 32-byte local or world property slots, shared content references and adjacent batch merging; typed normal clicks, clip/opacity, CPU blend and lifecycle pixels | Arbitrary paths/general-shape shadows/filters, automatic font subscriptions, robust glyph pressure/compaction, general/rich-text materials; bidi/rich-text/selection/color/subpixel text; image decoding/fit/filter/color/animation/atlas/batching and physical resource budgets |
| Flex / responsive contract | Opt-in validated Flex subset, separate layout/scale environment, threshold-equality rules, RTL/actual-window resize, bounded fixed content-box queries with dependency rejection | Broad Flex conformance/rounding, full environment subscriptions, constraint builders, general query dependency graph and adaptive structures; indexed rule dispatch |
| Damage / cache / compositor | Old/new clipped ink/effect damage; preserved offscreen target, scissored clear/replay; Full/Damage/area-threshold Auto; isolated layer content cache and cached/uncached A/B; persistent mixed composition topology/shared layer draw arrays; clipped ink bounds cached per immutable paint page, nested bounded textures with per-target cameras, persistent same-extent textures/buffers/bindings with queue-ordered 48-byte origin updates and raster invalidation, live/allocated texture payload and raster-area counters; configurable unique layer-allocation payload cap across live cache/in-flight epochs, pressure wait/retirement, observational resident/retired/peak counters | Remaining layer, resource-ID, budget and lease scans, driver/other-resource/global device-epoch physical memory accounting, cache admission/LRU, measured cost model, multiple damage regions, native surface buffer-age contract |
| FramePacket / frames in flight | Owned resolved/local-placement snapshots and ordered content offsets with page-sharing indexed base deltas, exact-snapshot agreement checks, private normal-producer transition provenance, untrusted/missing-delta full recovery and conflicting-same-revision rejection, plus immutable image resources, skip-frame resync, stale rejection, single-pending producer coalescing and wake intent, bounded GPU queue, native completion worker/host wake with no idle timer, owner snapshot/texture/binding retirement without another frame, subscription cancellation/reset/resubscription, poll/finish/reset with resource reupload; Windows shared output snapshots with producer/native read fences and completion-safe bounded allocation reuse; platform D3D11 identity replacement advances a full-resync consumer epoch while old immutable snapshots retain their original allocation ownership | Driver-induced TDR/physical device removal, general resource/font epochs, native visible presentation and other platforms, cross-platform idle/energy qualification and deferring superseded snapshot production |

## Verification and present limits

Core regression cases cover final-value equality, batch rejection without partial
application, color/translation route coexistence, shared chunks, inherited
translation hit testing, persistent-vs-fresh Flex geometry, zero-size viewport,
subtree removal, slot reuse, invalid viewport/translation and stable child reorder.

The resolved compatibility path for local material/paint publication extracts dirty visible nodes and copies affected
64-item pages plus the N/64 page directory. Resolved visual properties extract disjoint
dirty subtrees and copy only modified paths in persistent property maps. The
experimental local snapshot route instead retains unchanged descendants and
publishes sparse local placement overrides for stable property-only frames.
Stable-visibility/order layout updates observe actual Taffy output writes and
extract only changed items or moved/scrolled subtrees. Structural/visibility/order
and viewport changes still walk all live nodes and rebuild indices. Commands and unchanged pages are
shared; unchanged input reuses the complete CPU frame. Consumer item validation
reuses stable immutable pages; changes to the effect map still check full effect references,
and composition topology reuses unchanged pages/layer arrays. Backend resource-ID,
layer invalidation and completion-lease scans remain before a sublinear complete
scene-update claim.

The initial style surface exposes the pinned Taffy Style. The full gpui-box Flex
API and constrained CSS subset remain pending. Geometry currently contains
floating-point border bounds, translation, rectangular clip, scroll and isolated opacity. Resources and
accessibility are tracked above for their subsequent implementation stages.

The progress table uses implementation and verification evidence. The active goal
remains open until the complete requested flow and corresponding acceptance work
are finished.

## Foundation validation

- Eight core tests passed with `cargo test -p gpui-box --features retained`.
- The GPUI `normal_click` integration test passed. It locates the rendered buttons,
  dispatches mouse clicks, runs the event loop and checks the actual published
  frame. Changing color changes the display commands. A temporary color restored
  within one transaction retains the frame revision and shared paint-item array.
- The desktop executable built successfully. Core and shared-example Clippy
  checks run with `-D warnings`.
- The existing CLI and Android host lifecycle tests passed, three cases total.

The GPUI integration test uses `gpui/test-support`, which adds its upstream test
and benchmark dependency closure to Cargo.lock. Native screenshot verification
and cross-device qualification remain in the acceptance table.
