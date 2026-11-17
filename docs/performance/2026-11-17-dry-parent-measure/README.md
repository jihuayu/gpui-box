# Dry parent measurement validation — 2026-11-17

Windows x64 / Rust1.98.1 locked/offline, existing research worktree. Callback
counts establish avoided custom placement plans, not wall-time or p99 gains.

## Commands

```powershell
cargo +1.98.1 test -p gpui-box --features retained --locked --offline --quiet
cargo +1.98.1 test -p gpui-box --features retained-text,retained-wgpu --locked --offline --quiet
cargo +1.98.1 test -p gpui-box --features retained-text,retained-wgpu --locked --offline dry_tests -- --include-ignored --nocapture
cargo +1.98.1 test -p gpui-box-example-hello --features retained-wgpu --locked --offline --quiet
cargo +1.98.1 test --manifest-path vendor/taffy/Cargo.toml --lib --locked --offline --quiet
cargo +1.98.1 test --manifest-path vendor/taffy/Cargo.toml --locked --offline --no-default-features --features std,taffy_tree,flexbox,grid,block_layout,float_layout,content_size --lib --quiet
cargo +1.98.1 check --manifest-path vendor/taffy/Cargo.toml --locked --offline --no-default-features --quiet
cargo +1.98.1 check --manifest-path vendor/taffy/Cargo.toml --locked --offline --no-default-features --features alloc,taffy_tree,flexbox --quiet
cargo +1.98.1 check --manifest-path vendor/taffy/Cargo.toml --locked --offline --all-features --quiet
cargo +1.98.1 clippy -p gpui-box --features retained-text,retained-wgpu --lib --no-deps --locked --offline -- -D warnings
git diff --check
```

## Evidence and limits

| Check | Result |
| --- | --- |
| gpui-box retained | 359 passed |
| gpui-box retained-text + retained-wgpu | 392 passed, 15 ignored |
| Explicit dry-tests suite, including actual WGPU | 9 passed |
| hello retained-wgpu, including real paragraph metrics/pixels | 30 passed, 2 ignored |
| Taffy default / project feature library suites | 209 / 203 passed |
| Taffy no-default, alloc/Flex and all-features checks | Passed |
| Framework library Clippy, warnings denied | Passed |
| git diff --check | Passed |

Focused suites overlap the full suites and are not additional independent totals.
The ordinary suites' ignored hardware tests are not counted as passed. The
explicit dry suite includes the real WGPU case on the adapter named below.

Dry/legacy client Stack parents measure identical real wrapping Flex descendants
with5px host padding and a following footer at widths90/50/130/90. Every CPU byte
and hit match ordinary forced-full Flex, in local and resolved output. The dry
client constructs **4 complete plans versus 26 for legacy**, separately in local
and resolved output. It also executes **26 pure metric queries** and still
measures actual children. This is callback/work evidence, not timing evidence.
Old frames remain valid after the UI owner drops. Public MinContent/
MaxContent queries return50x48 and130x28 without a complete custom plan and leave
published geometry/revision/live cache intact.

A finite24-case style/bounds matrix compares dry against complete-plan fallback:
normal, min/max-content preferred width, intrinsic limits, inline/full containment
and aspect ratio, crossed with unbounded, loose70x100 and tight50x60 host bounds.
This is equivalence between these two custom-parent paths, not independent CSS
conformance. Independent ordinary Flex pixels above remain separate evidence.

Nested custom hosts, percentage-basis distinctions, hidden/restoration, typed
snapshot identity/replacement, inline/full enforced assignments, authored metadata
and first/last baseline transport are covered. NaN/negative/nonfinite metrics,
negative-infinity baseline, known-size violation, delegate error and final baseline
disagreement reject atomic frame edits with old source/frame retained.
The subsequent [owned overflow stage](../2026-11-17-owned-container-overflow/README.md)
allows finite signed parent baselines outside host height; the original
out-of-height rejection was too restrictive and its test now rejects nonfinite
negative infinity instead. Leaf intrinsic metrics remain separately validated.
Vendor tests force ComputeSize despite incoming PerformLayout, verify exact repeat hits,
changed-child invalidation, unchanged published layouts in their tested domain,
input/node validation and that successful dry output cannot cache a failing full
plan as successful.

Bundled Noto Sans CJK SC actual text at60/140/90/60 content widths has equal dry
metrics and final host/footer layout, full CPU glyph pixels and hit to an
ordinary full Flex tree. Intrinsic queries after publication do not alter the
old geometry/frame; old glyph frames survive UI/font/raster/paragraph owner drop.

Explicit NVIDIA GeForce GTX1650Ti / Vulkan / driver571.96: four real local GPU
submissions at90/50/130/90 outer widths match the ordinary full reference pixels.
No visible presentation, modern desktop GPU effects, CPU continuous-frame,
sustained60/120Hz or display-visible input-tail qualification follows.

Costs and precise contracts are in [the ADR](../../RETAINED_DRY_PARENT_MEASURE.md).
The public dry query still clones the owned tree; normal baseline/hidden kernel
probes can use temporary layout writes. Optional metrics may add final work.
Adapter-dependent ignored tests do not count as passed; focused suites overlap
full regression suites. All-targets Clippy is outside this stage's claim.
