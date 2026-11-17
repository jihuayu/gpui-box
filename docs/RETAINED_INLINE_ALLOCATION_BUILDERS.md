# Ordinary content-sized inline allocation builders

Status: experimental framework contract implemented on 2026-11-17. The complete
RFC goal remains active. Extends [inline constraint builders](RETAINED_INLINE_CONSTRAINT_BUILDERS.md)
and [independent parent allocation](RETAINED_PARENT_ALLOCATION_BUILDERS.md).
This is a structural layout capability; no timing or sustained-frame gain is claimed.

Follow-up: [content-independent Row allocation](RETAINED_ROW_INLINE_ALLOCATION.md)
extends the shared proof to Row/RowReverse with all wrap modes when all siblings'
numeric bases/limits exclude content feedback. The original Column-only scope
below is historical; arbitrary content-dependent Row/ratio/intrinsic cases remain
unproven. Final observations now include Row ordered children/effective item styles.

## Public contract

`LogicalEdit::inline_allocation_builder(host, callback)` reads the ordinary
host's **incoming border-box inline assignment** before constructing its keyed
descendants. The host can be an ordinary Flex or ConstraintBox container with
Normal containment. Its height can follow the constructed content and update
ancestors and following siblings. Applications no longer need to insert an
InlineSize/Size containment boundary for this proven class of ordinary Column
layouts.

Use it inside `LogicalUi::edit_frame` or `edit_local_frame`, after prerequisite
sources, ancestor styles, child ordering and parent algorithms have been staged.
Nested builders see the current draft. Automatic responsive sessions settle
before reading, and the complete final transaction validates before sources,
keyed lifecycle, geometry, frame and effects publish. Callback side effects
outside draft edits, panic and shared memo warming retain the existing boundaries.

```rust,ignore
edit.inline_allocation_builder(host, |input, edit| {
    // Incoming host border-box width, before its own padding is removed.
    let width = input.known_width.ok_or(Error::UnsafeConstraintBuilder)?;
    let branch = edit.reconcile_keys(scope, &[u64::from(width >= 600.0)])?[0];
    if edit.renders(branch)?.is_empty() {
        build_branch(edit, branch, host, width >= 600.0)?;
    }
    Ok(())
})?;
```

The shared `InlineConstraintBuilderInput` gains `sizing_mode`, `known_width`
and `known_width_is_definite`. Complete literals/exhaustive patterns require
migration. The old outgoing `inline_constraint_builder` also fills these fields.
There is no stable cross-language ABI commitment.

| Field | Meaning |
| --- | --- |
| sizing_mode | Inherent/Content treatment of the host's own style, not intrinsic query demand |
| known_width | Parent-selected host border-box width, if assigned |
| known_width_is_definite | Whether that selected width establishes percentage definiteness |
| min / max | Independent hard border-box bounds; max may be infinity |
| available | Soft inline opportunity, preserving Definite/Unbounded/MinContent/MaxContent |
| percentage_basis | Independently established parent inline percentage basis |

Known width is not necessarily tight bounds, and neither soft space nor the
parent basis is the host's used content width. Ordinary Flex's final item call
uses Content mode after resolving item styles; callers must not assume Inherent.
The input exposes no height, block opportunity, final content extent or child
measurement. Select only from independent inline inputs.

## Dependency proof and atomic validation

The shared proof traverses the complete chain to the external root. Ordinary
Flex ancestors must be Column/ColumnReverse and NoWrap; each child width must
be authored numeric/percentage/stretch or auto stretched without auto inline
margins. Tight-width ConstraintBox edges qualify. Enforced independent
`ParentInlineLayout` phases qualify after proving their parent's own width.
Parent algorithm identity and layout-ordered child lists remain observations.

Aspect coupling, out-of-flow ancestry, intrinsic width limits, ordinary Row/wrap
ancestors, Loose box edges and unproven nonstretched shrink-wrap reject. The root
also needs an actual external anchor: numeric authored width, finite normal
inline opportunity or a hard-tight inline interval. Percentage/stretch roots in
Unbounded/MinContent/MaxContent with no numeric/tight anchor reject **before the
callback**. This prevents a syntactically percentage width from disguising
content-dependent root sizing. The rule is conservative; it does not claim all
safe cases or arbitrary feedback layouts.

The previous exact fixed-width shortcut is deliberately disabled for this new
proof. Fixing a host's used content width under Row/wrap/aspect does not prove
that its incoming soft opportunity or parent percentage basis is independent of
constructed content. Existing used-content query and outgoing contained-builder
proofs retain their prior behavior.

After construction, final normal layout must reproduce every projected input
and still satisfy the complete chain proof. Custom algorithm/order observations
must also match even when a replacement returns the same width. Failure rolls
back source values, bindings, keyed state/effects, tree/geometry, stats and the
published frame. There is no callback replay or iterative convergence solver.

## Kernel integration and lifetime

Project-owned Taffy now supports opt-in `set_track_layout_input`,
`tracked_layout_input` and `clear_tracked_layout_inputs`. Normal PerformLayout
records the actual incoming input after the shared algorithm/cache path. Size
queries and scratch complete-baseline queries do not overwrite it. Hidden
subtrees retire records while retaining the opt-in capability; node deletion,
generation reuse, tree clear and clone isolation follow ownership.

Tracking selects existing complete input/output cache identity through the
whole invocation, including ancestors that could otherwise hide changed soft
space. Opt-in invalidates the host and ancestors so a warm frame cannot omit
the first observation. Final validation also marks frame production dirty:
layout invalidation alone cannot populate a newly opted-in record if a cached
FramePacket bypasses layout.

Framework observations are transaction-scoped. A successful final check clears
the tracking capability and records before installing the live candidate.
Failed private candidates cannot leak them. Exact cache entries remain valid
until ordinary invalidation; tracking does not clear useful caches. A following
ordinary unchanged frame performs zero layout. Repeating the builder itself
still incurs observation work and is not claimed as a P0 operation.

## ADR and costs

Decision: expose a separate incoming inline builder backed by actual kernel
inputs and the shared dependency proof. Keep the existing outgoing contained
builder and full-axis independent allocation builder contracts distinct.

Requiring containment for every ordinary responsive branch changes intrinsic
contributions and adds artificial wrapper nodes. Using final content width as
incoming constraints merges different domains. A local fixed-width shortcut
cannot prove soft input independence. Running frontend construction inside
Taffy measurement entangles repeated intrinsic probes with keyed lifetimes.
Those alternatives were not adopted.

Costs: additional temporary inline-input fields, two lazy side-table maps in
TaffyTree, observation records, opt-in ancestor invalidation, exact invocation
caches and a final observation layout pass. Side-table headers/capacity and
exact cache entries can outlive cleared records; no measured memory reduction
is asserted. Ancestor proof scans, custom snapshot/order copies, complete atomic
candidates and per-builder private previews remain. Repeated same-input builder
transactions can still do layout; FramePacket layout counters exclude preview
work. Existing style/NodeData/GPU/FramePacket formats are unchanged. Wall time,
peak allocation, cache budgets and tail latency were not profiled.

## Verification and open scope

See [evidence and reproduction](performance/2026-11-17-inline-allocation-builders/README.md).
Tests compare full/local CPU output, actual shaped Noto glyphs and explicit GPU
readback to complete ordinary Flex references. These are same-kernel protocol
and output controls, not independent browser conformance.

General Row/wrap/aspect dependency proofs, full-axis ordinary content-feedback
builders without a cut, automatic cross-builder/query scheduling, complete
RenderBox, incremental candidates/proofs/rollback and global budgets remain open.
Broad intrinsic/percentage/cycle/Flex/Grid/Block/writing-mode consistency and
primitive/effect/native lifecycle also retain separate gates. Modern desktop
heavy-effects GPU, CPU sustained complete-frame delivery, sustained60/120Hz and
display-visible input tails are not established by these finite tests.

[Ordinary content-width queries](RETAINED_ORDINARY_CONTAINER_QUERIES.md) now reuse
this complete-chain proof and shared external-inline-anchor guard in existing
ContainerInline rules. They read used content width, while this builder keeps
its incoming border-box input. Both compose in atomic scoped responsive sessions.
