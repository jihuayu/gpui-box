# Intrinsic containment fallback validation — 2026-11-17

Windows x64, Rust1.98.1, existing research worktree. Capability/correctness stage;
no timing, peak-memory, sustainedHz or visible-presentation measurement.

```powershell
node docs/performance/2026-11-17-containment-fallback/capture.mjs D:/Scoop/persist/nodejs/bin/node_modules/agent-browser/node_modules/playwright-core/index.mjs
cargo +1.98.1 test -p gpui-box --features retained --locked --offline --quiet
cargo +1.98.1 test -p gpui-box --features retained-text,retained-wgpu --locked --offline --quiet
cargo +1.98.1 test -p gpui-box --features retained-text,retained-wgpu --locked --offline --quiet hardware_intrinsic_fallback -- --include-ignored --nocapture
cargo +1.98.1 test -p gpui-box-example-hello --features retained-wgpu --locked --offline --quiet
cargo +1.98.1 clippy -p gpui-box --features retained-text,retained-wgpu --lib --no-deps --locked --offline -- -D warnings
$env:CARGO_PROFILE_TEST_DEBUG='0'
$env:CARGO_PROFILE_TEST_INCREMENTAL='false'
$env:CARGO_PROFILE_TEST_CODEGEN_UNITS='256'
cargo +1.98.1 test --manifest-path vendor/taffy/Cargo.toml --lib --locked --offline -j 1 --quiet
cargo +1.98.1 test --manifest-path vendor/taffy/Cargo.toml --lib --locked --offline -j 1 --no-default-features --features std,taffy_tree,flexbox,grid,block_layout,float_layout,content_size --quiet
cargo +1.98.1 check --manifest-path vendor/taffy/Cargo.toml --locked --offline --no-default-features --quiet
cargo +1.98.1 check --manifest-path vendor/taffy/Cargo.toml --locked --offline --no-default-features --features alloc,taffy_tree,flexbox --quiet
cargo +1.98.1 check --manifest-path vendor/taffy/Cargo.toml --locked --offline --all-features --quiet
git diff --check
```

The temporary low-memory vendor test profile matches previous stages, after
their LLVM OOM. It does not modify production compiler configuration.

## Independent reference

| Check | Actual result |
| --- | --- |
| retained full suite |412 passed |
| retained-text + retained-wgpu full suite |445 passed,24 ignored |
| explicit hardware intrinsic-fallback case |1 passed |
| hello retained-wgpu including real Noto glyph case |41 passed,2 ignored |
| framework library Clippy `-D warnings` |passed |
| vendored Taffy default / project library suites |221 /215 passed |
| vendor no-default / alloc+tree+Flex / all-features checks |passed |

The suites overlap; ignored hardware tests count only when explicitly executed.
Retained-only retains the pre-existing SpatialUpdate unused-field warning and
dependency builds print linker messages. Clippy is framework library scope,
not an all-targets/dependency Clippy claim.

Chromium145.0.7632.6; exact user agent/schema in browser-reference.json. Raw TSV
has840 cases: four directions × Normal/InlineSize/Size × none/zero/both/height-only/
width-only fallback × BorderBox/ContentBox × seven preferred/limit/allocation
scenarios. Every box has3px padding and2px border per side; actual nested content
is180x90 with shrink disabled. Numeric fallback is60x25 inner units. The narrow
scenario uses30px main space with explicit zero minima; grow uses300x200 parent
space. Positions, host size and actual content size are compared within0.02px.
The JSON also records four ordinary/decoded-image root ratio cases (60x25 with
natural ratio suppressed,60x30 with authored ratio2).

Initial600-case geometry captured browser default flex-start against framework
explicit Start, causing300 reverse-direction position mismatches; all sizes
matched. The fixture now explicitly declares justify-content:start on both sides.
No failed geometry is omitted, and the expanded840-case matrix also includes
shrink/grow instead of only unconstrained placeholders.

## Normal paths and controls

Typed layout sources update fallback width60→100→0→60 through resolved/local
frames; percentage child width follows the actual final inner extent. Complete
CPU pixels match independent full ordinary boxes with explicit used dimensions.
Transaction writes restored to the original final value keep revision/zero
layout, hidden restoration recovers output, and old packets outlive UI owners.
Full-size-contained hosts stay independent of180px-wide responsive descendants;
fallback-width changes cross actual80px query thresholds and idle repeats do no
layout. Changing the host to unsafe InlineSize under Row still rejects. Initial
query test expectations overlooked ordinary cross-axis stretch and child shrink;
the fixture now explicitly aligns the host Start and disables the test child's
shrink, preserving expected natural70px child overflow.

Content/Inherent dry queries, tight bounds, numeric/intrinsic min/max and
ConstraintBox/owned parent common styles preserve the fallback. A pure wrapping
content protocol observes its used inner20/40/100px width. Real Noto shaped
paragraphs wrap at50/120/50px; all CPU glyph pixels equal independent explicit
width full references, and old text frames retain resources after owners drop.
Fixed and decoded80x40 images retain natural-ratio suppression and explicit
ratio semantics. Explicit Scroll gutters reserve6px while only real10x5 content
contributes scrollable overflow, not fallback60x25. Independent allocator
builder tests reject a captured sibling fallback edit even when allocation is
numerically equal. Measured child-style views expose actual per-axis values.

The explicitly run GPU case uses NVIDIA GeForce GTX1650Ti / Vulkan / driver571.96.
Four local submit/readbacks for width60→100→0→60 match all pixels against separate
fixed-geometry full references. This is finite offscreen correctness; it does
not qualify modern desktop heavy effects, display-visible input tails, CPU
smoothness or sustained frame delivery.

The first vendor full suite failed its intentional layout-size guard:
Style<String> grew from560 to576 bytes. The guard now records this real16-byte
cost, with the corresponding Arc<str> style528→544. This is not a memory saving.
See [contract and limits](../../RETAINED_CONTAINMENT_FALLBACK.md).
