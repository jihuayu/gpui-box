# Flex automatic minimum validation — 2026-11-17

Windows x64, Rust1.98.1, original research worktree. Capability/correctness stage;
no new wall-time, memory-saving or continuous presentation measurement.

```powershell
node docs/performance/2026-11-17-flex-auto-min/capture.mjs D:/Scoop/persist/nodejs/bin/node_modules/agent-browser/node_modules/playwright-core/index.mjs
cargo +1.98.1 test -p gpui-box --features retained --locked --offline --quiet
cargo +1.98.1 test -p gpui-box --features retained-text,retained-wgpu --locked --offline --quiet
cargo +1.98.1 test -p gpui-box --features retained-text,retained-wgpu --locked --offline auto_min_tests -- --include-ignored --nocapture
cargo +1.98.1 test -p gpui-box-example-hello --features retained-wgpu --locked --offline --quiet
cargo +1.98.1 clippy -p gpui-box --features retained-text,retained-wgpu --lib --no-deps --locked --offline -- -D warnings
$env:CARGO_PROFILE_TEST_DEBUG = '0'
$env:CARGO_PROFILE_TEST_INCREMENTAL = 'false'
$env:CARGO_PROFILE_TEST_CODEGEN_UNITS = '256'
cargo +1.98.1 test --manifest-path vendor/taffy/Cargo.toml --lib --locked --offline -j 1 --quiet
cargo +1.98.1 test --manifest-path vendor/taffy/Cargo.toml --lib --locked --offline -j 1 --no-default-features --features std,taffy_tree,flexbox,grid,block_layout,float_layout,content_size --quiet
cargo +1.98.1 check --manifest-path vendor/taffy/Cargo.toml --locked --offline --no-default-features --quiet
cargo +1.98.1 check --manifest-path vendor/taffy/Cargo.toml --locked --offline --no-default-features --features alloc,taffy_tree,flexbox --quiet
cargo +1.98.1 check --manifest-path vendor/taffy/Cargo.toml --locked --offline --all-features --quiet
git diff --check
```

Both standalone vendor suites used the temporary low-memory test profile above.
This profile was established in a previous stage after LLVM OOM; production
compiler configuration is unchanged.

## Verified results

| Check | Actual result |
| --- | --- |
| retained full suite |403 passed |
| retained-text + retained-wgpu full suite |436 passed,23 ignored |
| explicit automatic-minimum suite including real GPU |4 passed |
| hello retained-wgpu with real Noto glyph case |40 passed,2 ignored |
| framework library Clippy `-D warnings` | passed |
| vendored Taffy default / project library suite |220 /214 passed |
| vendor no-default / alloc+tree+Flex / all-features checks | passed |

Full suites overlap. The final framework suites include the decoded-image case
and all816 browser comparisons; early runs of401 or the three-case explicit
specialization preceded those test additions and are not the final counts.

## Browser evidence

Chromium145.0.7632.6, exact user agent/schema in browser-reference.json and raw
816-case geometry in browser-reference.tsv. Four directions × replaced/ordinary
content × preferred cross Auto/20/80 × ten limit/overflow cases × three box models.
Padded boxes have5px padding plus2px border per side. Replaced content is a decoded
100×50 SVG with explicit aspect-ratio2; ordinary content is a fixed100×50 nested
Flex child. The core720 cases use main container opportunity30px; cross container extent240px. Another96 cases use MinContent/MaxContent main root sizes with explicit cross sizes. Explicit
justify Start and align-items Start avoid unrelated default-alignment differences.

Initial unmodified implementation reproduced56 mismatches in the original216-case
plain matrix. Fixes exposed/handled ratio provenance, replaced versus ordinary
minima, final cross sizing and content-box transfer arithmetic. The extended matrix
keeps all padded/explicit-minimum cases; it is not restricted to a passing subset.

Twelve non-replaced Row/RowReverse cases with explicit cross size and cross max30
are normative deviations: CSS Flexbox4.5 requires transferred content caps60px
(plain/BorderBox) or74px (ContentBox with14px insets), whereas Chromium retains
100/114px natural width. The Rust test first asserts the captured browser value,
then separately checks the normative result. They are excluded from browser-match
counts:804 browser matches plus12 separately asserted normative cases. Other
comparisons allow0.02px for browser subpixel distribution.

## Normal routes and correctness controls

Typed LayoutStyle sources update a normal ratio host from cross20→80→20,
preserving ordinary100px content width or increasing to160px. Resolved/local
complete CPU pixels match independently fixed-size ordinary full controls;
the controls explicitly disable shrink so their preferred size cannot shrink
back inside the30px container. Initial controls missed this requirement and
failed; the failure is not counted as successful evidence. Restored final-value
transactions keep revision and zero layout work; old packets outlive UI owners.

Decoded RGBA image resources with real padding/border and ContentBox retain
ratio-derived54×34 /174×94 /54×34 used boxes, full LayoutResult and CPU pixels
against explicit-size controls; old frames outlive image/UI owners. Actual
bundled Noto glyphs retain their measured min-content width when a20px cross
size transfers40px, then enlarge when height80 transfers160px. Complete leaf
geometry and all glyph pixels match explicit-size full controls.

The explicitly run hardware case uses NVIDIA GeForce GTX1650Ti / Vulkan /
driver571.96, three local submit/readbacks for20→80→20, with all pixels compared
against independent fixed geometry. This is offscreen correctness, not visible
presentation, frame timing or qualification of modern desktop GPU heavy effects.

## Limits

Finite geometry controls do not prove all ratios, percentage/auto cycles, natural
ratio versus authored auto-ratio combinations, text/rich-text/vertical writing,
Grid/Block or arbitrary custom parent semantics. Suites overlap; ignored hardware
tests count only when explicitly executed. Costs and remaining RFC scope are
recorded in [the contract/ADR](../../RETAINED_FLEX_AUTOMATIC_MINIMUM.md).
