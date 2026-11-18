# Keyed list reading-position verification, 2026-11-19

Worktree: `D:\oss\ggpui-worktrees\rendering-performance-research`.
Branch: `codex/rendering-performance-research`.
Base: `aefa7a942b22cded76394a563cd5cbe656096397`.
See [API](../../RETAINED_VIRTUAL_LIST.md) and
[decision/costs](../../ADR_RETAINED_LIST_ANCHORING.md).
Full RFC goal remains active; no numerical99%/p99 acceptance gate.

## Controls

- Pure keyed prepend/remove/reorder/declared-height changes preserve anchor
  position; removed anchor prefers following then preceding surviving key.
  Empty/no-survivor, shorter anchor, invalid tolerance,
  duplicate keys and metric/key length checks do not invoke payload factories.
- FollowEnd follows only an existing reader at the end, including explicit empty
  start; a reader who scrolls away keeps that key. Viewport resize and data append
  adopt one tuple and retain the mounted list instance.
- 100,000 indexed keys plus one prepend rebase a distant anchor without any row
  factory invocation. This is metadata/structural evidence, not byte/timing proof.
- Ordinary ComponentProducer consumes dataset source. Multiple data transitions
  retain live local state and the same anchor node at y=-5px, matching a fresh
  complete Flex tree's CPU pixels. Hardware variant compares full GPU images.
- A wheel before data arrival changes source first; rebase uses that newer value.
  Invalid final opacity and derived tuple failure keep old data/source/lifecycle/
  frame. Helper savepoint restores partial dataset writes when scroll is readonly.
  Derived data+scroll observer sees final tuple once, never an invalid midpoint.
- Ordinary activation invokes EventContext rebase and publishes data/scroll/frame
  in the same input candidate.
- Real Noto at1.25scale and fractional offset compares pre-update visible image
  with prepend/height-change/remove results, plus independent full data Flex/text
  CPU/GPU images. Old immutable glyph frame outlives producer/font owners.
- GPUI TestWindow native wheel first advances source, then100,000-key dataset
  prepend rebases45→65px and installs a newer complete CPU image/matching input
  scene while retaining visible content. This is application/native-event
  integration, not OS injection or displayed cadence.

## Reproduction

```powershell
cargo +1.98.1 test -p gpui-box --features retained --locked --offline
cargo +1.98.1 test -p gpui-box --features retained-text,retained-wgpu --locked --offline
cargo +1.98.1 test -p gpui-box-example-hello --features retained-wgpu --locked --offline
cargo +1.98.1 test -p gpui-box --features retained-text,retained-wgpu --locked --offline hardware_virtual_list_anchor -- --ignored --nocapture
cargo +1.98.1 test -p gpui-box-example-hello --features retained-wgpu --locked --offline hardware_virtual_list_anchor_real_noto -- --ignored --nocapture
```

Final retained-only771passed/1ignored and text/WGPU806/69; each15compiler doctests.
Hello92passed/37ignored, including regular Noto anchor/control and native host
wheel→prepend→complete CPU installation. The two installed CPU images have equal
bytes after45→65px compensation. Eight regular core anchor controls pass.
Explicit core anchor GPU1passed/four complete images and real-Noto anchor
GPU1passed/three complete images at1.25scale/fractional offset, on
GTX1650Ti/Vulkan/driver571.96. Library Clippy-Dwarnings, CPU desktop without WGPU,
full retained-gpu harness, scoped format/diff,1,133local links across296docs and
five inherited WIP hashes pass. Windows MSVC/Rust-Cargo1.98.1/locked-offline debug
correctness. Ignored cases do not count as hardware execution. No new FPS/
tail-latency/byte-memory or sustained60/120Hz gain claim.
Metadata index rebuild/deletion scan, declared heights, ordinary candidate/layout
costs and host-driven native presentation remain limits.
