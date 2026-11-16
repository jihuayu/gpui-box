# Scope-owned responsive rule lifecycle

Date: 2026-11-16. Implemented public subset; the full RFC remains in progress.

`LogicalEdit` can now declare, update and remove responsive rules in the same
atomic edit that creates their keyed scopes, sources and render nodes. A sparse
registry on `LogicalUi` owns rules by `(ScopeId, key)`; scope retirement detaches
their style writers. This extends the [explicit responsive construction
stage](RETAINED_STRUCTURAL_RESPONSIVE.md), whose caller-owned rule sets remain
available with their prior contract.

## Public API and ownership

- `responsive_rule(scope, key, source, target, query, styles)` declares a new rule
  or updates the existing declaration at that identity. The target must be
  render output owned by that exact scope. Sources and query hosts can belong
  to other live scopes, subject to dependency validation.
- `remove_responsive_rule(scope, key)` removes metadata and detaches its exact
  Style binding. The source and last style remain. A new declaration or ordinary
  typed binding can claim the endpoint in the same edit.
- `responsive_registered(environment)` evaluates all surviving registered rules
  inside `edit_frame` or `edit_local_frame`, before dependent builders. It reuses
  the existing viewport/container dependency stages and final observations.

Declaration/removal is also available in plain `edit`, which validates the
registry but does not evaluate breakpoints or produce a frame. Equal
declarations preserve the writer and avoid a registry copy. Changing breakpoint
styles or query metadata retains that writer; changing source or target
requires explicit removal and redeclaration. Duplicate registered sources or
targets are rejected, and ordinary endpoint binding conflicts are still checked.

Keys are scoped declaration identities, not global IDs or GPU handles. Rules
persist when omitted from a later render call, just like existing scoped state;
they disappear on explicit removal or retirement of the owning scope. Sources
are not implicitly created/disposed by rule removal. The registry contains no
Window, Taffy or backend resources.

## Construction example

This fragment assumes an empty root with the framework Column contract. The
same edit creates the rule source, target and responsive metadata, then selects
the style before building its content:

```rust
let (commit, frame) = ui.edit_local_frame(
    constraints,
    paint_viewport,
    |_| Ok(()),
    |(), edit| {
        let branch = edit.reconcile_keys(root_scope, &[branch_key])?[0];
        let base = LayoutStyle::column().fixed_inline_size(70.0);
        let host = match edit.renders(branch)?.first() {
            Some(host) => *host,
            None => edit.append_layout(branch, root_node, base.clone(), 0)?,
        };
        let source = edit.state(branch, 1, || base.clone())?;
        edit.responsive_rule(
            branch,
            1,
            source,
            host,
            LayoutQuery::ViewportInline,
            LayoutBreakpointStyles::new(
                base.clone(),
                vec![(150.0, base.fixed_inline_size(130.0))],
            )?,
        )?;
        edit.responsive_registered(&environment)?;
        Ok(host)
    },
)?;
```

A ContainerInline declaration can reference another provisional host in the
same draft. Dependent rules may be declared first if their current structure
and all registered alternatives satisfy the existing proof. Rules do not
provide a way to inspect an invalid intermediate tree; declaration previews
must satisfy structural/leaf/constraint-box invariants. A full/inline builder
can follow selection at its explicit call site. Layout environment, actual
constraints and finite paint viewport remain separate.

## Atomicity and retirement

```mermaid
flowchart LR
    A[Current keyed draft] --> B[Declare or remove owned rules]
    B --> C[Retire rules of dead scopes]
    C --> D[Explicit responsive selection and builders]
    D --> E[Registry and final observation validation]
    E --> F[Publish sources, scopes, rules and frame]
    F --> G[Run final effects and cleanup]
```

The rule map is copied lazily on its first mutation. New bindings remain inside
the existing reactive draft. Registration validates ownership, conflicts and
dependency proofs before installing its writer or modifying the map. A caught
invalid registration therefore leaves the endpoint available for another
declaration. Failed reservations can consume identity counters, as with other
provisional handles, but cannot become published writers.

Final keyed edit validation includes all surviving rules, even if no selection
was requested. The registry is installed only after reactive/render validation
and before effect delivery. A source error, missing host/target, invalid proof,
observed writer removal, closure error or final finite-geometry failure
preserves the prior registry and owner state. The [responsive-stage
savepoint](RETAINED_STRUCTURAL_RESPONSIVE.md) still protects source settlement
when a selection error is caught by the caller. Arbitrary callback side effects,
panic and shared resource memo-cache warming remain outside rollback.

Retirement handles rules declared in that same edit as well as published rules.
Creating then cancelling a branch before selection leaves no rule/writer.
Replacing a keyed branch filters dead rules before validating new provisional
targets; evaluation detaches dead writers before resolving surviving rules.
End-of-edit retirement also works when no responsive selection is requested.
The existing non-draft `LogicalUi::reconcile_keys` path likewise preflights and
retires rules for the deleted scope set, including rules using surviving sources.

A surviving declaration cannot retain a retired source or query host. Portal
output remains owned by its logical scope, and moving it out of a deleted
render host does not authorize a dangling ContainerInline reference. Remove or
update that rule before retiring the dependency. Removing a rule after an
earlier responsive call does not erase its observation; such an edit rejects
if final writer/proof validation fails. Build/selection sequencing is explicit.

## ADR: owner-local declarations with explicit evaluation

**Status:** implemented.

**Context:** a caller-owned `LayoutResponsiveBindings` could not atomically
refer to provisional sources/nodes or retire with a keyed branch. Mutating an
external collection during a failed edit could leak rules and escaped handles.

**Decision:** keep a sparse owner registry with stable scoped keys, validate a
private draft and install it with the existing source/logical/frame commit.
Retain explicit evaluation and the existing proof/DAG, rather than executing
user builders again or inferring dependencies from observed numeric equality.

**Alternatives:** separate registration commits expose partial state and cannot
atomically create branch output. Storing mutable references to caller rule sets
inside the edit complicates rollback and ownership. Implicit retirement by
omission would change the existing scoped-state persistence convention. A full
automatic scheduler still requires public builder/dependency contracts.

**Costs:** `LogicalUi` adds one sparse BTreeMap header; each registered rule owns
a scope/key, source/target/query identities, writer and breakpoint styles. The
first metadata mutation clones the whole registry. Non-equal declarations build
full private tree previews and run existing rule/sink/proof scans; final edits
with registered rules run another whole preview/preflight. Selection clones a
flat rule list and retains existing per-stage previews, source savepoints and
final observations. Retirement scans rules/scopes and validates exact writers.
No per-layout-node field, GPU format or indexed automatic planner is added.
There is no timing, allocation reduction or tail-latency improvement claim.

## Verification and remaining scope

See [validation record](performance/2026-11-16-scoped-responsive-rules/README.md).
Independent CPU/GPU full references cover new keyed branches with new sources
and two responsive rules; real bundled Noto output exercises scope-owned
declarations through repeated narrow/wide selection. Failure, retirement,
cross-scope references, endpoint reuse and old snapshots are checked separately.

Still open: automatic cross-builder/query scheduling and environment
subscriptions; dynamic dependency/lifetime changes beyond these rule
declarations; general full-axis content-sized builders and complete RenderBox;
custom allocator query contracts; incremental planning/rollback and cached
proof dispatch; broad Flex/intrinsic/percentage/cyclic/baseline/device rounding
conformance; general resource/effect/native lifecycle and performance acceptance.
Modern desktop GPU effects, CPU smooth complete-frame delivery, sustained
60/120Hz and display-visible input tails remain open in the
[full implementation table](RETAINED_IMPLEMENTATION_PROGRESS.md).
