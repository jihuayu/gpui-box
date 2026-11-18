# Virtual-list and controlled-wheel verification, 2026-11-19

Worktree: `D:\oss\ggpui-worktrees\rendering-performance-research`.
Branch: `codex/rendering-performance-research`.
Base: `63b74036ca89500c249a0321a16d9ac97d6a3d8a`.
See [contract](../../RETAINED_VIRTUAL_LIST.md) and
[decision/costs](../../ADR_RETAINED_VIRTUAL_LIST.md).
Full RFC goal stays active. "99%" remains figurative common fluid/intuitive UI.

## Controls

- 100,000 fixed20px rows with100×80 viewport and20px overscan: initially5live
  row components, at most6across distant jumps, fewer100live sources and20paint
  items. Only selected row factories run. Overlap keeps state/output identity;
  retirement rejects old source handles. Removing list cleans all row effects.
- Declared variable extents exercise exact window boundaries, limits, empty/zero
  viewport, invalid/overflow inputs and nearest reveal. All rows in the small
  full reference are constructed independently, including offscreen content.
- Multiple offsets/large jumps and wheel movement compare all CPU pixels to a
  fresh full Flex tree. Explicit hardware variant compares all GPU pixels too.
- Data reorder and viewport resize preserve overlapping keyed local state;
  data shrink clamps effective offset and retires absent rows. Requested source
  remains explicit. Invalid limits/duplicate selected keys preserve old complete
  frame/source/lifecycle; no partial row adoption.
- Retired virtual target still passes residual wheel distance from its handling
  list host to an ordinary outer scroll default. Final opacity failure rolls
  source, lifecycle/router/frame back. Precision test cannot consume a1px delta
  beyond a representable f32 step or reverse residual sign.
- Mounted focus/capture retire on matching scene adoption; off-window reveal
  mounts a new row, then permits explicit focus. Row task cancels and late output
  rejects. Old immutable frame remains consumable after owner destruction.
- Proven ordinary two-axis Flex allocation composes with list viewport props on
  resize, preserving explicit parent/window distinction and bounded row work.
- Real Noto rows at1.25scale and fractional scroll offsets compare full CPU/GPU
  images to a full ordinary Flex/text reference. Hidden text baselines are not
  measured; host box/content/overflow and visible text image are the assertions.
- GPUI TestWindow sends a native ScrollEvent through the host adapter and public
  list handler, updates scroll source and installs a complete CPU picture/input
  scene with bounded retained items. This is app/native-event integration, not OS
  injection or displayed-cadence evidence.

## Reproduction

```powershell
cargo +1.98.1 test -p gpui-box --features retained --locked --offline
cargo +1.98.1 test -p gpui-box --features retained-text,retained-wgpu --locked --offline
cargo +1.98.1 test -p gpui-box-example-hello --features retained-wgpu --locked --offline
cargo +1.98.1 test -p gpui-box --features retained-text,retained-wgpu --locked --offline hardware_virtual_list -- --ignored --nocapture
cargo +1.98.1 test -p gpui-box-example-hello --features retained-wgpu --locked --offline hardware_virtual_list_real_noto -- --ignored --nocapture
```

Final retained-only763passed/1ignored and text/WGPU798/68; each15compiler
doctests. Hello91passed/36ignored, including real host wheel/CPU image installation
and the regular Noto full-reference control. Nine regular core list/scroll controls
pass. Library Clippy-Dwarnings, CPU desktop without WGPU, complete retained-gpu
harness, scoped format/diff,1,124local links across294reachable docs and five
inherited WIP hashes pass. Explicit core list GPU1passed with five complete images
and real-Noto list GPU1passed with four complete images at1.25scale/fractional
offsets on GTX1650Ti/Vulkan/driver571.96. Windows MSVC/Rust-Cargo1.98.1/locked-offline
debug correctness. Ignored tests are not hardware executions. No new FPS/tail-latency/byte-memory or sustained60/120Hz
improvement claim. Declared row heights, mounted-only focus, ordinary candidate
scans, factory/mount work and host-driven presentation remain documented limits.
