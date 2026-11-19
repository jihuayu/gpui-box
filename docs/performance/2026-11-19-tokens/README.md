# Typed token / Style IR correctness, 2026-11-19

Base: 820468b96a9919aa69211952dd6d855055a94d0d.
Worktree: `D:\oss\ggpui-worktrees\rendering-performance-research`.
Branch: `codex/rendering-performance-research`.
[API](../../RETAINED_TOKENS.md) · [Decision](../../ADR_RETAINED_TOKENS.md).
Full RFC remains active; “99%” is figurative, not a numerical gate.

Core controls verify typed complete defaults/reset, dependent overrides,
number/linear length/encoded-color operations, Send/Sync immutable data, malformed
operands/type mismatch/NaN/overflow, missing/actual cycle path/duplicate/unknown
override/limits, shorthand projections, rule/condition selection, final opacity
validation and logical provider→computed→ordinary style sink updates. UI schema
7 token batches exercise exact replay, recovery and failed source/frame rollback.

Real Noto paragraph uses token-driven width90/145/65, padding4/8/2 and surface
colors over ordinary StylePlan/UiDocument. Four states compare all bounds/content
bounds, full CPU pixels and full GPU images with explicit literal fresh full
Flex, including a following sibling. Last state changes only an unread token
and requires zero style evaluations. Illegal resolved opacity rejects without
changing accepted source or complete frame; old frame outlives UI/resources.

```powershell
cargo +1.98.1 test -p gpui-box --features retained --locked --offline
cargo +1.98.1 test -p gpui-box --features retained-text,retained-wgpu --locked --offline
cargo +1.98.1 test -p gpui-box-example-hello --features retained-wgpu --locked --offline
cargo +1.98.1 test -p gpui-box-example-hello --features retained-wgpu --locked --offline hardware_token_style_real_noto -- --ignored --nocapture
cargo +1.98.1 clippy -p gpui-box --features retained-text,retained-wgpu --lib --no-deps --locked --offline -- -D warnings
```

Final Windows MSVC / Rust-Cargo1.98.1 / locked-offline debug results:

| Check | Result |
| --- | --- |
| retained-only | 805 passed, 1 ignored; 17 compiler doctests |
| text + WGPU | 840 passed, 69 ignored; 17 compiler doctests |
| hello | 104 passed, 42 ignored |
| Explicit real Noto GPU | 1 passed; four complete images at 1.25 scale, GTX1650Ti / Vulkan / 571.96 |
| Library Clippy with `-D warnings` | Passed |
| CPU desktop without WGPU / retained-gpu harness checks | Passed |
| Scoped rustfmt / diff / inherited WIP | Passed; five baseline hashes unchanged |
| Focused local Markdown links | 313 reachable documents, 1,214 links; no missing paths |

Logs under `target/token-validation/`: `core-retained-complete.log`,
`core-wgpu-verified.log`, `hello-verified.log`, `hardware-noto-verified.log`,
`clippy-verified.log`, `cpu-host-verified.log`, `gpu-harness-verified.log`.
Ignored does not establish actual GPU execution. The explicit hardware test was
rerun on the final source after the FlexBasis Content validation control.
This is a capability/correctness stage with no new wall-time/FPS/memory or
sustained60/120Hz claim. Table resolution, expression vectors, source/candidate/
style/layout work still apply. Portable serialization/ABI, complete tokens/parts,
render-domain preparation and independent presentation remain open.
