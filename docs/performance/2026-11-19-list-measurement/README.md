# Mounted-row automatic height verification, 2026-11-19

Worktree: `D:\oss\ggpui-worktrees\rendering-performance-research`.
Branch: `codex/rendering-performance-research`.
Base: `9a6a4e5ffc1e760f98a1045622690028d473d998`.
See [API](../../RETAINED_VIRTUAL_LIST.md) and
[decision/costs](../../ADR_RETAINED_LIST_MEASUREMENT.md).
Full RFC remains active. Figurative99% creates no numeric/p99 acceptance gate.

## Controls and evidence boundaries

- 100,000 keyed estimates with20px declared/30px ordinary Flex content measures
  seven mounted rows at offset45px and20px overscan, updates only those heights,
  and rebases offset to55px to preserve reading key2/inset5px. Less than100 live
  sources; payload calls stay below20 for this transition. A stable follow-up
  makes no source transaction/revision, subsequent unchanged revisions skip queries.
- Smaller40px estimates/20px content expose new rows on later turns. Each call
  does one round; query budget failure adopts no data/offset/frame. Sparse extent
  updates validate/coalesce and equal values share the original prefix snapshot.
- Hidden/zero-width lists retain estimates; unmount removes the registration.
  Duplicate feedback source ownership and unindexed replacement reject atomically.
  Invalid downstream opacity during feedback keeps the old source tuple/frame.
  FollowEnd responds to current near-end intent; later scroll-away plus new data
  preserves that newer reader intent. Resolved and local frame entry paths work.
- Public batched dry queries match individual queries and retain published
  geometry/frame after a later invalid query. No live cache warming is claimed.
- Real Noto long/short Chinese/English text uses natural height, with widths100,
  65 and110. Mounted metrics match paragraph wrapping; entire CPU images and
  explicit GPU images compare against a fresh ordinary full40-row Flex tree.
  Offscreen rows are not required to have measured metrics. Old glyph frames
  remain consumable after UI/font owners are dropped.
- GPUI TestWindow OwnerDemo automatically measures before normal CPU image
  construction, updates45→55px, installs a new complete picture/matching input
  scene, and compares installed image bytes with an ordinary full Flex reference.
  It verifies host integration, not OS input injection or visible display cadence.

## Reproduction

```powershell
cargo +1.98.1 test -p gpui-box --features retained --locked --offline
cargo +1.98.1 test -p gpui-box --features retained-text,retained-wgpu --locked --offline
cargo +1.98.1 test -p gpui-box-example-hello --features retained-wgpu --locked --offline
cargo +1.98.1 test -p gpui-box-example-hello --features retained-wgpu --locked --offline hardware_measured_virtual_list_real_noto -- --ignored --nocapture
cargo +1.98.1 clippy -p gpui-box --features retained-text,retained-wgpu --lib --no-deps --locked --offline -- -D warnings
```

Final retained-only780passed/1ignored; text/WGPU815passed/69ignored; each15compiler
doctests. Hello94passed/38ignored, including the regular Noto measurement and
installed complete CPU picture controls. Explicit measured-Noto GPU1passed,
three complete images at1.25scale on GTX1650Ti/Vulkan/driver571.96. Eight regular
core list-measurement controls plus one public batch-query control pass.
Library Clippy-Dwarnings, CPU desktop and retained-gpu harness checks pass.
Scoped rustfmt/diff checks,1,144local links across298documents and five inherited
WIP hashes pass. Windows MSVC/Rust-Cargo1.98.1/locked-offline debug correctness;
ignored tests are not counted as hardware execution. Logs
live in `target/measured-list-validation/`. Debug correctness tests do not establish
release timing improvements. Prefix rebuilding O(N), private live-tree copies,
ordinary source/candidate/layout cost, estimate evolution across turns and
GPUI-driven final presentation remain limits. No sustained60/120Hz, tail-latency,
byte-memory or new FPS gain claim.
