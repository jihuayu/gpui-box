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
The [validation-cache stage](RETAINED_VALIDATION_CACHE.md) reuses consumer checks
for cache-owned immutable pages and stable node indices, with full fallback for
identity/order/scale changes and complete effect-reference checks on property edits.
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

## Full-goal acceptance tracking

| Requested area | Current implementation | Remaining acceptance work |
| --- | --- | --- |
| Normal UI update chain | Desktop click → signal → typed property sink → transaction → layout → FramePacket → paint/persistent WGPU, with hardware pixel checks | Native-surface presentation and complete general routing |
| P0 equality and transactions | Source equality before fanout, final-value coalescing, explicit computed DAG with ordered propagation/equality cutoff, transaction-consistent reads, atomic source/derived/sink validation, nested savepoints, keyed dependency lifetime checks, synchronous post-commit effects with deduplication and replacement/scope/drop cleanup; bounded background last-value source inbox with executor wake, owner drain and stale-source cleanup; one outer frame intent, real normal-click/worker computed/effect route and idle-render assertions | General asynchronous/reentrant effect scheduling, dynamic dependency tracking, dense graph scheduling costs, generalized producer priority/backpressure/byte budgets and visible-window automatic wake/OS idle-wakeup/energy measurements |
| P0–P5 routing | Solid/decorated-quad color, paint, property, layout, reorder and decoded-image resource intents; reason records; P1 reuses commands with 0 layout/recording and stable instance dispatch; indexed producer patches and immutable-page consumer validation reuse | General primitive/material capabilities, semantic channels, general asynchronous resource invalidation and eliminate remaining full resource/order scanning |
| Logical / Render Tree | Independent generation-checked scopes, zero/multiple outputs and portals, atomic existing-node topology/removal batches and mixed-key reconciliation, multi-host keyed reorders, portal reparenting retaining scope/state/render/chunks, unified sparse render construction/topology/property edits with existing source/computed writes and binding construction/unbinding/replacement plus keyed scope/state/computed/effect construction and retirement in one validated revision, final-cycle/ownership/leaf/revision preflight, provisional ID safety, subtree source/binding/output cleanup; gallery and hardware validation | Dynamic dependency/lifetime changes during construction, asynchronous effects, event/focus/semantics lifetime and legacy islands; eliminate whole-scope/order scans and full topology validation/extraction |
| Persistent Taffy | Long-lived nodes, style/children setters, owned image/fixed/shaped paragraph snapshots, constraint-dependent wrap, baseline/overflow updates, actual leaf/measure-call counts, root cache and resize handling | General constraint-dependent custom inputs, paragraph conformance, independent islands, full node visits/cache-hit counters |
| Paint chunks / property trees | Shared local fill commands; immutable 64-item paint pages with selective local material and disjoint property-subtree extraction, indexed node patches/subtree ranges; persistent ordered page-bounds hit index with selective path updates; immutable cached glyph geometry and base-relative backend updates preserving unmodified draws; sparse transform/clip/scroll/effect chains with inherited state | General stable chunk IDs/store, full affine transforms, selective layout extraction, persistent property-map updates, tighter/adaptive spatial indexing and general hit geometry |
| GPU arena | Generic CPU allocation table with generations and deferred slot reuse, WGPU geometric growth and completion-held buffer/binding leases | General resource/material records, budgets, compaction and other backends |
| Batch GPU updates | Adjacent upload merging; 10,000-object translation and 10,000-signal color pixel verification; CPU per-signal/coalesced/typed-batch comparison; completed-work redraw A/B | Sustained 60/120Hz animation, general batch/fragmented order throughput and tuning |
| P2 property / composite | Translation/clip/scroll reuse commands and update GPU instances/hit geometry; glyph local geometry uses independent 32-byte node origin/clip records; isolated subtree opacity supports nesting and 16-byte property-only updates | General shared GPU transform/clip trees, affine properties, texture import for window presentation and NativeView geometry |
| Text / images / complex primitives | Solid/decorated quads; immutable decoded RGBA textures, base-relative image draw updates and reference-counted shared texture ownership; owned uniform-font LTR paragraphs and grayscale glyph masks, four-width chunks, system/font/scale identity, packed R8 pages, persistent 48-byte local glyph ranges, independent 16-byte color / 32-byte property slots and adjacent batch merging; typed normal clicks, clip/opacity, CPU blend and lifecycle pixels | Path/shadow types, automatic font subscriptions, robust glyph pressure/compaction, general/rich-text materials; bidi/rich-text/selection/color/subpixel text; image decoding/fit/filter/color/animation/atlas/batching and physical resource budgets |
| Flex / responsive contract | Opt-in validated Flex subset, separate layout/scale environment, threshold-equality rules, RTL/actual-window resize, bounded fixed content-box queries with dependency rejection | Broad Flex conformance/rounding, full environment subscriptions, constraint builders, general query dependency graph and adaptive structures; indexed rule dispatch |
| Damage / cache / compositor | Old/new clipped ink/effect damage; preserved offscreen target, scissored clear/replay; Full/Damage/area-threshold Auto; isolated layer content cache and cached/uncached A/B; persistent mixed composition topology/shared layer draw arrays; live-layer 256 MiB guard | Tight layer bounds, backend selective validation/invalidation, cross-frame physical memory accounting, cache admission/LRU, measured cost model, multiple damage regions, native surface buffer-age contract |
| FramePacket / frames in flight | Owned snapshots/base deltas and immutable image resources, skip-frame resync, stale rejection, single-pending producer coalescing and wake intent, bounded GPU queue, native completion worker/host wake with no idle timer, owner snapshot/texture/binding retirement without another frame, subscription cancellation/reset/resubscription, poll/finish/reset with resource reupload | Forced device/surface loss, general resource/font epochs, native surface presentation, cross-platform idle/energy qualification and deferring superseded snapshot production |

## Verification and present limits

Core regression cases cover final-value equality, batch rejection without partial
application, color/translation route coexistence, shared chunks, inherited
translation hit testing, persistent-vs-fresh Flex geometry, zero-size viewport,
subtree removal, slot reuse, invalid viewport/translation and stable child reorder.

Local material/paint publication extracts dirty visible nodes and copies affected
64-item pages plus the N/64 page directory. Visual properties extract disjoint
dirty subtrees and copy sparse property maps. Layout/visibility/order changes
still walk all live nodes and rebuild indices. Commands and unchanged pages are
shared; unchanged input reuses the complete CPU frame. Consumer item validation
reuses stable immutable pages; property edits still check full effect references,
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
