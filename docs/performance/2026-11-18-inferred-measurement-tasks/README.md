# Inferred measurement task verification, 2026-11-18

Worktree: `D:\oss\ggpui-worktrees\rendering-performance-research`.
Branch: `codex/rendering-performance-research`.
Base: `47d9c56d3a0b7c8c52e8cb111cad11bc6f2e8853`.
See [public contract](../../RETAINED_INFERRED_MEASUREMENT_TASKS.md).
Full RFC remains active. This is framework scheduling capability, not a timing,
memory, FPS or native smoothness claim. "99%" stays figurative user direction.

## Controls

Five new regular core cases plus one ignored explicit GPU wrapper cover:

- A full Allocation body task1 is registered before a task90 below a measured
  sibling's root, with no after. It executes90→1, consumes actual remaining
  height after title measurement, and matches independent numeric full Flex.
  Resolved/local output × three size/measurement states yields6complete CPU/GPU
  comparisons in the explicit hardware wrapper. Old packets survive owner drop.
- Persistent body producer is registered first, before its title producer, with
  no explicit dependency. Initial and changed-source transactions execute
  title→body; same-value source writes reuse the plan/callbacks. Geometry remains
  the normal remaining-space allocation.
- A title task adds a descendant task99 dynamically. The pending body waits for
  it, executing90→99→1. Explicit+inferred body←title edge deduplicates; added
  body←inner measurement and inner←title ancestry total3edges.
- An explicit reverse edge conflicts with the inferred measurement edge and
  rejects the complete queued cycle before any callback/source adoption. A zero
  edge limit also rejects before callbacks; inferred edges count toward limits.
- A body callback adds a new title predecessor only after consuming its input.
  The next graph rejects retroactive order and preserves complete old pixels.

The existing measured-sibling once-only test is updated from expecting a late
measurement failure to successful automatic title-first scheduling and body60px
height. It deliberately gives body ID1 and title ID2 without after. Direct
imperative late sibling mutation tests remain, preserving final validation.

Real Noto uses task1 body before task90 title registration, no after, with actual
long→short→long title shaping/wrapping and100×210→180×180→140×210 root sizes.
Order90→1 and one deduplicated graph edge are asserted. The numeric full reference
derives title height from the shaped paragraph and uses fixed header/body boxes;
complete title LayoutResult/baselines and every CPU pixel agree. The explicit
GPU control compares3submissions/3complete images. Old glyph frames remain usable
after UI/text/font/raster owners retire.

Scheduling and final order validation share one edge-derivation helper. It reads
current frame constraints and pure proof records without invoking layout or user
callbacks. Existing independent-parent/containment paths keep precedence. No
Taffy/vendor/GPU algorithm, public data shape or source schema changes.

## Results and limits

Paired Rust/Cargo1.98.1, Windows MSVC, locked/offline debug correctness. Explicit
GPU: NVIDIA GeForce GTX1650Ti, Vulkan, driver571.96.

| Check | Observed result |
| --- | --- |
| Final complete retained | 599 passed, 1 ignored |
| Final complete text/WGPU | 633 passed, 52 ignored |
| Final complete hello | 67 passed, 18 ignored |
| Final logical filter | 112 passed, 10 ignored |
| Five new regular core cases / real-Noto CPU | Included in suites above |
| Explicit numeric full-Flex GPU | 1 passed, 6 submissions/6 complete images |
| Explicit real-Noto GPU | 1 passed, 3 submissions/3 complete images |
| Final library Clippy -D warnings | Passed |
| Complete retained-gpu performance harness check | Passed |
| Scoped rustfmt/diff and285local links across7documents | Passed |
| Five unrelated WIP file hashes | Unchanged |

All production and test behavior was fixed before these final full runs and
both explicit GPU controls. Counts overlap; ignored hardware
tests count only when explicitly run. Existing retained-only SpatialUpdate
unused-field and dependency linker warnings remain without suppression.

Graph derivation repeats full proof/sibling scans, creates task-host/ID maps and
sets, and runs after every task plus final validation. Ordinary private previews,
builder layout/input recording and full candidate/geometry checks remain. Hosts
inside a measured subtree gain conservative edges even if their callbacks are
paint-only. Nonlocal callback writes outside their hosts remain explicit after
dependencies; no arbitrary closure output discovery is claimed.

Finite CPU/GPU reference agreement does not establish native displayed latency,
modern desktop effects cadence, independent presentation or continuous CPU output.

## Development corrections

The first producer test combined edit.properties()'s mutable borrow and edit.read
in one expression. It now reads the source into a local before staging measure;
production ownership and callback behavior are unchanged. Existing lifecycle
panic-isolation tests emit expected caught-panic diagnostics with nocapture and
still pass; these are not failed regressions.

## Reproduction

```powershell
$env:TEMP='D:/oss/ggpui-worktrees/rendering-performance-research/target/style-ir-temp'
$env:TMP=$env:TEMP
cargo +1.98.1 test -p gpui-box --features retained --locked --offline --quiet
cargo +1.98.1 test -p gpui-box --features retained-text,retained-wgpu --locked --offline --quiet
cargo +1.98.1 test -p gpui-box-example-hello --features retained-wgpu --locked --offline --quiet
cargo +1.98.1 test -p gpui-box --features retained-text,retained-wgpu --locked --offline --quiet retained::logical -- --nocapture
cargo +1.98.1 test -p gpui-box --features retained-text,retained-wgpu --locked --offline --quiet hardware_inferred_measurement_tasks -- --ignored --nocapture
cargo +1.98.1 test -p gpui-box-example-hello --features retained-wgpu --locked --offline --quiet hardware_inferred_measurement_noto_plan -- --ignored --nocapture
cargo +1.98.1 clippy -p gpui-box --features retained-text,retained-wgpu --lib --no-deps --locked --offline -- -D warnings
cargo +1.98.1 check -p gpui-box-rendering-perf --features retained-gpu --locked --offline --quiet
```
