# Scoped frontend event verification, 2026-11-18

Worktree: `D:\oss\ggpui-worktrees\rendering-performance-research`.
Branch: `codex/rendering-performance-research`.
Base: `02699c19236713a787ede3b531e31dd4d1279b11`.
See [contract/lifetime/costs](../../RETAINED_SCOPED_EVENTS.md).
Complete RFC remains active; no timing/FPS/memory/native fluency benefit claim.

## Controls

Eight regular core controls and explicit ignored GPU wrapper:

- Shared UiInputRouter produces mouse Activate against a LogicalUi input scene.
  Callback stages width/color through EventContext; post-commit log runs only
  after adoption. Three states (20/red → 30/green → 40/blue) retain the render node;
  ordinary layout width and all CPU/GPU pixels match independent numeric full Flex.
  Old frame remains consumable after owner disposal.
  This case has no persistent producer/session: dispatch itself settles complete
  geometry at the current published constraints before post-commit work runs.
- Callback error and final invalid opacity both preserve source 0/frame and do
  not execute queued after_commit work. No failed UI candidate sends business.
- Retired handler/scope ignores delayed click, including before new local
  geometry is installed. Manual reconciliation clears handlers/policies; scope
  re-entry gets a new generation.
- Equal callback handle redeclaration retains ID. Different callback without
  updating old policy rejects; replacing callback+policy reserves fresh ID and
  rejects old routed click.
- Registry/policy declaration followed by invalid final opacity leaves no adopted
  handler or interaction entry.
- Existing Tab/Enter and Space-up behavior reaches callback via shared router.
  Disable rejects delayed activation; later render reparenting rejects old route.
  Current resolved FramePacket-only geometry uses existing LocalFrame conversion.
- Failed scope-removal candidate preserves old callback/captured Rc. Handler
  from another scope cannot attach to the child. Successful transactional removal
  retires callback and releases captures/policies.
- Callback unwind after staged source write/after_commit work rolls back both,
  preserving source 0 and the same complete frame. Expected panic is diagnostic.

One compile-fail control rejects general properties/structural capability from
EventContext; existing nine source/context/provider/command controls remain.

Real Noto EventLabel resolves Store and SaveModel provider, registers its handler
and policy through ComponentContext. A routed pointer Activate captures the latest
UI snapshot's expected business revision, queues after_commit command send, then
the application commits Store; explicit UI apply changes width 100 → 140 → 80 and
long→short→long text/colors. No command is pending before dispatch; ticket observer
can be dropped without cancelling accepted business work. Full title LayoutResult/
baseline and every CPU/GPU pixel match paragraph-metric numeric full Flex for three
hardware frames. Scope/node identity persists, callbacks run for the three models,
and the old glyph frame outlives business/UI/font/raster owners.
Prior direct Store/Command/provider text
cases retain their full references in the extended harness.

## Results and limits

Rust/Cargo 1.98.1, Windows MSVC, locked/offline debug correctness. Initial targeted
case exposed missing geometry
after retirement; retired-ID precheck now ignores those callbacks before current
scene acquisition. Current FramePacket publication can use existing conversion,
while dirty/stale geometry still rejects. Initial Clippy style finding was fixed
without suppression. Final review also removed the source-only event path:
dispatch now settles the complete local frame at the published environment even
without persistent producers, before running post-commit business work. The core
pixel control deliberately has no persistent session. Final suites/hardware
results below cover this corrected source.

- No-default-features suite: 16 passed; 2 compiler doctests passed.
- Final retained suite: 691 passed, 1 ignored; 10 compiler doctests passed.
- Final text/WGPU suite: 725 passed, 63 ignored; 10 compiler doctests passed.
- Final hello suite: 78 passed, 29 ignored.
- Explicit scoped-events GPU: 1 passed, three complete image comparisons.
- Explicit scoped-events Noto GPU: 1 passed, three complete image comparisons.
- Final library Clippy with `-D warnings` and complete retained-gpu harness check
  passed, without lint suppression.
- Scoped rustfmt, diff whitespace, 308 local Markdown links and five pre-existing
  WIP file hashes passed.

The hardware controls used NVIDIA GeForce GTX 1650 Ti, Vulkan, driver 571.96.
Hardware counts are explicit executions of ignored tests and overlap the full
suites; ignored tests are not treated as executed hardware coverage.

Input scene construction and dispatch acceptance still walk the full tree/build
maps; handler lookup is linear, registry mutation copies maps and final validation
scans policies/handlers. Existing source/producer/candidate/layout work remains.
No per-node registry field, solver/GPU/source schema change. This is explicit
host routing and finite offscreen equivalence, not native displayed latency or
continuous CPU/GPU smoothness evidence. Automatic LogicalUi style/wheel/reveal,
IME/semantics and native host installation remain open.

## Reproduction

```powershell
$env:TEMP='D:/oss/ggpui-worktrees/rendering-performance-research/target/style-ir-temp'
$env:TMP=$env:TEMP
cargo +1.98.1 test -p gpui-box --no-default-features --locked --offline --quiet
cargo +1.98.1 test -p gpui-box --features retained --locked --offline --quiet
cargo +1.98.1 test -p gpui-box --features retained-text,retained-wgpu --locked --offline --quiet
cargo +1.98.1 test -p gpui-box-example-hello --features retained-wgpu --locked --offline --quiet
cargo +1.98.1 test -p gpui-box --features retained-text,retained-wgpu --locked --offline --quiet hardware_scoped_events -- --ignored --nocapture
cargo +1.98.1 test -p gpui-box-example-hello --features retained-wgpu --locked --offline --quiet hardware_scoped_events_noto -- --ignored --nocapture
cargo +1.98.1 clippy -p gpui-box --features retained-text,retained-wgpu --lib --no-deps --locked --offline -- -D warnings
cargo +1.98.1 check -p gpui-box-rendering-perf --features retained-gpu --locked --offline --quiet
```
