# Wheel scrolling evidence, 2026-11-18

Worktree: `D:\oss\ggpui-worktrees\rendering-performance-research`.
Branch: `codex/rendering-performance-research`.
Base: `29e9cbb9850d516c4ef93d4c0f0de41e0fc37352`.
See [public contract and ADR](../../RETAINED_WHEEL_SCROLLING.md).
Full RFC remains active. Capability/correctness evidence only; no new timing,
memory, native input/presentation p99 or sustained FPS improvement claim.

## Controls

Seven regular framework cases cover:

- Nested two-axis containers, inner maximum30x60/outer40x80. Four events use
  latest committed offsets with the original adopted geometry, clamp and chain
  signed residuals independently by axis, preserve focus/other properties, and
  compare every CPU pixel with a separately constructed numeric forced-full
  tree including explicit clips and isolated opacity. Initial geometry remains
  immutable; later adoption updates stationary-pointer hit from leaf to tail.
- Batched events accumulate in order. Exact batch replay scrolls once; stale
  prebuilt plans reject by normal base/sequence/replay checks.
- Style-owned Scroll yields to ancestors; Hidden allows programmatic scrolling
  but consumes no wheel. Narrow Scroll operations preserve unrelated fields.
- Disabled target, changed ancestry, recovery tree, invalid position/nonfinite
  delta reject or produce unconsumed input without partial source adoption.
- Requested offset500 starts the next wheel from visible clamped extent; huge
  finite deltas clamp without overflow. A fully reached boundary keeps visual
  revision/no-frame-request even if an empty source batch is accepted.
- Primary capture is independent of wheel targeting; wheel does not change focus
  or activate. Outside scroll geometry returns host remainder. Only adoption
  moves hit geometry.
- Fractional rounding does not reverse residual direction; an invalid operation
  after planned scrolling fails the whole batch and preserves offsets/frame.

The explicit framework GPU control uses four submissions/readbacks against the
same independent numeric forced-full trees. The reference does not run the wheel
planner; group opacity and both clip chains remain in the fixture.

A GPUI integration test receives two actual ScrollEvents through normal on_scroll,
converts native displacement, then router → plan_input_scroll → ordinary UiBatch
→ complete canvas paint. It checks2events/50pxoffset/-50pxcontent origin, no focus
change and published hit geometry. This is GPUI test-platform evidence, not
native displayed interaction/p99.

Real Noto text scrolls at10/10/1000/-15px. Complete LayoutResult/baseline stays
unchanged by scrolling and matches a separate ordinary forced-full tree. Four
full CPU/GPU images compare all pixels including glyph clipping. Initial packet
survives retirement of UI/resource/font/rasterizer owners. Existing producer
real-text GPU separately checks schema4 constructors and ordinary publication.

## Implementation history

First compile used x/y on framework LayoutSize overflow, corrected to existing
width/height fields. First6core cases passed5and failed a fixture expecting only
ResyncRequired: the plan shared the last accepted sequence with a different body
and correctly returned ReplayConflict. The assertion now accepts ordinary stale
protocol rejection; runtime was not relaxed. Corrected6cases, actual GPUI wheel
and library Clippy passed. A seventh case then added fractional rounding and
final batch rollback; final arithmetic records actual f64 consumption separately
and prevents f32 rounding from reversing residual direction. The initial example
deprecated on_scroll_wheel listener was migrated to normal on_scroll.

## Verification results

Windows MSVC, paired Rust/Cargo1.98.1, locked/offline debug correctness checks.
Explicit GPU cases used NVIDIA GeForce GTX1650Ti, Vulkan, driver571.96.

| Check | Observed result |
| --- | --- |
| Complete retained suite | 561 passed, 1 ignored |
| Complete retained-text/retained-wgpu suite | 594 passed, 44 ignored |
| Complete hello retained-wgpu suite | 60 passed, 12 ignored |
| Final focused wheel core cases | 7 passed |
| Actual GPUI ScrollEvent integration | 1 passed, included in hello suite |
| Explicit nested-chain hardware control | 1 passed, 4 submissions and 4 complete-image comparisons |
| Real-Noto wheel CPU control | 1 passed, included in hello suite; final focused rerun passed |
| Explicit real-Noto wheel hardware control | 1 passed, 4 submissions and 4 complete-image comparisons |
| Existing persistent-producer real-Noto hardware control | 1 passed, 4 submissions/1 skip and 5 complete-image comparisons |
| Final framework library Clippy with -D warnings | Passed |
| Scoped rustfmt, git diff --check, 262 local documentation links | Passed |
| Five unrelated WIP file hashes | Unchanged |

Counts overlap; ignored cases establish hardware evidence only when explicitly
executed. All production changes and the seventh rounding/rollback case preceded
the complete suites. A later test-only assertion checks stationary-pointer
hit adoption; final focused seven core cases and both real-Noto wheel CPU/GPU
controls passed after that assertion. No further production changes followed.
Existing retained-only unused SpatialUpdate and dependency linker messages remain;
no lint suppressions or vendor changes were added.

## Costs and remaining acceptance

Scroll metadata scans effective nodes/builds a sparse map for each accepted
publication, including empty batches. Old scenes retain that map/resources.
Planning uses linear adopted hit/path traversal, arrays and a temporary offset
map. Style-writer detection scans scroll hosts' definitions. Existing ordinary
source/style/resource/candidate/geometry checks remain. No timing/heap/FPS win
is inferred from normal property-path reuse.

No event acknowledgement/deduplication, gesture latching, inertia, bounce,
overscroll policy, cancelable wheel handler, anchoring/focus reveal, automatic
overflow clip, scrollbar chrome, independent compositor scroll or native
presentation/p99 is added. CPU continuous complete frames, modern desktop
effects and sustained60/120Hz retain separate unpassed acceptance gates.

## Reproduction

```powershell
$env:TEMP='D:/oss/ggpui-worktrees/rendering-performance-research/target/style-ir-temp'
$env:TMP=$env:TEMP
cargo +1.98.1 test -p gpui-box --features retained --locked --offline --quiet
cargo +1.98.1 test -p gpui-box --features retained-text,retained-wgpu --locked --offline --quiet
cargo +1.98.1 test -p gpui-box-example-hello --features retained-wgpu --locked --offline --quiet
cargo +1.98.1 test -p gpui-box --features retained-text,retained-wgpu --locked --offline --quiet hardware_wheel_chain -- --ignored --nocapture
cargo +1.98.1 test -p gpui-box-example-hello --features retained-wgpu --locked --offline --quiet hardware_wheel_scroll_real -- --ignored --nocapture
cargo +1.98.1 clippy -p gpui-box --features retained-text,retained-wgpu --lib --no-deps --locked --offline -- -D warnings
```
