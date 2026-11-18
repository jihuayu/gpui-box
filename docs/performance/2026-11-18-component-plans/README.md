# Component selection plan verification,2026-11-18

Worktree: `D:\oss\ggpui-worktrees\rendering-performance-research`.
Branch: `codex/rendering-performance-research`.
Base: `2fa41ea5d99333f7301e643348f488abcc3757ba`.
See [contract and costs](../../RETAINED_COMPONENT_PLANS.md).
Complete RFC remains active; no measured timing/FPS/memory/fluency benefit claim.

## Controls

Nine regular core cases and one explicit ignored GPU wrapper:

- One registered ComponentProducer selects a bool branch, match arm and keyed
  list under three stable regions. Four model states update same-arm payloads,
  switch component type, change arm key while retaining component type, clear a
  selected region, and reorder/update the list. Key1's list scope/state/node/live
  ReadProp remain. Mounts4→4→6→6, cleanup0→0→2→3; remaining effects clean once at
  owner drop. Resolved/local initial sessions ×4models compare8complete CPU/GPU
  images with independent numeric ordinary full Flex; complete root layout too.
- Heterogeneous u32/u64 payloads in separate sites drive an interval calculation.
  Individual lower10→30 with old upper20 fails; one plan to30/50 succeeds and
  evaluates the shared derived expression once. Post-commit observed width10→20.
  A new observer declared first in the plan mounts only after both later planned
  final values settle, reading30/50. No hidden intermediate source tuple.
- Explicit Context apply creates one child's content while Context producer
  registers another. Selecting the parent away retires its nested producer and
  scopes through ordinary lifetime machinery.
- Dynamic conditional source reads ignore the unselected payload. Final source
  choose=false→true→false in one transaction reuses the same instance without a
  selector call. Switching changes dependency ownership; the old payload source
  stops causing selection, the new one does.
- No-else show skips its factory when hidden. A selected fallible factory rejects
  input30 and preserves prior source10/active scope. Hide retires content; hidden
  input changes do no factory work, re-entry creates a new generation.
- Error/panic during a later mount cannot adopt already-prepared siblings. Even
  a caught application error poisons the complete outer edit; old frame remains.
- Duplicate site declarations and ancestor/descendant site writers in one plan
  reject without changing old children. No invisible intermediate mount.
- Caught selected payload factory errors/unwind poison the pure plan; applying it rejects
  all prior declarations. Inactive factories never execute.
- Selector failure rolls back its source transaction and preserves prior
  scope/frame. Invalid final opacity after a successful plan application rejects
  the outer edit, including its payload replacement.

One new compile-fail doc verifies read-only ComponentInputs cannot obtain general
properties/edit capability; previous4compile-fail docs stay covered.

One new real Noto control uses automatic typed match selection for a title plus
an independent footer. Width100→140 updates title payload without remounting;
width80 switches to ErrorLabel, mounting fresh while footer remains. Long→short→
long text and colors match numeric full Flex using actual paragraph wrap metrics;
complete title LayoutResult/baseline and all CPU/GPU pixels compare,3hardware
frames. The previous single-label and independent-region reorder paths reuse the
extended harness and remain covered. Old glyph frames survive owner retirement.

The original component paths share Prepared with plans, so complete typed
component/region/manual logical/producer/responsive suites check regression.
No solver/vendor/GPU/renderer algorithm or persistent source schema change.

## Results and limits

Rust/Cargo1.98.1, Windows MSVC, locked/offline debug correctness.

| Check | Observed result |
| --- | --- |
| Final complete retained | 652passed,1ignored |
| Final complete text/WGPU | 686passed,59ignored |
| Final complete hello | 74passed,25ignored |
| Compile-fail docs | 5passed in both core configurations |
| Nine new core / real-Noto CPU | Included in complete suites above |
| Explicit component plan GPU | 1passed,8submissions/8complete images |
| Explicit real-Noto plan GPU | 1passed,3submissions/3complete images |
| Library Clippy -D warnings | Passed |
| Complete retained-gpu harness check | Passed |
| Scoped format/diff /301local links across5docs | Passed |
| Five unrelated WIP hashes | Unchanged |

Explicit GPU: NVIDIA GeForce GTX1650Ti, Vulkan, driver571.96. Initial development
suites preceded the factory-error/unwind invalid flag; final suites/hardware
cover final production code. Final text/WGPU additionally includes the explicit
saved-payload rollback assertion; retained's same case had already covered final
production behavior. Counts overlap; only explicitly executed ignored wrappers
are hardware evidence. Expected caught factory/mount panics print diagnostics
under nocapture. Existing unused SpatialUpdate/dependency linker warnings remain.

Finite offscreen comparisons are not a native
presentation or sustained cadence qualification. ComponentPlan adds boxed
declaration and Prepared dispatch records per site plus ordinary rows/maps/sets;
one shared prop checkpoint replaces separate checkpoints, but no wall-time
improvement is inferred. Global source/derived scans, producer/DAG previews,
logical ordering, full candidates and geometry costs remain. No lint suppression.

## Reproduction

```powershell
$env:TEMP='D:/oss/ggpui-worktrees/rendering-performance-research/target/style-ir-temp'
$env:TMP=$env:TEMP
cargo +1.98.1 test -p gpui-box --features retained --locked --offline --quiet
cargo +1.98.1 test -p gpui-box --features retained-text,retained-wgpu --locked --offline --quiet
cargo +1.98.1 test -p gpui-box-example-hello --features retained-wgpu --locked --offline --quiet
cargo +1.98.1 test -p gpui-box --features retained-text,retained-wgpu --locked --offline --quiet hardware_component_plans -- --ignored --nocapture
cargo +1.98.1 test -p gpui-box-example-hello --features retained-wgpu --locked --offline --quiet hardware_component_plans_noto -- --ignored --nocapture
cargo +1.98.1 clippy -p gpui-box --features retained-text,retained-wgpu --lib --no-deps --locked --offline -- -D warnings
cargo +1.98.1 check -p gpui-box-rendering-perf --features retained-gpu --locked --offline --quiet
```
