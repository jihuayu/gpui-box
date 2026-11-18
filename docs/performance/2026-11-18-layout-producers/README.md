# Persistent layout producer evidence, 2026-11-18

Worktree: `D:\oss\ggpui-worktrees\rendering-performance-research`.
Branch: `codex/rendering-performance-research`.
Base: `9efd087561aff56b2d1befbf95c5f93dde67495b`.
See [public contract and ADR](../../RETAINED_LAYOUT_PRODUCERS.md).
Full RFC active; no wall-time/peak/RSS/FPS improvement claim.

## Controls

Nine regular core cases cover:

- Automatic ordinary source transactions after registration without an explicit
  responsive environment. A conditional producer tracks selected/a or selected/b,
  ignores an unread source, retires obsolete conditional reads and reuses content
  after A-to-B-to-A final equality. Keyed content height and old output agree.
- A resizing parent registers a new child producer dynamically, then retires its
  keyed scope at smaller width before the pending child can execute. Re-expansion
  uses a new generation; old local output remains readable. Equal width reuses
  both callbacks; child content follows incoming Flex allocation. A final test-only
  extension also checks keyed effect setup/cleanup counts across shrink/expand.
- Cloned equal callback handles retain identity/cache. Explicit invalidation and
  callback replacement rerun; removing the last producer ends its implicit frame
  session. An independent responsive session remains separate.
- Callback failure and invalid final composed geometry roll back sources, live
  registrations/cache, stats and exact old frame. Failed callback external call
  counts are intentionally not rolled back, matching the side-effect contract.
- Explicit sibling prerequisites settle responsive writers before readers. Cycles
  reject before callback execution; a later source writer cannot publish stale
  construction after another producer consumed the prior value.
- Real background UpdateSender writes are drained into automatic construction;
  equal computed output reuses content, while direct external render edits
  conservatively invalidate and rebuild authored producer output.
- Registration without a frame session, duplicate hosts and caught registration
  errors reject outer publication rather than partially adopting metadata.
- A surviving producer's cross-scope source cannot retire. Removing the producer
  and source together succeeds; failed removal retains scope/state/frame.
- All four builder kinds receive their existing Full/Inline payloads in one
  automatic local publication, then reuse content on an equal source transaction.

The real Noto example registers one producer once, then uses ordinary automatic
resize/source operations at widths80/180/180/120/120. One color update, two actual
width changes and initial construction run4callbacks. Equal source/layout reuses
content on the other operations. For each final frame, a separate ordinary
forced-full tree compares complete LayoutResult, baseline, wrap height and all
CPU pixels. An initial glyph packet remains consumable after UI/font/rasterizer
owners retire. The explicit GPU counterpart compares all5complete images and
expects4submits plus1same-frame skip.

The prior scheduled nested-Flex GPU case separately exercises ephemeral plans:
4complete images with3submits/1skip. No producer-specific Flex solver, resource
format or GPU bypass is used. Tests cover finite framework correctness; they are
not native visible interaction or continuous presentation evidence.

## Implementation and verification history

Initial tests used feature-gated LocalFrame.same under retained-only builds;
its crate-private identity check is now available under test configuration too.
Two responsive fixture constructors initially omitted Result propagation and
were fixed before execution. Internal responsive writer validation initially
appeared as a tracked semantic source read, incorrectly rejecting its own
legitimate later settlement; it now reads directly through the internal reactive
draft. Actual user reads remain tracked and late source changes still reject.
The expanded7cases passed after that correction. Nine final regular cases pass.

Strict Clippy initially flagged a collapsible source-validation if; it was
corrected, not suppressed. Registry changes and dynamically retired pending tasks
retain completed observations; lifetime issuance is bounded by the plan task cap
even if callbacks replace pending registrations. Existing direct/ephemeral plan
errors still poison their outer edit.

| Check | Actual result |
| --- | --- |
| final focused producer cases | 9 passed |
| retained complete suite | 548 passed, 1 ignored manual benchmark |
| text + WGPU complete suite | 581 passed, 43 ignored |
| hello complete suite | 58 passed, 11 ignored |
| explicit persistent real Noto GPU | 1 passed, 4 submits / 1 skip, 5 full images |
| explicit ephemeral nested Flex GPU | 1 passed, 3 submits / 1 skip, 4 full images |
| final framework library Clippy `-D warnings` | passed |
| scoped rustfmt / diff / local documentation links | passed, 268 local links |

Hardware: NVIDIA GeForce GTX1650Ti / Vulkan / driver571.96, Rust/Cargo1.98.1
Windows MSVC locked/offline debug checks. Counts overlap; ignored hardware is
evidence only when explicitly run. Production code and final nine core cases
were fixed before complete regression and hardware runs. A later test-only
effect/cleanup extension passed all9focused cases again. No vendor/features/
schema changed, no new browser/vendor run claimed. Existing retained-only unused
SpatialUpdate fields and dependency linker messages remain. A retained-only
strict lint invocation would include that existing unused-field warning; final
strict Clippy scope is the text/WGPU framework library, as reproduced below.

## Costs and remaining gates

Each producer owns an Rc callback, host/kind/prerequisite metadata and optional
input/source-value observations. Modification clones the registry and observed
maps; observations retain previous typed Rc values. Every automatic edit scans
all registrations and the planner can rebuild/clone the registration/task maps,
source/tree previews and DAG after each task. Content-callback reuse still runs
the existing builder measurement/proof/final tracked layout and whole geometry
checks. Source comparison short-circuits at the first mismatch and callbacks do
not execute during sorting/equality. Current global render/logical invalidation
is conservative, not a fine-grained source/output reverse index.

No timing/performance gain or bounded callback runtime is inferred from callback
counts. Structural task/edge caps are not heap/time budgets. Full RenderBox,
general content feedback, arbitrary writer discovery, per-output ownership,
source reverse subscriptions, native independent presentation/common input p99,
modern desktop effects and complete CPU continuous frames remain open.

## Reproduction

```powershell
$env:TEMP='D:/oss/ggpui-worktrees/rendering-performance-research/target/style-ir-temp'
$env:TMP=$env:TEMP
cargo +1.98.1 test -p gpui-box --features retained --locked --offline --quiet
cargo +1.98.1 test -p gpui-box --features retained-text,retained-wgpu --locked --offline --quiet
cargo +1.98.1 test -p gpui-box-example-hello --features retained-wgpu --locked --offline --quiet
cargo +1.98.1 test -p gpui-box-example-hello --features retained-wgpu --locked --offline --quiet hardware_persistent_producer_real -- --ignored --nocapture
cargo +1.98.1 test -p gpui-box --features retained-text,retained-wgpu --locked --offline --quiet hardware_scheduled_nested -- --ignored --nocapture
cargo +1.98.1 clippy -p gpui-box --features retained-text,retained-wgpu --lib --no-deps --locked --offline -- -D warnings
```
