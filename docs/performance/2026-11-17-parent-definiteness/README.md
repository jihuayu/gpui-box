# Owned parent selected-size provenance, 2026-11-17

Worktree: `D:\oss\ggpui-worktrees\rendering-performance-research`.
Branch: `codex/rendering-performance-research`.
See [contract and architecture decision](../../RETAINED_PARENT_DEFINITENESS.md).
The complete RFC goal remains active. No wall-time improvement, memory reduction,
p99 or sustained visible-frame qualification is claimed.

## Reproduction and finite controls

Before the final-basis fix, the focused regression observed baseline42 where12
was expected: its client policy intentionally exports measured child width as
the first baseline, plus2px host top inset. Parent-selected84x64/padding2 with
false width definiteness should leave the natural10px child width unchanged;
the old final callback resolved the50% child to40px. This failure was observed
in tool output. An earlier test-setup assertion incorrectly expected fixed
baseline metrics to scale with height; that was replaced with explicit measured
size export before validating the real failure.

```powershell
cargo +1.98.1 test -p gpui-box --features retained --locked --offline --quiet definiteness_tests -- --nocapture
cargo +1.98.1 test -p gpui-box-example-hello --features retained-wgpu --locked --offline --quiet owned_parent_selected_indefinite -- --nocapture
```

- Public dry complete metrics preserve84x64 selected size through false/false,
  true/true, true/false, false/true and restored flags. A50% child with natural
  10x12 content and10% padding uses40px width and8px padding only with definite
  host width. Automatic child height then includes its real insets. First/last
  metrics expose the actual child sizes. No frame is published.
- Callback clients directly observe full and inline definiteness, including
  selected dimensions, hard-tight overrides and initially absent dimensions.
  A normally selected20px width becomes definite, while the final auto10px
  height stays content-derived. Both metrics and final-plan calls assert input
  consistency. Pure policies can now reject indefinite inputs explicitly.
- Normal typed parent sources cycle the same dimensions with different flags
  through both a measured custom parent and Loose ConstraintBox. Resolved/local
  frames compare complete host/child LayoutResult and all CPU pixels against
  independently authored numeric-size/numeric-padding full references. This
  covers final child padding metadata as well as painted bounds and overflow.
  Restoring the original source handle within a transaction gives unchanged
  revision and0 layout; old packets survive owner destruction.
- A policy rejects the newly observable indefinite width during atomic
  edit_frame/edit_local_frame. Source, geometry, reactive stats and published
  revision are unchanged; the next warm frame needs0 layout and matches old
  complete pixels. Ordinary previously committed transaction behavior is not
  redefined by this test.
- A standalone vendor test alternates equal80x60 selected sizes with all axis
  flags using both exact size and complete baseline queries. Its child has50%
  dimensions and independent natural10x12 content. Restored flags exercise
  cache keys; the policy also asserts callback provenance in final phases.
- A real bundled Noto paragraph under selected144x400/padding2 retains its
  natural MaxContent width with indefinite inline size, or resolves70px with
  definite inline size. Exact glyphs, full host/text LayoutResult and all CPU
  pixels match separately authored numeric-width controls over false→true→false.
  Old frames remain valid after font/raster/content/tree owners drop. The first
  320x600 CPU fixture used an insufficient1MiB working budget and returned
  ResourceLimit; raising only its test budget to4MiB permits the complete test.
- The explicit hardware case cycles five flag states in each of the custom
  parent and ConstraintBox trees, comparing all output pixels with the same
  separately authored numeric full controls. Explicit execution on NVIDIA
  GeForce GTX1650Ti / Vulkan / driver571.96 passed all10 submit/readback
  comparisons. This is finite offscreen correctness, not visible presentation.
  Ignored hardware tests alone are not passing evidence.

These full controls share the existing framework box algorithms but author no
percentage sizes/padding for the target child. There is no browser comparison
for arbitrary pure parent policies, and this does not qualify broad CSS behavior.

## Final checks and reproduction

| Check | Actual result |
| --- | --- |
| retained full suite |420 passed |
| retained-text + retained-wgpu full suite |453 passed,26 ignored |
| explicit owned-parent definiteness hardware case |1 passed,10 complete-pixel submits |
| hello retained-wgpu, including actual Noto glyphs |43 passed,2 ignored |
| vendored Taffy default / project-config library suite |224 /218 passed |
| framework library Clippy `-D warnings` |passed |
| vendor no-default / alloc+tree+Flex / all-features check |passed |
| scoped framework/glyph/vendor-test rustfmt and git diff check |passed |

Suites overlap, so counts must not be aggregated as independent coverage.
The initial text/WGPU suite compiled before the final policy-failure test and
reported452 passed; the final rerun453 includes it. Only actually executed
hardware cases count as passed. Vendor suites used the existing temporary
low-memory test profile below; no production settings were changed. Original
retained-only unused SpatialUpdate-field warnings and dependency linker
messages remain; Clippy claims framework library scope only.

```powershell
cargo +1.98.1 test -p gpui-box --features retained --locked --offline --quiet
cargo +1.98.1 test -p gpui-box --features retained-text,retained-wgpu --locked --offline --quiet
cargo +1.98.1 test -p gpui-box --features retained-text,retained-wgpu --locked --offline --quiet hardware_owned_parent_definiteness -- --ignored --nocapture
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

## Cost boundary

The framework and engine callback input structs gain bool provenance fields;
actual struct padding/whole-tree peak allocations were not measured. No new
style, persistent node/cache, GPU or FramePacket field, resource lease, iterative
solver or callback pass is added. Existing private candidates/exact caches retain
their memory/time cost. Modern GPU heavy effects, CPU continuous complete-frame
smoothness, sustained60/120Hz and visible input tails remain separate work.
