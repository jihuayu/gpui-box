# Source-driven content task verification,2026-11-18

Worktree: `D:\oss\ggpui-worktrees\rendering-performance-research`.
Branch: `codex/rendering-performance-research`.
Base: `6e78bfdd467c9bd0d768efb2d72da3172fe0d14a`.
See [contract and costs](../../RETAINED_CONTENT_PRODUCERS.md).
The complete RFC remains active. No timing/FPS/memory or native smoothness gain
is claimed; "99%" remains figurative direction, not a statistical gate.

## Controls

Eight regular core cases and one ignored explicit GPU wrapper:

- A persistent Content producer builds a naturally sized Column list inside an
  Auto-size Row with Unbounded normal axes. Models [(1,20),(2,30)]→[(2,40),(1,25)]
  →[(1,15),(3,35)] change data/order/membership. Key1 retains its scope/local
  height/node; key2 retires. Resolved/local initial sessions ×3models compare
  all CPU pixels to independently authored ordinary full Flex. Hardware wrapper
  compares6submissions/complete images. Unrelated sources reuse the accepted
  plan; old frames remain usable after owner retirement.
- Once-only Content root under intrinsic Auto Grid appends descendant Content
  task1 from parent task90. Order90→1,2tasks/1ancestry edge and natural37×19
  root verify that construction observes no layout dimensions or allocation cut.
- A Content title producer runs before a full Allocation remaining-space body,
  although the body registers first and no explicit after is supplied. Title
  height20→35 changes the body70→55 at120×90 through ordinary inferred edges.
- A failed Content branch stages source, scope, output and effect changes and
  then returns InvalidStyle. Sources, scopes, ordinary/producer stats and the
  exact old LocalFrame remain; failed mount effects do not execute.
- An explicit reverse prerequisite conflicts with ancestry and rejects the
  Content cycle before callbacks. A caught callback error still poisons the
  outer plan; both cases retain old source and complete pixels.
- Parent Content producer mounts a child Content producer with local state and
  effect. Source changes update its ordinary size. Hiding retires the pending
  child without another call, disposes source/effect once and removes its reverse
  reads. Unread hidden updates reuse the plan; re-entry creates new scope/source
  generations and a fresh local default. Old output matches a separate full tree.
- Conditional Content reads switch a→b. Unread changes and final restored values
  skip callbacks. Selected+b update uses one final draft; the old a dependency
  leaves the accepted reverse index.
- Content callback explicitly invokes root allocation_builder. A120→160 viewport
  change reruns that callback and switches the keyed content to an80px child;
  repeating the160viewport reuses it. Content marker equality cannot hide a
  nested dimension observation across changed frame inputs.

Real Noto Content producer replaces long→short→long prepared text in a Grid
with Auto columns/implicit rows and no external definite axes. Paragraph natural
size drives ordinary Grid geometry. The independent full Flex reference uses
prepared-paragraph natural metrics for its numeric outer box. Complete title
LayoutResult/baselines and every CPU pixel agree; GPU wrapper compares3complete
images. Old glyph frames remain valid after UI/text/font/raster owners retire.

Production adds Content to the two existing Rust enums and one planner dispatch,
plus exact frame-input equality for Content callback reuse;
no Taffy/vendor/GPU/layout or source-schema algorithm changes. Other kinds retain
their original proof and final observation paths. The complete suite continues
covering existing nested allocation/producer/responsive/cache regressions.

## Results and limits

Paired Rust/Cargo1.98.1, Windows MSVC, locked/offline debug correctness.
Explicit GPU: NVIDIA GeForce GTX1650Ti, Vulkan, driver571.96.

| Check | Observed result |
| --- | --- |
| Final complete retained | 614 passed, 1 ignored |
| Final complete text/WGPU | 648 passed, 54 ignored |
| Final complete hello | 69 passed,20ignored |
| Eight regular core / real-Noto CPU | Included in complete suites |
| Explicit natural keyed-list GPU | 1 passed,6submissions/6complete images |
| Explicit real-Noto GPU | 1 passed,3submissions/3complete images |
| Library Clippy -D warnings | Passed with final production correction |
| Complete retained-gpu performance harness | Passed with final production correction |
| Scoped format/diff and293local links across6documents | Passed |
| Five unrelated WIP hashes | Unchanged |

Retained/text suites were rerun after the Content resize guard and new regression.
Clippy/harness and real-Noto GPU ran with that final production correction; the
final resize regression was already present. Counts overlap; ignored cases count
only when explicitly executed. Existing retained-only SpatialUpdate unused-field
and dependency linker warnings remain without suppression.

Content avoids its own dimension proof/observation/builder preview, but ordinary
per-task planning previews, DAG work, registry/source observations, candidate
copies, logical sorting/retirement and full geometry validation remain. There is
no resource/time budget or arbitrary callback output index. Finite offscreen
controls do not prove native presentation/latency, modern desktop effects or
sustained complete CPU frames. Full component/source integration remains open.

## Development corrections

Initial test scaffolding needed an explicit Rc import and a unit-valued source
closure (ReactiveTransaction::set returns bool). The old dynamic-child frame
reference was changed to the actual transparent wrapper rather than the separate
blue list fixture. These are test construction corrections; no production
validation was relaxed.

Review identified that marker-only Content input equality could otherwise reuse
a callback which explicitly observes a nested builder across changed frame
inputs. Content callback reuse now requires the exact accepted frame environment;
the resize regression verifies actual new branch/geometry. Full final validation
is rerun for this production correction.

## Reproduction

```powershell
$env:TEMP='D:/oss/ggpui-worktrees/rendering-performance-research/target/style-ir-temp'
$env:TMP=$env:TEMP
cargo +1.98.1 test -p gpui-box --features retained --locked --offline --quiet
cargo +1.98.1 test -p gpui-box --features retained-text,retained-wgpu --locked --offline --quiet
cargo +1.98.1 test -p gpui-box-example-hello --features retained-wgpu --locked --offline --quiet
cargo +1.98.1 test -p gpui-box --features retained-text,retained-wgpu --locked --offline --quiet hardware_content_producers -- --ignored --nocapture
cargo +1.98.1 test -p gpui-box-example-hello --features retained-wgpu --locked --offline --quiet hardware_content_producer_noto -- --ignored --nocapture
cargo +1.98.1 clippy -p gpui-box --features retained-text,retained-wgpu --lib --no-deps --locked --offline -- -D warnings
cargo +1.98.1 check -p gpui-box-rendering-perf --features retained-gpu --locked --offline --quiet
```
