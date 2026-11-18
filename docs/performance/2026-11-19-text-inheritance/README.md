# Logical inherited Text verification, 2026-11-19

Base: 7a338506a2d44ebca4c93306bab8316464e74203.
Worktree: `D:\oss\ggpui-worktrees\rendering-performance-research`.
Branch: `codex/rendering-performance-research`.
[API](../../RETAINED_TEXT_COMPONENTS.md) · [Decision](../../ADR_RETAINED_TEXT_COMPONENTS.md).
Complete RFC remains active; “99%” is figurative, not a numerical gate.

## Controls

Core controls cover independent inherited properties, Initial vs Inherit,
absolute vs multiplier line height, finite/positive/range validation, pure
Send/Sync data, nested partial scopes, live outer updates, local state and
identity retention, no added render boxes, rollback and retirement. A shared
portal render host cannot change logical text inheritance. Standalone optional
absence still keeps source changes in the complete-frame validation path.

Real Noto Text uses nested partial scopes and local text declarations over nine
successive states: initial, masked outer foreground, restored inheritance,
outer font-size/width, local size/absolute-height, masked outer size, new content,
explicit font revision, and width-only change. At 1.25 scale all layout results,
first/last baselines, following ordinary Flex sibling and complete CPU/GPU images
compare against freshly shaped explicit full ordinary Flex references. Resource
preparation counts and identity check that foreground/masked/width-only changes
reuse resources; color-only/masked changes do not add layout runs. Same scope/
node identity survives. Invalid scale, tab, actual line height and inherited
size reject with previous complete output; old frame outlives UI/font owners.

GPUI TestWindow simulates a click on ordinary Text, runs Activate/EventContext
to change the inherited size to 22 and foreground, installs the rewrapped full
CPU picture and matching input scene. Actual installed bytes and LayoutResult
compare with an explicit full Flex/paragraph reference. A custom adapter control
checks that explicit font revisions reach preparation and a wrong-scale resource
is rejected with previous output intact, then a valid revision retries.
This is owner/event/
image installation, not OS injection or displayed cadence.

## Reproduction and results

```powershell
cargo +1.98.1 test -p gpui-box --features retained --locked --offline
cargo +1.98.1 test -p gpui-box --features retained-text,retained-wgpu --locked --offline
cargo +1.98.1 test -p gpui-box-example-hello --features retained-wgpu --locked --offline
cargo +1.98.1 test -p gpui-box-example-hello --features retained-wgpu --locked --offline hardware_inherited_text_real_noto -- --ignored --nocapture
cargo +1.98.1 clippy -p gpui-box --features retained-text,retained-wgpu --lib --no-deps --locked --offline -- -D warnings
```

Completed-source results. Windows MSVC,
Rust/Cargo 1.98.1, locked/offline debug correctness.

| Check | Result | Log under `target/text-style-validation/` |
| --- | --- | --- |
| Retained-only | 800 passed, 1 ignored; 16 compiler doctests | `core-retained-final.log` |
| Text plus WGPU | 835 passed, 69 ignored; 16 compiler doctests | `core-verified.log` |
| Hello including inherited text host click and adapter rejection | 100 passed, 40 ignored | `hello-complete.log` |
| Explicit real Noto GPU | 1 passed; 9 complete images at 1.25 scale | `hardware-noto.log` |
| Strict library Clippy `-D warnings` | Passed | `clippy-verified.log` |
| CPU desktop without WGPU | Passed | `cpu-desktop-verified.log` |
| Desktop plus retained-gpu harness | Passed | `host-verified.log` |

Scoped format/diff and 5 inherited WIP hashes pass. Focused traversal reaches
307 documents and checks 1,165 local links with no missing target. Explicit GPU:
NVIDIA GeForce GTX 1650 Ti, Vulkan, driver 571.96. Ignored tests do not establish
GPU execution.
This is an API/correctness stage with no new wall-time/FPS/tail or memory claim.
Owner preparation, consumer fanout, ordinary graph/candidate/layout, explicit
host invalidation and unsupported text restrictions remain. Rich text, semantic
source unification, budgeted preparation and independent presentation stay open.
