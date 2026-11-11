# Transactional computed dependencies

`ReactiveUi::computed` and `LogicalUi::computed` connect pure derived values to
the same typed property sinks as writable signals. The gallery color button
now takes the ordinary click → writable source → computed material → typed
color sink → FramePacket → paint/WGPU route. This advances P0 in the
[full implementation goal](RETAINED_IMPLEMENTATION_PROGRESS.md).

## Dependency and publication contract

Each computation declares immutable dependencies using `signal.dependency()`;
the callback receives a read-only `ComputedInputs`. Reads outside that set
return `UndeclaredDependency`. Registration validates runtime identity and
generation, computes an initial value, then installs the source and reverse
edges. It can reference only existing sources, so new registrations cannot
create cycles. Duplicate dependencies become one edge. A computed value uses a
typed `Signal<T>` for reads and sinks; writes return `ReadonlySignal`, including
same-value attempts.

Changed writable sources accumulate until the outer commit or an explicit
derived read. Pending dependents are visited in dependency-rank order, with
duplicate work deduplicated. A diamond's join runs after both changed branches
and once per settlement. Equal derived output stops propagation along that
edge; unrelated pending branches remain valid. A writable source restored
before settlement does not visit its subscribers. Sources without derived
dependents do not allocate graph dirty-root records.

`ReactiveTransaction::read` takes `&mut self`: reading a derived value settles
all current pending graph work before returning a reference. More writes after
that read may require another settlement. This is deliberately once per
settlement rather than once per outer event when the event explicitly observes
intermediate values. Regardless of reads, sinks only receive final changed
values at outer publication. A read followed by a return to the committed input
also restores every affected derived value and dispatches no sinks.

Propagation computes into a private overlay. A returned callback error installs
none of that settlement, so a caller may catch the error, correct its writable
input and retry. Nested savepoints restore staged direct/derived values, dirty
roots and counters. The complete final sink batch is validated before either
writable values, computed values or the render tree changes. Invalid sink
properties and outer callback errors publish no state or frame request.

`ReactiveStats` / `ReactiveCommit` expose computed evaluations and equal-output
counts. They describe successful settlements retained by successful outer
transactions; failed attempts and rolled-back nested scopes are excluded.
`changed_sources` includes both writable and computed final values. Callbacks
must be pure and source values treated as immutable snapshots: UI rollback
cannot undo external callback effects, hidden captured inputs or interior
mutation bypassing `set`.

## Keyed lifetime

`LogicalUi::computed(scope, key, dependencies, evaluate)` initializes once for
the scope lifetime. Key reuse retains the original callback and checks Rust
type, writable/computed kind and dependency identities. Updating a captured
non-signal value does not replace the callback; represent changing inputs as
signals. Scope reorder preserves the derived handle and all edges.

Before reconciliation removes anything, it checks the union of all removed
sources for dependents outside that set. A surviving dependent yields
`SignalInUse` before scope/render/binding mutation. All removed sources are
disposed in descending rank across scopes, independent of scope and source-key
order. Derived disposal removes reverse dependency edges and bindings; source
slot reuse increments generation. Standalone disposal likewise rejects live
dependents rather than silently invalidating another computation.

## Validation and measurements

Core tests pass 61 cases with retained-text/WGPU, and 55 with retained alone.
Six new tests cover ordered diamonds, equality, intermediate reads, nested
rollback, compute/sink failures, caught-error retry, read-only sources,
foreign/stale handles, keyed lifetime and rejected cross-scope disposal. A
96-node derived DAG is checked against independent full recomputation over 200
events containing multiple writes, reads and failed nested scopes. Recycled
slots ensure dependency rank, rather than slot index, controls ordering.

Ten real-font/gallery tests pass, including the ordinary and hardware click
flows through the new computed material. Combined and feature-specific Clippy
checks pass with `-D warnings`, and the desktop WGPU executable builds.

The hardware suite passes 139 pixel scenarios. Four new `computed-*` cases
cover equality cutoff with no submit, one nested event changing ten colors and
translations, read/restore with no submit, and invalid-opacity rollback. The
batch performs three derived evaluations, twenty final property sink writes,
one render revision, zero layout/recording and 1,600 GPU instance bytes. The
supported output matches original full GPUI rendering; the opacity rollback
case compares a fresh uncached retained renderer.

[Frozen evidence](performance/2026-11-12-computed-dag/README.md) includes source
identities, adapter, CPU raw samples and pixel results. The paired CPU benchmark
uses 1,000/10,000 render nodes in groups of ten, one writable input and one
integer bucket per group, plus one derived color per node. The control eagerly
recomputes every derived value after a changed input using the same typed sinks
and transaction machinery. It skips source-equal events. Both modes produce
equal geometry, material and commands after each round.

Three rounds rotate mode order, with twelve warmup samples and sixty measured
samples per mode/round. Very short workloads average 1,000/100/10 events per
sample for source-equal/bucket-equal/one-group; all-groups measures one event.
Timing covers source writes, computation, equality, sink dispatch and tree
commit, excluding frame extraction, GPU and presentation. Sparse updates avoid
most derived computation, while dense updates still pay ordered-map scheduling
and staging costs and are slower than eager recomputation. This does not show
an end-to-end FPS or energy improvement.

```powershell
./tooling/rendering-perf/run-computed.ps1 -Output target/computed-runtime-perf
./tooling/rendering-perf/run-retained-gpu.ps1 -Output target/retained-computed-gpu
```

## Remaining scope

This implements a static explicit computed DAG. Automatic/dynamic dependency
tracking, arbitrary effect scheduling and cleanup, cross-event coalescing and
cross-thread delivery remain open. Transaction reads settle all pending work,
including unrelated dirty branches; deeply nested savepoints clone staged maps.
Dense graph scheduling needs separate optimization and measurements. Native GPU
surface presentation, OS idle/completion wakes and the other full RFC acceptance
items remain in the main table.
