# Source-driven typed branch, match and list plans

Status: experimental explicit Rust runtime API,2026-11-18. Complete RFC remains
active. Extends [components](RETAINED_TYPED_COMPONENTS.md),
[independent regions](RETAINED_COMPONENT_SITES.md) and
[prop batches](RETAINED_PROP_BATCHES.md). No new source schema or layout/renderer
algorithm. This is a runtime primitive; component/view macros remain future work.

## Selection, inputs and one shared checkpoint

`ComponentPlan` owns heterogeneous final declarations over existing ComponentSite
handles. A plan can select typed match arms, branch between types, clear content
or declare keyed lists. Constructing it invokes ordinary Rust expressions and
selected payload factories, but exposes no LogicalEdit, source writes, layout or
mount capability. Payloads must be owned; larger resources can stay behind shared
handles. No borrowed scrutinee fields escape into components.

```rust,ignore
let sites = edit.component_sites(owner, &[10, 20, 30])?;
let producer = ComponentProducer::new(move |inputs| {
    let model = inputs.read(model)?.clone();
    let mut plan = ComponentPlan::new();
    plan.branch::<Details, Collapsed>(sites[0], host, model.expanded,
        || Ok(model.details), || Ok(()))?;
    match model.phase {
        Phase::Ready(data) => plan.select::<ResultPanel>(sites[1], host, 0, data),
        Phase::Failed(data) => plan.select::<ErrorPanel>(sites[1], host, 1, data),
        Phase::Idle => plan.empty(sites[1], host),
    }
    plan.list::<Row>(sites[2], host, model.rows.into_iter().map(|r| (r.id, r)));
    Ok(plan)
});
edit.component_producer(owner, 0, host, &[], producer)?;
```

ComponentProducer is an owner-local immutable callback handle wrapping an ordinary
Content producer. Read-only `ComponentInputs::read/read_prop` records the existing
generation-safe dynamic source observations. Standard Rust match checks types,
arm order/guards and exhaustiveness. Explicit stable arm keys supply source-arm
identity; callers do not implement a dynamic View trait. `branch<T,F>` uses keys
1/0 and invokes **only** the selected fallible Props factory. Source reads inside
that factory are tracked too. No hidden capture analysis or TLS is involved.
`show<C>` is the no-else case: false retains an empty site and skips the factory;
true selects arm1. A selected factory error aborts selection before application.
If user code catches a factory error/unwind and returns the plan anyway, its
invalid flag rejects the entire application; earlier declarations cannot partially adopt.

`LogicalEdit::apply_components(plan)` uses three phases:

1. Validate sites and reconcile all declared child lists through the existing
   typed reconciliation helper, before any payload settlement or mount.
2. Declare all final inputs in **one existing PropBatch**, including different
   Props types/sites. Settle their ordinary derived graph once. No mount or
   intermediate input read occurs between region writes.
3. Mount only new identities. Even the first new mount sees all planned final
   inputs. Existing instances update via their live ReadProp/bindings/producers.

The original `components`/`components_at` path shares the same new Prepared helper
and keeps its single-call checkpoint behavior. ComponentContext provides
`apply_components` for its direct sites and `component_producer` for an owned
render host. Plan application on a Context checks direct-parent ownership; a
low-level LogicalEdit still carries the broader explicit owner capability.

Unmentioned sites retain content. A mentioned list is its complete final content;
duplicate item keys reject normally. Repeating the same site in one plan rejects
with DuplicateKey rather than mounting an invisible intermediate declaration.
A plan cannot write both an ancestor region and a region below it: the outer
selection could retire the inner handle. A newly selected child's own mount or
producer declares its nested sites. Plan validation adds no general writer/output
discovery or cross-producer transaction inference.

## Lifetime, automatic updates and rollback

Same site/arm key/component type/host retains scope, state, live input, outputs,
effects and nested registrations. Same arm with a new enum payload replaces the
input without remounting. Different arm key or type retires the old subtree and
mounts fresh state. Distinct source arms using the same component type require
distinct explicit keys. Empty selection keeps the site and unmounts its contents.
Re-entry after committed removal is fresh; KeepAlive is not implicit.

Selectors run through the existing persistent producer scheduler after ordinary
final source preparation. Multiple state writes in one source transaction select
only the final arm; restoring the initial final value can reuse the whole plan.
Unread/unselected sources do not become dependencies; successful conditional
reads replace previous observations. Equal producer declaration requires cloning
the same handle; a newly constructed callback invalidates cached observations.

Content still requires a captured frame session, one owned render-host writer,
the existing task/edge limits and ancestry/explicit/inferred measurement DAG. A
parent producer selects several regions sharing one host; no per-region loop or
speculative repeated convergence is introduced. `after` orders non-ancestral
writers. Late changes to an already-read input still fail final source validation.
Selection and pure computation execute on the UI owner, with no preemption or
independent renderer-side branch evaluation.

Any application error/unwind poisons the outer component edit, including caught
mount errors. An automatically invoked selector error aborts the producer/frame
transaction. Final source/lifecycle/layout/geometry validation still adopts all
scopes/inputs/output together; failed selection, derived calculation, mount or
final geometry keeps the old complete UI. Effects/cleanup run only post-commit.
Arbitrary external side effects in factories/mounts, PartialEq or Drop are outside
rollback; use the existing effects contract for adopted side effects.

## Costs and remaining work

The plan holds a boxed declaration per site and a boxed Prepared dispatch record
per applied declaration, plus per-list rows/scopes/inputs/retained sets. It trades
separate per-site derived checkpoints for one shared prop batch. The dynamic
dispatch is only at selection/application, not per pixel or ordinary bound
property update. Source/derived whole-graph scans, normal reconciliation/maps,
retirement checks, producer planning previews, full logical sorting/candidate
copies and geometry validation remain. No measured timing/FPS or memory benefit
is claimed. No new persistent Scope/Node field or source schema version.

See [verification and reproduction](performance/2026-11-18-component-plans/README.md).
Automatic explicit Rust branch/match/list selection and heterogeneous per-plan
input settlement are implemented. Site placement itself is still explicitly
reconciled; view macros/automatic arm-key generation, hooks, KeepAlive, providers/
Store/Command, source-model unification, event/semantics integration, general
output ownership, complete RenderBox, resource budgets and independent native
presentation remain active. "99%" is figurative common fluid/intuitive behavior,
not a numerical success-rate or p99 gate.
