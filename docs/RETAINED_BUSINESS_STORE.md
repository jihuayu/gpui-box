# Business snapshots and scoped UI selectors

Status: experimental explicit Rust framework API,2026-11-18. Complete RFC remains
active. Implements the first business Store boundary from
[runtime architecture](UI_RUNTIME_ARCHITECTURE.md), using the existing
[component plans](RETAINED_COMPONENT_PLANS.md) and source/frame transaction.
The follow-up [typed Command transport](RETAINED_TYPED_COMMANDS.md) supplies bounded
requests/results/cancellation; execution policy, providers and cross-language
Store schema remain separate work.

## Business ownership and complete revisions

`gpui_box::store` is available without retained/desktop/GPU features. It contains
`StoreWriter<S>`, `ReadStore<S>`, `StoreSnapshot<S>`, `StoreRevision`,
`StoreSubscription<S>`, `StoreCommit` and `StoreError`. State is owned Send+Sync
data; it must not be mutated through interior mutability after publication.

The unique non-Clone writer belongs to the application executor. Build/validate
the next state outside publication; `commit(expected_revision,next)` rejects a
foreign or stale revision and atomically replaces the whole snapshot. Equal
state uses PartialEq outside the lock and returns changed=false without a new
revision or wake. State comparison/allocation and user payload destruction stay
outside the publication lock. One Store is one atomic boundary; independent
stores have no implicit joint revision.

```rust,ignore
let mut writer = StoreWriter::new(Document { title, body, saving: false })?;
let read = writer.read_only();
let base = read.snapshot()?;
let next = Document {
    title: next_title,
    body: next_body,
    saving: false,
};
let committed = writer.commit(base.revision(), next)?;
// This proves business commit, not UI adoption or display.
```

StoreSnapshot retains an Arc of the entire state; cloning is constant-size.
Equality is store/revision identity, independent of field type and UI revision.
Old snapshots remain readable after replacement and Store disposal. ReadStore
has no setter, writer conversion or UI handle; it can be moved/cloned across
application threads. A compiler control rejects calling commit on ReadStore.
Optimistic expected revision is still required even when replacement equals the
current state, so a stale business operation cannot silently claim success.

## Latest-whole-snapshot observation

Each subscription records a consumed revision and one optional executor Waker,
not a queue of field patches or an unbounded event list. pending/take_latest
captures the latest complete committed state. ready integrates with a host
executor; no poll timer/thread is installed. Store commit wakes subscribed
executors after releasing the state lock; it never synchronously evaluates
selectors or invokes UI callbacks. Publication scans live subscriptions to wake
them, so cost is O(subscription count), not O(1) fan-out.

`pending()` plus `acknowledge(captured_revision)` supports slow/fallible consumers:
a newer commit arriving during UI work remains pending when the older consumed
revision is acknowledged. `take_latest()` is the convenience consume-at-once
operation. Dropping/cancelling a subscription removes it and wakes a waiter.
Dropping the writer closes observation, but subscribers can consume the last
unseen state before ready returns false; ReadStore/old snapshots stay readable.
Poisoned locks and stale/foreign acknowledgement return explicit StoreError.

Only visual observation may skip intermediate complete revisions. Business
events, audit history and Command results must use a separate reliable protocol.
This is deliberately distinct from UpdateSender's per-source latest values.

## UI bridge, selectors and lifetime

```rust,ignore
let mut input = ui.attach_store(owner_scope, 10, &read)?;
let title = ui.select_store(owner_scope, 11, input.prop(),
    |document| Ok(document.title.clone()))?;
// A ComponentProducer reads title with inputs.read_selector(title), and returns
// an ordinary ComponentPlan; a component can bind/select through its Context.

// At an admitted host update opportunity (possibly after input handling):
if let Some(applied) = ui.apply_store(&mut input)? {
    assert_eq!(input.applied_revision(), Some(applied.revision));
}
```

`attach_store` captures one snapshot into a keyed scope-owned read-only source;
occupied keys reject rather than silently rebinding a provider/store identity.
`StoreInput` is owner-local and exposes the same ReadProp plus ready and the last
successfully applied business revision. All selectors over this input therefore
read one captured immutable revision for the entire UI transaction, even if the
application commits again concurrently. Two separate StoreInput instances are
separate consumption boundaries; use one shared input for a consistent UI update.

StoreSelector is an owner-local read-only handle over an ordinary computed source.
select_store exists on LogicalUi, LogicalEdit and ComponentContext; read_selector
on those owners, ComponentInputs and ComputedInputs; bind_selector connects the
existing typed Property. Equality cutoff, rank ordering, bindings, effects and
dynamic producer observations remain the existing source machinery. Same keyed
selector reuse keeps its original function/dependencies; it does not install a
second graph or reevaluate components on every Store revision.

`apply_store` samples at most one latest whole snapshot and stages it through the
existing source helper inside LogicalUi::edit. All dependent selectors settle
before component producers/layout/output validation. Unchanged projections can
skip component selection and keep the published visual frame, while still
advancing the business input revision. Ordinary state-only/no-session hosts keep
their existing request-frame/explicit frame retrieval protocol; an active
producer/responsive session prepares and validates complete output automatically.

Failed selector/derived/component/layout/geometry adoption keeps the old input,
sources, state and frame and **does not acknowledge** the business revision.
The host may retry, report the error, or accept a newer valid revision; no busy
retry loop is installed. Business commits remain committed if the UI rejects
them. StoreApply reports source/UI adoption; no GPU/display evidence is inferred.

The scoped input source owns a cancellation token. Successful scope retirement
or UI disposal cancels visual subscription even if the host still holds the
StoreInput handle. Failed candidate retirement does not cancel it. A disposed
source/generation cannot accept a later business snapshot. Dropping StoreInput
also cancels observation, leaving the last adopted UI snapshot frozen until
ordinary scope disposal. None of these operations closes the business writer.
Cross-scope surviving selector dependencies retain the existing SignalInUse
disposal rejection; provider resolution/lifetime is not guessed from state type.

## Costs, controls and remaining work

See [verification/reproduction](performance/2026-11-18-business-store/README.md).
Each business commit compares State, allocates its immutable Arc and scans live
subscriptions. Retained historical snapshots can retain large States; no global
byte budget/persistent keyed collection/delta log is supplied. Snapshot sampling
uses a short Mutex lock. UI adoption still pays the shared prop staging/derived
scans, producer plan/candidate/logical ordering/layout/geometry costs. Selector
work is owner-local and has no preemption budget. No wall-time/FPS/memory benefit
is claimed. No solver/vendor/GPU format change; source input schema remains6.

Implemented: one-writer complete Store revisions, read-only subscriptions and
scoped explicit UI bridge/selectors. The follow-up Command stage adds queue/
tickets/cancellation/results. Remaining: provider/StoreKey resolution, application collections
and task integration, multi-input coordinated admission, serialized cross-language
schema, hooks/macros/KeepAlive, complete semantic input model, resource budgets and
independent native presentation. Complete RFC stays active. "99%" is a figurative
direction toward common fluid/intuitive UI, not a numeric success ratio or p99 gate.
