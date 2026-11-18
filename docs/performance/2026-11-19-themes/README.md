# Typed logical theme verification,2026-11-19

Base:6de978c37e214f79541df85b44beabe219e5fa98.
Worktree:`D:\oss\ggpui-worktrees\rendering-performance-research`.
Branch:`codex/rendering-performance-research`.
[Usage](../../RETAINED_THEMES.md) · [Decision](../../ADR_RETAINED_THEMES.md).
Full RFC remains active; no numerical99% or p99 gate.

## Controls

Eight core controls exercise complete defaults/reset, key identity, invalid/
nonreflexive construction, Send/Sync group snapshots, portal/nearest logical
inheritance, render reparent independence, source equality, unchanged projection
sink suppression, color-only zero-layout routing, missing/foreign/replaced source
errors, scope/source retirement and failed source/frame/provider adoption.
Transparent scope adds no render node and forwards live theme/child inputs
together; a derived reader rejects invalid tuples, same child identity/local
state remains, and failed final tuple retains the previous complete frame.
Old owned frames remain consumable after UI retirement.

Real Noto theme scope uses90/145/65px width,4/8/2px padding, background/foreground
changes, and a following ordinary Flex sibling in separate logical component
sites. LayoutResult/first-last baseline/sibling geometry and all CPU/GPU pixels
compare with a fresh ordinary full Flex/text reference at1.25scale. Theme switches
retain mount count1. Old glyph frame outlives font/UI owners.

GPUI TestWindow installs the CPU image of a source-themed button, routes a click
through ordinary Activate/EventContext, changes theme width80→140/color, installs
the complete new CPU image and matching input scene, and compares installed
bytes with full numeric Flex. Same scope/render identity is retained; no pending
CPU result/error remains. This is host/event integration, not OS input injection
or display cadence.

## Reproduction

```powershell
cargo +1.98.1 test -p gpui-box --features retained --locked --offline
cargo +1.98.1 test -p gpui-box --features retained-text,retained-wgpu --locked --offline
cargo +1.98.1 test -p gpui-box-example-hello --features retained-wgpu --locked --offline
cargo +1.98.1 test -p gpui-box-example-hello --features retained-wgpu --locked --offline hardware_theme_scope_real_noto -- --ignored --nocapture
cargo +1.98.1 clippy -p gpui-box --features retained-text,retained-wgpu --lib --no-deps --locked --offline -- -D warnings
```

## Completed-source validation

Windows MSVC, Rust/Cargo 1.98.1, locked/offline debug correctness.

| Check | Result | Log under `target/theme-validation/` |
| --- | --- | --- |
| Retained-only | 795 passed, 1 ignored; 16 compiler doctests | `core-retained-verified.log` |
| Text plus WGPU | 830 passed, 69 ignored; 16 compiler doctests | `core-verified.log` |
| Hello including simulated click / actual CPU image install | 96 passed, 39 ignored | `hello-final.log` |
| Explicit real Noto GPU comparison | 1 passed, 3 complete images at 1.25 scale | `hardware-noto.log` |
| Strict library Clippy `-D warnings` | Passed | `clippy-final.log` |
| CPU desktop without WGPU | Passed | `cpu-desktop-check.log` |
| Desktop plus retained-gpu harness check | Passed | `host-check.log` |

Explicit GPU adapter: NVIDIA GeForce GTX 1650 Ti, Vulkan, driver 571.96.
Ignored tests do not establish GPU execution. Scoped format/diff checks
and the five inherited WIP baseline hashes pass. A focused local-link traversal
from the progress/architecture documents reaches 304 documents and checks
1,149 links with no missing target.

This is a capability/correctness stage, without
new FPS/tail-latency/byte-memory or sustained60/120Hz claims. Owner selector
fanout, source/graph/candidate/layout costs still apply. Portable token IR,
automatic text inheritance, parts/patch/system subscription, budgeted theme
scene preparation and independent native presentation remain open.
