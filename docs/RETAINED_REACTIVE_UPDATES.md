# Reactive property sinks and nested event transactions

`retained::reactive` connects UI-owner signals directly to persistent render
properties. The gallery's color, scroll and isolated-opacity buttons now update
signals. Their typed sinks commit to the same UiTree, Taffy and FramePacket
consumed by ordinary paint and the retained WGPU renderer.

This implements the next P0 stage of the [complete-runtime goal](RETAINED_IMPLEMENTATION_PROGRESS.md).
It supports color, Style, translation, clip, scroll and opacity endpoints.

## Source and transaction rules

Signals have runtime identity, slot and generation. Source equality is checked
before subscriber lookup. An outer transaction keeps staged final values;
writing A→B→A removes the staged source and dispatches no sinks. Each changed
source dispatches each bound sink once. The complete sink batch passes UiTree
validation before either source values or render state is published.

Nested scopes see preceding outer writes. Returning an error rolls back the
nested scope to its savepoint; the outer caller can handle it and continue.
An error propagated from the outer scope or an invalid sink property publishes
neither source values nor render updates. Each successful outer batch produces
at most one render revision and frame-request intent. There are no callbacks
while source publication is in progress. The host calls `notify` once when
`commit.request_frame` is true.

The [computed DAG follow-up](RETAINED_COMPUTED_DAG.md) adds explicit derived
dependencies, rank-ordered propagation, equality cutoff, consistent staged reads
and keyed scope lifetime checks. The gallery color sink now consumes a derived
source through the ordinary click and paint path. General effect scheduling and
asynchronous cross-thread source delivery remain future work.

The synchronous transaction boundary is explicit. Separate host events are not
implicitly merged. Savepoints currently clone the maps of
staged shared values, so deep nesting with a large dirty set has a CPU cost.
Rollback covers returned errors; arbitrary external side effects and caught
panics within a transaction callback are outside the transaction contract.

## Binding lifetime

Binding synchronizes and validates the initial property before registering the
sink. Each node/property accepts one source writer, rejecting conflicts. One
source can feed many properties. Unbinding leaves the last render value in
place. Disposing a signal detaches all its sinks; reused source slots cannot
revive stale handles. Removing a render subtree automatically removes its sinks,
including invisible descendants. Unbound sources stay alive until explicitly
disposed, so a shared source can outlive one consuming subtree.

`ReactiveUi` exposes immutable tree access and controlled append/remove APIs,
preventing callers from bypassing binding cleanup with direct tree mutation.
[Logical scopes](RETAINED_LOGICAL_SCOPES.md) now own both writable and computed
source lifetimes. Event/semantics ownership remains pending. The gallery's GPU
mode declares the actual solid-color patch capability.

## Event and hardware validation

Twenty-one retained-WGPU core tests passed, including five reactive cases for
equality, fanout, savepoint rollback, invalid-property atomicity, foreign/stale
handles, subtree cleanup, all typed sink families and unbind/rebind behavior.
Both normal-click tests passed, including the explicit hardware case.

The event test verifies a nested color/scroll transaction produces one revision
and one view render. The equality button dispatches no sink and requests no
reactive frame. Sixteen idle event-loop drains produce no extra view render.
These are deterministic scheduling assertions, not OS wakeup or energy measurements.
The hardware click test continues to compare color, scroll and isolated opacity
pixels after the signal route was connected.

The hardware runner passes 44 pixel scenarios. Its new 10,000-signal case makes
one outer commit, dispatches 10,000 sinks, requests one frame, performs zero
layout runs, and uploads 1,600,000 instance bytes in one or two adjacent ranges.
The final image matches the original full GPUI path. A subsequent nested
write/restore keeps the packet array and submits no GPU work. Existing
clip/damage, isolated layers and lifecycle cases also pass on GTX 1650 Ti/Vulkan.

Core/gallery/runner Clippy checks passed with `-D warnings`. The retained-WGPU
desktop executable built successfully. Existing native-presentation, resource
and primitive coverage limits remain in the goal's acceptance table.

## CPU measurements

The CPU executable measures source writes, sink dispatch, coalescing and tree
commit. Frame extraction, layout, GPU work, notify processing and presentation
are excluded. Equality uses 101 samples of 1,000 transactions each; reported
percentiles describe batch averages. No subscriber traversal occurs on equality.

| Subscribers | Same-value median | Write/restore median | Sink/frame intents |
| --- | ---: | ---: | ---: |
| 300 | about 0.06 µs | about 0.22 µs | 0 / 0 |
| 3,000 | about 0.06 µs | about 0.23 µs | 0 / 0 |
| 30,000 | about 0.06 µs | about 0.24 µs | 0 / 0 |

For changed colors, three modes use identical final geometry and command data.
Three rotating rounds each include eight warmups and forty measured updates.
Median commit time for changing every node:

| Nodes | One transaction per signal | One transaction for all signals | Direct typed batch |
| --- | ---: | ---: | ---: |
| 1,000 | 0.432 ms | 0.320 ms | 0.058 ms |
| 10,000 | 5.837 ms | 5.858 ms | 2.133 ms |

Across 120 measured updates, per-signal submission creates 120×N render-request
intents; both batch modes create 120. Those intents are not actual OS presents:
the host may coalesce work. The signal batch avoids intermediate source/render
publication and reduces request intents, but did not beat per-signal CPU commit
time at 10,000 nodes in this run. Direct typed batches remain preferable for a
high-volume animation producer. No frame-rate claim follows from these timings.

The transaction accumulator was changed from per-node BTreeMap entries holding
inline Style values to stable sorted contiguous pending records with boxed
optional Style payloads. Stable grouping preserves same-node last-writer order
and validates every patch before mutation. This reduces bulk temporary storage
without weakening equality or invalid-batch checks. Signals still use ordered
maps for staged values and binding lookup; that overhead is visible above.

```powershell
./tooling/rendering-perf/run-reactive.ps1 -Output target/reactive-runtime-perf
./tooling/rendering-perf/run-retained-gpu.ps1 -Output target/retained-reactive-gpu
mise exec -- cargo test --locked --offline -p gpui-box --features retained-wgpu
mise exec -- cargo test --locked --offline -p gpui-box-example-hello --features retained-wgpu normal_click -- --include-ignored
```

[Frozen evidence](performance/2026-11-11-reactive/README.md) includes CPU samples,
GPU validation and the measured source identities. The remaining full goal is
unchanged, including separate Logical/Render Tree, general primitives and
resources, responsive contracts and native GPU presentation.
