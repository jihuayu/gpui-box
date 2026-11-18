# Ordinary full-axis Flex allocation verification, 2026-11-18

Worktree: `D:\oss\ggpui-worktrees\rendering-performance-research`.
Branch: `codex/rendering-performance-research`.
Base: `38c9ca09a8d152bf2a116b2cf3c82cbd96950a6b`.
See [public contract](../../RETAINED_FULL_FLEX_ALLOCATION.md).
Full RFC remains active. This is framework capability work; no timing, FPS or
memory improvement is inferred from finite geometry/pixel checks. The user's
"99%" remains figurative ordinary fluid/intuitive interaction direction.

## Controls

Five regular core tests and one ignored explicit GPU wrapper cover:

- Four Flex directions × resolved/local output × three resize states. Ordinary
  grow1/2 assigns a 120×90 root as40×90 (Row) or120×30 (Column), then resizes and
  returns. A 400×300 constructed child cannot change allocation. Every CPU pixel
  matches an independent complete Flex tree with numeric fixed pane sizes. The
  explicit GPU wrapper compares24submissions/24complete images. Old frame
  output remains consumable after owner retirement; temporary observations
  retire after successful publication.
- Nested Row→Column persistent Allocation producer. Initial60×30 input,
  source10→40, same-value source skip and resize to80×40 use the ordinary once-only
  planner/source tracking. Complete output matches a numeric independent full
  tree; no containment is authored.
- Four directions × Wrap/WrapReverse × LTR/RTL,16combinations with percentage
  basis/cross size, percentage padding, physical gap and reserved Scroll gutters.
  Assigned sizes86.4×34.2 (Row) or43.2×68.4 (Column) and bases144×114 are asserted;
  complete LayoutResult and every CPU pixel match the full ordinary tree.
- Root builder preserves external soft120×90 inputs. Adding a new sibling after
  a child consumed its input invalidates ordered child observations atomically.
- Seven unsupported feedback policies reject before callback: Auto root block
  size, Auto main minimum, Content basis, wrapped Auto cross size, ratio,
  non-stretch Auto cross size and wrapped baseline group. A later sibling style
  policy change with equal numeric allocation rolls back source, structure,
  statistics and the complete frame.

Real Noto uses an ordinary split-pane Row, no containment, with3px padding and
automatic Hidden overflow clipping. Width/height160×50→220×100→180×70 and text
color changes run the same full builder. The independent full reference uses
numeric half-width panes, actual prepared text and ordinary Column layout.
Complete text LayoutResult (including baselines), wrap/clipping and all pixels
match. The explicit GPU control compares3submissions/3complete images. A failed
same-number sibling-policy batch preserves published revision/pixels; old glyph
frames survive UI/resource/font/raster owner retirement.

The existing custom-parent/containment tests remain in the scoped builder suite.
The original proof is tried first, preserving content-sized outer-ancestor and
loose independent inner-slot behavior. No vendor/Taffy/GPU layout/renderer
algorithm is modified by this stage.

## Results and limitations

Paired Rust/Cargo1.98.1, Windows MSVC, locked/offline debug correctness.
Explicit GPU adapter: NVIDIA GeForce GTX1650Ti, Vulkan, driver571.96.

| Check | Observed result |
| --- | --- |
| Final complete retained | 588 passed, 1 ignored |
| Final complete text/WGPU | 622 passed, 50 ignored |
| Final complete hello | 65 passed, 16 ignored |
| Final scoped logical::constraint_builder | 45 passed, 5 ignored |
| Five new core / real-Noto CPU | Included in the suites above |
| Explicit numeric-pane GPU | 1 passed, 24 submissions/24 complete images |
| Explicit real-Noto GPU | 1 passed, 3 submissions/3 complete images |
| Final framework library Clippy -D warnings | Passed |
| Complete retained-gpu performance harness check | Passed |
| Scoped rustfmt/diff and282local links across7documents | Passed |
| Five unrelated WIP file hashes | Unchanged |

All production and test changes, including baseline-group rejection, preceded
the final full runs and both explicit GPU controls. Ignored hardware tests count
only when explicitly run; counts from focused and complete suites overlap.
Existing retained-only SpatialUpdate unused-field/dependency linker warnings
remain; no lint suppression is introduced.

The full proof scans ancestor/sibling inputs and stores immutable styles/order;
the builder still uses private preview copies, exact final input/layout recording,
the ordinary candidate and complete geometry validation. This is not a speedup
or a general intrinsic/content-feedback solver. GPU readback equivalence does
not demonstrate native displayed smoothness, modern desktop heavy effects,
independent presentation or sustained CPU complete-frame delivery.

## Development corrections

The first core compile used tx.set's bool return with a unit render closure;
the fixture now returns explicit unit. A root ownership assertion initially
indexed scope outputs as if they contained the implicit render root; corrected
to the actual created output. An external hello test attempted the kernel-only
LocalFrame::same helper; it now checks the public revision and complete pixels.
The proof audit additionally excluded wrapped baseline groups because baseline
ascent/descent can change line size despite numeric border boxes. These changes
preserve complete geometry/image assertions and the existing layout algorithms.

## Reproduction

```powershell
$env:TEMP='D:/oss/ggpui-worktrees/rendering-performance-research/target/style-ir-temp'
$env:TMP=$env:TEMP
cargo +1.98.1 test -p gpui-box --features retained --locked --offline --quiet
cargo +1.98.1 test -p gpui-box --features retained-text,retained-wgpu --locked --offline --quiet
cargo +1.98.1 test -p gpui-box-example-hello --features retained-wgpu --locked --offline --quiet
cargo +1.98.1 test -p gpui-box --features retained-text,retained-wgpu --locked --offline --quiet logical::constraint_builder -- --nocapture
cargo +1.98.1 test -p gpui-box --features retained-text,retained-wgpu --locked --offline --quiet hardware_full_flex_two_axis -- --ignored --nocapture
cargo +1.98.1 test -p gpui-box-example-hello --features retained-wgpu --locked --offline --quiet hardware_full_flex_two_axis_real_noto -- --ignored --nocapture
cargo +1.98.1 clippy -p gpui-box --features retained-text,retained-wgpu --lib --no-deps --locked --offline -- -D warnings
cargo +1.98.1 check -p gpui-box-rendering-perf --features retained-gpu --locked --offline --quiet
```
