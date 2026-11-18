# Independent component region verification,2026-11-18

Worktree: `D:\oss\ggpui-worktrees\rendering-performance-research`.
Branch: `codex/rendering-performance-research`.
Base: `f67456cb654b2d20dfb163654faed10f84b98f62`.
See [contract, costs and limitations](../../RETAINED_COMPONENT_SITES.md).
Complete RFC remains active. No timing/FPS/memory/native fluency benefit claim.

## Controls

Seven regular core controls and one explicit ignored GPU wrapper:

- Three regions share one physical Flex parent, with no region render boxes.
  First region holds a keyed list, second a selected component, third a
  multiple-output component. The first/second both use row key1 without sharing
  state. Content update/reorder touches only selected regions; then region order
  changes, selected component type changes, a region retires and re-enters.
  Key1's scope/state/node/ReadProp persist in the unchanged list. Mounts3→3→4→4→5,
  cleanup0→0→1→2→2; all remaining effects clean once on owner drop. Resolved/local
  initial sessions ×5models compare10complete CPU/GPU images with an independent
  numeric ordinary full Flex reference. Complete root layout matches too.
- ComponentContext creates direct child regions and mounts several outputs.
  A nested ordinary Content producer writes one region. Parent source removes
  the enclosing region: nested registration/scope/output retire, surviving
  sibling scope stays, and natural Flex height30→10. Old frame still consumes.
- Stale and foreign region handles, or a region removed earlier in this same
  draft, reject. Swallowing the error still poisons the outer edit; original
  live regions remain.
- Duplicate region keys reject. Staged region removal/content change followed
  by invalid final opacity preserves all old regions and the published frame.
- Same component type under selected arm keys10→20→10 mounts fresh on each
  switch. Local state10 changed to17 does not migrate to another arm. Empty
  content retains site but retires children; re-entry gets initial state10.
  A same-key untyped manual scope is replaced rather than adopted as a site.
- ComponentContext rejects a sibling/another owner's site; caught error cannot
  adopt a partial instance or change the other site's content.
- Equal region/content declarations retain identity/output and do not request
  a visual frame; the existing published LocalFrame remains identical.

One additional real Noto control uses two regions with the same row key7. Title
width100→140→80 and long→short→long text update one label while a second fixed
label retains its input. Region order reverses at140 and returns at80, with both
components mounted only once. The independent full Flex uses actual paragraph
wrap metrics for numeric root dimensions and ordinary text children. Complete
title LayoutResult/baseline and CPU/GPU pixels agree,3hardware images. Old glyph
frame survives UI/font/raster owner disposal. The pre-existing single-label
control reuses the shared harness and stays covered.

No kernel/vendor/GPU/renderer algorithm changes. Component/Site identities share
the existing optional boxed scope record; no new persistent per-scope field or
source schema change. General manual scope/producer/responsive regression suites
exercise the same reconciliation, frame publication and lifecycle boundaries.

## Results and limits

Paired Rust/Cargo1.98.1, Windows MSVC, locked/offline debug correctness. Explicit
GPU is NVIDIA GeForce GTX1650Ti, Vulkan, driver571.96.

| Check | Observed result |
| --- | --- |
| Final complete retained | 643passed,1ignored |
| Final complete text/WGPU | 677passed,58ignored |
| Final complete hello | 73passed,24ignored |
| Compile-fail docs | 4passed in both core configurations |
| Seven new core / real-Noto CPU | Included in complete suites above |
| Explicit independent regions GPU | 1passed,10submissions/10complete images |
| Explicit real-Noto regions GPU | 1passed,3submissions/3complete images |
| Library Clippy -D warnings | Passed |
| Complete retained-gpu harness check | Passed |
| Scoped format/diff /298local links across5docs | Passed |
| Five unrelated WIP hashes | Unchanged |

The initial retained suite included6of7new controls,642passed/1ignored. The final
repeat includes the added equal-declaration control. Text/WGPU, hello and hardware
cover all final production and test code. Counts overlap; ignored cases are not
hardware evidence except the two explicitly executed cases above. Existing
retained-only unused SpatialUpdate fields/dependency linker warnings remain.

Finite offscreen images establish output
equivalence for these controls, not native displayed smoothness or modern GPU
performance. Per-region scopes/maps/sets and per-content props batches add work;
global source scans, logical sorting, producer previews and full final candidate/
geometry costs remain. No lint suppressions or percentile acceptance gate.

## Reproduction

```powershell
$env:TEMP='D:/oss/ggpui-worktrees/rendering-performance-research/target/style-ir-temp'
$env:TMP=$env:TEMP
cargo +1.98.1 test -p gpui-box --features retained --locked --offline --quiet
cargo +1.98.1 test -p gpui-box --features retained-text,retained-wgpu --locked --offline --quiet
cargo +1.98.1 test -p gpui-box-example-hello --features retained-wgpu --locked --offline --quiet
cargo +1.98.1 test -p gpui-box --features retained-text,retained-wgpu --locked --offline --quiet hardware_component_sites -- --ignored --nocapture
cargo +1.98.1 test -p gpui-box-example-hello --features retained-wgpu --locked --offline --quiet hardware_component_sites_noto -- --ignored --nocapture
cargo +1.98.1 clippy -p gpui-box --features retained-text,retained-wgpu --lib --no-deps --locked --offline -- -D warnings
cargo +1.98.1 check -p gpui-box-rendering-perf --features retained-gpu --locked --offline --quiet
```
