# Inferred measurement prerequisites for layout construction

Status: experimental framework capability, 2026-11-18. The complete RFC remains
active. This extends the existing once-only LayoutBuildPlan and persistent
LayoutProducer scheduler; no public builder/producer type or source schema is
added. "99%" remains the user's figurative direction for fluid/intuitive UI.

## Behavior and API

An automatically measured title/toolbar can affect the space assigned to a
body. Previously [full Flex measured siblings](RETAINED_MEASURED_FLEX_SIBLINGS.md)
validated that dependency, but a caller also needed to sort tasks manually or
declare after. Now the existing planner derives those known layout prerequisites.

For each LayoutBuildKind::Allocation task on the proven ordinary-Flex path,
the planner obtains the same content-dependent sibling subtree inputs used by
the builder. Any queued task whose host is a node in one of those subtrees must
finish before that Allocation task. Tasks at the title root and tasks deeper in
its subtree both qualify. Registration order and numeric IDs do not override
that prerequisite. An explicit after for the same pair deduplicates normally.

```rust,ignore
plan.add(LayoutBuildTaskId(1), body, LayoutBuildKind::Allocation, &[], build_body)?;
plan.add(LayoutBuildTaskId(90), title, LayoutBuildKind::InlineAllocation, &[], build_title)?;
// Proven title measurement affects body: task90 runs before task1.
```

Persistent producers enter this same planner, so declaring the body registration
first no longer requires a forward producer ID merely to order title measurement.
Actual source reads and callback reuse still use the original producer contract.
The certified whole-plan no-change path remains intact.

## Structural proof and publication

The planner uses current source/render draft topology and the current captured
frame constraints, including before the first frame. It reads pure immutable
layout inputs only; it does not run layout, intrinsic algorithms or callbacks
while deriving edges. The existing independent-parent/containment proof keeps
precedence and supplies no new inferred edges in this stage. A path that is not
yet proven can be established by an earlier ancestor task; actual builder proof
still runs immediately before the callback and remains the authority.

Ancestry, explicit and inferred measurement prerequisites share one graph. Ready
tasks retain deterministic ID order. New descendant tasks added by a title
callback join the next graph and precede a still-pending body automatically.
Each callback has one execution opportunity; no numeric fixed-point replay.

- Cycles among explicit/inferred/ancestor edges reject before any callback in
  the currently queued graph. Inferred edges count toward max_dependency_edges
  and peak_dependency_edges, deduplicated with the other edge kinds.
- A newly added predecessor cannot retroactively precede an already executed
  body; that edit rejects. It cannot be rescued by an equal measured size.
- Final executed-order validation now re-derives the same complete graph from
  final topology/inputs and checks ranks and limits. It does not check only
  original explicit/ancestor order. Kind and limit data remain transaction-local
  callback-free observations.
- Normal full builder observations still validate complete actual inputs and
  measured sibling styles/structure/resources before source/keyed/frame
  adoption. Scheduling does not replace those checks. Errors poison the outer
  plan/edit, preserving ordinary rollback and immutable old frames.

This infers conservative dependencies from proven layout measurements and host
placement, not arbitrary closure write sets. A task with a host outside the
observed subtree can still imperatively edit it; such nonlocal source/output
prerequisites need explicit after. Final checks reject late changes. Tasks in a
measured subtree may create an edge even when their callback only changes paint;
the scheduler does not guess callback purity or suppress those edges.

## Costs and remaining work

Every graph derivation builds temporary task-host/ID maps and reuses the full
allocation proof, which scans/temporarily clones content-dependent sibling
layout records. The graph is rebuilt after every task and at final validation;
builder previews and the final candidate/geometry checks remain. This can be
costly for many related tasks. No index, byte/time budget, new kernel node or
renderer record is introduced; there is no timing/FPS/memory improvement claim.

[Verification](performance/2026-11-18-inferred-measurement-tasks/README.md) covers
reverse registration/IDs, nested title tasks, dynamic descendants, producer
transactions/same-value reuse, explicit cycles, edge limits, retroactive work,
normal complete output and real Noto wrapping. Explicit GPU output compares
with numeric full Flex references and old frames survive owner retirement.

General source/read/write dependency inference, independent output ownership,
fine-grained related-producer invalidation, query/custom-parent scheduling,
content feedback on the host path and complete RenderBox remain open. Component
source integration, resource budgets and independent native presentation remain
active. Finite correct output does not establish native displayed smoothness or
continuous GPU/CPU cadence.
