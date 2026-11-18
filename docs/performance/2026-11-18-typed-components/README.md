# Typed component verification,2026-11-18

Worktree: `D:\oss\ggpui-worktrees\rendering-performance-research`.
Branch: `codex/rendering-performance-research`.
Base: `89e30eda40eb61e77e7593f335dc95e96ec83148`.
See [component contract/costs](../../RETAINED_TYPED_COMPONENTS.md).
Full RFC remains active. No wall-time/FPS/memory/native fluency improvement claim.

## Controls

Seven regular core cases plus one explicit ignored GPU wrapper:

- Content parent declares keyed typed RowComponents. Mount-created state,
  computed width/color, bindings and post-commit effect stay stable across
  payload changes/reorder. Membership changes key2→key3; mount counts2→2→3,
  cleanup0→0→1 and key1's source/node/scope persist. Old event-style ReadProp
  handle reads latest width. Resolved/local session ×3models compares6complete
  CPU/GPU images with independent numeric full Flex. Old packets survive owner
  drop; remaining effects clean once at disposal.
- Empty component emits no render nodes. Equal declaration does not remount.
  Same key with a different Rust component TypeId (same Props type) remounts;
  changing host also remounts. Removal cleans and later re-entry mounts fresh.
- Nested component mounts a Content producer that declares a child with two
  sibling outputs. Source color update reuses both nested scope/output handles;
  final ordinary Flex size20×15 and both colors match expectations.
- Mount error and caught mount panic poison the outer edit. Tentative source,
  appended output/scopes/stats do not adopt; complete previous pixels remain.
- Duplicate keys reject before effects/mount adoption. A valid initial instance
  survives a staged type replacement followed by invalid final opacity; new
  mount effects and old cleanup do not run on the failed candidate.
- Replacing a nonempty nested type retires its descendant producer/scope/output
  before final publication. Old frame still resolves retired output geometry;
  new frame does not. New multiple-output type retains ordinary final size.
- ComponentContext mounts nested typed children directly, rejects reserved
  source key, and wrong runtime/scope, foreign host and removed host are rejected
  before component code runs. Old root instance survives those failed candidates.

One new compile-fail doc verifies ComponentContext cannot escape to a thread.
Existing3ReadProp/PropBatch compiler capability controls remain.

Real Noto typed label mounts once, with computed LayoutStyle/text binding from
one aggregate ReadProp. Width100→140→80 and long→short→long paragraph/3colors
retain its scope/node. Independent numeric full Flex uses actual paragraph wrap
metrics; complete title LayoutResult/baseline and all CPU/GPU pixels agree,
3hardware images. Old glyph output survives UI/font/raster owner retirement.

No vendor/kernel/GPU/renderer algorithm change; source schema remains6. Manual
scope reconciliation shares the extended helper with no typed identity request;
complete lifecycle/producer/responsive suites cover that original entry.

## Results and limits

Paired Rust/Cargo1.98.1, Windows MSVC, locked/offline debug correctness.
Explicit GPU: NVIDIA GeForce GTX1650Ti, Vulkan, driver571.96.

| Check | Observed result |
| --- | --- |
| Final complete retained | 636passed,1ignored |
| Final complete text/WGPU | 670passed,57ignored |
| Final complete hello | 72passed,23ignored |
| Compile-fail docs | 4passed in both core configurations |
| Seven new core / real-Noto CPU | Included in suites above |
| Explicit keyed component GPU | 1passed,6submissions/6complete images |
| Explicit real-Noto GPU | 1passed,3submissions/3complete images |
| Final library Clippy -D warnings | Passed |
| Complete retained-gpu performance harness | Passed |
| Scoped format/diff and294local links across5documents | Passed |
| Five unrelated WIP hashes | Unchanged |

All production/test behavior was fixed before final full suites and hardware
controls. Counts overlap; ignored cases count as hardware evidence only when
explicitly executed. Existing retained-only SpatialUpdate unused-field and
dependency linker warnings remain without suppression. Shared manual logical
scope lifecycle, responsive and producer regressions pass in complete suites.

Scope gains one optional boxed identity; typed instances allocate that record.
Collected rows/keys/identity sets and shared reconciliation/props temporary maps
add work. Complete derived scans, producer previews/DAGs, logical sorting,
candidate copies and full geometry checks remain. Mount callbacks can be costly
and have no execution budget. Finite offscreen references do not establish
modern desktop effects, input-to-visible latency or continuous complete CPU frames.
"99%" stays figurative direction rather than a success-rate/p99 requirement.

## Development notes

The initial5core cases and real-Noto CPU passed before adding nonempty type
replacement/descendant retirement and direct nested-context/invalid-host controls.
Expected caught mount panic prints diagnostics under nocapture and passes.
No lint or validation restrictions are suppressed.

## Reproduction

```powershell
$env:TEMP='D:/oss/ggpui-worktrees/rendering-performance-research/target/style-ir-temp'
$env:TMP=$env:TEMP
cargo +1.98.1 test -p gpui-box --features retained --locked --offline --quiet
cargo +1.98.1 test -p gpui-box --features retained-text,retained-wgpu --locked --offline --quiet
cargo +1.98.1 test -p gpui-box-example-hello --features retained-wgpu --locked --offline --quiet
cargo +1.98.1 test -p gpui-box --features retained-text,retained-wgpu --locked --offline --quiet hardware_typed_components -- --ignored --nocapture
cargo +1.98.1 test -p gpui-box-example-hello --features retained-wgpu --locked --offline --quiet hardware_typed_components_noto -- --ignored --nocapture
cargo +1.98.1 clippy -p gpui-box --features retained-text,retained-wgpu --lib --no-deps --locked --offline -- -D warnings
cargo +1.98.1 check -p gpui-box-rendering-perf --features retained-gpu --locked --offline --quiet
```
