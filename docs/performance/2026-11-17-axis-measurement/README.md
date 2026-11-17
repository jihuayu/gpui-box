# Axis measurement evidence, 2026-11-17

Worktree: `D:\oss\ggpui-worktrees\rendering-performance-research`.
Branch: `codex/rendering-performance-research`.
See [contract and ADR](../../RETAINED_AXIS_MEASUREMENT.md).
The complete RFC goal remains active. No wall-time speedup, memory reduction,
p99 or sustained visible-frame claim follows from callback counts alone.

## Finite verification controls

- Three public width probes (MinContent/MaxContent/Definite44) on the owned
  policy with padding2 produce24/104/44 border-box widths with exactly three
  width metric calls, zero height or complete metric calls and zero full plans.
  A subsequent vertical query preserves selected-width-dependent40px content
  height; an84px known outer width yields20px content height. Complete-size and
  baseline queries still return complete44x44 outer dimensions and run a full
  plan where required. The pure queries publish no frame.
- A client parent calls child measure_axis for both axes under44px space. Full
  independent allocation overrides a conflicting MaxContent request but keeps
  each scalar demand. The child receives horizontal/vertical requests and one
  final complete plan, produces44x40, and complete CPU output matches a new
  numeric full Flex control. A separate inline phase overrides a999px request
  with a44px tight child width while retaining both scalar requests.
- Normal typed parent snapshots change44→84→44, measured and independent full
  policies, resolved/local frames. Child geometry44x40→84x20→44x40 and all CPU
  pixels match separately authored numeric Flex controls. LogicalUi's public
  scalar wrappers equal actual corresponding complete extents. Restored final
  source identity in one transaction keeps revision and0 layout; old frames
  remain usable after owner destruction.
- Ordinary Flex has96 input combinations: four directions, three wrap modes,
  four soft/intrinsic spaces and two sizing modes, with numeric gaps/padding
  and three differently sized nonshrinking children. Both scalar extents equal
  corresponding full-query extents (192 scalar comparisons). This is same-
  kernel consistency, not an independent browser-conformance count.
- Complete-plan-only clients remain valid scalar-query fallbacks and see Both
  demand. Invalid metrics and invalid percentage bases reject without replacing
  prior published frames. Invalid child indices reject. Complete baseline
  requests and final placements never expose partial demand.
- Standalone vendor exact-cache test stores width80 with unused-height0, then
  full80x40. Repeated width requests hit without another callback and cannot
  reuse the full key incorrectly. A baseline query with an incoming horizontal
  axis still computes complete80x40, proving demand normalization.
- Actual bundled Noto paragraphs use60/140/60px opportunities beneath a pure
  scalar-measuring parent. Width/height scalar queries match complete baselines
  and glyph layout, MinContent/MaxContent widths match full queries, and all CPU
  glyph pixels match a new numeric-width Flex tree. Resource/UI owner release
  preserves the old frame. Initial test compilation caught a moved local frame
  in fixture bookkeeping; the corrected test saves computed pixels before move.
- The explicit WGPU case submits three widths through each measured/independent
  parent and compares all pixels to numeric full controls. Explicit execution on
  NVIDIA GeForce GTX1650Ti / Vulkan / driver571.96 passed all six full-pixel
  submit/readback comparisons. This is finite offscreen correctness. Ignored
  hardware tests alone are not passing evidence.

## Final checks and commands

| Check | Actual result |
| --- | --- |
| retained full suite |426 passed |
| retained-text + retained-wgpu full suite |459 passed,27 ignored |
| explicit scalar parent WGPU case |1 passed,6 complete-pixel submissions |
| hello retained-wgpu, including real Noto glyphs |44 passed,2 ignored |
| vendor default / project-config library suite |225 /219 passed |
| framework library Clippy `-D warnings` |passed |
| vendor no-default / alloc+tree+Flex / all-features check |passed |
| scoped framework/glyph/vendor-test rustfmt and git diff check |passed |

The retained suite first reported425 before the final inline-slot test; the
final426 run includes all new tests. The suites overlap and should not be
aggregated as independent coverage. The standalone vendor suites used the
existing temporary low-memory profile below, without changing production build
settings. Retained-only original unused SpatialUpdate fields and dependency
linker messages remain. The Clippy claim covers the framework library only.

```powershell
cargo +1.98.1 test -p gpui-box --features retained --locked --offline --quiet
cargo +1.98.1 test -p gpui-box --features retained-text,retained-wgpu --locked --offline --quiet
cargo +1.98.1 test -p gpui-box --features retained-text,retained-wgpu --locked --offline --quiet hardware_scalar_parent -- --ignored --nocapture
cargo +1.98.1 test -p gpui-box-example-hello --features retained-wgpu --locked --offline --quiet
cargo +1.98.1 clippy -p gpui-box --features retained-text,retained-wgpu --lib --no-deps --locked --offline -- -D warnings

$env:CARGO_PROFILE_TEST_DEBUG='0'
$env:CARGO_PROFILE_TEST_INCREMENTAL='false'
$env:CARGO_PROFILE_TEST_CODEGEN_UNITS='256'
cargo +1.98.1 test --manifest-path vendor/taffy/Cargo.toml --lib --locked --offline --quiet
cargo +1.98.1 test --manifest-path vendor/taffy/Cargo.toml --lib --no-default-features --features std,taffy_tree,flexbox,grid,block_layout,float_layout,content_size --locked --offline --quiet
cargo +1.98.1 check --manifest-path vendor/taffy/Cargo.toml --no-default-features --locked --offline --quiet
cargo +1.98.1 check --manifest-path vendor/taffy/Cargo.toml --no-default-features --features alloc,taffy_tree,flexbox --locked --offline --quiet
cargo +1.98.1 check --manifest-path vendor/taffy/Cargo.toml --all-features --locked --offline --quiet
```

## Costs and limits

Parent callback inputs gain requested demand; full and inline allocation inputs,
ChildLayoutInput, persistent cache/style/node/GPU/packet formats do not grow.
The scalar API returns one finite nonnegative extent; custom unrelated unknown
axes may be finite placeholders only inside the kernel's distinct query key.
Width can skip dependent-height and plan work; height may require initial
complete metrics. Legacy plans still cost allocations. Private public query
clones, exact cache maps and normal ancestor work remain. Target struct padding,
peak memory, wall-time and tail latency have not been measured.

Full RenderBox, arbitrary feedback builders, generic builder/query scheduling,
broad intrinsic/ratio/percentage conformance and native/effect lifecycle remain
open, as do modern heavy-effect GPU, CPU smooth complete frames, sustained60/120Hz
and visible input tails.
