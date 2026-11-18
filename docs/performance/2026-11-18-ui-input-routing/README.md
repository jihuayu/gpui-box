# Adopted-frame input routing evidence,2026-11-18

Worktree:`D:\oss\ggpui-worktrees\rendering-performance-research`.
Branch:`codex/rendering-performance-research`.
Base:`10ef2fc69971cf15f606eec7f02c96ab7a078f00`.
See [contract and ADR](../../RETAINED_UI_INPUT_ROUTING.md).
Full RFC remains active. No wall-time, memory, FPS or native input-tail claim.

## Controls

Ten regular framework cases exercise:

- Exact nested hit target, ancestor enter/leave and pointer bubbling; primary
  capture keeps target on movement, owner pressed state clears on drag-out and
  restores on return, release outside does not activate, release inside does.
- Tab/Shift-Tab declaration order remains independent of Flex order, wraps and
  skips disabled/hidden nodes. Keyboard focus-visible, Enter down/Space up,
  repeat suppression and pending-key retirement on focus/window blur are checked.
- An already adopted input scene keeps old visible geometry after a newer
  document translation publishes. Only explicit adoption moves the hit geometry.
  Old eligible dispatch is valid after motion, stale scene adoption rejects and
  a scene/router remains usable after document/resource owner drop.
- Disabled ancestors cancel pointer/focus/key activation and block clicks through
  a front overlay. Pass-through skips its own box while preserving descendants;
  nested controls activate once and press/release across different owners does
  not activate either. Recovery cancels old focus/capture even with surviving
  external IDs and emits feedback clears for retained authored press state.
- Handler replacement, subtree removal, greater-generation slot reuse, recovery
  and reparented complete ancestry invalidate delayed events. Runtime/epoch/tree
  identities are checked; switching epoch requires explicit router reset.
- Clip and scroll use the same immutable geometry as LocalFrame and resolved
  full frame; old snapshots preserve their earlier hit regions. A real worker
  consumes an input scene after owner retirement. Input scene/effects/router
  compile-check Send+Sync without transferring UiDocument or raw Taffy state.
- Invalid handler0 even when overwritten later, invalid host viewport, NaN
  pointer positions and ineligible focus fail without partial document/router
  state. Old schemas1/2 explicitly reject.
- Feedback goes through ordinary UiBatch/StyleState, preserves selected and
  uses the normal style route. Five event transitions compare all CPU pixels
  against independently authored numeric-color full trees. Old final frames
  remain consumable after all owners drop.

The explicit GPU case runs those five feedback states using3local submissions
and2same-frame skips, comparing every image pixel with direct full reference
output at all five steps. Existing
actual Noto style GPU control separately verifies schema-migrated ordinary text
width/wrap/baseline/opacity/resources through five submissions.

A new GPUI integration view receives actual MouseMove/MouseDown/MouseUp events
from TestAppContext.simulate_click, forwards them to public UiInputRouter,
validates handler IDs, applies feedback as ordinary source batches and paints
the complete result through normal canvas. It checks one activation, down/up,
focus, final hovered fill and100px geometry. This is a GPUI test platform, not
native visible display or sustained pointer latency evidence.

## Build history and scope

Initial test compile used a nonexistent LayoutEdges::auto; replaced with the
existing all(Auto). Initial GPUI test used a nonexistent PaintCommand name then
lacked a debug selector; both were corrected before actual mouse dispatch.
A disabled-overlay assertion initially expected no lifecycle Blur event, though
clicking empty/disabled space conventionally clears focus; it now distinguishes
cleanup from pointer delivery. A nested-control recovery fixture expected Blur
without registering a focus handler; the fixture now registers the callback.
These failures are not final acceptance results.

The first explicit GPU fixture unconditionally unwrapped a submission on all
five events. The renderer correctly skips the two unchanged visual revisions;
the final test checks exact3submits/2skips and every resulting full image. The
runtime was not changed to force unnecessary GPU work.

UiInteraction enlarged UiOperation's already large inline Create variant and
strict Clippy identified the size difference. Schema3 boxes Create/complete
Layout payloads rather than broadly suppressing the lint. Existing snapshot,
planning and real-text callers are migrated. This trades create/layout heap
allocations for smaller ordinary-operation records; no memory/performance gain
is claimed without measurement.

No vendor kernel code/features changed, so no new vendor suite/browser capture
is claimed. Existing layout/browser fixtures remain in ordinary framework tests.
The normal example WIP is preserved; the new event view is a separate test file.
Existing dependency linker messages/retained-only unused SpatialUpdate fields
remain. Final counts and follow-up scope are recorded below.

## Actual verification

| Check | Actual result |
| --- | --- |
| retained complete suite |527passed,1ignored manual benchmark |
| text+WGPU complete suite |560passed,41ignored |
| hello complete suite including actual mouse dispatch |56passed,9ignored |
| final focused input cases |10passed,1ignored hardware |
| explicit input feedback GPU |1passed,3submits/2skips,5complete-pixel comparisons |
| explicit actual Noto style GPU |1passed,5complete-pixel submissions |
| final framework library Clippy `-D warnings` |passed |
| scoped rustfmt/diff and document links |passed |

The complete suites include the schema/payload migration and new input cases.
After them, recovery feedback clearing and nested-owner pressed behavior were
tightened; the final focused10cases and explicit input GPU rerun cover those
changes. A final slot-reuse/recovery-clear extension is part of that focused
run. Hardware is GTX1650Ti/Vulkan/driver571.96, Rust/Cargo1.98.1 Windows MSVC,
locked/offline debug checks. Counts overlap; ignored cases are hardware evidence
only when explicitly executed. Finite correctness is not display/p99 evidence.

## Reproduction

```powershell
$env:TEMP='D:/oss/ggpui-worktrees/rendering-performance-research/target/style-ir-temp'
$env:TMP=$env:TEMP
cargo +1.98.1 test -p gpui-box --features retained --locked --offline --quiet
cargo +1.98.1 test -p gpui-box --features retained-text,retained-wgpu --locked --offline --quiet
cargo +1.98.1 test -p gpui-box-example-hello --features retained-wgpu --locked --offline --quiet
cargo +1.98.1 test -p gpui-box --features retained-text,retained-wgpu --locked --offline --quiet hardware_input_feedback -- --ignored --nocapture
cargo +1.98.1 test -p gpui-box-example-hello --features retained-wgpu --locked --offline --quiet hardware_style_ir_real -- --ignored --nocapture
cargo +1.98.1 clippy -p gpui-box --features retained-text,retained-wgpu --lib --no-deps --locked --offline -- -D warnings
```

## Remaining gates

Routing uses linear local hit geometry with temporary contexts, path and output
arrays. Input policy graphs add O(N) memory/rebuild/detection work and retained
scenes prolong resource/mapping lifetime. Feedback still uses normal UI owner
candidate publication. This is neither an independent compositor interaction
overlay nor an implementation of native capture/focus/gesture/IME/accessibility.
Full RenderBox, automatic builder/query scheduling, modern desktop heavy effects,
CPU continuous complete-frame smoothness, native independent presentation,
sustained60/120Hz and representative interaction p99 remain active requirements.
