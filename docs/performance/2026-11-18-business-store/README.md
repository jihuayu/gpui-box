# Business Store verification,2026-11-18

Worktree: `D:\oss\ggpui-worktrees\rendering-performance-research`.
Branch: `codex/rendering-performance-research`.
Base: `2283ecee4d75dec2b02afccd143e0d6605c77be0`.
See [contract/costs](../../RETAINED_BUSINESS_STORE.md).
Full RFC remains active; no timing/FPS/memory/native fluency benefit claim.

## Controls

Five backend-independent Store cases:

- A business writer moved to a worker commits200complete title/body states.
  The delayed observer takes revision200 with consistent200/400, not field
  mixing; old state0 survives writer/read/subscription disposal.
- Equal State commit keeps revision and reports unchanged; foreign Store revision
  and stale expected revision reject, including a stale equal-value replacement.
- A consumer captures revision1, writer commits2, consumer acknowledges1;
  revision2 stays pending. Backwards acknowledgement rejects.
- Executor polling/commit wake returns ready; closing writer still permits final
  unseen snapshot, then ready=false. No polling timer or per-field queue.
- Cancel wakes/unsubscribes the observer; writer remains usable.

Four UI integration controls plus explicit GPU wrapper:

- Store tuple selector drives a mount-once Row via existing ComponentProducer /
  ComponentPlan. Each pair of business commits samples only the final whole
  snapshot;3model frames retain scope/node and match all CPU/GPU pixels against
  independent numeric full Flex. Unrelated State-only commit advances Store input
  without rerunning component selection or changing published LocalFrame.
- Two selectors from one snapshot drive interval width. Invalid30/20 business
  state commits, but repeated UI attempts preserve old10/20/frame/applied revision;
  newer valid30/50 adopts atomically and computes20. Without active producer/
  responsive session, ordinary host requests the new frame explicitly.
- Successful component scope retirement cancels input.ready even while host keeps
  StoreInput; later apply rejects stale source. Occupied attachment key rejects;
  business writer continues after UI scope retirement.
- Failed scope-removal candidate with invalid final opacity preserves the scope
  subscription. A later business revision still applies to the live input.

Compile-fail controls reject writes through ReadStore and StoreSelector.

Real Noto Store state width100→140→80, long→short→long and colors reaches one
typed Label through a tuple selector. Intermediate width25 revisions are skipped
as complete visual states; selector/component callback runs3times, scope/node
stay stable. Independent full Flex uses actual paragraph wrap metrics for root
height; complete title LayoutResult/baseline and CPU/GPU pixels compare for3
hardware frames. Old glyph packet outlives business/UI/font/raster owners.

## Results and limits

Paired Rust/Cargo1.98.1, Windows MSVC, locked/offline debug correctness.

| Check | Observed result |
| --- | --- |
| No-default-feature Store tests | 5passed,1compile-fail doc |
| Final complete retained | 661passed,1ignored |
| Final complete text/WGPU | 695passed,60ignored |
| Final complete hello | 75passed,26ignored |
| Compile-fail docs | 7passed in both core configurations |
| Nine new core / real-Noto CPU | Included in complete suites above |
| Explicit Store bridge GPU | 1passed,3complete images |
| Explicit real-Noto Store GPU | 1passed,3complete images |
| Library Clippy -D warnings | Passed |
| Complete retained-gpu harness check | Passed |
| Scoped format/diff /299local links across4docs | Passed |
| Five unrelated WIP hashes | Unchanged |

Explicit GPU: NVIDIA GeForce GTX1650Ti, Vulkan, driver571.96. Final full suites /
hardware cover final production/test code, including cancelled scope/failed
retirement and lock-external Waker clone/drop. Initial suites preceded those
additions. Counts overlap; only explicitly run ignored wrappers count as hardware
evidence. Existing unused SpatialUpdate/dependency linker warnings remain.

No vendor/solver/GPU algorithm changes.
Initial interval test used stale cached layout without the no-session host frame
request; corrected to exercise the existing request-frame protocol, with source
and pixel checks kept. No test or lint requirement is disabled.

Publication scans subscriptions, and full State PartialEq/retained Arc snapshots
can be costly. UI adoption pays existing source/derived/producer/candidate costs;
resource budget and admission are not automated. Finite offscreen comparisons do
not establish native presentation or modern GPU/CPU continuous smoothness.

## Reproduction

```powershell
$env:TEMP='D:/oss/ggpui-worktrees/rendering-performance-research/target/style-ir-temp'
$env:TMP=$env:TEMP
cargo +1.98.1 test -p gpui-box --no-default-features --locked --offline --quiet
cargo +1.98.1 test -p gpui-box --features retained --locked --offline --quiet
cargo +1.98.1 test -p gpui-box --features retained-text,retained-wgpu --locked --offline --quiet
cargo +1.98.1 test -p gpui-box-example-hello --features retained-wgpu --locked --offline --quiet
cargo +1.98.1 test -p gpui-box --features retained-text,retained-wgpu --locked --offline --quiet hardware_business_store -- --ignored --nocapture
cargo +1.98.1 test -p gpui-box-example-hello --features retained-wgpu --locked --offline --quiet hardware_business_store_noto -- --ignored --nocapture
cargo +1.98.1 clippy -p gpui-box --features retained-text,retained-wgpu --lib --no-deps --locked --offline -- -D warnings
cargo +1.98.1 check -p gpui-box-rendering-perf --features retained-gpu --locked --offline --quiet
```
