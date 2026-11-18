# Source-driven content tasks in the ordinary UI build plan

Status: experimental Rust framework capability,2026-11-18. Complete RFC remains
active. Extends [once-only plans](RETAINED_LAYOUT_BUILD_PLAN.md) and
[persistent producers](RETAINED_LAYOUT_PRODUCERS.md) with a layout-independent
content kind. This is a runtime primitive for frontend components, not completed
component macros, ReadProp, Store/Command or a new UI source model.

Follow-up: [ReadProp](RETAINED_READ_PROPS.md) adds live read-only payload handles,
keyed draft replacement and ordinary computed/binding/effect integration. It
keeps mount-created state/outputs while same-key inputs change. Full component
contexts/sites/macros and shared source integration remain open.

## The missing direction

Ordinary component structure often depends on data, not on its allocated width
or height. A conditional detail panel, keyed result list or changed text can be
constructed before Flex/Grid measures its natural size. Requiring every such
callback to obtain an independently proved layout slot rejects normal automatic
sizes for no dependency reason: this content does not read that slot.

`LayoutBuildKind::Content` now receives `LayoutBuildInput::Content`, with no
dimensions. The same task/producer registration, host identity, scope ownership,
dynamic source reads and final transaction boundary apply. The normal persistent
kernel measures the resulting tree. No containment or numeric root anchor is
inserted, and content-dependent Flex/Grid parents need no allocation proof for
this kind. Dimension-dependent builders keep all their existing proofs.

```rust,ignore
let producer = LayoutProducer::new(move |input, edit| {
    let LayoutBuildInput::Content = input else { unreachable!() };
    let model = edit.read(rows)?.clone();
    let keys: Vec<_> = model.iter().map(|row| row.id).collect();
    let scopes = edit.reconcile_keys(list_scope, &keys)?;
    for (scope, row) in scopes.into_iter().zip(model) {
        update_or_mount_row(edit, scope, host, row)?;
    }
    Ok(())
});
ui.edit_local_frame(constraints, paint_viewport, |_| Ok(()), |(), edit| {
    edit.layout_producer(list_scope, 0, host,
        LayoutBuildKind::Content, &[], producer)?;
    Ok(())
})?;
// Future ordinary source transactions enter the same registered plan.
ui.transaction(|tx| tx.set(rows, next_rows))?;
```

Content may also be registered in a borrowed once-only LayoutBuildPlan and may
append dynamic descendant tasks through its existing queue. These two public
enums add a variant; exhaustive frontend matches must handle it. Source schema
remains6. No Taffy types, Window, GPU or mutable foreign callbacks enter messages.

## One scheduler and publication contract

The planner still validates host existence, one task per host, ancestry and
explicit prerequisites, complete cycles, limits, dynamic additions and final
executed order. Ancestor content tasks precede descendant tasks regardless of
ID. A Content title task in a measured Flex sibling subtree also precedes its
dependent full Allocation body through the existing inferred measurement graph.
No separate content queue or replay-to-convergence loop is introduced.

The Content dispatch invokes its callback once after the planner's ordinary
current-draft preview. It does not call a constraint/allocation builder or add
an incoming-layout observation for this task. A callback that explicitly uses
another builder still acquires that builder's normal observations. Arbitrary
nonlocal writes still require explicit after; this is not callback output
ownership or general read/write dependency discovery.

Persistent Content producers reuse the same source observations and reverse
index. Reads through edit.read, state and computed record generation-safe typed
values; successful conditional changes replace the old read set. Same-value
final writes and unrelated source changes can reuse accepted content/plans.
Resize/render changes remain conservatively handled by the existing complete
planner even though Content has no numeric input. Content callback reuse also
requires the exact same captured constraints/paint viewport, because a callback
may explicitly invoke a nested dimension builder whose inputs must be observed
again on resize. No new partial-filtering or resize-skip promise is made.

Keyed scope/state/render identity, producer registrations, effects and complete
source/layout/frame state adopt atomically. Same key can update data while
preserving state; reordered keys preserve instances; removed branches retire
sources, pending producers and effects. Re-entry after a committed removal gets
a new generation. Failed construction installs neither new source values nor
mount effects. Existing Fn/FnOnce side-effect discipline still applies: external
mutation performed directly by caller code cannot be rolled back.

Registration still requires a captured frame environment, including a finite
paint viewport. Layout constraints may be normally Unbounded and root/track
sizes may be automatic. This does not make LogicalUi and UiDocument dual writers
or supply a new shared component/source representation.

## Costs and evidence

No persistent record is added: Content is one discriminant in existing enums
and one scheduler dispatch arm. It avoids the **task's own** allocation proof,
tracked incoming input and builder measurement preview. The planner still
creates private current-draft previews, re-derives/sorts the DAG per task and
validates final order. Producer observation/index management, scope/order scans,
candidate copying and complete geometry checks remain. Dense or related changes
can rerun unrelated callbacks because output ownership remains coarse.

See [controls and reproduction](performance/2026-11-18-content-producers/README.md).
Controls cover natural keyed lists, same-key changed data/reorder/removal,
conditional source invalidation, final-value coalescing, dynamic descendant
tasks/producers, branch state/effect retirement and re-entry, title/body mixed
kinds, cycles and swallowed errors, plus real Noto under intrinsic Auto Grid.
Complete CPU/GPU images match independently authored full references, and old
paint/glyph frames survive owner retirement.

No wall-time/FPS/memory improvement or native fluency is inferred. Full component
initialization/ReadProp/Branch/Match/KeepAlive/macros, Store/Command, common source
integration, output ownership/fine-grained scheduling, full RenderBox/content
feedback proofs, resource budgets and independent native presentation remain
active. "99%" remains figurative: common interactions should feel fluid and
intuitive, without a success-rate or p99 acceptance requirement.
