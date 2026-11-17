# Ordinary inline allocation builder evidence, 2026-11-17

Worktree: `D:\oss\ggpui-worktrees\rendering-performance-research`.
Branch: `codex/rendering-performance-research`.
See [contract and ADR](../../RETAINED_INLINE_ALLOCATION_BUILDERS.md).
The full RFC goal stays active. No timing, memory or sustained-frame gain is claimed.

## Finite controls

- A normal, uncontained Column host with padding5 receives80/80/140/140/80px.
  Keyed descendants have20/60px content height and intentionally overflow to
  width400. Incoming known width and its definiteness, parent basis, loose0..infinity
  bounds and actual Content mode are checked. Host height is30/70; following
  footer placement and all CPU pixels match a fresh complete ordinary Flex tree.
  Resolved/local paths, repeated equal branches, old frames after owner drop,
  cleared final observations and the next ordinary0-layout frame are asserted.
- Nested builders read newly staged ordinary hosts under a Tight/Loose box
  padded3 and outer host padded5. Actual incoming widths are viewport minus6
  and minus16. Content height propagates through both hosts/wrapper/footer.
- An enforced independent custom inline phase assigns70px under140px root.
  The ordinary host preserves that slot and content-derived40px height.
  Replacing the parent snapshot with a new equal-output algorithm after reading
  rejects by dependency identity; numeric equality is insufficient.
- Post-observation root width, Row, Wrap or aspect changes reject without
  publishing geometry/revision/stats/tracking. Nonstretched auto, aspect and
  intrinsic hosts reject. Even exact min=max40px hosts under Row/wrap/aspect
  ancestors reject: used-width proof cannot substitute for incoming-input proof.
- Percentage roots under Unbounded/MinContent/MaxContent with no anchor reject
  before callbacks. Numeric80px root width and hard-tight80px external width
  both allow ordinary content-derived host height under unbounded soft space.
- Normal typed root style source80/80/140/80 retains render/state handles for
  equal keys and runs/cleans effects at actual branch retirement. A later source
  update, provisional key99, binding removal and changed final root width reject;
  prior source/stats/revision/effects survive, and a subsequent source update
  proves the live binding also survived rollback.
- Real bundled Noto text is shaped/rasterized and keyed by incoming80/140/80px
  width inside an ordinary host padded5. Complete host/text/footer LayoutResult,
  actual paragraph height at width minus10 and all CPU glyph pixels match a
  fresh full ordinary Flex tree. Old glyph frames survive font/resource/UI drop.
- Vendor tracking tests check actual final100px input, cache reuse, non-writing
  size/baseline queries, clone isolation, hidden-ancestor retirement, restored80px
  input, node deletion/reuse and tree clear.

Explicit WGPU execution on NVIDIA GeForce GTX1650Ti / Vulkan / driver571.96
submitted80/140/80px local frames. All pixels matched fresh complete ordinary
Flex references for all three submissions. This is finite offscreen correctness,
not display-visible, heavy-effects or sustained-presentation qualification.
These are same-kernel controls; no new independent browser-conformance count.

## Failures found during implementation

An initial test assumed Inherent mode; actual ordinary Flex final assignment is
Content after item style resolution, and the assertion was corrected. Repeated
unchanged construction first failed because layout dirty alone did not defeat a
cached frame; final observation now also marks frame work dirty. A source-driven
rollback fixture initially patched a field still written by a changed live
binding, so it never changed the final input. It now stages unbinding before the
patch, and verifies rollback preserves the binding. Additional review identified
the unresolved percentage-root risk; a real external-inline-anchor guard and
pre-callback rejection tests were added. These failures are not counted as passes.

## Checks and reproduction

| Check | Actual result |
| --- | --- |
| retained full suite |436 passed |
| retained-text + retained-wgpu full suite |469 passed,29 ignored |
| explicit ordinary-inline WGPU case |1 passed,3 complete-pixel submissions |
| hello retained-wgpu, including actual Noto glyphs |45 passed,2 ignored |
| vendor default / project-config library suite |228 /222 passed |
| framework library Clippy `-D warnings` |passed |
| vendor no-default / alloc+tree+Flex / all-features checks |passed |
| scoped framework/glyph/vendor-test rustfmt and git diff check |passed |

The final six-case builder-focused suite passed after adding the external-anchor
guards. The full suites above include those guards and the source/binding
rollback fixture; an earlier435-test run failed that fixture and is not a pass.

```powershell
cargo +1.98.1 test -p gpui-box --features retained --locked --offline --quiet
cargo +1.98.1 test -p gpui-box --features retained-text,retained-wgpu --locked --offline --quiet
cargo +1.98.1 test -p gpui-box --features retained-text,retained-wgpu --locked --offline --quiet hardware_ordinary_inline_allocation -- --ignored --nocapture
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

Suites overlap. Ignored GPU cases are evidence only when explicitly executed.
The temporary vendor test profile does not change production settings. Existing
retained-only unused SpatialUpdate fields and dependency linker messages remain;
Clippy claims framework library scope. See the ADR for extra map/cache, preview,
proof and forced observation-pass costs and all remaining acceptance gates.
