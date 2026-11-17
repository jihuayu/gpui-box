# Content measurement demand evidence, 2026-11-17

Worktree: `D:\oss\ggpui-worktrees\rendering-performance-research`.
Branch: `codex/rendering-performance-research`.
See [contract and ADR](../../RETAINED_CONTENT_AXIS_MEASUREMENT.md).
Full RFC acceptance stays open. No benchmark/timing/peak-memory gain is claimed.

## Finite controls

- A content client models12 natural10x8 cells and returns zero for an unrelated
  scalar extent. Three horizontal probes (MinContent/MaxContent/Definite44) on
  padding2 yield14/124/44 with exactly three horizontal calls, rather than a
  dependent-height pass. A vertical44px query first observes complete demand,
  then Vertical, returning28 outer height. Complete baseline output is44x28,
  first8/last24; final normal output uses complete natural overflow.
- Eight box scenarios × four soft/intrinsic modes × two axes produce64 scalar
  comparisons against a complete-metrics content control. Scenarios include
  width MinContent limit, height Min/MaxContent limits, simultaneous width/height
  limits, authored ratio2, InlineSize containment with40px fallback and explicit
  six-pixel Scroll gutters (the gutter case uses the existing kernel-style
  compatibility entry). Normal final LayoutResult also matches in all cases.
  Internal height-limit requests are asserted as Vertical rather than outer
  Horizontal. No callback is used as a second box model.
- Vendor direct tests observe Horizontal/Vertical/Both for width/height limits
  and final layout, for all three incoming axes. Partial responses produce the
  same complete output as the legacy callback wrapper. Known-size ComputeSize
  still short-circuits, while unknown-size scalar demand reaches the callback.
- Normal typed measure/style sources change counts/widths12/44→5/44→12/84→12/44.
  Resolved/local complete host/footer LayoutResult and all CPU pixels match a
  separately built ordinary Flex tree containing actual10x8 children. Those
  reference leaves have first baseline6, so actual line baseline propagation is
  also checked. Restoring the same final snapshot within one transaction gives
  unchanged revision and0 layout; old frames work after UI retirement.
- A client answers a scalar width but rejects Both. Width query20 succeeds,
  complete-baseline query fails and no frame is published. During normal atomic
  edit_frame/edit_local_frame replacement, the complete failure preserves prior
  source, reactive stats, geometry and published revision. Scalar success cannot
  authorize incomplete final geometry.
- The existing actual bundled Noto content client now intentionally returns
  unrelated-axis placeholders.60/140px widths and text replacement compare
  scalar axes, Content Min/MaxContent widths, complete size/baselines/footer and
  all CPU colored-box pixels to the independent ParagraphSnapshot adapter.
  These are real font metrics, not character-width approximation or a new custom
  glyph-paint protocol. Old frames survive content/font/UI owner retirement.
- Explicit WGPU submits the four count/width states and compares all pixels to
  the ordinary Flex reference. Actual NVIDIA GeForce GTX1650Ti / Vulkan /
  driver571.96 execution passed all four complete-pixel submit/readback
  comparisons. This is finite offscreen correctness. Ignored hardware tests
  alone are not passing evidence.

These are same-kernel protocol/box-model controls, not independent CSS browser
conformance. Initial test compilation found an attempted public scrollbar_width
field (not part of LayoutStyle); the gutter fixture now uses the existing
kernel compatibility style and does not add a new public field. Vendor tests
also needed explicit default Style type annotations. Such failures are not
counted as successful validation.

## Final checks and reproduction

| Check | Actual result |
| --- | --- |
| retained full suite |430 passed |
| retained-text + retained-wgpu full suite |463 passed,28 ignored |
| explicit content-axis WGPU case |1 passed,4 complete-pixel submissions |
| hello retained-wgpu, including real Noto content metrics |44 passed,2 ignored |
| vendor default / project-config library suite |227 /221 passed |
| framework library Clippy `-D warnings` |passed |
| vendor no-default / alloc+tree+Flex / all-features check |passed |
| scoped framework/glyph/vendor-test rustfmt and git diff check |passed |

Suites overlap and their counts must not be aggregated as independent coverage.
Ignored hardware tests count only when actually executed. The final extra
scalar-success/complete-failure assertions were also rerun explicitly after
being added. Vendor suites used the existing temporary low-memory profile below;
production settings were not changed. Retained-only original unused SpatialUpdate
fields and dependency linker messages remain; Clippy claims framework library scope.

```powershell
cargo +1.98.1 test -p gpui-box --features retained --locked --offline --quiet
cargo +1.98.1 test -p gpui-box --features retained-text,retained-wgpu --locked --offline --quiet
cargo +1.98.1 test -p gpui-box --features retained-text,retained-wgpu --locked --offline --quiet hardware_content_axis -- --ignored --nocapture
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

## Costs and remaining acceptance

The input gains a temporary optional enum. Common leaf arithmetic is shared
with a legacy wrapper; there is no extra persistent style/node/cache/GPU/frame
field, snapshot lease, iterative solver or duplicate layout algorithm. Width
can skip a dependent-height call, but intrinsic limits still need their own
probes and height may require initial full metrics. Public private-tree clones,
exact caches and ancestor work retain costs; actual struct padding, wall-time,
peak memory and tail latency were not measured. Modern GPU heavy effects, CPU
continuous complete-frame smoothness, sustained60/120Hz and display-visible
input latency remain separate acceptance work.
