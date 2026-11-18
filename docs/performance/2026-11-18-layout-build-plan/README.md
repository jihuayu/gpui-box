# Once-only layout build plan evidence, 2026-11-18

Worktree: `D:\oss\ggpui-worktrees\rendering-performance-research`.
Branch: `codex/rendering-performance-research`.
Base: `1e77b448d6c1af6e088121b558a174f5a18f1f46`.
See [contract and ADR](../../RETAINED_LAYOUT_BUILD_PLAN.md).
The full RFC remains active. This stage adds a framework capability; it makes
no wall-time, peak-memory, native input-tail or sustained FPS improvement claim.

## Controls

Nine regular framework cases exercise:

- Nested ordinary Flex hosts registered in reverse parent/child order and with
  reverse numeric task IDs. Four edits at widths 80/140/140/80 execute the parent
  before the child exactly once each; two tasks report three planning previews
  and one dependency edge. Branch content height updates the following sibling,
  while equal keyed branches retain their render handles. Every CPU image pixel
  matches a separately constructed ordinary forced-full Flex tree.
- A parent creates a keyed child host and appends a task during its callback.
  Both callbacks borrow transaction-local data without a static lifetime;
  scoped state is retained through the normal logical owner.
- Explicit sibling prerequisites order source/style writers before a dependent
  builder reads its incoming assignment. Forward references within the current
  queued graph work; the reader observes the updated 70px allocation.
- Ancestry/explicit cycles, unknown prerequisites, duplicate hosts, dependency
  edge limits and callback errors reject without publishing source/geometry/
  stats/effects. Catching scheduling errors does not allow partial publication.
- Dynamic ancestor insertion cannot retroactively precede completed work. Late
  root-size changes still fail the existing final builder-input check. Source
  writes and output roll back together.
- Constraint, InlineConstraint, Allocation and InlineAllocation tasks all run
  in one final publication with their original Full/Inline payloads and proofs.
- Scope-owned responsive viewport rules settle before scheduled inputs at
  environment widths 100/200/100, choosing 70/130/70px allocation. Keyed effect
  delivery/cleanup follows successful adoption; failed plans run neither.
- Equal-width late reparenting cannot reverse the already executed parent/
  descendant order, even though the observed numeric widths still match.
- Failed dynamic additions, reentrant plans and use without a captured frame
  session poison the outer edit even when errors are caught.

One additional example case uses the bundled real Noto text system. At widths
80/180/120/180, an outer task creates/reuses an inner keyed host and appends its
text-building task. Incoming widths preserve 4px padding on both sides. The
complete text LayoutResult, wrap height, baseline and all CPU pixels match a
separate direct ordinary forced-full Flex tree. An earlier immutable glyph
packet remains usable after UI, font-system and rasterizer owners retire.

Two explicit GPU controls use these same scenarios and compare complete
readback images with direct full references. Both currently submit four times.
The repeated 140px state still refreshes the local snapshot because final inline
input tracking dirties layout. GPU same-frame reuse requires the exact snapshot,
so this fixture does not establish unchanged-builder/layout/GPU skipping.

## Implementation and verification history

The first all-builder-kind fixture used an indefinite block-size root. Its full
incoming allocation correctly failed `UnsafeConstraintBuilder`. Giving that
fixture a definite 160px root height made the full-axis input independent; no
proof or runtime restriction was relaxed. Earlier regular checks passed five,
then eight cases as the contract developed. A final ninth case covers caught
dynamic-add, reentrancy and missing-session errors.

The callback initially received the plan itself. It now receives an append-only
queue, so callers cannot remove/replace pending or completed tasks by taking the
executing plan. Completed ranks, rather than an unordered completed set, detect
both new retroactive dependencies and reversed already-completed ancestry.
Callback-free final order observations also detect topology changes after the
plan returns. Existing final input/proof validation remains authoritative.

Production code was unchanged during the final full verification chain. The
retained-only suite compiled before the last test addition; it passed 535 tests
with one ignored manual benchmark. The text/WGPU suite compiled the final nine
regular cases and passed 569 with 42 ignored. The example suite passed 57 with
10 ignored. Ignored hardware tests count as hardware evidence only when run
explicitly, not from these complete-suite totals.

| Check | Actual result |
| --- | --- |
| retained complete suite before ninth test-only addition | 535 passed, 1 ignored |
| final retained focused plan cases | 9 passed |
| text + WGPU complete suite including nine plan cases | 569 passed, 42 ignored |
| hello complete suite including real Noto plan case | 57 passed, 10 ignored |
| explicit nested Flex GPU rerun | 1 passed, 4 submissions, 4 complete-image comparisons |
| explicit real Noto GPU | 1 passed, 4 submissions, 4 complete-image comparisons |
| final framework library Clippy `-D warnings` | passed |
| scoped rustfmt / diff / local documentation links | passed, 257 local links checked |

Hardware controls used NVIDIA GeForce GTX 1650 Ti, Vulkan, driver 571.96 on
Windows MSVC with Rust/Cargo 1.98.1, locked/offline debug checks. Counts overlap.
These are offscreen correctness results, not displayed frames or latency data.

The first explicit GPU run passed all four pixel comparisons, then failed a new
assertion expecting three submits. Actual submits were four for the final-input
tracking reason above. The final fixture records that current cost rather than
forcing submission skips or changing the layout proof. It was rerun explicitly
after correcting the assertion; the failed initial expectation is not counted
as an acceptance pass.

No vendor algorithm/features or source schema changed. Existing dependency
linker messages and retained-only unused SpatialUpdate fields remain. Other
worktree changes are preserved. Framework library Clippy scope does not imply
linting every vendored crate or all targets. No fresh vendor/browser capture is
claimed; these are framework scheduling and complete-output controls.

## Reproduction

From the worktree in Windows PowerShell, using the established Rust/Cargo 1.98.1
MSVC environment and D-drive temporary directory:

```powershell
$env:TEMP='D:/oss/ggpui-worktrees/rendering-performance-research/target/style-ir-temp'
$env:TMP=$env:TEMP
cargo +1.98.1 test -p gpui-box --features retained --locked --offline --quiet
cargo +1.98.1 test -p gpui-box --features retained-text,retained-wgpu --locked --offline --quiet
cargo +1.98.1 test -p gpui-box-example-hello --features retained-wgpu --locked --offline --quiet
cargo +1.98.1 test -p gpui-box --features retained-text,retained-wgpu --locked --offline --quiet hardware_scheduled_nested -- --ignored --nocapture
cargo +1.98.1 test -p gpui-box-example-hello --features retained-wgpu --locked --offline --quiet hardware_scheduled_builders_real -- --ignored --nocapture
cargo +1.98.1 clippy -p gpui-box --features retained-text,retained-wgpu --lib --no-deps --locked --offline -- -D warnings
```

## Costs and remaining acceptance

One boxed callback per task, map/set storage, repeated deterministic graph checks
and current-draft previews add CPU/allocation work. Each existing builder also
retains its private layout preview and final observation. `planning_previews`
does not count those layout passes or repeated responsive settlement. Structural
task/edge limits do not bound arbitrary callback execution time or total bytes.
No new persistent tree, packet/GPU record or alternative Flex algorithm is added.

This schedules one requested transaction. Persistent producer registration,
cross-event invalidation, source read/write dependency discovery and skipping
unchanged callbacks need a separate reusable producer contract. General content/
full-axis feedback and complete RenderBox remain open. Native independent
presentation, common-interaction p99, modern desktop effects and continuous
complete CPU frames retain separate acceptance gates. These correctness tests
cannot establish the user's 99% fluency goal or close the full RFC.
