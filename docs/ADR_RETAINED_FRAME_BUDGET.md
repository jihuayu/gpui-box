# ADR: Desktop frame deadlines and responsive software rendering

Status: active implementation; scheduling and publication optimizations exist.
An experimental local-first snapshot now feeds the normal GPU gallery. An actual
cooperative CPU raster prototype exists; its native integration, native sustained
presentation and complete local hit/damage/cache design remain open. This
decision does not close the RFC acceptance goal.

## Requirements

The primary target is modern desktop GPUs at 120 Hz, with an 8.333 ms frame
interval, including decorative effects. Avoiding burst stalls takes priority
over a favorable average. Report p50/p95/p99/max, missed deadlines, obsolete
visual samples and consecutive unsatisfied target ticks. Software rendering
prioritizes responsiveness, retaining layout, clipping, group opacity, text,
hit geometry, state transactions and lifecycle semantics.

This host's Ryzen 7 4800H / GTX 1650 Ti is a regression baseline, not a modern
desktop GPU qualification machine. Native display timing and GPU-completed
offscreen timing are distinct evidence.

```mermaid
flowchart LR
    Source[Independent source / input] --> State[Owner state transactions]
    Clock[Absolute rational frame clock] --> Sample[Latest visual sample]
    Sample --> State
    State --> Scene[Local paint and property snapshot]
    Scene --> GPU[Desktop GPU consumer]
    Scene --> CPU[Experimental cooperative software consumer]
    GPU --> Complete[GPU completion receipt]
    GPU --> Native[Native scene and Present attempt]
    Native --> Display[Compositor/display confirmation: pending]
    CPU --> Native
```

## Decisions affecting framework APIs

1. Keep state transaction/event ordering separate from visual sampling. A host
   may skip obsolete animation samples, never arbitrary business commands or
   resource/lifecycle operations. `FrameClock` has absolute rational deadlines
   and reports skipped visual samples; it creates no polling timer or worker.
   Existing `UpdateSender` is explicitly last-value delivery and is exercised
   with a real independent OS producer and owner waker in sustained tests.
2. Bound visual pending work before expensive packet construction. The existing
   one-packet `FrameQueue` only bounds already-built output; it cannot undo CPU
   work spent producing superseded packets. Keep GPU in-flight limits and
   completion-safe resource ownership independent of animation cadence.
3. Avoid making fully resolved per-descendant world geometry the permanent
   primary API. Today's `PaintItems::get`/index return `&PaintItem` and every
   item owns world coordinates. Therefore moving one ancestor still publishes
   O(N) descendant geometry even when GPU upload is 96 bytes. A subsequent
   local-first snapshot must expose explicit `resolve_geometry(node)` and
   consistent hit-test/damage traversal, with shared immutable local chunks and
   property ancestry. Preserve old snapshots and complete resync. The new
   experimental `LocalFrame` now implements publication and a direct GPU
   spatial consumer with those explicit resolution APIs. Damage remains full,
   hit traversal linear, and opacity/layout/material/skipped-base transitions
   recover through full resolution. See the [local snapshot decision](ADR_RETAINED_LOCAL_SNAPSHOT.md).
4. Retain the current packet contract during measured optimizations: reuse
   owned local payloads and ordered f32 geometry operations for property-only
   updates; aggregate sparse property edits across disjoint subtrees; rebuild
   dense persistent maps once rather than repeatedly copying AVL paths. Keep
   `SelectiveReextract` as a same-runtime baseline. Hash lookup tables do not
   define paint order; explicit preorder and ordered property maps still do.
5. Separate requested rendering preference from actual backend capabilities.
   Experimental `RenderPreference` describes intent only; `SoftwareJob` consumes
   `SoftwareEffects` but no native host automatically activates CPU fallback.
   Software may omit decorative box shadows only with explicit application
   permission. It must not approximate semantic group opacity, clipping, text
   content or event geometry. A real CPU raster path and fidelity/performance
   tests are required before calling software rendering supported. WGPU
   software adapters and GPU framebuffer readback do not establish that path.
6. A native Present attempt has its own receipt: window-local sequence,
   device epoch, monotonic call start/return, native status and normal-queue
   acceptance. Occlusion is a non-error status but is not queued display work.
   Failed/skipped draws clear old attempts; recovery clears feedback and
   advances epoch. Unsupported platforms return `None`. A Present-return
   timestamp is never named display completion or input-to-photon latency.

## Admission and quality contracts still requiring implementation

### Implemented owner-side capacity observation

`WgpuFrameRenderer::poll_frame_capacity` now performs nonblocking completion
collection and reports device epoch, actual retained GPU flight count and
configured limit. The single owner may inspect it before replaceable source
fanout/snapshot work. It is an observation, not an escaping reservation: another
submit or device replacement invalidates it. Layer/resource admission and
driver submission can still wait or fail. Keep lossless events outside this
gate. No arbitrary callback gains a deadline/preemption guarantee.

The normal gallery gates only its explicitly last-value background inbox. A
full GPU leaves those values in the inbox; the existing completion task wakes
the owner waiter. There is no polling timer, extra completion worker or mutable
tree crossing threads. Normal click/lifecycle operations remain immediate and
atomic. Epoch closure reopens a completion subscription before waking the
waiter; an error terminates the visual delivery task explicitly.

`CompletionReceiver::ready_receipt` adds a native-wait observation instant and
serial, preserving the existing serial-only `ready`. One coalesced event covers
earlier submissions; its timestamp is a conservative upper bound for their
completion. The harness reports owner receipt observation delay separately.
It cannot recover exact per-submit GPU times, native Present or display times.
Two in-flight work items improve overlap only when the measured workload can
benefit; CPU-bound dense fanout may leave admission idle.

```mermaid
flowchart LR
    Inbox[Latest visual inbox] --> Gate[Owner capacity observation]
    Gate -->|room| Fanout[Atomic source and sink work]
    Fanout --> Snapshot[Local snapshot]
    Snapshot --> Submit[Bounded GPU submit]
    Submit --> Worker[Existing completion worker]
    Worker --> Receipt[Timestamped covering receipt]
    Receipt --> Gate
    Events[Lossless UI events] --> Fanout
```

### Remaining admission and software contracts

Desktop GPU is the primary target. A missed budget must first trigger bounded
visual sampling, reduced redundant state/snapshot work and reuse of unchanged
effect content; it must not silently turn off application-requested effects.
The 120 Hz budget covers arrival-to-completion age as well as throughput. A
deeper GPU queue cannot be accepted as a latency improvement unless frame age,
pending work and p99/max also pass. Sustained validation must include bursts,
not just a continuously warmed stable scene. Surface size, scale, primitive
mix, effects and resource working set belong to every acceptance result.

CPU rendering prioritizes owner/input responsiveness. Backend capability
negotiation must distinguish supported exact operations, explicitly permitted
decorative simplification and unsupported operations before frame work starts.
It must not call GPU readback or a WGPU software adapter a CPU renderer. Merely
declaring `SoftwareResponsive` does not activate a renderer or enforce a budget.

The experimental resumable software job owns immutable input, yields at work-unit
boundaries and exposes cancellation/completion. General resource epochs are open.
Only a complete internally consistent output may replace the previously shown
frame. Discarding superseded visual jobs must not discard state transactions,
cleanup or resource releases. Input/hit geometry must identify the shown scene;
invisible partially rasterized state must not silently become the event target.
One oversized primitive or allocation must have an explicit outcome instead of
an unbounded owner-thread loop. These contracts now have a CPU prototype and
synthetic fidelity/cadence evidence; native responsiveness and broader fidelity
remain required before API stability.

Preconstruction admission must happen before source fanout/snapshot building
for replaceable animation work, while lossless owner events still settle in
order. It needs a host demand/deadline, bounded pending/in-flight capacity and
readiness wakeups with no idle polling. Resource or effect working-set admission
is a separate condition from frame cadence. Current `FrameClock`, last-value
inbox and postconstruction `FrameQueue` cover only parts of this contract.
No arbitrary callback may be named budgeted solely because it receives a
deadline; non-yielding user code cannot be preempted safely by that interface.

## Alternatives and consequences

Using average FPS hides long stalls. Sleeping relative to completed work hides
missed arrivals and accumulates drift. The sustained harness instead uses an
independent absolute-clock producer and reports source skips/coalescing/misses.
Synchronous per-frame GPU waits provide a conservative completion scope and
must be complemented by pipelined throughput and visible presentation tests.

Making the GPU faster alone cannot remove CPU packet publication or owner
scheduling stalls. Implicitly lowering all effect quality risks semantic
differences. Increasing queue depth can improve throughput but worsens input
age and memory pressure; queue bounds, explicit simplification and local-space
composition are preferred architectural controls.

The property-only resolver still clones resolved items and scans changed
subtrees. Dense rebuilding can copy unchanged map entries; it is selected only
above 64 real edits and above a quarter of existing entries. This is a measured
heuristic, not a universal threshold. Rerun on modern desktop hardware before
adopting performance claims or tuning defaults.

The local snapshot route now uses indexed immutable placement pages rather
than the resolved resolver's AVL maps. Full backend spatial validation also
uses a contiguous projected mirror, preserving all checks. The
[dense-page evidence](performance/2026-11-12-dense-pages/README.md) compares exact
previous/current binaries with unchanged layer policy. These changes reduce
dense CPU cost but do not remove owner transaction work, full validation,
driver/completion waits or the effect-layer raster/cache design limitations.

The [compact transport evidence](performance/2026-11-12-compact-transport/README.md)
keeps spatial transport private, removes paint/resource clones and sorts touched
dirty-bit words for GPU ranges. Source-local ordered memberships remain the
dispatch authority when exact binding lookup uses a hash table. Dense submit
medians improve 16–19%; commit/publication regress and total gains are small.
Sustained independent/effects p99 remains about 39 ms at 120 Hz. No application
API should promise frame deadlines from reduced upload volume alone. Dense
state preparation, preconstruction admission and local-space effect content
identity remain necessary, independently of software backend implementation.

Dense property preflight now prepares one grouped node at a time
after the existing stable sort. It retains every final validated change until
the whole batch succeeds; it does not publish per-node state. This avoids a
second dense array of wide pending material/resource/property records and
redundant identity lookups. Each overwritten input is still checked, last writer
order within each node is stable, and source/sink publication remains atomic.
The choice among errors in a multiply-invalid transaction is not a priority or
event-order API. The [grouped preflight evidence](performance/2026-11-12-grouped-preflight/README.md)
shows 21.6–25.2% dense commit median and 6.9–14.5% completed median improvement
in alternating binaries, with an ancestor/effects regression retained.
Sustained dense/effects p99 remains 29.55–34.33 ms and still fails 60/120 Hz.
A reduced temporary working set alone does not establish deadline compliance.

Private slot-indexed membership journals now replace ordered dirty sets in the
owner, retaining complete node generations and both local/resolved consumer
accumulation lifetimes. Insertion order is never paint/event/transaction order.
The [journal evidence](performance/2026-11-13-dirty-journal/README.md) shows
30.2–39.4% dense commit median reduction, but dense/effects 120 Hz p99 is still
32.59–33.87 ms. No deadline guarantee or unchecked caller batch API follows.

### Separate owner blocking from completion delay

The frame-budget analyzer derives commit + publication + submission for each
individual raw sample before computing percentiles. It also derives source age
at submit and the residual after submit. It never sums phase percentiles or
labels that residual GPU execution time: driver work, finish/wakeup scheduling
and previous queue work may contribute. Submission is an owner-blocking span,
not a pure CPU instruction measure. Superseded ticks remain outside consumed
sample distributions and count independently as unsatisfied work.

In the journal-stage baseline, independent/effects 120 Hz owner spans alone have
12.76–13.19 ms medians and 15.80–16.39 ms p99. Every non-initial consumed sample
already exceeds 8.333 ms in owner work (527/586 samples), before synchronous
finish. Ancestor/effects owner spans have about 1.3 ms medians. These results
support separate owner-work and resource/completion budgets: a faster raster
GPU alone cannot establish dense 120 Hz. Per-frame submission may still include
driver waits; do not infer a hardware-independent CPU lower bound. A CPU renderer
must avoid running raster/effect jobs inside those lossless owner transactions,
and must preserve an immutable complete shown frame while bounded worker jobs
yield or are superseded. Implemented backend negotiation/cooperative raster and
visible input responsiveness tests remain required.

Run `python tooling/rendering-perf/analyze-frame-budget.py <cadence-directory>`
against completed raw captures for reproducible same-sample accounting.

### Current dense preflight outcome and next ordering

The [dense backend stage](performance/2026-11-13-dense-validation/README.md)
keeps the full-walk control and reduces dense submission medians 7.0–19.7%
in alternating exact binaries. Current sustained dense/effects owner medians
remain 12.33–12.56 ms at 120 Hz; p99 is 15.73–23.71 ms. Every non-initial
consumed sample (514/618) is already over budget before finish. One run's
source-to-completion p99 worsens to 38.80 ms while the other improves to
28.24 ms; sequential historical comparisons cannot establish causality.

All 48 current-version pipelined cases have zero capacity and layer-budget waits.
Dense/effects 120 Hz completion upper-bound p99 is 27.37–29.36 ms; source-to-owner
completion observation p99 is 37.83–39.59 ms. Receipt handling alone adds
13.10–14.21 ms p99. Before-work admission loses in the second process and wins
slightly in the first with no waits, so this is not an observed gate benefit.
Keeping the owner runnable is independently necessary for resource retirement
and future interaction responsiveness. Native hidden regressions pass but do
not qualify visible presentation. Physical input latency is still unmeasured.

Prioritize the following API-relevant work before tuning GPU queue depth:

1. Preserve local content and placement separately for isolated effects, with
   sampling/clip/resource epochs in reuse eligibility. Dense independently
   moving children still invalidate their content; ancestor motion may qualify
   for reuse. See the proposed spatial ADR contract and exactness acceptance.
2. Bound owner construction and consumer dispatch costs before promising a
   frame deadline. A backend submit currently includes many node/resource/layer
   checks and may wait on driver or resource admission. A capacity observation
   is not a nonblocking submission reservation or a cost guarantee.
3. Define producer visual demand separately from lossless state transitions.
   Replaceable samples may remain pending until admitted; a ten-thousand-sink
   transaction must never be half committed merely to yield to a deadline.
4. Implement an actual software consumer with bounded jobs and capability
   outcomes. Keep input/lifecycle owner work runnable, retain a complete shown
   frame, and explicitly associate interaction geometry with that scene. The
   GPU offscreen measurements do not qualify CPU responsiveness.

### Dense invalidation probes: avoid permanent API from a losing optimization

The [dependency-closure follow-up](performance/2026-11-13-dense-invalidation/README.md)
tries to reuse validated final-topology membership for raster invalidation.
The unconditional inverse-user gather/sort probe regresses ancestor submit
medians 15.7–35.1%. Restricting it to dense submitted transport still yields no
stable dense/effects gain (both submit medians lose 0.8%). Both exact source ZIPs,
raw captures, binary identities and the unconditional sustained deadline failures
are preserved. The contiguous reference-selection candidate also regresses dense/effects
submit medians 23.0/30.3%; restore the `634c8ba` runtime and retain the added
hardware regressions.
These results are evidence against exposing application-selected dirty dispatch
strategies or assuming a single frontier policy is optimal for all animation.

Keep ancestor transforms as a semantic sharing opportunity and independently
changing children as real content changes. Backend policy may select between
validated equivalent algorithms, but must retain atomic failure/retry, complete
snapshot recovery and exact clip/isolation/resource behavior. No unchecked batch
API or automatic decoration simplification is justified by these probes.

### Completion retirement is an independent owner event

The [pipeline burst follow-up](performance/2026-11-13-pipeline-completion/README.md)
shows that waiting exclusively for the next visual source delays already-ready
completion handling. The benchmark now services both channels on the owner;
the native gallery already uses a separate completion task. Retirement and
device/lifecycle handling must remain runnable without another visual update.
This contract applies to GPU completion and cooperative software-job completion;
it does not authorize partial commits or imply a nonblocking submit guarantee.

Do not expose an application-selectable completion policy. An owner executor
should service completion independently of replaceable source cadence and
retire snapshots/resources on that owner. A single polling host may await either
source or completion with cancellation-safe readiness; it must inspect completion
before admitting more work when both are ready. No timer-driven polling loop
or extra rendering worker is needed by this harness correction.

Serviced120Hz handling p99 falls to0.50–1.14ms, but completion upper-bound p99
stays22.56–24.77ms and three of four completion-loss comparisons worsen.
Dense owner-span medians remain20.10–22.03ms. Queue depth/admission timing is
not a demonstrated remedy; prioritize total necessary work and bounded owner
scheduling. Native visible display/input and CPU fallback acceptance stay open.

## Remaining acceptance

The [transaction attribution study](performance/2026-11-13-transaction-phases/README.md)
adds compile-time-only source settlement/sink/tree/publication spans and nested
property preflight/application spans. Dense120Hz sink medians1.87–2.15ms,
preflight1.45–1.82ms and application1.07–1.28ms dominate settlement0.024–0.028ms.
This shared-source/10k-sink fixture is not a general graph benchmark. Inspect
sink construction/dispatch and staged property representation next, retaining
complete atomic validation, writer order and independent resource lifetimes.
Three diagnostic120Hz p99 pairs worsen; timing/layout/serialization overhead
is explicitly retained. No speedup or transaction-splitting policy is implied.

The [preflight capacity probe](performance/2026-11-13-preflight-reserve/README.md)
counts sorted node groups to reserve the final wide change list. Reversed pairs
improve six dense commit medians and regress two; total owner improves four and
regresses four. All four120Hz p95 comparisons worsen, with both round2loss totals
worse. Restore the original runtime: fewer growth copies do not establish lower
latency, especially with an added scan and unnecessary capacity on equal groups.
Attribute settlement/fanout/preflight/application/cleanup before the next change;
the atomic complete transaction contract remains unchanged.

- Local-first indexed damage/hit performance and general incremental updates;
  affine/complex clip support. The experimental snapshot's correctness path
  currently uses full damage and linear hit traversal.
- Local-space layer cache keys/placement so ancestor movement can retain
  unchanged effect raster content instead of rerasterizing each layer.
- Native visible 60/120 Hz sustained source-to-present qualification, accurate
  per-scene receipt correlation, refresh/occlusion information and a supported
  compositor/display observation path. Physical input latency is separate.
- Pipelined measurements, larger pixel extents, text/images, resize/resource
  churn and cold cache/allocator/device-recovery spikes.
- Experimental [CPU consumer](ADR_RETAINED_SOFTWARE_CONSUMER.md) now implements
  progressive raster jobs; general capabilities, native integration and software
  responsiveness/fidelity acceptance remain open.
- Modern desktop hardware runs, multiple independent runs and idle/energy data.

References: [progress](RETAINED_IMPLEMENTATION_PROGRESS.md),
[GPU placement](ADR_RETAINED_SPATIAL_GPU.md),
[native surface](ADR_RETAINED_NATIVE_SURFACE.md).
